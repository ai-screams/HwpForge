//! Table placement and object alignment round-trip (§19.6 A, S1–S3).
//!
//! A Hancom-saved table writes its own `<hp:pos>` and `textWrap`/`textFlow`;
//! re-encoding used to replace them with fixed values (41,522 of 42,180
//! corpus tables changed). Shapes and pictures lost `vertAlign`/`horzAlign`,
//! `affectLSpacing` and `holdAnchorAndSO`. Inputs are repo fixtures with one
//! `<hp:pos>` rewritten.

use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::placement::{HorzAlign, ObjectPlacement, VertAlign};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder};

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

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

/// The first `<hp:pos …/>` after the first `<{tag} ` element start, and that
/// element's start tag.
fn element_and_pos(xml: &str, tag: &str) -> (String, String) {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    let start = xml[at..at + xml[at..].find('>').unwrap() + 1].to_string();
    let p = at + xml[at..].find("<hp:pos ").expect("pos");
    let pos = xml[p..p + xml[p..].find("/>").unwrap() + 2].to_string();
    (start, pos)
}

fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle).unwrap_or_else(|| panic!("{name} in {tag}")) + needle.len();
    &tag[at..at + tag[at..].find('"').unwrap()]
}

const POS_ATTRS: [&str; 11] = [
    "treatAsChar",
    "affectLSpacing",
    "flowWithText",
    "allowOverlap",
    "holdAnchorAndSO",
    "vertRelTo",
    "horzRelTo",
    "vertAlign",
    "horzAlign",
    "vertOffset",
    "horzOffset",
];

fn pos_values(pos: &str) -> Vec<&str> {
    POS_ATTRS.iter().map(|a| attr(pos, a)).collect()
}

const X4C: &str = "header_footer/x4c-footer-in-spanning-cell.hwpx";
const X4C_TABLE_POS: &str = r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#;

fn table_with(pos: &str, wrap: &str) -> Vec<u8> {
    let source = fixture(X4C);
    let xml = section_xml(&source);
    assert_eq!(xml.matches(X4C_TABLE_POS).count(), 1, "fixture table pos");
    let xml = xml.replacen(X4C_TABLE_POS, pos, 1).replacen(
        r#"textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak"#,
        &format!(r#"{wrap} lock="0" dropcapstyle="None" pageBreak"#),
        1,
    );
    with_section(&source, &xml)
}

/// A-T1: an inline table and a floating, right-aligned, offset table keep
/// every `<hp:pos>` attribute and their `textWrap`/`textFlow` through a
/// re-encode. 이것을 실패시키는 것: 인코더 표 `<hp:pos>` 고정값 복귀 / 디코더가
/// 표 배치를 읽지 않기 / 정렬 미전달.
#[test]
fn table_placement_round_trips() {
    for (pos, wrap) in [
        (
            r#"<hp:pos treatAsChar="1" affectLSpacing="1" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#,
            r#"textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES""#,
        ),
        (
            r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="0" allowOverlap="1" holdAnchorAndSO="1" vertRelTo="PAPER" horzRelTo="PAGE" vertAlign="BOTTOM" horzAlign="RIGHT" vertOffset="27415" horzOffset="13"/>"#,
            r#"textWrap="SQUARE" textFlow="LEFT_ONLY""#,
        ),
    ] {
        let source = table_with(pos, wrap);
        let (start, ours) = element_and_pos(&section_xml(&reencode(&source)), "hp:tbl");
        let (want_start, theirs) = element_and_pos(&section_xml(&source), "hp:tbl");
        assert_eq!(pos_values(&ours), pos_values(&theirs), "{pos}");
        for a in ["textWrap", "textFlow"] {
            assert_eq!(attr(&start, a), attr(&want_start, a), "{a}");
        }
    }
}

/// A table without `<hp:pos>` keeps its own `textWrap`/`textFlow` through a
/// re-encode (review r1: the missing position used to drop them too), and
/// one with the default wrap and flow still decodes to no placement.
///
/// 이것을 실패시키는 것: pos 없는 표에서 조기 반환으로 되돌리기(SQUARE·LEFT_ONLY
/// 가 기본값으로) / 기본값과 같은 표를 `Some` 으로 남기기.
#[test]
fn table_without_pos_keeps_its_wrap() {
    let source = table_with("", r#"textWrap="SQUARE" textFlow="LEFT_ONLY""#);
    assert!(!section_xml(&source).contains(X4C_TABLE_POS));
    let (start, _) = element_and_pos(&section_xml(&reencode(&source)), "hp:tbl");
    assert_eq!(attr(&start, "textWrap"), "SQUARE");
    assert_eq!(attr(&start, "textFlow"), "LEFT_ONLY");
    let plain = table_with("", r#"textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES""#);
    assert_eq!(decoded_table_placement(&plain), None);
}

fn api_table_doc(placement: Option<ObjectPlacement>) -> Vec<u8> {
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("c", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(8000).unwrap(),
    );
    let mut table = Table::new(vec![TableRow::new(vec![cell])]);
    if let Some(p) = placement {
        table = table.with_placement(p);
    }
    let para = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let mut styles = HwpxStyleStore::new();
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

fn decoded_table_placement(bytes: &[u8]) -> Option<ObjectPlacement> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let mut found = None;
    for run in doc.document.sections()[0].paragraphs.iter().flat_map(|p| &p.runs) {
        if let RunContent::Table(t) = &run.content {
            found = Some(t.placement.clone());
        }
    }
    found.expect("table")
}

/// A-T2: an API table (`placement: None`) writes the same `<hp:pos>` as
/// before and decodes back to `None`; a placement differing from that
/// default in only one of the four new fields stays `Some`.
///
/// 이것을 실패시키는 것: 접기 조건을 `legacy_inline_defaults` 로(API 표가
/// `Some`) / 새 네 필드 중 하나를 접기 비교에서 빼기(각각).
#[test]
fn table_placement_folds_only_the_full_default() {
    let plain = api_table_doc(None);
    let (_, pos) = element_and_pos(&section_xml(&plain), "hp:tbl");
    assert_eq!(pos, X4C_TABLE_POS);
    assert_eq!(decoded_table_placement(&plain), None);

    let default = decoded_table_placement(&table_with(
        r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="BOTTOM" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#,
        r#"textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES""#,
    ))
    .expect("bottom alignment is not the default");
    let base = default.with_vert_align(VertAlign::Top);
    for one in [
        base.clone().with_vert_align(VertAlign::Center),
        base.clone().with_horz_align(HorzAlign::Center),
        base.clone().with_affect_line_spacing(true),
        base.clone().with_hold_anchor_and_so(true),
    ] {
        assert_eq!(decoded_table_placement(&api_table_doc(Some(one.clone()))), Some(one));
    }
    assert_eq!(decoded_table_placement(&api_table_doc(Some(base))), None, "the default folds");
}

const X6_FIRST_POS: &str = r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="0" allowOverlap="1" holdAnchorAndSO="0" vertRelTo="PAPER" horzRelTo="PAPER" vertAlign="TOP" horzAlign="LEFT" vertOffset="11339" horzOffset="17008"/>"#;

/// A-T5: a floating rectangle's alignment and flags survive a re-encode, and
/// an alignment string this codec does not know is reported and written
/// TOP. 이것을 실패시키는 것: 도형 인코더 TOP/LEFT·0 고정 복귀 / 디코더 정렬
/// 읽기 삭제 / 미지값 경고 삭제.
#[test]
fn object_alignment_and_flags_round_trip() {
    let source = fixture("shapes/x6-fill.hwpx");
    let xml = section_xml(&source);
    assert_eq!(xml.matches(X6_FIRST_POS).count(), 1);
    let aligned = X6_FIRST_POS
        .replace(r#"affectLSpacing="0""#, r#"affectLSpacing="1""#)
        .replace(r#"holdAnchorAndSO="0""#, r#"holdAnchorAndSO="1""#)
        .replace(r#"vertAlign="TOP""#, r#"vertAlign="CENTER""#)
        .replace(r#"horzAlign="LEFT""#, r#"horzAlign="OUTSIDE""#);
    let edited = with_section(&source, &xml.replacen(X6_FIRST_POS, &aligned, 1));
    let (_, ours) = element_and_pos(&section_xml(&reencode(&edited)), "hp:rect");
    assert_eq!(pos_values(&ours), pos_values(&aligned));

    let odd = X6_FIRST_POS.replace(r#"vertAlign="TOP""#, r#"vertAlign="MIDDLE""#);
    let edited = with_section(&source, &xml.replacen(X6_FIRST_POS, &odd, 1));
    let doc = HwpxDecoder::decode(&edited).expect("decode");
    assert!(
        doc.warnings.iter().any(|w| matches!(
            w,
            DecodeWarning::UnknownEnumValue { attribute: "hp:pos@vertAlign", raw, .. } if raw == "MIDDLE"
        )),
        "{:?}",
        doc.warnings
    );
}

const F17_FIRST_PIC_POS: &str = r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="0" allowOverlap="1" holdAnchorAndSO="0" vertRelTo="PAPER" horzRelTo="PAPER" vertAlign="TOP" horzAlign="LEFT" vertOffset="11339" horzOffset="8504"/>"#;

/// A floating picture keeps its alignment and flags too. 이것을 실패시키는
/// 것: 그림 인코더의 정렬·두 플래그 고정값 복귀 / 그림 디코더의 정렬 읽기 삭제.
#[test]
fn picture_alignment_and_flags_round_trip() {
    let source = fixture("images/f17-caption-numbering.hwpx");
    let xml = section_xml(&source);
    assert!(xml.contains(F17_FIRST_PIC_POS));
    let aligned = F17_FIRST_PIC_POS
        .replace(r#"affectLSpacing="0""#, r#"affectLSpacing="1""#)
        .replace(r#"holdAnchorAndSO="0""#, r#"holdAnchorAndSO="1""#)
        .replace(r#"vertAlign="TOP""#, r#"vertAlign="BOTTOM""#)
        .replace(r#"horzAlign="LEFT""#, r#"horzAlign="CENTER""#);
    let edited = with_section(&source, &xml.replacen(F17_FIRST_PIC_POS, &aligned, 1));
    let (_, ours) = element_and_pos(&section_xml(&reencode(&edited)), "hp:pic");
    assert_eq!(pos_values(&ours), pos_values(&aligned));
}
