//! Hancom fixture 32: fill values survive both codecs and the existing group fold.
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

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

fn rect_xml(bytes: &[u8]) -> Vec<String> {
    let xml = PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap();
    xml.match_indices("<hp:rect ")
        .map(|(start, _)| {
            let end = start + xml[start..].find("</hp:rect>").unwrap() + "</hp:rect>".len();
            xml[start..end].to_string()
        })
        .collect()
}

type Matrix = [f64; 6];
const IDENTITY: Matrix = [1., 0., 0., 0., 1., 0.];
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}
fn apply(m: Matrix, p: (f64, f64)) -> (f64, f64) {
    (m[0] * p.0 + m[1] * p.1 + m[2], m[3] * p.0 + m[4] * p.1 + m[5])
}

struct BrushGeometry {
    org: (f64, f64),
    level: usize,
    pairs: Vec<(Matrix, Matrix)>,
    corners: Vec<(f64, f64)>,
    points: Vec<(f64, f64)>,
    angle: i32,
}

fn brush_geometry(xml: &str) -> BrushGeometry {
    let mut reader = Reader::from_str(xml);
    let mut org = (0., 0.);
    let mut level = 0;
    let mut angle = 0;
    let mut points = Vec::new();
    let mut pairs = Vec::new();
    let mut trans = IDENTITY;
    let mut scale = IDENTITY;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let attrs: Attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (a.key.as_ref().to_string(), a.value.to_string())
                    })
                    .collect();
                let n = |key: &str| attrs[key].parse::<f64>().unwrap();
                let matrix = || [n("e1"), n("e2"), n("e3"), n("e4"), n("e5"), n("e6")];
                match e.local_name().as_ref() {
                    "rect" => level = attrs["groupLevel"].parse().unwrap(),
                    "orgSz" => org = (n("width"), n("height")),
                    "rotationInfo" => angle = attrs["angle"].parse().unwrap(),
                    "transMatrix" => trans = matrix(),
                    "scaMatrix" => scale = matrix(),
                    "rotMatrix" => pairs.push((scale, matrix())),
                    "pt0" | "pt1" | "pt2" | "pt3" => points.push((n("x"), n("y"))),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let chain = pairs.iter().fold(trans, |m, (s, r)| multiply(multiply(m, *r), *s));
    let corners =
        [(0., 0.), (org.0, 0.), org, (0., org.1)].into_iter().map(|p| apply(chain, p)).collect();
    BrushGeometry { org, level, pairs, corners, points, angle }
}

fn plain_rect_hashes(bytes: &[u8]) -> Vec<u64> {
    // Stable FNV-1a over the exact XML bytes, frozen from 9d0e1d91's
    // ordinary encoder in both input routes (not edit-base raw copy).
    rect_xml(bytes)
        .iter()
        .filter(|s| !s.contains("<hc:imgBrush"))
        .map(|s| {
            s.bytes()
                .fold(0xcbf29ce484222325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
        })
        .collect()
}

/// 이것을 실패시키는 것: HWP5/HWPX 중 하나만 O 보존, 그림 쌍 수 1로 복귀,
/// native B의 rotationInfo=180 복사, 비그림 자식의 writer 바이트 변경.
#[test]
fn image_fill_original_box_pairs_and_corners_match_hancom_both_routes() {
    let truth = fixture("hwpx");
    let native: Vec<_> = rect_xml(&truth)
        .iter()
        .filter(|x| x.contains("<hc:imgBrush"))
        .map(|x| brush_geometry(x))
        .collect();
    assert_eq!(native[1].angle, 180, "native metadata differs from the composed transform");
    let source = HwpxDecoder::decode(&truth).unwrap();
    assert!(source.warnings.is_empty(), "{:?}", source.warnings);
    let encoded = HwpxEncoder::encode(
        &source.document.validate().unwrap(),
        &source.style_store,
        &source.image_store,
    )
    .unwrap();
    let (converted, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    for bytes in [converted, encoded] {
        assert_eq!(
            plain_rect_hashes(&bytes),
            [
                0x4a39446168f37dac,
                0xddd0882e09be0092,
                0xb083a74f64df8085,
                0xa009e966aae82a98,
                0xfe1e1925a6799571,
                0x2f23eae8a73e4db9,
                0x88e6b382e26c2238,
                0x0b3c61b7d9039649,
                0x0179533547b1288b,
                0x42873f552ab0b678,
            ]
        );
        let xml = rect_xml(&bytes);
        let images: Vec<_> = xml.iter().filter(|x| x.contains("<hc:imgBrush")).collect();
        assert_eq!(images.len(), 2);
        for (i, raw) in images.iter().enumerate() {
            let got = brush_geometry(raw);
            assert_eq!(got.org, (9800., 2250.));
            assert_eq!(got.level, 1);
            assert_eq!(got.pairs.len(), got.level + 1);
            assert_eq!(got.pairs[0], (IDENTITY, IDENTITY), "probe candidate 1");
            assert_eq!(got.pairs[1].0, [1., 0., 0., 0., 4.822222, 0.]);
            assert_eq!(got.points, [(0., 0.), (9800., 0.), (9800., 2250.), (0., 2250.)]);
            assert_eq!(got.angle, if i == 0 { 0 } else { 30 });
            for (a, b) in got.corners.iter().zip(&native[i].corners) {
                assert!((a.0 - b.0).hypot(a.1 - b.1) < 2., "{i}: {a:?} != {b:?}");
            }
        }
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        let mut count = 0;
        decoded.document.for_each_paragraph(|p| {
            for run in &p.runs {
                let RunContent::Control(c) = &run.content else { continue };
                let Control::Group { children, .. } = c.as_ref() else { continue };
                for child in children {
                    if let GroupChild::Shape(Control::Rect {
                        width,
                        height,
                        frame,
                        style: Some(style),
                        ..
                    }) = child
                    {
                        if matches!(style.fill, Some(Fill::Image { .. })) {
                            let o = frame.original_size.unwrap();
                            assert_eq!((o.width.as_i32(), o.height.as_i32()), (9800, 2250));
                            assert_eq!((width.as_i32(), height.as_i32()), (9800, 10850));
                            count += 1;
                        }
                    }
                }
            }
        });
        assert_eq!(count, 2);
    }
}

/// 이것을 실패시키는 것: writer에서 groupLevel 1만 보정하거나 group plan의
/// 세 쌍을 쓰지 않기. 조상 배율은 자식에 두 번 적용하지 않는다.
#[test]
fn image_fill_nested_writer_uses_one_pair_per_level() {
    use hwpforge_foundation::{HwpUnit, Size};
    for scaled in [false, true] {
        let mut d = HwpxDecoder::decode(&fixture("hwpx")).unwrap();
        d.document.for_each_paragraph_mut(|p| {
            for run in &mut p.runs {
                let RunContent::Control(c) = &mut run.content else { continue };
                if let Control::Group { children, width, height, frame, .. } = c.as_mut() {
                    let inner = Control::Group {
                        children: std::mem::take(children),
                        width: *width,
                        height: *height,
                        frame: Default::default(),
                        placement: None,
                        inst_id: None,
                    };
                    children.push(GroupChild::Shape(inner));
                    if scaled {
                        frame.original_size = Some(Size::new(
                            HwpUnit::new(4900).unwrap(),
                            HwpUnit::new(7950).unwrap(),
                        ));
                    }
                }
            }
        });
        let output =
            HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
                .unwrap();
        let native: Vec<_> = rect_xml(&fixture("hwpx"))
            .into_iter()
            .filter(|s| s.contains("<hc:imgBrush"))
            .map(|s| brush_geometry(&s))
            .collect();
        let shapes = rect_xml(&output);
        let images: Vec<_> = shapes.iter().filter(|s| s.contains("<hc:imgBrush")).collect();
        assert_eq!(images.len(), 2);
        for (xml, expected) in images.iter().zip(native) {
            let got = brush_geometry(xml);
            assert_eq!((got.level, got.pairs.len()), (2, 3));
            assert_eq!(got.org, (9800., 2250.));
            for (a, b) in got.corners.iter().zip(expected.corners) {
                assert!((a.0 - b.0).hypot(a.1 - b.1) < 2., "{a:?} != {b:?}");
            }
        }
    }
}

/// 이것을 실패시키는 것: per-shape writer가 group plan과 달리 O를 접거나
/// 큰 잔차를 무경고로 쓰거나 거부한 그림 O를 실제 출력에 남기기.
#[test]
fn image_fill_writer_rejects_only_the_picture() {
    use hwpforge_foundation::{HwpUnit, Size};
    let mut d = HwpxDecoder::decode(&fixture("hwpx")).unwrap();
    d.document.for_each_paragraph_mut(|p| {
        for run in &mut p.runs {
            let RunContent::Control(c) = &mut run.content else { continue };
            let Control::Group { children, .. } = c.as_mut() else { continue };
            for child in children {
                let GroupChild::Shape(Control::Rect { width, frame, style: Some(style), .. }) =
                    child
                else {
                    continue;
                };
                if matches!(style.fill, Some(Fill::Image { .. })) {
                    *width = HwpUnit::new(49).unwrap();
                    frame.original_size = Some(Size::new(
                        HwpUnit::new(100_000_000).unwrap(),
                        HwpUnit::new(2250).unwrap(),
                    ));
                }
            }
        }
    });
    // JSON Core has no source provenance: writer admission must stand on its own.
    d.document = serde_json::from_slice(&serde_json::to_vec(&d.document).unwrap()).unwrap();
    let encoded = HwpxEncoder::encode_with_diagnostics(
        &d.document.validate().unwrap(),
        &d.style_store,
        &d.image_store,
        Default::default(),
    )
    .unwrap();
    // Caller-supplied Core is checked by the same full-chain admission.
    assert!(!encoded.warnings.is_empty());
    for raw in rect_xml(&encoded.bytes).iter().filter(|x| x.contains("<hc:imgBrush")) {
        let g = brush_geometry(raw);
        assert_eq!(g.org, (49., 10850.));
        assert_eq!(g.pairs.len(), g.level + 1);
        assert_eq!(g.points[2], (49., 10850.));
    }
}

// Change A's first and last leaves into r1's mixed pair. Keep every other
// source field so HEAD's exact sibling XML can be frozen for both codecs.
fn mixed_geometry(
    index: usize,
    angle: f64,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
    sibling: ((i32, i32), (i32, i32)),
) -> Option<((i32, i32), Matrix)> {
    match index {
        0 => Some((
            sibling.0,
            [
                f64::from(sibling.1 .0) / f64::from(sibling.0 .0),
                0.,
                0.,
                0.,
                f64::from(sibling.1 .1) / f64::from(sibling.0 .1),
                0.,
            ],
        )),
        5 => {
            let (s, c) = angle.to_radians().sin_cos();
            let (sx, sy) = (
                f64::from(display.0) / f64::from(original.0),
                f64::from(display.1) / f64::from(original.1),
            );
            Some((
                original,
                [sx * c, -sy * s, f64::from(shift.0), sx * s, sy * c, f64::from(shift.1)],
            ))
        }
        _ => None,
    }
}

fn mixed_hwpx(angle: f64) -> Vec<u8> {
    let (o, d) = if angle == 0. || angle > 350. {
        ((100_000_000, 100), (49, 100))
    } else {
        ((5000, 500), (10000, 1000))
    };
    mixed_hwpx_case(angle, o, d, (0, 0))
}

fn mixed_hwp5(angle: f64) -> Vec<u8> {
    let (o, d) = if angle == 0. || angle > 350. {
        ((100_000_000, 100), (49, 100))
    } else {
        ((5000, 500), (10000, 1000))
    };
    mixed_hwp5_case(angle, o, d, (0, 0))
}

fn mixed_hwpx_case(
    angle: f64,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
) -> Vec<u8> {
    mixed_hwpx_with_sibling(angle, original, display, shift, ((100, 50), (200, 100)))
}

fn mixed_hwpx_with_sibling(
    angle: f64,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
    sibling: ((i32, i32), (i32, i32)),
) -> Vec<u8> {
    let input = fixture("hwpx");
    let mut zip = zip::ZipArchive::new(Cursor::new(&input)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let rects = rect_xml(&input);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if entry.name().unwrap() == "Contents/section0.xml" {
            let mut xml = String::from_utf8(data).unwrap();
            for (index, old) in rects.iter().enumerate() {
                let Some(((w, h), matrix)) =
                    mixed_geometry(index, angle, original, display, shift, sibling)
                else {
                    continue;
                };
                let mut new = old.clone();
                // Replace entire observed tags rather than depending on attribute order.
                for (tag, replacement) in [
                    ("hp:orgSz", format!(r#"<hp:orgSz width="{w}" height="{h}"/>"#)),
                    ("hp:curSz", format!(r#"<hp:curSz width="{}" height="{}"/>"#, w * 2, h * 2)),
                    ("hc:pt0", r#"<hc:pt0 x="0" y="0"/>"#.into()),
                    ("hc:pt1", format!(r#"<hc:pt1 x="{w}" y="0"/>"#)),
                    ("hc:pt2", format!(r#"<hc:pt2 x="{w}" y="{h}"/>"#)),
                    ("hc:pt3", format!(r#"<hc:pt3 x="0" y="{h}"/>"#)),
                ] {
                    let start = new.find(&format!("<{tag} ")).unwrap();
                    let end = start + new[start..].find("/>").unwrap() + 2;
                    new.replace_range(start..end, &replacement);
                }
                let mut rendering = String::from("<hp:renderingInfo>");
                for (tag, m) in [
                    ("transMatrix", IDENTITY),
                    ("scaMatrix", IDENTITY),
                    ("rotMatrix", IDENTITY),
                    ("scaMatrix", IDENTITY),
                    ("rotMatrix", matrix),
                ] {
                    rendering.push_str(&format!(
                        r#"<hc:{tag} e1="{}" e2="{}" e3="{}" e4="{}" e5="{}" e6="{}"/>"#,
                        m[0], m[1], m[2], m[3], m[4], m[5]
                    ));
                }
                rendering.push_str("</hp:renderingInfo>");
                let start = new.find("<hp:renderingInfo>").unwrap();
                let end = new.find("</hp:renderingInfo>").unwrap() + "</hp:renderingInfo>".len();
                new.replace_range(start..end, &rendering);
                xml = xml.replacen(old, &new, 1);
            }
            data = xml.into_bytes();
        }
        out.start_file(entry.name().unwrap(), zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn mixed_hwp5_case(
    angle: f64,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
) -> Vec<u8> {
    mixed_hwp5_with_sibling(angle, original, display, shift, ((100, 50), (200, 100)))
}

fn mixed_hwp5_with_sibling(
    angle: f64,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
    sibling: ((i32, i32), (i32, i32)),
) -> Vec<u8> {
    let mut file = cfb::CompoundFile::open(Cursor::new(fixture("hwp"))).unwrap();
    let mut compressed = Vec::new();
    file.open_stream("/BodyText/Section0").unwrap().read_to_end(&mut compressed).unwrap();
    let mut data = Vec::new();
    flate2::read::DeflateDecoder::new(compressed.as_slice()).read_to_end(&mut data).unwrap();
    let mut at = 0;
    let mut rect_index = 0;
    let mut active = None;
    while at < data.len() {
        let header = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        at += 4;
        let mut size = (header >> 20) as usize;
        if size == 0xfff {
            size = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
        }
        let record = &mut data[at..at + size];
        match header & 0x3ff {
            0x4c if record.starts_with(b"cer$") => {
                active = mixed_geometry(rect_index, angle, original, display, shift, sibling);
                rect_index += 1;
                if let Some(((w, h), matrix)) = active {
                    assert_eq!(u16::from_le_bytes(record[12..14].try_into().unwrap()), 1);
                    let pairs = u16::from_le_bytes(record[46..48].try_into().unwrap()) as usize;
                    assert_eq!(pairs, 2);
                    for (offset, value) in [(16, w), (20, h), (24, 2 * w), (28, 2 * h)] {
                        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    record[32..36].copy_from_slice(&0u32.to_le_bytes());
                    for j in 0..1 + 2 * pairs {
                        let m = if j == 2 * pairs { matrix } else { IDENTITY };
                        for (k, value) in m.iter().enumerate() {
                            let offset = 48 + j * 48 + k * 8;
                            record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
                        }
                    }
                }
            }
            0x4e => {
                if let Some(((w, h), _)) = active.take() {
                    for (j, (x, y)) in [(0i32, 0i32), (w, 0), (w, h), (0, h)].iter().enumerate() {
                        record[1 + j * 8..5 + j * 8].copy_from_slice(&x.to_le_bytes());
                        record[5 + j * 8..9 + j * 8].copy_from_slice(&y.to_le_bytes());
                    }
                }
            }
            _ => {}
        }
        at += size;
    }
    assert_eq!(rect_index, 12);
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&data).unwrap();
    file.create_stream("/BodyText/Section0")
        .unwrap()
        .write_all(&encoder.finish().unwrap())
        .unwrap();
    file.into_inner().into_inner()
}

/// 이것을 실패시키는 것: 그림 자식 30.4°의 손실이 그룹 fallback/retry를
/// 켜서 O=100×50, D=200×100인 비그림 형제의 XML을 HEAD와 다르게 쓰기.
#[test]
fn image_fill_fractional_turn_keeps_head_sibling_xml_both_routes() {
    for hwp5 in [false, true] {
        let input = if hwp5 { mixed_hwp5(30.4) } else { mixed_hwpx(30.4) };
        let (output, warnings) = if hwp5 {
            let (bytes, warnings) =
                hwp5_to_hwpx_bytes_with_diagnostics(&input, ConvertOptions::default()).unwrap();
            (bytes, format!("{warnings:?}"))
        } else {
            let decoded = HwpxDecoder::decode(&input).unwrap();
            let decode_warnings = format!("{:?}", decoded.warnings);
            assert!(
                decode_warnings.contains("shape_emission")
                    && decode_warnings.contains("GroupChild(5)"),
                "{decode_warnings}"
            );
            let encoded = HwpxEncoder::encode_with_diagnostics(
                &decoded.document.validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
                Default::default(),
            )
            .unwrap();
            (encoded.bytes, format!("{decode_warnings} {:?}", encoded.warnings))
        };
        assert!(warnings.contains("shape_emission"), "{warnings}");
        assert!(warnings.contains("GroupChild(5)"), "{warnings}");
        assert!(!warnings.contains("shape_drawn_extent"), "{warnings}");
        assert!(!warnings.contains("object_original_size"), "{warnings}");
        let sibling = brush_geometry(&rect_xml(&output)[0]);
        assert_eq!(sibling.org, (100., 50.));
        assert_eq!(sibling.points[2], (100., 50.));
        assert_eq!(sibling.pairs[0].0, [2., 0., 0., 0., 2., 0.]);
        // FNV-1a over every non-image rect's entire XML, captured by running
        // this same input through 9d0e1d91's ordinary decoder and encoder.
        assert_eq!(
            plain_rect_hashes(&output),
            [
                0x70a9d287a13fa569,
                0xddd0882e09be0092,
                0xb083a74f64df8085,
                0xa009e966aae82a98,
                0xfe1e1925a6799571,
                0x2f23eae8a73e4db9,
                0x88e6b382e26c2238,
                0xb3c61b7d9039649,
                0x179533547b1288b,
                0x42873f552ab0b678
            ],
            "hwp5={hwp5}"
        );
    }
}

/// 이것을 실패시키는 것: 새 그림 O의 조상 보정 손실이 형제까지 접기,
/// 또는 실제 방출 체인의 추가 손실을 그림 자식 경고 없이 기록하기.
#[test]
fn image_fill_new_ancestor_error_is_local_to_the_child() {
    use hwpforge_core::control::{
        geometry::{
            drop_original_size, group_child_frame_plan, preserve_group_original_boxes,
            AngleRounding, CoefficientRounding, EmissionPolicy,
        },
        ShapeStyle,
    };
    use hwpforge_foundation::{HwpUnit, Size};
    let u = |v| HwpUnit::new(v).unwrap();
    let mut expected = None;
    for legacy in [true, false] {
        let mut d = HwpxDecoder::decode(&fixture("hwpx")).unwrap();
        let mut preservation_warnings = Vec::new();
        d.document.for_each_paragraph_mut(|p| {
            for run in &mut p.runs {
                let RunContent::Control(c) = &mut run.content else { continue };
                let Control::Group { children, width, height, frame, .. } = c.as_mut() else {
                    continue;
                };
                let fill = children
                    .iter()
                    .find_map(|child| match child {
                        GroupChild::Shape(Control::Rect { style: Some(style), .. }) => {
                            style.fill.as_ref().filter(|f| matches!(f, Fill::Image { .. })).cloned()
                        }
                        _ => None,
                    })
                    .unwrap();
                let mut brush = Control::rect(u(100_000_000), u(100)).unwrap();
                brush.resize(u(100), u(100));
                if let Control::Rect { style, .. } = &mut brush {
                    *style = Some(ShapeStyle {
                        rotation: Some(30.),
                        fill: Some(fill),
                        ..Default::default()
                    });
                }
                let candidate = group_child_frame_plan(&brush, &EmissionPolicy::exact()).unwrap();
                drop_original_size(&mut brush);
                let mut plain = Control::rect(u(100), u(50)).unwrap();
                plain.resize(u(200), u(100));
                *children = vec![GroupChild::Shape(plain), GroupChild::Shape(brush)];
                *width = u(1000);
                *height = u(1000);
                frame.original_size = Some(Size::new(u(1), u(1)));
                if !legacy {
                    preservation_warnings.extend(preserve_group_original_boxes(
                        c,
                        &[(1, candidate)],
                        &EmissionPolicy::new(
                            AngleRounding::WholeDegrees,
                            CoefficientRounding::Decimals(6),
                            true,
                        ),
                    ));
                }
            }
        });
        let encoded = HwpxEncoder::encode_with_diagnostics(
            &d.document.validate().unwrap(),
            &d.style_store,
            &d.image_store,
            Default::default(),
        )
        .unwrap();
        let plain: Vec<_> =
            rect_xml(&encoded.bytes).into_iter().filter(|s| !s.contains("<hc:imgBrush")).collect();
        if legacy {
            assert!(encoded.warnings.is_empty(), "{:?}", encoded.warnings);
            expected = Some(plain);
        } else {
            assert_eq!(&plain, expected.as_ref().unwrap());
            let warnings = format!("{preservation_warnings:?}");
            assert_eq!(preservation_warnings.len(), 2, "{warnings}");
            assert!(
                warnings.contains("full stored transform")
                    && preservation_warnings.iter().all(|(path, _)| path == &[1]),
                "{warnings}"
            );
            for xml in &plain {
                let g = brush_geometry(xml);
                assert_eq!(g.org, (100., 50.));
                assert_eq!(g.points[2], (100., 50.));
            }
        }
    }
}

/// 이것을 실패시키는 것: 359.99999°가 0°로 정규화되면 그림 O가 그룹 fallback을 켜기.
#[test]
fn image_fill_normalized_turn_keeps_head_sibling_xml_both_routes() {
    for angle in [0., 359.99999] {
        for hwp5 in [false, true] {
            let input = if hwp5 { mixed_hwp5(angle) } else { mixed_hwpx(angle) };
            let (output, warnings) = if hwp5 {
                let (bytes, warnings) =
                    hwp5_to_hwpx_bytes_with_diagnostics(&input, ConvertOptions::default()).unwrap();
                (bytes, format!("{warnings:?}"))
            } else {
                let decoded = HwpxDecoder::decode(&input).unwrap();
                let encoded = HwpxEncoder::encode_with_diagnostics(
                    &decoded.document.validate().unwrap(),
                    &decoded.style_store,
                    &decoded.image_store,
                    Default::default(),
                )
                .unwrap();
                (encoded.bytes, format!("{:?} {:?}", decoded.warnings, encoded.warnings))
            };
            if angle != 0. {
                assert!(
                    warnings.contains("49.000000")
                        && (warnings.contains("GroupChild(5)") || warnings.contains(".child[5]")),
                    "{warnings}"
                );
            }
            let sibling = brush_geometry(&rect_xml(&output)[0]);
            assert_eq!(sibling.org, (100., 50.));
            assert_eq!(sibling.pairs.len(), 1);

            assert!(!warnings.contains("shape_drawn_extent"), "{warnings}");
            assert_eq!(
                plain_rect_hashes(&output),
                [
                    0x70a9d287a13fa569,
                    0xddd0882e09be0092,
                    0xb083a74f64df8085,
                    0xa009e966aae82a98,
                    0xfe1e1925a6799571,
                    0x2f23eae8a73e4db9,
                    0x88e6b382e26c2238,
                    0xb3c61b7d9039649,
                    0x179533547b1288b,
                    0x42873f552ab0b678
                ],
                "hwp5={hwp5}"
            );
            assert_image_zip_fixed_point(&output);
            assert_image_stamp_edit(&output);
            let image = brush_geometry(&rect_xml(&output)[5]);
            assert_eq!(image.org, (49., 100.));
            assert_eq!(image.points[2], (49., 100.));
        }
    }
}

/// 이것을 실패시키는 것: 부모 (3,3) 아래 로컬 0.9와 조상 1.9를 통과시켜 2.8 무경고 출력.
#[test]
fn image_fill_combined_residual_folds_only_child_both_routes() {
    use hwpforge_core::control::{
        geometry::{
            drop_original_size, group_child_frame_plan, preserve_group_original_boxes,
            AngleRounding, CoefficientRounding, EmissionPolicy,
        },
        ShapeStyle,
    };
    use hwpforge_foundation::{HwpUnit, Size};
    let u = |v| HwpUnit::new(v).unwrap();
    for route in ["hwp5", "hwpx", "json"] {
        let hwp5 = route == "hwp5";
        let mut expected = None;
        for legacy in [true, false] {
            let mut d = if hwp5 {
                // Retain the route's decoded fills/styles while constructing the r2 Core input.
                let (bytes, _) =
                    hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default())
                        .unwrap();
                HwpxDecoder::decode(&bytes).unwrap()
            } else {
                HwpxDecoder::decode(&fixture("hwpx")).unwrap()
            };
            let mut preservation_warnings = Vec::new();
            d.document.for_each_paragraph_mut(|p| {
                for run in &mut p.runs {
                    let RunContent::Control(c) = &mut run.content else { continue };
                    let Control::Group { children, width, height, frame, .. } = c.as_mut() else {
                        continue;
                    };
                    let fill = children
                        .iter()
                        .find_map(|child| match child {
                            GroupChild::Shape(Control::Rect { style: Some(style), .. }) => style
                                .fill
                                .as_ref()
                                .filter(|f| matches!(f, Fill::Image { .. }))
                                .cloned(),
                            _ => None,
                        })
                        .unwrap();
                    let mut brush = Control::rect(u(1_900_000), u(32)).unwrap();
                    brush.resize(u(94), u(96));
                    if let Control::Rect { style, .. } = &mut brush {
                        *style = Some(ShapeStyle {
                            rotation: Some(90.),
                            fill: Some(fill),
                            ..Default::default()
                        });
                    }
                    let candidate =
                        group_child_frame_plan(&brush, &EmissionPolicy::exact()).unwrap();
                    drop_original_size(&mut brush);
                    let mut sibling = Control::rect(u(100), u(50)).unwrap();
                    sibling.resize(u(200), u(100));
                    *children = vec![GroupChild::Shape(sibling), GroupChild::Shape(brush)];
                    *width = u(3000);
                    *height = u(3000);
                    frame.original_size = Some(Size::new(u(1000), u(1000)));
                    if !legacy {
                        preservation_warnings.extend(preserve_group_original_boxes(
                            c,
                            &[(1, candidate)],
                            &EmissionPolicy::new(
                                AngleRounding::WholeDegrees,
                                CoefficientRounding::Decimals(6),
                                true,
                            ),
                        ));
                    }
                }
            });
            if route == "json" {
                d.document =
                    serde_json::from_slice(&serde_json::to_vec(&d.document).unwrap()).unwrap();
            }
            let encoded = HwpxEncoder::encode_with_diagnostics(
                &d.document.validate().unwrap(),
                &d.style_store,
                &d.image_store,
                Default::default(),
            )
            .unwrap();
            // Entire non-image XML, captured from HEAD with the folded r2 input.
            assert_eq!(plain_rect_hashes(&encoded.bytes), [0xaaa1c9a2178eb15a, 0x582a66a9d0630206]);
            assert_image_zip_fixed_point(&encoded.bytes);
            assert_image_stamp_edit(&encoded.bytes);
            let rects = rect_xml(&encoded.bytes);
            let siblings: Vec<_> =
                rects.iter().filter(|s| !s.contains("<hc:imgBrush")).cloned().collect();
            if legacy {
                assert!(encoded.warnings.is_empty(), "{:?}", encoded.warnings);
                expected = Some(siblings);
            } else {
                assert_eq!(&siblings, expected.as_ref().unwrap());
                let warnings = format!("{preservation_warnings:?}");
                assert_eq!(preservation_warnings.len(), 2, "{warnings}");
                assert!(
                    warnings.contains("2.800001")
                        && preservation_warnings.iter().all(|(path, _)| path == &[1]),
                    "{warnings}"
                );
                for xml in rects.iter().filter(|s| s.contains("<hc:imgBrush")) {
                    let geometry = brush_geometry(xml);
                    assert_eq!(geometry.org, (94., 96.));
                    assert_eq!(geometry.points[2], (94., 96.));
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 그림 O 보존 뒤 재디코드가 frame을 바꿔 stamp 왕복을 거부하기.
#[test]
fn image_fill_core_and_fixture_output_are_idempotent() {
    let input =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("hwp"), ConvertOptions::default()).unwrap().0;
    let first = HwpxDecoder::decode(&input).unwrap();
    let expected = serde_json::to_value(&first.document).unwrap();
    let options = hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true);
    let output = HwpxEncoder::encode_with_options(
        &first.document.validate().unwrap(),
        &first.style_store,
        &first.image_store,
        options,
    )
    .unwrap();
    let second = HwpxDecoder::decode(&output).unwrap();
    assert_eq!(serde_json::to_value(&second.document).unwrap(), expected);
    let again = HwpxEncoder::encode_with_options(
        &second.document.validate().unwrap(),
        &second.style_store,
        &second.image_store,
        options,
    )
    .unwrap();
    assert_eq!(output, again);
    assert_eq!(input, output);
    assert_image_stamp_edit(&input);
}

/// 이것을 실패시키는 것: 정상 그림 또는 359.99999° 이동 유무가 형제의 O/배율을 바꾸기.
#[test]
fn image_fill_decision_oracles_both_routes() {
    for (angle, original, display, shift) in [
        (0., (5000, 500), (10000, 1000), (0, 0)),
        (30., (5000, 500), (10000, 1000), (0, 0)),
        (30.4, (5000, 500), (10000, 1000), (0, 0)),
        (359.99999, (100_000_000, 100), (49, 100), (0, 0)),
        (359.99999, (100_000_000, 100), (49, 100), (1000, 1000)),
        (359.99999, (4_000_000, 100), (982, 100), (0, 0)),
        (359.99999, (4_000_000, 100), (982, 100), (1000, 1000)),
        (0., (100_000_000, 100), (49, 100), (0, 0)),
    ] {
        for hwp5 in [false, true] {
            let input = if hwp5 {
                mixed_hwp5_case(angle, original, display, shift)
            } else {
                mixed_hwpx_case(angle, original, display, shift)
            };
            let (output, warnings) = if hwp5 {
                let (b, w) =
                    hwp5_to_hwpx_bytes_with_diagnostics(&input, ConvertOptions::default()).unwrap();
                (b, format!("{w:?}"))
            } else {
                let d = HwpxDecoder::decode(&input).unwrap();
                let e = HwpxEncoder::encode_with_diagnostics(
                    &d.document.validate().unwrap(),
                    &d.style_store,
                    &d.image_store,
                    Default::default(),
                )
                .unwrap();
                (e.bytes, format!("{:?} {:?}", d.warnings, e.warnings))
            };
            // Frozen from archived HEAD: normal cases use original input, all
            // counterexamples use the input whose image O/points were folded to D.
            assert_eq!(
                plain_rect_hashes(&output),
                [
                    0x70a9d287a13fa569,
                    0xddd0882e09be0092,
                    0xb083a74f64df8085,
                    0xa009e966aae82a98,
                    0xfe1e1925a6799571,
                    0x2f23eae8a73e4db9,
                    0x88e6b382e26c2238,
                    0xb3c61b7d9039649,
                    0x179533547b1288b,
                    0x42873f552ab0b678,
                ],
                "hwp5={hwp5} angle={angle} O={original:?} shift={shift:?}"
            );
            assert_image_zip_fixed_point(&output);
            assert_image_stamp_edit(&output);
            let image = brush_geometry(&rect_xml(&output)[5]);
            let accepted = angle == 30.
                || (angle == 0. && original.0 == 5000)
                || (original.0 == 4_000_000 && shift == (1000, 1000));
            assert_eq!(image.org == (f64::from(original.0), f64::from(original.1)), accepted);
            if !accepted {
                assert!(warnings.contains("full stored transform"), "{warnings}");
                assert_eq!(image.org, (f64::from(display.0), f64::from(display.1)));
            }
            assert!(!warnings.contains("object_original_size"), "{warnings}");
        }
    }
}

/// 이것을 실패시키는 것: 그림 채택/거부 출력의 첫 ZIP이 재디코드 후 다른 쌍/메타데이터를 쓰기.
fn assert_image_zip_fixed_point(input: &[u8]) {
    let first = HwpxDecoder::decode(input).unwrap();
    let expected = serde_json::to_value(&first.document).unwrap();
    let output = HwpxEncoder::encode_with_options(
        &first.document.validate().unwrap(),
        &first.style_store,
        &first.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&HwpxDecoder::decode(&output).unwrap().document).unwrap(),
        expected
    );
    // Show XML instead of dumping thousands of ZIP bytes on failure.
    let before = PackageReader::new(input).unwrap().read_section_xml(0).unwrap();
    let after = PackageReader::new(&output).unwrap().read_section_xml(0).unwrap();
    let differences: Vec<_> =
        before.split('>').zip(after.split('>')).filter(|(a, b)| a != b).collect();
    assert!(differences.is_empty(), "{differences:?}");
    assert_eq!(input, output);
    for xml in rect_xml(input).iter().filter(|s| s.contains("<hc:imgBrush")) {
        let g = brush_geometry(xml);
        assert_eq!(g.pairs.len(), g.level + 1);
    }
}

fn json_image_case(
    angle: f32,
    original: (i32, i32),
    display: (i32, i32),
    shift: (i32, i32),
    sibling: ((i32, i32), (i32, i32)),
) -> hwpforge_smithy_hwpx::HwpxDocument {
    use hwpforge_foundation::{HwpUnit, Size};
    let u = |v| HwpUnit::new(v).unwrap();
    let mut d = HwpxDecoder::decode(&fixture("hwpx")).unwrap();
    let mut changed = false;
    d.document.for_each_paragraph_mut(|p| {
        for run in &mut p.runs {
            let RunContent::Control(c) = &mut run.content else { continue };
            let Control::Group { children, .. } = c.as_mut() else { continue };
            if changed {
                continue;
            }
            changed = true;
            for (index, o, size, angle, offset) in
                [(0, sibling.0, sibling.1, 0., (0, 0)), (5, original, display, angle, shift)]
            {
                let GroupChild::Shape(Control::Rect {
                    frame, width, height, style, placement, ..
                }) = &mut children[index]
                else {
                    panic!()
                };
                frame.original_size = (o != size).then_some(Size::new(u(o.0), u(o.1)));
                *width = u(size.0);
                *height = u(size.1);
                style.as_mut().unwrap().rotation =
                    (angle.rem_euclid(360.) != 0.).then_some(angle.rem_euclid(360.));
                *placement = (offset != (0, 0)).then(|| {
                    hwpforge_core::placement::ObjectPlacement::new(
                        hwpforge_core::ObjectTextWrap::InFrontOfText,
                        hwpforge_core::ObjectRelativeTo::Paper,
                        hwpforge_core::ObjectRelativeTo::Paper,
                    )
                    .with_allow_overlap(true)
                    .with_horz_offset(u(offset.0))
                    .with_vert_offset(u(offset.1))
                });
            }
        }
    });
    assert!(changed);
    d.document = serde_json::from_slice(&serde_json::to_vec(&d.document).unwrap()).unwrap();
    d
}

/// 이것을 실패시키는 것: 비그림 형제 fallback 때 HWP5/HWPX/JSON 그림의 각도 손실을 생략.
#[test]
fn image_fill_sibling_fallback_checks_final_emission_all_routes() {
    let sibling = ((100_000_000, 100), (49, 100));
    for route in ["hwp5", "hwpx", "json"] {
        let (output, warnings) = match route {
            "hwp5" => {
                let input =
                    mixed_hwp5_with_sibling(30.4, (10000, 1000), (10000, 1000), (0, 0), sibling);
                let decoded = hwpforge_smithy_hwp5::Hwp5Decoder::decode(&input).unwrap();
                assert!(format!("{:?}", decoded.warnings).contains("full stored transform"));
                let (b, w) =
                    hwp5_to_hwpx_bytes_with_diagnostics(&input, ConvertOptions::default()).unwrap();
                (b, format!("{w:?}"))
            }
            _ => {
                let d = if route == "hwpx" {
                    HwpxDecoder::decode(&mixed_hwpx_with_sibling(
                        30.4,
                        (10000, 1000),
                        (10000, 1000),
                        (0, 0),
                        sibling,
                    ))
                    .unwrap()
                } else {
                    json_image_case(30.4, (10000, 1000), (10000, 1000), (0, 0), sibling)
                };
                if route == "hwpx" {
                    assert!(format!("{:?}", d.warnings).contains("full stored transform"));
                }
                let e = HwpxEncoder::encode_with_diagnostics(
                    &d.document.validate().unwrap(),
                    &d.style_store,
                    &d.image_store,
                    Default::default(),
                )
                .unwrap();
                (e.bytes, format!("{:?} {:?}", d.warnings, e.warnings))
            }
        };
        assert!(
            warnings.contains("full stored transform") && warnings.contains("35.080"),
            "{route}: {warnings}"
        );
        assert_image_zip_fixed_point(&output);
        assert_image_stamp_edit(&output);
        assert_eq!(
            plain_rect_hashes(&output),
            [
                15095287089478694006,
                15983424808851210386,
                12719193732061036677,
                11532004948022143640,
                18311100784432485745,
                3396816828926676409,
                9864769408527508024,
                809629475267647049,
                106207554521081995,
                4793869963380176504
            ]
        );
        let g = brush_geometry(&rect_xml(&output)[0]);
        assert_eq!(g.org, (49., 100.), "fallback sibling {route}");
    }
}

/// 이것을 실패시키는 것: JSON Core writer가 채택/거부를 검사해도 최초 출력의 정규형이 달라지기.
#[test]
fn image_fill_json_attacks_are_idempotent() {
    for (angle, original, display, shift, accepted) in [
        (0., (5000, 500), (10000, 1000), (0, 0), true),
        (30., (5000, 500), (10000, 1000), (0, 0), true),
        (30.4, (5000, 500), (10000, 1000), (0, 0), false),
        (359.99999, (100_000_000, 100), (49, 100), (0, 0), false),
        (359.99999, (100_000_000, 100), (49, 100), (1000, 1000), false),
        (0., (100_000_000, 100), (49, 100), (0, 0), false),
        (359.99999, (4_000_000, 100), (982, 100), (0, 0), false),
        (359.99999, (4_000_000, 100), (982, 100), (1000, 1000), true),
    ] {
        let d = json_image_case(angle, original, display, shift, ((100, 50), (200, 100)));
        let e = HwpxEncoder::encode_with_diagnostics(
            &d.document.validate().unwrap(),
            &d.style_store,
            &d.image_store,
            Default::default(),
        )
        .unwrap();
        assert_eq!(format!("{:?}", e.warnings).contains("full stored transform"), !accepted);
        assert_eq!(
            plain_rect_hashes(&e.bytes),
            [
                0x70a9d287a13fa569,
                0xddd0882e09be0092,
                0xb083a74f64df8085,
                0xa009e966aae82a98,
                0xfe1e1925a6799571,
                0x2f23eae8a73e4db9,
                0x88e6b382e26c2238,
                0xb3c61b7d9039649,
                0x179533547b1288b,
                0x42873f552ab0b678
            ]
        );
        assert_image_zip_fixed_point(&e.bytes);
        assert_image_stamp_edit(&e.bytes);
    }
}

/// 이것을 실패시키는 것: Core 비교만 통과하고 실제 stamp 편집이 그림 행렬/XML을 바꾸기.
fn assert_image_stamp_edit(input: &[u8]) {
    use hwpforge_core::{Paragraph, Run};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::stamp::{HwpxStamper, StampAction, StampSpec};
    let mut d = HwpxDecoder::decode(input).unwrap();
    // The raw attacks deliberately replace leaves without updating the native
    // parent size. Stamp correctly rejects that unrelated group_display_size
    // loss. Repair only that test shell; all leaf XML must remain identical.
    if format!("{:?}", d.warnings).contains("group_display_size") {
        use hwpforge_core::control::geometry::{emitted_corners, group_bounds, EmissionPolicy};
        d.document.for_each_paragraph_mut(|p| {
            for run in &mut p.runs {
                let RunContent::Control(c) = &mut run.content else { continue };
                let Control::Group { children, width, height, frame, .. } = c.as_mut() else {
                    continue;
                };
                let bounds = group_bounds(children.iter().flat_map(|c| {
                    match c {
                        GroupChild::Shape(c) => emitted_corners(c, &EmissionPolicy::exact())
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    }
                }))
                .unwrap();
                if frame.original_size.is_none() {
                    *width = hwpforge_foundation::HwpUnit::new(
                        (bounds.max_x() - bounds.min_x()).round() as i32,
                    )
                    .unwrap();
                    *height = hwpforge_foundation::HwpUnit::new(
                        (bounds.max_y() - bounds.min_y()).round() as i32,
                    )
                    .unwrap();
                }
            }
        });
    }
    let mut marker = Paragraph::new(ParaShapeIndex::new(0));
    marker.add_run(Run::text("검증 (   )", CharShapeIndex::new(0)));
    d.document.sections_mut()[0].paragraphs.push(marker);
    let base = HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
        .unwrap();
    assert_eq!(rect_xml(input), rect_xml(&base));
    let plan = HwpxStamper::plan_bytes_v2(&base).unwrap();
    let c = plan.text.into_iter().find(|c| c.marker == "(   )").expect("added stamp marker");
    let result = HwpxStamper::stamp(
        &base,
        &[StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "picfill_edit".into(), hint: None },
        }],
    )
    .expect("actual stamp edit");
    assert_ne!(base, result.bytes);
    assert_eq!(result.manifest.fields.len(), 1);
    assert_eq!(rect_xml(input), rect_xml(&result.bytes));
    assert_image_zip_fixed_point(&result.bytes);
}
