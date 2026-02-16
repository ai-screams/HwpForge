//! Golden roundtrip tests: MD fixture → Core → HWPX → decode → verify.

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_core::{Control, Paragraph, RunContent};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxStyleStore};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Helper: MD file → Core → HWPX bytes → decode back.
///
/// Returns the original `MdDocument` (for metadata/structure checks) and the
/// HWPX decode result. The document is cloned before validation so we only
/// decode the fixture file once.
fn roundtrip_fixture(
    name: &str,
) -> (hwpforge_smithy_md::MdDocument, hwpforge_smithy_hwpx::HwpxDocument) {
    let template = builtin_default().unwrap();
    let md_result = MdDecoder::decode_file(fixture_path(name), &template).unwrap();
    let validated = md_result.document.clone().validate().unwrap();
    let store = HwpxStyleStore::from_registry(&md_result.style_registry);
    let hwpx_bytes = HwpxEncoder::encode(&validated, &store).unwrap();
    let hwpx_result = HwpxDecoder::decode(&hwpx_bytes).unwrap();
    (md_result, hwpx_result)
}

#[test]
fn golden_simple_body_roundtrip() {
    let (md_result, hwpx_result) = roundtrip_fixture("simple_body.md");

    // Assert: MD decode metadata has title "Simple Body Test"
    assert_eq!(md_result.document.metadata().title.as_deref(), Some("Simple Body Test"));

    // Assert: HWPX decode has 1 section
    assert_eq!(hwpx_result.document.sections().len(), 1);

    // Assert: paragraphs contain "제목" and "본문 텍스트"
    let section = &hwpx_result.document.sections()[0];
    let all_text: Vec<String> = section.paragraphs.iter().map(Paragraph::text_content).collect();

    assert!(all_text.iter().any(|t| t.contains("제목")), "Expected to find '제목' in paragraphs");
    assert!(
        all_text.iter().any(|t| t.contains("본문 텍스트")),
        "Expected to find '본문 텍스트' in paragraphs"
    );
}

#[test]
fn golden_full_elements_roundtrip() {
    let (md_result, hwpx_result) = roundtrip_fixture("full_elements.md");

    // Assert: MD decode metadata has title "Full Elements Test", author "HwpForge", date "2026-02-16"
    assert_eq!(md_result.document.metadata().title.as_deref(), Some("Full Elements Test"));
    assert_eq!(md_result.document.metadata().author.as_deref(), Some("HwpForge"));
    assert_eq!(md_result.document.metadata().created.as_deref(), Some("2026-02-16"));

    // Assert: HWPX decode has 1 section
    assert_eq!(hwpx_result.document.sections().len(), 1);

    let section = &hwpx_result.document.sections()[0];

    // Assert: paragraphs count >= 10 (headings + body + list items + table)
    assert!(
        section.paragraphs.len() >= 10,
        "Expected at least 10 paragraphs, got {}",
        section.paragraphs.len()
    );

    // Assert: at least one paragraph contains a Table run
    let has_table =
        section.paragraphs.iter().flat_map(|p| &p.runs).any(|run| run.content.as_table().is_some());
    assert!(has_table, "Expected to find at least one Table run");

    // Assert: MD decode captures a Hyperlink control.
    // Note: HWPX encoder does not preserve Control::Hyperlink (Phase 4 limitation),
    // so we only verify the MD decode result here, not the HWPX roundtrip.
    let md_has_hyperlink =
        md_result.document.sections().iter().flat_map(|s| &s.paragraphs).flat_map(|p| &p.runs).any(
            |run| {
                matches!(
                    run.content,
                    RunContent::Control(ref ctrl) if matches!(
                        ctrl.as_ref(),
                        Control::Hyperlink { .. }
                    )
                )
            },
        );
    assert!(md_has_hyperlink, "Expected MD decode to find at least one Hyperlink control");
}

#[test]
fn golden_multi_section_roundtrip() {
    let (md_result, hwpx_result) = roundtrip_fixture("multi_section.md");

    // Assert: MD decode produces 3 sections
    assert_eq!(md_result.document.sections().len(), 3);

    // Assert: HWPX decode produces 3 sections
    assert_eq!(hwpx_result.document.sections().len(), 3);

    // Assert: each section has at least 1 paragraph
    for (i, section) in hwpx_result.document.sections().iter().enumerate() {
        assert!(!section.paragraphs.is_empty(), "Section {} should have at least 1 paragraph", i);
    }

    // Assert: section texts contain "섹션 1", "섹션 2", "섹션 3"
    let section_texts: Vec<String> = hwpx_result
        .document
        .sections()
        .iter()
        .map(|section| {
            section.paragraphs.iter().map(Paragraph::text_content).collect::<Vec<_>>().join(" ")
        })
        .collect();

    assert!(
        section_texts[0].contains("섹션 1"),
        "Section 0 should contain '섹션 1', got: {}",
        section_texts[0]
    );
    assert!(
        section_texts[1].contains("섹션 2"),
        "Section 1 should contain '섹션 2', got: {}",
        section_texts[1]
    );
    assert!(
        section_texts[2].contains("섹션 3"),
        "Section 2 should contain '섹션 3', got: {}",
        section_texts[2]
    );
}

#[test]
fn golden_lossy_encode_then_decode_stability() {
    let template = builtin_default().unwrap();

    // Decode simple_body.md with template
    let md_result = MdDecoder::decode_file(fixture_path("simple_body.md"), &template).unwrap();
    let validated = md_result.document.validate().unwrap();

    // Encode to lossy markdown (without template mapping)
    let lossy_md = MdEncoder::encode_lossy(&validated).unwrap();

    // Re-decode the lossy output
    let re_decoded = MdDecoder::decode(&lossy_md, &template).unwrap();

    // Assert: re-decoded document has same section count
    assert_eq!(re_decoded.document.sections().len(), validated.sections().len());

    // Assert: similar paragraph count (lossy may normalize slightly)
    let original_para_count: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();
    let re_decoded_para_count: usize =
        re_decoded.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    assert!(
        re_decoded_para_count >= original_para_count - 2,
        "Re-decoded paragraph count {} should be close to original {}",
        re_decoded_para_count,
        original_para_count
    );
}

#[test]
fn golden_lossless_roundtrip() {
    let template = builtin_default().unwrap();

    // Decode simple_body.md with template
    let md_result = MdDecoder::decode_file(fixture_path("simple_body.md"), &template).unwrap();
    let validated = md_result.document.validate().unwrap();

    // Encode to lossless markdown
    let lossless_md = MdEncoder::encode_lossless(&validated).unwrap();

    // Decode lossless back
    let lossless_decoded = MdDecoder::decode_lossless(&lossless_md).unwrap();

    // Assert: same section count
    assert_eq!(lossless_decoded.sections().len(), validated.sections().len());

    // Assert: same paragraph count
    let original_para_count: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();
    let lossless_para_count: usize =
        lossless_decoded.sections().iter().map(|s| s.paragraphs.len()).sum();
    assert_eq!(lossless_para_count, original_para_count);

    // Assert: same text content
    let original_text: Vec<String> = validated
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .map(Paragraph::text_content)
        .collect();

    let lossless_text: Vec<String> = lossless_decoded
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .map(Paragraph::text_content)
        .collect();

    assert_eq!(lossless_text, original_text);
}
