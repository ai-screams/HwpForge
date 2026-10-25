//! The 1 MiB minimum stack documented on `HwpxDecoder::decode` for optimized
//! builds, checked on the public path with real packages.
//!
//! This lives in an integration test, not in the library's unit tests, so it
//! measures the library as it ships. The unit-test binary compiles the
//! `#[cfg(test)]` code together with the decoder, and with
//! `codegen-units = 1` a unit test that calls the deserializer on an inner
//! type changes how the recursive deserialize chain is inlined: there the
//! same inputs needed about twice the stack (1,329 KiB against 631 KiB for
//! B 44 on macOS arm64), while the shipped decoder did not change.
//!
//! Only optimized builds are covered (an unoptimized build needs more), so
//! the whole file is compiled out of debug builds and CI runs it with
//! `--cargo-profile release` in `Verify › Python`. An overflow aborts the
//! test process, so a failure shows as SIGABRT.
#![cfg(not(debug_assertions))]

#[path = "support/depth_package.rs"]
mod support;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxError};
use support::{nested_section, package, Place, DEEPEST};

/// The minimum stack `HwpxDecoder::decode` documents for optimized builds.
const DOCUMENTED_MIN_STACK: usize = 1 << 20;

/// Runs the public `HwpxDecoder::decode` on a thread with `stack` bytes;
/// returns the decoded sections' debug text.
fn decode_package_on_stack(bytes: Vec<u8>, stack: usize) -> Result<String, HwpxError> {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || HwpxDecoder::decode(&bytes).map(|d| format!("{:?}", d.document.sections())))
        .expect("spawn decode thread")
        .join()
        .expect("decode thread panicked")
}

/// B 44 is the deepest text-box input the recursion budget still admits and
/// the input that needs the most stack; B 100 is past the budget.
// 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 10000 으로 올리기 — B 100 이
// 한도에 막히지 않고 1 MiB 스택을 넘쳐 테스트 프로세스가 abort 한다.
// 스택을 512 KiB 로 줄이기 — B 44 가 abort 한다.
// 이것을 실패시키는 것: subList 가드 문구에서 depth + 1 → depth.
#[test]
fn xml_limit_release_decodes_on_documented_1_mib_stack() {
    let decode = |pattern: &str, levels: usize| {
        decode_package_on_stack(
            package(nested_section(pattern, levels), Place::Body),
            DOCUMENTED_MIN_STACK,
        )
    };
    for (pattern, levels) in [("T", 32), ("B", 32), ("TB", 32)] {
        let debug = decode(pattern, levels).expect("32 levels decode on 1 MiB");
        assert!(debug.contains(DEEPEST), "{pattern} x{levels}: deepest text lost");
    }
    assert!(matches!(
        decode("B", 44),
        Err(HwpxError::InvalidStructure { detail }) if detail == "sublist nesting depth 33 exceeds limit of 32 in Contents/section0.xml"
    ));
    assert!(matches!(
        decode("B", 100),
        Err(HwpxError::XmlParse { detail, .. }) if detail.contains("recursion limit of 224")
    ));
}

mod relations {
    //! The 21 ownership relations Core counts toward nesting depth (the same
    //! list as `hwpforge-core`'s `depth::tests::EDGES`), each at exactly
    //! depth 32: a chain of text boxes topped by one relation, encoded from
    //! Core and decoded on the documented stack.
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::control::{Control, FieldBoundary, FieldStart, GroupChild, ShapePoint};
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{Document, ObjectFrame, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{ArcType, CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxEncoder};

    /// Relation and its depth cost (Core `depth::tests::EDGES`).
    pub(super) const RELATIONS: [(&str, usize); 21] = [
        ("table cell", 1),
        ("table caption", 1),
        ("image caption", 2),
        ("textbox body", 1),
        ("ellipse body", 1),
        ("polygon body", 1),
        ("textbox caption", 2),
        ("ellipse caption", 2),
        ("polygon caption", 2),
        ("line caption", 2),
        ("rect caption", 2),
        ("arc caption", 2),
        ("curve caption", 2),
        ("connect caption", 2),
        ("footnote", 1),
        ("endnote", 1),
        ("memo content", 1),
        ("memo anchor", 1),
        ("field annotation", 1),
        ("group shape", 2),
        ("group picture", 3),
    ];

    fn u() -> HwpUnit {
        HwpUnit::new(1000).unwrap()
    }
    fn ps(r: Run) -> Vec<Paragraph> {
        vec![Paragraph::with_runs(vec![r], ParaShapeIndex::new(0))]
    }
    fn ctrl(c: Control) -> Run {
        Run::control(c, CharShapeIndex::new(0))
    }
    fn cap(r: Run) -> Caption {
        Caption::new(ps(r), CaptionSide::Bottom)
    }
    fn group(child: GroupChild) -> Control {
        Control::Group {
            children: vec![child],
            width: u(),
            height: u(),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        }
    }
    fn polygon() -> Control {
        let p = |x, y| ShapePoint::new(x, y);
        Control::polygon(vec![p(0, 0), p(100, 0), p(0, 100)]).unwrap()
    }

    fn wrap(relation: &str, r: Run) -> Run {
        match relation {
            "table cell" => Run::table(
                Table::new(vec![TableRow::new(vec![TableCell::new(ps(r), u())])]),
                CharShapeIndex::new(0),
            ),
            "table caption" => {
                let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(
                    ps(Run::text("c", CharShapeIndex::new(0))),
                    u(),
                )])]);
                t.caption = Some(cap(r));
                Run::table(t, CharShapeIndex::new(0))
            }
            "image caption" | "group picture" => {
                // An assetless frame needs no image store entry.
                let mut i = Image::new("", u(), u(), ImageFormat::Png);
                i.caption = Some(cap(r));
                if relation == "group picture" {
                    ctrl(group(GroupChild::Picture(i)))
                } else {
                    Run::image(i, CharShapeIndex::new(0))
                }
            }
            "textbox body" => ctrl(Control::text_box(ps(r), u(), u())),
            "ellipse body" => ctrl(Control::ellipse_with_text(u(), u(), ps(r))),
            "polygon body" => {
                let mut c = polygon();
                if let Control::Polygon { paragraphs, .. } = &mut c {
                    *paragraphs = ps(r);
                }
                ctrl(c)
            }
            "footnote" => ctrl(Control::footnote(ps(r))),
            "endnote" => ctrl(Control::endnote(ps(r))),
            "memo content" => ctrl(Control::memo(ps(r))),
            "memo anchor" => ctrl(Control::memo_with_anchor(
                ps(Run::text("memo", CharShapeIndex::new(0))),
                vec![r],
            )),
            "field annotation" => {
                // The reader carries a field's sub-list only for `MEMO`; one
                // without memo parameters stays a field boundary.
                let mut s = FieldStart::default();
                s.id = "1".into();
                s.kind = "MEMO".into();
                s.annotation = Some(ps(r));
                ctrl(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) })
            }
            "group shape" => ctrl(group(GroupChild::Shape(Control::text_box(ps(r), u(), u())))),
            other => {
                let a = ShapePoint::new(0, 0);
                let b = ShapePoint::new(100, 100);
                let mut c = match other {
                    "textbox caption" => {
                        Control::text_box(ps(Run::text("b", CharShapeIndex::new(0))), u(), u())
                    }
                    "ellipse caption" => Control::ellipse(u(), u()),
                    "polygon caption" => polygon(),
                    "line caption" => Control::line(a, b).unwrap(),
                    "rect caption" => Control::rect(u(), u()).unwrap(),
                    "arc caption" => Control::arc(ArcType::default(), u(), u()),
                    "curve caption" => Control::curve(vec![a, b]).unwrap(),
                    "connect caption" => Control::connect_line(a, b).unwrap(),
                    _ => panic!("unknown relation {other}"),
                };
                match &mut c {
                    Control::TextBox { caption, .. }
                    | Control::Ellipse { caption, .. }
                    | Control::Polygon { caption, .. }
                    | Control::Line { caption, .. }
                    | Control::Rect { caption, .. }
                    | Control::Arc { caption, .. }
                    | Control::Curve { caption, .. }
                    | Control::ConnectLine { caption, .. } => *caption = Some(cap(r)),
                    _ => unreachable!(),
                }
                ctrl(c)
            }
        }
    }

    /// A package whose body holds `relation` over text boxes, `depth` in all.
    pub(super) fn package_at(relation: &str, cost: usize, depth: usize) -> Vec<u8> {
        let mut r = Run::text(super::DEEPEST, CharShapeIndex::new(0));
        for _ in 0..depth - cost {
            r = ctrl(Control::text_box(ps(r), u(), u()));
        }
        let mut runs = vec![wrap(relation, r)];
        if relation == "field annotation" {
            runs.push(Run::text("body", CharShapeIndex::new(0)));
            runs.push(ctrl(Control::FieldBoundary {
                boundary: FieldBoundary::End { begin_id: "1".into(), field_id: String::new() },
            }));
        }
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(runs, ParaShapeIndex::new(0))],
            PageSettings::a4(),
        ));
        let styles = style_store_for_preset("default").unwrap();
        HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap()
    }
}

/// Every relation Core counts decodes at depth 32 on the documented stack,
/// with the innermost text kept. A memo anchor is the one relation HWPX
/// cannot nest through: the encoder drops a non-text anchor run (reported as
/// `memo_anchor` content loss) and the decoder folds only text-only spans into
/// a memo, so a decoded memo anchor never holds a container.
// 이것을 실패시키는 것: 메모·필드 주석 subList 를 `depth + 1` 로 디코드하기
// (`field_start_boundary`) — "memo content"·"field annotation" 32단계가
// sublist 한도에 걸린다(T·B·TB 는 그대로 통과).
#[test]
fn every_relation_decodes_at_depth_32_on_documented_1_mib_stack() {
    for (relation, cost) in relations::RELATIONS {
        if relation == "memo anchor" {
            continue;
        }
        let bytes = relations::package_at(relation, cost, 32);
        let debug = decode_package_on_stack(bytes, DOCUMENTED_MIN_STACK)
            .unwrap_or_else(|e| panic!("{relation}: {e}"));
        assert!(debug.contains(DEEPEST), "{relation}: deepest text lost");
    }
}
