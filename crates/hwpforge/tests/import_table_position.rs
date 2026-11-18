#![cfg(feature = "ops-docx")]
use hwpforge::ops::convert_docx;
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use std::io::{Cursor, Read, Write};

fn package(body: &str, mode: &str) -> Vec<u8> {
    let bytes = docx::package_with_numbering_and_settings(body, "", Some("720"));
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut file = input.by_index(i).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        if file.name().unwrap() == "options.xml" {
            text = format!("<w:settings xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:compat><w:compatSetting w:name='compatibilityMode' w:uri='http://schemas.microsoft.com/office/word' w:val='{mode}'/></w:compat></w:settings>");
        }
        output.start_file(file.name().unwrap(), zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(text.as_bytes()).unwrap();
    }
    output.finish().unwrap().into_inner()
}
// 이것을 실패시키는 것: signed offset 소실, 중첩 표에 최상위 정렬 적용.
#[test]
fn imported_table_horizontal_placement_xml() {
    for (mode, properties, align, offset) in [
        ("14", "", "LEFT", "4294966756"),
        ("15", "<w:tblInd w:w='-283'/>", "LEFT", "4294965881"),
        ("15", "<w:jc w:val='center'/><w:tblInd w:w='567'/>", "CENTER", "0"),
        ("14", "<w:jc w:val='right'/>", "RIGHT", "4294966756"),
    ] {
        let inner = "<w:tbl><w:tblPr><w:jc w:val='center'/></w:tblPr><w:tblGrid><w:gridCol w:w='1000'/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>";
        let body = format!("<w:tbl><w:tblPr>{properties}</w:tblPr><w:tblGrid><w:gridCol w:w='4000'/></w:tblGrid><w:tr><w:tc>{inner}</w:tc></w:tr></w:tbl>");
        let bytes = convert_docx(&package(&body, mode)).unwrap().bytes;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
        let root = xml::Element::parse(&text);
        let tables = root.descendants("hp:tbl");
        assert_eq!(tables.len(), 2);
        for (table, (align, offset)) in tables.iter().zip([(align, offset), ("LEFT", "0")]) {
            let position = table.child("hp:pos");
            assert_eq!(position.attr("horzRelTo"), Some("COLUMN"));
            assert_eq!(position.attr("horzAlign"), Some(align));
            assert_eq!(position.attr("horzOffset"), Some(offset));
            assert_eq!(position.attr("vertAlign"), Some("TOP"));
            assert_eq!(position.attr("vertOffset"), Some("0"));
            assert_eq!(position.attr("treatAsChar"), Some("0"));
        }
    }
}
