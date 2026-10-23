//! P21: the HWP5 paragraph shape's "한 줄로 입력" (property2 bits 0-1)
//! against the HWPX Hancom saved from the same document. The fixture is a
//! Hancom (macOS) document whose third body paragraph uses para shape 20,
//! the only definition with the option on (서식 → 문단 모양 → 확장 → 한 줄로
//! 입력).

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

fn fixture(ext: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/user_samples/sample-single-line-input.{ext}"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn header(hwpx: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// `(paraPr id, lineWrap)` for every paragraph shape, in order.
fn line_wraps(header: &str) -> Vec<(u32, String)> {
    header
        .split("<hh:paraPr id=\"")
        .skip(1)
        .map(|rest| {
            let id = rest[..rest.find('"').unwrap()].parse().unwrap();
            let body = &rest[..rest.find("</hh:paraPr>").unwrap()];
            let at = body.find("lineWrap=\"").expect("lineWrap") + "lineWrap=\"".len();
            (id, body[at..at + body[at..].find('"').unwrap()].to_string())
        })
        .collect()
}

/// 이것을 실패시키는 것: SQUEEZE 대입 삭제(paraPr 20 이 BREAK) · 값 0 을
/// SQUEEZE 로(나머지 20 개가 SQUEEZE).
#[test]
fn single_line_input_matches_hancom_line_wrap() {
    let (ours, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default())
            .expect("convert");
    let hancom = line_wraps(&header(&fixture("hwpx")));
    let ours = line_wraps(&header(&ours));
    assert_eq!(hancom.len(), 21);
    assert_eq!(ours, hancom);
    let squeezed: Vec<u32> =
        ours.iter().filter(|(_, wrap)| wrap == "SQUEEZE").map(|(id, _)| *id).collect();
    assert_eq!(squeezed, [20]);
    let line_wrap_warnings: Vec<_> =
        warnings.iter().filter(|w| format!("{w:?}").contains("line_wrap")).collect();
    assert!(line_wrap_warnings.is_empty(), "{line_wrap_warnings:?}");
}
