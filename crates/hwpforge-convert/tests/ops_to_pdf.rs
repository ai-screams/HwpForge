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
//! The HWP5 leg cannot be retyped that way — its face names come out of the
//! binary, not out of a store this test can edit — so it is covered from the
//! other side: `with_declared_face` writes the committed synthetic font into a
//! temporary directory with its `name` table rewritten to declare the face the
//! fixture asks for. Exact-face resolution is untouched; the directory simply
//! contains a file that declares that name.
//! `hwp5_input_renders_end_to_end_on_any_checkout` is therefore CI-enforced,
//! and `an_undeclared_face_is_still_refused_rather_than_substituted` is its
//! guard. `hwp5_input_renders_end_to_end_when_hancom_fonts_present` keeps the
//! same path on the real Hancom faces and prints a `SKIPPED` line where they
//! are absent.

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

/// The face every Hancom-authored fixture in `pdf-rules/` asks for.
///
/// Measured, not assumed: converting `rules-bold.hwp` emits exactly one
/// `<hh:font face="…">` and it is this one. The renderer resolves faces
/// through the font file's own `name` table, so this is the name a font must
/// *declare* — not what its file is called.
const HWP5_FIXTURE_FACE: &str = "함초롬바탕";

/// A copy of `font` whose `name` table declares `face`.
///
/// The exact-face contract stays intact: this does not teach the resolver to
/// accept a different typeface, it hands it a file that genuinely declares the
/// name the document asks for — the deterministic equivalent of installing
/// that face. Only the `name` table is replaced (appended at the end, with the
/// table directory repointed); the glyphs, metrics and the `OS/2` table the
/// embedding-licence gate reads are the committed synthetic face's own.
fn with_declared_face(font: &[u8], face: &str) -> Vec<u8> {
    fn utf16be(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    // A minimal format-0 `name` table: family, subfamily, full name, all as
    // Windows/UCS-2/en-US records, which is what `ttf_parser` decodes.
    let entries: [(u16, Vec<u8>); 3] =
        [(1, utf16be(face)), (2, utf16be("Regular")), (4, utf16be(face))];
    let count = u16::try_from(entries.len()).expect("three records");
    let mut records = Vec::new();
    let mut storage = Vec::new();
    for (name_id, text) in &entries {
        records.extend_from_slice(&3u16.to_be_bytes()); // platform: Windows
        records.extend_from_slice(&1u16.to_be_bytes()); // encoding: UCS-2
        records.extend_from_slice(&0x0409u16.to_be_bytes()); // language: en-US
        records.extend_from_slice(&name_id.to_be_bytes());
        records.extend_from_slice(&u16::try_from(text.len()).expect("short name").to_be_bytes());
        records
            .extend_from_slice(&u16::try_from(storage.len()).expect("short table").to_be_bytes());
        storage.extend_from_slice(text);
    }
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    table.extend_from_slice(&count.to_be_bytes());
    table.extend_from_slice(&(6 + 12 * count).to_be_bytes()); // string storage offset
    table.extend_from_slice(&records);
    table.extend_from_slice(&storage);

    let num_tables = usize::from(u16::from_be_bytes([font[4], font[5]]));
    let mut out = font.to_vec();
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let offset = u32::try_from(out.len()).expect("font is small");
    let length = u32::try_from(table.len()).expect("table is small");
    out.extend_from_slice(&table);

    let mut repointed = false;
    for index in 0..num_tables {
        let record = 12 + 16 * index;
        if &font[record..record + 4] == b"name" {
            out[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
            out[record + 12..record + 16].copy_from_slice(&length.to_be_bytes());
            repointed = true;
        }
    }
    assert!(repointed, "the committed synthetic face has a name table");
    out
}

/// A font directory holding the synthetic face under the fixture's face name.
fn fixture_face_font_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hwp5-fixture-face");
    std::fs::create_dir_all(&dir).expect("create the font directory");
    let source = std::fs::read(test_font_dir().join("HwpForgeTest-Regular.ttf"))
        .expect("the committed synthetic face");
    std::fs::write(dir.join("fixture-face.ttf"), with_declared_face(&source, HWP5_FIXTURE_FACE))
        .expect("write the renamed face");
    dir
}

#[test]
fn hwp5_input_renders_end_to_end_on_any_checkout() {
    // The CI-enforced success path: HWP5 bytes in, a real PDF out, with no
    // machine-specific font installed. `hwp5_input_reaches_the_renderer_…`
    // only proves the pipeline *reaches* the renderer; this one proves the
    // renderer finishes.
    let output = to_pdf(
        &fixture("rules-bold.hwp"),
        &ToPdfOptions::default().with_font_dirs(vec![fixture_face_font_dir()]).with_degraded(true),
    )
    .expect("render");

    assert!(output.bytes.starts_with(b"%PDF-"), "PDF header");
    assert!(output.pages >= 1, "pages = {}", output.pages);
    assert!(output.bytes.len() > 1_000, "real content ({} bytes)", output.bytes.len());

    // The HWP5 leg really ran: the input is an OLE2 container, and the only
    // route from OLE2 bytes to a PDF is the convert leg. (This fixture
    // converts cleanly, so there is no "convert"-stage warning to point at —
    // a silent leg, not an absent one.)
    assert!(
        fixture("rules-bold.hwp").starts_with(&[0xD0, 0xCF, 0x11, 0xE0]),
        "the fixture is HWP5, not an HWPX package with a .hwp name"
    );
    for warning in &output.warnings {
        assert!(matches!(warning.stage(), "convert" | "decode" | "render"), "{}", warning.stage());
    }
    // The degradation is named, never silent.
    assert!(
        codes(&output.warnings).iter().all(|code| code != "OTHER"),
        "{:?}",
        codes(&output.warnings)
    );
}

#[test]
fn an_undeclared_face_is_still_refused_rather_than_substituted() {
    // The guard on the test above: the synthetic face resolves *because* it
    // declares the fixture's name, not because the resolver gave up and picked
    // something. The same directory without the rename resolves nothing.
    let error = to_pdf(
        &fixture("rules-bold.hwp"),
        &ToPdfOptions::default().with_font_dirs(vec![test_font_dir()]).with_degraded(true),
    )
    .expect_err("no file declares the fixture's face");

    assert_eq!(error.code(), OpsCode::PdfRenderFailed);
    assert_eq!(error.cause(), Some(PdfErrorCode::FontUnresolved));
}

#[test]
fn hwp5_input_renders_end_to_end_when_hancom_fonts_present() {
    // fixture-optional: the real Hancom faces are the machine-specific half of
    // the pair, so this test is *skipped*, not passed, without them —
    // `hwp5_input_renders_end_to_end_on_any_checkout` is what CI enforces.
    if !Path::new(HANCOM_TTF_DIR).exists() {
        eprintln!(
            "SKIPPED hwp5_input_renders_end_to_end_when_hancom_fonts_present: \
             Hancom 폰트 번들 없음 ({HANCOM_TTF_DIR})"
        );
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
