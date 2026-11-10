//! Generated preview-bearing document shared by binding boundary tests.
#![allow(dead_code)]
use hwpforge::{core, foundation, hwpx};
use std::io::{Cursor, Read, Write};

// 이것을 실패시키는 것: 합성 container에 Preview rootfile을 추가하는 분기 생략.
pub(crate) fn base() -> Vec<u8> {
    let run = core::Run {
        char_shape_id: foundation::CharShapeIndex::new(0),
        content: core::RunContent::Control(Box::new(core::Control::Field {
            field_type: foundation::FieldType::ClickHere,
            hint_text: Some("old".into()),
            help_text: None,
            name: Some("name".into()),
            display_text: "old".into(),
        })),
    };
    let mut document = core::Document::new();
    document.add_section(core::Section::with_paragraphs(
        vec![core::Paragraph::with_runs(vec![run], foundation::ParaShapeIndex::new(0))],
        core::PageSettings::a4(),
    ));
    let bytes = hwpx::HwpxEncoder::encode(
        &document.validate().unwrap(),
        &hwpx::style_store_for_preset("default").unwrap(),
        &Default::default(),
    )
    .unwrap();
    let mut original = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..original.len() {
        let mut entry = original.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "META-INF/container.xml" {
            bytes = String::from_utf8(bytes).unwrap().replace("</ocf:rootfiles>",
                "<ocf:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/></ocf:rootfiles>").into_bytes();
        }
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    for path in ["Preview/PrvText.txt", "Preview/PrvImage.png"] {
        output.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(b"derived").unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let mut package = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut container = String::new();
    package.by_name("META-INF/container.xml").unwrap().read_to_string(&mut container).unwrap();
    assert!(container.contains("Preview/PrvText.txt"));
    drop(package);
    bytes
}

pub(crate) fn patch_json(bytes: &[u8]) -> String {
    let mut exported =
        hwpx::HwpxPatcher::export_section_for_edit(bytes, 0, false).unwrap().exported;
    let sections = [exported.section.clone()];
    let inventory = core::field_inventory::FieldInventory::from_sections(&sections);
    assert!(core::field_inventory::FieldInventory::replace_values(
        std::slice::from_mut(&mut exported.section),
        &[(&inventory.fields()[0], "new")]
    ));
    serde_json::to_string(&exported).unwrap()
}

pub(crate) fn assert_removed(bytes: &[u8]) {
    let mut package = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(package.by_name("Preview/PrvText.txt").is_err());
    assert!(package.by_name("Preview/PrvImage.png").is_err());
    let mut xml = String::new();
    package.by_name("META-INF/container.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert!(!xml.contains("Preview/"));
}

pub(crate) fn assert_warning(value: &serde_json::Value) {
    let warnings: Vec<_> = value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["code"] == "PREVIEW_REMOVED")
        .collect();
    assert_eq!(warnings.len(), 1, "{value}");
    assert!(warnings[0].get("parts").is_none());
}

/// A preview-bearing form followed by two independently addressable plain paragraphs.
pub(crate) fn paragraph_base() -> Vec<u8> {
    let mut original = zip::ZipArchive::new(Cursor::new(base())).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..original.len() {
        let mut entry = original.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "Contents/section0.xml" {
            bytes=String::from_utf8(bytes).unwrap().replace("</hs:sec>",r#"<hp:p id="701" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>삭제할 문단</hp:t></hp:run></hp:p><hp:p id="007" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>유지할 문단</hp:t></hp:run></hp:p></hs:sec>"#).into_bytes();
        }
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}
