use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocationRef {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FileFingerprint {
    pub uri: String,
    pub content_hash: String,
    pub size: u64,
    pub mtime_ms: u64,
    pub line_ending: String,
    pub encoding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextMatch {
    pub uri: String,
    pub range: Range,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_indexed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_total_estimate: Option<u64>,
}

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
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FileEdit {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<FileFingerprint>,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub files_touched: u64,
    pub edits: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<u64>,
    pub requires_confirmation: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadSpansOutput {
    pub workspace_revision: String,
    pub spans: Vec<TextSpan>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    #[default]
    Literal,
    Regex,
    Word,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchTextInput {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<SearchMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchTextOutput {
    pub workspace_revision: String,
    pub engine: String,
    pub matches: Vec<TextMatch>,
    pub truncated: bool,
    pub coverage: Coverage,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreparePlanInput {
    pub file_edits: Vec<FileEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparePlanOutput {
    pub plan_id: String,
    pub workspace_id: String,
    pub workspace_revision: String,
    pub summary: PlanSummary,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ApplyPlanInput {
    pub plan_id: String,
    pub expected_workspace_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPlanOutput {
    pub applied: bool,
    pub workspace_id: String,
    pub previous_revision: String,
    pub new_revision: String,
    pub files_touched: u64,
    pub edits_applied: u64,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DiscardPlanInput {
    pub plan_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscardPlanOutput {
    pub discarded: bool,
    pub plan_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SemanticEnsureInput {
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}
