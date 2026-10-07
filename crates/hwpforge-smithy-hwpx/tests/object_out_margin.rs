//! Object outer margin (S18).
//!
//! Hancom writes the space kept around a drawn object or picture on its
//! `<hp:outMargin>` (369 objects in 70 corpus documents are not 0 — the
//! Ministry of Justice bullets sit 140 HWPUNIT higher because of it). The
//! decoder did not read it and the encoder wrote 0. Inputs are repo fixtures
//! with one top-level object's margin rewritten.

use std::io::{Cursor, Read, Write};

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
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

const ODD: &str = r#"<hp:outMargin left="1" right="2" top="3" bottom="140"/>"#;

/// The fixture with the first `<hp:{tag} ` object's own outer margin (the
/// first `<hp:outMargin>` after its `<hp:pos>`) rewritten to [`ODD`].
fn with_odd_margin(path: &str, tag: &str) -> Vec<u8> {
    let source = fixture(path);
    let xml = section_xml(&source);
    let at = xml.find(&format!("<hp:{tag} ")).unwrap_or_else(|| panic!("{path}: no {tag}"));
    let pos = at + xml[at..].find("<hp:pos ").expect("pos");
    let m = pos + xml[pos..].find("<hp:outMargin ").expect("outMargin");
    let end = m + xml[m..].find("/>").unwrap() + 2;
    with_section(&source, &format!("{}{ODD}{}", &xml[..m], &xml[end..]))
}

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

/// Each object kind keeps its outer margin through decode (Core JSON holds
/// it) and re-encode; the untouched fixture carries none.
///
/// 이것을 실패시키는 것: 디코더가 읽지 않기 / 종류별 인코더 자리 하나를 0 상수로
/// 되돌리기 / 0 여백도 `Some` 으로 싣기.
#[test]
fn object_out_margins_round_trip() {
    for (path, tag) in [
        ("shapes/textbox.hwpx", "rect"),
        ("user_samples/sample-gso-connectline-native.hwpx", "rect"),
        ("shapes/rect.hwpx", "ellipse"),
        ("user_samples/sample-gso-arc.hwpx", "ellipse"),
        ("shapes/line.hwpx", "line"),
        ("shapes/group-f1-g1-ungrouped.hwpx", "polygon"),
        ("user_samples/sample-gso-curve.hwpx", "curve"),
        ("user_samples/sample-gso-connectline-native.hwpx", "connectLine"),
        ("images/f6-dangling-image.hwpx", "pic"),
        ("shapes/group-slanted-child.hwpx", "container"),
    ] {
        let source = fixture(path);
        let plain =
            serde_json::to_string(&HwpxDecoder::decode(&source).expect("decode").document).unwrap();
        assert!(!plain.contains("out_margin"), "{path}: {tag} — untouched fixture");
        let input = with_odd_margin(path, tag);
        let json =
            serde_json::to_string(&HwpxDecoder::decode(&input).expect("decode").document).unwrap();
        assert!(json.contains("\"out_margin\""), "{path}: {tag} — Core");
        assert!(section_xml(&reencode(&input)).contains(ODD), "{path}: {tag} — re-encode");
    }
}
