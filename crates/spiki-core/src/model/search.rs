use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Coverage, FileFingerprint, Range, Scope, Warning};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
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
