#![cfg(feature = "ops-hwpx")]
#[path = "support/document.rs"]
mod document;
use document::editable;
#[path = "support/stamping.rs"]
mod stamping;
use stamping::stamp_all;
#[path = "support/ops_char_shadow.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read, Write};

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

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
