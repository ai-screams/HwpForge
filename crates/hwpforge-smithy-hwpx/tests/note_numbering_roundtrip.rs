//! 공개 encode→decode→encode 왕복에서 각주 번호 연속성 검증 (H-N2 반례 잠금).
//!
//! `<hp:startNum>` 에는 각주/미주 속성이 없어 디코더가 `BeginNum{footnote:1}`
//! 을 **합성**한다 — 이 합성값을 카운터 재시작 신호로 읽으면 재인코드에서
//! 후속 섹션 번호가 1 로 리셋된다 (1,2,3 → 1,2,1). 이 게이트가 그 회귀를
//! 공개 API 경로 그대로 잠근다.

use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

fn note_section(bodies: &[&str]) -> Section {
    let runs = bodies
        .iter()
        .map(|b| {
            Run::control(
                Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text(*b, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]),
                CharShapeIndex::new(0),
            )
        })
        .collect();
    Section::with_paragraphs(
        vec![Paragraph::with_runs(runs, ParaShapeIndex::new(0))],
        PageSettings::a4(),
    )
}

/// ZIP 산출물에서 섹션 XML 을 꺼낸다.
fn section_xml(bytes: &[u8], index: usize) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name(&format!("Contents/section{index}.xml"))
        .expect("section entry")
        .read_to_string(&mut xml)
        .expect("read");
    xml
}

/// 문서 순서의 FOOTNOTE autoNum `num` 시퀀스.
fn footnote_autonum_sequence(xml: &str) -> Vec<u32> {
    xml.split(r#"<hp:autoNum num=""#)
        .skip(1)
        .filter_map(|rest| {
            let (num, tail) = rest.split_once('"')?;
            tail.starts_with(r#" numType="FOOTNOTE""#).then(|| num.parse().ok())?
        })
        .collect()
}

#[test]
fn public_roundtrip_keeps_note_numbers_across_sections() {
    let store = minimal_store();
    let images = ImageStore::new();

    let mut doc = Document::new();
    doc.add_section(note_section(&["하나", "둘"]));
    doc.add_section(note_section(&["셋"]));
    let validated = doc.validate().expect("validate");

    let first = HwpxEncoder::encode(&validated, &store, &images).expect("encode 1");
    assert_eq!(footnote_autonum_sequence(&section_xml(&first, 0)), vec![1, 2]);
    assert_eq!(footnote_autonum_sequence(&section_xml(&first, 1)), vec![3], "첫 인코드 s2=3");

    // 공개 왕복: decode 가 합성 begin_num 을 만들어도 번호는 유지돼야 한다.
    let decoded = HwpxDecoder::decode(&first).expect("decode");
    let revalidated = decoded.document.validate().expect("re-validate");
    let second = HwpxEncoder::encode(&revalidated, &decoded.style_store, &decoded.image_store)
        .expect("encode 2");
    assert_eq!(footnote_autonum_sequence(&section_xml(&second, 0)), vec![1, 2]);
    assert_eq!(
        footnote_autonum_sequence(&section_xml(&second, 1)),
        vec![3],
        "재인코드에서 s2 가 1 로 리셋되면 H-N2 회귀"
    );
}

/// 3차 평결 High(신규): 첫 섹션의 **명시적** `begin_num`(= header
/// `<hh:beginNum>` 실값)은 문서 시작에서 반영돼야 하며 공개 왕복에서
/// 유지돼야 한다 — 전부 무시하면 사용자/한컴의 시작번호 7 이 손실된다.
#[test]
fn public_roundtrip_keeps_explicit_document_begin_num() {
    use hwpforge_core::section::BeginNum;

    let store = minimal_store();
    let images = ImageStore::new();

    let mut s1 = note_section(&["하나", "둘"]);
    s1.begin_num = Some(BeginNum { footnote: 7, ..Default::default() });
    let mut doc = Document::new();
    doc.add_section(s1);
    doc.add_section(note_section(&["셋"]));
    let validated = doc.validate().expect("validate");

    let first = HwpxEncoder::encode(&validated, &store, &images).expect("encode 1");
    assert_eq!(footnote_autonum_sequence(&section_xml(&first, 0)), vec![7, 8]);
    assert_eq!(footnote_autonum_sequence(&section_xml(&first, 1)), vec![9], "연속 유지");

    // header <hh:beginNum footnote="7"> 왕복 → 재인코드에서도 7,8,9 유지.
    let decoded = HwpxDecoder::decode(&first).expect("decode");
    let revalidated = decoded.document.validate().expect("re-validate");
    let second = HwpxEncoder::encode(&revalidated, &decoded.style_store, &decoded.image_store)
        .expect("encode 2");
    assert_eq!(footnote_autonum_sequence(&section_xml(&second, 0)), vec![7, 8]);
    assert_eq!(
        footnote_autonum_sequence(&section_xml(&second, 1)),
        vec![9],
        "명시적 시작번호가 왕복에서 손실되면 3차 평결 High 회귀"
    );
}

/// 4차 평결 Critical (pre-existing, 재현 확정): heading 문단의 첫 run 이
/// 하이퍼링크(전체-run 치환 placeholder)이면 titleMark 후주입이 치환 키를
/// 어긋내 내부 마커(`__HWPHL_*`)가 최종 XML 로 유출됐다. titleMark 부착을
/// 보류(경고)하고 마커는 절대 유출되지 않아야 한다.
#[test]
fn heading_hyperlink_first_run_must_not_leak_markers() {
    let store = minimal_store();
    let images = ImageStore::new();

    let plain = Paragraph::with_runs(
        vec![Run::text("첫 문단", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut heading = Paragraph::with_runs(
        vec![Run::control(
            Control::Hyperlink { text: "링크".into(), url: "https://example.com".into() },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    heading.heading_level = Some(1);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![plain, heading], PageSettings::a4()));
    let validated = doc.validate().expect("validate");

    let bytes = HwpxEncoder::encode(&validated, &store, &images).expect("encode");
    let xml = section_xml(&bytes, 0);
    assert!(!xml.contains("__HWP"), "내부 치환 마커 유출 (4차 평결 Critical): {xml}");
    assert!(xml.contains("fieldBegin"), "하이퍼링크 치환 자체는 실행돼야 함: {xml}");
}
