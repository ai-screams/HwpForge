//! Hancom's assetless picture frame survives regenerating edits.
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read};

use hwpforge::ops::{restyle, RestyleOptions};

/// 이것을 실패시키는 것: 빈 참조 그림 버리기 / 빈 참조에 CONTENT_LOST 추가 /
/// C4 빈 참조 제외 삭제(restyle 의미 손상 게이트가 거부).
#[test]
fn assetless_f6_restyle_preserves_both_pictures() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/images/f6-dangling-image-resaved.hwpx"
    ))
    .unwrap();
    let out = restyle(&bytes, &RestyleOptions::default().with_preset("modern")).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(out.bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert_eq!(xml.matches("<hp:pic ").count(), 2);
    assert!(xml.contains(r#"binaryItemIDRef="""#));
    assert!(xml.contains(r#"binaryItemIDRef="image1""#));
}
