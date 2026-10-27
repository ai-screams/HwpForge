//! A `charPr` whose ratio differs between languages is lost content:
//! the package decode reports it, and the editors that
//! re-encode their input — restyle, set-cell, stamp — refuse it rather than
//! write the hangul ratio for every language. The input is a package this
//! codec wrote (all three editors accept it), its header edited.
#![cfg(feature = "ops-hwpx")]

#[path = "support/document.rs"]
mod document;
use document::editable;
#[path = "support/stamping.rs"]
mod stamping;
use stamping::stamp_all;
use std::io::{Cursor, Read, Write};

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

/// `bytes` with the first `charPr`'s latin ratio set to 90.
fn with_latin_ratio_90(bytes: &[u8]) -> Vec<u8> {
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
            let header = String::from_utf8(data).expect("utf-8");
            let at = header.find("<hh:ratio ").expect("ratio");
            let latin = at + header[at..].find("latin=\"").expect("latin") + "latin=\"".len();
            let end = latin + header[latin..].find('"').expect("end");
            data = format!("{}90{}", &header[..latin], &header[end..]).into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// 이것을 실패시키는 것: 패키지 디코드가 header 경고를 합치지 않는 것(편집 셋이
/// 모두 통과) / 언어별 비교를 지우는 것.
#[test]
fn per_language_ratio_is_reported_and_refused_by_the_editors() {
    let source = editable();
    let cell = |b: &[u8]| {
        set_cell(b, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"))
    };
    let opts = RestyleOptions::default().with_preset("modern");
    restyle(&source, &opts).expect("plain restyle");
    cell(&source).expect("plain set-cell");
    stamp_all(&source).expect("plain stamp");

    let lost = with_latin_ratio_90(&source);
    let decoded = HwpxDecoder::decode(&lost).expect("decode");
    assert!(
        decoded
            .warnings
            .iter()
            .any(|w| w.is_content_loss() && format!("{w:?}").contains("char_shape_per_language")),
        "{:?}",
        decoded.warnings
    );
    for (surface, err) in [
        ("restyle", restyle(&lost, &opts).map(|_| ()).unwrap_err()),
        ("set-cell", cell(&lost).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&lost).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("char_shape_per_language"), "{surface}: {err}");
    }
}
