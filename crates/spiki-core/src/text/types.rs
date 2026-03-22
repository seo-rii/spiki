use std::path::PathBuf;

use crate::model::Warning;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRoot {
    pub uri: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownFile {
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LoadedTextFile {
    pub text: String,
    pub encoding: String,
    pub line_ending: String,
    pub size: u64,
    pub mtime_ms: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub include_ignored: bool,
    pub include_generated: bool,
    pub include_default_excluded: bool,
    pub max_index_file_size_bytes: u64,
    pub default_exclude_components: Vec<String>,
    pub forced_exclude_components: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub files: Vec<PathBuf>,
    pub known_files: Vec<(PathBuf, KnownFile)>,
    pub warnings: Vec<Warning>,
}
