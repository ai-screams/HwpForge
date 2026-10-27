#[path = "support/hancom_notes_gate.rs"]
mod shared;
use shared::*;

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleLookup};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};

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
    // 한컴 정합: suffixChar=")" (F1 실측). 번호-본문 간격은 별도 spacer run —
    // 본문 텍스트는 불변이다 (치환 마커 봉인 유지).
    assert!(xml.contains(r#"suffixChar=")""#), "autoNumFormat suffixChar must be ')'");
    assert!(xml.contains("<hp:t> </hp:t>"), "spacer run between number head and body");
    assert!(xml.contains("<hp:t>각주 하나</hp:t>"), "body text must stay unmodified");
    // 외부 note 속성도 한컴 wire 정합.
    assert!(
        xml.contains(r#"number="1""#) && xml.contains(r#"suffixChar="41""#),
        "footNote number/suffixChar attributes must be emitted"
    );
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

/// 각주 첫 텍스트가 치환 마커 경로(InlineText 등)를 타면
/// 선행 공백 주입이 마커를 변조해 내부 `__HWP` 문자열이 최종 XML 에 남는가.
#[test]
fn note_head_must_not_corrupt_replacement_markers() {
    use hwpforge_core::inline::{InlineSegment, InlineTabAttr, InlineText};
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::{Run, RunContent};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    // 탭이 든 InlineText 본문 — 인코더가 치환 마커로 처리하는 경로.
    let inline = InlineText {
        segments: vec![
            InlineSegment::Plain("앞".to_string()),
            InlineSegment::Tab(InlineTabAttr {
                width: HwpUnit::new(1000).unwrap(),
                leader: 0,
                tab_type: 0,
            }),
            InlineSegment::Plain("뒤".to_string()),
        ],
    };
    let body = Paragraph::with_runs(
        vec![Run {
            content: RunContent::InlineText(inline),
            char_shape_id: CharShapeIndex::new(0),
        }],
        ParaShapeIndex::new(0),
    );
    let host = Paragraph::with_runs(
        vec![
            Run::text("본문", CharShapeIndex::new(0)),
            Run {
                content: RunContent::Control(Box::new(hwpforge_core::control::Control::footnote(
                    vec![body],
                ))),
                char_shape_id: CharShapeIndex::new(0),
            },
        ],
        ParaShapeIndex::new(0),
    );
    let mut doc = hwpforge_core::document::Document::new();
    doc.add_section(hwpforge_core::section::Section::with_paragraphs(
        vec![host],
        hwpforge_core::PageSettings::default(),
    ));
    let validated = doc.validate().expect("validate");
    let mut store = hwpforge_smithy_hwpx::HwpxStyleStore::with_default_fonts("함초롬돋움");
    store.push_char_shape(hwpforge_smithy_hwpx::HwpxCharShape::default());
    store.push_para_shape(hwpforge_smithy_hwpx::HwpxParaShape::default());
    let bytes = hwpforge_smithy_hwpx::HwpxEncoder::encode(
        &validated,
        &store,
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("encode");
    let cursor = std::io::Cursor::new(&bytes);
    let mut z = zip::ZipArchive::new(cursor).expect("zip");
    use std::io::Read;
    let mut xml = String::new();
    z.by_name("Contents/section0.xml").expect("s0").read_to_string(&mut xml).expect("read");
    assert!(!xml.contains("__HWP"), "internal replacement markers leaked into output:\n{xml}");
    assert!(xml.contains("hp:tab"), "tab must be emitted as real wire");
}

/// 번호-본문 쌍을 구조적으로 검증 (출현 횟수가 아니라 대응).
#[test]
fn autonum_numbers_pair_with_their_bodies() {
    let template = builtin_default().expect("builtin");
    let md = "가[^1] 나[^2]\n\n[^1]: 알파 본문\n\n[^2]: 베타 본문\n";
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
    z.by_name("Contents/section0.xml").expect("s0").read_to_string(&mut xml).expect("read");
    // footNote 블록 단위로 (num, 본문) 쌍을 추출해 순서 대응을 잠근다.
    let mut pairs = Vec::new();
    let mut rest = xml.as_str();
    while let Some(i) = rest.find("<hp:footNote ") {
        let block_end = rest[i..].find("</hp:footNote>").map(|e| i + e).unwrap_or(rest.len());
        let block = &rest[i..block_end];
        let num = block
            .split(r#"<hp:autoNum num=""#)
            .nth(1)
            .and_then(|t| t.split('"').next())
            .unwrap_or("?")
            .to_string();
        let body = if block.contains("알파") {
            "알파"
        } else if block.contains("베타") {
            "베타"
        } else {
            "?"
        };
        pairs.push((num, body));
        rest = &rest[block_end..];
    }
    assert_eq!(
        pairs,
        vec![("1".to_string(), "알파"), ("2".to_string(), "베타")],
        "autoNum numbers must pair with their bodies in source order"
    );
}
