//! Header `<hh:beginNum>` against section `<hp:startNum>` (§19 G).
//!
//! The header holds the document's start numbers (XSD `positiveInteger`,
//! Hancom always writes 1); a section's `startNum` restarts that section
//! (0 = continue from the previous one). Core has only the section layer,
//! so the encoder writes the header's page/pic/tbl/equation as Hancom does
//! (1) and keeps footnote/endnote, which the decoder merges into the first
//! section. Hancom answers: F16 a (new page number 5 in the text), b/c
//! (section starts at page 5 / 6), F17 (section starts pictures at 3 and
//! tables at 7) — each with a header of ones.

use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::{BeginNum, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}")).read_to_string(&mut out).unwrap();
    out
}

/// `bytes` with entry `name` replaced by `xml`; every other entry copied.
fn with_part(bytes: &[u8], name: &str, xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let entry_name = entry.name().to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        out.start_file(entry_name.as_str(), options).expect("start");
        if entry_name == name {
            out.write_all(xml.as_bytes()).expect("write");
        } else {
            out.write_all(&data).expect("write");
        }
    }
    out.finish().expect("finish").into_inner()
}

/// The attribute values of the first `<{tag} …>` in `xml`, in `names` order.
fn attrs(xml: &str, tag: &str, names: &[&str]) -> Vec<String> {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    let element = &xml[at..at + xml[at..].find('>').expect("tag end")];
    names
        .iter()
        .map(|name| {
            let needle = format!(" {name}=\"");
            let start = element.find(&needle).unwrap_or_else(|| panic!("{name} in {element}"));
            let rest = &element[start + needle.len()..];
            rest[..rest.find('"').expect("quote")].to_string()
        })
        .collect()
}

const HEADER: [&str; 6] = ["page", "footnote", "endnote", "pic", "tbl", "equation"];
const START: [&str; 4] = ["page", "pic", "tbl", "equation"];

fn header_numbers(bytes: &[u8]) -> Vec<String> {
    attrs(&part(bytes, "Contents/header.xml"), "hh:beginNum", &HEADER)
}

fn start_numbers(bytes: &[u8]) -> Vec<String> {
    attrs(&part(bytes, "Contents/section0.xml"), "hp:startNum", &START)
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

fn ones() -> Vec<String> {
    vec!["1".to_string(); 6]
}

/// T1a/T2/F17: a Hancom file re-encodes with Hancom's header of ones, and
/// each section keeps its own start numbers — "continue" (0) stays 0, a
/// restart at 5/6 or pictures at 3 and tables at 7 stay where Hancom put
/// them (the header used to get the section's values: 5, 6, 3, 7 or 0).
///
/// 이것을 실패시키는 것: header 를 다시 첫 구역 `begin_num` 에서 만드는 것
/// (f5c·F16a 는 page 0, F16b/c 는 5·6, F17 은 pic 3·tbl 7).
#[test]
fn hancom_start_numbers_reencode_with_a_header_of_ones() {
    for name in [
        "master_pages/f5c-odd-even.hwpx",
        "master_pages/f16a-newnum-page5.hwpx",
        "master_pages/f16b-section-start5.hwpx",
        "master_pages/f16c-section-start6.hwpx",
        "images/f17-caption-numbering.hwpx",
    ] {
        let source = fixture(name);
        assert_eq!(header_numbers(&source), ones(), "{name} as Hancom saved it");
        let out = reencode(&source);
        assert_eq!(header_numbers(&out), ones(), "{name}");
        assert_eq!(start_numbers(&out), start_numbers(&source), "{name}");
    }
    let f16a = fixture("master_pages/f16a-newnum-page5.hwpx");
    let new_numbers =
        |bytes: &[u8]| part(bytes, "Contents/section0.xml").matches("<hp:newNum ").count();
    assert_eq!(new_numbers(&reencode(&f16a)), new_numbers(&f16a), "F16a's paragraph restart");
}

/// `bytes` (f5c) with the header's start numbers replaced.
fn f5c_with_header(beginnum: &str) -> Vec<u8> {
    let source = fixture("master_pages/f5c-odd-even.hwpx");
    let header = part(&source, "Contents/header.xml").replace(
        r#"<hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/>"#,
        beginnum,
    );
    assert!(header.contains(beginnum), "the fixture's header has the default start numbers");
    with_part(&source, "Contents/header.xml", &header)
}

/// G-T7: without a section `startNum` the first section continues (0) and
/// takes only the header's footnote and endnote numbers; the header is
/// written back with those and ones.
///
/// 이것을 실패시키는 것: 병합 갈래를 지우는 것(각주·미주 7·9 소실), 옛
/// 통째 대입으로 되돌리는 것(구역 `startNum page` 가 1 — 이어서가 재시작이
/// 된다).
#[test]
fn without_a_section_start_number_the_header_note_numbers_are_kept() {
    let source = f5c_with_header(
        r#"<hh:beginNum page="1" footnote="7" endnote="9" pic="1" tbl="1" equation="1"/>"#,
    );
    let section = part(&source, "Contents/section0.xml");
    let at = section.find("<hp:startNum ").expect("startNum");
    let end = at + section[at..].find("/>").expect("empty element") + 2;
    let source = with_part(
        &source,
        "Contents/section0.xml",
        &format!("{}{}", &section[..at], &section[end..]),
    );

    let doc = HwpxDecoder::decode(&source).expect("decode");
    let begin = doc.document.sections()[0].begin_num.expect("merged from the header");
    assert_eq!(
        (begin.page, begin.pic, begin.tbl, begin.equation, begin.footnote, begin.endnote),
        (0, 0, 0, 0, 7, 9)
    );
    let out = reencode(&source);
    assert_eq!(header_numbers(&out), ["1", "7", "9", "1", "1", "1"]);
    assert_eq!(start_numbers(&out), ["0", "0", "0", "0"]);
}

/// G-T8: a document built through the API or `from-json`, whose first
/// section continues (`page 0`), gets Hancom's header of ones — the
/// generated files (x2-base, f14-base) used to start at page 0.
///
/// 이것을 실패시키는 것: header 를 다시 첫 구역 `begin_num` 에서 만드는 것.
#[test]
fn a_generated_document_gets_a_header_of_ones() {
    let mut section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("본문", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    section.begin_num =
        Some(BeginNum { page: 0, pic: 0, tbl: 0, equation: 0, ..BeginNum::default() });
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let out = HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode");
    assert_eq!(header_numbers(&out), ones());
    assert_eq!(start_numbers(&out), ["0", "0", "0", "0"]);
}
