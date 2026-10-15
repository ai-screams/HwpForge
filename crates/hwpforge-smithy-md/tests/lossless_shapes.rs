//! Lossless Markdown for text boxes, lines, ellipses and polygons (W3): the
//! style, vertical alignment, text margin and placement come back equal in
//! the body and in groups, their text is an exact subtree, and the old
//! `<textbox>text</textbox>` form still reads.

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{
    ArrowStyle, Control, Fill, GroupChild, LineStyle, ShapePoint, ShapeStyle,
};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::placement::ObjectPlacement;
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
use hwpforge_core::{Document, ObjectFrame, PageSettings, Paragraph, Run, RunContent, Section};
use hwpforge_foundation::{
    ArrowSize, ArrowType, CharShapeIndex, Color, DropCapStyle, Flip, GradientType, HwpUnit,
    ImageFillMode, ParaShapeIndex, PatternType, VerticalAlign,
};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn text(t: &str) -> Run {
    Run::text(t, CharShapeIndex::new(0))
}

fn doc(runs: Vec<Run>) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    d
}

fn encode(runs: Vec<Run>) -> Result<String, MdError> {
    MdEncoder::encode_lossless(&doc(runs).validate().unwrap())
}

fn round_trip(run: Run) -> Run {
    let md = encode(vec![run]).unwrap();
    MdDecoder::decode_lossless(&md).unwrap().sections()[0].paragraphs[0].runs[0].clone()
}

fn style_of(run: &Run) -> ShapeStyle {
    let RunContent::Control(c) = &run.content else { panic!() };
    match c.as_ref() {
        Control::TextBox { style, .. }
        | Control::Line { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Polygon { style, .. } => style.clone().unwrap(),
        other => panic!("{other:?}"),
    }
}

/// Every style field set, with the given fill and rotation.
fn full_style(fill: Option<Fill>, rotation: Option<f32>) -> ShapeStyle {
    ShapeStyle {
        line_color: Some(Color::from_rgb(255, 0, 0)),
        fill_color: Some(Color::from_rgb(0, 0, 255)),
        line_width: Some(u32::MAX),
        line_style: Some(LineStyle::DashDot),
        rotation,
        flip: Some(Flip::None),
        head_arrow: Some(ArrowStyle {
            arrow_type: ArrowType::Concave,
            size: ArrowSize::Large,
            filled: true,
        }),
        tail_arrow: Some(ArrowStyle {
            arrow_type: ArrowType::Oval,
            size: ArrowSize::Small,
            filled: false,
        }),
        fill,
        drop_cap_style: DropCapStyle::TripleLine,
        corner_ratio: Some(20),
    }
}

fn gradient() -> Fill {
    Fill::Gradient {
        gradient_type: GradientType::Radial,
        angle: -45,
        colors: vec![
            (Color::from_rgb(1, 2, 3), 0),
            (Color::from_rgb(4, 5, 6), 50),
            (Color::from_rgb(7, 8, 9), 100),
        ],
        center_x: Some(-10),
        center_y: None,
        step: Some(255),
        step_center: Some(0),
    }
}

fn floating() -> ObjectPlacement {
    let mut p = ObjectPlacement::legacy_inline_defaults();
    p.treat_as_char = false;
    p.vert_offset = u(-300);
    p.out_margin = Some(TableMargin { left: u(0), right: u(0), top: u(0), bottom: u(0) });
    p
}

/// The four shapes, each with every field set and the different fills, in
/// the body and inside a group; the text box's text holds a picture, a
/// group and a nested styled text box.
///
/// 이것을 실패시키는 것: 표지 `data-style` 삭제 · 선 색·두께·종류 각각 방출 삭제 ·
/// 화살표·flip·drop cap·corner 방출 삭제 · stops 순서 뒤집기 · 그러데이션 선택
/// 필드 삭제 · 무늬 방출 삭제 · `Fill::None` 을 없음으로 · 세로 정렬 Top 아닌데 생략 ·
/// 안쪽 여백 방출 삭제 · 묶음 자식 도형 갈래 삭제 · 글상자 안 그림을 비교에서 놓침.
#[test]
fn shape_round_trips_every_field() {
    let fills = [
        None,
        Some(Fill::None),
        Some(Fill::Solid { color: Color::from_rgb(0x12, 0x34, 0x56) }),
        Some(gradient()),
        Some(Fill::Pattern {
            pattern_type: PatternType::CrossDiagonal,
            fg_color: Color::from_rgb(9, 9, 9),
            bg_color: Color::from_rgb(250, 250, 250),
        }),
    ];
    let inner_box = Control::TextBox {
        paragraphs: vec![para(vec![text("안")])],
        width: u(100),
        height: u(100),
        frame: ObjectFrame::new(),
        placement: None,
        caption: None,
        style: Some(full_style(None, Some(1.5))),
        text_vertical_align: VerticalAlign::Bottom,
        text_margin: None,
    };
    let picture =
        Image::new("BinData/BIN0003", u(10), u(10), ImageFormat::Unknown("BIN0003".into()));
    for fill in fills {
        let style = Some(full_style(fill.clone(), Some(12.345678)));
        let text_box = Control::TextBox {
            paragraphs: vec![
                para(vec![text("글"), Run::image(picture.clone(), CharShapeIndex::new(2))]),
                para(vec![Run::control(inner_box.clone(), CharShapeIndex::new(1))]),
            ],
            width: u(9000),
            height: u(3000),
            frame: ObjectFrame::new(),
            placement: Some(floating()),
            caption: None,
            style: style.clone(),
            text_vertical_align: VerticalAlign::Center,
            text_margin: Some(TableMargin { left: u(1), right: u(2), top: u(3), bottom: u(4) }),
        };
        let line = Control::Line {
            start: ShapePoint::new(-5, 0),
            end: ShapePoint::new(1000, 7),
            width: u(1005),
            height: u(7),
            frame: ObjectFrame::new(),
            placement: Some(floating()),
            caption: None,
            style: style.clone(),
        };
        let ellipse = Control::Ellipse {
            center: ShapePoint::new(50, 50),
            axis1: ShapePoint::new(100, 50),
            axis2: ShapePoint::new(50, 100),
            width: u(100),
            height: u(100),
            frame: ObjectFrame::new(),
            placement: None,
            paragraphs: vec![para(vec![text("타원")])],
            caption: None,
            style: style.clone(),
            text_vertical_align: VerticalAlign::Bottom,
        };
        let polygon = Control::Polygon {
            vertices: vec![ShapePoint::new(0, 0), ShapePoint::new(10, 0), ShapePoint::new(0, -10)],
            width: u(10),
            height: u(10),
            frame: ObjectFrame::new(),
            placement: None,
            paragraphs: Vec::new(),
            caption: None,
            style,
            text_vertical_align: VerticalAlign::Top,
        };
        for shape in [text_box, line, ellipse, polygon] {
            let run = Run::control(shape.clone(), CharShapeIndex::new(3));
            assert_eq!(round_trip(run.clone()), run, "{fill:?}");
            let group = Control::Group {
                children: vec![GroupChild::Picture(picture.clone()), GroupChild::Shape(shape)],
                width: u(9000),
                height: u(9000),
                frame: ObjectFrame::new(),
                placement: None,
                inst_id: None,
            };
            let run = Run::control(group, CharShapeIndex::new(3));
            assert_eq!(round_trip(run.clone()), run, "group {fill:?}");
        }
    }
}

/// A rotation comes back bit for bit: `12.345678` (not two decimals), `-0`
/// kept apart from `0`, `Some(0)` from `None`; a value that is not finite is
/// refused by the encoder.
///
/// 이것을 실패시키는 것: rotation 을 `{:.2}` 로 · `-0.0` 을 `0` 으로 · 비유한값 통과.
#[test]
fn rotation_is_written_exactly() {
    let shape = |rotation| {
        let mut e = Control::ellipse(u(10), u(10));
        if let Control::Ellipse { style, .. } = &mut e {
            *style = Some(ShapeStyle { rotation, ..ShapeStyle::default() });
        }
        Run::control(e, CharShapeIndex::new(0))
    };
    for r in [Some(12.345678_f32), Some(-0.0), Some(0.0), None] {
        let back = style_of(&round_trip(shape(r))).rotation;
        assert_eq!(back.map(f32::to_bits), r.map(f32::to_bits), "{r:?}");
    }
    for r in [f32::NAN, f32::INFINITY] {
        assert!(matches!(encode(vec![shape(Some(r))]), Err(MdError::UnsupportedStructure { .. })));
    }
}

/// The spelled values come from one table on both sides, so the markup is
/// checked against strings written here: colors as `#RRGGBB` (red is
/// `#FF0000`, blue `#0000FF`), Core variant names, stops in order.
///
/// 이것을 실패시키는 것: 색 R·B 뒤바꿈 · 철자 하나를 바꾸기(양쪽 함께) · stops 순서 뒤집기.
#[test]
fn shape_markup_spells_core_names() {
    let mut e = Control::ellipse(u(10), u(10));
    if let Control::Ellipse { style, .. } = &mut e {
        *style = Some(full_style(Some(gradient()), Some(1.0)));
    }
    let md = encode(vec![Run::control(e, CharShapeIndex::new(0))]).unwrap();
    for expected in [
        r##"data-style="true" data-line-color="#FF0000" data-line-width-unit="4294967295" data-line-style="DashDot" data-fill-color="#0000FF" data-rotation="1" data-flip="None""##,
        r#"data-head-arrow-type="Concave" data-head-arrow-size="Large" data-head-arrow-filled="true" data-tail-arrow-type="Oval" data-tail-arrow-size="Small" data-tail-arrow-filled="false""#,
        r##"data-fill-kind="Gradient" data-fill-gradient-type="Radial" data-fill-gradient-angle="-45" data-fill-gradient-stops="#010203@0;#040506@50;#070809@100" data-fill-gradient-center-x="-10" data-fill-gradient-step="255" data-fill-gradient-step-center="0""##,
        r#"data-drop-cap="TripleLine" data-corner-ratio="20""#,
    ] {
        assert!(md.contains(expected), "{expected}\n{md}");
    }
}

/// An image fill, a shape caption and a color with its reserved high byte
/// set refuse the document.
///
/// 이것을 실패시키는 것: 그림 채우기 거부 · 도형 caption 거부 · 예약 바이트 거부 각각 삭제.
#[test]
fn what_shapes_cannot_hold_is_refused() {
    let with = |f: &dyn Fn(&mut Option<ShapeStyle>, &mut Option<Caption>)| {
        let mut e = Control::ellipse(u(10), u(10));
        if let Control::Ellipse { style, caption, .. } = &mut e {
            f(style, caption);
        }
        encode(vec![Run::control(e, CharShapeIndex::new(0))])
    };
    let refused =
        |r: Result<String, MdError>| matches!(r, Err(MdError::UnsupportedStructure { .. }));
    assert!(refused(with(&|s, _| {
        *s = Some(ShapeStyle {
            fill: Some(Fill::Image { image_id: "a".into(), mode: ImageFillMode::default() }),
            ..ShapeStyle::default()
        })
    })));
    assert!(refused(with(&|_, c| {
        *c = Some(Caption::new(vec![para(vec![text("c")])], CaptionSide::Bottom))
    })));
    assert!(refused(with(&|s, _| {
        *s = Some(ShapeStyle {
            line_color: Some(Color::from_raw(0x0100_0000)),
            ..ShapeStyle::default()
        })
    })));
    assert!(with(&|_, _| {}).is_ok());
}

/// A shape's text is an exact subtree, as a picture caption is: a paragraph
/// without runs, a table without rows, a cell or a note without paragraphs
/// refuse the document;
/// and an object under a table caption inside a text box is refused too.
///
/// 이것을 실패시키는 것: 글상자 문단 `runs: []` · 글상자 안 빈 각주 · 빈 행 표 · 빈 칸
/// 거부 각각 삭제 · 글상자 → 표 → caption → 그림 거부 삭제.
#[test]
fn shape_text_admission() {
    let cell = |paragraphs: Vec<Paragraph>| TableCell::new(paragraphs, u(3000));
    let table = |rows: Vec<TableRow>| Run::table(Table::new(rows), CharShapeIndex::new(0));
    let captioned_table = {
        let t = Table::new(vec![TableRow::new(vec![cell(vec![para(vec![text("c")])])])])
            .with_caption(Caption::new(
                vec![para(vec![Run::image(
                    Image::new("a", u(1), u(1), ImageFormat::Png),
                    CharShapeIndex::new(0),
                )])],
                CaptionSide::Bottom,
            ));
        Run::table(t, CharShapeIndex::new(0))
    };
    let cases: Vec<(&str, Vec<Paragraph>)> = vec![
        ("no runs", vec![Paragraph::with_runs(Vec::new(), ParaShapeIndex::new(0))]),
        ("no rows", vec![para(vec![table(Vec::new())])]),
        ("empty cell", vec![para(vec![table(vec![TableRow::new(vec![cell(Vec::new())])])])]),
        (
            "empty note",
            vec![para(vec![Run::control(Control::footnote(Vec::new()), CharShapeIndex::new(0))])],
        ),
        ("table caption object", vec![para(vec![captioned_table])]),
    ];
    for (name, paragraphs) in cases {
        let tb = Control::text_box(paragraphs, u(100), u(100));
        let d = doc(vec![Run::control(tb, CharShapeIndex::new(0))]);
        if let Ok(v) = d.validate() {
            assert!(
                matches!(MdEncoder::encode_lossless(&v), Err(MdError::UnsupportedStructure { .. })),
                "{name}"
            );
        }
    }
}

const SECTION: &str = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0">"#;
const BOX: &str = r#"data-char-shape="0" data-width-unit="10" data-height-unit="10""#;

fn decode_body(body: &str) -> Result<Document, MdError> {
    let md = encode(vec![text("t")]).unwrap();
    let frontmatter = &md[..md.find("<section").unwrap()];
    MdDecoder::decode_lossless(&format!("{frontmatter}{SECTION}{body}</p></section>\n"))
}

/// The old form `<textbox>text</textbox>` (no `<p>`, no new attribute)
/// still reads as one paragraph, as before; an old box with paragraphs reads
/// with no style.
///
/// 이것을 실패시키는 것: 옛 형식 `<textbox>글자</textbox>` 거부.
#[test]
fn old_text_box_form_reads() {
    let d = decode_body(&format!("<textbox {BOX}>글자</textbox>")).unwrap();
    let RunContent::Control(c) = &d.sections()[0].paragraphs[0].runs[0].content else { panic!() };
    let Control::TextBox { paragraphs, style, .. } = c.as_ref() else { panic!() };
    assert_eq!(paragraphs.len(), 1);
    assert_eq!(paragraphs[0].runs[0].content.plain_text().unwrap(), "글자");
    assert!(style.is_none());
}

/// STYLE and shape grammar errors.
///
/// 이것을 실패시키는 것: 표지 없는 STYLE 속성 허용 · 화살표 3 개 중 하나 누락 허용 ·
/// Solid 에 gradient 속성 섞임 허용 · `line_width` 부호 있는 파싱 · 비유한 rotation 읽기 ·
/// `p` 와 텍스트 섞임 허용 · 새 형식 글상자의 문단 없음 허용 · 빈 stops 거부 ·
/// 옛 형식 판정 조건 하나 지우기 · 빈 점 열 거부 · 쌍이 아닌 점 허용.
#[test]
fn shape_grammar_errors() {
    let ellipse = |attrs: &str| {
        decode_body(&format!(
            r#"<ellipse {BOX} data-cx="0" data-cy="0" data-ax1-x="0" data-ax1-y="0" data-ax2-x="0" data-ax2-y="0" {attrs} />"#
        ))
    };
    assert!(ellipse("").is_ok());
    assert!(matches!(
        ellipse(r##"data-line-color="#000000""##),
        Err(MdError::LosslessParse { .. })
    ));
    assert!(matches!(
        ellipse(r#"data-style="true" data-head-arrow-type="Normal" data-head-arrow-size="Small""#),
        Err(MdError::LosslessMissingAttribute { attribute: "data-head-arrow-filled", .. })
    ));
    assert!(matches!(
        ellipse(
            r##"data-style="true" data-fill-kind="Solid" data-fill-solid-color="#000000" data-fill-gradient-angle="0""##
        ),
        Err(MdError::LosslessParse { .. })
    ));
    assert!(matches!(
        ellipse(r#"data-style="true" data-line-width-unit="-1""#),
        Err(MdError::LosslessInvalidAttribute { .. })
    ));
    for r in ["NaN", "inf", "x"] {
        assert!(
            matches!(
                ellipse(&format!(r#"data-style="true" data-rotation="{r}""#)),
                Err(MdError::LosslessInvalidAttribute { .. })
            ),
            "{r}"
        );
    }
    assert!(matches!(
        ellipse(r#"data-style="true" data-fill-kind="Image""#),
        Err(MdError::LosslessInvalidAttribute { .. })
    ));
    let d = ellipse(r#"data-style="true" data-fill-kind="Gradient" data-fill-gradient-type="Linear" data-fill-gradient-angle="0" data-fill-gradient-stops="""#)
        .unwrap();
    let RunContent::Control(c) = &d.sections()[0].paragraphs[0].runs[0].content else { panic!() };
    let Control::Ellipse { style: Some(st), .. } = c.as_ref() else { panic!() };
    assert!(matches!(&st.fill, Some(Fill::Gradient { colors, .. }) if colors.is_empty()));
    let p = r#"<p data-para-shape="0"><span data-char-shape="0">a</span></p>"#;
    assert!(matches!(
        decode_body(&format!("<textbox {BOX}>{p}words</textbox>")),
        Err(MdError::LosslessParse { .. })
    ));
    const PLACE: &str = r#"data-text-wrap="Square" data-text-flow="BothSides" data-treat-as-char="false" data-flow-with-text="true" data-allow-overlap="false" data-vert-rel-to="Para" data-horz-rel-to="Para" data-vert-offset-unit="0" data-horz-offset-unit="0" data-vert-align="TOP" data-horz-align="LEFT" data-affect-line-spacing="false" data-hold-anchor="false""#;
    for new_attr in [
        r#"data-text-valign="Center""#,
        r#"data-style="true""#,
        PLACE,
        r#"data-text-margin-left-unit="0" data-text-margin-right-unit="0" data-text-margin-top-unit="0" data-text-margin-bottom-unit="0""#,
    ] {
        assert!(
            matches!(
                decode_body(&format!("<textbox {BOX} {new_attr}>x</textbox>")),
                Err(MdError::LosslessParse { .. })
            ),
            "{new_attr}"
        );
    }
    let d = decode_body(&format!(r#"<polygon {BOX} data-points="" />"#)).unwrap();
    let RunContent::Control(c) = &d.sections()[0].paragraphs[0].runs[0].content else { panic!() };
    assert!(matches!(c.as_ref(), Control::Polygon { vertices, .. } if vertices.is_empty()));
    assert!(matches!(
        decode_body(&format!(r#"<polygon {BOX} data-points="1;2" />"#)),
        Err(MdError::LosslessInvalidAttribute { .. })
    ));
}

/// Rectangles, arcs, curves and connect lines (W4) come back equal in the
/// body and in a group: every arc point told apart, a curve whose segment
/// list is not one shorter than its points, empty curve lists, a connect
/// line with and without routing points and a control character in its
/// type.
///
/// 이것을 실패시키는 것: `arc` 점 하나씩 누락 · `segments` 를 점 개수에서 다시 만듦 ·
/// `control-points` 빈 값 거부 · `connect-type` 에 `escape_attr` 대신 `escape_html`.
#[test]
fn w4_shapes_round_trip() {
    use hwpforge_foundation::{ArcType, CurveSegmentType};
    let style = Some(full_style(Some(Fill::Solid { color: Color::from_rgb(1, 1, 1) }), Some(3.0)));
    let p = |x, y| ShapePoint::new(x, y);
    let rect = Control::Rect {
        width: u(100),
        height: u(50),
        frame: ObjectFrame::new(),
        placement: Some(floating()),
        caption: None,
        style: style.clone(),
    };
    let arc = Control::Arc {
        arc_type: ArcType::Pie,
        center: p(1, 2),
        axis1: p(3, 4),
        axis2: p(5, 6),
        start1: p(7, 8),
        end1: p(9, 10),
        start2: p(11, 12),
        end2: p(13, 14),
        width: u(100),
        height: u(100),
        frame: ObjectFrame::new(),
        placement: None,
        caption: None,
        style: style.clone(),
    };
    let curve = |points: Vec<ShapePoint>, segment_types: Vec<CurveSegmentType>| Control::Curve {
        points,
        segment_types,
        width: u(100),
        height: u(100),
        frame: ObjectFrame::new(),
        placement: None,
        caption: None,
        style: style.clone(),
    };
    let connect = |control_points: Vec<ShapePoint>, connect_type: &str| Control::ConnectLine {
        start: p(0, 0),
        end: p(100, -20),
        control_points,
        connect_type: connect_type.into(),
        width: u(100),
        height: u(20),
        frame: ObjectFrame::new(),
        placement: None,
        caption: None,
        style: None,
    };
    for shape in [
        rect,
        arc,
        curve(vec![p(0, 0), p(5, 5), p(9, 0)], vec![CurveSegmentType::Curve]),
        curve(Vec::new(), Vec::new()),
        connect(Vec::new(), "STRAIGHT_ONEWAY"),
        connect(vec![p(50, 0), p(50, -20)], "BENT\tx"),
    ] {
        let run = Run::control(shape.clone(), CharShapeIndex::new(3));
        assert_eq!(round_trip(run.clone()), run);
        let group = Control::Group {
            children: vec![GroupChild::Shape(shape)],
            width: u(100),
            height: u(100),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        let run = Run::control(group, CharShapeIndex::new(3));
        assert_eq!(round_trip(run.clone()), run);
    }
}

/// A text box without paragraphs (style or not) and a polygon with fewer
/// than three points pass Core validation where it does not walk — a
/// picture caption, a text box's text, a cell or a note in either — but
/// would not come back as written: the encoder refuses them.
///
/// 이것을 실패시키는 것: 빈 글상자 거부 삭제 · 점 세 개 미만 다각형 거부 삭제.
#[test]
fn nested_empty_shapes_are_refused() {
    let empty_box = |style: Option<ShapeStyle>| {
        let mut tb = Control::text_box(Vec::new(), u(10), u(10));
        if let Control::TextBox { style: s, .. } = &mut tb {
            *s = style;
        }
        Run::control(tb, CharShapeIndex::new(0))
    };
    let flat_polygon = {
        let mut p = Control::polygon(vec![
            ShapePoint::new(0, 0),
            ShapePoint::new(1, 0),
            ShapePoint::new(0, 1),
        ])
        .unwrap();
        if let Control::Polygon { vertices, .. } = &mut p {
            vertices.clear();
        }
        Run::control(p, CharShapeIndex::new(0))
    };
    let in_caption = |leaf: Run| {
        let mut image = Image::new("a", u(10), u(10), ImageFormat::Png);
        image.caption = Some(Caption::new(vec![para(vec![leaf])], CaptionSide::Bottom));
        Run::image(image, CharShapeIndex::new(0))
    };
    let in_box = |leaf: Run| {
        Run::control(
            Control::text_box(vec![para(vec![leaf])], u(10), u(10)),
            CharShapeIndex::new(0),
        )
    };
    let in_cell = |leaf: Run| {
        Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![para(vec![in_box(leaf)])],
                u(100),
            )])]),
            CharShapeIndex::new(0),
        )
    };
    let in_note = |leaf: Run| {
        Run::control(Control::footnote(vec![para(vec![in_box(leaf)])]), CharShapeIndex::new(0))
    };
    for leaf in [empty_box(None), empty_box(Some(ShapeStyle::default())), flat_polygon] {
        for (name, run) in [
            ("caption", in_caption(leaf.clone())),
            ("box", in_box(leaf.clone())),
            ("cell", in_cell(leaf.clone())),
            ("note", in_note(leaf.clone())),
        ] {
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![para(vec![run])], PageSettings::a4()));
            let Ok(v) = d.validate() else { continue };
            assert!(
                matches!(MdEncoder::encode_lossless(&v), Err(MdError::UnsupportedStructure { .. })),
                "{name}"
            );
        }
    }
    // At least the caption path reaches the encoder (validation does not walk it).
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(
        vec![para(vec![in_caption(empty_box(None))])],
        PageSettings::a4(),
    ));
    assert!(d.validate().is_ok());
}
