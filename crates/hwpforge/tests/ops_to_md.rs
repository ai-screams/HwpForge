//! `ops::markdown::to_md` — three renderings, and what each one costs.
#![cfg(feature = "ops-md")]

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::markdown::{to_md, MdExportOptions, MdMode};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn export(name: &str, mode: MdMode) -> hwpforge::ops::markdown::MdExportOutput {
    to_md(&fixture(name), &MdExportOptions::default().with_mode(mode))
        .unwrap_or_else(|e| panic!("{name} as {}: {e}", mode.as_str()))
}

#[test]
fn styled_is_what_you_get_without_asking() {
    let implicit = to_md(&fixture("SimpleTable.hwpx"), &MdExportOptions::default()).expect("to_md");

    assert_eq!(implicit.mode, MdMode::Styled);
    assert_eq!(implicit.markdown, export("SimpleTable.hwpx", MdMode::Styled).markdown);
}

#[test]
fn all_three_modes_render_the_same_document() {
    for mode in [MdMode::Styled, MdMode::Lossy, MdMode::Lossless] {
        let out = export("SimpleTable.hwpx", mode);

        assert_eq!(out.mode, mode);
        assert!(!out.markdown.trim().is_empty(), "{} produced nothing", mode.as_str());
    }
}

#[test]
fn only_styled_extracts_images() {
    // The other two renderings have no syntax for an extracted binary, so
    // an image map from them would be bytes no reader could reach.
    let styled = export("SimplePicture.hwpx", MdMode::Styled);
    assert!(!styled.images.is_empty(), "the picture fixture must yield an image");

    for mode in [MdMode::Lossy, MdMode::Lossless] {
        assert!(export("SimplePicture.hwpx", mode).images.is_empty(), "{}", mode.as_str());
    }
}

#[test]
fn every_extracted_image_is_referenced_by_the_markdown_that_carries_it() {
    let out = export("SimplePicture.hwpx", MdMode::Styled);

    for key in out.images.keys() {
        assert!(
            out.markdown.contains(key.as_str()),
            "extracted `{key}` is unreachable from the text"
        );
        assert!(!out.images[key].is_empty(), "`{key}` came back empty");
    }
}

#[test]
fn lossless_carries_frontmatter_so_the_decoder_can_read_it_back() {
    let out = export("SimpleTable.hwpx", MdMode::Lossless);

    assert!(out.markdown.starts_with("---"), "{}", &out.markdown[..out.markdown.len().min(80)]);
}

#[test]
fn meta_carries_exactly_the_mode_images_and_warnings_keys() {
    let out = export("SimplePicture.hwpx", MdMode::Lossy);

    let value = serde_json::to_value(out.meta()).expect("serialise");
    let mut fields: Vec<&str> =
        value.as_object().expect("object").keys().map(String::as_str).collect();
    fields.sort_unstable();
    assert_eq!(fields, ["images", "mode", "warnings"]);
    assert_eq!(value["mode"], "lossy", "the meta reports the wire spelling, not the enum");
    assert!(value["images"].is_object());
    assert!(value["warnings"].is_array());
}

#[test]
fn meta_images_keep_their_keys_and_bytes() {
    let out = export("SimplePicture.hwpx", MdMode::Styled);
    let meta = out.meta();

    assert_eq!(
        meta.images.keys().collect::<Vec<_>>(),
        out.images.keys().collect::<Vec<_>>(),
        "the meta must not rename or reorder image keys"
    );
    for (key, bytes) in &out.images {
        assert_eq!(meta.images[key].as_ref(), bytes.as_slice(), "{key}");
    }
}

#[test]
fn meta_round_trips_through_json() {
    // `serde_bytes` must not be write-only: the Python stub and the pytest
    // contract test both read the shape back.
    let meta = export("SimplePicture.hwpx", MdMode::Styled).meta();

    let json = serde_json::to_string(&meta).expect("serialise");
    let back: hwpforge::ops::markdown::MdExportMeta =
        serde_json::from_str(&json).expect("deserialise");

    assert_eq!(back.mode, meta.mode);
    assert_eq!(back.images, meta.images);
}

/// A one-row table whose single cell spans two columns.
///
/// Plain GFM has no syntax for a span, so the lossy encoder flattens the
/// grid and says so; the styled encoder emits HTML and keeps the span.
fn document_with_a_merged_cell() -> Vec<u8> {
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
    use hwpforge::hwpx::HwpxEncoder;

    let cell_text = Paragraph::with_runs(
        vec![Run::text("합친 칸", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut wide = TableCell::new(vec![cell_text], HwpUnit::from_mm(80.0).expect("width"));
    wide.col_span = 2;
    let table = Table::new(vec![TableRow::new(vec![wide])]);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let mut store = HwpxStyleStore::new();
    for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode")
}

#[test]
fn lossy_reports_the_merged_cells_it_had_to_flatten() {
    // Warning-first: the plain grid silently loses the span otherwise.
    let bytes = document_with_a_merged_cell();

    let out = to_md(&bytes, &MdExportOptions::default().with_mode(MdMode::Lossy)).expect("to_md");

    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert!(codes.iter().any(|c| c == "TABLE_MERGE_FLATTENED"), "{codes:?}");
}

#[test]
fn styled_keeps_the_span_and_so_has_nothing_to_report() {
    let bytes = document_with_a_merged_cell();

    let out = to_md(&bytes, &MdExportOptions::default().with_mode(MdMode::Styled)).expect("to_md");

    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert!(!codes.iter().any(|c| c == "TABLE_MERGE_FLATTENED"), "{codes:?}");
    assert!(out.markdown.contains("colspan"), "the span must survive as HTML:\n{}", out.markdown);
}

#[test]
fn non_hwpx_input_is_a_decode_failure_in_every_mode() {
    for mode in [MdMode::Styled, MdMode::Lossy, MdMode::Lossless] {
        let err = to_md(b"not a zip at all", &MdExportOptions::default().with_mode(mode))
            .expect_err("must reject");

        assert_eq!(err.code(), OpsCode::DecodeFailed, "{}: {err}", mode.as_str());
    }
}

#[test]
fn an_unknown_mode_name_is_rejected_rather_than_defaulted() {
    let err = MdExportOptions::default().with_mode_name("styled ").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::InvalidInput, "{err}");
}
