use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use parking_lot::MutexGuard;
use serde::Deserialize;
use serde_json::Value;

use crate::text::CanonicalRoot;

use super::error::{spiki_error, SpikiCode, SpikiResult};
use super::state::{Runtime, RuntimeConfig, WorkspaceState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticBindingKind {
    Builtin,
    Lsp,
}

#[derive(Debug, Clone)]
pub struct SemanticBinding {
    pub language: String,
    pub provider_id: String,
    pub kind: SemanticBindingKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub initialization_options: Option<Value>,
    pub workspace_configuration: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceSettings {
    pub max_index_file_size_bytes: u64,
    pub plan_ttl: Duration,
    pub default_exclude_components: Vec<String>,
    pub forced_exclude_components: Vec<String>,
    pub watch_enabled: bool,
    pub semantic_bindings: HashMap<String, SemanticBinding>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpikiConfigFile {
    runtime: Option<RuntimeSection>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeSection {
    max_index_file_size_bytes: Option<u64>,
    plan_ttl_seconds: Option<u64>,
    default_exclude_components: Option<Vec<String>>,
    forced_exclude_components: Option<Vec<String>>,
    watch: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LanguagesFile {
    bindings: Option<HashMap<String, BindingSection>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingSection {
    kind: Option<String>,
    provider: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    initialization_options: Option<Value>,
    workspace_configuration: Option<Value>,
}

impl WorkspaceSettings {
    pub(crate) fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self {
            max_index_file_size_bytes: config.max_index_file_size_bytes,
            plan_ttl: config.plan_ttl,
            default_exclude_components: config.default_exclude_components.clone(),
            forced_exclude_components: config.forced_exclude_components.clone(),
            watch_enabled: config.watch_enabled,
            semantic_bindings: HashMap::new(),
        }
    }
}

pub(crate) fn load_workspace_settings(
    roots: &[CanonicalRoot],
    base_config: &RuntimeConfig,
) -> SpikiResult<WorkspaceSettings> {
    let mut settings = WorkspaceSettings::from_runtime_config(base_config);

    for root in roots {
        for config_path in [
            root.path.join(".spiki").join("config.yaml"),
            root.path.join("spiki.yaml"),
        ] {
            if !config_path.is_file() {
                continue;
            }
            let config_file: SpikiConfigFile = load_yaml_file(&config_path)?;
            if let Some(runtime) = config_file.runtime {
                if let Some(value) = runtime.max_index_file_size_bytes {
                    settings.max_index_file_size_bytes = value;
                }
                if let Some(value) = runtime.plan_ttl_seconds {
                    settings.plan_ttl = Duration::from_secs(value);
                }
                if let Some(value) = runtime.default_exclude_components {
                    settings.default_exclude_components = value;
                }
                if let Some(value) = runtime.forced_exclude_components {
                    settings.forced_exclude_components = value;
                }
                if let Some(value) = runtime.watch {
                    settings.watch_enabled = value;
                }
            }
        }

        for languages_path in [
            root.path.join(".spiki").join("languages.yaml"),
            root.path.join("spiki.languages.yaml"),
        ] {
            if !languages_path.is_file() {
                continue;
            }
            let languages_file: LanguagesFile = load_yaml_file(&languages_path)?;
            if let Some(bindings) = languages_file.bindings {
                for (language, binding) in bindings {
                    let kind = binding.kind.unwrap_or_else(|| String::from("builtin"));
                    let provider_id = binding.provider.unwrap_or_else(|| format!("{kind}:{language}"));
                    let semantic_binding = if kind == "builtin" {
                        SemanticBinding {
                            language: language.clone(),
                            provider_id,
                            kind: SemanticBindingKind::Builtin,
                            command: None,
                            args: Vec::new(),
                            env: binding.env.unwrap_or_default(),
                            initialization_options: binding.initialization_options,
                            workspace_configuration: binding.workspace_configuration,
                        }
                    } else if kind == "lsp" {
                        let command = binding.command.ok_or_else(|| {
                            spiki_error(
                                SpikiCode::InvalidRequest,
                                format!(
                                    "{} declares lsp binding for {} without command",
                                    languages_path.display(),
                                    language
                                ),
                            )
                        })?;
                        SemanticBinding {
                            language: language.clone(),
                            provider_id,
                            kind: SemanticBindingKind::Lsp,
                            command: Some(command),
                            args: binding.args.unwrap_or_default(),
                            env: binding.env.unwrap_or_default(),
                            initialization_options: binding.initialization_options,
                            workspace_configuration: binding.workspace_configuration,
                        }
                    } else {
                        return Err(spiki_error(
                            SpikiCode::InvalidRequest,
                            format!(
                                "{} declares unsupported semantic binding kind {} for {}",
                                languages_path.display(),
                                kind,
                                language
                            ),
                        ));
                    };
                    settings.semantic_bindings.insert(language, semantic_binding);
                }
            }
        }
    }

    Ok(settings)
}

pub(crate) fn workspace_binding(
    settings: &MutexGuard<'_, WorkspaceSettings>,
    language: &str,
) -> Option<SemanticBinding> {
    settings.semantic_bindings.get(language).cloned()
}

pub(crate) fn configure_workspace_watcher(
    workspace: &WorkspaceState,
    roots: &[CanonicalRoot],
    settings: &WorkspaceSettings,
) -> SpikiResult<()> {
    if !settings.watch_enabled {
        workspace.watcher.lock().take();
        return Ok(());
    }
    if workspace.watcher.lock().is_some() {
        return Ok(());
    }

    let watcher_dirty = workspace.dirty.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if result.is_ok() {
            watcher_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    })
    .map_err(|error| {
        spiki_error(
            SpikiCode::Internal,
            format!("Failed to create workspace watcher: {error}"),
        )
    })?;

    for root in roots {
        watcher
            .watch(root.path.as_ref(), RecursiveMode::Recursive)
            .map_err(|error| {
                spiki_error(
                    SpikiCode::Internal,
                    format!("Failed to watch {}: {error}", root.path.display()),
                )
            })?;
    }

    *workspace.watcher.lock() = Some(watcher);
    Ok(())
}

impl Runtime {
    pub(crate) fn reload_workspace_settings(
        &self,
        workspace: &WorkspaceState,
        roots: &[CanonicalRoot],
    ) -> SpikiResult<()> {
        let settings = load_workspace_settings(roots, &self.state.config)?;
        configure_workspace_watcher(workspace, roots, &settings)?;
        *workspace.settings.lock() = settings;
        Ok(())
    }
}

fn load_yaml_file<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> SpikiResult<T> {
    let text = fs::read_to_string(path).map_err(|error| {
        spiki_error(
            SpikiCode::Internal,
            format!("Failed to read {}: {error}", path.display()),
        )
    })?;
    serde_yaml::from_str(&text).map_err(|error| {
        spiki_error(
            SpikiCode::InvalidRequest,
            format!("Failed to parse {}: {error}", path.display()),
        )
    })
}
