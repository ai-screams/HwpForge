#[path = "support/ops_to_pdf.rs"]
mod shared;
use shared::*;

use std::path::{Path, PathBuf};

use hwpforge_convert::ops::{to_pdf, ConvertOpsWarning, ToPdfOptions};
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_smithy_pdf::font::FontDiscovery;

// ── dispatch ────────────────────────────────────────────────────

#[test]
fn bytes_that_are_neither_container_are_refused_before_any_work() {
    let error = to_pdf(b"%PDF-1.7 already rendered", &options()).expect_err("not a container");

    assert_eq!(error.code(), OpsCode::UnrecognizedFormat);
    assert_eq!(error.code().as_str(), "UNRECOGNIZED_FORMAT");
    assert!(error.cause().is_none(), "nothing reached the renderer");
    assert!(error.hint().is_some(), "the caller is told the format is sniffed, not guessed");

    // Empty input takes the same path rather than panicking on a short slice.
    assert_eq!(to_pdf(b"", &options()).expect_err("empty").code(), OpsCode::UnrecognizedFormat);
}

// ── the committed fixture pair ──────────────────────────────────

/// The document half of the pair the Python bindings render.
const PY_FIXTURE: &str = "crates/hwpforge-bindings-py/tests/fixtures/synthetic_face.hwpx";

/// The font half: one committed synthetic face, copied byte for byte from
/// smithy-pdf's own test fonts.
const PY_FONT_DIR: &str = "crates/hwpforge-bindings-py/tests/fixtures/fonts";

/// The one face in that directory, and the only face the document names.
const PY_FONT_FILE: &str = "HwpForgeTest-Regular.ttf";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

#[test]
fn the_committed_fixture_pair_renders_with_no_installed_font() {
    // The pair exists because a pytest suite cannot build fonts or rewrite
    // name tables at test time: both halves are on disk, and this is what
    // proves they still fit each other. The options are the ones the Python
    // lane passes — explicit discovery, fatal mode, no cache rejection — so a
    // regression here is a regression there.
    let bytes = std::fs::read(workspace_root().join(PY_FIXTURE)).expect("the committed document");
    let options = ToPdfOptions::default()
        .with_font_dirs(vec![workspace_root().join(PY_FONT_DIR)])
        .with_discovery(FontDiscovery::ExplicitOnly)
        .with_degraded(false)
        .with_partial_cache_reject(false);

    let output = to_pdf(&bytes, &options).expect("render");

    assert!(output.bytes.starts_with(b"%PDF-"), "PDF header");
    assert_eq!(output.pages, 1, "the whole document fits one page");
    assert!(
        output.warnings.is_empty(),
        "the pair renders clean — no degradation, no skip: {:?}",
        codes(&output.warnings)
    );

    // The report the Python layer hands back is exactly these two keys.
    let value = serde_json::to_value(output.meta()).expect("serialise meta");
    let mut keys: Vec<&String> = value.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["pages", "warnings"], "the FFI key set is a contract");
}

#[test]
fn the_committed_font_is_the_synthetic_face_unmodified() {
    // The copy is the whole point: if smithy-pdf regenerates its test fonts,
    // the pair must be refreshed rather than silently drifting.
    let committed = std::fs::read(workspace_root().join(PY_FONT_DIR).join(PY_FONT_FILE))
        .expect("the committed font");
    let source = std::fs::read(test_font_dir().join(PY_FONT_FILE)).expect("the source font");
    assert_eq!(committed, source, "re-copy {PY_FONT_FILE} from smithy-pdf's test fonts");

    // One face is all the directory holds — the document names exactly one.
    let entries: Vec<String> = std::fs::read_dir(workspace_root().join(PY_FONT_DIR))
        .expect("read the font directory")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".ttf"))
        .collect();
    assert_eq!(entries, vec![PY_FONT_FILE.to_string()]);
}
