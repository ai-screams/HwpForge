#[allow(dead_code)]
#[path = "support/standalone_rotation.rs"]
mod shared;
use shared::*;

use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use std::collections::BTreeMap;
fn fixture(dir: &str, name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/{dir}/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
}

/// 이것을 실패시키는 것: 이동량 0인 9종의 offset 외 속성·행렬·패키지 변경.
#[test]
fn zero_translation_nine_kinds_match_6a0e7d6f_bytes() {
    use std::io::Read;
    let head = fixture("standalone-rotation", "head-6a0-zero-nine.hwpx");
    let objects = wire(&head);
    assert_eq!(objects.len(), 9);
    // 이것을 실패시키는 것: B' 기본 NORMAL을 Some으로 강제하거나 EllipseSweep 출력 변경.
    let decoded = HwpxDecoder::decode(&head).unwrap();
    let mut ellipse_arcs = 0;
    decoded.document.for_each_paragraph(|p| {
        for r in &p.runs {
            if let hwpforge_core::RunContent::Control(c) = &r.content {
                if let hwpforge_core::Control::Arc { definition, .. } = c.as_ref() {
                    assert_eq!(*definition, hwpforge_core::control::ArcDefinition::EllipseSweep);
                    ellipse_arcs += 1;
                }
            }
        }
    });
    assert_eq!(ellipse_arcs, 1);
    for o in objects {
        if let Some(line) = o.fields.get("lineShape") {
            for side in ["head", "tail"] {
                assert_eq!(line[&format!("{side}Style")], "NORMAL");
                assert_eq!(line[&format!("{side}Sz")], "MEDIUM_MEDIUM");
                assert_eq!(line[&format!("{side}fill")], "1");
            }
        }
        assert_eq!((o.v("offset", "x"), o.v("offset", "y")), (0, 0));
        assert_eq!((o.matrices[0][2], o.matrices[0][5]), (0., 0.));
    }
    let parts = |bytes: &[u8]| {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut parts = BTreeMap::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            parts.insert(entry.name().unwrap().into_owned(), data);
        }
        parts
    };
    let actual = parts(&reencode(&head));
    let expected = parts(&head);
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[&name] == bytes, "ZIP part {name} differs, no ID/XML normalization");
    }
}
