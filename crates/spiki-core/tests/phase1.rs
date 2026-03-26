use std::collections::BTreeSet;
use std::fs;

use serde_json::json;
use spiki_core::model::{FileEdit, Position, Range, Scope, SemanticEnsureInput, TextEdit};
use spiki_core::text::{
    file_uri_from_path, fingerprint_for_file, read_text_file, set_scan_log_path_for_test,
};
use spiki_core::{
    ApplyPlanInput, DiscardPlanInput, PreparePlanInput, ReadSpansInput, Runtime, RuntimeConfig,
    SearchTextInput, WorkspaceStatusInput,
};
use tempfile::tempdir;

#[test]
fn search_text_respects_gitignore_by_default() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(temp.path().join("app.ts"), "const needle = 1;\n").unwrap();
    fs::write(
        temp.path().join("ignored.txt"),
        "needle should not appear\n",
    )
    .unwrap();

    let runtime = Runtime::new(Default::default());
    let root_uri = file_uri_from_path(temp.path());
    let view = runtime.upsert_view("session_test", &[root_uri]).unwrap();

    let status = runtime
        .workspace_status(
            &view,
            WorkspaceStatusInput {
                include_backends: Some(true),
                include_coverage: Some(true),
            },
        )
        .unwrap();
    assert_eq!(status.workspace_revision, "rev_1");

    let output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("needle"),
                mode: None,
                case_sensitive: None,
                scope: None,
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();

    assert_eq!(output.matches.len(), 1);
    assert!(output.matches[0].uri.ends_with("/app.ts"));
}

#[test]
fn search_text_scans_workspace_once_by_default() {
    let temp = tempdir().unwrap();
    let scan_log = temp.path().join("scan.log");
    fs::write(temp.path().join("app.ts"), "const needle = 1;\n").unwrap();
    set_scan_log_path_for_test(Some(scan_log.clone()));

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("needle"),
                mode: None,
                case_sensitive: None,
                scope: None,
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();
    set_scan_log_path_for_test(None);

    assert_eq!(output.matches.len(), 1);
    assert_eq!(
        fs::read_to_string(&scan_log)
            .unwrap()
            .lines()
            .filter(|line| *line == "scan")
            .count(),
        1
    );
}

#[test]
fn search_text_can_include_default_excluded_directories_when_requested() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("dist")).unwrap();
    fs::write(
        temp.path().join("dist").join("generated.ts"),
        "const needle = 1;\n",
    )
    .unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let default_output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("needle"),
                mode: None,
                case_sensitive: None,
                scope: None,
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();
    assert_eq!(default_output.matches.len(), 0);

    let include_default_excluded_output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("needle"),
                mode: None,
                case_sensitive: None,
                scope: Some(Scope {
                    uris: None,
                    include_ignored: None,
                    include_generated: None,
                    include_default_excluded: Some(true),
                    exclude_globs: None,
                    max_files: None,
                }),
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();

    assert_eq!(include_default_excluded_output.matches.len(), 1);
    assert!(include_default_excluded_output.matches[0]
        .uri
        .ends_with("/dist/generated.ts"));
}

#[test]
fn search_text_marks_truncated_when_max_files_limits_search() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("a.ts"), "const value = 1;\n").unwrap();
    fs::write(temp.path().join("b.ts"), "const needle = 1;\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("needle"),
                mode: None,
                case_sensitive: None,
                scope: Some(Scope {
                    uris: None,
                    include_ignored: None,
                    include_generated: None,
                    include_default_excluded: None,
                    exclude_globs: None,
                    max_files: Some(1),
                }),
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();

    assert_eq!(output.matches.len(), 0);
    assert!(output.truncated);
}

#[test]
fn workspace_status_can_index_paths_removed_from_default_excludes() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("dist")).unwrap();
    fs::write(
        temp.path().join("dist").join("generated.ts"),
        "const needle = 1;\n",
    )
    .unwrap();

    let runtime = Runtime::new(RuntimeConfig {
        max_index_file_size_bytes: 2 * 1024 * 1024,
        plan_ttl: std::time::Duration::from_secs(30 * 60),
        default_exclude_components: Vec::new(),
        forced_exclude_components: vec![String::from(".git")],
    });
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .workspace_status(
            &view,
            WorkspaceStatusInput {
                include_backends: Some(false),
                include_coverage: Some(true),
            },
        )
        .unwrap();

    assert_eq!(output.coverage.unwrap().files_indexed, Some(1));
}

#[test]
fn search_text_input_rejects_unknown_fields() {
    let error = serde_json::from_value::<SearchTextInput>(json!({
        "query": "needle",
        "scope": {
            "includeDefaultExcluded": true,
            "unexpected": true
        }
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unexpected"));
}

#[test]
fn search_text_case_insensitive_uses_original_text_offsets() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("unicode.txt"), "İx\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .search_text(
            &view,
            SearchTextInput {
                query: String::from("x"),
                mode: None,
                case_sensitive: Some(false),
                scope: None,
                context_lines: Some(0),
                limit: Some(20),
            },
        )
        .unwrap();

    assert_eq!(output.matches.len(), 1);
    assert_eq!(output.matches[0].range.start.line, 0);
    assert_eq!(output.matches[0].range.start.character, 1);
    assert_eq!(output.matches[0].range.end.character, 2);
}

#[test]
fn read_spans_returns_context_and_fingerprint() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .read_spans(
            &view,
            ReadSpansInput {
                spans: vec![spiki_core::model::ReadSpanRequest {
                    uri: file_uri_from_path(&file_path),
                    range: Range {
                        start: Position {
                            line: 1,
                            character: 0,
                        },
                        end: Position {
                            line: 1,
                            character: 4,
                        },
                    },
                    context_lines: Some(1),
                }],
            },
        )
        .unwrap();

    assert_eq!(output.spans.len(), 1);
    assert_eq!(output.spans[0].text, "beta");
    assert_eq!(output.spans[0].before.as_deref(), Some("alpha\n"));
    assert_eq!(output.spans[0].after.as_deref(), Some("gamma\n"));
    assert_eq!(
        output.spans[0].fingerprint.as_ref().unwrap().line_ending,
        "lf"
    );
}

#[test]
fn apply_plan_updates_files_and_discard_marks_plan() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let loaded = read_text_file(&file_path).unwrap();
    let fingerprint = fingerprint_for_file(&file_path, &loaded);
    let (plan_id, revision) = runtime
        .seed_plan_for_test(
            &view,
            vec![FileEdit {
                uri: file_uri_from_path(&file_path),
                fingerprint: Some(fingerprint),
                edits: vec![
                    TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 13,
                            },
                        },
                        new_text: String::from("newName"),
                    },
                    TextEdit {
                        range: Range {
                            start: Position {
                                line: 1,
                                character: 12,
                            },
                            end: Position {
                                line: 1,
                                character: 19,
                            },
                        },
                        new_text: String::from("newName"),
                    },
                ],
            }],
        )
        .unwrap();

    let apply = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: plan_id.clone(),
                expected_workspace_revision: revision.clone(),
            },
        )
        .unwrap();
    assert!(apply.applied);
    assert_eq!(apply.previous_revision, revision);
    assert_eq!(
        fs::read_to_string(&file_path).unwrap(),
        "const newName = 1;\nconsole.log(newName);\n"
    );
    let applied_again = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: plan_id.clone(),
                expected_workspace_revision: revision.clone(),
            },
        )
        .unwrap_err();
    assert_eq!(applied_again.code, "AE_NOT_FOUND");

    let (discard_plan_id, _) = runtime
        .seed_plan_for_test(
            &view,
            vec![FileEdit {
                uri: file_uri_from_path(&file_path),
                fingerprint: Some(fingerprint_for_file(
                    &file_path,
                    &read_text_file(&file_path).unwrap(),
                )),
                edits: vec![TextEdit {
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 6,
                        },
                        end: Position {
                            line: 0,
                            character: 13,
                        },
                    },
                    new_text: String::from("skipName"),
                }],
            }],
        )
        .unwrap();

    let discarded = runtime
        .discard_plan(
            &view,
            DiscardPlanInput {
                plan_id: discard_plan_id.clone(),
            },
        )
        .unwrap();
    assert!(discarded.discarded);
    assert_eq!(discarded.plan_id, discard_plan_id);
    let discarded_again = runtime
        .discard_plan(
            &view,
            DiscardPlanInput {
                plan_id: discard_plan_id.clone(),
            },
        )
        .unwrap();
    assert!(!discarded_again.discarded);
    let discarded_apply = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: discard_plan_id,
                expected_workspace_revision: apply.new_revision,
            },
        )
        .unwrap_err();
    assert_eq!(discarded_apply.code, "AE_NOT_FOUND");
}

#[test]
fn apply_plan_cleans_up_staged_multifile_artifacts() {
    let temp = tempdir().unwrap();
    let first_path = temp.path().join("first.ts");
    let second_path = temp.path().join("second.ts");
    fs::write(&first_path, "const firstValue = 1;\n").unwrap();
    fs::write(&second_path, "const secondValue = 2;\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let first_loaded = read_text_file(&first_path).unwrap();
    let second_loaded = read_text_file(&second_path).unwrap();
    let (plan_id, revision) = runtime
        .seed_plan_for_test(
            &view,
            vec![
                FileEdit {
                    uri: file_uri_from_path(&first_path),
                    fingerprint: Some(fingerprint_for_file(&first_path, &first_loaded)),
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 16,
                            },
                        },
                        new_text: String::from("renamedOne"),
                    }],
                },
                FileEdit {
                    uri: file_uri_from_path(&second_path),
                    fingerprint: Some(fingerprint_for_file(&second_path, &second_loaded)),
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 17,
                            },
                        },
                        new_text: String::from("renamedTwo"),
                    }],
                },
            ],
        )
        .unwrap();

    let apply = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id,
                expected_workspace_revision: revision,
            },
        )
        .unwrap();
    assert!(apply.applied);
    assert_eq!(
        fs::read_to_string(&first_path).unwrap(),
        "const renamedOne = 1;\n"
    );
    assert_eq!(
        fs::read_to_string(&second_path).unwrap(),
        "const renamedTwo = 2;\n"
    );

    let leftover_artifacts = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains(".spiki-tmp-") || name.contains(".spiki-bak-"))
        .collect::<Vec<_>>();
    assert!(leftover_artifacts.is_empty(), "{leftover_artifacts:?}");
}

#[test]
fn expired_plans_are_swept_on_subsequent_plan_activity() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

    let runtime = Runtime::new(RuntimeConfig {
        plan_ttl: std::time::Duration::from_millis(10),
        ..Default::default()
    });
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let expired = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: None,
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 13,
                            },
                        },
                        new_text: String::from("newName"),
                    }],
                }],
            },
        )
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));

    let fresh = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: None,
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 1,
                                character: 12,
                            },
                            end: Position {
                                line: 1,
                                character: 19,
                            },
                        },
                        new_text: String::from("newName"),
                    }],
                }],
            },
        )
        .unwrap();

    let expired_error = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: expired.plan_id,
                expected_workspace_revision: expired.workspace_revision,
            },
        )
        .unwrap_err();
    assert_eq!(expired_error.code, "AE_NOT_FOUND");

    let apply = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: fresh.plan_id,
                expected_workspace_revision: fresh.workspace_revision,
            },
        )
        .unwrap();
    assert!(apply.applied);
}

#[test]
fn prepare_plan_creates_public_edit_flow() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let prepared = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: None,
                    edits: vec![
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 6,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 1,
                                    character: 12,
                                },
                                end: Position {
                                    line: 1,
                                    character: 19,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                    ],
                }],
            },
        )
        .unwrap();

    assert!(prepared.plan_id.starts_with("plan_"));
    assert_eq!(prepared.workspace_revision, "rev_1");
    assert_eq!(prepared.summary.files_touched, 1);
    assert_eq!(prepared.summary.edits, 2);

    let apply = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id: prepared.plan_id,
                expected_workspace_revision: prepared.workspace_revision,
            },
        )
        .unwrap();
    assert!(apply.applied);
    assert_eq!(
        fs::read_to_string(&file_path).unwrap(),
        "const newName = 1;\nconsole.log(newName);\n"
    );
}

#[test]
fn prepare_plan_rejects_overlapping_edits() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let error = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: None,
                    edits: vec![
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 6,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("first"),
                        },
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 10,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("second"),
                        },
                    ],
                }],
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "AE_INVALID_REQUEST");
}

#[test]
fn prepare_plan_rejects_duplicate_file_edits_for_same_file() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let uri = file_uri_from_path(&file_path);
    let error = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![
                    FileEdit {
                        uri: uri.clone(),
                        fingerprint: None,
                        edits: vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 6,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("firstName"),
                        }],
                    },
                    FileEdit {
                        uri,
                        fingerprint: None,
                        edits: vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: 1,
                                    character: 12,
                                },
                                end: Position {
                                    line: 1,
                                    character: 19,
                                },
                            },
                            new_text: String::from("secondName"),
                        }],
                    },
                ],
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "AE_INVALID_REQUEST");
    assert!(error.message.contains("Duplicate file edit entry"));
}

#[test]
fn apply_plan_rejects_stored_duplicate_file_edits() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\nconsole.log(oldName);\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let loaded = read_text_file(&file_path).unwrap();
    let fingerprint = fingerprint_for_file(&file_path, &loaded);
    let uri = file_uri_from_path(&file_path);
    let (plan_id, revision) = runtime
        .seed_plan_for_test(
            &view,
            vec![
                FileEdit {
                    uri: uri.clone(),
                    fingerprint: Some(fingerprint.clone()),
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 6,
                            },
                            end: Position {
                                line: 0,
                                character: 13,
                            },
                        },
                        new_text: String::from("firstName"),
                    }],
                },
                FileEdit {
                    uri,
                    fingerprint: Some(fingerprint),
                    edits: vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 1,
                                character: 12,
                            },
                            end: Position {
                                line: 1,
                                character: 19,
                            },
                        },
                        new_text: String::from("secondName"),
                    }],
                },
            ],
        )
        .unwrap();

    let error = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id,
                expected_workspace_revision: revision,
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "AE_INVALID_REQUEST");
    assert!(error.message.contains("contains duplicate file edits"));
}

#[test]
fn prepare_plan_rejects_empty_file_edit() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let error = runtime
        .prepare_plan(
            &view,
            PreparePlanInput {
                file_edits: vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: None,
                    edits: Vec::new(),
                }],
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "AE_INVALID_REQUEST");
    assert!(error.message.contains("must contain at least one edit"));
}

#[test]
fn apply_plan_rejects_stored_empty_file_edit() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("sample.ts");
    fs::write(&file_path, "const oldName = 1;\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let loaded = read_text_file(&file_path).unwrap();
    let fingerprint = fingerprint_for_file(&file_path, &loaded);
    let (plan_id, revision) = runtime
        .seed_plan_for_test(
            &view,
            vec![FileEdit {
                uri: file_uri_from_path(&file_path),
                fingerprint: Some(fingerprint),
                edits: Vec::new(),
            }],
        )
        .unwrap();

    let error = runtime
        .apply_plan(
            &view,
            ApplyPlanInput {
                plan_id,
                expected_workspace_revision: revision,
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "AE_INVALID_REQUEST");
    assert!(error.message.contains("contains an empty file edit"));
}

#[test]
fn apply_plan_preserves_original_file_encoding_and_bom() {
    let temp = tempdir().unwrap();
    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();
    let original_text = "const oldName = 1;\nconsole.log(oldName);\n";
    let updated_text = "const newName = 1;\nconsole.log(newName);\n";

    for (file_name, bytes, expected_encoding, expected_bom) in [
        (
            "bom.ts",
            {
                let mut value = vec![0xEF, 0xBB, 0xBF];
                value.extend_from_slice(original_text.as_bytes());
                value
            },
            "utf-8-bom",
            vec![0xEF, 0xBB, 0xBF],
        ),
        (
            "utf16le.ts",
            {
                let mut value = vec![0xFF, 0xFE];
                for unit in original_text.encode_utf16() {
                    value.extend_from_slice(&unit.to_le_bytes());
                }
                value
            },
            "utf-16le",
            vec![0xFF, 0xFE],
        ),
        (
            "utf16be.ts",
            {
                let mut value = vec![0xFE, 0xFF];
                for unit in original_text.encode_utf16() {
                    value.extend_from_slice(&unit.to_be_bytes());
                }
                value
            },
            "utf-16be",
            vec![0xFE, 0xFF],
        ),
    ] {
        let file_path = temp.path().join(file_name);
        fs::write(&file_path, bytes).unwrap();

        let loaded = read_text_file(&file_path).unwrap();
        let fingerprint = fingerprint_for_file(&file_path, &loaded);
        let (plan_id, revision) = runtime
            .seed_plan_for_test(
                &view,
                vec![FileEdit {
                    uri: file_uri_from_path(&file_path),
                    fingerprint: Some(fingerprint),
                    edits: vec![
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 6,
                                },
                                end: Position {
                                    line: 0,
                                    character: 13,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                        TextEdit {
                            range: Range {
                                start: Position {
                                    line: 1,
                                    character: 12,
                                },
                                end: Position {
                                    line: 1,
                                    character: 19,
                                },
                            },
                            new_text: String::from("newName"),
                        },
                    ],
                }],
            )
            .unwrap();

        let apply = runtime
            .apply_plan(
                &view,
                ApplyPlanInput {
                    plan_id,
                    expected_workspace_revision: revision,
                },
            )
            .unwrap();
        assert!(apply.applied);

        let reloaded = read_text_file(&file_path).unwrap();
        assert_eq!(reloaded.encoding, expected_encoding);
        assert_eq!(reloaded.text, updated_text);

        let rewritten_bytes = fs::read(&file_path).unwrap();
        assert!(rewritten_bytes.starts_with(&expected_bom));
    }
}

#[test]
fn semantic_status_detects_built_in_web_framework_profiles() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src/app")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{
  "dependencies": {
    "react": "18.3.0",
    "next": "14.2.0",
    "preact": "10.24.0",
    "vue": "3.4.0",
    "nuxt": "3.13.0",
    "svelte": "5.0.0",
    "@sveltejs/kit": "2.0.0",
    "@angular/core": "18.0.0",
    "astro": "4.0.0",
    "solid-js": "1.8.0",
    "@solidjs/start": "1.0.0",
    "@builder.io/qwik": "1.8.0",
    "ember-source": "5.0.0",
    "lit": "3.2.0",
    "alpinejs": "3.14.0",
    "gatsby": "5.13.0",
    "@remix-run/react": "2.11.0",
    "@remix-run/dev": "2.11.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("tsconfig.json"),
        "{\n  \"compilerOptions\": {}\n}\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/app/App.tsx"),
        "export function App() { return <main />; }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/app/App.vue"),
        "<template><div /></template>\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/app/App.svelte"),
        "<script>let count = 0;</script>\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/app/page.astro"),
        "---\nconst x = 1;\n---\n<div />\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("angular.json"),
        "{\n  \"projects\": {}\n}\n",
    )
    .unwrap();
    fs::write(temp.path().join("next.config.js"), "module.exports = {};\n").unwrap();
    fs::write(
        temp.path().join("nuxt.config.ts"),
        "export default defineNuxtConfig({});\n",
    )
    .unwrap();
    fs::write(temp.path().join("gatsby-config.ts"), "export default {};\n").unwrap();
    fs::write(
        temp.path().join("remix.config.js"),
        "module.exports = {};\n",
    )
    .unwrap();
    fs::write(temp.path().join("astro.config.mjs"), "export default {};\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime.semantic_status(&view, None).unwrap();
    let languages = output
        .backends
        .into_iter()
        .map(|backend| backend.language)
        .collect::<BTreeSet<_>>();

    for expected in [
        "react-ts",
        "nextjs",
        "remix",
        "gatsby",
        "preact",
        "nuxt",
        "sveltekit",
        "angular",
        "astro",
        "solidstart",
        "qwik",
        "ember",
        "lit",
        "alpine",
    ] {
        assert!(languages.contains(expected), "missing backend {expected}");
    }
}

#[test]
fn semantic_ensure_returns_web_framework_backend_binding() {
    let temp = tempdir().unwrap();
    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime
        .semantic_ensure(
            &view,
            SemanticEnsureInput {
                language: String::from("astro"),
                action: Some(String::from("warm")),
            },
        )
        .unwrap();

    assert_eq!(output.backend.language, "astro");
    assert_eq!(output.backend.provider.as_deref(), Some("phase1-web:astro"));
    assert_eq!(output.backend.state, "ready");
}

#[test]
fn semantic_status_detects_general_language_profiles() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/main.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.cpp"),
        "int main() { return 0; }\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/Main.java"), "class Main {}\n").unwrap();
    fs::write(temp.path().join("src/Main.kt"), "fun main() = Unit\n").unwrap();
    fs::write(temp.path().join("src/main.py"), "print('ok')\n").unwrap();
    fs::write(
        temp.path().join("src/main.go"),
        "package main\nfunc main() {}\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp.path().join("src/main.rb"), "puts 'ok'\n").unwrap();
    fs::write(temp.path().join("src/main.swift"), "print(\"ok\")\n").unwrap();
    fs::write(temp.path().join("src/Program.cs"), "class Program {}\n").unwrap();
    fs::write(temp.path().join("src/Script.fsx"), "printfn \"ok\"\n").unwrap();
    fs::write(temp.path().join("src/App.vb"), "Module App\nEnd Module\n").unwrap();
    fs::write(
        temp.path().join("src/Main.scala"),
        "object Main extends App\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/Main.hs"), "main = print \"ok\"\n").unwrap();
    fs::write(
        temp.path().join("src/main.ml"),
        "let () = print_endline \"ok\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.pas"), "begin end.\n").unwrap();
    fs::write(temp.path().join("src/main.d"), "void main() {}\n").unwrap();
    fs::write(
        temp.path().join("src/main.m"),
        "@implementation App\n@end\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.mm"),
        "@implementation App\n@end\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.php"), "<?php echo 'ok';\n").unwrap();
    fs::write(temp.path().join("src/main.pl"), "print qq(ok\\n);\n").unwrap();
    fs::write(temp.path().join("src/main.lua"), "print('ok')\n").unwrap();
    fs::write(temp.path().join("src/build.sh"), "echo ok\n").unwrap();
    fs::write(temp.path().join("src/main.asm"), "global _start\n").unwrap();
    fs::write(
        temp.path().join("src/main.f90"),
        "program main\nend program main\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.scm"), "(display \"ok\")\n").unwrap();
    fs::write(
        temp.path().join("src/main.adb"),
        "procedure Main is begin null; end Main;\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.awk"), "{ print $0 }\n").unwrap();
    fs::write(temp.path().join("src/main.tcl"), "puts ok\n").unwrap();
    fs::write(temp.path().join("src/main.r"), "print('ok')\n").unwrap();
    fs::write(temp.path().join("src/main.jl"), "println(\"ok\")\n").unwrap();
    fs::write(temp.path().join("src/main.clj"), "(println \"ok\")\n").unwrap();
    fs::write(temp.path().join("src/main.lisp"), "(print \"ok\")\n").unwrap();
    fs::write(temp.path().join("src/main.erl"), "main() -> ok.\n").unwrap();
    fs::write(temp.path().join("src/main.exs"), "IO.puts(\"ok\")\n").unwrap();
    fs::write(temp.path().join("src/main.dart"), "void main() {}\n").unwrap();
    fs::write(temp.path().join("src/main.nim"), "echo \"ok\"\n").unwrap();
    fs::write(temp.path().join("src/main.pro"), "main :- true.\n").unwrap();
    fs::write(
        temp.path().join("src/design.sv"),
        "module design; endmodule\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/Main.hx"), "class Main {}\n").unwrap();
    fs::write(temp.path().join("src/main.bas"), "PRINT \"ok\"\n").unwrap();
    fs::write(temp.path().join("CMakeLists.txt"), "project(sample)\n").unwrap();
    fs::write(temp.path().join("pom.xml"), "<project />\n").unwrap();
    fs::write(temp.path().join("build.gradle.kts"), "plugins { java }\n").unwrap();
    fs::write(
        temp.path().join("pyproject.toml"),
        "[project]\nname = 'sample'\n",
    )
    .unwrap();
    fs::write(temp.path().join("go.mod"), "module example.com/sample\n").unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'sample'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(temp.path().join("sample.csproj"), "<Project />\n").unwrap();
    fs::write(temp.path().join("sample.fsproj"), "<Project />\n").unwrap();
    fs::write(temp.path().join("sample.vbproj"), "<Project />\n").unwrap();
    fs::write(
        temp.path().join("build.sbt"),
        "scalaVersion := \"2.13.14\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("Package.swift"),
        "// swift-tools-version: 5.10\n",
    )
    .unwrap();
    fs::write(temp.path().join("sample.cabal"), "name: sample\n").unwrap();
    fs::write(temp.path().join("stack.yaml"), "resolver: lts-22.0\n").unwrap();
    fs::write(temp.path().join("dune-project"), "(lang dune 3.11)\n").unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let output = runtime.semantic_status(&view, None).unwrap();
    let languages = output
        .backends
        .into_iter()
        .map(|backend| backend.language)
        .collect::<BTreeSet<_>>();

    for expected in [
        "c-native",
        "cpp-native",
        "java-jvm",
        "java-maven",
        "java-gradle",
        "kotlin-jvm",
        "pyproject-python",
        "go-module",
        "cargo-rust",
        "ruby",
        "swift-package",
        "dotnet-csharp",
        "dotnet-fsharp",
        "dotnet-vbnet",
        "scala-sbt",
        "haskell-cabal",
        "haskell-stack",
        "ocaml-opam",
        "pascal",
        "d",
        "php",
        "perl",
        "lua",
        "bash",
        "assembly",
        "objective-c",
        "objective-cpp",
        "fortran",
        "scheme",
        "ada",
        "awk",
        "tcl",
        "r",
        "julia",
        "clojure",
        "common-lisp",
        "erlang",
        "elixir",
        "dart",
        "nim",
        "prolog",
        "systemverilog",
        "haxe",
        "freebasic",
    ] {
        assert!(languages.contains(expected), "missing backend {expected}");
    }
}

#[test]
fn semantic_status_tracks_leaf_backends_and_actions() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{
  "dependencies": {
    "react": "18.3.0",
    "react-dom": "18.3.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("tsconfig.json"),
        "{\n  \"compilerOptions\": {}\n}\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/App.tsx"),
        "export function App() { return <main />; }\n",
    )
    .unwrap();

    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    let initial = runtime.semantic_status(&view, None).unwrap();
    assert_eq!(initial.backends.len(), 1);
    assert_eq!(initial.backends[0].language, "react-ts");
    assert_eq!(initial.backends[0].state, "off");

    let warmed = runtime
        .semantic_ensure(
            &view,
            SemanticEnsureInput {
                language: String::from("react-ts"),
                action: Some(String::from("warm")),
            },
        )
        .unwrap();
    assert_eq!(warmed.backend.language, "react-ts");
    assert_eq!(warmed.backend.state, "ready");

    let after_warm = runtime.semantic_status(&view, None).unwrap();
    assert_eq!(after_warm.backends.len(), 1);
    assert_eq!(after_warm.backends[0].language, "react-ts");
    assert_eq!(after_warm.backends[0].state, "ready");

    let stopped = runtime
        .semantic_ensure(
            &view,
            SemanticEnsureInput {
                language: String::from("react-ts"),
                action: Some(String::from("stop")),
            },
        )
        .unwrap();
    assert_eq!(stopped.backend.state, "off");

    let after_stop = runtime
        .semantic_status(&view, Some(String::from("react-ts")))
        .unwrap();
    assert_eq!(after_stop.backends.len(), 1);
    assert_eq!(after_stop.backends[0].state, "off");
}

#[test]
fn semantic_ensure_supports_general_language_profiles() {
    let temp = tempdir().unwrap();
    let runtime = Runtime::new(Default::default());
    let view = runtime
        .upsert_view("session_test", &[file_uri_from_path(temp.path())])
        .unwrap();

    for language in [
        "c",
        "c-native",
        "cpp",
        "cpp-native",
        "java",
        "java-jvm",
        "java-maven",
        "java-gradle",
        "kotlin",
        "kotlin-jvm",
        "python",
        "pyproject-python",
        "go",
        "go-module",
        "rust",
        "cargo-rust",
        "ruby",
        "swift",
        "swift-package",
        "csharp",
        "dotnet-csharp",
        "fsharp",
        "dotnet-fsharp",
        "vbnet",
        "dotnet-vbnet",
        "scala",
        "scala-sbt",
        "haskell",
        "haskell-cabal",
        "haskell-stack",
        "ocaml",
        "ocaml-opam",
        "d",
        "php",
        "pascal",
        "lua",
        "perl",
        "shell",
        "bash",
        "assembly",
        "objective-c",
        "objective-cpp",
        "fortran",
        "scheme",
        "ada",
        "awk",
        "tcl",
        "r",
        "julia",
        "clojure",
        "common-lisp",
        "erlang",
        "elixir",
        "dart",
        "nim",
        "prolog",
        "freebasic",
        "haxe",
        "systemverilog",
    ] {
        let output = runtime
            .semantic_ensure(
                &view,
                SemanticEnsureInput {
                    language: language.to_string(),
                    action: Some(String::from("warm")),
                },
            )
            .unwrap();
        let expected_provider = format!("phase1-general:{language}");

        assert_eq!(output.backend.language, language);
        assert_eq!(
            output.backend.provider.as_deref(),
            Some(expected_provider.as_str())
        );
        assert_eq!(output.backend.state, "ready");
    }
}
