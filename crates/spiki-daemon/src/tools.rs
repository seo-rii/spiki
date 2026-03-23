use anyhow::{Context, Result};
use schemars::schema_for;
use serde_json::{json, Value};
use spiki_core::{
    ApplyPlanInput, DiscardPlanInput, ExecutionError, PreparePlanInput, ReadSpansInput, Runtime,
    SearchTextInput, SemanticEnsureInput, SemanticStatusInput, WorkspaceStatusInput,
};

use crate::session::Session;

pub(crate) async fn handle_tool_call(session: &Session, params: Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call missing name")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let view = session.ensure_view().await?;

    let result = match name {
        "ae.workspace.status" => match serde_json::from_value::<WorkspaceStatusInput>(arguments) {
            Ok(input) => match session.runtime.workspace_status(&view, input) {
                Ok(output) => tool_success(
                    format!(
                        "workspace {} at {} with {} roots",
                        output.workspace_id,
                        output.workspace_revision,
                        output.roots.len()
                    ),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.workspace.read_spans" => match serde_json::from_value::<ReadSpansInput>(arguments) {
            Ok(input) => match session.runtime.read_spans(&view, input) {
                Ok(output) => tool_success(
                    format!(
                        "read {} spans at {}",
                        output.spans.len(),
                        output.workspace_revision
                    ),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.workspace.search_text" => match serde_json::from_value::<SearchTextInput>(arguments) {
            Ok(input) => match session.runtime.search_text(&view, input) {
                Ok(output) => tool_success(
                    format!("found {} text matches", output.matches.len()),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.edit.prepare_plan" => match serde_json::from_value::<PreparePlanInput>(arguments) {
            Ok(input) => match session.runtime.prepare_plan(&view, input) {
                Ok(output) => tool_success(
                    format!(
                        "prepared plan {} with {} edits",
                        output.plan_id, output.summary.edits
                    ),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.edit.apply_plan" => match serde_json::from_value::<ApplyPlanInput>(arguments) {
            Ok(input) => match session.runtime.apply_plan(&view, input) {
                Ok(output) => tool_success(
                    format!("applied {} edits", output.edits_applied),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.edit.discard_plan" => match serde_json::from_value::<DiscardPlanInput>(arguments) {
            Ok(input) => match session.runtime.discard_plan(&view, input) {
                Ok(output) => tool_success(
                    format!("discarded={} for {}", output.discarded, output.plan_id),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.semantic.status" => match serde_json::from_value::<SemanticStatusInput>(arguments) {
            Ok(input) => match session.runtime.semantic_status(&view, input.language) {
                Ok(output) => tool_success(
                    format!("{} semantic backends tracked", output.backends.len()),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        "ae.semantic.ensure" => match serde_json::from_value::<SemanticEnsureInput>(arguments) {
            Ok(input) => match session.runtime.semantic_ensure(&view, input) {
                Ok(output) => tool_success(
                    format!(
                        "semantic backend {} is {}",
                        output.backend.language, output.backend.state
                    ),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        _ => tool_failure(ExecutionError {
            code: String::from("AE_NOT_FOUND"),
            message: format!("Unknown tool {name}"),
            retryable: false,
            details: None,
        }),
    };

    Ok(result)
}

pub(crate) fn tool_specs() -> Vec<Value> {
    let workspace_status_schema =
        serde_json::to_value(schema_for!(WorkspaceStatusInput)).expect("workspace status schema");
    let read_spans_schema =
        serde_json::to_value(schema_for!(ReadSpansInput)).expect("read spans schema");
    let search_text_schema =
        serde_json::to_value(schema_for!(SearchTextInput)).expect("search text schema");
    let prepare_plan_schema =
        serde_json::to_value(schema_for!(PreparePlanInput)).expect("prepare plan schema");
    let apply_plan_schema =
        serde_json::to_value(schema_for!(ApplyPlanInput)).expect("apply plan schema");
    let discard_plan_schema =
        serde_json::to_value(schema_for!(DiscardPlanInput)).expect("discard plan schema");
    let semantic_status_schema =
        serde_json::to_value(schema_for!(SemanticStatusInput)).expect("semantic status schema");
    let semantic_ensure_schema =
        serde_json::to_value(schema_for!(SemanticEnsureInput)).expect("semantic ensure schema");

    vec![
        json!({
            "name": "ae.workspace.status",
            "title": "Workspace Status",
            "description": "Summarize the active view, workspace revision, coverage, and backend state.",
            "inputSchema": workspace_status_schema
        }),
        json!({
            "name": "ae.workspace.read_spans",
            "title": "Read Spans",
            "description": "Read precise file spans with optional surrounding context.",
            "inputSchema": read_spans_schema
        }),
        json!({
            "name": "ae.workspace.search_text",
            "title": "Search Text",
            "description": "Run ignore-aware literal, regex, or whole-word text search.",
            "inputSchema": search_text_schema
        }),
        json!({
            "name": "ae.edit.prepare_plan",
            "title": "Prepare Plan",
            "description": "Validate and store a new edit plan for later apply or discard.",
            "inputSchema": prepare_plan_schema
        }),
        json!({
            "name": "ae.edit.apply_plan",
            "title": "Apply Plan",
            "description": "Apply a previously prepared edit plan after CAS validation.",
            "inputSchema": apply_plan_schema
        }),
        json!({
            "name": "ae.edit.discard_plan",
            "title": "Discard Plan",
            "description": "Discard a stored edit plan without applying it.",
            "inputSchema": discard_plan_schema
        }),
        json!({
            "name": "ae.semantic.status",
            "title": "Semantic Status",
            "description": "Return detected leaf semantic backends and their current skeleton lifecycle state for the active workspace.",
            "inputSchema": semantic_status_schema
        }),
        json!({
            "name": "ae.semantic.ensure",
            "title": "Semantic Ensure",
            "description": "Warm, stop, or refresh the skeleton semantic backend state cache for a language profile.",
            "inputSchema": semantic_ensure_schema
        }),
    ]
}

fn tool_success(summary: String, structured: Value) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": summary
            }
        ],
        "structuredContent": structured,
        "isError": false
    })
}

fn tool_failure(error: ExecutionError) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": format!("{}: {}", error.code, error.message)
            }
        ],
        "structuredContent": error,
        "isError": true
    })
}

fn invalid_arguments(error: serde_json::Error) -> ExecutionError {
    ExecutionError {
        code: String::from("AE_INVALID_REQUEST"),
        message: error.to_string(),
        retryable: false,
        details: None,
    }
}
