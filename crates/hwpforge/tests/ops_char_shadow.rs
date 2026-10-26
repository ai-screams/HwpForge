#![cfg(feature = "ops-hwpx")]
#[path = "support/ops_char_shadow.rs"]
mod shared;
use shared::*;

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
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
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

/// `bytes` with the first `<hh:shadow>` type set to `SPARKLE`.
fn with_unknown_shadow_type(bytes: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == "Contents/header.xml" {
            let h = String::from_utf8(data).expect("utf-8");
            assert!(h.contains(r#"<hh:shadow type="NONE""#));
            data = h
                .replacen(r#"<hh:shadow type="NONE""#, r#"<hh:shadow type="SPARKLE""#, 1)
                .into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
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

/// 이것을 실패시키는 것: 디코더가 모르는 그림자 종류를 무음으로 NONE 에 접는 것(편집 셋이
/// 모두 통과) / `char_shadow` 손실을 `UnknownEnumValue` 로 분류하는 것.
#[test]
fn unknown_shadow_type_is_refused_by_the_editors() {
    let source = editable();
    let cell = |b: &[u8]| {
        set_cell(b, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("쇠부리"))
    };
    let opts = RestyleOptions::default().with_preset("modern");
    restyle(&source, &opts).expect("plain restyle");
    cell(&source).expect("plain set-cell");
    stamp_all(&source).expect("plain stamp");

    let lost = with_unknown_shadow_type(&source);
    let decoded = HwpxDecoder::decode(&lost).expect("decode");
    assert!(
        decoded
            .warnings
            .iter()
            .any(|w| w.is_content_loss() && format!("{w:?}").contains("char_shadow")),
        "{:?}",
        decoded.warnings
    );
    for (surface, err) in [
        ("restyle", restyle(&lost, &opts).map(|_| ()).unwrap_err()),
        ("set-cell", cell(&lost).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&lost).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("char_shadow"), "{surface}: {err}");
    }
}

/// The byte-preserving `patch` keeps the original header, so it does not
/// need to refuse: it succeeds and the header bytes are unchanged.
///
/// 이것을 실패시키는 것: `char_shadow` 거부를 patch 까지 넓히는 것.
#[test]
fn patch_keeps_an_unreadable_shadow_byte_for_byte() {
    let lost = with_unknown_shadow_type(&editable());
    let section = export_section(&lost, &ExportSectionOptions::default().with_section(0))
        .expect("export_section")
        .section;
    let opts = PatchOptions::default().with_patch(serde_json::to_string(&section).unwrap());
    let out = patch(&lost, &opts).expect("patch");
    assert_eq!(header(&out.bytes), header(&lost));
}
