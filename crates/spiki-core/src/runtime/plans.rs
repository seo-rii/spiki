#[cfg(test)]
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use crate::model::{
    ApplyPlanInput, ApplyPlanOutput, DiscardPlanInput, DiscardPlanOutput, FileEdit,
    InspectPlanInput, InspectPlanOutput, PlanSummary, PreparePlanInput, PreparePlanOutput,
};
use crate::text::{
    apply_edits_to_text, ensure_path_in_roots, fingerprint_for_file, path_from_file_uri,
    read_text_file, scan_workspace, ScanOptions,
};

use super::error::{spiki_error, SpikiCode, SpikiResult};
use super::state::{PlanState, Runtime, StoredPlan, ViewContext, WorkspaceMeta};

#[cfg(test)]
thread_local! {
    static APPLY_PLAN_BEFORE_COMMIT_HOOK: RefCell<Option<Box<dyn FnMut()>>> =
        const { RefCell::new(None) };
}

#[cfg(test)]
fn run_apply_plan_before_commit_hook_for_test() {
    APPLY_PLAN_BEFORE_COMMIT_HOOK.with(|slot| {
        if let Some(mut hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn run_apply_plan_before_commit_hook_for_test() {}

struct RenderedPlanFile {
    path: PathBuf,
    bytes: Vec<u8>,
}

struct StagedPlanFile {
    path: PathBuf,
    temp_path: PathBuf,
    backup_path: PathBuf,
}

impl Runtime {
    fn sweep_terminal_plans(meta: &mut WorkspaceMeta) {
        let now = Utc::now();
        meta.plans
            .retain(|_, plan| plan.state == PlanState::Ready && plan.expires_at > now);
    }

    pub fn prepare_plan(
        &self,
        view: &ViewContext,
        input: PreparePlanInput,
    ) -> SpikiResult<PreparePlanOutput> {
        self.refresh_workspace(view, None)?;
        if input.file_edits.is_empty() {
            return Err(spiki_error(
                SpikiCode::InvalidRequest,
                String::from("prepare_plan requires at least one file edit"),
            ));
        }

        let mut prepared_file_edits = Vec::new();
        let mut edits = 0u64;
        let mut seen_files = BTreeSet::new();

        for file_edit in input.file_edits {
            let path = path_from_file_uri(&file_edit.uri)?;
            let canonical = ensure_path_in_roots(&path, &view.roots_canonical)?;
            if !seen_files.insert(canonical.clone()) {
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!("Duplicate file edit entry for {}", canonical.display()),
                ));
            }
            if file_edit.edits.is_empty() {
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!(
                        "File edit for {} must contain at least one edit",
                        canonical.display()
                    ),
                ));
            }
            let loaded = read_text_file(&canonical)?;
            let actual = fingerprint_for_file(&canonical, &loaded);
            if let Some(expected) = &file_edit.fingerprint {
                if &actual != expected {
                    return Err(spiki_error(
                        SpikiCode::StalePlan,
                        format!("Fingerprint mismatch for {}", file_edit.uri),
                    ));
                }
            }

            let _ = apply_edits_to_text(&loaded.text, &file_edit.edits, &loaded.line_ending)?;
            edits += file_edit.edits.len() as u64;
            prepared_file_edits.push(FileEdit {
                uri: file_edit.uri,
                fingerprint: Some(actual),
                edits: file_edit.edits,
            });
        }

        let summary = PlanSummary {
            files_touched: prepared_file_edits.len() as u64,
            edits,
            languages: None,
            blocked: Some(0),
            requires_confirmation: true,
        };
        let revision = self.current_revision(view);
        let plan_id =
            self.store_plan(view, revision.clone(), prepared_file_edits, summary.clone())?;

        Ok(PreparePlanOutput {
            plan_id,
            workspace_id: view.workspace_id.clone(),
            workspace_revision: revision,
            summary,
            warnings: Vec::new(),
        })
    }

    pub fn apply_plan(
        &self,
        view: &ViewContext,
        input: ApplyPlanInput,
    ) -> SpikiResult<ApplyPlanOutput> {
        self.refresh_workspace(view, None)?;
        let _guard = view.workspace.write_lock.lock();
        let mut meta = view.workspace.meta.lock();
        Self::sweep_terminal_plans(&mut meta);
        let current_revision = format!("rev_{}", meta.revision);
        let plan = meta.plans.get(&input.plan_id).cloned().ok_or_else(|| {
            spiki_error(
                SpikiCode::NotFound,
                format!("Plan {} not found", input.plan_id),
            )
        })?;

        if plan.expires_at <= Utc::now() {
            meta.plans.remove(&input.plan_id);
            return Err(spiki_error(
                SpikiCode::StalePlan,
                format!("Plan {} has expired", plan.plan_id),
            ));
        }
        if plan.state != PlanState::Ready {
            meta.plans.remove(&input.plan_id);
            return Err(spiki_error(
                SpikiCode::Conflict,
                format!("Plan {} is not ready to apply", plan.plan_id),
            ));
        }
        if plan.view_id != view.view_id {
            return Err(spiki_error(
                SpikiCode::Forbidden,
                format!("Plan {} belongs to a different view", plan.plan_id),
            ));
        }
        if plan.workspace_revision != input.expected_workspace_revision
            || current_revision != input.expected_workspace_revision
        {
            meta.plans.remove(&input.plan_id);
            return Err(spiki_error(
                SpikiCode::StalePlan,
                format!("Plan {} is stale", plan.plan_id),
            ));
        }

        let mut rewritten_files = Vec::new();
        let mut files_touched = 0u64;
        let mut edits_applied = 0u64;
        let mut seen_files = BTreeSet::new();

        for file_edit in &plan.file_edits {
            let path = path_from_file_uri(&file_edit.uri)?;
            let canonical = ensure_path_in_roots(&path, &view.roots_canonical)?;
            if !seen_files.insert(canonical.clone()) {
                meta.plans.remove(&input.plan_id);
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!("Plan {} contains duplicate file edits", plan.plan_id),
                ));
            }
            if file_edit.edits.is_empty() {
                meta.plans.remove(&input.plan_id);
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!("Plan {} contains an empty file edit", plan.plan_id),
                ));
            }
            let loaded = read_text_file(&canonical)?;
            if let Some(expected) = &file_edit.fingerprint {
                let actual = fingerprint_for_file(&canonical, &loaded);
                if &actual != expected {
                    meta.plans.remove(&input.plan_id);
                    return Err(spiki_error(
                        SpikiCode::StalePlan,
                        format!("Fingerprint mismatch for {}", file_edit.uri),
                    ));
                }
            }
            let rewritten =
                apply_edits_to_text(&loaded.text, &file_edit.edits, &loaded.line_ending)?;
            let rewritten_bytes = if loaded.encoding == "utf-8" {
                rewritten.into_bytes()
            } else if loaded.encoding == "utf-8-bom" {
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(rewritten.as_bytes());
                bytes
            } else if loaded.encoding == "utf-16le" {
                let mut bytes = vec![0xFF, 0xFE];
                for unit in rewritten.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                bytes
            } else if loaded.encoding == "utf-16be" {
                let mut bytes = vec![0xFE, 0xFF];
                for unit in rewritten.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_be_bytes());
                }
                bytes
            } else {
                return Err(spiki_error(
                    SpikiCode::Unsupported,
                    format!(
                        "Unsupported text encoding {} for {}",
                        loaded.encoding,
                        canonical.display()
                    ),
                ));
            };
            rewritten_files.push(RenderedPlanFile {
                path: canonical,
                bytes: rewritten_bytes,
            });
            files_touched += 1;
            edits_applied += file_edit.edits.len() as u64;
        }

        // Stage rewritten bytes into temp files before touching any original file.
        let mut staged_files: Vec<StagedPlanFile> = Vec::new();
        for rewritten_file in &rewritten_files {
            let temp_path = rewritten_file.path.with_file_name(format!(
                ".{}.spiki-tmp-{}",
                rewritten_file
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("spiki"),
                Uuid::now_v7().simple()
            ));
            let backup_path = rewritten_file.path.with_file_name(format!(
                ".{}.spiki-bak-{}",
                rewritten_file
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("spiki"),
                Uuid::now_v7().simple()
            ));
            if let Err(error) = fs::write(&temp_path, &rewritten_file.bytes) {
                let _ = fs::remove_file(&temp_path);
                for staged_file in staged_files {
                    let _ = fs::remove_file(staged_file.temp_path);
                }
                return Err(spiki_error(
                    SpikiCode::Internal,
                    format!("Failed to write {}: {error}", rewritten_file.path.display()),
                ));
            }
            staged_files.push(StagedPlanFile {
                path: rewritten_file.path.clone(),
                temp_path,
                backup_path,
            });
        }

        run_apply_plan_before_commit_hook_for_test();

        // Revalidate live file fingerprints after staging but before commit.
        for file_edit in &plan.file_edits {
            let path = path_from_file_uri(&file_edit.uri)?;
            let canonical = ensure_path_in_roots(&path, &view.roots_canonical)?;
            let loaded = match read_text_file(&canonical) {
                Ok(value) => value,
                Err(error) => {
                    for staged_file in &staged_files {
                        let _ = fs::remove_file(&staged_file.temp_path);
                    }
                    return Err(error);
                }
            };
            if let Some(expected) = &file_edit.fingerprint {
                let actual = fingerprint_for_file(&canonical, &loaded);
                if &actual != expected {
                    for staged_file in &staged_files {
                        let _ = fs::remove_file(&staged_file.temp_path);
                    }
                    meta.plans.remove(&input.plan_id);
                    return Err(spiki_error(
                        SpikiCode::StalePlan,
                        format!("Fingerprint mismatch for {} before commit", file_edit.uri),
                    ));
                }
            }
        }

        let mut backed_up = Vec::new();
        for staged_file in &staged_files {
            if let Err(error) = fs::rename(&staged_file.path, &staged_file.backup_path) {
                for staged_file in &staged_files {
                    let _ = fs::remove_file(&staged_file.temp_path);
                }
                for (rollback_path, rollback_backup) in backed_up.into_iter().rev() {
                    let _ = fs::rename(rollback_backup, rollback_path);
                }
                return Err(spiki_error(
                    SpikiCode::Internal,
                    format!(
                        "Failed to stage {} for apply: {error}",
                        staged_file.path.display()
                    ),
                ));
            }
            backed_up.push((staged_file.path.clone(), staged_file.backup_path.clone()));
        }

        let mut committed = Vec::new();
        for (index, staged_file) in staged_files.iter().enumerate() {
            if let Err(error) = fs::rename(&staged_file.temp_path, &staged_file.path) {
                let _ = fs::remove_file(&staged_file.temp_path);
                let _ = fs::rename(&staged_file.backup_path, &staged_file.path);

                for (rollback_path, rollback_backup) in committed.into_iter().rev() {
                    let _ = fs::remove_file(&rollback_path);
                    let _ = fs::rename(&rollback_backup, &rollback_path);
                }
                for remaining_file in staged_files.iter().skip(index + 1) {
                    let _ = fs::remove_file(&remaining_file.temp_path);
                    let _ = fs::rename(&remaining_file.backup_path, &remaining_file.path);
                }
                return Err(spiki_error(
                    SpikiCode::Internal,
                    format!("Failed to commit {}: {error}", staged_file.path.display()),
                ));
            }
            committed.push((staged_file.path.clone(), staged_file.backup_path.clone()));
        }

        for (_, backup_path) in committed {
            let _ = fs::remove_file(backup_path);
        }

        meta.plans.remove(&input.plan_id);
        meta.revision += 1;
        let next_revision = format!("rev_{}", meta.revision);
        let settings = view.workspace.settings.lock().clone();
        meta.known_files = scan_workspace(
            &view.roots_canonical,
            None,
            ScanOptions {
                include_ignored: false,
                include_generated: false,
                include_default_excluded: false,
                max_index_file_size_bytes: settings.max_index_file_size_bytes,
                default_exclude_components: settings.default_exclude_components.clone(),
                forced_exclude_components: settings.forced_exclude_components.clone(),
            },
        )?
        .known_files
        .into_iter()
        .collect();
        view.workspace
            .dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);

        Ok(ApplyPlanOutput {
            applied: true,
            workspace_id: view.workspace_id.clone(),
            previous_revision: input.expected_workspace_revision,
            new_revision: next_revision,
            files_touched,
            edits_applied,
            warnings: Vec::new(),
        })
    }

    pub fn discard_plan(
        &self,
        view: &ViewContext,
        input: DiscardPlanInput,
    ) -> SpikiResult<DiscardPlanOutput> {
        self.refresh_workspace(view, None)?;
        let mut meta = view.workspace.meta.lock();
        Self::sweep_terminal_plans(&mut meta);
        let discarded = if let Some(plan) = meta.plans.get(&input.plan_id) {
            if plan.view_id == view.view_id {
                meta.plans.remove(&input.plan_id);
                true
            } else {
                false
            }
        } else {
            false
        };

        Ok(DiscardPlanOutput {
            discarded,
            plan_id: input.plan_id,
        })
    }

    pub fn inspect_plan(
        &self,
        view: &ViewContext,
        input: InspectPlanInput,
    ) -> SpikiResult<InspectPlanOutput> {
        self.refresh_workspace(view, None)?;
        let mut meta = view.workspace.meta.lock();
        Self::sweep_terminal_plans(&mut meta);
        let plan = meta.plans.get(&input.plan_id).cloned().ok_or_else(|| {
            spiki_error(
                SpikiCode::NotFound,
                format!("Plan {} not found", input.plan_id),
            )
        })?;
        if plan.view_id != view.view_id {
            return Err(spiki_error(
                SpikiCode::Forbidden,
                format!("Plan {} belongs to a different view", plan.plan_id),
            ));
        }
        if plan.state != PlanState::Ready || plan.expires_at <= Utc::now() {
            meta.plans.remove(&input.plan_id);
            return Err(spiki_error(
                SpikiCode::StalePlan,
                format!("Plan {} is no longer available", input.plan_id),
            ));
        }

        Ok(InspectPlanOutput {
            plan_id: plan.plan_id,
            workspace_id: view.workspace_id.clone(),
            workspace_revision: plan.workspace_revision,
            summary: plan._summary,
            file_edits: plan.file_edits,
            warnings: Vec::new(),
        })
    }

    pub fn seed_plan_for_test(
        &self,
        view: &ViewContext,
        file_edits: Vec<FileEdit>,
    ) -> SpikiResult<(String, String)> {
        self.refresh_workspace(view, None)?;
        let revision = self.current_revision(view);
        let summary = PlanSummary {
            files_touched: file_edits.len() as u64,
            edits: file_edits
                .iter()
                .map(|value| value.edits.len() as u64)
                .sum(),
            languages: None,
            blocked: Some(0),
            requires_confirmation: true,
        };
        let plan_id = self.store_plan(view, revision.clone(), file_edits, summary)?;

        Ok((plan_id, revision))
    }

    fn store_plan(
        &self,
        view: &ViewContext,
        revision: String,
        file_edits: Vec<FileEdit>,
        summary: PlanSummary,
    ) -> SpikiResult<String> {
        let plan_id = format!("plan_{}", Uuid::now_v7().simple());
        let settings = view.workspace.settings.lock().clone();
        let plan_ttl = chrono::Duration::from_std(settings.plan_ttl).map_err(|error| {
            spiki_error(
                SpikiCode::Internal,
                format!(
                    "Configured plan_ttl {:?} is out of range for chrono duration conversion: {error}",
                    settings.plan_ttl
                ),
            )
        })?;
        let mut meta = view.workspace.meta.lock();
        Self::sweep_terminal_plans(&mut meta);
        meta.plans.insert(
            plan_id.clone(),
            StoredPlan {
                plan_id: plan_id.clone(),
                view_id: view.view_id.clone(),
                workspace_revision: revision,
                _created_at: Utc::now(),
                expires_at: Utc::now() + plan_ttl,
                file_edits,
                _summary: summary,
                state: PlanState::Ready,
            },
        );
        Ok(plan_id)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;

    use tempfile::tempdir;

    use crate::model::{ApplyPlanInput, FileEdit, Position, Range, TextEdit, WorkspaceStatusInput};
    use crate::runtime::{Runtime, RuntimeConfig};
    use crate::text::{file_uri_from_path, fingerprint_for_file, read_text_file};

    use super::APPLY_PLAN_BEFORE_COMMIT_HOOK;

    #[test]
    fn apply_plan_revalidates_live_files_before_commit() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("sample.ts");
        fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

        let runtime = Runtime::new(Default::default());
        let view = runtime
            .upsert_view("session_test", &[file_uri_from_path(temp.path())])
            .unwrap();
        let mut previous_revision = None;
        let mut revision_stabilized = false;
        for _ in 0..8 {
            let status = runtime
                .workspace_status(
                    &view,
                    WorkspaceStatusInput {
                        include_backends: Some(false),
                        include_coverage: Some(false),
                    },
                )
                .unwrap();
            if previous_revision.as_deref() == Some(status.workspace_revision.as_str()) {
                revision_stabilized = true;
                break;
            }
            previous_revision = Some(status.workspace_revision);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            revision_stabilized,
            "workspace revision did not stabilize before seeding the plan"
        );

        let loaded = read_text_file(&file_path).unwrap();
        let fingerprint = fingerprint_for_file(&file_path, &loaded);
        let (plan_id, revision) = runtime
            .seed_plan_for_test(
                &view,
                vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: Some(fingerprint),
                    edits: vec![
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 6,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 1,
                                    character: 12,
                                },
                                end: Position {
                                    line: 1,
                                    character: 19,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                    ],
                }],
            )
            .unwrap();

        let external_contents = String::from("const external = 1;\nconsole.log(external);\n");
        let hook_ran = Arc::new(AtomicBool::new(false));
        APPLY_PLAN_BEFORE_COMMIT_HOOK.with(|slot| {
            let file_path = file_path.clone();
            let external_contents = external_contents.clone();
            let hook_ran = hook_ran.clone();
            *slot.borrow_mut() = Some(Box::new(move || {
                hook_ran.store(true, Ordering::Relaxed);
                fs::write(&file_path, &external_contents).unwrap();
            }));
        });

        let error = runtime
            .apply_plan(
                &view,
                ApplyPlanInput {
                    plan_id: plan_id.clone(),
                    expected_workspace_revision: revision.clone(),
                },
            )
            .unwrap_err();

        assert!(hook_ran.load(Ordering::Relaxed));
        assert_eq!(error.code, "AE_STALE_PLAN");
        assert!(error.message.contains("Fingerprint mismatch"));
        assert_eq!(fs::read_to_string(&file_path).unwrap(), external_contents);

        let missing = runtime
            .apply_plan(
                &view,
                ApplyPlanInput {
                    plan_id,
                    expected_workspace_revision: revision,
                },
            )
            .unwrap_err();
        assert_eq!(missing.code, "AE_NOT_FOUND");
    }

    #[test]
    fn seed_plan_for_test_rejects_out_of_range_plan_ttl() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("sample.ts");
        fs::write(&file_path, "const answer = 42;\n").unwrap();

        let runtime = Runtime::new(RuntimeConfig {
            plan_ttl: Duration::MAX,
            watch_enabled: false,
            ..RuntimeConfig::default()
        });
        let view = runtime
            .upsert_view("session_test", &[file_uri_from_path(temp.path())])
            .unwrap();
        let loaded = read_text_file(&file_path).unwrap();
        let fingerprint = fingerprint_for_file(&file_path, &loaded);

        let error = runtime
            .seed_plan_for_test(
                &view,
                vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: Some(fingerprint),
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 12,
                            },
                        },
                        new_text: String::from("result"),
                    }],
                }],
            )
            .unwrap_err();

        assert_eq!(error.code, "AE_INTERNAL");
        assert!(error.message.contains("plan_ttl"));
    }
}
