#![allow(dead_code)]

use std::io::{Cursor, Read};

pub(crate) fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}
