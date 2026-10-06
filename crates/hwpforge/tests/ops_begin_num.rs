//! The re-encoding surfaces write Hancom's header start numbers (§19 G, T6).
//!
//! `restyle` and `from_json` (with and without a base) re-encode through
//! the HWPX encoder; each must keep the header `<hh:beginNum>` at 1 and the
//! section's own restart (`<hp:startNum page="5">`, Hancom's F16b) where
//! Hancom put it. Before, the header took the section's 5.
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read, Write};

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::section::BeginNum;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::HwpxEncoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

fn f16b() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/master_pages/f16b-section-start5.hwpx"
    );
    std::fs::read(path).expect("F16b")
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).expect(name).read_to_string(&mut out).expect("utf-8");
    out
}

fn element<'a>(xml: &'a str, tag: &str) -> &'a str {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    &xml[at..at + xml[at..].find('>').expect("tag end")]
}

fn assert_hancom_numbers(bytes: &[u8], surface: &str) {
    let header = part(bytes, "Contents/header.xml");
    assert_eq!(
        element(&header, "hh:beginNum"),
        r#"<hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/"#,
        "{surface}"
    );
    let section = part(bytes, "Contents/section0.xml");
    assert!(element(&section, "hp:startNum").contains(r#" page="5""#), "{surface}");
}

/// 이것을 실패시키는 것: header 를 첫 구역 `begin_num` 에서 만드는 것
/// (세 표면 모두 header `page="5"`).
#[test]
fn reencoding_surfaces_keep_hancoms_header_start_numbers() {
    let source = f16b();
    let restyled =
        restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle").bytes;
    assert_hancom_numbers(&restyled, "restyle");

    let json = to_json(&source, &ToJsonOptions::default()).expect("to_json").document.to_string();
    let rebuilt = from_json(&json, &FromJsonOptions::default().with_base(source.clone()))
        .expect("from_json --base")
        .bytes;
    assert_hancom_numbers(&rebuilt, "from_json --base");
    let generated = from_json(&json, &FromJsonOptions::default()).expect("from_json").bytes;
    assert_hancom_numbers(&generated, "from_json");
}

/// `bytes` with its header's start numbers replaced by `beginnum`.
fn with_header_numbers(bytes: &[u8], beginnum: &str) -> Vec<u8> {
    let header = part(bytes, "Contents/header.xml");
    let old = element(&header, "hh:beginNum").to_string() + ">";
    let header = header.replace(&old, beginnum);
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
        let data = if name == "Contents/header.xml" { header.as_bytes().to_vec() } else { data };
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// §3.2: a header start number Core cannot carry (page 5) is lost content,
/// so restyle — which would write it back as 1 — refuses the input. A
/// header of 0 (the trace of our older output) is only repaired: restyle
/// goes through, writes ones and reports the repair.
///
/// 이것을 실패시키는 것: header 손실 경고를 지우는 것(restyle 이 5 를 1 로
/// 다시 씀), 0 고침을 손실로 분류하는 것(restyle 이 거부).
#[test]
fn restyle_refuses_a_lost_header_start_number_and_repairs_a_zero() {
    let opts = RestyleOptions::default().with_preset("modern");
    let lost = with_header_numbers(
        &f16b(),
        r#"<hh:beginNum page="5" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/>"#,
    );
    let err = restyle(&lost, &opts).unwrap_err();
    assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{err}");
    assert!(err.to_string().contains("begin_num"), "{err}");

    let zero = with_header_numbers(
        &f16b(),
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="0" tbl="0" equation="0"/>"#,
    );
    let out = restyle(&zero, &opts).expect("a repair is not a loss");
    assert_hancom_numbers(&out.bytes, "restyle of a header of 0");
    assert!(
        out.warnings.iter().any(|w| w.info().code == "BEGIN_NUM_REPAIRED"),
        "{:?}",
        out.warnings
    );
}

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// A package this codec wrote — set-cell and stamp accept it — whose only
/// section restarts at page 5 (F16b's form), with a stamp candidate
/// (`(   )`) and a 1×2 label table. Its header comes out as ones.
fn editable() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let mut section =
        Section::with_paragraphs(vec![text_para("소속: (   )"), host], PageSettings::a4());
    section.begin_num =
        Some(BeginNum { page: 5, pic: 0, tbl: 0, equation: 0, ..BeginNum::default() });
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode");
    assert_hancom_numbers(&bytes, "the codec's own output");
    bytes
}

fn edit_cell(bytes: &[u8]) -> Result<hwpforge::ops::edit::SetCellOutput, hwpforge::ops::OpsError> {
    set_cell(bytes, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"))
}

fn stamp_all(bytes: &[u8]) -> Result<hwpforge::ops::stamp::StampOutput, hwpforge::ops::OpsError> {
    let specs: Vec<StampSpec> = stamp_plan(bytes)
        .expect("plan")
        .plan
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker.clone(),
            action: StampAction::Field { name: "소속".into(), hint: None },
        })
        .collect();
    assert!(!specs.is_empty(), "the paren blank is a stamp candidate");
    stamp(bytes, &StampMap::Legacy(specs), &StampOptions::default())
}

/// T6 for the preserve-first editors: set-cell and stamp keep the header at
/// ones and the section's own restart at 5; a header of 0 is repaired and
/// reported; a header start number Core cannot carry is refused.
///
/// 이것을 실패시키는 것: header 를 첫 구역 `begin_num` 에서 만드는 것(header
/// `page="5"`), 고침 경고를 지우는 것, 0 고침을 손실로 분류하는 것(편집 거부),
/// 손실 경고를 지우는 것(편집이 5 를 1 로 다시 씀).
#[test]
fn set_cell_and_stamp_keep_hancoms_header_start_numbers() {
    let source = editable();
    let edited = edit_cell(&source).expect("set-cell");
    assert_hancom_numbers(&edited.bytes, "set-cell");
    let stamped = stamp_all(&source).expect("stamp");
    assert_hancom_numbers(&stamped.bytes, "stamp");

    let zero = with_header_numbers(
        &source,
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="0" tbl="0" equation="0"/>"#,
    );
    let edited = edit_cell(&zero).expect("set-cell repairs a header of 0");
    assert_hancom_numbers(&edited.bytes, "set-cell of a header of 0");
    assert!(
        edited.warnings.iter().any(|w| w.info().code == "BEGIN_NUM_REPAIRED"),
        "{:?}",
        edited.warnings
    );
    let stamped = stamp_all(&zero).expect("stamp repairs a header of 0");
    assert_hancom_numbers(&stamped.bytes, "stamp of a header of 0");
    assert!(
        stamped.warnings.iter().any(|w| w.info().code == "BEGIN_NUM_REPAIRED"),
        "{:?}",
        stamped.warnings
    );

    let lost = with_header_numbers(
        &source,
        r#"<hh:beginNum page="5" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/>"#,
    );
    for (surface, err) in [
        ("set-cell", edit_cell(&lost).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&lost).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("begin_num"), "{surface}: {err}");
    }
}

/// A header with a 0 to repair and values to lose at once is refused: the
/// loss is not hidden behind the repair.
///
/// 이것을 실패시키는 것: 고침 경고 뒤에 조기 반환하는 것, 손실 판정에서
/// tbl·equation 을 빼는 것.
#[test]
fn a_mixed_header_is_refused_by_the_editors() {
    let mixed = with_header_numbers(
        &editable(),
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="1" tbl="2" equation="3"/>"#,
    );
    let opts = RestyleOptions::default().with_preset("modern");
    for (surface, err) in [
        ("restyle", restyle(&mixed, &opts).map(|_| ()).unwrap_err()),
        ("set-cell", edit_cell(&mixed).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&mixed).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("begin_num"), "{surface}: {err}");
    }
}
