//! Cell line wrap (§19.22 D3, S15).
//!
//! Hancom writes a cell's line wrap on its `<hp:subList lineWrap>` (58,237
//! SQUEEZE cells in 590 corpus documents). The decoder did not read it and
//! the encoder always wrote BREAK, so a re-saved document lost it. The input
//! is the repo's 2×2 table fixture with the first three cells' wraps
//! rewritten.

use std::io::{Cursor, Read, Write};

use hwpforge_core::run::RunContent;
use hwpforge_core::table::CellLineWrap;
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

/// The fixture with its first cells' `lineWrap` set to `wraps` in order.
fn with_wraps(wraps: &[&str]) -> Vec<u8> {
    let source = fixture("tables/table_01_basic_2x2.hwpx");
    let mut xml = section_xml(&source);
    for wrap in wraps {
        xml = xml.replacen("lineWrap=\"BREAK\"", &format!("lineWrap=\"@{wrap}\""), 1);
    }
    with_section(&source, &xml.replace("lineWrap=\"@", "lineWrap=\""))
}

fn cell_wraps(bytes: &[u8]) -> (Vec<CellLineWrap>, Vec<DecodeWarning>) {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let mut out = Vec::new();
    for p in &doc.document.sections()[0].paragraphs {
        for run in &p.runs {
            if let RunContent::Table(t) = &run.content {
                out.extend(t.rows.iter().flat_map(|r| r.cells.iter().map(|c| c.line_wrap)));
            }
        }
    }
    (out, doc.warnings)
}

/// SQUEEZE and KEEP reach Core and come back out of the encoder; BREAK stays.
///
/// 이것을 실패시키는 것: 디코더가 `lineWrap` 을 읽지 않기 / 인코더 고정 BREAK.
#[test]
fn cell_line_wrap_round_trips() {
    let input = with_wraps(&["SQUEEZE", "KEEP"]);
    let (wraps, _) = cell_wraps(&input);
    assert_eq!(
        wraps,
        [CellLineWrap::Squeeze, CellLineWrap::Keep, CellLineWrap::Break, CellLineWrap::Break]
    );
    let doc = HwpxDecoder::decode(&input).expect("decode");
    let out = HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode");
    let xml = section_xml(&out);
    let found: Vec<&str> = xml
        .match_indices("lineWrap=\"")
        .map(|(i, m)| {
            let rest = &xml[i + m.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect();
    assert_eq!(found, ["SQUEEZE", "KEEP", "BREAK", "BREAK"]);
}

/// An unknown `lineWrap` is reported and read as BREAK.
///
/// 이것을 실패시키는 것: 미지 값 경고를 지우기.
#[test]
fn unknown_cell_line_wrap_is_reported() {
    let (wraps, warnings) = cell_wraps(&with_wraps(&["FOO"]));
    assert_eq!(wraps[0], CellLineWrap::Break);
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            DecodeWarning::UnknownEnumValue { attribute: "hp:subList@lineWrap", raw, .. } if raw == "FOO"
        )),
        "{warnings:?}"
    );
}

/// S19: a cell without its own margin (`hasMargin="0"`) keeps the values
/// Hancom left in its `<hp:cellMargin>` through decode and re-encode; one
/// whose values are the table's padding carries nothing extra.
///
/// 이것을 실패시키는 것: 디코더가 휴면 값을 읽지 않기 / 인코더가 쓰지 않기 /
/// 표 여백과 같은 값도 싣기.
#[test]
fn dormant_cell_margins_round_trip() {
    let source = fixture("tables/table_01_basic_2x2.hwpx");
    let xml = section_xml(&source);
    let padding = r#"<hp:cellMargin left="510" right="510" top="141" bottom="141"/>"#;
    let own = r#"<hp:cellMargin left="11" right="12" top="13" bottom="14"/>"#;
    assert_eq!(xml.matches(padding).count(), 4);
    let input = with_section(&source, &xml.replacen(padding, own, 1));
    let doc = HwpxDecoder::decode(&input).expect("decode");
    let mut dormant = Vec::new();
    for p in &doc.document.sections()[0].paragraphs {
        for run in &p.runs {
            if let RunContent::Table(t) = &run.content {
                for c in t.rows.iter().flat_map(|r| &r.cells) {
                    assert!(c.margin.is_none());
                    dormant.push(c.dormant_margin.map(|m| m.left.as_i32()));
                }
            }
        }
    }
    assert_eq!(dormant, [Some(11), None, None, None]);
    // Without `<hp:inMargin>` the padding is the 510/510/141/141 default,
    // and cells holding it carry nothing either.
    let no_in = xml.replacen(padding, own, 1);
    let no_in = strip_first_element(&no_in, "<hp:inMargin ");
    let bare = HwpxDecoder::decode(&with_section(&source, &no_in)).expect("decode");
    let mut bare_dormant = Vec::new();
    for p in &bare.document.sections()[0].paragraphs {
        for run in &p.runs {
            if let RunContent::Table(t) = &run.content {
                assert!(t.in_margin.is_none());
                bare_dormant.extend(
                    t.rows
                        .iter()
                        .flat_map(|r| &r.cells)
                        .map(|c| c.dormant_margin.map(|m| m.left.as_i32())),
                );
            }
        }
    }
    assert_eq!(bare_dormant, [Some(11), None, None, None]);
    let out = HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode");
    let out_xml = section_xml(&out);
    assert_eq!(out_xml.matches(own).count(), 1, "{out_xml}");
    assert_eq!(out_xml.matches(padding).count(), 3);
    assert_eq!(out_xml.matches("hasMargin=\"0\"").count(), 4);
}

/// Removes the first self-closing element starting with `open`.
fn strip_first_element(xml: &str, open: &str) -> String {
    let start = xml.find(open).expect("element");
    let end = start + xml[start..].find("/>").expect("self-closing") + 2;
    format!("{}{}", &xml[..start], &xml[end..])
}

/// S19: diff reports a changed dormant margin on its raw axis (the cell
/// differs beyond its text).
///
/// 이것을 실패시키는 것: 디코더가 휴면 값을 읽지 않기(그러면 package 차이만
/// 남는다) / 표 여백과 같은 값 정리를 지우기.
#[test]
fn diff_reports_a_changed_dormant_margin() {
    let source = fixture("tables/table_01_basic_2x2.hwpx");
    let xml = section_xml(&source);
    let padding = r#"<hp:cellMargin left="510" right="510" top="141" bottom="141"/>"#;
    let own = r#"<hp:cellMargin left="11" right="12" top="13" bottom="14"/>"#;
    let input = with_section(&source, &xml.replacen(padding, own, 1));
    let diff = hwpforge_smithy_hwpx::HwpxDiffer::diff(&source, &input).expect("diff");
    let raw: Vec<_> = diff.semantic.raw.iter().map(|r| r.detail.as_str()).collect();
    assert_eq!(raw, ["$.runs[0].content.Table.rows[0].cells[0].dormant_margin"], "{diff:?}");
}

/// S19: set-cell re-encodes the package and keeps another cell's dormant
/// margin.
///
/// 이것을 실패시키는 것: 디코더가 휴면 값을 읽지 않기 / 인코더가 쓰지 않기.
#[test]
fn set_cell_keeps_a_dormant_margin() {
    let padding = r#"<hp:cellMargin left="510" right="510" top="141" bottom="141"/>"#;
    let own = r#"<hp:cellMargin left="11" right="12" top="13" bottom="14"/>"#;
    let form = fixture("tables/merged_grid_form.hwpx");
    let form = with_section(&form, &section_xml(&form).replacen(padding, own, 1));
    let spec = hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            1, 1,
        )),
        text: "x".to_string(),
    };
    let edited = hwpforge_smithy_hwpx::HwpxCellEditor::set_cells_with_diagnostics(&form, &[spec])
        .expect("set-cell");
    assert_eq!(section_xml(&edited.value.bytes).matches(own).count(), 1);
}

/// S19: Hancom fills the dormant `<hp:cellMargin>` of some cells with
/// `4294967295` on every side (4,484 cells in 33 of 485 Hancom-saved
/// documents, all on `hasMargin="0"`; none holds a negative margin). The
/// value reads as -1 and must be written back as `4294967295` — the margin
/// attributes are `xs:nonNegativeInteger`, so `-1` is not valid HWPX.
///
/// 이것을 실패시키는 것: 여백 속성을 i32 로 쓰기(`-1` 이 나간다) / 한 변만
/// 부호 없는 값으로 쓰기.
#[test]
fn dormant_unset_cell_margin_round_trips_as_unsigned() {
    let source = fixture("tables/table_01_basic_2x2.hwpx");
    let xml = section_xml(&source);
    let padding = r#"<hp:cellMargin left="510" right="510" top="141" bottom="141"/>"#;
    let unset = r#"<hp:cellMargin left="4294967295" right="4294967295" top="4294967295" bottom="4294967295"/>"#;
    let input = with_section(&source, &xml.replacen(padding, unset, 1));
    let doc = HwpxDecoder::decode(&input).expect("decode");
    let mut dormant = Vec::new();
    for p in &doc.document.sections()[0].paragraphs {
        for run in &p.runs {
            if let RunContent::Table(t) = &run.content {
                dormant.extend(t.rows.iter().flat_map(|r| &r.cells).map(|c| {
                    c.dormant_margin.map(|m| {
                        [m.left.as_i32(), m.right.as_i32(), m.top.as_i32(), m.bottom.as_i32()]
                    })
                }));
            }
        }
    }
    assert_eq!(dormant, [Some([-1; 4]), None, None, None]);
    let out = HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode");
    let out_xml = section_xml(&out);
    assert_eq!(out_xml.matches(unset).count(), 1, "{out_xml}");
    let negative = out_xml
        .split("<hp:cellMargin ")
        .skip(1)
        .filter(|rest| rest[..rest.find("/>").expect("self-closing")].contains('-'))
        .count();
    assert_eq!(negative, 0, "{out_xml}");
}
