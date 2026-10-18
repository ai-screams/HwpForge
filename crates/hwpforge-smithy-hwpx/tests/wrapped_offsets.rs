//! Object offsets are written the way Hancom writes them (§19 P20, S19).
//!
//! In 485 Hancom-saved HWPX files a negative object offset is always the
//! wrapped unsigned decimal: `<hp:pos vertOffset>` 401 times and
//! `horzOffset` 44 times, `<hp:offset x>` 1,188 times and `y` 888 times, and
//! none of the 8,887 `<hp:pos>` or 3,440 `<hp:offset x y>` elements holds a
//! negative literal. The reader folds the value to a
//! negative `i32`; the writer wrote it back signed. The same file writes the
//! translation of `<hc:transMatrix>` as a signed literal, so that one stays
//! signed.

use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Control, Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

/// -2429, the most common wrapped `vertOffset` in the Hancom set.
const V: &str = "4294964867";
/// -106, the most common wrapped `horzOffset`.
const H: &str = "4294967190";
/// -310, the most common wrapped `<hp:offset x>`.
const X: &str = "4294966986";

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

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

fn styles() -> HwpxStyleStore {
    // 이것을 실패시키는 것: 합성 fixture에서 참조하는 원본 글꼴 정의 누락.
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

fn encode_one(run: Run) -> Vec<u8> {
    let para = Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles(), &ImageStore::new())
        .expect("encode")
}

/// No offset attribute in `xml` is written as a negative literal.
fn assert_no_signed_offset(xml: &str) {
    for bad in ["vertOffset=\"-", "horzOffset=\"-"] {
        assert!(!xml.contains(bad), "{bad} in {xml}");
    }
    for rest in xml.split("<hp:offset ").skip(1) {
        let attrs = &rest[..rest.find("/>").expect("self-closing")];
        assert!(!attrs.contains("=\"-"), "<hp:offset {attrs}/> in {xml}");
    }
}

fn negative_placement() -> ObjectPlacement {
    ObjectPlacement::new(ObjectTextWrap::Square, ObjectRelativeTo::Para, ObjectRelativeTo::Column)
        .with_vert_offset(HwpUnit::new(-2429).unwrap())
        .with_horz_offset(HwpUnit::new(-106).unwrap())
}

/// A Hancom table whose `<hp:pos>` holds wrapped offsets decodes to the
/// signed Core offsets and is written back with the same wrapped values.
///
/// 이것을 실패시키는 것: `HxTablePos` 의 `vertOffset`·`horzOffset` 에서
/// `serialize_with = "ser_i32_as_u32"` 빼기(각각 — `-2429`·`-106` 이 나간다).
#[test]
fn table_pos_offsets_round_trip_wrapped() {
    let source = fixture("tables/table_01_basic_2x2.hwpx");
    let zero = r#"vertOffset="0" horzOffset="0""#;
    let wrapped = format!(r#"vertOffset="{V}" horzOffset="{H}""#);
    let input = with_section(&source, &section_xml(&source).replacen(zero, &wrapped, 1));

    let doc = HwpxDecoder::decode(&input).expect("decode");
    let placement = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Table(t) => Some(t.placement.clone().expect("floating table")),
            _ => None,
        })
        .expect("table");
    assert_eq!((placement.vert_offset.as_i32(), placement.horz_offset.as_i32()), (-2429, -106));

    let out = section_xml(&reencode(&input));
    assert_eq!(out.matches(&wrapped).count(), 1, "{out}");
    assert_no_signed_offset(&out);
}

/// A group whose second picture sits at −310 (Hancom: `<hp:offset x>`
/// wrapped, `transMatrix e3` signed) and whose own `<hp:pos>` holds wrapped
/// offsets keeps both forms: the offset and the position are written
/// wrapped, the matrix translation signed. Decoding the output gives the
/// same group.
///
/// 이것을 실패시키는 것: 묶음 자식 offset 쓰기(`set_group_child_offset`)를
/// i32 로 되돌리기 / `HxTablePos` 를 i32 로(묶음 `<hp:pos>`) / 행렬 이동을
/// 부호 없는 값으로 쓰기.
#[test]
fn group_child_offset_and_group_pos_round_trip_wrapped() {
    let source = fixture("shapes/group-f3a-logo.hwpx");
    // All three pictures move 310 left, so the group keeps the size of its
    // bounds; the second one (instid 2, at 0) lands on −310.
    let mut xml = section_xml(&source)
        .replacen(r#"<hp:offset x="9921" y="0"/>"#, r#"<hp:offset x="9611" y="0"/>"#, 1)
        .replacen(r#"e3="9921""#, r#"e3="9611""#, 1)
        .replacen(r#"<hp:offset x="19842" y="0"/>"#, r#"<hp:offset x="19532" y="0"/>"#, 1)
        .replacen(r#"e3="19842""#, r#"e3="19532""#, 1)
        .replacen(
            r#"vertOffset="11339" horzOffset="8504""#,
            &format!(r#"vertOffset="{V}" horzOffset="{H}""#),
            1,
        );
    let second = xml.find(r#"instid="2" reverse="0">"#).expect("second picture");
    let tail = xml[second..]
        .replacen(r#"<hp:offset x="0" y="0"/>"#, &format!(r#"<hp:offset x="{X}" y="0"/>"#), 1)
        .replacen(r#"e3="0""#, r#"e3="-310""#, 1);
    xml.replace_range(second.., &tail);
    let input = with_section(&source, &xml);
    let before = HwpxDecoder::decode(&input).expect("decode");
    assert!(before.warnings.is_empty(), "{:?}", before.warnings);

    let bytes = reencode(&input);
    let out = section_xml(&bytes);
    assert!(out.contains(&format!(r#"<hp:offset x="{X}" y="0"/>"#)), "{out}");
    assert!(out.contains(r#"e3="-310" e4="0" e5="1" e6="0""#), "{out}");
    assert!(out.contains(&format!(r#"vertOffset="{V}" horzOffset="{H}""#)), "{out}");
    assert_no_signed_offset(&out);

    let again = HwpxDecoder::decode(&bytes).expect("decode again");
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    let runs = |d: &hwpforge_smithy_hwpx::HwpxDocument| {
        d.document.sections()[0].paragraphs.iter().map(|p| p.runs.clone()).collect::<Vec<_>>()
    };
    assert_eq!(runs(&again), runs(&before));
}

/// A TextArt anchors at its `<hp:offset>`; a wrapped offset decodes to the
/// signed placement and is written back wrapped.
///
/// 이것을 실패시키는 것: 글맵시 `<hp:offset x="{hx}" y="{vy}"/>` 를 i32 로 쓰기.
#[test]
fn text_art_offset_round_trips_wrapped() {
    let source = fixture("shapes/textart_multiline.hwpx");
    let wrapped = format!(r#"<hp:offset x="{H}" y="{V}"/>"#);
    let xml = section_xml(&source).replacen(r#"<hp:offset x="0" y="0"/>"#, &wrapped, 1);
    let input = with_section(&source, &xml);
    let doc = HwpxDecoder::decode(&input).expect("decode");
    let placement = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::TextArt { placement, .. } => placement.clone(),
                _ => None,
            },
            _ => None,
        })
        .expect("text art placement");
    assert_eq!((placement.horz_offset.as_i32(), placement.vert_offset.as_i32()), (-106, -2429));

    let out = section_xml(&reencode(&input));
    assert_eq!(out.matches(&wrapped).count(), 1, "{out}");
    assert_no_signed_offset(&out);
}

/// HWP5 stores offsets as signed `i32`; a Core table placed at negative
/// offsets (as the HWP5 decoder produces it) is written wrapped and decodes
/// back to the same placement.
///
/// 이것을 실패시키는 것: `HxTablePos` 를 i32 로 쓰기.
#[test]
fn signed_core_table_offsets_are_written_wrapped() {
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("c", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(8000).unwrap(),
    );
    let table = Table::new(vec![TableRow::new(vec![cell])]).with_placement(negative_placement());
    let bytes = encode_one(Run::table(table, CharShapeIndex::new(0)));
    let out = section_xml(&bytes);
    assert!(out.contains(&format!(r#"vertOffset="{V}" horzOffset="{H}""#)), "{out}");
    assert_no_signed_offset(&out);

    let doc = HwpxDecoder::decode(&bytes).expect("decode");
    let placement = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Table(t) => t.placement.clone(),
            _ => None,
        })
        .expect("table placement");
    assert_eq!(placement, negative_placement());
}

/// An HWP5 OLE chart is written as a hand-built `<hp:switch>` whose chart
/// and OLE fallback both carry `<hp:pos>`; negative Core offsets are written
/// wrapped in both, and the chart decodes with the signed offsets.
///
/// 이것을 실패시키는 것: `build_embedded_chart_run_xml` 의 `{vo}`·`{ho}` 를
/// i32 로 쓰기.
#[test]
fn hwp5_chart_offsets_are_written_wrapped() {
    let chart = Control::EmbeddedChart {
        chart_xml: r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#.to_string(),
        ole_bytes: vec![0; 16],
        width: HwpUnit::new(20000).unwrap(),
        height: HwpUnit::new(10000).unwrap(),
        placement: Some(negative_placement()),
    };
    let bytes = encode_one(Run::control(chart, CharShapeIndex::new(0)));
    let out = section_xml(&bytes);
    assert_eq!(out.matches(&format!(r#"vertOffset="{V}" horzOffset="{H}""#)).count(), 2, "{out}");
    assert_no_signed_offset(&out);
}

/// A group moved 145 up: every picture's `<hp:offset y>` is written wrapped
/// and its `transMatrix e6` signed, as Hancom writes them, independently of
/// `x`. Decoding the output gives the same group.
///
/// 이것을 실패시키는 것: 묶음 자식 offset 의 `y` 만 i32 로 쓰기 / 행렬 `e6`
/// 을 부호 없는 값으로 쓰기.
#[test]
fn group_child_negative_y_round_trips_wrapped() {
    let source = fixture("shapes/group-f3a-logo.hwpx");
    let mut xml = section_xml(&source);
    let first = xml.find("<hp:pic ").expect("first picture");
    // Only the `transMatrix` translation changes: each picture also has
    // `rotMatrix` entries followed by a `scaMatrix`.
    let tail = xml[first..].replace(r#"y="0"/><hp:orgSz"#, r#"y="4294967151"/><hp:orgSz"#).replace(
        r#"e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix"#,
        r#"e4="0" e5="1" e6="-145"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix"#,
    );
    xml.replace_range(first.., &tail);
    assert_eq!(xml.matches(r#"y="4294967151""#).count(), 3);
    assert_eq!(xml.matches(r#"e6="-145""#).count(), 3);
    let input = with_section(&source, &xml);
    let before = HwpxDecoder::decode(&input).expect("decode");
    assert!(before.warnings.is_empty(), "{:?}", before.warnings);

    let bytes = reencode(&input);
    let out = section_xml(&bytes);
    assert_eq!(out.matches(r#"y="4294967151"/>"#).count(), 3, "{out}");
    assert_eq!(out.matches(r#"e5="1" e6="-145"/>"#).count(), 3, "{out}");
    assert_no_signed_offset(&out);

    let again = HwpxDecoder::decode(&bytes).expect("decode again");
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    let runs = |d: &hwpforge_smithy_hwpx::HwpxDocument| {
        d.document.sections()[0].paragraphs.iter().map(|p| p.runs.clone()).collect::<Vec<_>>()
    };
    assert_eq!(runs(&again), runs(&before));
}

/// A TextArt whose Core size is negative (validation rejects only 0) writes
/// `<hp:curSz>` as the unsigned wrap Hancom uses; the derived `scaMatrix`
/// stays signed.
///
/// 이것을 실패시키는 것: 글맵시 `<hp:curSz>` 의 `width`·`height` 를 i32 로
/// 쓰기(각각) / 크기 계산(`scaMatrix`)까지 부호 없는 값으로 하기.
#[test]
fn text_art_negative_size_writes_wrapped_cur_sz() {
    let art = Control::TextArt {
        frame: hwpforge_core::ObjectFrame::new(),
        text: "글".to_string(),
        shape: "WAVE2".to_string(),
        font_name: "함초롬바탕".to_string(),
        font_style: "보통".to_string(),
        align: "LEFT".to_string(),
        line_spacing: 120,
        char_spacing: 100,
        width: HwpUnit::new(-14173).unwrap(),
        height: HwpUnit::new(-2355).unwrap(),
        placement: None,
        fill_color: None,
        inst_id: None,
    };
    let out = section_xml(&encode_one(Run::control(art, CharShapeIndex::new(0))));
    assert!(out.contains(r#"<hp:curSz width="4294953123" height="4294964941"/>"#), "{out}");
    assert!(
        out.contains(r#"<hc:scaMatrix e1="-1.000000" e2="0" e3="0" e4="0" e5="-0.166161""#),
        "{out}"
    );
}
