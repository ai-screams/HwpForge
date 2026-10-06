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
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder};

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
/// (f5c·F16a 는 page 0, F16b/c 는 5·6, F17 은 pic 3·tbl 7), 문단 새 번호의
/// 값이나 종류를 바꾸는 것.
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
    let new_numbers = |bytes: &[u8]| -> Vec<Vec<String>> {
        let xml = part(bytes, "Contents/section0.xml");
        xml.match_indices("<hp:newNum ")
            .map(|(at, _)| attrs(&xml[at..], "hp:newNum", &["num", "numType"]))
            .collect()
    };
    assert_eq!(new_numbers(&f16a), [vec!["5".to_string(), "PAGE".to_string()]], "F16a as made");
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

fn repaired_axes(warnings: &[DecodeWarning]) -> Vec<Vec<&'static str>> {
    warnings
        .iter()
        .filter_map(|w| match w {
            DecodeWarning::BeginNumRepaired { axes } => Some(axes.clone()),
            _ => None,
        })
        .collect()
}

fn lost_begin_num(warnings: &[DecodeWarning]) -> Vec<&DecodeWarning> {
    warnings
        .iter()
        .filter(|w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind == "begin_num"))
        .collect()
}

/// T5: a header start number of 0 (a schema violation our older output
/// wrote) is reported once, naming the axes, and a re-encode writes 1. It is a
/// repair, not a loss: editors still accept the input.
///
/// 이것을 실패시키는 것: 0 경고 갈래를 지우는 것, 축 판정을 page 하나로
/// 줄이는 것, 0 을 손실(`ContentLost`)로 보고하는 것.
#[test]
fn a_zero_header_start_number_is_repaired_and_reported() {
    let all = f5c_with_header(
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="0" tbl="0" equation="0"/>"#,
    );
    let doc = HwpxDecoder::decode(&all).expect("decode");
    assert_eq!(repaired_axes(&doc.warnings), [vec!["page", "pic", "tbl", "equation"]]);
    assert!(lost_begin_num(&doc.warnings).is_empty(), "{:?}", doc.warnings);
    assert!(!doc.warnings.iter().any(DecodeWarning::is_content_loss), "{:?}", doc.warnings);
    assert_eq!(header_numbers(&reencode(&all)), ones());

    let pic_only = f5c_with_header(
        r#"<hh:beginNum page="1" footnote="1" endnote="1" pic="0" tbl="1" equation="1"/>"#,
    );
    let doc = HwpxDecoder::decode(&pic_only).expect("decode");
    assert_eq!(repaired_axes(&doc.warnings), [vec!["pic"]]);

    let clean = HwpxDecoder::decode(&fixture("master_pages/f5c-odd-even.hwpx")).expect("decode");
    assert!(repaired_axes(&clean.warnings).is_empty(), "{:?}", clean.warnings);
}

/// A valid header start number other than 1 has no place in Core (no
/// document layer, option B), so a re-encode writes 1: reported as lost
/// content, so editors that re-encode refuse the input.
///
/// 이것을 실패시키는 것: 손실 갈래를 지우는 것, 손실 판정을 `> 2` 로 하는 것
/// (2 가 빠진다), 1 도 손실로 보는 것.
#[test]
fn a_header_start_number_core_cannot_carry_is_lost() {
    let source = f5c_with_header(
        r#"<hh:beginNum page="5" footnote="1" endnote="1" pic="2" tbl="1" equation="1"/>"#,
    );
    let doc = HwpxDecoder::decode(&source).expect("decode");
    let lost = lost_begin_num(&doc.warnings);
    assert_eq!(lost.len(), 1, "{:?}", doc.warnings);
    let DecodeWarning::ContentLost { part, reason, .. } = lost[0] else { unreachable!() };
    assert_eq!(part, "Contents/header.xml");
    assert_eq!(reason, "header beginNum page=5 pic=2 is not carried (a re-encode writes 1)");
    assert!(lost[0].is_content_loss());
    assert!(repaired_axes(&doc.warnings).is_empty());
    assert_eq!(header_numbers(&reencode(&source)), ones());
}

/// A header with both a 0 (repaired) and valid values other than 1 (lost)
/// reports both, each with its own axes.
///
/// 이것을 실패시키는 것: 고침 경고 뒤에 조기 반환하는 것(손실 경고가 빠진다),
/// 손실 판정에서 tbl·equation 을 빼는 것.
#[test]
fn a_header_with_a_zero_and_a_lost_value_reports_both() {
    let source = f5c_with_header(
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="1" tbl="2" equation="3"/>"#,
    );
    let doc = HwpxDecoder::decode(&source).expect("decode");
    assert_eq!(repaired_axes(&doc.warnings), [vec!["page"]]);
    let lost = lost_begin_num(&doc.warnings);
    assert_eq!(lost.len(), 1, "{:?}", doc.warnings);
    let DecodeWarning::ContentLost { reason, .. } = lost[0] else { unreachable!() };
    assert_eq!(reason, "header beginNum tbl=2 equation=3 is not carried (a re-encode writes 1)");
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
