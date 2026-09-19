//! `ops::style::restyle` — a regenerating edit, and therefore fail-closed.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::style::{restyle, RestyleOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// A store with every language slot filled, which is what the encoder needs
/// before it will write a font table.
fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

/// A document whose footnote body starts with a heading.
///
/// The encoder has no safe place to put the note's number head there, so it
/// reports `NoteHeadSkipped` — a semantic loss. This is the same
/// construction `hwpforge-smithy-hwpx`'s `note_numbering_roundtrip` tests
/// use for the stamper's fail-closed case.
fn document_that_loses_a_note_head() -> Vec<u8> {
    let mut heading_body = Paragraph::with_runs(
        vec![Run::text("제목 각주", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    heading_body.heading_level = Some(1);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode")
}

#[test]
fn applies_the_preset_font_and_keeps_the_document_readable() {
    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("modern"))
            .expect("restyle");

    assert_eq!(out.preset, "modern");
    let decoded = HwpxDecoder::decode(&out.bytes).expect("the output must decode");
    assert!(!decoded.document.sections().is_empty());
    assert!(
        decoded.style_store.iter_fonts().any(|f| f.face_name == "맑은 고딕"),
        "the modern preset's base font must reach the font table"
    );
}

#[test]
fn shape_indices_survive_so_the_document_is_not_restyled_into_nonsense() {
    // The store is kept and only face names are swapped; a fresh preset
    // store would renumber char/para shapes the document still references.
    let source = fixture("SimpleTable.hwpx");
    let before = HwpxDecoder::decode(&source).expect("decode source");

    let out = restyle(&source, &RestyleOptions::default().with_preset("classic")).expect("restyle");
    let after = HwpxDecoder::decode(&out.bytes).expect("decode output");

    assert_eq!(before.style_store.char_shape_count(), after.style_store.char_shape_count());
    assert_eq!(before.style_store.para_shape_count(), after.style_store.para_shape_count());
}

#[test]
fn an_unknown_preset_is_refused_before_anything_is_decoded() {
    // Bad bytes *and* a bad preset: reporting `PRESET_NOT_FOUND` proves the
    // lookup happens first, so a typo never costs a decode.
    let err = restyle(b"not an hwpx", &RestyleOptions::default().with_preset("neo"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
    assert!(err.to_string().contains("neo"), "{err}");
    assert!(err.hint().is_some(), "the CLI hint points at `templates`");
}

#[test]
fn a_caller_who_forgets_the_preset_gets_an_error_not_a_house_style() {
    let err =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = restyle(b"not a zip at all", &RestyleOptions::default().with_preset("modern"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn semantic_loss_returns_no_bytes_at_all() {
    // Restyling regenerates the whole package. If the encoder could not
    // place a footnote's number head, the output means something different
    // from the input, so the caller must get an error rather than a
    // plausible-looking file.
    let err = restyle(
        &document_that_loses_a_note_head(),
        &RestyleOptions::default().with_preset("modern"),
    )
    .expect_err("a lost note head must not pass silently");

    assert_eq!(err.code(), OpsCode::EncodeSemanticLoss, "{err}");
    let hwpforge::ops::OpsError::EncodeSemanticLoss { warnings, others } = &err else {
        panic!("wrong variant: {err:?}");
    };
    assert!(!warnings.is_empty(), "the refusal must say what was lost");
    assert!(warnings.iter().all(|w| w.code == "NOTE_HEAD_SKIPPED"), "{warnings:?}");
    assert!(
        !others.iter().any(|w| w.code == "NOTE_HEAD_SKIPPED"),
        "semantic losses must not also appear in `others`: {others:?}"
    );
}

#[test]
fn a_document_without_fonts_has_nothing_to_rebind() {
    // A store with shapes but no font table. If the encoder accepts it, the
    // package decodes into a document `restyle` cannot act on, which is the
    // one input `NoFonts` exists for.
    let mut store = HwpxStyleStore::new();
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("글꼴 없음", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new())
        .expect("a fontless store must still encode for this branch to be reachable");
    assert!(
        HwpxDecoder::decode(&bytes).expect("decode").style_store.iter_fonts().next().is_none(),
        "the package must really come back without a font table"
    );

    let err = restyle(&bytes, &RestyleOptions::default().with_preset("modern"))
        .expect_err("there is no base font to replace");

    assert_eq!(err.code(), OpsCode::NoFonts, "{err}");
    assert!(err.hint().is_some(), "the CLI hint points at `validate`");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("latest"))
            .expect("restyle");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    let mut fields: Vec<&str> =
        value.as_object().expect("object").keys().map(String::as_str).collect();
    fields.sort_unstable();
    // Actual: {"preset":"latest","sections":1,"paragraphs":2,"warnings":[]}
    assert_eq!(fields, ["paragraphs", "preset", "sections", "warnings"]);
    assert_eq!(value["preset"], "latest");
    assert!(value["warnings"].is_array());
}

#[test]
fn meta_deserializes_json_written_before_sections_and_paragraphs_existed() {
    // The exact key set hwpforge 0.16.5's `RestyleMeta` wrote — pinned by
    // `meta_carries_exactly_the_preset_and_warnings_keys` at main@350851f,
    // before this review fix (`git show 350851f:crates/hwpforge/tests/ops_restyle.rs`).
    let old_json = r#"{"preset":"modern","warnings":[]}"#;

    let meta: hwpforge::ops::RestyleMeta =
        serde_json::from_str(old_json).expect("older-writer JSON must still deserialize");

    assert_eq!(meta.preset, "modern");
    assert_eq!(meta.sections, 0, "no `sections` key in older JSON — must default, not fail");
    assert_eq!(meta.paragraphs, 0, "no `paragraphs` key in older JSON — must default, not fail");
}

#[test]
fn sections_and_paragraphs_match_an_inspect_of_the_output_without_a_second_decode() {
    // `restyle` measures its own counts on the document it already
    // decoded, before re-encoding — this pins that measurement against an
    // independent `inspect` of the bytes it produced, so the two can never
    // silently drift.
    use hwpforge::ops::{inspect, InspectOptions};

    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("modern"))
            .expect("restyle");

    let inspected = inspect(&out.bytes, &InspectOptions::default()).expect("inspect the output");
    let top_level_paragraphs: usize =
        inspected.report.section_details.iter().map(|s| s.top_level_paragraphs).sum();

    assert_eq!(out.sections, inspected.report.sections);
    assert_eq!(out.paragraphs, top_level_paragraphs);
}

/// Anything reaching `warnings` is something the caller may ignore.
///
/// The loop below is only meaningful if it has something to iterate, so the
/// exact list is pinned first. It is empty, and that is a property of the
/// encoder rather than of this operation: the only non-semantic
/// `EncodeWarning` is `LayoutCacheDropped`, which the encoder raises solely
/// under `EncodeOptions::emit_layout_cache` — an opt-in no editing surface
/// sets. No committed fixture reaches it.
///
/// Pinning emptiness rather than waving at it means the day a fixture does
/// produce one, this test fails and the classification assertion below stops
/// being decorative. The non-empty half of the same contract is already
/// covered by `semantic_loss_returns_no_bytes_at_all`, where the refusal
/// carries both warning groups.
#[test]
fn every_returned_warning_is_non_semantic() {
    let out = restyle(&fixture("sample1.hwpx"), &RestyleOptions::default().with_preset("modern"))
        .expect("restyle");

    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert_eq!(codes, Vec::<String>::new(), "this encode raises nothing: {codes:?}");

    for code in &codes {
        assert_ne!(code, "NOTE_HEAD_SKIPPED", "{codes:?}");
        assert_ne!(code, "NOTE_RESTART_IGNORED", "{codes:?}");
        assert_ne!(code, "TITLE_MARK_SKIPPED", "{codes:?}");
    }
}
