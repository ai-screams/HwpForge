use super::*;

/// The committed synthetic family, as its `name` table declares it.
pub(super) const TEST_FACE: &str = "HwpForge Test";

/// The fixture's path, relative to the workspace root.
pub(super) const FIXTURE: &str = "crates/hwpforge-bindings-py/tests/fixtures/synthetic_face.hwpx";

pub(super) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}
