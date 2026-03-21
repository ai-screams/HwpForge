//! Golden tests: decode real HWPX files, and round-trip encode→decode.

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

fn workspace_fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("tests/fixtures");
    p.push(name);
    p
}

// ════════════════════════════════════════════════════════════════
// Phase 3: Decode-only golden tests
// ════════════════════════════════════════════════════════════════

// ── sample1.hwpx ────────────────────────────────────────────────

#[test]
#[ignore = "sample1.hwpx has malformed XML with duplicate <hp:t> fields"]
fn decode_sample1() {
    let path = fixture_path("sample1.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    // Must have at least 1 section
    assert!(!result.document.sections().is_empty());

    // Style store should have fonts, char shapes, para shapes
    assert!(result.style_store.font_count() > 0);
    assert!(result.style_store.char_shape_count() > 0);
    assert!(result.style_store.para_shape_count() > 0);

    // First section should have paragraphs
    let section = &result.document.sections()[0];
    assert!(!section.paragraphs.is_empty());

    // Page settings should have reasonable dimensions
    let ps = &section.page_settings;
    assert!(ps.width.as_i32() > 0);
    assert!(ps.height.as_i32() > 0);
}

// ── SimpleEdit.hwpx ─────────────────────────────────────────────

#[test]
fn decode_simple_edit() {
    let path = fixture_path("SimpleEdit.hwpx");
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
}

// ── SimplePicture.hwpx ──────────────────────────────────────────

#[test]
fn decode_simple_picture() {
    let path = fixture_path("SimplePicture.hwpx");
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
    assert!(has_image, "SimplePicture.hwpx should contain at least one image");
}

// ── PageSize_Margin.hwpx ────────────────────────────────────────

#[test]
fn decode_page_size_margin() {
    let path = fixture_path("PageSize_Margin.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Page settings should be present with non-zero margins
    let ps = &result.document.sections()[0].page_settings;
    assert!(ps.width.as_i32() > 0);
    assert!(ps.height.as_i32() > 0);
}

#[test]
fn decode_user_sample_tab_preserves_custom_tab_def_and_inline_tab() {
    let path = workspace_fixture_path("user_samples/sample-tab.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(
        result.style_store.iter_tabs().any(|tab| tab.id > 2 && !tab.stops.is_empty()),
        "expected at least one custom tab definition with explicit tab stops"
    );

    let para = &result.document.sections()[0].paragraphs[0];
    assert_eq!(para.runs[0].content.as_text(), Some("LEFT\tRIGHT"));

    let para_shape = result.style_store.para_shape(para.para_shape_id).unwrap();
    assert!(para_shape.tab_pr_id_ref > 2, "paragraph should reference a custom tab definition");
}

#[test]
fn roundtrip_user_sample_tab_preserves_custom_tab_def_and_inline_tab() {
    let bytes = std::fs::read(workspace_fixture_path("user_samples/sample-tab.hwpx")).unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();
    let validated = original.document.validate().unwrap();
    let encoded =
        HwpxEncoder::encode(&validated, &original.style_store, &original.image_store).unwrap();
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    assert!(
        roundtripped.style_store.iter_tabs().any(|tab| tab.id > 2 && !tab.stops.is_empty()),
        "custom tab definitions should survive a full HWPX roundtrip"
    );
    assert_eq!(
        roundtripped.document.sections()[0].paragraphs[0].runs[0].content.as_text(),
        Some("LEFT\tRIGHT")
    );
}

#[test]
fn decode_user_sample_table_tab_preserves_inline_tab_in_cell_text() {
    let path = workspace_fixture_path("user_samples/sample-table-tab.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    let table = result.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|para| &para.runs)
        .find_map(|run| run.content.as_table())
        .expect("expected a table");
    assert_eq!(
        table.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(),
        Some("CELLLEFT\tCELLRIGHT")
    );
}

#[test]
fn roundtrip_user_sample_table_tab_preserves_inline_tab_in_cell_text() {
    let bytes =
        std::fs::read(workspace_fixture_path("user_samples/sample-table-tab.hwpx")).unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();
    let validated = original.document.validate().unwrap();
    let encoded =
        HwpxEncoder::encode(&validated, &original.style_store, &original.image_store).unwrap();
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    let table = roundtripped.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|para| &para.runs)
        .find_map(|run| run.content.as_table())
        .expect("expected a table");
    assert_eq!(
        table.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(),
        Some("CELLLEFT\tCELLRIGHT")
    );
}

#[test]
fn roundtrip_user_sample_checkable_bullet_basic_preserves_checkable_semantics() {
    let bytes =
        std::fs::read(workspace_fixture_path("user_samples/sample-checkable-bullet-basic.hwpx"))
            .unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();
    let validated = original.document.validate().unwrap();
    let encoded =
        HwpxEncoder::encode(&validated, &original.style_store, &original.image_store).unwrap();
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    let paragraphs = &roundtripped.document.sections()[0].paragraphs;
    let unchecked = paragraphs
        .iter()
        .find(|paragraph| paragraph.text_content().contains("unchecked item A"))
        .expect("fixture should contain unchecked task item");
    let checked = paragraphs
        .iter()
        .find(|paragraph| paragraph.text_content().contains("checked item B"))
        .expect("fixture should contain checked task item");
    let unchecked_shape = roundtripped.style_store.para_shape(unchecked.para_shape_id).unwrap();
    let checked_shape = roundtripped.style_store.para_shape(checked.para_shape_id).unwrap();
    let bullet = roundtripped
        .style_store
        .iter_bullets()
        .find(|bullet| bullet.id == unchecked_shape.heading_id_ref)
        .expect("roundtripped bullet definition should exist");

    assert_eq!(bullet.checked_char.as_deref(), Some("☑"));
    assert!(!unchecked_shape.checked);
    assert!(checked_shape.checked);
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
fn roundtrip_simple_edit() {
    assert_roundtrip("SimpleEdit.hwpx");
}

#[test]
fn roundtrip_simple_table() {
    assert_roundtrip("SimpleTable.hwpx");
}

#[test]
fn roundtrip_simple_picture() {
    assert_roundtrip("SimplePicture.hwpx");
}

#[test]
fn roundtrip_page_size_margin() {
    assert_roundtrip("PageSize_Margin.hwpx");
}

#[test]
#[ignore = "sample1.hwpx has malformed XML with duplicate <hp:t> fields"]
fn roundtrip_sample1() {
    assert_roundtrip("sample1.hwpx");
}

// ── line.hwpx ──────────────────────────────────────────────────

#[test]
fn decode_line() {
    let path = fixture_path("line.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Should contain at least one line shape
    let has_line = result
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .any(|ctrl| ctrl.is_line());
    assert!(has_line, "line.hwpx should contain at least one line shape");
}

#[test]
fn roundtrip_line() {
    assert_roundtrip("line.hwpx");
}

// ── equations.hwpx ──────────────────────────────────────────────

#[test]
fn decode_equations() {
    let path = fixture_path("equations.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Count equation controls
    let equation_count = result
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_equation())
        .count();
    assert!(
        equation_count >= 1,
        "equations.hwpx should contain at least 1 equation, found {equation_count}"
    );
}

#[test]
fn roundtrip_equations() {
    let bytes = std::fs::read(fixture_path("equations.hwpx")).unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();

    // Count original equations
    let orig_eq_count = original
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_equation())
        .count();

    // Encode → decode
    let validated = original.document.clone().validate().unwrap();
    let images = hwpforge_core::image::ImageStore::new();
    let encoded = HwpxEncoder::encode(&validated, &original.style_store, &images).unwrap();
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    // Count roundtripped equations
    let rt_eq_count = roundtripped
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_equation())
        .count();

    assert_eq!(
        orig_eq_count, rt_eq_count,
        "equation count mismatch after roundtrip: original={orig_eq_count}, roundtripped={rt_eq_count}"
    );
}

// ── charts.hwpx ────────────────────────────────────────────────

#[test]
fn decode_charts() {
    let path = fixture_path("charts.hwpx");
    let result = HwpxDecoder::decode_file(&path).unwrap();

    assert!(!result.document.sections().is_empty());

    // Count chart controls
    let chart_count = result
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_chart())
        .count();
    assert!(chart_count >= 1, "charts.hwpx should contain at least 1 chart, found {chart_count}");
}

#[test]
fn roundtrip_charts() {
    let bytes = std::fs::read(fixture_path("charts.hwpx")).unwrap();
    let original = HwpxDecoder::decode(&bytes).unwrap();

    // Count original charts
    let orig_chart_count = original
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_chart())
        .count();

    // Encode → decode
    let validated = original.document.clone().validate().unwrap();
    let images = hwpforge_core::image::ImageStore::new();
    let encoded = HwpxEncoder::encode(&validated, &original.style_store, &images).unwrap();
    let roundtripped = HwpxDecoder::decode(&encoded).unwrap();

    // Count roundtripped charts
    let rt_chart_count = roundtripped
        .document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .filter(|ctrl| ctrl.is_chart())
        .count();

    assert_eq!(
        orig_chart_count, rt_chart_count,
        "chart count mismatch after roundtrip: original={orig_chart_count}, roundtripped={rt_chart_count}"
    );
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
        start: ShapePoint::new(0, 0),
        end: ShapePoint::new(14000, 0),
        width: HwpUnit::new(14000).unwrap(),
        height: HwpUnit::new(100).unwrap(),
        horz_offset: 0,
        vert_offset: 0,
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
