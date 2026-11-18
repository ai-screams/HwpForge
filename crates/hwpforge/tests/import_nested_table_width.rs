#![cfg(feature = "ops-docx")]
use hwpforge::ops::convert_docx;
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use std::io::{Cursor, Read};
// 이것을 실패시키는 것: 중첩 grid를 컨테이너로 정규화하거나 출력 칸 폭을 원래 값과 다르게 방출.
#[test]
fn imported_nested_table_width_xml() {
    for (kind, width, cell) in [
        ("auto", "17010", "8505"),
        ("nil", "17010", "8505"),
        ("pct", "10260", "5130"),
        ("unknown", "10260", "5130"),
    ] {
        let body=format!("<w:tbl><w:tblPr><w:tblW w:w='2268'/></w:tblPr><w:tr><w:tc><w:tbl><w:tblPr><w:tblW w:type='{kind}' w:w='5000'/></w:tblPr><w:tblGrid><w:gridCol w:w='1701'/><w:gridCol w:w='1701'/></w:tblGrid><w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>");
        let result =
            convert_docx(&docx::package_with_numbering_and_settings(&body, "", Some("720")))
                .unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(result.bytes)).unwrap();
        let mut text = String::new();
        zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
        let root = xml::Element::parse(&text);
        let tables = root.descendants("hp:tbl");
        let t = tables[1];
        assert_eq!(t.child("hp:sz").attr("width"), Some(width));
        for c in t.descendants("hp:cellSz") {
            assert_eq!(c.attr("width"), Some(cell));
        }
    }
}
