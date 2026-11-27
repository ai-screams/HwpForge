use hwpforge_blueprint::{
    builtins::builtin_default,
    inheritance::resolve_template,
    markdown::{apply_markdown_heading_levels, apply_markdown_list_levels},
    registry::StyleRegistry,
    style::ParaShape,
    template::Template,
};
use hwpforge_core::{Document, ImageStore, PageSettings, Section};
use hwpforge_foundation::{CharShapeIndex, FontId, FontIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    style_store_for_preset, HancomStyleSet, HwpxEncoder, HwpxStyleStore, PackageReader,
};
use std::collections::HashMap;
#[path = "support/document.rs"]
mod document;
#[path = "support/xml.rs"]
mod xml;
use xml::Element;

const THIRD_BODY: &str = "meta:\n  name: third_body\nstyles:\n  title:\n    char_shape: { font: 맑은 고딕, size: 20pt }\n  code:\n    char_shape: { font: D2Coding, size: 10pt }\n  body:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  heading1:\n    char_shape: { font: 한컴바탕, size: 16pt }\n";
const RESERVED_STYLES: &str = "meta:\n  name: reserved_names\nstyles:\n  body:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  __hwpforge_md_footnote:\n    char_shape: { font: 한컴바탕, size: 9pt }\n  __hwpforge_md_endnote:\n    char_shape: { font: 한컴바탕, size: 9pt }\n  __hwpforge_md_list_continuation_level_0:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  __hwpforge_md_code_continuation_level_0:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  __hwpforge_md_list_plain:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  heading6:\n    char_shape: { font: 한컴바탕, size: 12pt }\nmarkdown_mapping:\n  heading1: __hwpforge_md_footnote\n  heading2: __hwpforge_md_endnote\n  heading3: __hwpforge_md_list_continuation_level_0\n  heading4: __hwpforge_md_code_continuation_level_0\n  heading5: __hwpforge_md_list_plain\n";
const LOOKALIKE_STYLES: &str = "meta:\n  name: lookalike_names\nstyles:\n  body:\n    char_shape: { font: 한컴바탕, size: 10pt }\n  __hwpforge_md_x:\n    char_shape: { font: 한컴바탕, size: 16pt }\n  __hwpforge_md_list_continuation_level_10:\n    char_shape: { font: 한컴바탕, size: 14pt }\nmarkdown_mapping:\n  heading1: __hwpforge_md_x\n  heading2: __hwpforge_md_list_continuation_level_10\n";
const SETS: [HancomStyleSet; 3] =
    [HancomStyleSet::Modern, HancomStyleSet::Classic, HancomStyleSet::Latest];

fn marked(template: &Template) -> StyleRegistry {
    let resolved = resolve_template(template, &HashMap::new()).unwrap();
    let mut registry = StyleRegistry::from_template(&resolved).unwrap();
    apply_markdown_heading_levels(&mut registry, resolved.markdown_mapping.as_ref().unwrap())
        .unwrap();
    registry
}

fn third_body() -> StyleRegistry {
    marked(&Template::from_yaml(THIRD_BODY).unwrap())
}

fn header(store: &HwpxStyleStore) -> Element {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![document::text_para("본문")],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), store, &ImageStore::new()).unwrap();
    Element::parse(&PackageReader::new(&bytes).unwrap().read_header_xml().unwrap())
}

fn registry_header(registry: &StyleRegistry, set: HancomStyleSet) -> Element {
    header(&HwpxStyleStore::from_registry_with(registry, set).unwrap())
}

fn assert_slot(header: &Element, index: usize, expected: &str) {
    let shapes = header.descendants("hh:charPr");
    let fonts = shapes[index].child("hh:fontRef");
    for lang in ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"] {
        assert_eq!(fonts.attr(lang), Some(expected), "charPr {index} {lang}");
    }
}

fn assert_body_slots(header: &Element, expected: &str) {
    for index in [0, 3] {
        assert_slot(header, index, expected);
    }
}

fn face<'a>(header: &'a Element, id: &str) -> &'a str {
    header
        .descendants("hh:fontface")
        .into_iter()
        .find(|n| n.attr("lang") == Some("HANGUL"))
        .unwrap()
        .children
        .iter()
        .find(|n| n.attr("id") == Some(id))
        .unwrap()
        .attr("face")
        .unwrap()
}

fn assert_next(header: &Element, pairs: &[(&str, &str)]) {
    let styles = header.descendants("hh:style");
    for (name, next) in pairs {
        let style = styles.iter().find(|s| s.attr("name") == Some(*name)).unwrap();
        assert_eq!(style.attr("nextStyleIDRef"), Some(*next), "{name}");
    }
}

// 이것을 실패시키는 것: 바탕 칸을 글꼴 수로 결정하거나 0으로 고정, 다음 스타일에 기본 개수만 출력.
#[test]
fn builtin_text_styles_use_the_body_font_slot() {
    for set in SETS {
        let header = registry_header(&third_body(), set);
        assert_body_slots(&header, "2");
        for index in [1, 2, 4, 5, 6] {
            assert_slot(&header, index, "0");
        }
        assert_eq!(face(&header, "2"), "한컴바탕");
        for (name, shape) in [("바탕글", "0"), ("각주", "3"), ("미주", "3")] {
            let styles = header.descendants("hh:style");
            let style = styles.iter().find(|s| s.attr("name") == Some(name)).unwrap();
            assert_eq!(style.attr("charPrIDRef"), Some(shape));
        }
        let pairs = match set {
            HancomStyleSet::Classic => {
                [("title", "18"), ("code", "19"), ("body", "20"), ("heading1", "20")]
            }
            HancomStyleSet::Modern => {
                [("title", "22"), ("code", "23"), ("body", "24"), ("heading1", "24")]
            }
            HancomStyleSet::Latest => {
                [("title", "23"), ("code", "24"), ("body", "25"), ("heading1", "25")]
            }
            _ => unreachable!("only the three declared sets are tested"),
        };
        assert_next(&header, &pairs);
    }
    let registry = marked(&Template::from_yaml("meta:\n  name: single\nstyles:\n  body:\n    char_shape: { font: 한컴바탕, size: 10pt }\n").unwrap());
    assert_body_slots(&registry_header(&registry, HancomStyleSet::Modern), "0");
}

// 이것을 실패시키는 것: 미지정 다음 스타일을 0으로, 제목도 자기로,
// 지정된 다음 스타일의 기본 개수만 22로 고정.
#[test]
fn template_styles_continue_with_themselves_and_headings_with_body() {
    let template = resolve_template(&builtin_default().unwrap(), &HashMap::new()).unwrap();
    let mut registry = marked(&template);
    apply_markdown_list_levels(&mut registry, template.markdown_mapping.as_ref().unwrap()).unwrap();
    let header = registry_header(&registry, HancomStyleSet::Modern);
    let styles = header.descendants("hh:style");
    let expected = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "0", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "22", "22", "22", "26", "27", "28", "22", "22", "22",
        "32", "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46",
        "47", "48", "49", "50", "51", "52", "53", "54",
    ];
    assert_eq!(styles.len(), expected.len());
    for (style, next) in styles.iter().zip(expected) {
        assert_eq!(style.attr("nextStyleIDRef"), Some(next), "{:?}", style.attr("name"));
    }
    let builtin_names = [
        "바탕글",
        "본문",
        "개요 1",
        "개요 2",
        "개요 3",
        "개요 4",
        "개요 5",
        "개요 6",
        "개요 7",
        "개요 8",
        "개요 9",
        "개요 10",
        "쪽 번호",
        "머리말",
        "각주",
        "미주",
        "메모",
        "차례 제목",
        "차례 1",
        "차례 2",
        "차례 3",
        "캡션",
    ];
    for (style, name) in styles[..22].iter().zip(builtin_names) {
        assert_eq!(style.attr("name"), Some(name));
    }
    let template_names = [
        "body",
        "heading1",
        "heading2",
        "heading3",
        "code",
        "blockquote",
        "list_item",
        "heading4",
        "heading5",
        "heading6",
        "numbered_item",
        "check_item",
        "__hwpforge_md_list_continuation_level_0",
        "__hwpforge_md_code_continuation_level_0",
        "__hwpforge_md_list_continuation_level_1",
        "__hwpforge_md_code_continuation_level_1",
        "__hwpforge_md_list_continuation_level_2",
        "__hwpforge_md_code_continuation_level_2",
        "__hwpforge_md_list_continuation_level_3",
        "__hwpforge_md_code_continuation_level_3",
        "__hwpforge_md_list_continuation_level_4",
        "__hwpforge_md_code_continuation_level_4",
        "__hwpforge_md_list_continuation_level_5",
        "__hwpforge_md_code_continuation_level_5",
        "__hwpforge_md_list_continuation_level_6",
        "__hwpforge_md_code_continuation_level_6",
        "__hwpforge_md_list_continuation_level_7",
        "__hwpforge_md_code_continuation_level_7",
        "__hwpforge_md_list_continuation_level_8",
        "__hwpforge_md_code_continuation_level_8",
        "__hwpforge_md_list_continuation_level_9",
        "__hwpforge_md_code_continuation_level_9",
        "__hwpforge_md_list_plain",
    ];
    for (style, name) in styles[22..].iter().zip(template_names) {
        assert_eq!(style.attr("name"), Some(name));
    }
    for (set, pairs) in [
        (HancomStyleSet::Classic, [("body", "18"), ("heading1", "18"), ("code", "22")]),
        (HancomStyleSet::Latest, [("body", "23"), ("heading1", "23"), ("code", "27")]),
    ] {
        assert_next(&registry_header(&registry, set), &pairs);
    }
}

// 이것을 실패시키는 것: 본문 표시가 없을 때 첫 항목 추정, 미지정 다음 스타일을 0으로.
#[test]
fn registries_without_a_body_style_keep_the_default_font_slot() {
    let header = header(&style_store_for_preset("default").unwrap());
    assert_body_slots(&header, "1");
    assert_eq!(face(&header, "0"), "함초롬돋움");
    assert_eq!(face(&header, "1"), "함초롬돋움");
    let styles = header.descendants("hh:style");
    let expected = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "0", "13", "14", "15", "16",
        "17", "18", "19", "20", "21",
    ];
    assert_eq!(styles.len(), expected.len());
    for (style, next) in styles.iter().zip(expected) {
        assert_eq!(style.attr("nextStyleIDRef"), Some(next));
    }
    let source = third_body();
    let mut registry = StyleRegistry::with_fonts(
        ["맑은 고딕", "D2Coding", "한컴바탕"]
            .into_iter()
            .map(|n| FontId::new(n.to_string()).unwrap())
            .collect(),
    );
    registry.char_shapes = source.char_shapes[..3].to_vec();
    registry.para_shapes = vec![ParaShape::default()];
    for (i, name) in ["title", "mono", "text"].into_iter().enumerate() {
        registry
            .register_style(
                name.into(),
                CharShapeIndex::new(i),
                ParaShapeIndex::new(0),
                FontIndex::new(i),
            )
            .unwrap();
    }
    let header = registry_header(&registry, HancomStyleSet::Modern);
    assert_body_slots(&header, "1");
    assert_next(&header, &[("title", "22"), ("mono", "23"), ("text", "24")]);
}

// 이것을 실패시키는 것: 없는 정체 조회를 첫 항목으로 대체하여 오류 무시.
#[test]
fn a_missing_body_or_next_style_is_an_error() {
    let original = serde_json::to_value(third_body()).unwrap();
    for body in [true, false] {
        let mut value = original.clone();
        if body {
            value["body_style"] = 999.into();
        } else {
            value["style_entries"]["heading1"]["next_style"] = 999.into();
        }
        let registry: StyleRegistry = serde_json::from_value(value).unwrap();
        assert!(HwpxStyleStore::from_registry(&registry).is_err(), "body={body}");
    }
}

// 이것을 실패시키는 것: 정체 번호를 항목 순번으로 직접 사용.
#[test]
fn style_identities_map_to_positions_not_to_their_numbers() {
    let mut value = serde_json::to_value(third_body()).unwrap();
    value["style_entries"]["title"]["style_ref"] = 2.into();
    value["style_entries"]["body"]["style_ref"] = 0.into();
    let registry: StyleRegistry = serde_json::from_value(value).unwrap();
    let header = registry_header(&registry, HancomStyleSet::Modern);
    assert_body_slots(&header, "0");
    assert_next(&header, &[("heading1", "22"), ("title", "22"), ("code", "23"), ("body", "24")]);
}

// 이것을 실패시키는 것: 글꼴 이름 조회 대신 항목의 font_id 사용.
#[test]
fn the_body_font_slot_follows_the_body_char_shape() {
    let mut value = serde_json::to_value(third_body()).unwrap();
    value["style_entries"]["body"]["char_shape_id"] = 999.into();
    let registry: StyleRegistry = serde_json::from_value(value).unwrap();
    assert!(HwpxStyleStore::from_registry(&registry).is_err());
    let mut value = serde_json::to_value(third_body()).unwrap();
    value["fonts"][2] = "없는 글꼴".into();
    let registry: StyleRegistry = serde_json::from_value(value).unwrap();
    let header = registry_header(&registry, HancomStyleSet::Modern);
    assert_body_slots(&header, "0");
    let styles = header.descendants("hh:style");
    let body = styles.iter().find(|s| s.attr("name") == Some("body")).unwrap();
    let char_id = body.attr("charPrIDRef").unwrap().parse::<usize>().unwrap();
    assert_slot(&header, char_id, "0");
}

// 이것을 실패시키는 것: 템플릿 생성에서 역할 표시 생략.
#[test]
fn a_template_registry_alone_gets_the_body_font_and_next_styles() {
    let template = builtin_default().unwrap();
    let registry = StyleRegistry::from_template(&template).unwrap();
    let header = registry_header(&registry, HancomStyleSet::Modern);
    let styles = header.descendants("hh:style");
    assert_eq!(styles.len(), 34);
    assert_body_slots(&header, "0");
    assert_eq!(face(&header, "0"), "한컴바탕");
    assert_eq!(face(&header, "1"), "D2Coding");
    let expected = ["22", "22", "22", "22", "26", "27", "28", "22", "22", "22", "32", "33"];
    for (style, next) in styles[22..].iter().zip(expected) {
        assert_eq!(style.attr("nextStyleIDRef"), Some(next));
    }
    let marked = registry_header(&marked(&template), HancomStyleSet::Modern);
    let marked_styles = marked.descendants("hh:style");
    assert_eq!(marked_styles.len(), 34);
    for (a, b) in styles.iter().zip(marked_styles) {
        assert_eq!(a.attr("nextStyleIDRef"), b.attr("nextStyleIDRef"));
    }
    for index in [0, 3] {
        assert_eq!(
            header.descendants("hh:charPr")[index].child("hh:fontRef").attributes,
            marked.descendants("hh:charPr")[index].child("hh:fontRef").attributes
        );
    }
}

// 이것을 실패시키는 것: 예약 이름 검사 삭제, 예약 접두사 전체 허용.
#[test]
fn a_reserved_style_shared_with_a_heading_names_itself_as_next() {
    for (yaml, names, expected, count) in [
        (
            RESERVED_STYLES,
            &[
                "body",
                "__hwpforge_md_footnote",
                "__hwpforge_md_endnote",
                "__hwpforge_md_list_continuation_level_0",
                "__hwpforge_md_code_continuation_level_0",
                "__hwpforge_md_list_plain",
                "heading6",
            ][..],
            &["22", "23", "24", "25", "26", "27", "22"][..],
            29,
        ),
        (
            LOOKALIKE_STYLES,
            &["body", "__hwpforge_md_x", "__hwpforge_md_list_continuation_level_10"][..],
            &["22", "22", "22"][..],
            25,
        ),
    ] {
        let header =
            registry_header(&marked(&Template::from_yaml(yaml).unwrap()), HancomStyleSet::Modern);
        let styles = header.descendants("hh:style");
        assert_eq!(styles.len(), count);
        for ((style, name), next) in styles[22..].iter().zip(names).zip(expected) {
            assert_eq!(style.attr("name"), Some(*name));
            assert_eq!(style.attr("nextStyleIDRef"), Some(*next), "{name}");
        }
    }
}
