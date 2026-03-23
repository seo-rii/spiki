use globset::{Glob, GlobSetBuilder};

use crate::model::{
    ExecutionError, ReadSpansInput, ReadSpansOutput, SearchTextInput, SearchTextOutput,
    SemanticEnsureInput, SemanticEnsureOutput, SemanticStatusOutput, Warning,
    WorkspaceStatusInput, WorkspaceStatusOutput,
};
use crate::text::{
    build_text_span, ensure_path_in_roots, file_uri_from_path, path_from_file_uri, read_text_file,
    scan_workspace, search_file, ScanOptions, ScanResult,
};

use super::error::SpikiResult;
use super::languages::{backend_for_language, detected_backends};
use super::state::{Runtime, ViewContext};

impl Runtime {
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
        let scan = if scope
            .as_ref()
            .map(|value| {
                !value.include_ignored.unwrap_or(false)
                    && !value.include_generated.unwrap_or(false)
                    && !value.include_default_excluded.unwrap_or(false)
            })
            .unwrap_or(true)
        {
            let mut scope_targets = Vec::new();
            if let Some(uris) = scope.as_ref().and_then(|value| value.uris.as_ref()) {
                for uri in uris {
                    let path = path_from_file_uri(uri)?;
                    scope_targets.push(ensure_path_in_roots(&path, &view.roots_canonical)?);
                }
            }
            scope_targets.sort();
            scope_targets.dedup();

            let exclude_globs = if let Some(patterns) = scope
                .as_ref()
                .and_then(|value| value.exclude_globs.as_ref())
            {
                let mut builder = GlobSetBuilder::new();
                for pattern in patterns {
                    builder.add(Glob::new(pattern).map_err(|error| {
                        super::error::spiki_error(
                            super::error::SpikiCode::InvalidRequest,
                            format!("Invalid exclude glob {pattern}: {error}"),
                        )
                    })?);
                }
                Some(builder.build().map_err(|error| {
                    super::error::spiki_error(
                        super::error::SpikiCode::InvalidRequest,
                        format!("Failed to build exclude globs: {error}"),
                    )
                })?)
            } else {
                None
            };

            let meta = view.workspace.meta.lock();
            let mut files = meta.known_files.keys().cloned().collect::<Vec<_>>();
            if !scope_targets.is_empty() {
                files.retain(|path| scope_targets.iter().any(|target| path.starts_with(target)));
            }
            if let Some(exclude_globs) = &exclude_globs {
                files.retain(|path| !exclude_globs.is_match(path));
            }
            files.sort();
            let mut known_files = meta
                .known_files
                .iter()
                .filter(|(path, _)| {
                    scope_targets.is_empty()
                        || scope_targets.iter().any(|target| path.starts_with(target))
                })
                .filter(|(path, _)| {
                    exclude_globs
                        .as_ref()
                        .map(|value| !value.is_match(path))
                        .unwrap_or(true)
                })
                .map(|(path, known_file)| (path.clone(), known_file.clone()))
                .collect::<Vec<_>>();
            known_files.sort_by(|left, right| left.0.cmp(&right.0));
            ScanResult {
                files,
                known_files,
                warnings: Vec::new(),
            }
        } else {
            scan_workspace(
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
                    include_default_excluded: scope
                        .as_ref()
                        .and_then(|value| value.include_default_excluded)
                        .unwrap_or(false),
                    max_index_file_size_bytes: self.state.config.max_index_file_size_bytes,
                    default_exclude_components: self.state.config.default_exclude_components.clone(),
                    forced_exclude_components: self.state.config.forced_exclude_components.clone(),
                },
            )?
        };
        warnings.extend(scan.warnings);
        let limit = input.limit.unwrap_or(200);
        let context_lines = input.context_lines.unwrap_or(1);
        let mode = input.mode.unwrap_or_default();
        let case_sensitive = input.case_sensitive.unwrap_or(false);
        let max_files = scope
            .as_ref()
            .and_then(|value| value.max_files)
            .unwrap_or(usize::MAX);
        let mut truncated = scan.files.len() > max_files;

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
}
