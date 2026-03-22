use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use blake3::Hasher;
use encoding_rs::{UTF_16BE, UTF_16LE};

use crate::model::FileFingerprint;
use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::paths::file_uri_from_path;
use super::types::LoadedTextFile;

pub fn read_text_file(path: &Path) -> SpikiResult<LoadedTextFile> {
    let bytes = fs::read(path).map_err(|error| {
        spiki_error(
            SpikiCode::Internal,
            format!("Failed to read {}: {error}", path.display()),
        )
    })?;
    let metadata = fs::metadata(path).map_err(|error| {
        spiki_error(
            SpikiCode::Internal,
            format!("Failed to stat {}: {error}", path.display()),
        )
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    let (text, encoding) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (
            String::from_utf8(bytes[3..].to_vec()).map_err(|_| {
                spiki_error(
                    SpikiCode::Unsupported,
                    format!("{} is not valid UTF-8", path.display()),
                )
            })?,
            String::from("utf-8-bom"),
        )
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _, had_errors) = UTF_16LE.decode(&bytes[2..]);
        if had_errors {
            return Err(spiki_error(
                SpikiCode::Unsupported,
                format!("{} is not valid UTF-16LE", path.display()),
            ));
        }
        (decoded.into_owned(), String::from("utf-16le"))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _, had_errors) = UTF_16BE.decode(&bytes[2..]);
        if had_errors {
            return Err(spiki_error(
                SpikiCode::Unsupported,
                format!("{} is not valid UTF-16BE", path.display()),
            ));
        }
        (decoded.into_owned(), String::from("utf-16be"))
    } else {
        (
            String::from_utf8(bytes.clone()).map_err(|_| {
                spiki_error(
                    SpikiCode::Unsupported,
                    format!("{} is not valid UTF-8 text", path.display()),
                )
            })?,
            String::from("utf-8"),
        )
    };
    let mut hasher = Hasher::new();
    hasher.update(text.as_bytes());
    let line_ending = if text.contains("\r\n") { "crlf" } else { "lf" };

    Ok(LoadedTextFile {
        text,
        encoding,
        line_ending: String::from(line_ending),
        size: metadata.len(),
        mtime_ms: modified,
        content_hash: hasher.finalize().to_hex().to_string(),
    })
}

pub fn fingerprint_for_file(path: &Path, file: &LoadedTextFile) -> FileFingerprint {
    FileFingerprint {
        uri: file_uri_from_path(path),
        content_hash: file.content_hash.clone(),
        size: file.size,
        mtime_ms: file.mtime_ms,
        line_ending: file.line_ending.clone(),
        encoding: file.encoding.clone(),
    }
}
