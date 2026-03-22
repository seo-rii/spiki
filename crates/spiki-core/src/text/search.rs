use std::path::Path;

use regex::RegexBuilder;

use crate::model::{Position, Range, SearchMode, TextMatch};
use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::types::LoadedTextFile;

pub fn search_file(
    _path: &Path,
    uri: &str,
    file: &LoadedTextFile,
    query: &str,
    mode: SearchMode,
    case_sensitive: bool,
    context_lines: u32,
    limit: usize,
) -> SpikiResult<Vec<TextMatch>> {
    let lines: Vec<&str> = file.text.split_inclusive('\n').collect();
    let mut matches = Vec::new();
    if limit == 0 {
        return Ok(matches);
    }

    match mode {
        SearchMode::Regex => {
            let regex = RegexBuilder::new(query)
                .case_insensitive(!case_sensitive)
                .build()
                .map_err(|error| {
                    spiki_error(
                        SpikiCode::InvalidRequest,
                        format!("Invalid regex query: {error}"),
                    )
                })?;
            for (line_index, line) in lines.iter().enumerate() {
                for capture in regex.find_iter(line) {
                    matches.push(TextMatch {
                        uri: uri.to_string(),
                        range: Range {
                            start: Position {
                                line: line_index as u32,
                                character: line[..capture.start()].chars().count() as u32,
                            },
                            end: Position {
                                line: line_index as u32,
                                character: line[..capture.end()].chars().count() as u32,
                            },
                        },
                        snippet: context_snippet(&lines, line_index, context_lines),
                        score: None,
                        fingerprint: None,
                    });
                    if matches.len() >= limit {
                        return Ok(matches);
                    }
                }
            }
        }
        SearchMode::Literal | SearchMode::Word => {
            let pattern = if mode == SearchMode::Word {
                format!(r"\b{}\b", regex::escape(query))
            } else {
                regex::escape(query)
            };
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(!case_sensitive)
                .build()
                .map_err(|error| {
                    spiki_error(
                        SpikiCode::InvalidRequest,
                        format!(
                            "Invalid {} query: {error}",
                            if mode == SearchMode::Word {
                                "word"
                            } else {
                                "literal"
                            }
                        ),
                    )
                })?;
            for (line_index, line) in lines.iter().enumerate() {
                for capture in regex.find_iter(line) {
                    matches.push(TextMatch {
                        uri: uri.to_string(),
                        range: Range {
                            start: Position {
                                line: line_index as u32,
                                character: line[..capture.start()].chars().count() as u32,
                            },
                            end: Position {
                                line: line_index as u32,
                                character: line[..capture.end()].chars().count() as u32,
                            },
                        },
                        snippet: context_snippet(&lines, line_index, context_lines),
                        score: None,
                        fingerprint: None,
                    });
                    if matches.len() >= limit {
                        return Ok(matches);
                    }
                }
            }
        }
    }

    Ok(matches)
}

fn context_snippet(lines: &[&str], line_index: usize, context_lines: u32) -> String {
    let start = line_index.saturating_sub(context_lines as usize);
    let end = (line_index + context_lines as usize + 1).min(lines.len());
    lines[start..end].join("")
}
