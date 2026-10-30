#![allow(dead_code)]

use std::io::{Cursor, Read, Write};
const SECTION0: &str = "Contents/section0.xml";

pub(crate) fn section_xml_unchecked(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// Replaces `from` with `to` in `Contents/section0.xml` — the only way to get
/// a character the encoder refuses into a base package.
pub(crate) fn splice(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    splice_part(bytes, SECTION0, from, to)
}

/// Replaces `from` with `to` in the package part `part` — the way to get a
/// wire value the encoder never writes into a base package.
pub(crate) fn splice_part(bytes: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut entry = zin.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let method = entry.compression();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == part {
            let text = String::from_utf8(data).expect("utf-8");
            assert!(text.contains(from), "{from:?} not in {text}");
            data = text.replace(from, to).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method))
            .expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

pub(crate) fn section_xml_utf8(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut entry = zip.by_name(SECTION0).expect("section0");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("utf-8");
    text
}

pub(crate) fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}")).read_to_string(&mut out).unwrap();
    out
}

pub(crate) fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

pub(crate) fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        let body = if name == "Contents/section0.xml" { xml.as_bytes() } else { &data };
        out.write_all(body).expect("write");
    }
    out.finish().expect("finish").into_inner()
}
