#![allow(dead_code)]

use hwpforge_core::image::ImageStore;
use hwpforge_core::{Document, Draft};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

pub(crate) fn dotum_styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

pub(crate) fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

pub(crate) fn batang_styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

pub(crate) fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &styles, &ImageStore::new()).expect("encode")
}

/// Compare a supported direct-table cell edit with the public preserving patch API.
pub(crate) fn assert_cell_patch(
    base: &[u8],
    output: &[u8],
    section: usize,
    cell: usize,
    text: &str,
) {
    use hwpforge_core::RunContent;
    use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxPatcher};
    use std::{
        collections::BTreeMap,
        io::{Cursor, Read},
    };
    fn payloads(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut entry = zip.by_index(i).unwrap();
                let name = entry.name().unwrap().to_string();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                (name, data)
            })
            .collect()
    }
    let decoded = HwpxDecoder::decode(base).unwrap();
    let source = &decoded.document.sections()[section];
    let preservation = HwpxPatcher::export_section_preservation(base, section, source).unwrap();
    let mut replacement = source.clone();
    let table = replacement
        .paragraphs
        .iter_mut()
        .flat_map(|p| &mut p.runs)
        .find_map(|run| match &mut run.content {
            RunContent::Table(table) => Some(table),
            _ => None,
        })
        .expect("direct table");
    table.rows[0].cells[cell].paragraphs[0].runs[0].content = RunContent::Text(text.into());
    let patched = HwpxPatcher::patch_section_preserving(
        base,
        section,
        &replacement,
        None,
        Some(&preservation),
    )
    .unwrap();
    assert_eq!(payloads(output), payloads(&patched));
}
