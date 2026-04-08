use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use spiki_core::model::{BackendState, LocationRef};
use spiki_core::text::{path_from_file_uri, read_text_file};
use spiki_core::{
    DefinitionInput, DefinitionOutput, Runtime, SemanticBinding, SemanticBindingKind,
    SemanticEnsureInput, SemanticEnsureOutput, SemanticStatusOutput, ViewContext,
    WorkspaceStatusInput,
};
use tokio::io::{BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::protocol::{read_frame, write_frame};

const LSP_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct SemanticSupervisor {
    backends: Mutex<HashMap<String, Arc<Mutex<LspBackend>>>>,
}

struct LspBackend {
    child: Child,
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_request_id: u64,
    backend: BackendState,
    documents: HashMap<String, OpenDocumentState>,
}

struct OpenDocumentState {
    version: i32,
    content_hash: String,
}

impl SemanticSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            backends: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn status(
        &self,
        runtime: &Runtime,
        view: &ViewContext,
        language: Option<String>,
    ) -> Result<SemanticStatusOutput> {
        let output = runtime.workspace_status(
            view,
            WorkspaceStatusInput {
                include_backends: Some(true),
                include_coverage: Some(false),
            },
        )?;
        let detected_backends = output.backends.unwrap_or_default();
        if let Some(language) = language {
            let fallback = runtime
                .workspace_semantic_binding(view, &language)
                .map(|binding| BackendState {
                    language: language.clone(),
                    state: String::from("off"),
                    provider: Some(binding.provider_id),
                    idle_for_ms: Some(0),
                    last_error: None,
                })
                .or_else(|| {
                    detected_backends
                        .iter()
                        .find(|backend| backend.language == language)
                        .cloned()
                })
                .unwrap_or_else(|| BackendState {
                    language: language.clone(),
                    state: String::from("off"),
                    provider: Some(String::from("phase1-skeleton")),
                    idle_for_ms: Some(0),
                    last_error: None,
                });
            return Ok(SemanticStatusOutput {
                workspace_id: output.workspace_id,
                backends: vec![
                    self.status_for_language(runtime, view, &language)
                        .await?
                        .unwrap_or(fallback),
                ],
            });
        }

        let mut merged = Vec::with_capacity(detected_backends.len());
        for backend in detected_backends {
            let fallback: BackendState = backend.clone();
            merged.push(
                self.status_for_language(runtime, view, &backend.language)
                    .await?
                    .unwrap_or(fallback),
            );
        }
        Ok(SemanticStatusOutput {
            workspace_id: output.workspace_id,
            backends: merged,
        })
    }

    pub(crate) async fn ensure(
        &self,
        runtime: &Runtime,
        view: &ViewContext,
        input: SemanticEnsureInput,
    ) -> Result<SemanticEnsureOutput> {
        let action = input.action.unwrap_or_else(|| String::from("warm"));
        let binding = runtime
            .workspace_semantic_binding(view, &input.language)
            .ok_or_else(|| anyhow!("semantic binding is not configured for {}", input.language))?;
        if binding.kind == SemanticBindingKind::Builtin {
            return Ok(runtime.semantic_ensure(
                view,
                SemanticEnsureInput {
                    language: input.language,
                    action: Some(action),
                },
            )?);
        }
        let backend = if action == "stop" {
            self.stop_backend(view, &binding).await?;
            BackendState {
                language: input.language,
                state: String::from("off"),
                provider: Some(binding.provider_id),
                idle_for_ms: Some(0),
                last_error: None,
            }
        } else if action == "refresh" {
            self.stop_backend(view, &binding).await?;
            self.ensure_backend(view, &binding).await?
        } else {
            self.ensure_backend(view, &binding).await?
        };

        Ok(SemanticEnsureOutput {
            workspace_id: view.workspace_id.clone(),
            backend,
        })
    }

    pub(crate) async fn definition(
        &self,
        runtime: &Runtime,
        view: &ViewContext,
        input: DefinitionInput,
    ) -> Result<DefinitionOutput> {
        let binding = runtime
            .workspace_semantic_binding(view, &input.language)
            .ok_or_else(|| anyhow!("semantic binding is not configured for {}", input.language))?;
        if binding.kind != SemanticBindingKind::Lsp {
            return Err(anyhow!(
                "semantic definition requires an lsp binding for {}",
                input.language
            ));
        }

        let backend_handle = self.ensure_backend_handle(view, &binding).await?;
        let mut backend = backend_handle.lock().await;
        sync_document(&mut backend, &input.uri, Path::new(&path_from_file_uri(&input.uri)?)).await?;
        let result = request_response(
            &mut backend,
            "textDocument/definition",
            json!({
                "textDocument": {
                    "uri": input.uri
                },
                "position": {
                    "line": input.position.line,
                    "character": input.position.character
                }
            }),
        )
        .await?;
        let definitions = parse_definition_response(result)?;

        Ok(DefinitionOutput {
            workspace_id: view.workspace_id.clone(),
            workspace_revision: runtime.workspace_revision(view),
            engine: String::from("semantic"),
            backend: backend.backend.clone(),
            definitions,
            warnings: Vec::new(),
        })
    }

    async fn status_for_language(
        &self,
        runtime: &Runtime,
        view: &ViewContext,
        language: &str,
    ) -> Result<Option<BackendState>> {
        let Some(binding) = runtime.workspace_semantic_binding(view, language) else {
            return Ok(None);
        };
        if binding.kind == SemanticBindingKind::Builtin {
            return Ok(runtime
                .semantic_status(view, Some(language.to_string()))?
                .backends
                .into_iter()
                .next());
        }
        let key = backend_key(view, &binding);
        let handle = {
            let backends = self.backends.lock().await;
            backends.get(&key).cloned()
        };
        if let Some(handle) = handle {
            let backend = handle.lock().await;
            return Ok(Some(backend.backend.clone()));
        }
        Ok(Some(BackendState {
            language: language.to_string(),
            state: String::from("off"),
            provider: Some(binding.provider_id),
            idle_for_ms: Some(0),
            last_error: None,
        }))
    }

    async fn ensure_backend(
        &self,
        view: &ViewContext,
        binding: &SemanticBinding,
    ) -> Result<BackendState> {
        let handle = self.ensure_backend_handle(view, binding).await?;
        let backend = handle.lock().await;
        Ok(backend.backend.clone())
    }

    async fn ensure_backend_handle(
        &self,
        view: &ViewContext,
        binding: &SemanticBinding,
    ) -> Result<Arc<Mutex<LspBackend>>> {
        if binding.kind != SemanticBindingKind::Lsp {
            return Err(anyhow!(
                "semantic binding {} is not an lsp backend",
                binding.provider_id
            ));
        }

        let key = backend_key(view, binding);
        let mut backends = self.backends.lock().await;
        if let Some(handle) = backends.get(&key).cloned() {
            let mut backend = handle.lock().await;
            if backend.child.try_wait()?.is_none() {
                backend.backend.state = String::from("ready");
                drop(backend);
                return Ok(handle);
            }
            backends.remove(&key);
        }

        let backend = spawn_backend(view, binding).await?;
        let handle = Arc::new(Mutex::new(backend));
        backends.insert(key, handle.clone());
        Ok(handle)
    }

    async fn stop_backend(&self, view: &ViewContext, binding: &SemanticBinding) -> Result<()> {
        let key = backend_key(view, binding);
        let handle = self.backends.lock().await.remove(&key);
        if let Some(handle) = handle {
            let mut backend = handle.lock().await;
            backend.child.kill().await.ok();
            backend.child.wait().await.ok();
        }
        Ok(())
    }
}

fn backend_key(view: &ViewContext, binding: &SemanticBinding) -> String {
    format!("{}:{}", view.workspace_id, binding.provider_id)
}

async fn spawn_backend(view: &ViewContext, binding: &SemanticBinding) -> Result<LspBackend> {
    let command = binding
        .command
        .as_ref()
        .context("lsp binding missing command")?;
    let mut child = Command::new(command);
    child.args(&binding.args);
    child.stdin(std::process::Stdio::piped());
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::null());
    for (key, value) in &binding.env {
        child.env(key, value);
    }
    let mut child = child
        .spawn()
        .with_context(|| format!("failed to spawn semantic backend {}", binding.provider_id))?;
    let stdin = child
        .stdin
        .take()
        .context("semantic backend stdin unavailable")?;
    let stdout = child
        .stdout
        .take()
        .context("semantic backend stdout unavailable")?;
    let mut backend = LspBackend {
        child,
        writer: BufWriter::new(stdin),
        reader: BufReader::new(stdout),
        next_request_id: 1,
        backend: BackendState {
            language: binding.language.clone(),
            state: String::from("starting"),
            provider: Some(binding.provider_id.clone()),
            idle_for_ms: Some(0),
            last_error: None,
        },
        documents: HashMap::new(),
    };

    let root_uri = view.roots.first().cloned().context("workspace has no roots")?;
    let workspace_folders = view
        .roots
        .iter()
        .map(|uri| {
            json!({
                "uri": uri,
                "name": uri
            })
        })
        .collect::<Vec<_>>();
    let initialize_result = request_response(
        &mut backend,
        "initialize",
        json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "spiki-daemon",
                "version": env!("CARGO_PKG_VERSION")
            },
            "rootUri": root_uri,
            "workspaceFolders": workspace_folders,
            "capabilities": {},
            "initializationOptions": binding.initialization_options.clone()
        }),
    )
    .await?;
    if initialize_result.get("capabilities").is_none() {
        return Err(anyhow!(
            "semantic backend {} returned an invalid initialize response",
            binding.provider_id
        ));
    }
    notify_only(&mut backend, "initialized", json!({})).await?;
    if let Some(configuration) = &binding.workspace_configuration {
        notify_only(
            &mut backend,
            "workspace/didChangeConfiguration",
            json!({
                "settings": configuration
            }),
        )
        .await?;
    }
    backend.backend.state = String::from("ready");
    Ok(backend)
}

async fn sync_document(backend: &mut LspBackend, uri: &str, path: &Path) -> Result<()> {
    let file = read_text_file(path)?;
    let language_id = match path.extension().and_then(|value| value.to_str()).unwrap_or_default() {
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "rs" => "rust",
        "go" => "go",
        "py" => "python",
        "vue" => "vue",
        "svelte" => "svelte",
        other if !other.is_empty() => other,
        _ => "plaintext",
    };

    if let Some(document) = backend.documents.get_mut(uri) {
        if document.content_hash == file.content_hash {
            return Ok(());
        }
        let next_version = document.version + 1;
        document.version = next_version;
        document.content_hash = file.content_hash.clone();
        let changed_text = file.text;
        let _ = document;
        notify_only(
            backend,
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": next_version
                },
                "contentChanges": [
                    {
                        "text": changed_text
                    }
                ]
            }),
        )
        .await?;
        return Ok(());
    }

    backend.documents.insert(
        uri.to_string(),
        OpenDocumentState {
            version: 1,
            content_hash: file.content_hash,
        },
    );
    notify_only(
        backend,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": file.text
            }
        }),
    )
    .await
}

async fn notify_only(backend: &mut LspBackend, method: &str, params: Value) -> Result<()> {
    write_frame(
        &mut backend.writer,
        &json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }),
    )
    .await
}

async fn request_response(backend: &mut LspBackend, method: &str, params: Value) -> Result<Value> {
    let request_id = backend.next_request_id.to_string();
    backend.next_request_id += 1;
    write_frame(
        &mut backend.writer,
        &json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        }),
    )
    .await?;

    tokio::time::timeout(LSP_REQUEST_TIMEOUT, async {
        loop {
            let Some(message) = read_frame(&mut backend.reader).await? else {
                return Err(anyhow!("semantic backend closed its stdio stream"));
            };
            if message.get("method").is_some() {
                continue;
            }
            if message.get("id") != Some(&Value::String(request_id.clone())) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(anyhow!(
                    "semantic backend request {} failed: {}",
                    method,
                    error
                ));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    })
    .await
    .map_err(|_| {
        anyhow!(
            "semantic backend request {} timed out after {}ms",
            method,
            LSP_REQUEST_TIMEOUT.as_millis()
        )
    })?
}

fn parse_definition_response(result: Value) -> Result<Vec<LocationRef>> {
    if result.is_null() {
        return Ok(Vec::new());
    }
    if let Some(array) = result.as_array() {
        let mut locations = Vec::new();
        for item in array {
            if let Some(location) = parse_location(item) {
                locations.push(location);
            } else if let Some(location) = parse_location_link(item) {
                locations.push(location);
            }
        }
        return Ok(locations);
    }
    if let Some(location) = parse_location(&result) {
        return Ok(vec![location]);
    }
    if let Some(location) = parse_location_link(&result) {
        return Ok(vec![location]);
    }
    Err(anyhow!("semantic backend returned an unsupported definition payload"))
}

fn parse_location(value: &Value) -> Option<LocationRef> {
    Some(LocationRef {
        uri: value.get("uri")?.as_str()?.to_string(),
        range: serde_json::from_value(value.get("range")?.clone()).ok()?,
    })
}

fn parse_location_link(value: &Value) -> Option<LocationRef> {
    Some(LocationRef {
        uri: value.get("targetUri")?.as_str()?.to_string(),
        range: serde_json::from_value(value.get("targetRange")?.clone()).ok()?,
    })
}
