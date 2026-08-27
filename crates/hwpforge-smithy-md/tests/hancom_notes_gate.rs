//! W3 — 한컴 native 각주/미주 fixture 전수 게이트.
//!
//! Fixture 는 macOS 한컴(appVersion 12.30.0.6446)이 저작한 실물이다
//! (각주·미주 MD 브리지 에픽, `.docs/planning/2026-08-27-*.md` §6).
//! 게이트 3종:
//!
//! 1. **G0→G1**: 한컴 HWPX 를 `to-md` 하면 기대한 `[^N]`/`[^eN]` 구조가 나온다.
//! 2. **G1 재입력**: 그 MD 를 `convert`(MdDecoder) 가 받아들인다 — 왕복 파손의
//!    원 발단이 재발하지 않음을 잠근다.
//! 3. **G1==G3 고정점**: G1 을 MD→HWPX→MD 로 돌려도 바이트 동일.

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleLookup};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 한컴 HWPX → styled MD (G0→G1).
fn hancom_to_md(name: &str) -> String {
    let bytes = fixture_bytes(name);
    let doc = HwpxDecoder::decode(&bytes).expect("hancom fixture decodes");
    let validated = doc.document.validate().expect("validates");
    let lookup = HwpxStyleLookup::new(&doc.style_store, &doc.image_store);
    MdEncoder::encode_styled(&validated, &lookup).markdown
}

/// MD → HWPX → styled MD (G1→G2→G3).
fn md_roundtrip(markdown: &str) -> String {
    let template = builtin_default().expect("builtin");
    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode");
    let bridge = HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge");
    let rebound = bridge.rebind_draft_document(md_doc.document).expect("rebind");
    let validated = rebound.validate().expect("validate");
    let bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode");
    let doc = HwpxDecoder::decode(&bytes).expect("HWPX decode");
    let validated = doc.document.validate().expect("validate2");
    let lookup = HwpxStyleLookup::new(&doc.style_store, &doc.image_store);
    MdEncoder::encode_styled(&validated, &lookup).markdown
}

/// 게이트 본체 — 각 fixture 에 기대 부분 문자열을 잠근다.
fn gate(name: &str, expect_all: &[&str]) {
    let g1 = hancom_to_md(name);
    for needle in expect_all {
        assert!(g1.contains(needle), "{name}: G1 must contain {needle:?}\nG1:\n{g1}");
    }
    let g3 = md_roundtrip(&g1);
    assert_eq!(g1, g3, "{name}: G1 must be an MD fixed point");
}

#[test]
fn f1_single_footnote() {
    gate("F1_footnote_single.hwpx", &["[^1]", "[^1]: 각주 본문 한 줄"]);
}

#[test]
fn f2_multiparagraph_footnote() {
    gate(
        "F2_footnote_multipara.hwpx",
        &["[^1]: 각주 첫 문단", "\n\n    각주 둘째 문단", "\n\n    각주 셋째 문단"],
    );
}

#[test]
fn f3_single_endnote() {
    gate("F3_endnote_single.hwpx", &["[^e1]", "[^e1]: 미주 본문 한 줄"]);
}

#[test]
fn f4_mixed_notes_independent_counters() {
    gate(
        "F4_mixed_notes.hwpx",
        &["[^1]", "[^e1]", "[^2]", "[^1]: 첫 각주", "[^2]: 둘째 각주", "[^e1]: 혼합 미주"],
    );
}

#[test]
fn f5_table_cell_note_source_order() {
    gate("F5_table_cell_note.hwpx", &["가[^1]", "[^2]", "[^1]: 셀 각주", "[^2]: 본문 각주"]);
}

#[test]
fn f6_styled_note_body() {
    gate("F6_styled_note_body.hwpx", &["**굵은 글씨**", "*기울임 글씨*"]);
}

#[test]
fn f7_multiple_notes_same_paragraph() {
    gate(
        "F7_multi_notes.hwpx",
        &["첫째[^1]", "둘째[^2]", "[^3]", "[^1]: 각주 하나", "[^2]: 각주 둘", "[^3]: 각주 셋"],
    );
}

/// W3.5 — 우리 생성 HWPX 의 각주/미주 본문에 autoNum 번호 머리가 있어야 한다
/// (시각 게이트 발견: 없으면 한컴 각주 영역에 번호가 표시되지 않음.
///  한컴 F7 실측: num 은 종류별 실제 순번 캐시 — 1/2/3).
#[test]
fn generated_notes_carry_autonum_heads() {
    let template = builtin_default().expect("builtin");
    let md = "가[^1] 나[^e1] 다[^2]\n\n[^1]: 각주 하나\n\n[^e1]: 미주 하나\n\n[^2]: 각주 둘\n";
    let md_doc = MdDecoder::decode(md, &template).expect("decode");
    let bridge = HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge");
    let rebound = bridge.rebind_draft_document(md_doc.document).expect("rebind");
    let validated = rebound.validate().expect("validate");
    let bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("encode");

    let cursor = std::io::Cursor::new(&bytes);
    let mut z = zip::ZipArchive::new(cursor).expect("zip");
    use std::io::Read;
    let mut xml = String::new();
    z.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).expect("read");

    // 각주 2개: num=1,2 / 미주 1개: num=1 — 각 본문 subList 안에 autoNum 이 선다.
    let foot: Vec<&str> = xml.matches(r#"numType="FOOTNOTE""#).collect();
    let end: Vec<&str> = xml.matches(r#"numType="ENDNOTE""#).collect();
    assert_eq!(foot.len(), 2, "two footnote autoNum heads expected\n{xml}");
    assert_eq!(end.len(), 1, "one endnote autoNum head expected");
    assert!(
        xml.contains(r#"<hp:autoNum num="1" numType="FOOTNOTE""#)
            && xml.contains(r#"<hp:autoNum num="2" numType="FOOTNOTE""#),
        "footnote autoNum nums must be sequential (1, 2)"
    );
    assert!(
        xml.contains(r#"<hp:autoNum num="1" numType="ENDNOTE""#),
        "endnote autoNum num must be 1"
    );
    // 한컴 정합: suffixChar=")" (F1 실측), 본문 텍스트 선행 공백.
    assert!(xml.contains(r#"suffixChar=")""#), "autoNumFormat suffixChar must be ')'");
    assert!(xml.contains("<hp:t> 각주 하나</hp:t>"), "body text gets a leading space");
}

/// autoNum 주입 후에도 MD 고정점은 유지된다 (디코더가 autoNum 을 드롭하고
/// 선행 공백은 trim 되므로 G1 불변).
#[test]
fn autonum_injection_keeps_md_fixed_point() {
    let md = "가[^1] 나[^e1]\n\n[^1]: 각주 본문\n\n[^e1]: 미주 본문\n";
    let g1 = md_roundtrip(md);
    let g3 = md_roundtrip(&g1);
    assert_eq!(g1, g3);
    assert!(g1.contains("[^1]: 각주 본문"), "no number-head text may leak into MD: {g1}");
}
