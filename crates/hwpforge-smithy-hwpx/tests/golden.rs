//! Golden tests: decode real HWPX files, and round-trip encode→decode.

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
    let encoded = HwpxEncoder::encode(&validated, &original.style_store).unwrap();

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
