#![allow(dead_code)]

#[path = "fixtures.rs"]
pub(super) mod fixtures;

use std::path::PathBuf;

/// The committed synthetic family, as its `name` table declares it.
pub(super) const TEST_FACE: &str = "HwpForge Test";

/// The fixture's path, relative to the workspace root.
pub(super) const FIXTURE: &str = "crates/hwpforge-bindings-py/tests/fixtures/synthetic_face.hwpx";

pub(super) fn workspace_root() -> PathBuf {
    fixtures::workspace_root()
}
