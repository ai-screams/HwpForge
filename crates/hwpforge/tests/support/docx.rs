//! Small synthetic DOCX packages shared by import contract tests.
use std::io::{Cursor, Write};

pub fn package(body: &str) -> Vec<u8> {
    build_package(body, "", false)
}

/// Adds complete source defaults so only the requested content losses are reported.
#[allow(dead_code)] // The diagnostic inventory also includes this helper but uses raw packages.
pub fn package_with_defaults(body: &str, relationships: &str) -> Vec<u8> {
    build_package(body, relationships, true)
}

fn build_package(body: &str, relationships: &str, defaults: bool) -> Vec<u8> {
    // Explicit source defaults isolate transport diagnostics from formatting fallbacks.
    let content_types = if defaults {
        "<Override PartName='/styles.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml'/>"
    } else {
        ""
    };
    let styles_relation = if defaults {
        "<Relationship Id='styles' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles' Target='styles.xml'/>"
    } else {
        ""
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let document = format!("<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body>{body}</w:body></w:document>");
    for (name, xml) in [
        ("[Content_Types].xml", "<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/></Types>"),
        ("_rels/.rels", "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='body.xml'/></Relationships>"),
        ("body.xml", document.as_str()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        let xml = if name == "[Content_Types].xml" {
            xml.replace("</Types>", &format!("{content_types}</Types>"))
        } else {
            xml.to_owned()
        };
        zip.write_all(xml.as_bytes()).unwrap();
    }
    if defaults || !relationships.is_empty() {
        zip.start_file("_rels/body.xml.rels", zip::write::SimpleFileOptions::default()).unwrap();
        write!(zip, "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{styles_relation}{relationships}</Relationships>").unwrap();
    }
    if defaults {
        zip.start_file("styles.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all("<w:styles xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='함초롬바탕' w:hAnsi='함초롬바탕' w:eastAsia='함초롬바탕'/><w:sz w:val='20'/><w:color w:val='000000'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before='0' w:after='0' w:line='240' w:lineRule='auto'/><w:ind w:left='0' w:right='0' w:firstLine='0'/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>".as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
