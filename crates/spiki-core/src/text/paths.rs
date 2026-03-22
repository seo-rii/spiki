use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use url::Url;

use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::types::CanonicalRoot;

pub fn canonical_roots_from_uris(uris: &[String]) -> SpikiResult<Vec<CanonicalRoot>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();

    for uri in uris {
        let path = path_from_file_uri(uri)?;
        let canonical = fs::canonicalize(&path).map_err(|error| {
            spiki_error(
                SpikiCode::Forbidden,
                format!("Failed to canonicalize root {uri}: {error}"),
            )
        })?;

        if seen.insert(canonical.clone()) {
            roots.push(CanonicalRoot {
                uri: file_uri_from_path(&canonical),
                path: canonical,
            });
        }
    }

    if roots.is_empty() {
        return Err(spiki_error(
            SpikiCode::Forbidden,
            "At least one accessible file:// root is required",
        ));
    }

    roots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(roots)
}

pub fn path_from_file_uri(uri: &str) -> SpikiResult<PathBuf> {
    let parsed = Url::parse(uri).map_err(|error| {
        spiki_error(
            SpikiCode::InvalidRequest,
            format!("Invalid file URI {uri}: {error}"),
        )
    })?;

    if parsed.scheme() != "file" {
        return Err(spiki_error(
            SpikiCode::Forbidden,
            format!("Only file:// URIs are supported: {uri}"),
        ));
    }

    parsed.to_file_path().map_err(|_| {
        spiki_error(
            SpikiCode::InvalidRequest,
            format!("Invalid local file URI {uri}"),
        )
    })
}

pub fn file_uri_from_path(path: &Path) -> String {
    Url::from_file_path(path)
        .expect("absolute path must convert to file URI")
        .to_string()
}

pub fn ensure_path_in_roots(path: &Path, roots: &[CanonicalRoot]) -> SpikiResult<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        spiki_error(
            SpikiCode::Forbidden,
            format!("Failed to resolve path {}: {error}", path.display()),
        )
    })?;

    if roots.iter().any(|root| canonical.starts_with(&root.path)) {
        return Ok(canonical);
    }

    Err(spiki_error(
        SpikiCode::Forbidden,
        format!("Path {} is outside the active roots", path.display()),
    ))
}
