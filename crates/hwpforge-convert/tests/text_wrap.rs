//! HWP5 object text wrap (§19 E) against Hancom's own HWPX save.
//!
//! X2: four floating rectangles, set in Hancom to wrap 어울림 (square),
//! 자리 차지 (top and bottom), 글 앞으로 (in front of text) and 글 뒤로
//! (behind text), top to bottom. Hancom writes their property words with
//! bits 21–23 = 0, 1, 3, 2 — a four-value table, not the six values of the
//! published spec — and its HWPX save of the same file is the answer.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/shapes").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn section0(hwpx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    xml
}

fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle).unwrap_or_else(|| panic!("{name} in {tag}")) + needle.len();
    &tag[at..at + tag[at..].find('"').expect("closing quote")]
}

/// Each rectangle's `textWrap`, keyed by its `vertOffset` (the four are
/// stacked down the page, so the offset pairs them across the two files).
fn rect_wraps(xml: &str) -> BTreeMap<String, String> {
    xml.match_indices("<hp:rect ")
        .map(|(at, _)| {
            let rect = &xml[at..at + xml[at..].find('>').expect("tag end")];
            let pos_at = at + xml[at..].find("<hp:pos ").expect("pos");
            let pos = &xml[pos_at..pos_at + xml[pos_at..].find('>').expect("pos end")];
            (attr(pos, "vertOffset").to_string(), attr(rect, "textWrap").to_string())
        })
        .collect()
}

/// E-T3: the converted rectangles wrap as Hancom's own HWPX of the same
/// file does, one by one.
///
/// 이것을 실패시키는 것: 매핑 표의 1·2·3 중 둘을 맞바꾸는 것(각각), 6값
/// 스펙 표로 되돌리는 것.
#[test]
fn x2_wrap_modes_match_hancom() {
    let (hwpx, _) = hwp5_to_hwpx_bytes_with_diagnostics(
        &fixture("x2-wrap-modes.hwp"),
        ConvertOptions::default(),
    )
    .expect("convert");
    let ours = rect_wraps(&section0(&hwpx));
    let hancom = rect_wraps(&section0(&fixture("x2-wrap-modes.hwpx")));
    assert_eq!(hancom.len(), 4, "{hancom:?}");
    assert_eq!(
        hancom.values().map(String::as_str).collect::<Vec<_>>(),
        ["SQUARE", "TOP_AND_BOTTOM", "IN_FRONT_OF_TEXT", "BEHIND_TEXT"],
        "the fixture as made, top to bottom"
    );
    assert_eq!(ours, hancom);
}
