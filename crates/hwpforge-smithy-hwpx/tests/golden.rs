use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

// ════════════════════════════════════════════════════════════════
// Phase 3: Decode-only golden tests
// ════════════════════════════════════════════════════════════════

// ── garden_notes.hwpx ─────────────────────────────────────────────

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn decode_simple_edit() {
    let path = fixture_path("garden_notes.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());
    assert!(result.style_store.char_shape_count() > 0);
}

// ── SimpleTable.hwpx ────────────────────────────────────────────

#[test]
fn decode_simple_table() {
    let path = fixture_path("SimpleTable.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Should contain at least one table
    let has_table = result
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .any(|r| r.content.is_table());
    assert!(has_table, "SimpleTable.hwpx should contain at least one table");
    // 이것을 실패시키는 것: 셀 1을 9로, 병합 span을 1로, 캐시/스타일을 제거하기.
    // 이것을 실패시키는 것: 행/anchor/본문 또는 폰트·문자·문단 레지스트리를 제거하기.
    let section = &result.document.sections()[0];
    let table =
        section.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.content.as_table()).unwrap();
    let cells: Vec<_> = table.rows.iter().flat_map(|r| &r.cells).collect();
    assert_eq!(table.rows.len(), 3);
    assert_eq!(cells.len(), 5);
    assert_eq!(
        cells.iter().map(|c| (c.col_span, c.row_span)).collect::<Vec<_>>(),
        [(2, 2), (1, 1), (1, 1), (1, 1), (2, 1)]
    );
    assert_eq!(
        cells
            .iter()
            .map(|c| c.paragraphs[0].runs[0].content.as_text().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "5", "4"]
    );
    assert!(section
        .paragraphs
        .iter()
        .any(|p| p.runs.iter().any(|r| r.content.as_text().is_some())));
    assert!(section.paragraphs[0].layout_cache.as_ref().is_some_and(|c| !c.is_empty()));
    assert_eq!(result.style_store.style(0).unwrap().name, "본문");
    assert!(result.style_store.font_count() > 0);
    assert!(result.style_store.char_shape_count() > 0);
    assert!(result.style_store.para_shape_count() > 0);
}

// ── green_swatch.hwpx ──────────────────────────────────────────

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn decode_simple_picture() {
    let path = fixture_path("green_swatch.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Should contain at least one image
    let has_image = result
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .any(|r| r.content.is_image());
    assert!(has_image, "green_swatch.hwpx should contain at least one image");
}

// ── small_page.hwpx ────────────────────────────────────────

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn decode_page_size_margin() {
    let path = fixture_path("small_page.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Page settings should be present with non-zero margins
    let ps = &result.document.sections()[0].page_settings;
    assert!(ps.width.as_i32() > 0);
    assert!(ps.height.as_i32() > 0);
    // 이것을 실패시키는 것: small_page.hwpx의 왼쪽 또는 오른쪽 여백을 0으로 바꾸기.
    assert_eq!(ps.margin_left, hwpforge_foundation::HwpUnit::from_mm(17.0).unwrap());
    assert_eq!(ps.margin_right, hwpforge_foundation::HwpUnit::from_mm(23.0).unwrap());
}

// ════════════════════════════════════════════════════════════════
// Phase 4: Encode→Decode round-trip golden tests
// ════════════════════════════════════════════════════════════════

/// Helper: performs a full round-trip (decode → validate → encode → decode)
/// and asserts structural equality between original and re-decoded documents.
fn assert_roundtrip(fixture_name: &str) {
    let bytes = std::fs::read(fixture_path(fixture_name)).unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();

    // Clone the document before validate() consumes it
    let orig_doc = original.document.clone();

    // Validate (Draft → Validated) then encode
    let validated = original.document.validate().unwrap();
    let images = hwpforge_core::image::ImageStore::new();
    let encoded = HwpxEncoder::encode(&validated, &original.style_store, &images).unwrap();

    // Decode the encoded output
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    // ── Structure equality ───────────────────────────────────
    let orig_sections = orig_doc.sections();
    let rt_sections = roundtripped.document.sections();
    assert_eq!(orig_sections.len(), rt_sections.len(), "{fixture_name}: section count mismatch",);

    for (si, (orig_sec, rt_sec)) in orig_sections.iter().zip(rt_sections.iter()).enumerate() {
        assert_eq!(
            orig_sec.paragraphs.len(),
            rt_sec.paragraphs.len(),
            "{fixture_name}: section[{si}] paragraph count mismatch",
        );

        for (pi, (orig_para, rt_para)) in
            orig_sec.paragraphs.iter().zip(rt_sec.paragraphs.iter()).enumerate()
        {
            // Para shape ID preserved
            assert_eq!(
                orig_para.para_shape_id, rt_para.para_shape_id,
                "{fixture_name}: section[{si}].para[{pi}] para_shape_id mismatch",
            );

            assert_eq!(
                orig_para.runs.len(),
                rt_para.runs.len(),
                "{fixture_name}: section[{si}].para[{pi}] run count mismatch",
            );

            for (ri, (orig_run, rt_run)) in
                orig_para.runs.iter().zip(rt_para.runs.iter()).enumerate()
            {
                // Char shape ID preserved
                assert_eq!(
                    orig_run.char_shape_id, rt_run.char_shape_id,
                    "{fixture_name}: section[{si}].para[{pi}].run[{ri}] char_shape_id mismatch",
                );

                // Text content preserved
                if let Some(orig_text) = orig_run.content.as_text() {
                    let rt_text = rt_run.content.as_text().unwrap_or("<not text>");
                    assert_eq!(
                        orig_text, rt_text,
                        "{fixture_name}: section[{si}].para[{pi}].run[{ri}] text mismatch",
                    );
                }

                // Table structure preserved
                if let Some(orig_table) = orig_run.content.as_table() {
                    let rt_table = rt_run.content.as_table().expect("expected table in roundtrip");
                    assert_eq!(
                        orig_table.rows.len(),
                        rt_table.rows.len(),
                        "{fixture_name}: section[{si}].para[{pi}].run[{ri}] table row count mismatch",
                    );
                    for (row_i, (orig_row, rt_row)) in
                        orig_table.rows.iter().zip(rt_table.rows.iter()).enumerate()
                    {
                        assert_eq!(
                            orig_row.cells.len(),
                            rt_row.cells.len(),
                            "{fixture_name}: table row[{row_i}] cell count mismatch",
                        );
                    }
                }

                // Image path preserved
                if let Some(orig_img) = orig_run.content.as_image() {
                    let rt_img = rt_run.content.as_image().expect("expected image in roundtrip");
                    assert_eq!(
                        orig_img.path, rt_img.path,
                        "{fixture_name}: section[{si}].para[{pi}].run[{ri}] image path mismatch",
                    );
                    assert_eq!(
                        orig_img.width, rt_img.width,
                        "{fixture_name}: image width mismatch",
                    );
                    assert_eq!(
                        orig_img.height, rt_img.height,
                        "{fixture_name}: image height mismatch",
                    );
                }
            }
        }
    }

    // ── Style store equality ─────────────────────────────────
    assert_eq!(
        original.style_store.font_count(),
        roundtripped.style_store.font_count(),
        "{fixture_name}: font count mismatch",
    );
    assert_eq!(
        original.style_store.char_shape_count(),
        roundtripped.style_store.char_shape_count(),
        "{fixture_name}: char shape count mismatch",
    );
    assert_eq!(
        original.style_store.para_shape_count(),
        roundtripped.style_store.para_shape_count(),
        "{fixture_name}: para shape count mismatch",
    );

    // ── Page settings equality ───────────────────────────────
    let orig_ps = &orig_sections[0].page_settings;
    let rt_ps = &rt_sections[0].page_settings;
    assert_eq!(orig_ps, rt_ps, "{fixture_name}: page settings mismatch");
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn roundtrip_simple_edit() {
    assert_roundtrip("garden_notes.hwpx");
}

#[test]
fn roundtrip_simple_table() {
    assert_roundtrip("SimpleTable.hwpx");
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn roundtrip_simple_picture() {
    assert_roundtrip("green_swatch.hwpx");
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn roundtrip_page_size_margin() {
    assert_roundtrip("small_page.hwpx");
}

// ── chart from-scratch encoder→decoder roundtrip ────────────────

#[test]
fn encode_decode_chart_from_scratch() {
    use hwpforge_core::chart::{ChartData, ChartType};
    use hwpforge_core::control::Control;
    use hwpforge_core::document::Document;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };

    // Minimal style store
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    // Build document with a chart
    let chart_ctrl = Control::chart(
        ChartType::Column,
        ChartData::category(&["Q1", "Q2", "Q3"], &[("Revenue", &[100.0, 200.0, 150.0])]),
    );
    let para = Paragraph::with_runs(
        vec![Run::control(chart_ctrl, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    // Encode → decode
    let images = ImageStore::new();
    let validated = doc.validate().unwrap();
    let encoded = HwpxEncoder::encode(&validated, &store, &images).unwrap();
    let decoded = HwpxDecoder::decode(&encoded).unwrap();

    assert!(!decoded.document.sections().is_empty());

    // Must contain a chart control with correct data
    let chart_ctrl = decoded
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .find(|ctrl| ctrl.is_chart())
        .expect("encoded chart document should round-trip a chart");

    if let Control::Chart { chart_type, data, .. } = chart_ctrl {
        assert_eq!(*chart_type, ChartType::Column, "chart_type");
        match data {
            ChartData::Category { categories, series } => {
                assert_eq!(categories, &["Q1", "Q2", "Q3"]);
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].name, "Revenue");
                assert_eq!(series[0].values, vec![100.0, 200.0, 150.0]);
            }
            _ => panic!("expected Category data"),
        }
    } else {
        panic!("expected Control::Chart");
    }
}

// ── line shape from-scratch encoder→decoder roundtrip ────────────

#[test]
fn encode_decode_line_shape() {
    use hwpforge_core::control::{Control, ShapePoint};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };

    // Minimal style store
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    // Build document with a single line shape
    let line_ctrl = Control::Line {
        frame: hwpforge_core::ObjectFrame::new(),
        start: ShapePoint::new(0, 0),
        end: ShapePoint::new(14000, 0),
        width: HwpUnit::new(14000).unwrap(),
        height: HwpUnit::new(100).unwrap(),
        placement: None,
        caption: None,
        style: None,
    };
    let para = Paragraph::with_runs(
        vec![Run::control(line_ctrl, CharShapeIndex::new(0))],
        hwpforge_foundation::ParaShapeIndex::new(0),
    );

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    // Encode → decode
    let images = ImageStore::new();
    let validated = doc.validate().unwrap();
    let encoded = HwpxEncoder::encode(&validated, &store, &images).unwrap();
    let decoded = HwpxDecoder::decode(&encoded).unwrap();

    assert!(!decoded.document.sections().is_empty());

    // Must contain a line control with correct geometry
    let line_ctrl = decoded
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .find(|ctrl| ctrl.is_line())
        .expect("encoded line document should round-trip a line shape");

    if let Control::Line { start, end, width, height, .. } = line_ctrl {
        assert_eq!(start.x, 0, "start.x");
        assert_eq!(start.y, 0, "start.y");
        assert_eq!(end.x, 14000, "end.x");
        assert_eq!(end.y, 0, "end.y");
        assert_eq!(width.as_i32(), 14000, "width");
        assert_eq!(height.as_i32(), 100, "height");
    } else {
        panic!("expected Control::Line");
    }
}
