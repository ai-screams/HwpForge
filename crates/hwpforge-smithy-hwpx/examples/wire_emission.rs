//! Reproducible documents exercising drawing and note wire emission.
use hwpforge_core::{
    control::{Fill, GroupChild, ShapePoint, ShapeStyle},
    image::{Image, ImageFormat, ImageStore},
    table::{Table, TableCell, TableRow},
    Control, Document, ObjectFrame, ObjectPlacement, PageSettings, Paragraph, Run, Section,
};
use hwpforge_foundation::{
    ArcType, CharShapeIndex, Color, Flip, GradientType, HwpUnit, ImageFillMode, ParaShapeIndex,
    PatternType, Size, VerticalAlign,
};
use hwpforge_smithy_hwpx::{
    style_store::{HwpxBorderFill, HwpxCharShape, HwpxParaShape, HwpxStyleStore},
    HwpxEncoder,
};

pub fn unit(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}
pub fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}
pub fn run(c: Control) -> Run {
    Run::control(c, CharShapeIndex::new(0))
}
pub fn text(s: &str) -> Paragraph {
    para(vec![Run::text(s, CharShapeIndex::new(0))])
}
pub fn placement(x: i32, y: i32) -> ObjectPlacement {
    ObjectPlacement::legacy_inline_defaults().with_horz_offset(unit(x)).with_vert_offset(unit(y))
}
pub fn frame(w: i32, h: i32) -> ObjectFrame {
    ObjectFrame::new().with_original_size(Size::new(unit(w), unit(h)))
}
pub fn rect(w: i32, h: i32, rotation: Option<f32>, flip: Option<Flip>) -> Control {
    let mut c = Control::rect(unit(w), unit(h)).unwrap();
    if let Control::Rect { style, .. } = &mut c {
        *style = Some(ShapeStyle { rotation, flip, ..Default::default() });
    }
    c
}
pub fn art(w: i32, h: i32, color: Color) -> Control {
    Control::TextArt {
        frame: frame(200, 100),
        text: "A&B<글>".into(),
        shape: "WAVE2".into(),
        font_name: "함초롬바탕".into(),
        font_style: "보통".into(),
        align: "LEFT".into(),
        line_spacing: 120,
        char_spacing: 100,
        width: unit(w),
        height: unit(h),
        placement: None,
        fill_color: Some(color),
        inst_id: None,
    }
}
pub fn group(children: Vec<Control>, w: i32, h: i32) -> Control {
    Control::Group {
        frame: ObjectFrame::new(),
        children: children.into_iter().map(GroupChild::Shape).collect(),
        width: unit(w),
        height: unit(h),
        placement: None,
        inst_id: None,
    }
}
pub fn picture(rotation: Option<f32>, flip: Option<Flip>, w: i32, h: i32) -> Run {
    let mut image = Image::new("wire.png", unit(w), unit(h), ImageFormat::Png);
    image.rotation = rotation;
    image.flip = flip;
    image.frame = frame(w, h);
    Run::image(image, CharShapeIndex::new(0))
}
pub fn encode(runs: Vec<Run>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    for spelling in ["HORIZONTAL", "VERTICAL", "SLASH", "BACK_SLASH", "CROSS", "CROSS_DIAGONAL"] {
        let mut border = HwpxBorderFill::default_page_border();
        border.set_win_brush_fill("#123456", "#ABCDEF", "0", Some(spelling.into()));
        styles.push_border_fill(border);
    }
    let mut gradient = HwpxBorderFill::default_page_border();
    gradient.set_gradient_fill(hwpforge_smithy_hwpx::style_store::HwpxGradientFill {
        gradient_type: GradientType::Linear,
        angle: 17,
        center_x: 0,
        center_y: 0,
        step: 255,
        step_center: 50,
        alpha: 0,
        colors: vec![Color::BLACK, Color::WHITE],
    });
    styles.push_border_fill(gradient);
    let mut image = HwpxBorderFill::default_page_border();
    image.set_image_fill(hwpforge_smithy_hwpx::style_store::HwpxImageFill {
        mode: "TILE".into(),
        binary_item_id_ref: "wire".into(),
        bright: 0,
        contrast: 0,
        effect: "REAL_PIC".into(),
        alpha: 0,
    });
    styles.push_border_fill(image);
    let mut images = ImageStore::new();
    images.insert(
        "wire.png",
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 176, 89,
            16, 0, 0, 2, 72, 1, 45, 108, 176, 0, 156, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ],
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    HwpxEncoder::encode(&doc.validate().expect("valid document"), &styles, &images).expect("encode")
}
pub fn drawings() -> Vec<Control> {
    let points = vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(200, 0),
        ShapePoint::new(200, 100),
        ShapePoint::new(0, 100),
    ];
    vec![
        rect(200, 100, None, None),
        Control::TextBox {
            frame: ObjectFrame::new(),
            paragraphs: vec![text("box")],
            width: unit(200),
            height: unit(100),
            placement: None,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
            text_margin: None,
        },
        Control::ellipse(unit(200), unit(100)),
        Control::arc(ArcType::Normal, unit(200), unit(100)),
        Control::polygon(points.clone()).unwrap(),
        Control::curve(points).unwrap(),
    ]
}
pub fn set_fill(c: &mut Control, fill: Option<Fill>) {
    match c {
        Control::Rect { style, .. }
        | Control::TextBox { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Arc { style, .. }
        | Control::Polygon { style, .. }
        | Control::Curve { style, .. } => *style = Some(ShapeStyle { fill, ..Default::default() }),
        _ => unreachable!(),
    }
}
pub fn fills() -> Vec<Option<Fill>> {
    vec![
        None,
        Some(Fill::Solid { color: Color::from_rgb(18, 52, 86) }),
        Some(Fill::Gradient {
            gradient_type: GradientType::Linear,
            angle: 17,
            colors: vec![(Color::BLACK, 0), (Color::WHITE, 100)],
            center_x: None,
            center_y: None,
            step: None,
            step_center: None,
        }),
        Some(Fill::Image { image_id: "wire.png".into(), mode: ImageFillMode::Tile }),
        Some(Fill::None),
    ]
}
pub fn patterns() -> Vec<(PatternType, &'static str)> {
    vec![
        (PatternType::Horizontal, "HORIZONTAL"),
        (PatternType::Vertical, "VERTICAL"),
        (PatternType::BackSlash, "SLASH"),
        (PatternType::Slash, "BACK_SLASH"),
        (PatternType::Cross, "CROSS"),
        (PatternType::CrossDiagonal, "CROSS_DIAGONAL"),
    ]
}
pub fn documents() -> Vec<(&'static str, Vec<u8>)> {
    let arts = encode(vec![
        run(art(200, 100, Color::from_rgb(18, 52, 86))),
        run(art(400, 300, Color::from_rgb(171, 205, 239))),
    ]);
    let mut a = art(200, 100, Color::BLACK);
    if let Control::TextArt { placement: p, .. } = &mut a {
        *p = Some(placement(1200, 800));
    }
    let mut r = rect(200, 100, None, None);
    if let Control::Rect { placement: p, .. } = &mut r {
        *p = Some(placement(1200, 800));
    }
    let simple = group(vec![a.clone(), r.clone()], 4000, 2000);
    let mut inner = group(vec![a, r], 4000, 2000);
    if let Control::Group { placement: p, .. } = &mut inner {
        *p = Some(placement(500, 300));
    }
    let nested = group(vec![inner], 5000, 3000);
    let mut child = rect(600, 300, None, None);
    if let Control::Rect { frame: f, placement: p, .. } = &mut child {
        *f = frame(100, 50);
        *p = Some(placement(100, 40));
    }
    let mut scaled = group(vec![child], 700, 340);
    if let Control::Group { frame: f, .. } = &mut scaled {
        *f = frame(350, 170);
    }
    let groups = encode(vec![run(simple), run(nested), run(scaled)]);
    let mut notes = Vec::new();
    for id in [Some(42), Some(4294967297), None, Some(0), Some(u64::MAX)] {
        notes.push(run(Control::Footnote {
            inst_id: id.map(hwpforge_core::ObjectId::new),
            paragraphs: vec![text("foot"), text("tail")],
        }));
        notes.push(run(Control::Endnote {
            inst_id: id.map(hwpforge_core::ObjectId::new),
            paragraphs: vec![text("end"), text("tail")],
        }));
    }
    let cell = TableCell::new(
        vec![para(vec![
            run(Control::footnote_with_id(77, vec![text("cell foot")])),
            run(Control::endnote_with_id(88, vec![text("cell end")])),
        ])],
        unit(2000),
    );
    notes.push(Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0)));
    let notes = encode(notes);
    let mut mirrors = Vec::new();
    for (w, h, angle, flip) in [
        (101, 50, 0., Flip::Horizontal),
        (100, 51, 0., Flip::Vertical),
        (101, 51, 0., Flip::Both),
        (101, 51, 30., Flip::Horizontal),
        (101, 51, 90., Flip::Horizontal),
    ] {
        mirrors.push(run(rect(w, h, Some(angle), Some(flip))));
        mirrors.push(picture(Some(angle), Some(flip), w, h));
    }
    mirrors.push(run(group(vec![rect(101, 50, Some(0.), Some(Flip::Horizontal))], 101, 50)));
    let mirrors = encode(mirrors);
    let group_mirrors = encode(
        [(100, 51, 0., Flip::Vertical), (101, 51, 0.3, Flip::Both)]
            .into_iter()
            .map(|(w, h, angle, flip)| run(group(vec![rect(w, h, Some(angle), Some(flip))], w, h)))
            .collect(),
    );
    let mut all = Vec::new();
    for fill in fills() {
        for mut c in drawings() {
            set_fill(&mut c, fill.clone());
            all.push(run(c));
        }
    }
    for (pattern_type, _) in patterns() {
        let mut c = rect(200, 100, None, None);
        set_fill(
            &mut c,
            Some(Fill::Pattern {
                pattern_type,
                fg_color: Color::from_rgb(171, 205, 239),
                bg_color: Color::from_rgb(18, 52, 86),
            }),
        );
        all.push(run(c));
    }
    all.push(run(Control::line(ShapePoint::new(0, 0), ShapePoint::new(200, 100)).unwrap()));
    all.push(run(Control::connect_line(ShapePoint::new(0, 0), ShapePoint::new(200, 100)).unwrap()));
    all.push(picture(None, None, 200, 100));
    all.push(run(hwpforge_core::Control::chart(
        hwpforge_core::chart::ChartType::Column,
        hwpforge_core::chart::ChartData::category(&["A"], &[("B", &[1.])]),
    )));
    all.push(run(Control::EmbeddedChart {
        chart_xml:
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#
                .into(),
        ole_bytes: vec![0; 16],
        width: unit(200),
        height: unit(100),
        placement: None,
    }));
    all.push(run(group(drawings(), 200, 100)));
    all.push(run(Control::Group {
        frame: ObjectFrame::new(),
        children: vec![
            GroupChild::Picture(Image::new("wire.png", unit(200), unit(100), ImageFormat::Png)),
            GroupChild::Shape(
                Control::line(ShapePoint::new(0, 0), ShapePoint::new(200, 100)).unwrap(),
            ),
            GroupChild::Shape(
                Control::connect_line(ShapePoint::new(0, 0), ShapePoint::new(200, 100)).unwrap(),
            ),
        ],
        width: unit(200),
        height: unit(100),
        placement: None,
        inst_id: None,
    }));
    let shapes = encode(all);
    vec![
        ("textart", arts),
        ("groups", groups),
        ("notes", notes),
        ("mirrors", mirrors),
        ("group-mirrors", group_mirrors),
        ("shapes", shapes),
    ]
}
fn main() {
    std::fs::create_dir_all("temp").unwrap();
    for (name, bytes) in documents() {
        std::fs::write(format!("temp/wire-{name}.hwpx"), bytes).unwrap();
    }
}
