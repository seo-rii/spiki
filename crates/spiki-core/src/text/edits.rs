use crate::model::TextEdit;
use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::spans::range_to_offsets;

pub fn apply_edits_to_text(
    original_text: &str,
    edits: &[TextEdit],
    line_ending: &str,
) -> SpikiResult<String> {
    let mut resolved = Vec::new();
    for edit in edits {
        let (start, end) = range_to_offsets(original_text, &edit.range)?;
        resolved.push((
            start,
            end,
            normalize_line_endings(&edit.new_text, line_ending),
        ));
    }
    resolved.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    for window in resolved.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if next.0 < current.1 || (next.0 == current.0 && next.1 == current.1) {
            return Err(spiki_error(
                SpikiCode::InvalidRequest,
                String::from("Overlapping or duplicate edits are not allowed"),
            ));
        }
    }

    resolved.sort_by(|left, right| right.0.cmp(&left.0));

    let mut next = original_text.to_string();
    for (start, end, replacement) in resolved {
        next.replace_range(start..end, &replacement);
    }

    Ok(next)
}

fn normalize_line_endings(text: &str, line_ending: &str) -> String {
    if line_ending == "crlf" {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.replace("\r\n", "\n")
    }
}
