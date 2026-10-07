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
