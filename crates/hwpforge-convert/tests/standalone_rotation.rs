//! Contract §9: oracle values come from Hancom wire XML and fixed integers.
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use std::collections::BTreeMap;
type Matrix = [f64; 6];
fn mul(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}
fn point(m: Matrix, p: (f64, f64)) -> (f64, f64) {
    (m[0] * p.0 + m[1] * p.1 + m[2], m[3] * p.0 + m[4] * p.1 + m[5])
}
fn attrs(e: &BytesStart<'_>) -> BTreeMap<String, String> {
    e.attributes()
        .map(|a| {
            let a = a.unwrap();
            (a.key.as_ref().to_string(), a.value.to_string())
        })
        .collect()
}
#[derive(Debug, Default)]
struct Wire {
    kind: String,
    fields: BTreeMap<String, BTreeMap<String, String>>,
    matrices: Vec<Matrix>,
    points: Vec<(String, BTreeMap<String, String>)>,
}
impl Wire {
    fn v(&self, tag: &str, attr: &str) -> i32 {
        self.fields[tag][attr].parse::<i64>().unwrap() as i32
    }
    fn size(&self, tag: &str) -> (i32, i32) {
        (self.v(tag, "width"), self.v(tag, "height"))
    }
    fn map(&self) -> Matrix {
        self.matrices[1..].chunks_exact(2).fold(self.matrices[0], |m, p| mul(m, mul(p[1], p[0])))
    }
}
fn wire(bytes: &[u8]) -> Vec<Wire> {
    let xml = PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap();
    let mut r = Reader::from_str(&xml);
    let mut depth = 0usize;
    let mut active: Vec<(usize, usize)> = vec![];
    let mut out = vec![];
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let empty = xml.as_bytes().get(r.buffer_position() as usize - 2) == Some(&b'/');
                let name = e.local_name().as_ref().to_string();
                if matches!(
                    name.as_str(),
                    "arc"
                        | "ellipse"
                        | "rect"
                        | "polygon"
                        | "curve"
                        | "line"
                        | "connectLine"
                        | "pic"
                ) {
                    let a = attrs(&e);
                    let kind = if name == "ellipse" && a.get("hasArcPr").is_some_and(|v| v == "1") {
                        "arc".into()
                    } else {
                        name.clone()
                    };
                    active.push((out.len(), depth));
                    out.push(Wire { kind, ..Default::default() });
                } else if let Some(&(i, d)) = active.last() {
                    if depth == d + 1 {
                        out[i].fields.insert(name.clone(), attrs(&e));
                    }
                    if depth <= d + 2
                        && matches!(
                            name.as_str(),
                            "pt" | "center"
                                | "ax1"
                                | "ax2"
                                | "start1"
                                | "end1"
                                | "start2"
                                | "end2"
                                | "startPt"
                                | "endPt"
                                | "seg"
                        )
                    {
                        let coordinates = attrs(&e)
                            .into_iter()
                            .filter(|(k, _)| {
                                matches!(k.as_str(), "x" | "y" | "x1" | "x2" | "y1" | "y2")
                            })
                            .collect();
                        out[i].points.push((name.clone(), coordinates));
                    }
                    if matches!(name.as_str(), "transMatrix" | "scaMatrix" | "rotMatrix")
                        && depth == d + 2
                    {
                        let a = attrs(&e);
                        out[i].matrices.push(std::array::from_fn(|i| {
                            a[&format!("e{}", i + 1)].parse().unwrap()
                        }));
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth -= 1;
                if active.last().is_some_and(|&(_, d)| d == depth) {
                    active.pop();
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}
fn fixture(dir: &str, name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/{dir}/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
}
fn reencode(bytes: &[u8]) -> Vec<u8> {
    let d = HwpxDecoder::decode(bytes).unwrap();
    HwpxEncoder::encode_with_diagnostics(
        &d.document.validate().unwrap(),
        &d.style_store,
        &d.image_store,
        EncodeOptions::default(),
    )
    .unwrap()
    .bytes
}

fn check(reference: &[u8], bytes: &[u8], g7: bool) {
    let (a, b) = (wire(reference), wire(bytes));
    assert_eq!(a.len(), b.len(), "object count");
    for (a, b) in a.iter().zip(&b) {
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.points, b.points, "ordered original defining points");
        let o = a.size("orgSz");
        assert_eq!(o, b.size("orgSz"));
        assert_eq!(a.size("sz"), b.size("sz"));
        // 이것을 실패시키는 것: 독립 offset을 0으로 되돌리기.
        assert_eq!(
            (b.v("offset", "x"), b.v("offset", "y")),
            (b.matrices[0][2] as i32, b.matrices[0][5] as i32),
        );
        let expected = match o {
            (9643, 9262) => Some(((0, 0), (1670, 1790))),
            (6217, 9325) => Some(((0, 0), (1555, -1554))),
            (12831, 9293) => Some(((0, 0), (1464, 2585))),
            (10329, 8619) => Some(((0, 0), (-854, 855))),
            (9135, 8311) => Some(((8564, 32), (-566, 2139))),
            (8881, 32) => Some(((0, 0), (-4424, 4424))),
            (132, 12677) => Some(((11926, 265), (-733, 2965))),
            (10778, 89) => Some(((0, 0), (-5344, 5345))),
            (8570, 4594) => Some(((0, 0), (574, 1835))),
            (11440, 5124) => Some(((0, 0), (-3158, 3158))),
            (9630, 5212) => Some(((0, 0), (0, 0))),
            (5804, 5360) => Some(((0, 0), (1045, 1267))),
            (100, 100) if a.size("sz") == (10404, 2) => Some(((2, 10404), (5201, -5201))),
            (100, 100) => Some(((7677, 0), (-514, 1919))),
            _ => None,
        };
        if let Some((cur, t)) = expected {
            assert_eq!(b.size("curSz"), cur);
            assert_eq!((b.matrices[0][2], b.matrices[0][5]), (f64::from(t.0), f64::from(t.1)));
        }
        if g7 && a.size("sz") == (10404, 2) {
            continue;
        }
        let (ma, mb) = (a.map(), b.map());
        for i in [0, 1, 3, 4] {
            assert!((ma[i] - mb[i]).abs() < 0.0001, "linear {ma:?} {mb:?}");
        }
        let mut ps = vec![
            (0., 0.),
            (f64::from(o.0), 0.),
            (f64::from(o.0), f64::from(o.1)),
            (0., f64::from(o.1)),
        ];
        for (_, fields) in &a.points {
            for (x, y) in [("x", "y"), ("x1", "y1"), ("x2", "y2")] {
                if let (Some(x), Some(y)) = (fields.get(x), fields.get(y)) {
                    ps.push((x.parse().unwrap(), y.parse().unwrap()));
                }
            }
        }
        for p in ps {
            let (pa, pb) = (point(ma, p), point(mb, p));
            assert!(
                (pa.0 - pb.0).abs() <= 1.0001 && (pa.1 - pb.1).abs() <= 1.0001,
                "{} O={o:?}: {pa:?} {pb:?}",
                a.kind
            );
        }
    }
}

/// 이것을 실패시키는 것: 그룹 접기 대입, O를 D로 덮기, curSz 0 복원,
/// 홀수 중심 혼용, 외접 폭만 반올림, 음수 sy 부호 손실, 본문만 투영.
#[test]
fn hancom_standalone_pairs_and_cache_modes() {
    for name in ["rot-shapes", "rot-box-basis"] {
        let source = fixture("standalone-rotation", &format!("{name}.hwp"));
        let answer = fixture("standalone-rotation", &format!("{name}.hwpx"));
        for cache in [false, true] {
            let (out, warnings) = hwp5_to_hwpx_bytes_with_options(
                &source,
                ConvertOptions::default().with_carry_layout_cache(cache),
            )
            .unwrap();
            check(&answer, &out, name == "rot-box-basis");
            let g7 = warnings.iter().filter(|w| format!("{w:?}").contains("G7;")).count();
            assert_eq!(g7, if name == "rot-box-basis" { 2 } else { 0 });
            let again = reencode(&out);
            check(&out, &again, false);
        }
        check(&answer, &reencode(&answer), name == "rot-box-basis");
    }
}

/// 이것을 실패시키는 것: H/V/HV 문자 일치 강제, 대칭 이중 적용, 끝점 방향 손실.
#[test]
fn f9_composite_geometry() {
    let answer = fixture("shapes", "rotflip-single.hwpx");
    let (out, _) = hwp5_to_hwpx_bytes_with_options(
        &fixture("shapes", "rotflip-single.hwp"),
        ConvertOptions::default(),
    )
    .unwrap();
    check(&answer, &out, false);
    check(&answer, &reencode(&answer), false);
    for (o, (t, center)) in wire(&out).iter().zip([
        ((0, 0), (4252, 2834)),
        ((658, 2455), (6327, 5289)),
        ((848, 1747), (5100, 4581)),
        ((659, 2455), (6328, 5289)),
        ((658, 2456), (6327, 5290)),
    ]) {
        assert_eq!(o.size("curSz"), (0, 0));
        assert_eq!((o.matrices[0][2] as i32, o.matrices[0][5] as i32), t);
        assert_eq!((o.v("rotationInfo", "centerX"), o.v("rotationInfo", "centerY")), center);
    }
}

/// 이것을 실패시키는 것: 점 기반 도형이면 전단을 점으로 접어 무손실로 선언.
#[test]
fn f10_shear_preserves_declared_size_with_warning() {
    let source = fixture("shapes", "group-f10-rot-then-scale-ungrouped.hwpx");
    let d = HwpxDecoder::decode(&source).unwrap();
    assert_eq!(d.warnings.iter().filter(|w| format!("{w:?}").contains("shear=")).count(), 3);
    let out = reencode(&source);
    let (a, b) = (wire(&source), wire(&out));
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!(a.size("sz"), b.size("sz"));
        assert_eq!(b.v("rotationInfo", "angle"), 0);
    }
}

fn rewrite_section(bytes: &[u8], change: impl FnOnce(String) -> String) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut change = Some(change);
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let mut data = vec![];
        e.read_to_end(&mut data).unwrap();
        if e.name().unwrap() == "Contents/section0.xml" {
            data = change.take().unwrap()(String::from_utf8(data).unwrap()).into_bytes();
        }
        w.start_file(
            e.name().unwrap(),
            zip::write::SimpleFileOptions::default().compression_method(e.compression()),
        )
        .unwrap();
        w.write_all(&data).unwrap();
    }
    w.finish().unwrap().into_inner()
}
fn attr_first(mut xml: String, tag: &str, name: &str, value: &str) -> String {
    let begin = xml
        .match_indices(&format!("<{tag} "))
        .map(|(i, _)| i)
        .find(|&i| xml[i..i + xml[i..].find('>').unwrap()].contains(&format!(" {name}=\"")))
        .unwrap();
    let end = begin + xml[begin..].find('>').unwrap();
    let needle = format!(" {name}=\"");
    let at = begin + xml[begin..end].find(&needle).unwrap() + needle.len();
    let to = at + xml[at..].find('"').unwrap();
    xml.replace_range(at..to, value);
    xml
}
fn xml(bytes: &[u8]) -> String {
    PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap()
}

/// 이것을 실패시키는 것: angle/flip/offset 메타데이터를 합성 행렬 대신 사용.
#[test]
fn metadata_and_offset_probe_do_not_change_geometry_or_warn() {
    let source = fixture("shapes", "rotflip-single.hwpx");
    let changed = rewrite_section(&source, |x| {
        attr_first(
            attr_first(
                attr_first(attr_first(x, "hp:offset", "x", "3000"), "hp:offset", "y", "3000"),
                "hp:rotationInfo",
                "angle",
                "123",
            ),
            "hp:flip",
            "vertical",
            "1",
        )
    });
    assert_eq!(xml(&reencode(&source)), xml(&reencode(&changed)));
    let a = HwpxDecoder::decode(&source).unwrap();
    let b = HwpxDecoder::decode(&changed).unwrap();
    assert_eq!(a.warnings, b.warnings);
}

/// 이것을 실패시키는 것: transMatrix 이동을 버려 G7 탐침을 무시.
#[test]
fn matrix_translation_probe_reports_one_g7() {
    let source = fixture("shapes", "rotflip-single.hwpx");
    let changed = rewrite_section(&source, |x| attr_first(x, "hc:transMatrix", "e3", "3000"));
    let d = HwpxDecoder::decode(&changed).unwrap();
    assert_eq!(d.warnings.iter().filter(|w| format!("{w:?}").contains("G7;")).count(), 1);
    assert_eq!(xml(&reencode(&source)), xml(&reencode(&changed)));
}

/// 이것을 실패시키는 것: T의 선형부 무시 / 첫 쌍만 사용 / flip 이중 적용.
#[test]
fn equivalent_matrix_factorizations_share_one_core_geometry() {
    let source = fixture("shapes", "rotflip-single.hwpx");
    let m = wire(&source)[0].map();
    let matrix = |tag: &str, m: Matrix| {
        format!(
            "<hc:{tag} e1=\"{}\" e2=\"{}\" e3=\"{}\" e4=\"{}\" e5=\"{}\" e6=\"{}\"/>",
            m[0], m[1], m[2], m[3], m[4], m[5]
        )
    };
    let id = [1., 0., 0., 0., 1., 0.];
    for chain in [
        format!(
            "{}{}{}",
            matrix("transMatrix", m),
            matrix("scaMatrix", id),
            matrix("rotMatrix", id)
        ),
        format!(
            "{}{}{}{}{}",
            matrix("transMatrix", id),
            matrix("scaMatrix", [2., 0., 10., 0., 2., 20.]),
            matrix("rotMatrix", m),
            matrix("scaMatrix", [0.5, 0., -5., 0., 0.5, -10.]),
            matrix("rotMatrix", id)
        ),
    ] {
        let changed = rewrite_section(&source, |mut x| {
            let a = x.find("<hp:renderingInfo>").unwrap();
            let b = a + x[a..].find("</hp:renderingInfo>").unwrap() + "</hp:renderingInfo>".len();
            x.replace_range(a..b, &format!("<hp:renderingInfo>{chain}</hp:renderingInfo>"));
            x
        });
        assert_eq!(xml(&reencode(&source)), xml(&reencode(&changed)));
    }
}

/// 이것을 실패시키는 것: NaN 행렬을 항등으로 처리하고 성공 표시.
#[test]
fn invalid_matrix_has_specific_loss_and_unrotated_fallback() {
    let source = fixture("shapes", "rotflip-single.hwpx");
    let changed = rewrite_section(&source, |x| attr_first(x, "hc:rotMatrix", "e1", "NaN"));
    let d = HwpxDecoder::decode(&changed).unwrap();
    assert!(d.warnings.iter().any(|w| format!("{w:?}").contains("nonfinite renderingInfo")));
    let b = wire(&reencode(&changed));
    assert_eq!(b[0].v("flip", "horizontal"), 0);
    assert_eq!(b[0].size("sz"), wire(&source)[0].size("sz"));
}

/// 이것을 실패시키는 것: 여백 회전/이중 가산, crop 이중 축소, O와 I 혼동,
/// 넘친 글상자 외접 폭 단일 반올림, lastWidth에 D 사용.
#[test]
fn synthetic_exception_picture_and_textbox_use_contract_integers() {
    use hwpforge_core::control::ShapeStyle;
    use hwpforge_core::image::ImageCrop;
    use hwpforge_core::table::TableMargin;
    use hwpforge_core::{
        Control, Document, ObjectFrame, PageSettings, Paragraph, Run, RunContent, Section,
    };
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};
    let u = |x| HwpUnit::new(x).unwrap();
    // Reuse the committed small picture's asset; the 2.5 MB exceptions
    // package is deliberately absent from the repository.
    let d = HwpxDecoder::decode(&fixture("images", "cell_rowspan_image.hwpx")).unwrap();
    let mut image = None;
    d.document.for_each_paragraph(|p| {
        for r in &p.runs {
            if let RunContent::Image(i) = &r.content {
                if image.is_none() {
                    image = Some(i.clone());
                }
            }
        }
    });
    let mut image = image.expect("small fixture image");
    image.width = u(9168);
    image.height = u(9338);
    image.rotation = Some(30.);
    image.flip = None;
    image.frame = ObjectFrame::new().with_original_size(Size::new(u(9341), u(9338)));
    image.in_margin =
        Some(TableMargin { left: u(566), right: u(566), top: u(566), bottom: u(566) });
    image.image_extent = Some((u(153600), u(153600)));
    image.crop = Some(ImageCrop { left: u(0), top: u(0), right: u(150766), bottom: u(153600) });
    let mut textbox = Control::text_box(
        vec![Paragraph::with_runs(
            vec![Run::text("overflow", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        u(5790),
        u(12766),
    );
    if let Control::TextBox { frame, style, caption, .. } = &mut textbox {
        *caption = Some(hwpforge_core::caption::Caption::new(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            hwpforge_core::caption::CaptionSide::Bottom,
        ));
        *frame = ObjectFrame::new().with_original_size(Size::new(u(5790), u(3113)));
        *style = Some(ShapeStyle { rotation: Some(30.), ..Default::default() });
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::control(textbox, CharShapeIndex::new(0)),
                Run::image(image.clone(), CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let out = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &d.style_store,
        &d.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    let w = wire(&out.bytes);
    assert_eq!(w[0].size("sz"), (11398, 13950));
    assert_eq!(w[0].v("drawText", "lastWidth"), 11398);
    assert_eq!(w[0].v("caption", "width"), 11398);
    assert_eq!(w[1].size("orgSz"), (9341, 9338));
    assert_eq!(w[1].size("curSz"), (9168, 0));
    assert_eq!(w[1].size("sz"), (13740, 13802));
    assert_eq!((w[1].matrices[0][2], w[1].matrices[0][5]), (1720., 1666.));
    assert_eq!(w[1].v("imgDim", "dimwidth"), 153600);
    assert_eq!(w[1].v("imgClip", "right"), 150766);
    let again = HwpxDecoder::decode(&out.bytes).unwrap();
    let mut images = vec![];
    again.document.for_each_paragraph(|p| {
        for r in &p.runs {
            if let RunContent::Image(i) = &r.content {
                images.push(i.clone());
            }
        }
    });
    assert_eq!(images[0].in_margin, image.in_margin);
    assert_eq!(images[0].crop, image.crop);
    assert_eq!(images[0].width, image.width);
    check(&out.bytes, &reencode(&out.bytes), false);

    // 이것을 실패시키는 것: G7에 내용 상자를 source/recomputed sz로 표시하거나
    // 여백을 판정 상자에서 빼지 않기. 그림의 transMatrix만 이동한다.
    let picture_edit = |bytes: &[u8], edit: &dyn Fn(String) -> String| {
        rewrite_section(bytes, |x| {
            let at = x.find("<hp:pic ").unwrap();
            format!("{}{}", &x[..at], edit(x[at..].to_owned()))
        })
    };
    let shifted = picture_edit(&out.bytes, &|x| attr_first(x, "hc:transMatrix", "e3", "4720"));
    let shifted = HwpxDecoder::decode(&shifted).unwrap();
    let diagnostics: Vec<_> =
        shifted.warnings.iter().map(|w| format!("{w:?}")).filter(|w| w.contains("G7;")).collect();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("source sz=13740x13802"), "{diagnostics:?}");
    assert!(diagnostics[0].contains("recomputed sz=(13740, 13802)"));
    assert!(diagnostics[0].contains("transMatrix=(1720,1666)"));
    assert!(!again.warnings.iter().any(|w| format!("{w:?}").contains("G7;")));

    // 이것을 실패시키는 것: 검증 전 i32 여백 산술 / 잘못된 변을 ZERO로 대체 /
    // 음수 허용 / 첫 변만 검증 / 실패 후 D·회전·여백 부분 적용.
    for side in ["left", "right", "top", "bottom"] {
        for bad in [-1, i32::MIN, 100_000_001, 1_200_000_000, i32::MAX] {
            let malformed =
                picture_edit(&out.bytes, &|x| attr_first(x, "hp:inMargin", side, &bad.to_string()));
            let decoded = HwpxDecoder::decode(&malformed).unwrap();
            let losses: Vec<_> = decoded
                .warnings
                .iter()
                .map(|w| format!("{w:?}"))
                .filter(|w| w.contains("shape_transform"))
                .collect();
            assert_eq!(losses.len(), 1, "{side}={bad}: {losses:?}");
            assert!(losses[0].contains(&format!("invalid inner margin {side}={bad}")));
            assert!(losses[0].contains("using declared sz=13740x13802"));
            decoded.document.for_each_paragraph(|p| {
                for run in &p.runs {
                    if let RunContent::Image(i) = &run.content {
                        assert_eq!((i.width.as_i32(), i.height.as_i32()), (13740, 13802));
                        assert_eq!((i.rotation, i.flip, i.in_margin), (None, None, None));
                    }
                }
            });
            let reencoded = reencode(&malformed);
            let objects = wire(&reencoded);
            assert_eq!(objects[1].size("sz"), (13740, 13802));
        }
    }
    // 이것을 실패시키는 것: 무회전 그림의 불일치 메타데이터 경고 누락.
    let metadata_only = picture_edit(&out.bytes, &|mut x| {
        for tag in ["hc:transMatrix", "hc:scaMatrix", "hc:rotMatrix"] {
            for (name, value) in
                [("e1", "1"), ("e2", "0"), ("e3", "0"), ("e4", "0"), ("e5", "1"), ("e6", "0")]
            {
                x = attr_first(x, tag, name, value);
            }
        }
        x
    });
    let mismatch = HwpxDecoder::decode(&metadata_only).unwrap();
    let messages: Vec<_> = mismatch
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .filter(|w| w.contains("metadata mismatch"))
        .collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("picture id="));
    assert!(messages[0].contains("source angle=30, flip=(0,0)"));
    let all_huge = picture_edit(&out.bytes, &|mut x| {
        for side in ["left", "right", "top", "bottom"] {
            x = attr_first(x, "hp:inMargin", side, "1200000000");
        }
        x
    });
    assert_eq!(wire(&reencode(&all_huge))[1].size("sz"), (13740, 13802));
}

/// 이것을 실패시키는 것: 실제 HWP5 여백/원본 imgDim 누락 및 overflow 높이 복원 실패.
#[test]
#[ignore = "external large Hancom pair; small synthetic equivalent runs in CI"]
fn external_rot_exceptions_pair() {
    let dir =
        std::path::PathBuf::from(std::env::var("ROT_EXCEPTIONS_DIR").expect("ROT_EXCEPTIONS_DIR"));
    let source = std::fs::read(dir.join("rot-exceptions.hwp")).unwrap();
    let reference = std::fs::read(dir.join("rot-exceptions.hwpx")).unwrap();
    for cache in [false, true] {
        let (bytes, _) = hwp5_to_hwpx_bytes_with_options(
            &source,
            ConvertOptions::default().with_carry_layout_cache(cache),
        )
        .unwrap();
        check(&reference, &bytes, false);
        check(&bytes, &reencode(&bytes), false);
        let objects = wire(&bytes);
        let pic = objects.iter().find(|w| w.kind == "pic").unwrap();
        assert_eq!(pic.size("sz"), (13740, 13802));
        assert_eq!((pic.v("imgDim", "dimwidth"), pic.v("imgDim", "dimheight")), (153600, 153600));
        for side in ["left", "right", "top", "bottom"] {
            assert_eq!(pic.v("inMargin", side), 566);
        }
        assert_eq!(pic.v("imgClip", "right"), 150766);
    }
}

/// 이것을 실패시키는 것: 텍스트 splice에서 독립 도형을 재생성 / 번호 추가 소비.
#[test]
fn preservation_text_edit_keeps_raw_rotation_xml() {
    use hwpforge_smithy_hwpx::HwpxPatcher;
    let source = fixture("standalone-rotation", "rot-shapes.hwpx");
    let d = HwpxDecoder::decode(&source).unwrap();
    let original = d.document.sections()[0].clone();
    let mut edited = original.clone();
    let mut changed = false;
    for p in &mut edited.paragraphs {
        for run in &mut p.runs {
            if let hwpforge_core::RunContent::Text(t) = &mut run.content {
                if !t.is_empty() && !changed {
                    *t = "회전 시험".into();
                    changed = true;
                }
            }
        }
    }
    assert!(changed);
    let preservation = HwpxPatcher::export_section_preservation(&source, 0, &original).unwrap();
    let patched =
        HwpxPatcher::patch_section_preserving(&source, 0, &edited, None, Some(&preservation))
            .unwrap();
    let before = xml(&source);
    let after = xml(&patched);
    for kind in ["arc", "curve", "line", "connectLine", "rect"] {
        let mut rest = before.as_str();
        while let Some(at) = rest.find(&format!("<hp:{kind} ")) {
            rest = &rest[at..];
            let end = rest.find(&format!("</hp:{kind}>")).unwrap() + kind.len() + 6;
            assert!(after.contains(&rest[..end]), "{kind} was regenerated by text splice");
            rest = &rest[end..];
        }
    }
    assert_eq!(xml(&reencode(&source)), xml(&reencode(&source)), "generated IDs are deterministic");
}

/// 이것을 실패시키는 것: f32 360→0 뒤에도 읽기 분기 출처가 Core에 남아 있다고 가정.
/// Appendix A requires legacy curSz for identical normalized Core values.
#[test]
fn contract_collision_zero_normalized_rotation_and_legacy_cur_size() {
    use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};
    let u = |v| HwpUnit::new(v).unwrap();
    let mut rect = Control::rect(u(2000), u(1000)).unwrap();
    rect.frame_mut().unwrap().original_size = Some(Size::new(u(1000), u(1000)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(rect, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let styles = HwpxDecoder::decode(&fixture("shapes", "rotflip-single.hwpx")).unwrap();
    let source = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &styles.style_store,
        &styles.image_store,
        EncodeOptions::default(),
    )
    .unwrap()
    .bytes;
    let changed = rewrite_section(&source, |x| {
        attr_first(
            attr_first(x, "hc:rotMatrix", "e2", "0.00000001"),
            "hc:rotMatrix",
            "e4",
            "-0.00000001",
        )
    });
    let a = HwpxDecoder::decode(&source).unwrap();
    let b = HwpxDecoder::decode(&changed).unwrap();
    let m = wire(&changed)[0].map();
    assert!(m[1].abs() >= 1e-9 && m[3].abs() >= 1e-9, "new rotation path");
    assert_eq!(a.document.sections(), b.document.sections(), "the complete Core sections collide");
    let old = reencode(&source);
    let new = reencode(&changed);
    assert_eq!(xml(&old), xml(&new), "identical Core produces identical HWPX");
    if let Ok(dir) = std::env::var("ROT_COLLISION_OUTPUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("legacy-input.hwpx", &source),
            ("near-360-input.hwpx", &changed),
            ("legacy-output.hwpx", &old),
            ("near-360-output.hwpx", &new),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::write(
            dir.join("identical-core-sections.json"),
            serde_json::to_vec_pretty(a.document.sections()).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(wire(&old)[0].size("curSz"), (2000, 1000), "§3.1 legacy bytes");
    // Appendix A: normalization removed the effective transform.
    assert_eq!(
        wire(&new)[0].size("curSz"),
        (2000, 1000),
        "Appendix A: normalized zero uses legacy curSz"
    );
}

fn encode_controls(controls: Vec<hwpforge_core::Control>) -> Vec<u8> {
    encode_controls_diagnostics(controls).bytes
}

fn encode_controls_diagnostics(
    controls: Vec<hwpforge_core::Control>,
) -> hwpforge_smithy_hwpx::EncodeOutcome {
    use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let styles = HwpxDecoder::decode(&fixture("shapes", "rotflip-single.hwpx")).unwrap();
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            controls.into_iter().map(|c| Run::control(c, CharShapeIndex::new(0))).collect(),
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &styles.style_store,
        &styles.image_store,
        EncodeOptions::default(),
    )
    .unwrap()
}

fn native_arc(angle: f32) -> hwpforge_core::Control {
    use hwpforge_core::{
        control::{ShapePoint, ShapeStyle},
        Control,
    };
    use hwpforge_foundation::{ArcType, HwpUnit};
    let mut c =
        Control::arc(ArcType::Normal, HwpUnit::new(1000).unwrap(), HwpUnit::new(1000).unwrap());
    if let Control::Arc { definition, start1, end1, start2, end2, style, .. } = &mut c {
        *definition = hwpforge_core::control::ArcDefinition::AxisEndpoints;
        *start1 = ShapePoint::new(0, 0);
        *end1 = *start1;
        *start2 = *start1;
        *end2 = *start1;
        *style = Some(ShapeStyle { rotation: Some(angle), ..Default::default() });
    }
    c
}

/// 이것을 실패시키는 것: 부모 Arc의 XML 전체 replace로 캡션 안 타원·호·그룹까지 변환.
#[test]
fn native_arc_only_rewrites_its_own_fields() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        Control, Paragraph, Run, RunContent,
    };
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    let mut arc = native_arc(30.0);
    let d = HwpxDecoder::decode(&fixture("shapes", "rotflip-group.hwpx")).unwrap();
    let group = d.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Control(c) if matches!(c.as_ref(), Control::Group { .. }) => {
                Some(c.as_ref().clone())
            }
            _ => None,
        })
        .unwrap();
    let children = vec![
        Control::ellipse(HwpUnit::new(800).unwrap(), HwpUnit::new(400).unwrap()),
        native_arc(0.0),
        group,
    ];
    let reference = encode_controls(children.clone());
    if let Control::Arc { caption, .. } = &mut arc {
        *caption = Some(Caption::new(
            vec![Paragraph::with_runs(
                children.into_iter().map(|c| Run::control(c, CharShapeIndex::new(0))).collect(),
                ParaShapeIndex::new(0),
            )],
            CaptionSide::Bottom,
        ));
    }
    let out = encode_controls(vec![arc]);
    let objects = wire(&out);
    let expected = wire(&reference);
    assert_eq!(objects.len(), expected.len() + 1);
    for (a, b) in objects[1..].iter().zip(expected) {
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.fields, b.fields);
        assert_eq!(a.matrices, b.matrices);
    }
    assert_eq!(wire(&reencode(&out)).len(), objects.len());
}

/// 이것을 실패시키는 것: 선형부가 항등/NaN인 native Arc를 schema에서 Other로 폐기.
#[test]
fn native_arc_survives_zero_quantization_and_invalid_matrix() {
    for angle in [0.1, 359.9, 30.0] {
        let out = encode_controls(vec![native_arc(angle)]);
        assert_eq!(wire(&out).len(), 1);
        assert_eq!(wire(&reencode(&out)).len(), 1);
        let stale_level = rewrite_section(&out, |x| attr_first(x, "hp:arc", "groupLevel", "1"));
        assert_eq!(
            wire(&reencode(&stale_level)).len(),
            1,
            "paragraph membership overrides cached groupLevel"
        );
        let malformed = rewrite_section(&out, |x| attr_first(x, "hc:rotMatrix", "e1", "NaN"));
        let d = HwpxDecoder::decode(&malformed).unwrap();
        assert!(d.warnings.iter().any(|w| format!("{w:?}").contains("nonfinite renderingInfo")));
        assert_eq!(wire(&reencode(&malformed)).len(), 1);
    }
}

/// 이것을 실패시키는 것: 무회전 행렬의 angle/flip 메타데이터가 writer의 새 분기를 재활성화.
#[test]
fn unturned_matrix_metadata_cannot_reactivate_emission() {
    use hwpforge_core::Control;
    use hwpforge_foundation::HwpUnit;
    let u = |v| HwpUnit::new(v).unwrap();
    for control in [
        Control::rect(u(1000), u(1000)).unwrap(),
        Control::text_box(
            vec![hwpforge_core::Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0))],
            u(1000),
            u(1000),
        ),
    ] {
        let source = encode_controls(vec![control]);
        let changed = rewrite_section(&source, |x| {
            attr_first(
                attr_first(x, "hp:rotationInfo", "angle", "30"),
                "hp:flip",
                "horizontal",
                "1",
            )
        });
        assert_eq!(xml(&reencode(&source)), xml(&reencode(&changed)));
        let a = HwpxDecoder::decode(&source).unwrap();
        let b = HwpxDecoder::decode(&changed).unwrap();
        assert_eq!(a.document.sections(), b.document.sections());
        assert!(!a.warnings.iter().any(|w| format!("{w:?}").contains("metadata mismatch")));
        let mismatch: Vec<_> = b
            .warnings
            .iter()
            .map(|w| format!("{w:?}"))
            .filter(|w| w.contains("metadata mismatch"))
            .collect();
        assert_eq!(mismatch.len(), 1);
        assert!(mismatch[0].contains("id="));
        assert!(mismatch[0].contains("source angle=30, flip=(1,0)"));
        assert!(mismatch[0].contains("composite matrix has no rotation or reflection"));
        assert!(mismatch[0].contains("discarded rotation/flip metadata"));
        let kind = if xml(&source).contains("<hp:drawText") { "textbox" } else { "rect" };
        if let Ok(dir) = std::env::var("ROT_METADATA_INPUTS") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{kind}-plain.hwpx"), &source).unwrap();
            std::fs::write(format!("{dir}/{kind}-metadata.hwpx"), &changed).unwrap();
        }
        // Actual HEAD 69482cd6 decode/re-encode of these exact synthetic inputs.
        let head_plain_bytes = fixture("standalone-rotation", &format!("head-{kind}-plain.hwpx"));
        let head_metadata_bytes =
            fixture("standalone-rotation", &format!("head-{kind}-metadata.hwpx"));
        let head_plain = xml(&head_plain_bytes);
        let head_metadata = xml(&head_metadata_bytes);
        let parts = |bytes: &[u8]| {
            use std::io::Read;
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut parts = BTreeMap::new();
            for i in 0..zip.len() {
                let mut part = zip.by_index(i).unwrap();
                let mut bytes = vec![];
                part.read_to_end(&mut bytes).unwrap();
                parts.insert(part.name().unwrap().into_owned(), bytes);
            }
            parts
        };
        assert_eq!(
            parts(&reencode(&source)),
            parts(&head_plain_bytes),
            "true unturned HEAD package parts"
        );
        let current = reencode(&changed);
        assert_ne!(xml(&current), head_metadata, "Appendix B intentional HEAD difference");
        let mut head_parts = parts(&head_metadata_bytes);
        head_parts.insert("Contents/section0.xml".into(), xml(&current).into_bytes());
        assert_eq!(parts(&current), head_parts, "only the target section changes from HEAD");
        assert!(head_metadata.contains("angle=\"30\""));
        assert!(head_metadata.contains("horizontal=\"1\""));
        for (angle, horizontal, vertical) in [(30, 0, 0), (0, 1, 0), (0, 0, 1), (0, 0, 0)] {
            let input = rewrite_section(&source, |x| {
                let x = attr_first(x, "hp:rotationInfo", "angle", &angle.to_string());
                let x = attr_first(x, "hp:flip", "horizontal", &horizontal.to_string());
                attr_first(x, "hp:flip", "vertical", &vertical.to_string())
            });
            let decoded = HwpxDecoder::decode(&input).unwrap();
            assert_eq!(
                decoded
                    .warnings
                    .iter()
                    .filter(|w| format!("{w:?}").contains("metadata mismatch"))
                    .count(),
                usize::from(angle != 0 || horizontal != 0 || vertical != 0)
            );
            assert_eq!(xml(&reencode(&input)), head_plain);
        }
    }
}

/// 이것을 실패시키는 것: 작은 정의점 bounds로 O를 축소 / placement Some에서만 독립 B 계산.
#[test]
fn small_polygon_points_do_not_shrink_the_original_box() {
    use hwpforge_core::{
        control::{
            geometry::{fold_standalone, FoldInput, ShapeTransform},
            ShapePoint,
        },
        Control, ObjectPlacement,
    };
    for placement in [None, Some(ObjectPlacement::legacy_inline_defaults())] {
        let mut polygon = Control::polygon(vec![
            ShapePoint::new(0, 0),
            ShapePoint::new(20, 0),
            ShapePoint::new(0, 10),
        ])
        .unwrap();
        if let Control::Polygon { placement: p, .. } = &mut polygon {
            *p = placement;
        }
        fold_standalone(
            &mut polygon,
            &FoldInput::new(
                ShapeTransform::from_matrix([0., -1., 2000., 1., 0., 0.]),
                (1000., 2000.),
                (0., 0.),
            ),
        )
        .unwrap();
        let out = encode_controls(vec![polygon]);
        let objects = wire(&out);
        assert_eq!(objects[0].size("orgSz"), (1000, 2000));
        assert_eq!(objects[0].size("sz"), (2000, 1000));
        assert_eq!(objects[0].size("curSz"), (0, 0));
    }
}

/// 이것을 실패시키는 것: 큰 상자의 소수 회전 방출 손실을 경고 없이 성공으로 보고.
#[test]
fn whole_degree_emission_loss_is_reported() {
    use hwpforge_core::{control::ShapeStyle, Control};
    use hwpforge_foundation::HwpUnit;
    let mut rect =
        Control::rect(HwpUnit::new(100_000).unwrap(), HwpUnit::new(100_000).unwrap()).unwrap();
    if let Control::Rect { style, .. } = &mut rect {
        *style = Some(ShapeStyle { rotation: Some(30.25), ..Default::default() });
    }
    let out = encode_controls_diagnostics(vec![rect]);
    assert!(out.warnings.iter().any(|w| format!("{w:?}").contains("shape_emission")));
}

/// 이것을 실패시키는 것: 그림 offset=0 또는 source offset 보존.
#[test]
fn picture_offset_is_emitted_translation_and_hancom_oracle() {
    let answer = fixture("standalone-rotation", "rot-exceptions.hwpx");
    let (converted, _) = hwp5_to_hwpx_bytes_with_options(
        &fixture("standalone-rotation", "rot-exceptions.hwp"),
        ConvertOptions::default(),
    )
    .unwrap();
    for bytes in [&answer, &converted, &reencode(&answer), &reencode(&converted)] {
        let objects = wire(bytes);
        let p = objects.iter().find(|w| w.kind == "pic").unwrap();
        assert_eq!((p.v("offset", "x"), p.v("offset", "y")), (1720, 1666));
        assert_eq!((p.matrices[0][2], p.matrices[0][5]), (1720., 1666.));
    }
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
