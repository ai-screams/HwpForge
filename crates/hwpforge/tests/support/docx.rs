//! Small synthetic DOCX packages shared by import contract tests.
use std::io::{Cursor, Write};

pub fn package(body: &str) -> Vec<u8> {
    package_with_relationships(body, "")
}

pub fn package_with_relationships(body: &str, relationships: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let document = format!("<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body>{body}</w:body></w:document>");
    for (name, xml) in [
        ("[Content_Types].xml", "<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/></Types>"),
        ("_rels/.rels", "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='body.xml'/></Relationships>"),
        ("body.xml", document.as_str()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    if !relationships.is_empty() {
        zip.start_file("_rels/body.xml.rels", zip::write::SimpleFileOptions::default()).unwrap();
        write!(zip, "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{relationships}</Relationships>").unwrap();
    }
    zip.finish().unwrap().into_inner()
}
