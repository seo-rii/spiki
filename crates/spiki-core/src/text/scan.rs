use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;

use crate::model::{Scope, Warning};
use crate::runtime::{spiki_error, SpikiCode, SpikiResult};

use super::paths::{ensure_path_in_roots, path_from_file_uri};
use super::types::{CanonicalRoot, KnownFile, ScanOptions, ScanResult};

pub fn scan_workspace(
    roots: &[CanonicalRoot],
    scope: Option<&Scope>,
    options: ScanOptions,
) -> SpikiResult<ScanResult> {
    let mut files = Vec::new();
    let mut known_files = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let exclude_globs = build_excludes(scope)?;
    let root_ignores = build_root_ignores(roots, &mut warnings)?;
    let scope_targets = resolve_scope_targets(roots, scope)?;

    for target in scope_targets {
        let mut builder = WalkBuilder::new(&target);
        builder.standard_filters(!options.include_ignored);
        builder.hidden(false);

        for entry in builder.build() {
            let entry = match entry {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(Warning {
                        code: String::from("WALK_ERROR"),
                        message: error.to_string(),
                        severity: Some(String::from("warning")),
                    });
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Some(value) => value,
                None => continue,
            };
            if !file_type.is_file() {
                continue;
            }
            let canonical = match ensure_path_in_roots(path, roots) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if !seen.insert(canonical.clone()) {
                continue;
            }
            if is_default_excluded(&canonical) {
                continue;
            }
            if exclude_globs
                .as_ref()
                .is_some_and(|set| set.is_match(&canonical))
            {
                continue;
            }
            if !options.include_ignored && ignored_by_root_matchers(&canonical, &root_ignores) {
                continue;
            }
            if !options.include_generated && is_generated_candidate(&canonical) {
                continue;
            }
            let metadata = match fs::metadata(&canonical) {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(Warning {
                        code: String::from("STAT_ERROR"),
                        message: format!("Failed to stat {}: {error}", canonical.display()),
                        severity: Some(String::from("warning")),
                    });
                    continue;
                }
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_millis() as u64)
                .unwrap_or(0);
            if metadata.len() > options.max_index_file_size_bytes {
                warnings.push(Warning {
                    code: String::from("FILE_TOO_LARGE"),
                    message: format!("Skipped large file {}", canonical.display()),
                    severity: Some(String::from("info")),
                });
                continue;
            }
            files.push(canonical.clone());
            known_files.push((
                canonical,
                KnownFile {
                    size: metadata.len(),
                    mtime_ms: modified,
                },
            ));
        }
    }

    files.sort();
    known_files.sort_by(|left, right| left.0.cmp(&right.0));

    Ok(ScanResult {
        files,
        known_files,
        warnings,
    })
}

fn resolve_scope_targets(
    roots: &[CanonicalRoot],
    scope: Option<&Scope>,
) -> SpikiResult<Vec<PathBuf>> {
    let mut targets = Vec::new();

    if let Some(scope) = scope {
        if let Some(uris) = &scope.uris {
            for uri in uris {
                let path = path_from_file_uri(uri)?;
                targets.push(ensure_path_in_roots(&path, roots)?);
            }
        }
    }

    if targets.is_empty() {
        targets.extend(roots.iter().map(|root| root.path.clone()));
    }

    targets.sort();
    targets.dedup();
    Ok(targets)
}

fn build_root_ignores(
    roots: &[CanonicalRoot],
    warnings: &mut Vec<Warning>,
) -> SpikiResult<Vec<(PathBuf, Gitignore)>> {
    let mut matchers = Vec::new();

    for root in roots {
        let mut builder = GitignoreBuilder::new(&root.path);
        for ignore_name in [".gitignore", ".ignore", ".fdignore"] {
            let ignore_path = root.path.join(ignore_name);
            if ignore_path.is_file() {
                if let Some(error) = builder.add(&ignore_path) {
                    warnings.push(Warning {
                        code: String::from("IGNORE_PARSE_ERROR"),
                        message: error.to_string(),
                        severity: Some(String::from("warning")),
                    });
                }
            }
        }
        match builder.build() {
            Ok(matcher) if !matcher.is_empty() => matchers.push((root.path.clone(), matcher)),
            Ok(_) => {}
            Err(error) => {
                warnings.push(Warning {
                    code: String::from("IGNORE_BUILD_ERROR"),
                    message: error.to_string(),
                    severity: Some(String::from("warning")),
                });
            }
        }
    }

    Ok(matchers)
}

fn ignored_by_root_matchers(path: &Path, matchers: &[(PathBuf, Gitignore)]) -> bool {
    for (root, matcher) in matchers {
        if path.starts_with(root) && matcher.matched_path_or_any_parents(path, false).is_ignore() {
            return true;
        }
    }

    false
}

fn build_excludes(scope: Option<&Scope>) -> SpikiResult<Option<GlobSet>> {
    let Some(scope) = scope else {
        return Ok(None);
    };
    let Some(patterns) = &scope.exclude_globs else {
        return Ok(None);
    };

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            spiki_error(
                SpikiCode::InvalidRequest,
                format!("Invalid exclude glob {pattern}: {error}"),
            )
        })?);
    }
    Ok(Some(builder.build().map_err(|error| {
        spiki_error(
            SpikiCode::InvalidRequest,
            format!("Failed to build exclude globs: {error}"),
        )
    })?))
}

fn is_default_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git")
                | Some("node_modules")
                | Some("vendor")
                | Some("dist")
                | Some("build")
                | Some("target")
                | Some(".next")
                | Some(".turbo")
                | Some(".cache")
                | Some("coverage")
        )
    })
}

fn is_generated_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    name.contains(".generated.")
        || name.ends_with(".min.js")
        || name.ends_with(".gen.ts")
        || name.ends_with(".pb.go")
}
