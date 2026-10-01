//! Lossless Markdown 왕복에서 XML 엔티티·문자 참조가 글자로 복원되는지 잠근다 (#197).

use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

fn doc_with_text(text: &str) -> Document {
    let mut draft = Document::new();
    draft.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    draft
}

fn first_text(doc: &Document) -> String {
    doc.sections()[0].paragraphs[0].runs.iter().filter_map(|r| r.content.plain_text()).collect()
}

fn roundtrip(text: &str) -> String {
    let md = MdEncoder::encode_lossless(&doc_with_text(text).validate().unwrap()).unwrap();
    first_text(&MdDecoder::decode_lossless(&md).unwrap())
}

/// 본문 한 줄만 `raw` 로 바꾼 lossless Markdown 을 만든다.
fn md_with_raw_body(text: &str, raw: &str) -> String {
    let md = MdEncoder::encode_lossless(&doc_with_text(text).validate().unwrap()).unwrap();
    assert!(md.contains(text), "marker text must appear verbatim: {md}");
    md.replacen(text, raw, 1)
}

// 이것을 실패시키는 것: lossless.rs 의 `Event::GeneralRef` 처리를 `=> {}` (무시) 로 되돌림
#[test]
fn roundtrip_preserves_ampersand_and_angle_brackets() {
    assert_eq!(roundtrip("A & B < C 문단"), "A & B < C 문단");
}

// 이것을 실패시키는 것: 이름 엔티티 중 `gt` 분기 또는 `quot`·`apos` 분기를 지움
#[test]
fn roundtrip_preserves_gt_and_quotes() {
    assert_eq!(roundtrip("x > y \"q\" 'a'"), "x > y \"q\" 'a'");
}

// 이것을 실패시키는 것: 숫자 참조 해석(`resolve_char_ref`) 분기를 지움
#[test]
fn numeric_char_refs_decimal_and_hex_resolve() {
    let dec = md_with_raw_body("MARK", "it&#39;s");
    assert_eq!(first_text(&MdDecoder::decode_lossless(&dec).unwrap()), "it's");
    let hex = md_with_raw_body("MARK", "it&#x27;s &#xAC00;");
    assert_eq!(first_text(&MdDecoder::decode_lossless(&hex).unwrap()), "it's 가");
}

// 이것을 실패시키는 것: 알 수 없는 이름의 `_ => Err(..)` 를 `_ => Ok('?')` 로 바꿈
#[test]
fn unknown_entity_is_rejected() {
    let md = md_with_raw_body("MARK", "a&nbsp;b");
    let err = MdDecoder::decode_lossless(&md).unwrap_err();
    assert!(
        matches!(&err, MdError::LosslessParse { detail } if detail.contains("&nbsp;")),
        "{err}"
    );
}

// 이것을 실패시키는 것: 숫자 참조 실패를 `.or(Some('\u{FFFD}'))` 로 대체 글자 삼킴
#[test]
fn invalid_numeric_ref_is_rejected() {
    for raw in ["a&#xD800;b", "a&#1114112;b", "a&#xZZ;b"] {
        let md = md_with_raw_body("MARK", raw);
        let err = MdDecoder::decode_lossless(&md).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }), "{raw}: {err}");
    }
}

fn link_doc(url: &str, text: &str) -> Document {
    let mut draft = Document::new();
    draft.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(
                hwpforge_core::Control::Hyperlink { text: text.to_string(), url: url.to_string() },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    draft
}

fn first_link(doc: &Document) -> (String, String) {
    match &doc.sections()[0].paragraphs[0].runs[0].content {
        hwpforge_core::RunContent::Control(c) => match c.as_ref() {
            hwpforge_core::Control::Hyperlink { text, url } => (text.clone(), url.clone()),
            other => panic!("expected hyperlink, got {other:?}"),
        },
        other => panic!("expected control, got {other:?}"),
    }
}

// 속성값(href)은 normalized_value 가 풀고, 본문(<a> 텍스트)은 GeneralRef 경로가 푼다.
// 이것을 실패시키는 것: GeneralRef 처리를 `=> {}` 로 되돌림(텍스트 `&` 소실) 또는 attr_value 가 `normalized_value` 대신 raw `attr.value` 를 씀
#[test]
fn hyperlink_attribute_and_text_entities_roundtrip() {
    let md =
        MdEncoder::encode_lossless(&link_doc("http://x/?a=1&b=2", "R&D <1>").validate().unwrap())
            .unwrap();
    let back = MdDecoder::decode_lossless(&md).unwrap();
    assert_eq!(first_link(&back), ("R&D <1>".to_string(), "http://x/?a=1&b=2".to_string()));
}

// 이것을 실패시키는 것: attr_value 가 `normalized_value` 오류를 `unwrap_or_default` 로 삼킴
#[test]
fn unknown_entity_in_attribute_is_rejected() {
    let md =
        MdEncoder::encode_lossless(&link_doc("http://x/MARK", "t").validate().unwrap()).unwrap();
    let md = md.replacen("MARK", "a&nbsp;b", 1);
    assert!(matches!(MdDecoder::decode_lossless(&md), Err(MdError::LosslessParse { .. })));
}
