use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::model::{
    Coverage, ExecutionError, ReadSpansInput, ReadSpansOutput, Scope, SearchTextInput,
    SearchTextOutput, SemanticEnsureInput, SemanticEnsureOutput, SemanticStatusOutput, Warning,
    WorkspaceStatusInput, WorkspaceStatusOutput,
};
use crate::text::{
    build_text_span, canonical_roots_from_uris, ensure_path_in_roots, file_uri_from_path,
    path_from_file_uri, read_text_file, scan_workspace, search_file, KnownFile, ScanOptions,
};

use super::error::SpikiResult;
use super::languages::{backend_for_language, detected_backends};
use super::state::{
    workspace_id_for_roots, Runtime, RuntimeConfig, RuntimeState, ViewContext, WorkspaceMeta,
    WorkspaceState,
};

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            state: Arc::new(RuntimeState {
                config,
                workspaces: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn upsert_view(
        &self,
        client_session_id: impl Into<String>,
        roots: &[String],
    ) -> SpikiResult<ViewContext> {
        let client_session_id = client_session_id.into();
        let canonical_roots = canonical_roots_from_uris(roots)?;
        let workspace_id = workspace_id_for_roots(&canonical_roots);
        let workspace = {
            let mut workspaces = self.state.workspaces.lock();
            workspaces
                .entry(workspace_id.clone())
                .or_insert_with(|| {
                    Arc::new(WorkspaceState {
                        _workspace_id: workspace_id.clone(),
                        _roots: canonical_roots.clone(),
                        meta: Mutex::new(WorkspaceMeta {
                            revision: 0,
                            known_files: HashMap::new(),
                            semantic_backends: HashMap::new(),
                            plans: HashMap::new(),
                        }),
                        write_lock: Mutex::new(()),
                    })
                })
                .clone()
        };

        Ok(ViewContext {
            client_session_id: client_session_id.clone(),
            view_id: format!(
                "view_{}",
                blake3::hash(format!("{client_session_id}:{workspace_id}").as_bytes()).to_hex()
                    [..12]
                    .to_string()
            ),
            workspace_id,
            roots: canonical_roots
                .iter()
                .map(|root| root.uri.clone())
                .collect(),
            roots_canonical: canonical_roots,
            workspace,
        })
    }

    pub fn workspace_status(
        &self,
        view: &ViewContext,
        input: WorkspaceStatusInput,
    ) -> SpikiResult<WorkspaceStatusOutput> {
        let coverage = self.refresh_workspace(view, None)?;
        let include_backends = input.include_backends.unwrap_or(true);
        let include_coverage = input.include_coverage.unwrap_or(true);

        Ok(WorkspaceStatusOutput {
            client_session_id: view.client_session_id.clone(),
            view_id: view.view_id.clone(),
            workspace_id: view.workspace_id.clone(),
            workspace_revision: self.current_revision(view),
            roots: view.roots.clone(),
            coverage: include_coverage.then_some(coverage),
            backends: include_backends.then_some(detected_backends(view)),
            warnings: Vec::new(),
        })
    }

    pub fn read_spans(
        &self,
        view: &ViewContext,
        input: ReadSpansInput,
    ) -> SpikiResult<ReadSpansOutput> {
        self.refresh_workspace(view, None)?;
        let mut spans = Vec::new();

        for request in input.spans {
            let path = path_from_file_uri(&request.uri)?;
            let canonical = ensure_path_in_roots(&path, &view.roots_canonical)?;
            let file = read_text_file(&canonical)?;
            spans.push(build_text_span(
                &request.uri,
                &file,
                request.range,
                request.context_lines.unwrap_or(2),
                &canonical,
            )?);
        }

        Ok(ReadSpansOutput {
            workspace_revision: self.current_revision(view),
            spans,
            warnings: Vec::new(),
        })
    }

    pub fn search_text(
        &self,
        view: &ViewContext,
        input: SearchTextInput,
    ) -> SpikiResult<SearchTextOutput> {
        let coverage = self.refresh_workspace(view, None)?;
        let mut warnings = Vec::new();
        let mut matches = Vec::new();
        let scope = input.scope.clone();
        let scan = scan_workspace(
            &view.roots_canonical,
            scope.as_ref(),
            ScanOptions {
                include_ignored: scope
                    .as_ref()
                    .and_then(|value| value.include_ignored)
                    .unwrap_or(false),
                include_generated: scope
                    .as_ref()
                    .and_then(|value| value.include_generated)
                    .unwrap_or(false),
                max_index_file_size_bytes: self.state.config.max_index_file_size_bytes,
            },
        )?;
        warnings.extend(scan.warnings);
        let limit = input.limit.unwrap_or(200);
        let context_lines = input.context_lines.unwrap_or(1);
        let mode = input.mode.unwrap_or_default();
        let case_sensitive = input.case_sensitive.unwrap_or(false);
        let max_files = scope
            .as_ref()
            .and_then(|value| value.max_files)
            .unwrap_or(usize::MAX);
        let mut truncated = false;

        for path in scan.files.into_iter().take(max_files) {
            let file = match read_text_file(&path) {
                Ok(value) => value,
                Err(error) if error.code == "AE_UNSUPPORTED" => {
                    warnings.push(Warning {
                        code: String::from("READ_SKIPPED"),
                        message: error.message,
                        severity: Some(String::from("info")),
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            let uri = file_uri_from_path(&path);
            let remaining = limit.saturating_sub(matches.len());
            if remaining == 0 {
                truncated = true;
                break;
            }
            let mut file_matches = search_file(
                &path,
                &uri,
                &file,
                &input.query,
                mode.clone(),
                case_sensitive,
                context_lines,
                remaining,
            )?;
            if matches.len() + file_matches.len() >= limit {
                truncated = true;
            }
            matches.append(&mut file_matches);
            if matches.len() >= limit {
                matches.truncate(limit);
                break;
            }
        }

        Ok(SearchTextOutput {
            workspace_revision: self.current_revision(view),
            engine: String::from("text"),
            matches,
            truncated,
            coverage,
            warnings,
        })
    }

    pub fn semantic_status(
        &self,
        view: &ViewContext,
        language: Option<String>,
    ) -> SpikiResult<SemanticStatusOutput> {
        self.refresh_workspace(view, None)?;
        Ok(SemanticStatusOutput {
            workspace_id: view.workspace_id.clone(),
            backends: match language {
                Some(language) => {
                    let meta = view.workspace.meta.lock();
                    vec![meta
                        .semantic_backends
                        .get(&language)
                        .cloned()
                        .unwrap_or_else(|| backend_for_language(language))]
                }
                None => detected_backends(view),
            },
        })
    }

    pub fn semantic_ensure(
        &self,
        view: &ViewContext,
        input: SemanticEnsureInput,
    ) -> SpikiResult<SemanticEnsureOutput> {
        self.refresh_workspace(view, None)?;
        let action = input.action.unwrap_or_else(|| String::from("warm"));
        if !matches!(action.as_str(), "warm" | "stop" | "refresh") {
            return Err(super::error::spiki_error(
                super::error::SpikiCode::InvalidRequest,
                format!("Unsupported semantic action {}", action),
            ));
        }

        let mut backend = backend_for_language(input.language);
        if action == "stop" {
            backend.state = String::from("off");
            backend.idle_for_ms = Some(0);
        } else {
            backend.state = String::from("ready");
            backend.idle_for_ms = Some(0);
        }

        let mut meta = view.workspace.meta.lock();
        meta.semantic_backends
            .insert(backend.language.clone(), backend.clone());
        Ok(SemanticEnsureOutput {
            workspace_id: view.workspace_id.clone(),
            backend,
        })
    }

    pub fn execution_error(error: super::SpikiError) -> ExecutionError {
        error.into()
    }

    pub(crate) fn refresh_workspace(
        &self,
        view: &ViewContext,
        scope: Option<&Scope>,
    ) -> SpikiResult<Coverage> {
        let scan = scan_workspace(
            &view.roots_canonical,
            scope,
            ScanOptions {
                include_ignored: scope
                    .and_then(|value| value.include_ignored)
                    .unwrap_or(false),
                include_generated: scope
                    .and_then(|value| value.include_generated)
                    .unwrap_or(false),
                max_index_file_size_bytes: self.state.config.max_index_file_size_bytes,
            },
        )?;
        let new_known_files: HashMap<PathBuf, KnownFile> = scan.known_files.into_iter().collect();
        let mut meta = view.workspace.meta.lock();
        if meta.known_files != new_known_files {
            meta.revision += 1;
            meta.known_files = new_known_files;
            for plan in meta.plans.values_mut() {
                if plan.state == super::state::PlanState::Ready {
                    plan.state = super::state::PlanState::Stale;
                }
            }
        }

        Ok(Coverage {
            partial: false,
            files_indexed: Some(meta.known_files.len() as u64),
            files_total_estimate: Some(meta.known_files.len() as u64),
        })
    }

    pub(crate) fn current_revision(&self, view: &ViewContext) -> String {
        let meta = view.workspace.meta.lock();
        format!("rev_{}", meta.revision)
    }
}
