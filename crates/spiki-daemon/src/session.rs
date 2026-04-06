use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use spiki_core::Runtime;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};
use uuid::Uuid;

use crate::protocol::id_to_string;
use crate::semantic::SemanticSupervisor;
use crate::tools::{handle_tool_call, tool_specs, tool_supports_task_execution};

const SPIKI_SERVER_NAME: &str = "spiki";
const SPIKI_SERVER_VERSION: &str = "0.1.0-dev";
const SPIKI_PROTOCOL_VERSION: &str = "2025-11-25";
const SPIKI_BOOTSTRAP_VERSION: u32 = 1;
const DEFAULT_TASK_TTL_MS: u64 = 60_000;
const DEFAULT_TASK_POLL_INTERVAL_MS: u64 = 500;
const RELATED_TASK_META_KEY: &str = "io.modelcontextprotocol/related-task";
const MODEL_IMMEDIATE_RESPONSE_META_KEY: &str = "io.modelcontextprotocol/model-immediate-response";

pub(crate) struct Session {
    pub(crate) client_session_id: String,
    pub(crate) runtime: Runtime,
    pub(crate) writer: mpsc::UnboundedSender<Value>,
    pub(crate) pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    pub(crate) incoming_requests: Mutex<HashSet<String>>,
    pub(crate) cancelled_requests: Mutex<HashSet<String>>,
    pub(crate) request_lock: Mutex<()>,
    pub(crate) roots: Mutex<RootsState>,
    pub(crate) tasks: Mutex<HashMap<String, TaskRecord>>,
    pub(crate) next_request_id: AtomicU64,
    pub(crate) protocol_version: Mutex<String>,
    pub(crate) semantic_supervisor: Arc<SemanticSupervisor>,
}

#[derive(Default)]
pub(crate) struct RootsState {
    pub(crate) client_supports_roots: bool,
    pub(crate) cached: Option<Vec<String>>,
    pub(crate) dirty: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskSnapshot {
    pub(crate) task_id: String,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status_message: Option<String>,
    pub(crate) created_at: String,
    pub(crate) last_updated_at: String,
    pub(crate) ttl: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) poll_interval: Option<u64>,
}

pub(crate) struct TaskRecord {
    pub(crate) snapshot: TaskSnapshot,
    pub(crate) result: Option<Value>,
    pub(crate) waiter: Arc<Notify>,
    pub(crate) expires_at: Option<Instant>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskRequest {
    ttl: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdParams {
    task_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TasksListParams {
    cursor: Option<String>,
}

pub(crate) async fn handle_message(session: Arc<Session>, message: Value) -> Result<()> {
    let Some(id) = message.get("id").cloned() else {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            handle_notification(&session, method, message.get("params")).await?;
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

    let id_text = id_to_string(&id)?;
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .context("request missing method")?;
    let _request_guard = if method.starts_with("tasks/") {
        None
    } else {
        Some(session.request_lock.lock().await)
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    if session.is_request_cancelled(&id_text).await {
        session.finish_incoming_request(&id_text).await;
        return Ok(());
    }

    let request_result = handle_request(&session, &id_text, method, params).await;
    let was_cancelled = session.is_request_cancelled(&id_text).await;
    let send_result = if was_cancelled {
        Ok(())
    } else {
        match request_result {
            Ok(result) => send_response(&session, id, result),
            Err(error) => send_protocol_error(&session, id, error),
        }
    };
    session.finish_incoming_request(&id_text).await;
    send_result?;
    Ok(())
}

async fn handle_notification(
    session: &Arc<Session>,
    method: &str,
    params: Option<&Value>,
) -> Result<()> {
    if method == "notifications/initialized" {
        return Ok(());
    }
    if method == "notifications/roots/list_changed" || method == "roots/list_changed" {
        session.roots.lock().await.dirty = true;
        return Ok(());
    }
    if method == "notifications/cancelled" {
        session.mark_request_cancelled(params).await;
        return Ok(());
    }
    Ok(())
}

async fn handle_request(
    session: &Arc<Session>,
    request_id: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    match method {
        "initialize" => handle_initialize(session, params).await,
        "ping" => Ok(json!({})),
        "spiki/bootstrap_status" => Ok(json!({
            "serverInfo": {
                "name": SPIKI_SERVER_NAME,
                "version": SPIKI_SERVER_VERSION,
                "title": "spiki",
                "description": "Editor-oriented MCP workspace runtime",
                "websiteUrl": "https://github.com/seo-rii/spiki"
            },
            "protocolVersion": SPIKI_PROTOCOL_VERSION,
            "bootstrapVersion": SPIKI_BOOTSTRAP_VERSION
        })),
        "shutdown" => Ok(Value::Null),
        "tools/list" => Ok(json!({ "tools": tool_specs() })),
        "tools/call" => handle_tools_call_request(session, request_id, params).await,
        "tasks/list" => handle_tasks_list(session, params).await,
        "tasks/get" => handle_task_get(session, params).await,
        "tasks/result" => handle_task_result(session, request_id, params).await,
        "tasks/cancel" => handle_task_cancel(session, params).await,
        other => Err(anyhow!("method not found: {other}")),
    }
}

async fn handle_initialize(session: &Arc<Session>, params: Value) -> Result<Value> {
    let requested_protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(SPIKI_PROTOCOL_VERSION)
        .to_string();
    let client_supports_roots = params
        .get("capabilities")
        .and_then(|value| value.get("roots"))
        .is_some();
    let roots_present = params.get("roots").is_some();
    let init_roots = parse_root_uris(params.get("roots"));
    if roots_present && init_roots.is_none() {
        return Err(anyhow!(
            "invalid params: initialize.params.roots must be an array of roots"
        ));
    }
    if init_roots.as_ref().is_some_and(Vec::is_empty) {
        return Err(anyhow!(
            "invalid params: initialize.params.roots must not be empty"
        ));
    }
    let negotiated_protocol_version = if requested_protocol_version == SPIKI_PROTOCOL_VERSION {
        requested_protocol_version
    } else {
        SPIKI_PROTOCOL_VERSION.to_string()
    };

    {
        let mut version = session.protocol_version.lock().await;
        *version = negotiated_protocol_version.clone();
    }
    {
        let mut roots = session.roots.lock().await;
        roots.client_supports_roots = client_supports_roots;
        roots.cached = init_roots;
        roots.dirty = false;
    }

    Ok(json!({
        "protocolVersion": negotiated_protocol_version,
        "capabilities": {
            "tools": { "listChanged": false },
            "tasks": {
                "list": {},
                "cancel": {},
                "requests": {
                    "tools": {
                        "call": {}
                    }
                }
            },
            "experimental": {
                "spikiPluginScaffold": {
                    "clients": ["codex", "claude"],
                    "version": 1
                }
            }
        },
        "serverInfo": {
            "name": SPIKI_SERVER_NAME,
            "version": SPIKI_SERVER_VERSION,
            "title": "spiki",
            "description": "Editor-oriented MCP workspace runtime",
            "websiteUrl": "https://github.com/seo-rii/spiki"
        },
        "instructions": "spiki Phase 1 exposes text-first workspace tools, output schemas, and plugin-friendly metadata for Codex and Claude integrations."
    }))
}

async fn handle_tools_call_request(
    session: &Arc<Session>,
    request_id: &str,
    mut params: Value,
) -> Result<Value> {
    let task_request = params
        .get("task")
        .cloned()
        .map(serde_json::from_value::<TaskRequest>)
        .transpose()
        .map_err(|error| anyhow!("invalid params: {error}"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call missing name")?;

    if let Some(task_request) = task_request {
        if !tool_supports_task_execution(name) {
            return Err(anyhow!(
                "method not found: task-augmented tools/call is not supported for {name}"
            ));
        }

        let task = session
            .create_task_snapshot(task_request.ttl, Some(format!("Running {name}")))
            .await;
        let task_id = task.task_id.clone();
        let background_task_id = task_id.clone();
        params
            .as_object_mut()
            .context("invalid params: tools/call params must be an object")?
            .remove("task");
        let background_session = session.clone();
        tokio::spawn(async move {
            let result = match handle_tool_call(
                &background_session,
                &background_task_id,
                params,
                Some(&background_task_id),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    if background_session
                        .is_operation_cancelled(&background_task_id)
                        .await
                    {
                        task_error_result("AE_CANCELLED", "The task was cancelled by request.")
                    } else {
                        task_error_result("AE_INTERNAL", error.to_string())
                    }
                }
            };
            background_session
                .finish_task(&background_task_id, result)
                .await;
        });

        return Ok(json!({
            "task": task,
            "_meta": {
                RELATED_TASK_META_KEY: {
                    "taskId": task_id
                },
                MODEL_IMMEDIATE_RESPONSE_META_KEY: format!(
                    "Task {task_id} accepted. Poll tasks/get or wait on tasks/result."
                )
            }
        }));
    }

    handle_tool_call(session, request_id, params, None).await
}

async fn handle_tasks_list(session: &Arc<Session>, params: Value) -> Result<Value> {
    let input = serde_json::from_value::<TasksListParams>(params)
        .map_err(|error| anyhow!("invalid params: {error}"))?;
    Ok(json!({
        "tasks": session.list_tasks(input.cursor).await
    }))
}

async fn handle_task_get(session: &Arc<Session>, params: Value) -> Result<Value> {
    let input = serde_json::from_value::<TaskIdParams>(params)
        .map_err(|error| anyhow!("invalid params: {error}"))?;
    session.get_task(&input.task_id).await
}

async fn handle_task_result(
    session: &Arc<Session>,
    request_id: &str,
    params: Value,
) -> Result<Value> {
    let input = serde_json::from_value::<TaskIdParams>(params)
        .map_err(|error| anyhow!("invalid params: {error}"))?;
    session.await_task_result(request_id, &input.task_id).await
}

async fn handle_task_cancel(session: &Arc<Session>, params: Value) -> Result<Value> {
    let input = serde_json::from_value::<TaskIdParams>(params)
        .map_err(|error| anyhow!("invalid params: {error}"))?;
    session.cancel_task(&input.task_id).await
}

impl Session {
    pub(crate) async fn note_incoming_request(&self, request_id: String) {
        self.incoming_requests.lock().await.insert(request_id);
    }

    pub(crate) async fn is_request_cancelled(&self, request_id: &str) -> bool {
        self.cancelled_requests.lock().await.contains(request_id)
    }

    pub(crate) async fn is_operation_cancelled(&self, request_id: &str) -> bool {
        if self.is_request_cancelled(request_id).await {
            return true;
        }

        self.tasks
            .lock()
            .await
            .get(request_id)
            .is_some_and(|task| task.snapshot.status == "cancelled")
    }

    pub(crate) async fn send_progress(
        &self,
        request_id: &str,
        progress_token: &Value,
        progress: u64,
        total: u64,
        message: &str,
    ) -> Result<()> {
        let mut params = json!({
            "progressToken": progress_token,
            "progress": progress,
            "total": total,
            "message": message
        });
        if !progress_token.is_string() && !progress_token.is_u64() && !progress_token.is_i64() {
            return Ok(());
        }
        if self.tasks.lock().await.contains_key(request_id) {
            params["_meta"] = json!({
                RELATED_TASK_META_KEY: {
                    "taskId": request_id
                }
            });
        }
        self.writer
            .send(json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": params.take()
            }))
            .map_err(|_| anyhow!("failed to queue progress notification"))
    }

    pub(crate) async fn create_task_snapshot(
        &self,
        requested_ttl: Option<u64>,
        status_message: Option<String>,
    ) -> TaskSnapshot {
        let mut tasks = self.tasks.lock().await;
        sweep_expired_tasks(&mut tasks);
        let now = Instant::now();
        let ttl = Some(requested_ttl.unwrap_or(DEFAULT_TASK_TTL_MS));
        let timestamp = timestamp_now();
        let task = TaskSnapshot {
            task_id: Uuid::now_v7().to_string(),
            status: String::from("working"),
            status_message,
            created_at: timestamp.clone(),
            last_updated_at: timestamp,
            ttl,
            poll_interval: Some(DEFAULT_TASK_POLL_INTERVAL_MS),
        };
        tasks.insert(
            task.task_id.clone(),
            TaskRecord {
                snapshot: task.clone(),
                result: None,
                waiter: Arc::new(Notify::new()),
                expires_at: ttl.map(|value| now + Duration::from_millis(value)),
            },
        );
        drop(tasks);
        self.publish_task_status(&task).await;
        task
    }

    pub(crate) async fn list_tasks(&self, cursor: Option<String>) -> Vec<TaskSnapshot> {
        let mut tasks = self.tasks.lock().await;
        sweep_expired_tasks(&mut tasks);
        let mut snapshots = tasks
            .values()
            .map(|record| record.snapshot.clone())
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        if let Some(cursor) = cursor {
            if let Some(index) = snapshots.iter().position(|task| task.task_id == cursor) {
                return snapshots.into_iter().skip(index + 1).collect();
            }
        }
        snapshots
    }

    pub(crate) async fn get_task(&self, task_id: &str) -> Result<Value> {
        let mut tasks = self.tasks.lock().await;
        sweep_expired_tasks(&mut tasks);
        let task = tasks
            .get(task_id)
            .map(|record| record.snapshot.clone())
            .ok_or_else(|| anyhow!("invalid params: failed to retrieve task: task not found"))?;
        Ok(serde_json::to_value(task)?)
    }

    pub(crate) async fn await_task_result(&self, request_id: &str, task_id: &str) -> Result<Value> {
        loop {
            let waiter = {
                let mut tasks = self.tasks.lock().await;
                sweep_expired_tasks(&mut tasks);
                let task = tasks.get(task_id).ok_or_else(|| {
                    anyhow!("invalid params: failed to retrieve task: task not found")
                })?;
                if is_terminal_task_status(&task.snapshot.status) {
                    return Ok(relate_task_result(
                        task_id,
                        task.result.clone().unwrap_or_else(|| {
                            task_error_result("AE_INTERNAL", "Task completed without a result.")
                        }),
                    ));
                }
                task.waiter.clone()
            };

            tokio::select! {
                _ = waiter.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }

            if self.is_request_cancelled(request_id).await {
                return Err(anyhow!("request cancelled"));
            }
        }
    }

    pub(crate) async fn cancel_task(&self, task_id: &str) -> Result<Value> {
        let mut tasks = self.tasks.lock().await;
        sweep_expired_tasks(&mut tasks);
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow!("invalid params: failed to retrieve task: task not found"))?;
        if !is_terminal_task_status(&task.snapshot.status) {
            task.snapshot.status = String::from("cancelled");
            task.snapshot.status_message = Some(String::from("The task was cancelled by request."));
            task.snapshot.last_updated_at = timestamp_now();
            task.result = Some(task_error_result(
                "AE_CANCELLED",
                "The task was cancelled by request.",
            ));
            task.waiter.notify_waiters();
        }
        let snapshot = task.snapshot.clone();
        drop(tasks);
        self.publish_task_status(&snapshot).await;
        Ok(serde_json::to_value(snapshot)?)
    }

    pub(crate) async fn finish_task(&self, task_id: &str, result: Value) {
        let mut tasks = self.tasks.lock().await;
        sweep_expired_tasks(&mut tasks);
        let Some(task) = tasks.get_mut(task_id) else {
            return;
        };
        if is_terminal_task_status(&task.snapshot.status) {
            return;
        }
        task.snapshot.status = if result.get("isError").and_then(Value::as_bool) == Some(true) {
            String::from("failed")
        } else {
            String::from("completed")
        };
        task.snapshot.status_message = Some(match task.snapshot.status.as_str() {
            "completed" => String::from("The operation completed successfully."),
            _ => String::from("The operation completed with an error."),
        });
        task.snapshot.last_updated_at = timestamp_now();
        task.result = Some(result);
        task.waiter.notify_waiters();
        let snapshot = task.snapshot.clone();
        drop(tasks);
        self.publish_task_status(&snapshot).await;
    }

    async fn publish_task_status(&self, task: &TaskSnapshot) {
        let _ = self.writer.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/tasks/status",
            "params": task
        }));
    }

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

    async fn mark_request_cancelled(&self, params: Option<&Value>) {
        let Some(request_id) = params
            .and_then(|value| value.get("requestId"))
            .and_then(parse_request_id)
        else {
            return;
        };

        if !self.incoming_requests.lock().await.contains(&request_id) {
            return;
        }
        self.cancelled_requests.lock().await.insert(request_id);
    }

    async fn finish_incoming_request(&self, request_id: &str) {
        self.incoming_requests.lock().await.remove(request_id);
        self.cancelled_requests.lock().await.remove(request_id);
    }
}

fn sweep_expired_tasks(tasks: &mut HashMap<String, TaskRecord>) {
    let now = Instant::now();
    tasks.retain(|_, task| match task.expires_at {
        Some(expires_at) => expires_at > now,
        None => true,
    });
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn is_terminal_task_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn task_error_result(code: &str, message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [
            {
                "type": "text",
                "text": format!("{code}: {message}")
            }
        ],
        "structuredContent": {
            "code": code,
            "message": message,
            "retryable": false,
            "details": null
        },
        "isError": true
    })
}

fn relate_task_result(task_id: &str, mut result: Value) -> Value {
    let meta = result.as_object_mut().map(|object| {
        object
            .entry(String::from("_meta"))
            .or_insert_with(|| json!({}))
    });
    if let Some(meta) = meta.and_then(Value::as_object_mut) {
        meta.insert(
            String::from(RELATED_TASK_META_KEY),
            json!({
                "taskId": task_id
            }),
        );
    }
    result
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

fn parse_request_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(String::from)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
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
    } else if message.starts_with("invalid params:") {
        -32602
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
