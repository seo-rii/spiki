use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::RecommendedWatcher;
use parking_lot::Mutex;

use crate::model::{BackendState, FileEdit, PlanSummary};
use crate::text::{CanonicalRoot, KnownFile};

use super::config::WorkspaceSettings;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub max_index_file_size_bytes: u64,
    pub plan_ttl: Duration,
    pub default_exclude_components: Vec<String>,
    pub forced_exclude_components: Vec<String>,
    pub watch_enabled: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_index_file_size_bytes: 2 * 1024 * 1024,
            plan_ttl: Duration::from_secs(30 * 60),
            default_exclude_components: vec![
                String::from("node_modules"),
                String::from("vendor"),
                String::from("dist"),
                String::from("build"),
                String::from("target"),
                String::from(".next"),
                String::from(".turbo"),
                String::from(".cache"),
                String::from("coverage"),
            ],
            forced_exclude_components: vec![String::from(".git")],
            watch_enabled: true,
        }
    }
}

#[derive(Clone)]
pub struct ViewContext {
    pub client_session_id: String,
    pub view_id: String,
    pub workspace_id: String,
    pub roots: Vec<String>,
    pub(crate) roots_canonical: Vec<CanonicalRoot>,
    pub(crate) workspace: Arc<WorkspaceState>,
}

#[derive(Clone)]
pub struct Runtime {
    pub(crate) state: Arc<RuntimeState>,
}

pub(crate) struct RuntimeState {
    pub(crate) config: RuntimeConfig,
    pub(crate) workspaces: Mutex<HashMap<String, Arc<WorkspaceState>>>,
}

pub(crate) struct WorkspaceState {
    pub(crate) _workspace_id: String,
    pub(crate) _roots: Vec<CanonicalRoot>,
    pub(crate) settings: Mutex<WorkspaceSettings>,
    pub(crate) watcher: Mutex<Option<RecommendedWatcher>>,
    pub(crate) dirty: Arc<AtomicBool>,
    pub(crate) meta: Mutex<WorkspaceMeta>,
    pub(crate) write_lock: Mutex<()>,
}

#[derive(Debug)]
pub(crate) struct WorkspaceMeta {
    pub(crate) revision: u64,
    pub(crate) known_files: HashMap<PathBuf, KnownFile>,
    pub(crate) semantic_backends: HashMap<String, BackendState>,
    pub(crate) plans: HashMap<String, StoredPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanState {
    Ready,
    Stale,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredPlan {
    pub(crate) plan_id: String,
    pub(crate) view_id: String,
    pub(crate) workspace_revision: String,
    pub(crate) _created_at: DateTime<Utc>,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) file_edits: Vec<FileEdit>,
    pub(crate) _summary: PlanSummary,
    pub(crate) state: PlanState,
}

pub(crate) fn workspace_id_for_roots(roots: &[CanonicalRoot]) -> String {
    let joined = roots
        .iter()
        .map(|root| root.uri.as_str())
        .collect::<Vec<_>>()
        .join("|");
    format!("ws_{}", short_hash(&joined))
}

fn short_hash(input: &str) -> String {
    blake3::hash(input.as_bytes()).to_hex()[..12].to_string()
}
