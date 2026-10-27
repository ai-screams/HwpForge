#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The workspace root, derived from this crate's manifest directory.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

/// The committed synthetic faces smithy-pdf renders its own e2e with.
pub(crate) fn test_font_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hwpforge-smithy-pdf/tests/fonts")
}
