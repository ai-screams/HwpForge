//! Hancom fixture 31: definition, closure, points and hierarchy survive both codecs.
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};
use hwpforge_core::{
    control::{ArcDefinition, GroupChild},
    Control, RunContent,
};
use hwpforge_foundation::ArcType;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{events::Event, Reader};
use std::collections::BTreeMap;

type Attrs = BTreeMap<String, String>;
#[derive(Debug, PartialEq)]
struct ArcWire {
    group: usize,
    tag: String,
    kind: String,
    points: Vec<(String, Attrs)>,
    line: Attrs,
}
fn fixture(ext: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/shapes/arcs.{ext}", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
}
fn arcs(bytes: &[u8]) -> (usize, Vec<ArcWire>) {
    let xml = PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap();
    let mut r = Reader::from_str(&xml);
    let (mut depth, mut containers) = (0, 0);
    let mut active = None;
    let mut out = Vec::<ArcWire>::new();
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let tag = e.local_name().as_ref().to_string();
                let a: Attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (a.key.as_ref().to_string(), a.value.to_string())
                    })
                    .collect();
                if tag == "container" {
                    depth += 1;
                    containers += 1;
                }
                if tag == "arc" || tag == "ellipse" && a.get("hasArcPr").is_some_and(|v| v == "1") {
                    active = Some(out.len());
                    out.push(ArcWire {
                        group: depth,
                        tag: tag.clone(),
                        kind: a.get("type").or(a.get("arcType")).unwrap().clone(),
                        points: Vec::new(),
                        line: Attrs::new(),
                    });
                } else if let Some(i) = active {
                    if matches!(
                        tag.as_str(),
                        "center" | "ax1" | "ax2" | "start1" | "end1" | "start2" | "end2"
                    ) {
                        out[i].points.push((tag, a));
                    } else if tag == "lineShape" {
                        out[i].line = a
                            .into_iter()
                            .filter(|(k, _)| {
                                matches!(
                                    k.as_str(),
                                    "color"
                                        | "width"
                                        | "style"
                                        | "headStyle"
                                        | "tailStyle"
                                        | "headSz"
                                        | "tailSz"
                                        | "headfill"
                                        | "tailfill"
                                )
                            })
                            .collect();
                    }
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "arc" | "ellipse" => active = None,
                "container" => depth -= 1,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    (containers, out)
}
fn reencode(bytes: &[u8]) -> Vec<u8> {
    let d = HwpxDecoder::decode(bytes).unwrap();
    HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store).unwrap()
}
fn core_arcs(
    c: &Control,
    depth: usize,
    out: &mut Vec<(usize, ArcDefinition, ArcType, Vec<hwpforge_core::control::ShapePoint>)>,
) {
    match c {
        Control::Arc { definition, arc_type, .. } => out.push((
            depth,
            *definition,
            *arc_type,
            hwpforge_core::control::geometry::defining_points(c),
        )),
        Control::Group { children, .. } => {
            for child in children {
                if let GroupChild::Shape(c) = child {
                    core_arcs(c, depth + 1, out);
                }
            }
        }
        _ => {}
    }
}

fn check_core(document: &hwpforge_core::Document<hwpforge_core::Draft>) {
    let mut values = Vec::new();
    for s in document.sections() {
        for p in &s.paragraphs {
            for r in &p.runs {
                if let RunContent::Control(c) = &r.content {
                    core_arcs(c, 0, &mut values);
                }
            }
        }
    }
    assert_eq!(values.len(), 6);
    let oracle = arcs(&fixture("hwpx")).1;
    for (i, v) in values.iter().enumerate() {
        assert_eq!(
            *v,
            (
                usize::from(i >= 3),
                ArcDefinition::AxisEndpoints,
                [ArcType::Normal, ArcType::Pie, ArcType::Chord][i % 3],
                oracle[i]
                    .points
                    .iter()
                    .map(|(_, p)| hwpforge_core::control::ShapePoint::new(
                        p["x"].parse().unwrap(),
                        p["y"].parse().unwrap()
                    ))
                    .collect()
            )
        );
    }
}
/// 이것을 실패시키는 것: 묶음 arc 누락, 종류/정의점 복제 매핑, 태그를 회전/부모로 결정하기.
#[test]
fn six_hancom_arcs_keep_definition_points_closure_and_hierarchy() {
    let truth = fixture("hwpx");
    let expected = arcs(&truth);
    assert_eq!(expected.0, 1);
    assert_eq!(expected.1.len(), 6);
    for (i, a) in expected.1.iter().enumerate() {
        assert_eq!(a.tag, "arc");
        assert_eq!(a.group, usize::from(i >= 3));
        assert_eq!(a.kind, ["NORMAL", "PIE", "CHORD"][i % 3]);
        assert_eq!(a.points.len(), 3);
    }
    let (converted, warnings) =
        hwp5_to_hwpx_bytes_with_options(&fixture("hwp"), ConvertOptions::default()).unwrap();
    assert!(!warnings.iter().any(|w| format!("{w:?}").contains("arc omitted")), "{warnings:?}");
    check_core(&hwpforge_smithy_hwp5::Hwp5Decoder::decode(&fixture("hwp")).unwrap().document);
    for input in [&converted, &truth] {
        assert_eq!(arcs(&reencode(input)), expected);
        let d = HwpxDecoder::decode(input).unwrap();
        check_core(&d.document);
    }
    assert_eq!(arcs(&converted), expected);
}

/// 이것을 실패시키는 것: 실제 XML의 미지 호 종류·필수 점 누락을 정상 호로 대체하기.
#[test]
fn malformed_arc_xml_is_diagnosed_in_both_hosts() {
    use std::io::{Cursor, Read, Write};
    for native in [false, true] {
        for missing_point in [false, true] {
            let input = fixture("hwpx");
            let mut zip = zip::ZipArchive::new(Cursor::new(input)).unwrap();
            let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).unwrap();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                if entry.name().unwrap() == "Contents/section0.xml" {
                    let mut xml = String::from_utf8(data).unwrap();
                    if missing_point {
                        while let Some(start) = xml.find("<hc:center ") {
                            let end = start + xml[start..].find("/>").unwrap() + 2;
                            xml.replace_range(start..end, "");
                        }
                    } else {
                        for kind in ["NORMAL", "PIE", "CHORD"] {
                            xml = xml.replace(
                                &format!("<hp:arc type=\"{kind}\""),
                                "<hp:arc type=\"FUTURE_ARC\"",
                            );
                            // Hancom's root has other attributes before type.
                            xml = xml.replace(&format!(" type=\"{kind}\""), " type=\"FUTURE_ARC\"");
                        }
                    }
                    if !native {
                        xml = xml.replace("<hp:arc ", "<hp:ellipse hasArcPr=\"1\" ");
                        xml = xml.replace("</hp:arc>", "</hp:ellipse>");
                        xml = xml.replace(" type=\"", " arcType=\"");
                    }
                    data = xml.into_bytes();
                }
                out.start_file(entry.name().unwrap(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(&data).unwrap();
            }
            let bytes = out.finish().unwrap().into_inner();
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let diagnostic =
                if missing_point { "missing required points: center" } else { "FUTURE_ARC" };
            assert!(
                decoded.warnings.iter().filter(|w| format!("{w:?}").contains(diagnostic)).count()
                    >= 6,
                "{:?}",
                decoded.warnings
            );
            assert!(arcs(&reencode(&bytes)).1.is_empty());
        }
    }
}

/// 이것을 실패시키는 것: 회전/대칭/묶음 아래에서 호 정의 방식 또는 닫힘 종류 변경.
#[test]
fn axis_arc_identity_is_independent_of_transform_and_parent() {
    use hwpforge_core::{control::ShapeStyle, Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, Flip, HwpUnit, ParaShapeIndex};
    let source = HwpxDecoder::decode(&fixture("hwpx")).unwrap();
    let original = source.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Control(c) if c.is_arc() => Some(c.as_ref().clone()),
            _ => None,
        })
        .unwrap();
    for kind in [ArcType::Normal, ArcType::Pie, ArcType::Chord] {
        for (angle, flip, grouped) in [
            (None, None, false),
            (Some(30.0), None, false),
            (None, Some(Flip::Horizontal), false),
            (Some(30.0), Some(Flip::Horizontal), true),
        ] {
            let mut c = original.clone();
            if let Control::Arc { arc_type, style, .. } = &mut c {
                *arc_type = kind;
                let s = style.get_or_insert_with(ShapeStyle::default);
                s.rotation = angle;
                s.flip = flip;
            }
            let c = if grouped {
                Control::Group {
                    children: vec![GroupChild::Shape(c)],
                    width: HwpUnit::new(20000).unwrap(),
                    height: HwpUnit::new(20000).unwrap(),
                    frame: Default::default(),
                    placement: None,
                    inst_id: None,
                }
            } else {
                c
            };
            let mut doc = Document::new();
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::control(c, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            ));
            let bytes = HwpxEncoder::encode(
                &doc.validate().unwrap(),
                &source.style_store,
                &source.image_store,
            )
            .unwrap();
            for output in [&bytes, &reencode(&bytes)] {
                let (_, arcs) = arcs(output);
                assert_eq!(arcs.len(), 1);
                assert_eq!(arcs[0].tag, "arc");
                assert_eq!(arcs[0].kind, kind.to_string());
                assert_eq!(arcs[0].points.len(), 3);
                assert_eq!(arcs[0].group, usize::from(grouped));
            }
        }
    }
}
