//! W5-α 게이트 — 머리말/꼬리말 cardinality 보존 (Codex C1).
//!
//! 디코더가 ODD/EVEN 다중 머리말을 first-wins 로 무음 폐기하던 fail-open
//! 을 골든으로 잠근다. fixture = 한컴 직접 제작 실물
//! (`sample-header-footer-odd-even.hwpx`): wire 실측 = `<hp:header
//! applyPageType="ODD">` "홀수페이지 머리말" + `EVEN` "짝수페이지 머리말",
//! 각자 lineseg 1개 보유. 인코더는 원래 Vec 전체를 방출하므로 디코더만
//! 고치면 왕복이 닫힌다.

use hwpforge_foundation::ApplyPageType;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn fixture_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/user_samples/pages/sample-header-footer-odd-even.hwpx"
    );
    std::fs::read(path).expect("odd-even fixture")
}

#[test]
fn odd_even_headers_both_survive_decode() {
    let decoded = HwpxDecoder::decode(&fixture_bytes()).expect("decode");
    let section = &decoded.document.sections()[0];
    let kinds: Vec<ApplyPageType> = section.headers.iter().map(|h| h.apply_page_type).collect();
    assert_eq!(
        kinds,
        vec![ApplyPageType::Odd, ApplyPageType::Even],
        "wire 순서대로 ODD/EVEN 둘 다 보존"
    );
    let texts: Vec<String> =
        section.headers.iter().map(|h| h.paragraphs[0].text_content()).collect();
    assert_eq!(texts, vec!["홀수페이지 머리말", "짝수페이지 머리말"]);
    // PDF 재생 재료 — 각 머리말 문단의 조판 캐시도 함께 보존돼야 한다.
    for header in &section.headers {
        assert!(
            header.paragraphs[0].layout_cache.is_some(),
            "{:?} 머리말의 layout_cache 소실",
            header.apply_page_type
        );
    }
}

#[test]
fn odd_even_headers_roundtrip_cardinality() {
    let decoded = HwpxDecoder::decode(&fixture_bytes()).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store)
        .expect("encode");
    let again = HwpxDecoder::decode(&bytes).expect("re-decode");
    let headers = &again.document.sections()[0].headers;
    assert_eq!(headers.len(), 2, "재인코드 왕복 후에도 머리말 2개");
    assert_eq!(
        headers.iter().map(|h| h.apply_page_type).collect::<Vec<_>>(),
        vec![ApplyPageType::Odd, ApplyPageType::Even]
    );
}
