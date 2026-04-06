use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FileFingerprint, Range, Warning};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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
pub struct PreparePlanInput {
    pub file_edits: Vec<FileEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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
pub struct InspectPlanInput {
    pub plan_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectPlanOutput {
    pub plan_id: String,
    pub workspace_id: String,
    pub workspace_revision: String,
    pub summary: PlanSummary,
    pub file_edits: Vec<FileEdit>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ApplyPlanInput {
    pub plan_id: String,
    pub expected_workspace_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscardPlanOutput {
    pub discarded: bool,
    pub plan_id: String,
}
