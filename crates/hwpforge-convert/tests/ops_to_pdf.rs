//! `ops::to_pdf` end-to-end.
//!
//! # Why the fixtures are retyped
//!
//! The renderer never substitutes a typeface it cannot find — an unresolved
//! font name is [`PdfErrorCode::FontUnresolved`], not a silent fallback —
//! so rendering a Hancom-authored fixture needs the Hancom font bundle,
//! which CI does not have. smithy-pdf solved the same problem with committed
//! synthetic faces (`tests/fonts/`, fixed metrics per script).
//!
//! These tests reuse that: a real Hancom-saved HWPX fixture is decoded, every
//! font face is renamed to the committed test face, and the document is
//! re-encoded with its layout cache. The wire path under test is unchanged —
//! `to_pdf` still sniffs, decodes, validates and renders real HWPX bytes —
//! and only the font names differ, so the whole suite runs on any checkout.
//!
//! One test is gated on the Hancom bundle and skips without it: the HWP5 leg
//! rendered all the way to a PDF. Its CI-runnable counterpart,
//! `hwp5_input_reaches_the_renderer_through_the_convert_leg`, proves the same
//! plumbing by showing the pipeline gets as far as the renderer.

use std::path::{Path, PathBuf};

use hwpforge_convert::ops::{to_pdf, ConvertOpsWarning, ToPdfOptions};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder};
use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::PdfErrorCode;

/// The committed synthetic face every retyped fixture is rendered with.
const TEST_FACE: &str = "HwpForge Test";

/// Where the Hancom bundle lives on macOS (smithy-pdf's own constant).
const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

fn fixture(name: &str) -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/pdf-rules").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// The committed synthetic faces smithy-pdf renders its own e2e with.
fn test_font_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hwpforge-smithy-pdf/tests/fonts")
}

/// The base options for a retyped fixture: the committed faces, degraded.
///
/// `degraded` is not incidental. The synthetic faces carry fixed metrics for
/// a small character set, so a Hancom-authored fixture asks them for glyphs
/// they do not have. Fatal mode refuses that outright rather than emit tofu;
/// degraded renders and says `MISSING_GLYPHS`. Both halves of that contract
/// are asserted in `degraded_renders_what_fatal_refuses`.
fn options() -> ToPdfOptions {
    fatal_options().with_degraded(true)
}

/// The same fonts with the default (fatal) failure mode.
fn fatal_options() -> ToPdfOptions {
    ToPdfOptions::default().with_font_dirs(vec![test_font_dir()])
}

/// Decodes an HWPX fixture, renames every font to [`TEST_FACE`], optionally
/// appends a paragraph with **no** layout cache, and re-encodes with the
/// cache emitted.
fn retyped(name: &str, append_cacheless: bool) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(&fixture(name)).expect("decode fixture");
    let mut store = decoded.style_store;
    let faces: Vec<String> = store.iter_fonts().map(|font| font.face_name.clone()).collect();
    for face in faces {
        if face != TEST_FACE {
            store.replace_font(&face, TEST_FACE);
        }
    }

    let mut document = decoded.document;
    if append_cacheless {
        let section = document.sections_mut().first_mut().expect("at least one section");
        section.add_paragraph(Paragraph::with_runs(
            vec![Run::text("캐시 없는 문단", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
    }

    let validated = document.validate().expect("validate retyped document");
    HwpxEncoder::encode_with_diagnostics(
        &validated,
        &store,
        &decoded.image_store,
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect("re-encode retyped document")
    .bytes
}

fn codes(warnings: &[ConvertOpsWarning]) -> Vec<String> {
    warnings.iter().map(|w| w.info().code).collect()
}

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

// ── HWPX input ──────────────────────────────────────────────────

#[test]
fn an_hwpx_fixture_renders_at_least_one_page() {
    let output = to_pdf(&retyped("rules-justify.hwpx", false), &options()).expect("render");

    assert!(output.bytes.starts_with(b"%PDF-"), "PDF header");
    assert!(output.pages >= 1, "pages = {}", output.pages);
    assert!(output.bytes.len() > 1_000, "real content ({} bytes)", output.bytes.len());

    // HWPX input never touches the convert leg, so no warning claims it.
    assert!(
        output.warnings.iter().all(|w| w.stage() != "convert"),
        "{:?}",
        codes(&output.warnings)
    );
    for warning in &output.warnings {
        assert!(matches!(warning.stage(), "decode" | "render"), "{}", warning.stage());
    }
}

#[test]
fn meta_carries_exactly_pages_and_warnings() {
    let output = to_pdf(&retyped("rules-justify.hwpx", false), &options()).expect("render");
    let meta = output.meta();

    assert_eq!(meta.pages, output.pages);
    assert_eq!(meta.warnings.len(), output.warnings.len());

    let value = serde_json::to_value(&meta).expect("serialise meta");
    let mut keys: Vec<&String> = value.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["pages", "warnings"], "the FFI key set is a contract");
}

#[test]
fn warnings_arrive_in_pipeline_order() {
    let output = to_pdf(&retyped("rules-justify.hwpx", true), &options()).expect("render");

    let stage_rank = |w: &ConvertOpsWarning| match w.stage() {
        "convert" => 0,
        "decode" => 1,
        "render" => 2,
        other => panic!("unknown stage {other}"),
    };
    let ranks: Vec<u8> = output.warnings.iter().map(stage_rank).collect();
    assert!(ranks.windows(2).all(|pair| pair[0] <= pair[1]), "{ranks:?} is not sorted by stage");
}

// ── failure mode ────────────────────────────────────────────────

#[test]
fn degraded_renders_what_fatal_refuses() {
    // One input, one difference: the toggle. The retyped fixture asks the
    // synthetic faces for glyphs they do not carry.
    let bytes = retyped("rules-justify.hwpx", false);

    let error = to_pdf(&bytes, &fatal_options()).expect_err("fatal refuses missing glyphs");
    assert_eq!(error.code(), OpsCode::PdfRenderFailed);
    assert_eq!(
        error.cause(),
        Some(PdfErrorCode::GlyphsUnavailable),
        "silent tofu is refused by default"
    );

    let output = to_pdf(&bytes, &options()).expect("degraded renders with a warning");
    assert!(output.pages >= 1);
    let render_codes = codes(&output.warnings);
    assert!(
        render_codes.iter().any(|code| code == "MISSING_GLYPHS"),
        "the degradation is named, not silent: {render_codes:?}"
    );
}

// ── partial-cache policy ────────────────────────────────────────

#[test]
fn a_cacheless_paragraph_is_skipped_with_a_render_warning() {
    // The appended paragraph has no layout cache, so the default policy skips
    // it and says so. This is the render-warning fixture the FFI contract
    // tests need.
    let output = to_pdf(&retyped("rules-justify.hwpx", true), &options()).expect("render");

    let render_codes: Vec<String> =
        output.warnings.iter().filter(|w| w.stage() == "render").map(|w| w.info().code).collect();
    assert!(!render_codes.is_empty(), "at least one render warning");
    assert!(render_codes.iter().any(|code| code == "PARAGRAPH_SKIPPED"), "{render_codes:?}");
    // Partial drift detection: every warning this fixture raises is classified.
    assert!(!render_codes.iter().any(|code| code == "OTHER"), "{render_codes:?}");

    // The skip names where it happened — corpus triage depends on it.
    let skipped = output
        .warnings
        .iter()
        .find(|w| w.info().code == "PARAGRAPH_SKIPPED")
        .expect("the skip")
        .info();
    assert!(skipped.message.contains("paragraph without layout cache skipped"));
    assert!(skipped.message.contains(':'), "the location is prefixed: {}", skipped.message);
}

#[test]
fn partial_cache_reject_turns_that_skip_into_a_refusal() {
    let bytes = retyped("rules-justify.hwpx", true);

    // Same bytes, same fonts — only the toggle differs.
    to_pdf(&bytes, &options()).expect("skipping policy renders");

    let error = to_pdf(&bytes, &options().with_partial_cache_reject(true))
        .expect_err("rejecting policy refuses");
    assert_eq!(error.code(), OpsCode::PdfRenderFailed);
    assert_eq!(error.cause(), Some(PdfErrorCode::MissingLayoutCache));
    assert!(error.hint().is_some());
}

// ── HWP5 input ──────────────────────────────────────────────────

#[test]
fn hwp5_input_reaches_the_renderer_through_the_convert_leg() {
    // No font directory, so the render itself cannot succeed. That is the
    // point: reaching a *render*-stage failure proves the OLE2 sniff, the
    // HWP5 → HWPX conversion, the decode and the validation all ran. A
    // failure anywhere earlier would report a different code.
    let error = to_pdf(&fixture("rules-bold.hwp"), &ToPdfOptions::default())
        .expect_err("no fonts are available");

    assert_eq!(error.code(), OpsCode::PdfRenderFailed, "{error}");
    assert_eq!(
        error.cause(),
        Some(PdfErrorCode::FontUnresolved),
        "the pipeline got as far as resolving fonts"
    );
}

#[test]
fn hwp5_input_renders_end_to_end_with_the_hancom_bundle() {
    // fixture-optional: needs the Hancom font bundle, which CI does not have.
    if !Path::new(HANCOM_TTF_DIR).exists() {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    }

    let output = to_pdf(
        &fixture("rules-bold.hwp"),
        &ToPdfOptions::default().with_discovery(FontDiscovery::HancomBundle).with_degraded(true),
    )
    .expect("render");

    assert!(output.bytes.starts_with(b"%PDF-"));
    assert!(output.pages >= 1);
    // The HWP5 leg reports through stage "convert"; the other two stages are
    // the same as for HWPX input.
    for warning in &output.warnings {
        assert!(matches!(warning.stage(), "convert" | "decode" | "render"), "{}", warning.stage());
    }
}
