#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read, Write};

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// A stamp candidate (`(   )`) and a 1×2 label table.
fn editable() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let section =
        Section::with_paragraphs(vec![text_para("소속: (   )"), host], PageSettings::a4());
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// `bytes` with `para` appended as one more body paragraph.
fn with_paragraph(bytes: &[u8], para: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == "Contents/section0.xml" {
            let xml = String::from_utf8(data).expect("utf-8");
            let at = xml.rfind("</hs:sec>").expect("section end");
            data = format!("{}{para}{}", &xml[..at], &xml[at..]).into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

const ATTRS: &str =
    r#"id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0""#;

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

fn lost_kinds(bytes: &[u8]) -> Vec<String> {
    HwpxDecoder::decode(bytes)
        .expect("decode")
        .warnings
        .iter()
        .filter(|w| w.is_content_loss())
        .map(|w| format!("{w:?}"))
        .collect()
}

/// 이것을 실패시키는 것: `convert_paragraph_from` 의 `paragraph_content` 보고
/// 삭제 (둘째 줄 이하 모두 빨개짐), 또는 `HxParagraph::has_stray_content` 가
/// 요소나 글을 놓치는 것(`stray_paragraph_content`).
#[test]
fn a_paragraph_with_content_outside_a_run_is_reported_lost() {
    let source = editable();
    let cases = [
        ("text directly under p", format!("<hp:p {ATTRS}><hp:t>가상</hp:t></hp:p>")),
        ("unknown element under p", format!("<hp:p {ATTRS}><hp:imaginary/></hp:p>")),
        (
            "bare text under p",
            format!(r#"<hp:p {ATTRS}>가상<hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run></hp:p>"#),
        ),
    ];
    let opts = RestyleOptions::default().with_preset("modern");
    for (label, para) in cases {
        let bytes = with_paragraph(&source, &para);
        let lost = lost_kinds(&bytes);
        assert_eq!(lost.len(), 1, "{label}: {lost:?}");
        assert!(lost[0].contains("paragraph_content"), "{label}: {lost:?}");
        // The edit surfaces refuse it like any semantic loss.
        for (surface, err) in [
            ("restyle", restyle(&bytes, &opts).map(|_| ()).unwrap_err()),
            (
                "set-cell",
                set_cell(
                    &bytes,
                    &SetCellOptions::default().with_table(0).with_at("0,1").with_text("쇠부리"),
                )
                .map(|_| ())
                .unwrap_err(),
            ),
            ("stamp", stamp_all(&bytes).map(|_| ()).unwrap_err()),
        ] {
            assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{label} {surface}: {err}");
            assert!(err.to_string().contains("paragraph_content"), "{label} {surface}: {err}");
        }
        // Reading succeeds and says what it lost.
        let md = hwpforge::ops::markdown::to_md(&bytes, &Default::default()).expect("to-md");
        assert!(
            md.warnings.iter().any(|w| format!("{w:?}").contains("paragraph_content")),
            "{label}: {:?}",
            md.warnings
        );
    }
}

/// 이것을 실패시키는 것: 판정을 "run 이 둘 미만이면 보고" 처럼 넓히기
/// (`<hp:linesegarray>` 만 있거나 자기닫힘인 문단이 빨개진다), 또는 읽기가
/// 공백 텍스트를 넘기게 되는 것(공백 낀 문단).
#[test]
fn ordinary_paragraphs_gain_no_warning() {
    let source = editable();
    let line = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/></hp:linesegarray>"#;
    let cases = [
        format!(r#"<hp:p {ATTRS}><hp:run charPrIDRef="0"><hp:t>가상</hp:t></hp:run></hp:p>"#),
        format!("<hp:p {ATTRS}>{line}</hp:p>"),
        format!("<hp:p {ATTRS}/>"),
        format!("<hp:p {ATTRS}>   \n </hp:p>"),
        format!("<hp:p {ATTRS}>\n  <hp:run charPrIDRef=\"0\"><hp:t>가상</hp:t></hp:run>\n  {line}\n</hp:p>"),
    ];
    for para in cases {
        let bytes = with_paragraph(&source, &para);
        assert!(lost_kinds(&bytes).is_empty(), "{para}: {:?}", lost_kinds(&bytes));
    }
}

/// 이것을 실패시키는 것: 본문 경로가 Core 문단을 비운다는 가정이 깨지는 것
/// (소실 보고 옆에서도 Core 문단은 종전처럼 빈 문단 하나).
#[test]
fn the_core_paragraph_stays_empty() {
    let bytes = with_paragraph(&editable(), &format!("<hp:p {ATTRS}><hp:t>가상</hp:t></hp:p>"));
    let doc = HwpxDecoder::decode(&bytes).expect("decode").document;
    let last = doc.sections()[0].paragraphs.last().expect("paragraph");
    assert_eq!(last.text_content(), "");
}
