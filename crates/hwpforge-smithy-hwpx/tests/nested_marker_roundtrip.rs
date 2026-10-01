//! 중첩 컨테이너 안의 인라인 치환 마커가 끝까지 치환되는지 잠근다 (#195).
//!
//! 인코더는 serde 로 표현할 수 없는 혼합 내용(TAB·줄바꿈·하이퍼링크 등)을
//! 마커 텍스트로 직렬화한 뒤 진짜 XML 조각으로 바꿔 넣는다. 메모 본문·묶음
//! 개체 글상자처럼 **부모 조각 안에 들어 있는** 자식 마커는 예전에 치환되지
//! 않고 `<hp:t>__HWPTXT_…__</hp:t>` 로 출력에 남아, 본문 전체가 마커로
//! 바뀌었다.

use std::io::{Read, Write};

use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

const MEMO_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/user_samples/sample-memo-basic.hwpx"
);

/// 픽스처 메모 본문 (`<hp:t>` 그대로).
const FIXTURE_MEMO_BODY: &str = "쇠부리야 여기가 메모야";

fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

fn zip_entry(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut s = String::new();
    z.by_name(name).expect("entry").read_to_string(&mut s).expect("read");
    s
}

/// 섹션 XML 의 첫 MEMO `<hp:subList>` 조각.
fn memo_sublist(section_xml: &str) -> &str {
    let begin = section_xml.find(r#"type="MEMO""#).expect("MEMO fieldBegin");
    let rest = &section_xml[begin..];
    let start = rest.find("<hp:subList").expect("memo subList");
    let end = rest.find("</hp:subList>").expect("memo subList end");
    &rest[start..end]
}

fn memo_body_text(doc: &Document<hwpforge_core::Validated>) -> String {
    for para in &doc.sections()[0].paragraphs {
        for run in &para.runs {
            if let RunContent::Control(ctrl) = &run.content {
                if let Control::Memo { content, .. } = ctrl.as_ref() {
                    return content
                        .iter()
                        .flat_map(|p| p.runs.iter())
                        .filter_map(|r| r.content.plain_text())
                        .collect();
                }
            }
        }
    }
    panic!("no memo in decoded document");
}

/// 픽스처를 디코드해 메모 본문 첫 run 을 `body` 로 바꾸고 인코드한다
/// (이슈 재현의 `to-json` → 본문 수정 → `from-json` 경로와 같다).
fn encode_fixture_with_memo_body(body: RunContent) -> Vec<u8> {
    let bytes = std::fs::read(MEMO_FIXTURE).expect("fixture");
    let mut decoded = HwpxDecoder::decode(&bytes).expect("decode fixture");
    let mut replaced = false;
    for para in &mut decoded.document.sections_mut()[0].paragraphs {
        for run in &mut para.runs {
            if let RunContent::Control(ctrl) = &mut run.content {
                if let Control::Memo { content, .. } = ctrl.as_mut() {
                    content[0].runs[0].content = body.clone();
                    replaced = true;
                }
            }
        }
    }
    assert!(replaced, "fixture memo not found");
    let validated = decoded.document.validate().expect("validate");
    HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).expect("encode")
}

fn decode_validated(bytes: &[u8]) -> Document<hwpforge_core::Validated> {
    HwpxDecoder::decode(bytes).expect("re-decode").document.validate().expect("re-validate")
}

// 이것을 실패시키는 것: 인코더 `apply_run_xml_replacements` 를 한 pass 로
// 되돌리기 (루프 대신 `splice_pending_markers` 한 번) — 메모 본문 안 HWPTXT
// 마커가 남아 안전망 오류로 인코드가 실패한다.
#[test]
fn memo_body_tab_survives_encode_and_decode() {
    let out = encode_fixture_with_memo_body(RunContent::Text("MEMO\tBODY".to_string()));
    let section = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&section);
    assert!(!section.contains("__HWP"), "internal marker leaked: {sub}");
    assert!(sub.contains("<hp:tab"), "memo body must carry <hp:tab>: {sub}");
    assert_eq!(memo_body_text(&decode_validated(&out)), "MEMO\tBODY");
}

// 이것을 실패시키는 것: 위와 같은 단일 pass 회귀 (줄바꿈도 같은 HWPTXT 경로).
#[test]
fn memo_body_line_break_survives_encode_and_decode() {
    let out = encode_fixture_with_memo_body(RunContent::Text("MEMO\nBODY".to_string()));
    let section = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&section);
    assert!(!section.contains("__HWP"), "internal marker leaked: {sub}");
    assert!(sub.contains("<hp:lineBreak/>"), "memo body must carry <hp:lineBreak/>: {sub}");
    assert_eq!(memo_body_text(&decode_validated(&out)), "MEMO\nBODY");
}

// 이것을 실패시키는 것: 위와 같은 단일 pass 회귀 (NBSP 도 HWPTXT 경로 —
// `inline_text::requires_inline_text_markup`).
#[test]
fn memo_body_nbsp_survives_encode_and_decode() {
    let out = encode_fixture_with_memo_body(RunContent::Text("MEMO\u{00A0}BODY".to_string()));
    let section = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&section);
    assert!(!section.contains("__HWP"), "internal marker leaked: {sub}");
    assert_eq!(memo_body_text(&decode_validated(&out)), "MEMO\u{00A0}BODY");
}

/// 한컴식 입력: 픽스처 wire 의 메모 본문에 `<hp:tab .../>` 을 넣고
/// decode → encode → decode. 디코더의 인라인 탭 경로(`InlineText`)를 거쳐
/// 탭 속성(width/leader/type)까지 살아남아야 한다.
// 이것을 실패시키는 것: 단일 pass 회귀 — InlineText 도 항상 HWPTXT 마커 경로다.
#[test]
fn hancom_memo_tab_roundtrips_with_attributes() {
    let src = std::fs::read(MEMO_FIXTURE).expect("fixture");
    let tab = r#"<hp:tab width="4000" leader="0" type="1"/>"#;
    let patched_body = format!("<hp:t>쇠부리야{tab}여기가 메모야</hp:t>");
    let original_body = format!("<hp:t>{FIXTURE_MEMO_BODY}</hp:t>");

    // 픽스처 섹션 XML 의 메모 본문만 바꿔 다시 묶는다.
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(&src)).expect("zip");
    let mut patched = Vec::new();
    {
        let mut zout = zip::ZipWriter::new(std::io::Cursor::new(&mut patched));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).expect("entry");
            let name = f.name().to_string();
            let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
            let mut data = Vec::new();
            f.read_to_end(&mut data).expect("read entry");
            if name == "Contents/section0.xml" {
                let xml = String::from_utf8(data).expect("utf8");
                assert_eq!(xml.matches(&original_body).count(), 1, "fixture memo body moved");
                data = xml.replacen(&original_body, &patched_body, 1).into_bytes();
            }
            zout.start_file(name, opts).expect("start");
            zout.write_all(&data).expect("write");
        }
        zout.finish().expect("finish");
    }

    let decoded = HwpxDecoder::decode(&patched).expect("decode patched");
    let validated = decoded.document.validate().expect("validate");
    let out = HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store)
        .expect("encode");
    let section = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&section);
    assert!(!section.contains("__HWP"), "internal marker leaked: {sub}");
    assert!(sub.contains(tab), "tab attributes must survive: {sub}");
    assert_eq!(memo_body_text(&decode_validated(&out)), "쇠부리야\t여기가 메모야");
}

// 이것을 실패시키는 것: 단일 pass 회귀 — 메모 안 하이퍼링크(HWPHL 전체-run
// 마커)도 메모 조각 안에 숨어 있다.
#[test]
fn memo_body_hyperlink_is_emitted() {
    let link = Run::control(
        Control::Hyperlink { text: "LINK".to_string(), url: "https://example.com".to_string() },
        CharShapeIndex::new(0),
    );
    let memo = Control::memo(vec![Paragraph::with_runs(vec![link], ParaShapeIndex::new(0))]);
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::text("본문", CharShapeIndex::new(0)),
                Run::control(memo, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&xml);
    assert!(!xml.contains("__HWP"), "internal marker leaked: {sub}");
    assert!(sub.contains(r#"type="HYPERLINK""#), "memo must carry the hyperlink field: {sub}");
    assert!(sub.contains("LINK"), "hyperlink text must be emitted: {sub}");
}

// 이것을 실패시키는 것: 단일 pass 회귀 — 묶음 개체의 자식 글상자 조각은
// `encode_group_to_xml` 이 문자열로 만들어 HWPGRP 조각 안에 넣는다.
#[test]
fn group_text_box_tab_is_emitted() {
    let hu = |v: i32| HwpUnit::new(v).expect("unit");
    let text_box = Control::text_box(
        vec![Paragraph::with_runs(
            vec![Run::text("A\tB", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        hu(8000),
        hu(4000),
    );
    let group = Control::Group {
        children: vec![text_box],
        width: hu(8000),
        height: hu(4000),
        placement: None,
        inst_id: None,
    };
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(group, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    assert!(!xml.contains("__HWP"), "internal marker leaked: {xml}");
    let container = &xml[xml.find("<hp:container").expect("container")..];
    assert!(container.contains("<hp:t>A<hp:tab"), "group text box must carry the tab: {container}");
}

/// 3단 중첩: 메모 → 메모 → `"A\tB"` (HWPME → HWPME → HWPTXT). 2단 테스트만으로는
/// "pass 2회 고정" 루프가 통과한다.
// 이것을 실패시키는 것: `apply_run_xml_replacements` 의 루프를 pass 2회로 제한하기
// — 가장 안쪽 HWPTXT 가 남아 안전망 오류로 인코드가 실패한다.
#[test]
fn memo_inside_memo_tab_is_emitted() {
    let inner = Control::memo(vec![Paragraph::with_runs(
        vec![Run::text("A\tB", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )]);
    let outer = Control::memo(vec![Paragraph::with_runs(
        vec![Run::control(inner, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )]);
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::text("본문", CharShapeIndex::new(0)),
                Run::control(outer, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    assert!(!xml.contains("__HWP"), "internal marker leaked: {xml}");
    assert_eq!(xml.matches(r#"type="MEMO""#).count(), 2, "both memos emitted: {xml}");
    let inner_sub = &xml[xml.rfind("<hp:subList").expect("inner subList")..];
    assert!(inner_sub.contains("<hp:t>A<hp:tab"), "innermost memo must carry the tab: {inner_sub}");
}
