//! End-to-end integration tests for the full MD → Core → HWPX pipeline.
//!
//! Tests the complete flow:
//! 1. MdDecoder::decode(markdown, &template) → MdDocument { document, style_registry }
//! 2. document.validate() → Document<Validated>
//! 3. HwpxRegistryBridge::from_registry(&style_registry) → rebind Draft → store-local ids
//! 4. HwpxEncoder::encode(&validated, bridge.style_store()) → Vec<u8> (HWPX ZIP bytes)
//! 5. HwpxDecoder::decode(&bytes) → HwpxDocument { document, style_store }

use hwpforge_blueprint::builtins::{builtin_default, builtin_gov_proposal};
use hwpforge_blueprint::template::Template;
use hwpforge_core::{Document, Draft, RunContent};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleLookup};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};
use pretty_assertions::assert_eq;

/// Helper function to run the full pipeline and return decoded HWPX document (still Draft).
fn run_full_pipeline(markdown: &str, template: &Template) -> (Vec<u8>, Document<Draft>) {
    // 1. Decode markdown
    let md_doc = MdDecoder::decode(markdown, template).expect("MD decode should succeed");

    let bridge =
        HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge should succeed");
    let rebound =
        bridge.rebind_draft_document(md_doc.document).expect("style rebind should succeed");

    // 2. Validate Core document
    let validated = rebound.validate().expect("Validation should succeed");

    // 4. Encode to HWPX
    let hwpx_bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode should succeed");

    // 5. Decode HWPX back (returns Document<Draft>)
    let hwpx_doc = HwpxDecoder::decode(&hwpx_bytes).expect("HWPX decode should succeed");

    (hwpx_bytes, hwpx_doc.document)
}

#[test]
fn pipeline_simple_body_text() {
    let markdown = "# 제목\n\n본문 텍스트입니다.";
    let template = builtin_default().expect("builtin_default should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: decoded HWPX has at least 1 section
    assert!(!decoded.sections().is_empty(), "Should have at least 1 section");

    // Assert: paragraphs contain expected text
    let section = &decoded.sections()[0];
    assert!(!section.paragraphs.is_empty(), "Section should have paragraphs");

    let all_text: Vec<String> = section.paragraphs.iter().map(|p| p.text_content()).collect();
    let combined = all_text.join(" ");

    assert!(combined.contains("제목"), "Should contain heading text");
    assert!(combined.contains("본문 텍스트입니다"), "Should contain body text");
}

#[test]
fn pipeline_headings_h1_through_h6() {
    let markdown = r#"# H1 제목
## H2 제목
### H3 제목
#### H4 제목
##### H5 제목
###### H6 제목

본문 텍스트입니다."#;

    let template = builtin_default().expect("builtin_default should succeed");
    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: HWPX decoded doc has correct number of paragraphs (7: 6 headings + body)
    let section = &decoded.sections()[0];
    assert_eq!(section.paragraphs.len(), 7, "Should have 7 paragraphs (6 headings + 1 body)");

    // Verify heading texts are present
    let texts: Vec<String> = section.paragraphs.iter().map(|p| p.text_content()).collect();

    assert!(texts[0].contains("H1 제목"));
    assert!(texts[1].contains("H2 제목"));
    assert!(texts[2].contains("H3 제목"));
    assert!(texts[3].contains("H4 제목"));
    assert!(texts[4].contains("H5 제목"));
    assert!(texts[5].contains("H6 제목"));
    assert!(texts[6].contains("본문 텍스트입니다"));
}

#[test]
fn pipeline_table_roundtrip() {
    let markdown = "| A | B |\n|---|---|\n| 1 | 2 |";
    let template = builtin_default().expect("builtin_default should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: decoded HWPX contains a table run with correct row/col count
    let section = &decoded.sections()[0];
    assert!(!section.paragraphs.is_empty(), "Should have paragraphs");

    // Find the paragraph with a table
    let table_para = section
        .paragraphs
        .iter()
        .find(|p| p.runs.iter().any(|r| matches!(r.content, RunContent::Table(_))))
        .expect("Should have a paragraph with table");

    let table_run = table_para
        .runs
        .iter()
        .find(|r| matches!(r.content, RunContent::Table(_)))
        .expect("Should have a table run");

    if let RunContent::Table(table) = &table_run.content {
        // The markdown parser treats header as regular row, so only 1 data row after separator
        assert!(!table.rows.is_empty(), "Should have at least 1 row");
        // Note: col_count() returns the max column count across all rows
        let cols = table.col_count();
        assert_eq!(cols, 2, "Should have 2 columns");
    } else {
        panic!("Expected table content");
    }
}

#[test]
fn pipeline_multiple_sections() {
    let markdown = "First\n\n<!-- hwpforge:section -->\n\nSecond";
    let template = builtin_default().expect("builtin_default should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: decoded HWPX has 2 sections
    assert_eq!(decoded.sections().len(), 2, "Should have 2 sections");

    let first_text = decoded.sections()[0].paragraphs[0].text_content();
    let second_text = decoded.sections()[1].paragraphs[0].text_content();

    assert!(first_text.contains("First"));
    assert!(second_text.contains("Second"));
}

#[test]
fn pipeline_gov_proposal_template() {
    let markdown = "# 제안서 제목\n\n본문 내용입니다.";
    let template = builtin_gov_proposal().expect("builtin_gov_proposal should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: successfully produces HWPX
    assert!(!decoded.sections().is_empty(), "Should have at least 1 section");

    // Assert: style_store has fonts/char shapes
    // (This is implicit in successful encoding/decoding)
    let section = &decoded.sections()[0];
    assert!(!section.paragraphs.is_empty(), "Should have paragraphs");

    let text = section.paragraphs[0].text_content();
    assert!(text.contains("제안서 제목") || text.contains("본문 내용입니다"));
}

#[test]
fn pipeline_frontmatter_preserved() {
    let markdown = "---\ntitle: 제안서\nauthor: 김철수\ndate: 2026-02-16\n---\n\n본문입니다.";
    let template = builtin_default().expect("builtin_default should succeed");

    // Decode MD and check metadata
    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");

    // Assert: metadata is preserved in MD decode result
    let metadata = md_doc.document.metadata();
    assert_eq!(metadata.title.as_deref(), Some("제안서"));
    assert_eq!(metadata.author.as_deref(), Some("김철수"));
    // Note: "date" field is custom metadata, stored in created/modified or custom fields
    // For now, just verify title and author work

    // Continue with full pipeline
    let bridge =
        HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge should succeed");
    let rebound =
        bridge.rebind_draft_document(md_doc.document).expect("style rebind should succeed");
    let validated = rebound.validate().expect("Validation should succeed");

    let hwpx_bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode should succeed");

    let hwpx_doc = HwpxDecoder::decode(&hwpx_bytes).expect("HWPX decode should succeed");

    // HWPX should have the body text
    let section = &hwpx_doc.document.sections()[0];
    let text = section.paragraphs[0].text_content();
    assert!(text.contains("본문입니다"));
}

#[test]
fn pipeline_link_and_image() {
    let markdown = "[Rust](https://rust-lang.org) ![logo](logo.png)";
    let template = builtin_default().expect("builtin_default should succeed");

    // Verify the MD decode captures hyperlink + image
    let md_doc = MdDecoder::decode(markdown, &template).unwrap();
    let section = &md_doc.document.sections()[0];
    let para = &section.paragraphs[0];
    assert!(
        para.runs.iter().any(|r| matches!(r.content, RunContent::Control(_))),
        "MD decode should capture hyperlink control"
    );
    assert!(
        para.runs.iter().any(|r| matches!(r.content, RunContent::Image(_))),
        "MD decode should capture image run"
    );

    // Full pipeline produces valid HWPX (controls/images are lossy through HWPX roundtrip)
    let (_bytes, decoded) = run_full_pipeline(markdown, &template);
    assert!(!decoded.sections().is_empty(), "Should produce valid HWPX with sections");
    assert!(!decoded.sections()[0].paragraphs.is_empty(), "Should have paragraphs");
}

#[test]
fn pipeline_ordered_and_unordered_lists() {
    let markdown = "- item1\n- item2\n\n1. first\n2. second";
    let template = builtin_default().expect("builtin_default should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: decoded paragraphs contain list text
    let section = &decoded.sections()[0];
    let all_text: Vec<String> = section.paragraphs.iter().map(|p| p.text_content()).collect();

    let combined = all_text.join(" ");
    assert!(combined.contains("item1"));
    assert!(combined.contains("item2"));
    assert!(combined.contains("first"));
    assert!(combined.contains("second"));
}

#[test]
fn pipeline_task_list_preserves_checkable_hwpx_semantics() {
    let markdown = "- [ ] todo\n- [x] done";
    let template = builtin_default().expect("builtin_default should succeed");

    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");
    let bridge =
        HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge should succeed");
    let rebound =
        bridge.rebind_draft_document(md_doc.document).expect("style rebind should succeed");
    let validated = rebound.validate().expect("Validation should succeed");

    let hwpx_bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode should succeed");

    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("HWPX decode should succeed");
    let section = &decoded.document.sections()[0];
    let todo_para = section
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.text_content() == "todo")
        .expect("todo paragraph");
    let done_para = section
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.text_content() == "done")
        .expect("done paragraph");
    let todo_shape = decoded.style_store.para_shape(todo_para.para_shape_id).expect("todo shape");
    let done_shape = decoded.style_store.para_shape(done_para.para_shape_id).expect("done shape");
    let bullet = decoded
        .style_store
        .iter_bullets()
        .find(|bullet| bullet.id == todo_shape.heading_id_ref)
        .expect("task bullet");

    assert!(bullet.is_checkable());
    assert_eq!(bullet.checked_char.as_deref(), Some("☑"));
    assert!(!todo_shape.checked);
    assert!(done_shape.checked);
}

#[test]
fn pipeline_task_list_lossy_encode_uses_gfm_syntax() {
    let markdown = "- [ ] todo\n- [x] done";
    let template = builtin_default().expect("builtin_default should succeed");

    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");
    let validated = md_doc.document.validate().expect("Validation should succeed");
    let encoded = MdEncoder::encode(&validated, &template).expect("lossy encode should succeed");

    assert!(encoded.contains("- [ ] todo"));
    assert!(encoded.contains("- [x] done"));
}

#[test]
fn pipeline_ordered_task_list_normalizes_to_unordered_checkable_markdown() {
    let markdown = "1. [ ] todo\n2. [x] done";
    let template = builtin_default().expect("builtin_default should succeed");

    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");
    let validated = md_doc.document.validate().expect("Validation should succeed");
    let encoded = MdEncoder::encode(&validated, &template).expect("lossy encode should succeed");

    assert!(encoded.contains("- [ ] todo"));
    assert!(encoded.contains("- [x] done"));
    assert!(!encoded.contains("1. [ ]"));
    assert!(!encoded.contains("2. [x]"));
}

#[test]
fn pipeline_multi_paragraph_task_item_preserves_continuation_across_roundtrip() {
    let markdown = "- [ ] first paragraph of the same task item\n\n  second paragraph of the same task item\n\n- [x] next real task item";
    let template = builtin_default().expect("builtin_default should succeed");

    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");
    let template_validated =
        md_doc.document.clone().validate().expect("Template validate should succeed");
    let lossy =
        MdEncoder::encode(&template_validated, &template).expect("lossy encode should succeed");
    assert!(lossy.contains("- [ ] first paragraph of the same task item"));
    assert!(lossy.contains("\n\n  second paragraph of the same task item"));
    assert!(lossy.contains("- [x] next real task item"));

    let rebound_doc = md_doc.document.clone();
    let bridge =
        HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge should succeed");
    let rebound = bridge.rebind_draft_document(rebound_doc).expect("style rebind should succeed");
    let validated = rebound.validate().expect("Validation should succeed");

    let hwpx_bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode should succeed");

    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("HWPX decode should succeed");
    let validated_roundtrip =
        decoded.document.validate().expect("Roundtrip validate should succeed");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let styled = MdEncoder::encode_styled(&validated_roundtrip, &lookup);

    assert_eq!(styled.markdown.trim(), markdown);
}

#[test]
fn pipeline_style_store_counts_match_registry() {
    let markdown = "# 제목\n\n본문입니다.";
    let template = builtin_default().expect("builtin_default should succeed");

    // Decode simple MD → get style_registry
    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode should succeed");

    let bridge =
        HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge should succeed");
    let store = bridge.style_store();

    // Assert: store counts match registry counts
    // Fonts are mirrored across 7 language groups (HANGUL, LATIN, HANJA, JAPANESE, OTHER, SYMBOL, USER)
    assert_eq!(
        store.font_count(),
        md_doc.style_registry.font_count() * 7,
        "Font counts should match (7 lang groups)"
    );

    // 7 default charPr groups + user charShapes; 20 default paraPr groups + user paraShapes
    assert_eq!(
        store.char_shape_count(),
        7 + md_doc.style_registry.char_shape_count(),
        "Char shape counts should match (7 defaults + user)"
    );

    assert_eq!(
        store.para_shape_count(),
        20 + md_doc.style_registry.para_shape_count(),
        "Para shape counts should match (20 defaults + user)"
    );
}

#[test]
fn pipeline_empty_markdown_produces_valid_hwpx() {
    let markdown = "";
    let template = builtin_default().expect("builtin_default should succeed");

    let (_bytes, decoded) = run_full_pipeline(markdown, &template);

    // Assert: produces valid HWPX with at least 1 section and 1 paragraph
    assert!(!decoded.sections().is_empty(), "Should have at least 1 section");
    assert!(!decoded.sections()[0].paragraphs.is_empty(), "Should have at least 1 paragraph");
}
