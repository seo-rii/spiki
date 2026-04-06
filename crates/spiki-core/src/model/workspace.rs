use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{BackendState, FileFingerprint, Range, Warning};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    pub uri: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_indexed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_total_estimate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Scope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_generated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_default_excluded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_globs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_files: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceStatusInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_backends: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_coverage: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatusOutput {
    pub client_session_id: String,
    pub view_id: String,
    pub workspace_id: String,
    pub workspace_revision: String,
    pub roots: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backends: Option<Vec<BackendState>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReadSpanRequest {
    pub uri: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_lines: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadSpansInput {
    pub spans: Vec<ReadSpanRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadSpansOutput {
    pub workspace_revision: String,
    pub spans: Vec<TextSpan>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}
