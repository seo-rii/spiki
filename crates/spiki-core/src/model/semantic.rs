use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LocationRef, Position, Warning};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendState {
    pub language: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_for_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SemanticEnsureInput {
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SemanticStatusInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticStatusOutput {
    pub workspace_id: String,
    pub backends: Vec<BackendState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEnsureOutput {
    pub workspace_id: String,
    pub backend: BackendState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DefinitionInput {
    pub language: String,
    pub uri: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionOutput {
    pub workspace_id: String,
    pub workspace_revision: String,
    pub engine: String,
    pub backend: BackendState,
    pub definitions: Vec<LocationRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}
