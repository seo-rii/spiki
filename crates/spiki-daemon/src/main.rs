#[cfg(not(unix))]
compile_error!("spiki-daemon currently supports unix runtime sockets only");

mod app;
mod protocol;
mod session;
mod tools;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = app::parse_args()?;
    app::run(args.socket_path, args.runtime_dir).await
}
