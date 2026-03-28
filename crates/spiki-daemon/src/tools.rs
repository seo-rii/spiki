use anyhow::{anyhow, Context, Result};
use schemars::{schema_for, JsonSchema};
use serde_json::{json, Map, Value};
use spiki_core::{
    ApplyPlanInput, DiscardPlanInput, ExecutionError, PreparePlanInput, ReadSpansInput, Runtime,
    SearchTextInput, SemanticEnsureInput, SemanticStatusInput, WorkspaceStatusInput,
};

use crate::session::Session;

#[derive(Clone, Copy)]
enum ToolKind {
    WorkspaceStatus,
    ReadSpans,
    SearchText,
    PreparePlan,
    ApplyPlan,
    DiscardPlan,
    SemanticStatus,
    SemanticEnsure,
}

struct ToolDefinition {
    kind: ToolKind,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    task_support: Option<&'static str>,
}

const TOOL_DEFINITIONS: &[ToolDefinition] = &[
    ToolDefinition {
        kind: ToolKind::WorkspaceStatus,
        name: "ae.workspace.status",
        title: "Workspace Status",
        description: "Summarize the active view, workspace revision, coverage, and backend state.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::ReadSpans,
        name: "ae.workspace.read_spans",
        title: "Read Spans",
        description: "Read precise file spans with optional surrounding context.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::SearchText,
        name: "ae.workspace.search_text",
        title: "Search Text",
        description: "Run ignore-aware literal, regex, or whole-word text search.",
        task_support: Some("optional"),
    },
    ToolDefinition {
        kind: ToolKind::PreparePlan,
        name: "ae.edit.prepare_plan",
        title: "Prepare Plan",
        description: "Validate and store a new edit plan for later apply or discard.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::ApplyPlan,
        name: "ae.edit.apply_plan",
        title: "Apply Plan",
        description: "Apply a previously prepared edit plan after CAS validation.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::DiscardPlan,
        name: "ae.edit.discard_plan",
        title: "Discard Plan",
        description: "Discard a stored edit plan without applying it.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::SemanticStatus,
        name: "ae.semantic.status",
        title: "Semantic Status",
        description: "Return detected leaf semantic backends and their current skeleton lifecycle state for the active workspace.",
        task_support: None,
    },
    ToolDefinition {
        kind: ToolKind::SemanticEnsure,
        name: "ae.semantic.ensure",
        title: "Semantic Ensure",
        description: "Warm, stop, or refresh the skeleton semantic backend state cache for a language profile.",
        task_support: None,
    },
];

pub(crate) async fn handle_tool_call(
    session: &Session,
    request_id: &str,
    params: Value,
    related_task_id: Option<&str>,
) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call missing name")?;
    let definition = match tool_definition(name) {
        Some(definition) => definition,
        None => {
            return Ok(tool_failure(ExecutionError {
                code: String::from("AE_NOT_FOUND"),
                message: format!("Unknown tool {name}"),
                retryable: false,
                details: None,
            }));
        }
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let progress_token = params
        .get("_meta")
        .and_then(|value| value.get("progressToken"))
        .filter(|value| value.is_string() || value.is_i64() || value.is_u64())
        .cloned();

    if let Some(progress_token) = &progress_token {
        session
            .send_progress(
                related_task_id.unwrap_or(request_id),
                progress_token,
                1,
                3,
                &format!("Resolving workspace view for {name}"),
            )
            .await?;
    }
    if session.is_operation_cancelled(request_id).await {
        return Err(anyhow!("request cancelled"));
    }
    let view = session.ensure_view().await?;
    if let Some(progress_token) = &progress_token {
        session
            .send_progress(
                related_task_id.unwrap_or(request_id),
                progress_token,
                2,
                3,
                &format!("Running {name}"),
            )
            .await?;
    }
    if session.is_operation_cancelled(request_id).await {
        return Err(anyhow!("request cancelled"));
    }

    let result = match definition.kind {
        ToolKind::WorkspaceStatus => {
            match serde_json::from_value::<WorkspaceStatusInput>(arguments) {
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
            }
        }
        ToolKind::ReadSpans => match serde_json::from_value::<ReadSpansInput>(arguments) {
            Ok(input) => match validate_read_spans_input(&input) {
                Some(error) => tool_failure(error),
                None => match session.runtime.read_spans(&view, input) {
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
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        ToolKind::SearchText => match serde_json::from_value::<SearchTextInput>(arguments) {
            Ok(input) => match validate_search_text_input(&input) {
                Some(error) => tool_failure(error),
                None => match session.runtime.search_text(&view, input) {
                    Ok(output) => tool_success(
                        format!("found {} text matches", output.matches.len()),
                        serde_json::to_value(output)?,
                    ),
                    Err(error) => tool_failure(Runtime::execution_error(error)),
                },
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        ToolKind::PreparePlan => match serde_json::from_value::<PreparePlanInput>(arguments) {
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
        ToolKind::ApplyPlan => match serde_json::from_value::<ApplyPlanInput>(arguments) {
            Ok(input) => match session.runtime.apply_plan(&view, input) {
                Ok(output) => tool_success(
                    format!("applied {} edits", output.edits_applied),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        ToolKind::DiscardPlan => match serde_json::from_value::<DiscardPlanInput>(arguments) {
            Ok(input) => match session.runtime.discard_plan(&view, input) {
                Ok(output) => tool_success(
                    format!("discarded={} for {}", output.discarded, output.plan_id),
                    serde_json::to_value(output)?,
                ),
                Err(error) => tool_failure(Runtime::execution_error(error)),
            },
            Err(error) => tool_failure(invalid_arguments(error)),
        },
        ToolKind::SemanticStatus => {
            match serde_json::from_value::<SemanticStatusInput>(arguments) {
                Ok(input) => match session.runtime.semantic_status(&view, input.language) {
                    Ok(output) => tool_success(
                        format!("{} semantic backends tracked", output.backends.len()),
                        serde_json::to_value(output)?,
                    ),
                    Err(error) => tool_failure(Runtime::execution_error(error)),
                },
                Err(error) => tool_failure(invalid_arguments(error)),
            }
        }
        ToolKind::SemanticEnsure => {
            match serde_json::from_value::<SemanticEnsureInput>(arguments) {
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
            }
        }
    };

    if session.is_operation_cancelled(request_id).await {
        return Err(anyhow!("request cancelled"));
    }
    if let Some(progress_token) = &progress_token {
        session
            .send_progress(
                related_task_id.unwrap_or(request_id),
                progress_token,
                3,
                3,
                &format!("Completed {name}"),
            )
            .await?;
    }

    Ok(result)
}

pub(crate) fn tool_specs() -> Vec<Value> {
    TOOL_DEFINITIONS
        .iter()
        .map(|definition| {
            let mut tool = json!({
                "name": definition.name,
                "title": definition.title,
                "description": definition.description,
                "inputSchema": input_schema_for(definition.kind)
            });
            if let Some(task_support) = definition.task_support {
                tool["execution"] = json!({
                    "taskSupport": task_support
                });
            }
            tool
        })
        .collect()
}

pub(crate) fn tool_supports_task_execution(name: &str) -> bool {
    tool_definition(name)
        .and_then(|definition| definition.task_support)
        .is_some()
}

fn tool_definition(name: &str) -> Option<&'static ToolDefinition> {
    TOOL_DEFINITIONS
        .iter()
        .find(|definition| definition.name == name)
}

fn input_schema_for(kind: ToolKind) -> Value {
    match kind {
        ToolKind::WorkspaceStatus => {
            generated_schema::<WorkspaceStatusInput>("workspace status schema")
        }
        ToolKind::ReadSpans => read_spans_schema(),
        ToolKind::SearchText => search_text_schema(),
        ToolKind::PreparePlan => prepare_plan_schema(),
        ToolKind::ApplyPlan => generated_schema::<ApplyPlanInput>("apply plan schema"),
        ToolKind::DiscardPlan => generated_schema::<DiscardPlanInput>("discard plan schema"),
        ToolKind::SemanticStatus => {
            generated_schema::<SemanticStatusInput>("semantic status schema")
        }
        ToolKind::SemanticEnsure => semantic_ensure_schema(),
    }
}

fn read_spans_schema() -> Value {
    let mut schema = generated_schema::<ReadSpansInput>("read spans schema");
    set_schema_value(
        root_property_mut(&mut schema, "spans"),
        "minItems",
        json!(1),
    );
    schema
}

fn search_text_schema() -> Value {
    let mut schema = generated_schema::<SearchTextInput>("search text schema");
    set_schema_value(
        root_property_mut(&mut schema, "query"),
        "minLength",
        json!(1),
    );
    let limit = root_property_mut(&mut schema, "limit");
    set_schema_value(limit, "minimum", json!(1));
    set_schema_value(limit, "maximum", json!(10000));
    set_schema_value(limit, "default", json!(200));

    let scope = definition_mut(&mut schema, "Scope");
    set_schema_value(property_mut(scope, "uris"), "uniqueItems", json!(true));
    set_schema_value(property_mut(scope, "maxFiles"), "minimum", json!(1));
    schema
}

fn prepare_plan_schema() -> Value {
    let mut schema = generated_schema::<PreparePlanInput>("prepare plan schema");
    set_schema_value(
        root_property_mut(&mut schema, "fileEdits"),
        "minItems",
        json!(1),
    );
    set_schema_value(
        property_mut(definition_mut(&mut schema, "FileEdit"), "edits"),
        "minItems",
        json!(1),
    );
    schema
}

fn semantic_ensure_schema() -> Value {
    let mut schema = generated_schema::<SemanticEnsureInput>("semantic ensure schema");
    let action = root_property_mut(&mut schema, "action");
    set_schema_value(action, "enum", json!(["warm", "stop", "refresh"]));
    set_schema_value(action, "default", json!("warm"));
    schema
}

fn generated_schema<T: JsonSchema>(label: &str) -> Value {
    serde_json::to_value(schema_for!(T)).expect(label)
}

fn validate_read_spans_input(input: &ReadSpansInput) -> Option<ExecutionError> {
    if input.spans.is_empty() {
        return Some(invalid_request_message(
            "spans must contain at least one request",
        ));
    }

    None
}

fn validate_search_text_input(input: &SearchTextInput) -> Option<ExecutionError> {
    if input.query.is_empty() {
        return Some(invalid_request_message("query must not be empty"));
    }
    if let Some(limit) = input.limit {
        if !(1..=10000).contains(&limit) {
            return Some(invalid_request_message("limit must be between 1 and 10000"));
        }
    }
    if let Some(scope) = &input.scope {
        if matches!(scope.max_files, Some(0)) {
            return Some(invalid_request_message("scope.maxFiles must be at least 1"));
        }
        if let Some(uris) = &scope.uris {
            let mut seen = std::collections::BTreeSet::new();
            for uri in uris {
                if !seen.insert(uri) {
                    return Some(invalid_request_message(
                        "scope.uris must not contain duplicates",
                    ));
                }
            }
        }
    }

    None
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
    invalid_request_message(error.to_string())
}

fn invalid_request_message(message: impl Into<String>) -> ExecutionError {
    ExecutionError {
        code: String::from("AE_INVALID_REQUEST"),
        message: message.into(),
        retryable: false,
        details: None,
    }
}

fn root_property_mut<'a>(schema: &'a mut Value, property: &str) -> &'a mut Value {
    schema_object_mut(schema, "properties")
        .get_mut(property)
        .unwrap_or_else(|| panic!("schema missing property {property}"))
}

fn definition_mut<'a>(schema: &'a mut Value, name: &str) -> &'a mut Value {
    schema_object_mut(schema, "definitions")
        .get_mut(name)
        .unwrap_or_else(|| panic!("schema missing definition {name}"))
}

fn property_mut<'a>(schema: &'a mut Value, property: &str) -> &'a mut Value {
    schema_object_mut(schema, "properties")
        .get_mut(property)
        .unwrap_or_else(|| panic!("schema missing property {property}"))
}

fn schema_object_mut<'a>(schema: &'a mut Value, field: &str) -> &'a mut Map<String, Value> {
    schema
        .as_object_mut()
        .and_then(|value| value.get_mut(field))
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("schema missing object field {field}"))
}

fn set_schema_value(schema: &mut Value, key: &str, value: Value) {
    schema
        .as_object_mut()
        .unwrap_or_else(|| panic!("schema node for {key} is not an object"))
        .insert(String::from(key), value);
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use spiki_core::model::{Position, Range, ReadSpanRequest};
    use spiki_core::Scope;

    use super::{
        tool_specs, validate_read_spans_input, validate_search_text_input, ReadSpansInput,
        SearchTextInput,
    };

    #[test]
    fn tool_specs_include_phase1_input_constraints() {
        let tools = tool_specs();
        let read_spans_schema = find_input_schema(&tools, "ae.workspace.read_spans");
        assert_eq!(
            read_spans_schema["properties"]["spans"]["minItems"],
            json!(1)
        );

        let search_text_schema = find_input_schema(&tools, "ae.workspace.search_text");
        assert_eq!(
            find_tool(&tools, "ae.workspace.search_text")["execution"]["taskSupport"],
            json!("optional")
        );
        assert_eq!(
            search_text_schema["properties"]["query"]["minLength"],
            json!(1)
        );
        assert_eq!(
            search_text_schema["properties"]["limit"]["minimum"],
            json!(1)
        );
        assert_eq!(
            search_text_schema["properties"]["limit"]["maximum"],
            json!(10000)
        );
        assert_eq!(
            search_text_schema["definitions"]["Scope"]["properties"]["uris"]["uniqueItems"],
            json!(true)
        );
        assert_eq!(
            search_text_schema["definitions"]["Scope"]["properties"]["maxFiles"]["minimum"],
            json!(1)
        );

        let prepare_plan_schema = find_input_schema(&tools, "ae.edit.prepare_plan");
        assert_eq!(
            prepare_plan_schema["properties"]["fileEdits"]["minItems"],
            json!(1)
        );
        assert_eq!(
            prepare_plan_schema["definitions"]["FileEdit"]["properties"]["edits"]["minItems"],
            json!(1)
        );

        let semantic_ensure_schema = find_input_schema(&tools, "ae.semantic.ensure");
        assert_eq!(
            semantic_ensure_schema["properties"]["action"]["enum"],
            json!(["warm", "stop", "refresh"])
        );
        assert_eq!(
            semantic_ensure_schema["properties"]["action"]["default"],
            json!("warm")
        );
    }

    #[test]
    fn search_text_validation_rejects_invalid_limits_and_duplicate_scope_uris() {
        let zero_limit = SearchTextInput {
            query: String::from("needle"),
            mode: None,
            case_sensitive: None,
            scope: None,
            context_lines: None,
            limit: Some(0),
        };
        assert_eq!(
            validate_search_text_input(&zero_limit).unwrap().code,
            "AE_INVALID_REQUEST"
        );

        let duplicate_uris = SearchTextInput {
            query: String::from("needle"),
            mode: None,
            case_sensitive: None,
            scope: Some(Scope {
                uris: Some(vec![
                    String::from("file:///tmp/a"),
                    String::from("file:///tmp/a"),
                ]),
                include_ignored: None,
                include_generated: None,
                include_default_excluded: None,
                exclude_globs: None,
                max_files: Some(1),
            }),
            context_lines: None,
            limit: Some(10),
        };
        assert_eq!(
            validate_search_text_input(&duplicate_uris).unwrap().code,
            "AE_INVALID_REQUEST"
        );
    }

    #[test]
    fn read_spans_validation_rejects_empty_requests() {
        let input = ReadSpansInput { spans: Vec::new() };
        assert_eq!(
            validate_read_spans_input(&input).unwrap().code,
            "AE_INVALID_REQUEST"
        );

        let valid = ReadSpansInput {
            spans: vec![ReadSpanRequest {
                uri: String::from("file:///tmp/sample.ts"),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                context_lines: Some(0),
            }],
        };
        assert!(validate_read_spans_input(&valid).is_none());
    }

    fn find_tool<'a>(tools: &'a [Value], name: &str) -> &'a Value {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))
    }

    fn find_input_schema<'a>(tools: &'a [Value], name: &str) -> &'a Value {
        &find_tool(tools, name)["inputSchema"]
    }
}
