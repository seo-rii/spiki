use std::collections::BTreeSet;
use std::fs;

use chrono::Utc;
use uuid::Uuid;

use crate::model::{
    ApplyPlanInput, ApplyPlanOutput, DiscardPlanInput, DiscardPlanOutput, FileEdit, PlanSummary,
    PreparePlanInput, PreparePlanOutput,
};
use crate::text::{
    apply_edits_to_text, ensure_path_in_roots, fingerprint_for_file, path_from_file_uri,
    read_text_file, scan_workspace, ScanOptions,
};

use super::error::{spiki_error, SpikiCode, SpikiResult};
use super::state::{PlanState, Runtime, StoredPlan, ViewContext};

impl Runtime {
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
        let plan_id = self.store_plan(view, revision.clone(), prepared_file_edits, summary.clone());

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
        let current_revision = format!("rev_{}", meta.revision);
        let plan = meta.plans.get_mut(&input.plan_id).ok_or_else(|| {
            spiki_error(
                SpikiCode::NotFound,
                format!("Plan {} not found", input.plan_id),
            )
        })?;

        if plan.expires_at <= Utc::now() {
            plan.state = PlanState::Expired;
            return Err(spiki_error(
                SpikiCode::StalePlan,
                format!("Plan {} has expired", plan.plan_id),
            ));
        }
        if plan.state != PlanState::Ready {
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
            plan.state = PlanState::Stale;
            return Err(spiki_error(
                SpikiCode::StalePlan,
                format!("Plan {} is stale", plan.plan_id),
            ));
        }

        let mut original_files = Vec::new();
        let mut rewritten_files = Vec::new();
        let mut files_touched = 0u64;
        let mut edits_applied = 0u64;
        let mut seen_files = BTreeSet::new();

        for file_edit in &plan.file_edits {
            let path = path_from_file_uri(&file_edit.uri)?;
            let canonical = ensure_path_in_roots(&path, &view.roots_canonical)?;
            if !seen_files.insert(canonical.clone()) {
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!("Plan {} contains duplicate file edits", plan.plan_id),
                ));
            }
            if file_edit.edits.is_empty() {
                return Err(spiki_error(
                    SpikiCode::InvalidRequest,
                    format!("Plan {} contains an empty file edit", plan.plan_id),
                ));
            }
            let original_bytes = fs::read(&canonical).map_err(|error| {
                spiki_error(
                    SpikiCode::Internal,
                    format!("Failed to read {}: {error}", canonical.display()),
                )
            })?;
            let loaded = read_text_file(&canonical)?;
            if let Some(expected) = &file_edit.fingerprint {
                let actual = fingerprint_for_file(&canonical, &loaded);
                if &actual != expected {
                    plan.state = PlanState::Stale;
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
            original_files.push((canonical.clone(), original_bytes));
            rewritten_files.push((canonical, rewritten_bytes));
            files_touched += 1;
            edits_applied += file_edit.edits.len() as u64;
        }

        let mut written = Vec::new();
        for (path, content) in &rewritten_files {
            let temp_path = path.with_file_name(format!(
                ".{}.spiki-tmp-{}",
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("spiki"),
                Uuid::now_v7().simple()
            ));
            if let Err(error) =
                fs::write(&temp_path, content).and_then(|_| fs::rename(&temp_path, path))
            {
                let _ = fs::remove_file(&temp_path);
                for (rollback_path, rollback_content) in written {
                    let _ = fs::write(rollback_path, rollback_content);
                }
                return Err(spiki_error(
                    SpikiCode::Internal,
                    format!("Failed to write {}: {error}", path.display()),
                ));
            }
            if let Some((_, original)) = original_files
                .iter()
                .find(|(saved_path, _)| saved_path == path)
            {
                written.push((path.clone(), original.clone()));
            }
        }

        plan.state = PlanState::Applied;
        meta.revision += 1;
        let next_revision = format!("rev_{}", meta.revision);
        meta.known_files = scan_workspace(
            &view.roots_canonical,
            None,
            ScanOptions {
                include_ignored: false,
                include_generated: false,
                include_default_excluded: false,
                max_index_file_size_bytes: self.state.config.max_index_file_size_bytes,
                default_exclude_components: self.state.config.default_exclude_components.clone(),
                forced_exclude_components: self.state.config.forced_exclude_components.clone(),
            },
        )?
        .known_files
        .into_iter()
        .collect();

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
        let discarded = if let Some(plan) = meta.plans.get_mut(&input.plan_id) {
            if plan.view_id == view.view_id {
                plan.state = PlanState::Discarded;
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
        let plan_id = self.store_plan(view, revision.clone(), file_edits, summary);

        Ok((plan_id, revision))
    }

    fn store_plan(
        &self,
        view: &ViewContext,
        revision: String,
        file_edits: Vec<FileEdit>,
        summary: PlanSummary,
    ) -> String {
        let plan_id = format!("plan_{}", Uuid::now_v7().simple());
        let mut meta = view.workspace.meta.lock();
        meta.plans.insert(
            plan_id.clone(),
            StoredPlan {
                plan_id: plan_id.clone(),
                view_id: view.view_id.clone(),
                workspace_revision: revision,
                _created_at: Utc::now(),
                expires_at: Utc::now()
                    + chrono::Duration::from_std(self.state.config.plan_ttl).unwrap(),
                file_edits,
                _summary: summary,
                state: PlanState::Ready,
            },
        );
        plan_id
    }
}
