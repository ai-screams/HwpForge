//! Registry defaults must refer to fonts actually emitted in each language group.
use std::collections::BTreeMap;

use hwpforge_blueprint::{registry::StyleRegistry, template::Template};
use hwpforge_core::{Document, Draft, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    default_styles::HancomStyleSet, HwpxEncoder, HwpxRegistryBridge, HwpxStyleStore, PackageReader,
};
use serde::Deserialize;

fn registry(fonts: &[&str]) -> StyleRegistry {
    let mut yaml = String::from("meta:\n  name: font_regression\nstyles:\n");
    for (index, font) in fonts.iter().enumerate() {
        yaml.push_str(&format!(
            "  sample{index}:\n    char_shape:\n      font: {font}\n      size: 10pt\n"
        ));
    }
    StyleRegistry::from_template(&Template::from_yaml(&yaml).unwrap()).unwrap()
}

fn document(char_id: usize, para_id: usize) -> Document<Draft> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가상의 문장입니다. Example 123.", CharShapeIndex::new(char_id))],
            ParaShapeIndex::new(para_id),
        )],
        PageSettings::a4(),
    ));
    doc
}

fn encode_header(doc: Document<Draft>, store: &HwpxStyleStore) -> String {
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), store, &ImageStore::new())
        .expect("registry defaults must form an encodable document");
    PackageReader::new(&bytes).unwrap().read_header_xml().unwrap()
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "refList")]
    refs: References,
}
#[derive(Deserialize)]
struct References {
    fontfaces: FontFaces,
    #[serde(rename = "charProperties")]
    chars: CharProperties,
}
#[derive(Deserialize)]
struct FontFaces {
    #[serde(rename = "fontface")]
    groups: Vec<FontGroup>,
}
#[derive(Deserialize)]
struct FontGroup {
    #[serde(rename = "@lang")]
    lang: String,
    font: Vec<Font>,
}
#[derive(Deserialize)]
struct Font {
    #[serde(rename = "@id")]
    id: u32,
    #[serde(rename = "@face")]
    face: String,
}
#[derive(Deserialize)]
struct CharProperties {
    #[serde(rename = "charPr")]
    shapes: Vec<CharShape>,
}
#[derive(Deserialize)]
struct CharShape {
    #[serde(rename = "@id")]
    id: u32,
    #[serde(rename = "fontRef")]
    fonts: BTreeMap<String, u32>,
}

// Inspect the encoded XML, rather than duplicating the store's resolution logic.
fn assert_fonts(header: &str, expected_faces: &[&str]) -> Header {
    // Only the encoder's small synthetic test output is parsed here; the
    // production decoder's bounded XML entry point is crate-private.
    #[allow(clippy::disallowed_methods)]
    let header: Header = quick_xml::de::from_str(header).unwrap();
    let langs = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
    assert_eq!(header.refs.fontfaces.groups.len(), langs.len());
    assert!(!header.refs.chars.shapes.is_empty());
    for shape in &header.refs.chars.shapes {
        assert_eq!(shape.fonts.len(), langs.len());
        for lang in langs {
            let group = header.refs.fontfaces.groups.iter().find(|g| g.lang == lang).unwrap();
            assert_eq!(group.font.len(), expected_faces.len());
            let id = shape.fonts[&format!("@{}", lang.to_lowercase())];
            let font = group.font.iter().find(|font| font.id == id).unwrap_or_else(|| {
                panic!("charPr {} font {lang} reference {id} is absent", shape.id)
            });
            assert_eq!(font.face, expected_faces[id as usize]);
        }
    }
    header
}

/// 이것을 실패시키는 것: 기본 바탕 글꼴을 다시 ID 1로 고정,
/// 또는 단일 글꼴 이름을 함초롬바탕으로 교체.
#[test]
fn single_font_template_encodes_with_registered_font_refs() {
    let registry = registry(&["한컴바탕"]);
    let store = HwpxStyleStore::from_registry(&registry).unwrap();
    let style = store.iter_styles().find(|s| s.name == "sample0").unwrap();
    let header = encode_header(
        document(style.char_pr_id_ref as usize, style.para_pr_id_ref as usize),
        &store,
    );
    assert_fonts(&header, &["한컴바탕"]);

    // The bridge uses the same construction boundary, for every style set.
    for set in [HancomStyleSet::Classic, HancomStyleSet::Modern, HancomStyleSet::Latest] {
        let bridge = HwpxRegistryBridge::from_registry_with(&registry, set).unwrap();
        let entry = &registry.style_entries["sample0"];
        let doc = bridge
            .rebind_draft_document(document(entry.char_shape_id.get(), entry.para_shape_id.get()))
            .unwrap();
        assert_fonts(&encode_header(doc, bridge.style_store()), &["한컴바탕"]);
    }
}

/// 이것을 실패시키는 것: 글꼴이 정확히 하나일 때만 ID 0으로 해석.
#[test]
fn empty_registry_defaults_encode_with_hancom_font() {
    let registry = StyleRegistry::with_fonts(Vec::new());
    for set in [HancomStyleSet::Classic, HancomStyleSet::Modern, HancomStyleSet::Latest] {
        let store = HwpxStyleStore::from_registry_with(&registry, set).unwrap();
        assert_fonts(&encode_header(document(0, 0), &store), &["함초롬바탕"]);
    }
}

/// 이것을 실패시키는 것: 글꼴이 둘일 때도 기본 바탕 글꼴을 ID 0으로 변경.
#[test]
fn multiple_font_templates_keep_default_font_positions() {
    for fonts in [&["한컴바탕", "맑은 고딕"][..], &["한컴바탕", "맑은 고딕", "돋움"][..]]
    {
        let registry = registry(fonts);
        let faces: Vec<_> = registry.fonts.iter().map(|f| f.as_str()).collect();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let header = assert_fonts(&encode_header(document(0, 0), &store), &faces);
        for (shape, expected) in header.refs.chars.shapes.iter().zip([1, 0, 0, 1, 0, 0, 0]) {
            assert!(shape.fonts.values().all(|&id| id == expected), "charPr {}", shape.id);
        }
    }
}
