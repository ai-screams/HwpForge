use super::*;

pub(super) type Matrix = [f64; 6];
pub(super) fn mul(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}
pub(super) fn point(m: Matrix, p: (f64, f64)) -> (f64, f64) {
    (m[0] * p.0 + m[1] * p.1 + m[2], m[3] * p.0 + m[4] * p.1 + m[5])
}
pub(super) fn attrs(e: &BytesStart<'_>) -> BTreeMap<String, String> {
    e.attributes()
        .map(|a| {
            let a = a.unwrap();
            (a.key.as_ref().to_string(), a.value.to_string())
        })
        .collect()
}
#[derive(Debug, Default)]
pub(super) struct Wire {
    pub(super) kind: String,
    pub(super) fields: BTreeMap<String, BTreeMap<String, String>>,
    pub(super) matrices: Vec<Matrix>,
    pub(super) points: Vec<(String, BTreeMap<String, String>)>,
}
impl Wire {
    pub(super) fn v(&self, tag: &str, attr: &str) -> i32 {
        self.fields[tag][attr].parse::<i64>().unwrap() as i32
    }
    pub(super) fn size(&self, tag: &str) -> (i32, i32) {
        (self.v(tag, "width"), self.v(tag, "height"))
    }
    pub(super) fn map(&self) -> Matrix {
        self.matrices[1..].chunks_exact(2).fold(self.matrices[0], |m, p| mul(m, mul(p[1], p[0])))
    }
}
pub(super) fn wire(bytes: &[u8]) -> Vec<Wire> {
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
pub(super) fn reencode(bytes: &[u8]) -> Vec<u8> {
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

pub(super) fn check(reference: &[u8], bytes: &[u8], g7: bool) {
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
