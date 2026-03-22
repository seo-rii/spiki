use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use spiki_core::{Runtime, RuntimeConfig};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::protocol::{read_frame, write_frame};
use crate::session::{handle_message, RootsState, Session};

pub(crate) struct Args {
    pub(crate) socket_path: PathBuf,
    pub(crate) runtime_dir: PathBuf,
}

pub(crate) fn parse_args() -> Result<Args> {
    let mut socket_path = None;
    let mut runtime_dir = None;
    let mut iter = std::env::args().skip(1);

    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--socket" => socket_path = iter.next().map(PathBuf::from),
            "--runtime-dir" => runtime_dir = iter.next().map(PathBuf::from),
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }

    Ok(Args {
        socket_path: socket_path.context("--socket is required")?,
        runtime_dir: runtime_dir.context("--runtime-dir is required")?,
    })
}

pub(crate) async fn run(socket_path: PathBuf, runtime_dir: PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(&runtime_dir).await?;
    if Path::new(&socket_path).exists() {
        let _ = tokio::fs::remove_file(&socket_path).await;
    }
    tokio::fs::write(
        runtime_dir.join("daemon.pid"),
        std::process::id().to_string(),
    )
    .await?;
    let listener = UnixListener::bind(&socket_path)?;
    let runtime = Runtime::new(RuntimeConfig::default());
    let (shutdown_tx, _) = broadcast::channel(2);

    let signal_task = tokio::spawn(wait_for_shutdown(shutdown_tx.clone()));
    info!("spiki-daemon listening on {}", socket_path.display());

    loop {
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::select! {
            accept = listener.accept() => {
                let (stream, _) = accept?;
                let session_runtime = runtime.clone();
                let session_shutdown = shutdown_tx.subscribe();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, session_runtime, session_shutdown).await {
                        warn!("session ended with error: {error:#}");
                    }
                });
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    signal_task.abort();
    let _ = tokio::fs::remove_file(&socket_path).await;
    let _ = tokio::fs::remove_file(runtime_dir.join("daemon.pid")).await;
    Ok(())
}

async fn wait_for_shutdown(shutdown_tx: broadcast::Sender<()>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("signal setup");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    let _ = shutdown_tx.send(());
}

async fn handle_connection(
    stream: UnixStream,
    runtime: Runtime,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Value>();
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<Value>();
    let writer_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(message) = writer_rx.recv().await {
            if let Err(error) = write_frame(&mut writer, &message).await {
                return Err(error);
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let session = Arc::new(Session {
        client_session_id: format!("session_{}", Uuid::now_v7().simple()),
        runtime,
        writer: writer_tx,
        pending: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        request_lock: tokio::sync::Mutex::new(()),
        roots: tokio::sync::Mutex::new(RootsState::default()),
        next_request_id: std::sync::atomic::AtomicU64::new(1),
        protocol_version: tokio::sync::Mutex::new(String::from("2025-11-25")),
    });
    let request_session = session.clone();
    let request_task = tokio::spawn(async move {
        while let Some(message) = request_rx.recv().await {
            if let Err(error) = handle_message(request_session.clone(), message).await {
                error!("failed to handle request: {error:#}");
            }
        }
    });
    let mut reader = BufReader::new(reader);

    loop {
        tokio::select! {
            frame = read_frame(&mut reader) => {
                let Some(message) = frame? else {
                    break;
                };
                if message.get("method").is_some() {
                    if request_tx.send(message).is_err() {
                        break;
                    }
                    continue;
                }
                if let Err(error) = handle_message(session.clone(), message).await {
                    error!("failed to handle response: {error:#}");
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    drop(request_tx);
    request_task.abort();
    writer_task.abort();
    Ok(())
}
