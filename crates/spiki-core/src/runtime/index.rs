use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::model::Coverage;
use crate::text::{canonical_roots_from_uris, scan_workspace, ScanOptions};

use super::config::{SemanticBinding, SemanticBindingKind, WorkspaceSettings, workspace_binding};
use super::error::SpikiResult;
use super::languages::backend_for_language;
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
                        settings: Mutex::new(WorkspaceSettings::from_runtime_config(
                            &self.state.config,
                        )),
                        watcher: Mutex::new(None),
                        dirty: Arc::new(AtomicBool::new(true)),
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
        self.reload_workspace_settings(&workspace, &canonical_roots)?;
        workspace.dirty.store(true, Ordering::Relaxed);

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

    pub(crate) fn refresh_workspace(
        &self,
        view: &ViewContext,
        scope: Option<&crate::model::Scope>,
    ) -> SpikiResult<Coverage> {
        if scope.is_none() {
            let meta = view.workspace.meta.lock();
            if !view.workspace.dirty.load(Ordering::Relaxed) && !meta.known_files.is_empty() {
                return Ok(Coverage {
                    partial: false,
                    files_indexed: Some(meta.known_files.len() as u64),
                    files_total_estimate: Some(meta.known_files.len() as u64),
                });
            }
        }
        self.reload_workspace_settings(&view.workspace, &view.roots_canonical)?;
        let settings = view.workspace.settings.lock().clone();
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
                include_default_excluded: scope
                    .and_then(|value| value.include_default_excluded)
                    .unwrap_or(false),
                max_index_file_size_bytes: settings.max_index_file_size_bytes,
                default_exclude_components: settings.default_exclude_components.clone(),
                forced_exclude_components: settings.forced_exclude_components.clone(),
            },
        )?;
        let new_known_files = scan.known_files.into_iter().collect();
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
        view.workspace.dirty.store(false, Ordering::Relaxed);

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

    pub fn workspace_revision(&self, view: &ViewContext) -> String {
        self.current_revision(view)
    }

    pub fn workspace_settings(&self, view: &ViewContext) -> WorkspaceSettings {
        view.workspace.settings.lock().clone()
    }

    pub fn workspace_semantic_binding(
        &self,
        view: &ViewContext,
        language: &str,
    ) -> Option<super::config::SemanticBinding> {
        let settings = view.workspace.settings.lock();
        workspace_binding(&settings, language).or_else(|| {
            let backend = backend_for_language(language.to_string());
            let provider_id = backend.provider?;
            if provider_id == "phase1-skeleton" {
                return None;
            }
            Some(SemanticBinding {
                language: language.to_string(),
                provider_id,
                kind: SemanticBindingKind::Builtin,
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                initialization_options: None,
                workspace_configuration: None,
            })
        })
    }
}
