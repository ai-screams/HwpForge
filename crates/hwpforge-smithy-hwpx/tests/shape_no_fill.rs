//! 채우기 없는 도형은 다시 써도 채우기가 없다.
//!
//! 한컴은 채우기 없는 도형에 `<hc:fillBrush>` 를 쓰지 않는다. Core 의
//! `ShapeStyle.fill == None` 은 "인코더 기본값(흰 채우기)" 이라, 디코더가 이
//! 부재를 [`Fill::None`] 으로 올리지 않으면 재인코드가 흰 채우기를 덧씌워 아래
//! 내용을 가린다. 정답 fixture = 한컴 저장본 `tests/fixtures/shapes/polygon_simple.hwpx`
//! (fillBrush 없는 다각형 1개).

use std::io::{Cursor, Read};

use hwpforge_core::control::{Control, Fill, ShapeStyle};
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn fixture(rel: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{rel}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).expect("part").read_to_string(&mut out).unwrap();
    out
}

fn polygon_fill(bytes: &[u8]) -> Option<Fill> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let mut fill = None;
    doc.document.for_each_paragraph(|p| {
        for run in &p.runs {
            if let RunContent::Control(c) = &run.content {
                if let Control::Polygon { style, .. } = c.as_ref() {
                    fill = Some(style.as_ref().and_then(|s: &ShapeStyle| s.fill.clone()));
                }
            }
        }
    });
    fill.expect("the fixture has a polygon")
}

// 이것을 실패시키는 것: 디코더의 `Fill::None` 대입을 지우기(재인코드가 흰
// fillBrush 를 씀), 또는 인코더의 `Fill::None` 갈래를 지우기(같음).
#[test]
fn a_hancom_shape_without_fill_stays_without_fill_through_reencode() {
    let original = fixture("shapes/polygon_simple.hwpx");
    assert!(!part(&original, "Contents/section0.xml").contains("<hc:fillBrush"), "premise");
    assert_eq!(polygon_fill(&original), Some(Fill::None));

    let doc = HwpxDecoder::decode(&original).expect("decode");
    let validated = doc.document.validate().expect("validate");
    let again =
        HwpxEncoder::encode(&validated, &doc.style_store, &doc.image_store).expect("encode");
    let section = part(&again, "Contents/section0.xml");
    assert!(section.contains("<hp:polygon"), "{section}");
    assert!(!section.contains("<hc:fillBrush"), "no-fill shape got a fill: {section}");
    assert_eq!(polygon_fill(&again), Some(Fill::None));
}
