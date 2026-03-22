use std::path::Path;

use crate::model::{Position, Range, TextSpan};
use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::files::fingerprint_for_file;
use super::types::LoadedTextFile;

pub fn range_to_offsets(text: &str, range: &Range) -> SpikiResult<(usize, usize)> {
    let line_starts = line_starts(text);
    let start = position_to_offset(text, &line_starts, &range.start)?;
    let end = position_to_offset(text, &line_starts, &range.end)?;

    if end < start {
        return Err(spiki_error(
            SpikiCode::InvalidRequest,
            "Range end precedes range start",
        ));
    }

    Ok((start, end))
}

pub fn build_text_span(
    uri: &str,
    file: &LoadedTextFile,
    range: Range,
    context_lines: u32,
    path: &Path,
) -> SpikiResult<TextSpan> {
    let (start, end) = range_to_offsets(&file.text, &range)?;
    let lines: Vec<&str> = file.text.split_inclusive('\n').collect();
    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    let before_start = start_line.saturating_sub(context_lines as usize);
    let after_end = (end_line + context_lines as usize + 1).min(lines.len());
    let before = if before_start < start_line {
        Some(lines[before_start..start_line].join(""))
    } else {
        None
    };
    let after = if end_line + 1 < after_end {
        Some(lines[end_line + 1..after_end].join(""))
    } else {
        None
    };

    Ok(TextSpan {
        uri: uri.to_string(),
        range,
        before,
        text: file.text[start..end].to_string(),
        after,
        fingerprint: Some(fingerprint_for_file(path, file)),
    })
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn position_to_offset(
    text: &str,
    line_starts: &[usize],
    position: &Position,
) -> SpikiResult<usize> {
    let line = position.line as usize;
    if line > line_starts.len() {
        return Err(spiki_error(
            SpikiCode::InvalidRequest,
            format!("Line {} is out of range", position.line),
        ));
    }
    if line == line_starts.len() {
        if position.character == 0 {
            return Ok(text.len());
        }
        return Err(spiki_error(
            SpikiCode::InvalidRequest,
            format!("Line {} is out of range", position.line),
        ));
    }

    let start = line_starts[line];
    let end = if line + 1 < line_starts.len() {
        line_starts[line + 1]
    } else {
        text.len()
    };
    let slice = &text[start..end];
    let trimmed = slice
        .strip_suffix("\r\n")
        .or_else(|| slice.strip_suffix('\n'))
        .unwrap_or(slice);
    let mut chars = 0u32;
    for (byte_index, _) in trimmed.char_indices() {
        if chars == position.character {
            return Ok(start + byte_index);
        }
        chars += 1;
    }
    if chars == position.character {
        return Ok(start + trimmed.len());
    }

    Err(spiki_error(
        SpikiCode::InvalidRequest,
        format!(
            "Character {} is out of range for line {}",
            position.character, position.line
        ),
    ))
}
