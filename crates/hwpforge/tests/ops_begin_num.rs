//! The re-encoding surfaces write Hancom's header start numbers (§19 G, T6).
//!
//! `restyle` and `from_json` (with and without a base) re-encode through
//! the HWPX encoder; each must keep the header `<hh:beginNum>` at 1 and the
//! section's own restart (`<hp:startNum page="5">`, Hancom's F16b) where
//! Hancom put it. Before, the header took the section's 5.
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read};

use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
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
