//! A style reference past the header's definitions fails the decode for a
//! paragraph or run converted to Core: body text, chart and master page.
//!
//! A top-level run's `charPrIDRef` is looked up in more than one place (the
//! section's run walk, `convert_run`, and for a paragraph that yields no Core
//! run the empty-paragraph placeholder), so each test names the lookups that
//! must all go for it to fail.

#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;

use hwpforge_core::chart::{ChartData, ChartType};
use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::{MasterPage, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{ApplyPageType, CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use std::io::{Cursor, Read, Write};

const SECTION: &str = "Contents/section0.xml";
const MASTER: &str = "Contents/masterpage0.xml";

fn paragraph(run: Run) -> Paragraph {
    Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))
}

/// A body text paragraph, a chart paragraph and a master page paragraph,
/// with one `charPr` and one `paraPr` in the header.
fn source() -> Vec<u8> {
    let text = |s: &str| paragraph(Run::text(s, CharShapeIndex::new(0)));
    let chart = Control::chart(ChartType::Column, ChartData::category(&["C"], &[("S", &[1.0])]));
    let mut section = Section::with_paragraphs(
        vec![text("본문"), paragraph(Run::control(chart, CharShapeIndex::new(0)))],
        PageSettings::a4(),
    );
    section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![text("바탕")])]);
    let mut doc = Document::new();
    doc.add_section(section);
    HwpxEncoder::encode(&doc.validate().unwrap(), &encoding::dotum_styles(), &ImageStore::new())
        .unwrap()
}

/// `bytes` with the one occurrence of `from` in entry `name` replaced by `to`.
fn splice(bytes: &[u8], name: &str, from: &str, to: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if entry_name == name {
            let text = String::from_utf8(data).unwrap();
            assert_eq!(text.matches(from).count(), 1, "{from} in {name}");
            data = text.replacen(from, to, 1).into_bytes();
        }
        out.start_file(entry_name, options).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// Sets the reference at `from` (whose `0` is the reference) to the
/// definition count, which must fail, and to the last definition, which
/// must decode.
fn assert_bounded(name: &str, from: &str, kind: &str) {
    let source = source();
    let header = package::part(&source, "Contents/header.xml");
    let count = header.matches(&format!("<hh:{kind} ")).count();
    assert_eq!(count, 1, "{kind} definitions");
    let with_id = |id: usize| {
        let to = from.replacen(&format!("{kind}IDRef=\"0\""), &format!("{kind}IDRef=\"{id}\""), 1);
        assert!(to.contains(&format!("{kind}IDRef=\"{id}\"")), "{to}");
        HwpxDecoder::decode(&splice(&source, name, from, &to))
    };
    let error = with_id(count).expect_err("reference past the header").to_string();
    assert!(error.contains(&format!("broken {kind} reference ID {count}")), "{error}");
    with_id(count - 1).expect("last definition decodes");
}

// 이것을 실패시키는 것: 본문 문단의 `ctx.para_index` 를 `ParaShapeIndex::new` 로 바꾸기.
#[test]
fn body_paragraph_reference_is_bounded() {
    assert_bounded(SECTION, r#"<hp:p id="0" paraPrIDRef="0""#, "paraPr");
}

// 이것을 실패시키는 것: 구역 run 순회와 `convert_run` 의 `ctx.char_index` 를 함께
// `CharShapeIndex::new` 로 바꾸기(하나만 바꾸면 다른 하나가 거부한다).
#[test]
fn body_run_reference_is_bounded() {
    assert_bounded(SECTION, r#"<hp:run charPrIDRef="0"><hp:secPr"#, "charPr");
}

// 이것을 실패시키는 것: 구역 run 순회, `convert_run`, 빈 문단 자리 run 의 `ctx.char_index`
// 셋을 함께 `CharShapeIndex::new` 로 바꾸기(차트만 든 문단은 Core run 이 없어 자리 run 을 만든다).
#[test]
fn chart_run_reference_is_bounded() {
    assert_bounded(SECTION, r#"<hp:run charPrIDRef="0"><hp:switch>"#, "charPr");
}

// 이것을 실패시키는 것: 바탕쪽 디코드에 헤더 참조 대신 `None` 을 넘기기, 또는 `convert_run` 의
// `ctx.char_index` 만 바꾸기(바탕쪽 run 은 구역 run 순회를 거치지 않는다).
#[test]
fn master_page_references_are_bounded() {
    assert_bounded(MASTER, r#"<hp:p id="0" paraPrIDRef="0""#, "paraPr");
    assert_bounded(MASTER, r#"<hp:run charPrIDRef="0"><hp:t>"#, "charPr");
}
