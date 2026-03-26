use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use spiki_core::Runtime;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::protocol::id_to_string;
use crate::tools::{handle_tool_call, tool_specs};

const SPIKI_SERVER_NAME: &str = "spiki";
const SPIKI_SERVER_VERSION: &str = "0.1.0-dev";
const SPIKI_PROTOCOL_VERSION: &str = "2025-11-25";
const SPIKI_BOOTSTRAP_VERSION: u32 = 1;

pub(crate) struct Session {
    pub(crate) client_session_id: String,
    pub(crate) runtime: Runtime,
    pub(crate) writer: mpsc::UnboundedSender<Value>,
    pub(crate) pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    pub(crate) request_lock: Mutex<()>,
    pub(crate) roots: Mutex<RootsState>,
    pub(crate) next_request_id: AtomicU64,
    pub(crate) protocol_version: Mutex<String>,
}

#[derive(Default)]
pub(crate) struct RootsState {
    pub(crate) client_supports_roots: bool,
    pub(crate) cached: Option<Vec<String>>,
    pub(crate) dirty: bool,
}

pub(crate) async fn handle_message(session: Arc<Session>, message: Value) -> Result<()> {
    let Some(id) = message.get("id").cloned() else {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let _request_guard = session.request_lock.lock().await;
            handle_notification(&session, method).await?;
        }
        return Ok(());
    };

    if message.get("method").is_none() {
        let id_text = id_to_string(&id)?;
        if let Some(sender) = session.pending.lock().await.remove(&id_text) {
            let _ = sender.send(message);
        }
        return Ok(());
    }

    let _request_guard = session.request_lock.lock().await;
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .context("request missing method")?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    match handle_request(&session, method, params).await {
        Ok(result) => send_response(&session, id, result)?,
        Err(error) => send_protocol_error(&session, id, error)?,
    }
    Ok(())
}

async fn handle_notification(session: &Arc<Session>, method: &str) -> Result<()> {
    if method == "notifications/initialized" {
        return Ok(());
    }
    if method == "notifications/roots/list_changed" || method == "roots/list_changed" {
        session.roots.lock().await.dirty = true;
        return Ok(());
    }
    Ok(())
}

async fn handle_request(session: &Arc<Session>, method: &str, params: Value) -> Result<Value> {
    match method {
        "initialize" => handle_initialize(session, params).await,
        "ping" => Ok(json!({})),
        "spiki/bootstrap_status" => Ok(json!({
            "serverInfo": {
                "name": SPIKI_SERVER_NAME,
                "version": SPIKI_SERVER_VERSION
            },
            "protocolVersion": SPIKI_PROTOCOL_VERSION,
            "bootstrapVersion": SPIKI_BOOTSTRAP_VERSION
        })),
        "shutdown" => Ok(Value::Null),
        "tools/list" => Ok(json!({ "tools": tool_specs() })),
        "tools/call" => handle_tool_call(session, params).await,
        other => Err(anyhow!("method not found: {other}")),
    }
}

async fn handle_initialize(session: &Arc<Session>, params: Value) -> Result<Value> {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-11-25")
        .to_string();
    let client_supports_roots = params
        .get("capabilities")
        .and_then(|value| value.get("roots"))
        .is_some();
    let init_roots = parse_root_uris(params.get("roots"));

    {
        let mut version = session.protocol_version.lock().await;
        *version = protocol_version.clone();
    }
    {
        let mut roots = session.roots.lock().await;
        roots.client_supports_roots = client_supports_roots;
        roots.cached = init_roots;
        roots.dirty = false;
    }

    Ok(json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SPIKI_SERVER_NAME,
            "version": SPIKI_SERVER_VERSION
        },
        "instructions": "spiki Phase 1 exposes text-first workspace tools and safe apply skeletons."
    }))
}

impl Session {
    pub(crate) async fn ensure_view(&self) -> Result<spiki_core::ViewContext> {
        let roots = self.ensure_roots().await?;
        self.runtime
            .upsert_view(self.client_session_id.clone(), &roots)
            .map_err(anyhow::Error::from)
    }

    async fn ensure_roots(&self) -> Result<Vec<String>> {
        {
            let state = self.roots.lock().await;
            if !state.dirty {
                if let Some(cached) = &state.cached {
                    return Ok(cached.clone());
                }
            }
        }

        let should_request = {
            let state = self.roots.lock().await;
            state.client_supports_roots
        };
        if !should_request {
            return Err(anyhow!("client did not provide roots support"));
        }

        let fresh = self.request_roots_list().await?;
        {
            let mut state = self.roots.lock().await;
            state.cached = Some(fresh.clone());
            state.dirty = false;
        }
        Ok(fresh)
    }

    async fn request_roots_list(&self) -> Result<Vec<String>> {
        let id = self
            .next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);
        self.writer
            .send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "roots/list",
                "params": {}
            }))
            .map_err(|_| anyhow!("failed to send roots/list request"))?;
        let response = rx
            .await
            .map_err(|_| anyhow!("roots/list response channel closed"))?;
        if let Some(error) = response.get("error") {
            return Err(anyhow!("roots/list failed: {error}"));
        }
        let roots = parse_root_uris(response.get("result"))
            .ok_or_else(|| anyhow!("roots/list returned no usable roots"))?;
        if roots.is_empty() {
            return Err(anyhow!("roots/list returned an empty root set"));
        }
        Ok(roots)
    }
}

fn parse_root_uris(value: Option<&Value>) -> Option<Vec<String>> {
    let value = value?;
    let roots_value = value.get("roots").unwrap_or(value);
    let roots = roots_value.as_array()?;
    let mut uris = Vec::new();

    for root in roots {
        if let Some(uri) = root.as_str() {
            uris.push(uri.to_string());
            continue;
        }
        if let Some(uri) = root.get("uri").and_then(Value::as_str) {
            uris.push(uri.to_string());
        }
    }

    Some(uris)
}

fn send_response(session: &Session, id: Value, result: Value) -> Result<()> {
    session
        .writer
        .send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
        .map_err(|_| anyhow!("failed to queue response"))
}

fn send_protocol_error(session: &Session, id: Value, error: anyhow::Error) -> Result<()> {
    let message = error.to_string();
    let code = if message.starts_with("method not found:") {
        -32601
    } else if message == "request missing method" {
        -32600
    } else {
        -32603
    };
    session
        .writer
        .send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        }))
        .map_err(|_| anyhow!("failed to queue error response"))
}
