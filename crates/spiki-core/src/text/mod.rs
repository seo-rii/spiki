mod edits;
mod files;
mod paths;
mod scan;
mod search;
mod spans;
mod types;

pub use edits::apply_edits_to_text;
pub use files::{fingerprint_for_file, read_text_file};
pub use paths::{
    canonical_roots_from_uris, ensure_path_in_roots, file_uri_from_path, path_from_file_uri,
};
pub use scan::{scan_workspace, set_scan_log_path_for_test};
pub use search::search_file;
pub use spans::{build_text_span, range_to_offsets};
pub use types::{CanonicalRoot, KnownFile, LoadedTextFile, ScanOptions, ScanResult};
