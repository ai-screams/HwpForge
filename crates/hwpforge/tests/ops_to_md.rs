//! `ops::markdown::to_md` — three renderings, and what each one costs.
#![cfg(feature = "ops-md")]

#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::hwpx_fixture as fixture;
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::markdown::{to_md, MdExportOptions, MdMode};

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

// 이것을 실패시키는 것: CHAR 0·오른쪽/위/아래 여백·CHAR 탭의 MD 손실을 숨기기.
#[test]
fn w2_all_modes_report_margin_units_and_char_tab_loss() {
    use hwpforge::ops::{
        convert_md, from_json, to_json, ConvertMdOptions, FromJsonOptions, ToJsonOptions,
    };
    use serde_json::json;

    let base = convert_md("본문", None, &ConvertMdOptions::default()).unwrap().bytes;
    let mut value =
        serde_json::to_value(to_json(&base, &ToJsonOptions::default()).unwrap().exported).unwrap();
    let index = value["document"]["sections"][0]["paragraphs"][0]["para_shape_id"].as_u64().unwrap()
        as usize;
    for field in ["margin_left", "margin_right", "indent", "spacing_before", "spacing_after"] {
        value["styles"]["para_shapes"][index][field] = json!({"unit":"char", "value":0});
    }
    let tab_position = value["styles"]["tabs"].as_array().unwrap().len();
    value["styles"]["para_shapes"][index]["tab_pr_id_ref"] = json!(tab_position);
    value["styles"]["tabs"].as_array_mut().unwrap().push(json!({
        "id":tab_position, "auto_tab_left":false, "auto_tab_right":false,
        "stops":[{"position":{"unit":"char", "value":250},"align":"Left","leader":"NONE"}]
    }));
    let bytes = from_json(&value.to_string(), &FromJsonOptions::default()).unwrap().bytes;
    for mode in [MdMode::Styled, MdMode::Lossy, MdMode::Lossless] {
        let out = to_md(&bytes, &MdExportOptions::default().with_mode(mode)).unwrap();
        let warnings: Vec<_> = out
            .warnings
            .iter()
            .map(|w| w.info())
            .filter(|w| w.code == "PARAGRAPH_MEASURE_LOST")
            .collect();
        assert_eq!(warnings.len(), 6, "{mode:?}: {warnings:?}");
        for field in [
            "margin_left",
            "margin_right",
            "indent",
            "spacing_before",
            "spacing_after",
            &format!("tabs[{tab_position}].stops[0].position"),
        ] {
            assert!(warnings.iter().any(|w| w.message.contains(field)), "{field}: {warnings:?}");
        }
    }
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
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn only_styled_extracts_images() {
    // The other two renderings have no syntax for an extracted binary, so
    // an image map from them would be bytes no reader could reach.
    let styled = export("green_swatch.hwpx", MdMode::Styled);
    assert!(!styled.images.is_empty(), "the picture fixture must yield an image");

    for mode in [MdMode::Lossy, MdMode::Lossless] {
        assert!(export("green_swatch.hwpx", mode).images.is_empty(), "{}", mode.as_str());
    }
    // The lossless rendering carries the picture's original box (S20,
    // design §19.23.21: original 17664 x 5975, shown 25942 x 8775).
    // 이것을 실패시키는 것: 그림 원래 크기를 쓰지 않거나 거부하기.
    let lossless = export("green_swatch.hwpx", MdMode::Lossless).markdown;
    assert!(
        lossless.contains(r#"data-original-width-unit="17664" data-original-height-unit="5975""#),
        "{lossless}"
    );
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn every_extracted_image_is_referenced_by_the_markdown_that_carries_it() {
    let out = export("green_swatch.hwpx", MdMode::Styled);

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
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn meta_carries_exactly_the_mode_images_and_warnings_keys() {
    let out = export("green_swatch.hwpx", MdMode::Lossy);

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
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn meta_images_keep_their_keys_and_bytes() {
    let out = export("green_swatch.hwpx", MdMode::Styled);
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
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn meta_round_trips_through_json() {
    // `serde_bytes` must not be write-only: the Python stub and the pytest
    // contract test both read the shape back.
    let meta = export("green_swatch.hwpx", MdMode::Styled).meta();

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

/// Nine tables, each in the one cell of the next.
fn nine_nested_tables() -> Vec<u8> {
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::core::{Paragraph, Run};
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    let mut run = Run::text("core", CharShapeIndex::new(0));
    for _ in 0..9 {
        let cell = TableCell::new(
            vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
            HwpUnit::from_mm(80.0).expect("width"),
        );
        run = Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0));
    }
    hwpx_of_one_run(run)
}

/// A one-paragraph HWPX holding `run`.
fn hwpx_of_one_run(run: hwpforge::core::Run) -> Vec<u8> {
    use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Section};
    use hwpforge::foundation::ParaShapeIndex;
    use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
    use hwpforge::hwpx::HwpxEncoder;

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
        PageSettings::a4(),
    ));
    let mut store = HwpxStyleStore::new();
    for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &store, &ImageStore::new())
        .expect("encode")
}

/// What the lossy view leaves out for depth reaches the caller as a
/// warning: the ninth nested table is written as a notice and reported.
/// 이것을 실패시키는 것: `md_warning_code` 의 갈래 삭제(코드가 UNCLASSIFIED) ·
/// lossy 경고 전달 삭제.
#[test]
fn lossy_reports_the_nested_table_it_left_out() {
    let out = to_md(&nine_nested_tables(), &MdExportOptions::default().with_mode(MdMode::Lossy))
        .expect("to_md");
    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert_eq!(codes.iter().filter(|c| *c == "CONTAINER_TOO_DEEP").count(), 1, "{codes:?}");
    assert!(out.markdown.contains("(중첩 표 생략)"), "{}", out.markdown);
}

/// A styled warning reaches the caller too: an equation whose LaTeX would
/// end its `$...$` early is written as text and reported.
/// 이것을 실패시키는 것: styled 경고 전달 삭제 · `md_warning_code` 의 갈래 삭제.
#[test]
fn styled_reports_an_equation_written_as_text() {
    use hwpforge::core::{control::Control, Run};
    use hwpforge::foundation::CharShapeIndex;
    let run = Run::control(Control::equation("a $ b"), CharShapeIndex::new(0));
    let out = to_md(&hwpx_of_one_run(run), &MdExportOptions::default().with_mode(MdMode::Styled))
        .expect("to_md");
    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert_eq!(codes.iter().filter(|c| *c == "EQUATION_AS_TEXT").count(), 1, "{codes:?}");
    assert!(out.markdown.contains("a \\$ b"), "{}", out.markdown);
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
