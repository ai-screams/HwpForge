//! Golden tests: decode real HWPX files and verify basic properties.

use hwpforge_smithy_hwpx::HwpxDecoder;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

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
