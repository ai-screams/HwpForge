//! Character shadows from HWP5 against the HWPX Hancom saved from the same
//! document (S21). The fixture is a new Hancom (macOS 12.30) document:
//! charPr 0–6 NONE `#C0C0C0` 10 10, 7 DROP at the default, 8 DROP 5 5,
//! 9 CONTINUOUS `#123456` −3 4.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

fn fixture(ext: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/user_samples/sample-char-shadow.{ext}"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn header(hwpx: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// The `<hh:shadow …/>` of each `charPr`, in order.
fn shadows(header: &str) -> Vec<String> {
    header
        .match_indices("<hh:shadow ")
        .map(|(at, _)| header[at..at + header[at..].find("/>").expect("end") + 2].to_string())
        .collect()
}

/// 이것을 실패시키는 것: 하드코딩 복귀 · 종류 2 → DROP · BGR 생략(`#563412`) ·
/// 부호 없는 간격(`253`·`4294967293`) · x·y 뒤바꿈 · 종류 0 에서 색 미전달.
#[test]
fn converted_shadows_match_hancom() {
    let (ours, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default())
            .expect("convert");
    let hancom = shadows(&header(&fixture("hwpx")));
    assert_eq!(hancom.len(), 10);
    let ours = shadows(&header(&ours));
    assert_eq!(&ours[..10], &hancom[..]);
    let shadow_warnings: Vec<_> =
        warnings.iter().filter(|w| format!("{w:?}").contains("shadow")).collect();
    assert!(shadow_warnings.is_empty(), "{shadow_warnings:?}");
}
