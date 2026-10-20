//! Hancom fixture 32: fill values survive both codecs and the existing group fold.
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_core::{
    control::{Fill, GroupChild},
    image::ImageStore,
    Control, Document, Draft, RunContent,
};
use hwpforge_foundation::{Color, GradientType, ImageFillMode, PatternType};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{events::Event, Reader};

type Attrs = BTreeMap<String, String>;

#[derive(Debug, PartialEq)]
struct RectFill {
    ratio: u8,
    brush: Vec<(String, Attrs)>,
    image: Vec<u8>,
}

fn fixture(ext: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/shapes/fills.{ext}", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
}

/// Compare all fill attributes and ordered colours, resolving asset IDs to
/// bytes rather than assuming the two packages use the same asset names.
fn wire_fills(bytes: &[u8]) -> Vec<Vec<RectFill>> {
    let xml = PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut groups = Vec::<Vec<RectFill>>::new();
    let mut active = false;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let tag = e.local_name().as_ref().to_string();
                let mut attrs: Attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (a.key.as_ref().to_string(), a.value.to_string())
                    })
                    .collect();
                match tag.as_str() {
                    "container" => groups.push(Vec::new()),
                    "rect" => {
                        groups.last_mut().unwrap().push(RectFill {
                            ratio: attrs["ratio"].parse().unwrap(),
                            brush: Vec::new(),
                            image: Vec::new(),
                        });
                        active = true;
                    }
                    "winBrush" | "gradation" | "color" | "imgBrush" | "img" if active => {
                        let rect = groups.last_mut().unwrap().last_mut().unwrap();
                        // Hancom uses winBrush faceColor="none"; our writer
                        // omits fillBrush (since HEAD). Compare Core no-fill
                        // below; changing writer spelling is deferred until
                        // the user checks Hancom's fill editing UI.
                        if tag == "winBrush" && attrs["faceColor"] == "none" {
                            assert!(!attrs.contains_key("hatchStyle"));
                            continue;
                        }
                        if tag == "img" {
                            let id = attrs.remove("binaryItemIDRef").unwrap();
                            let prefix = format!("BinData/{id}.");
                            let index = (0..zip.len())
                                .find(|&i| {
                                    zip.by_index(i).unwrap().name().unwrap().starts_with(&prefix)
                                })
                                .expect("picture fill must reference a packaged asset");
                            zip.by_index(index).unwrap().read_to_end(&mut rect.image).unwrap();
                            assert_eq!(rect.image.len(), 314);
                        }
                        rect.brush.push((tag, attrs));
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.local_name().as_ref() == "rect" => active = false,
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|g| g.len() == 6));
    groups
}

fn core_fills(doc: &Document<Draft>, images: &ImageStore, image: &[u8]) -> Vec<Vec<(Fill, u8)>> {
    let mut groups = Vec::new();
    doc.for_each_paragraph(|p| {
        for run in &p.runs {
            let RunContent::Control(c) = &run.content else { continue };
            let Control::Group { children, .. } = c.as_ref() else { continue };
            let mut values = Vec::new();
            for child in children {
                let GroupChild::Shape(Control::Rect { style: Some(style), .. }) = child else {
                    panic!("expected rectangle, got {child:?}")
                };
                let mut fill = style.fill.clone().unwrap_or_else(|| Fill::Solid {
                    color: style.fill_color.expect("missing fill is not Core no-fill"),
                });
                if let Fill::Image { image_id, .. } = &mut fill {
                    let stem = image_id.trim_start_matches("BinData/");
                    let assets: Vec<_> = images
                        .iter()
                        .filter(|(key, _)| {
                            let key = key.trim_start_matches("BinData/");
                            key == stem || key.split('.').next() == Some(stem)
                        })
                        .collect();
                    assert_eq!(assets.len(), 1, "unresolved/ambiguous asset {image_id}");
                    assert_eq!(assets[0].1, image);
                    *image_id = "fixture32-image".into();
                }
                values.push((fill, style.corner_ratio.unwrap_or(0)));
            }
            groups.push(values);
        }
    });
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|g| g.len() == 6));
    groups
}

/// 이것을 실패시키는 것: 두 경로의 없음/단색/그라데이션 속성/무늬/그림 모드·자산/
/// 곡률 누락, 묶음 B 변환에 따라 채우기 값 변경, A/B 자식 누락·순서 변경.
#[test]
fn twelve_group_fills_match_hancom_through_both_codecs() {
    let truth = fixture("hwpx");
    let expected_wire = wire_fills(&truth);
    assert_eq!(
        expected_wire[0], expected_wire[1],
        "fill values are independent of group transform"
    );
    assert_eq!(expected_wire[0][4].brush[0].1["hatchStyle"], "SLASH");
    assert_eq!(expected_wire[0][5].brush[0].1["mode"], "TOTAL");
    let image = &expected_wire[0][5].image;
    let expected_core = vec![
        (Fill::None, 0),
        (Fill::Solid { color: Color::RED }, 0),
        (Fill::Solid { color: Color::WHITE }, 20),
        (
            Fill::Gradient {
                gradient_type: GradientType::Linear,
                angle: 45,
                colors: vec![
                    (Color::from_rgb(0x86, 0xAF, 0xDC), 0),
                    (Color::from_rgb(0x2D, 0x62, 0x9C), 0),
                ],
                center_x: Some(0),
                center_y: Some(0),
                step: Some(255),
                step_center: Some(50),
            },
            0,
        ),
        (
            Fill::Pattern {
                pattern_type: PatternType::BackSlash,
                fg_color: Color::BLACK,
                bg_color: Color::WHITE,
            },
            0,
        ),
        (Fill::Image { image_id: "fixture32-image".into(), mode: ImageFillMode::Stretch }, 0),
    ];
    let expected_core = vec![expected_core.clone(), expected_core];
    let source = hwpforge_smithy_hwp5::Hwp5Decoder::decode(&fixture("hwp")).unwrap();
    assert_eq!(core_fills(&source.document, &source.image_store, image), expected_core);
    let (converted, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(wire_fills(&converted), expected_wire);
    for input in [&truth, &converted] {
        let decoded = HwpxDecoder::decode(input).unwrap();
        assert_eq!(core_fills(&decoded.document, &decoded.image_store, image), expected_core);
        let output = HwpxEncoder::encode(
            &decoded.document.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        assert_eq!(wire_fills(&output), expected_wire);
        let roundtrip = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(core_fills(&roundtrip.document, &roundtrip.image_store, image), expected_core);
    }
}
