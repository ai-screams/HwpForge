use hwpforge_blueprint::{registry::StyleRegistry, template::Template};
use hwpforge_core::{Document, ImageStore, PageSettings, Section};
use hwpforge_foundation::FontId;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
#[path = "support/document.rs"]
mod document;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

fn registry(font: &str, latin: Option<&str>, east: Option<&str>) -> StyleRegistry {
    let mut yaml = format!("meta:\n  name: fonts\nstyles:\n  body:\n    char_shape:\n      font: {font}\n      size: 10pt\n");
    if let Some(value) = latin {
        yaml.push_str(&format!("      font_latin: {value}\n"));
    }
    if let Some(value) = east {
        yaml.push_str(&format!("      font_east_asian: {value}\n"));
    }
    StyleRegistry::from_template(&Template::from_yaml(&yaml).unwrap()).unwrap()
}

fn header(registry: &StyleRegistry) -> xml::Element {
    let store = HwpxStyleStore::from_registry(registry).unwrap();
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![document::text_para("한글 Latin")],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    xml::Element::parse(&package::part(&bytes, "Contents/header.xml"))
}

fn refs(header: &xml::Element, index: usize) -> Vec<&str> {
    let font = header.find("hh:charProperties").children[index].child("hh:fontRef");
    ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"]
        .map(|key| font.attr(key).unwrap())
        .to_vec()
}

#[test]
fn font_slots_map_to_exact_language_groups() {
    // 이것을 실패시키는 것: 각 그룹에 기본 글꼴을 복제하거나 한 칸의 fallback을 다른 칸에서 찾기.
    for (latin, east, expected) in [
        (None, None, ["0", "0", "0", "0", "0", "0", "0"]),
        (Some("Latin"), None, ["0", "1", "0", "0", "0", "0", "0"]),
        (None, Some("East"), ["1", "0", "1", "1", "0", "0", "0"]),
        (Some("Latin"), Some("East"), ["2", "1", "2", "2", "0", "0", "0"]),
    ] {
        let h = header(&registry("Base", latin, east));
        assert_eq!(refs(&h, 7), expected);
    }
}

#[test]
fn every_language_table_lists_base_then_slot_fonts() {
    // 이것을 실패시키는 것: 칸 글꼴을 일부 언어 표에만 넣거나 등록 순서와 다른 순서로 표를 쓰기.
    let h = header(&registry("Base", Some("Latin"), Some("East")));
    let fontfaces = h.find("hh:fontfaces");
    assert_eq!(fontfaces.attr("itemCnt"), Some("7"));
    let tables: Vec<_> = fontfaces
        .children
        .iter()
        .map(|face| {
            assert_eq!(face.name, "hh:fontface");
            let fonts: Vec<_> = face
                .children
                .iter()
                .map(|font| (font.attr("id").unwrap(), font.attr("face").unwrap()))
                .collect();
            (face.attr("lang").unwrap(), face.attr("fontCnt").unwrap(), fonts)
        })
        .collect();
    let fonts = vec![("0", "Base"), ("1", "Latin"), ("2", "East")];
    assert_eq!(
        tables,
        ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"]
            .map(|lang| (lang, "3", fonts.clone()))
            .to_vec()
    );
}

#[test]
fn unused_registered_font_remains_a_default_family() {
    // 이것을 실패시키는 것: 기본 font로 쓰이는 이름만 가족 후보로 남기기.
    let mut r = registry("B", None, None);
    r.fonts = vec![FontId::new("A").unwrap(), FontId::new("B").unwrap()];
    let h = header(&r);
    assert_eq!(refs(&h, 0), ["1"; 7]);
    assert_eq!(refs(&h, 1), ["0"; 7]);
    assert_eq!(refs(&h, 7), ["1"; 7]);
}

#[test]
fn slot_only_fonts_do_not_create_a_second_default_family() {
    // 이것을 실패시키는 것: fonts.len()으로 바탕을 고르거나 한 종류의 칸만 제외하기.
    for (latin, east) in [(Some("B"), None), (None, Some("B")), (Some("B"), Some("C"))] {
        let r = registry("A", latin, east);
        let h = header(&r);
        for index in 0..7 {
            assert_eq!(refs(&h, index), ["0"; 7]);
        }
    }
}

#[test]
fn default_families_keep_original_indices_after_filtering_and_json() {
    // 이것을 실패시키는 것: 돋움을 0으로 고정하거나 필터된 후보 위치를 글꼴 인덱스로 쓰기.
    let mut r = registry("Batang", Some("Arial"), None);
    r.fonts = vec![FontId::new("Arial").unwrap(), FontId::new("Batang").unwrap()];
    let r: StyleRegistry = serde_json::from_value(serde_json::to_value(r).unwrap()).unwrap();
    let h = header(&r);
    for index in 0..7 {
        assert_eq!(refs(&h, index), ["1"; 7]);
    }
    assert_eq!(refs(&h, 7), ["1", "0", "1", "1", "1", "1", "1"]);
}

#[test]
fn a_font_used_as_base_and_slot_stays_a_default_candidate() {
    // 이것을 실패시키는 것: 기본 font 사용 여부와 무관하게 칸에 등장한 글꼴을 모두 제외하기.
    let mut r = registry("B", Some("B"), Some("B"));
    r.fonts = vec![FontId::new("A").unwrap(), FontId::new("B").unwrap()];
    let h = header(&r);
    assert_eq!(refs(&h, 0), ["1"; 7]);
    assert_eq!(refs(&h, 1), ["0"; 7]);
}

#[test]
fn missing_fonts_still_fall_back_to_zero() {
    // 이것을 실패시키는 것: 미등록 글꼴의 0번 대체를 해석된 기본 가족 인덱스로 바꾸기.
    let mut r = registry("Batang", Some("Arial"), Some("MissingEast"));
    r.fonts = vec![FontId::new("Arial").unwrap(), FontId::new("Batang").unwrap()];
    let h = header(&r);
    assert_eq!(refs(&h, 7), ["0", "0", "0", "0", "1", "1", "1"]);
    r.char_shapes[0].font = "MissingBase".into();
    assert_eq!(refs(&header(&r), 7), ["0"; 7]);
    r.fonts.clear();
    assert_eq!(refs(&header(&r), 7), ["0"; 7]);
}
