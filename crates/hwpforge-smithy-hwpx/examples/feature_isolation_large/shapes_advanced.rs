use hwpforge_core::control::{ArrowStyle, Control, Fill, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    ApplyPageType, ArcType, ArrowSize, ArrowType, Color, CurveSegmentType, Flip, GradientType,
    HwpUnit, NumberFormatType, PageNumberPosition, PatternType,
};

use crate::{
    csi, empty, encode_and_save, mascot_intro, p, psi, showcase_store, CS_BOLD, CS_NORMAL,
    CS_SMALL, CS_WHITE, PS_CENTER, PS_LEFT, PS_RIGHT,
};

#[derive(Clone, Copy)]
struct ShapeSizes {
    width_8k: HwpUnit,
    width_10k: HwpUnit,
    width_14k: HwpUnit,
    height_1k: HwpUnit,
    height_6k: HwpUnit,
    height_8k: HwpUnit,
}

impl ShapeSizes {
    fn new() -> Self {
        Self {
            width_8k: HwpUnit::new(8000).unwrap(),
            width_10k: HwpUnit::new(10000).unwrap(),
            width_14k: HwpUnit::new(14000).unwrap(),
            height_1k: HwpUnit::new(1000).unwrap(),
            height_6k: HwpUnit::new(6000).unwrap(),
            height_8k: HwpUnit::new(8000).unwrap(),
        }
    }
}

fn shape_paragraph(control: Control) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(control, csi(CS_NORMAL))], psi(PS_LEFT))
}

fn push_shape_section(paras: &mut Vec<Paragraph>, title: &str, description: &str) {
    paras.push(p(title, CS_BOLD, PS_LEFT));
    paras.push(p(description, CS_SMALL, PS_LEFT));
    paras.push(empty());
}

fn push_shape_example(paras: &mut Vec<Paragraph>, label: &str, control: Control) {
    paras.push(p(label, CS_BOLD, PS_LEFT));
    paras.push(shape_paragraph(control));
    paras.push(empty());
}

fn push_page_break(paras: &mut Vec<Paragraph>) {
    paras.push(empty().with_page_break());
}

fn append_arc_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 1. Arc (호) 도형 ━━━",
        "Arc는 타원 위의 호를 그리는 도형입니다. ArcType에 따라 \
         Normal(열린 호), Pie(부채꼴 — 중심까지 선), Chord(활꼴 — 양 끝점 연결) \
         세 가지 형태로 렌더링됩니다.",
    );

    for (arc_type, label, line_color, fill_color) in [
        (
            ArcType::Normal,
            "▸ Normal — 열린 호 (빨간 선, 두께 50)",
            Color::from_rgb(200, 0, 0),
            None,
        ),
        (
            ArcType::Pie,
            "▸ Pie — 부채꼴 (파란 채움, 중심에서 호 양쪽 끝까지 직선 연결)",
            Color::from_rgb(0, 0, 200),
            Some(Color::from_rgb(200, 220, 255)),
        ),
        (
            ArcType::Chord,
            "▸ Chord — 활꼴 (녹색 채움, 호 양쪽 끝점을 직선 연결)",
            Color::from_rgb(0, 150, 0),
            Some(Color::from_rgb(220, 255, 220)),
        ),
    ] {
        push_shape_example(
            paras,
            label,
            Control::Arc {
                arc_type,
                center: ShapePoint::new(4000, 4000),
                axis1: ShapePoint::new(8000, 4000),
                axis2: ShapePoint::new(4000, 8000),
                start1: ShapePoint::new(8000, 4000),
                end1: ShapePoint::new(4000, 0),
                start2: ShapePoint::new(4000, 8000),
                end2: ShapePoint::new(0, 4000),
                width: sizes.width_8k,
                height: sizes.height_8k,
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(line_color),
                    fill_color,
                    line_width: Some(if matches!(arc_type, ArcType::Normal) { 50 } else { 30 }),
                    ..Default::default()
                }),
            },
        );
    }

    push_page_break(paras);
}

fn append_curve_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 2. Curve & ConnectLine ━━━",
        "Curve는 베지어 곡선으로 부드러운 곡선을 표현합니다. \
         CurveSegmentType::Curve(곡선)와 Line(직선)을 혼합할 수 있습니다. \
         ConnectLine은 두 도형을 연결하는 선입니다.",
    );

    push_shape_example(
        paras,
        "▸ 베지어 S-곡선 (보라색, 두께 50)",
        Control::Curve {
            points: vec![
                ShapePoint::new(0, 4000),
                ShapePoint::new(2000, 0),
                ShapePoint::new(6000, 8000),
                ShapePoint::new(8000, 4000),
            ],
            segment_types: vec![
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
            ],
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(128, 0, 128)),
                line_width: Some(50),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ 파동 곡선 (6개 제어점, 청록색, 두께 40)",
        Control::Curve {
            points: vec![
                ShapePoint::new(0, 4000),
                ShapePoint::new(1600, 0),
                ShapePoint::new(3200, 8000),
                ShapePoint::new(4800, 0),
                ShapePoint::new(6400, 8000),
                ShapePoint::new(8000, 4000),
            ],
            segment_types: vec![
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
                CurveSegmentType::Curve,
            ],
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 150, 150)),
                line_width: Some(40),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ 혼합 세그먼트 (Line → Curve → Line, 주황색)",
        Control::Curve {
            points: vec![
                ShapePoint::new(0, 6000),
                ShapePoint::new(3000, 6000),
                ShapePoint::new(5000, 0),
                ShapePoint::new(8000, 6000),
            ],
            segment_types: vec![
                CurveSegmentType::Line,
                CurveSegmentType::Curve,
                CurveSegmentType::Line,
            ],
            width: sizes.width_8k,
            height: sizes.height_6k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(230, 120, 0)),
                line_width: Some(40),
                line_style: Some(LineStyle::Dash),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ ConnectLine (STRAIGHT, 양방향 화살표)",
        Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(10000, 4000),
            control_points: vec![ShapePoint::new(5000, 0), ShapePoint::new(5000, 4000)],
            connect_type: "STRAIGHT".to_string(),
            width: sizes.width_10k,
            height: HwpUnit::new(4000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 100, 200)),
                line_width: Some(30),
                head_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Diamond,
                    size: ArrowSize::Medium,
                    filled: true,
                }),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Normal,
                    size: ArrowSize::Medium,
                    filled: true,
                }),
                ..Default::default()
            }),
        },
    );

    push_page_break(paras);
}

fn append_line_style_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 3. 선 스타일 (LineStyle) ━━━",
        "LineStyle은 선의 패턴을 결정합니다. \
         Solid(실선), Dash(파선), Dot(점선), DashDot(일점쇄선), \
         DashDotDot(이점쇄선) 5가지가 있습니다.",
    );

    let line_styles: [(LineStyle, &str, (u8, u8, u8)); 5] = [
        (LineStyle::Solid, "Solid — 실선 (기본값)", (0, 0, 0)),
        (LineStyle::Dash, "Dash — 파선", (200, 0, 0)),
        (LineStyle::Dot, "Dot — 점선", (0, 0, 200)),
        (LineStyle::DashDot, "DashDot — 일점쇄선", (0, 150, 0)),
        (LineStyle::DashDotDot, "DashDotDot — 이점쇄선", (200, 100, 0)),
    ];
    for (line_style, label, (red, green, blue)) in line_styles {
        push_shape_example(
            paras,
            &format!("▸ {label}"),
            Control::Line {
                start: ShapePoint::new(0, 500),
                end: ShapePoint::new(14000, 500),
                width: sizes.width_14k,
                height: sizes.height_1k,
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(red, green, blue)),
                    line_width: Some(50),
                    line_style: Some(line_style),
                    ..Default::default()
                }),
            },
        );
    }

    paras.push(p("▸ 선 두께 비교: 20 / 50 / 100 / 200 (HWPUNIT)", CS_BOLD, PS_LEFT));
    let widths: [(u32, &str); 4] = [(20, "20"), (50, "50"), (100, "100"), (200, "200")];
    for (line_width, label) in widths {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(shape_paragraph(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: sizes.width_14k,
            height: sizes.height_1k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(50, 50, 50)),
                line_width: Some(line_width),
                ..Default::default()
            }),
        }));
    }

    push_page_break(paras);
}

fn append_arrow_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 4. 화살표 스타일 (ArrowType) ━━━",
        "ArrowType은 6종: Normal(삼각형), Arrow(화살촉), Concave(오목), \
         Diamond(마름모), Oval(원형), Open(열린 삼각형). \
         각각 Small/Medium/Large 3가지 크기와 filled(채움)/unfilled(비움) 지정 가능. \
         head_arrow(시작점)와 tail_arrow(끝점)에 독립 설정됩니다.",
    );

    let arrow_types: [(ArrowType, &str, (u8, u8, u8)); 6] = [
        (ArrowType::Normal, "Normal — 삼각형 (filled)", (0, 0, 0)),
        (ArrowType::Arrow, "Arrow — 화살촉 (filled)", (200, 0, 0)),
        (ArrowType::Concave, "Concave — 오목 화살표 (filled)", (0, 0, 200)),
        (ArrowType::Diamond, "Diamond — 마름모 (filled)", (0, 150, 0)),
        (ArrowType::Oval, "Oval — 원형 (filled)", (150, 0, 150)),
        (ArrowType::Open, "Open — 열린 삼각형 (unfilled)", (200, 100, 0)),
    ];
    for (arrow_type, label, (red, green, blue)) in arrow_types {
        push_shape_example(
            paras,
            &format!("▸ {label}"),
            Control::Line {
                start: ShapePoint::new(0, 500),
                end: ShapePoint::new(14000, 500),
                width: sizes.width_14k,
                height: sizes.height_1k,
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(red, green, blue)),
                    line_width: Some(50),
                    tail_arrow: Some(ArrowStyle {
                        arrow_type,
                        size: ArrowSize::Medium,
                        filled: arrow_type != ArrowType::Open,
                    }),
                    ..Default::default()
                }),
            },
        );
    }

    paras.push(p("▸ 크기 비교: Small / Medium / Large", CS_BOLD, PS_LEFT));
    let arrow_sizes: [(ArrowSize, &str); 3] =
        [(ArrowSize::Small, "Small"), (ArrowSize::Medium, "Medium"), (ArrowSize::Large, "Large")];
    for (arrow_size, label) in arrow_sizes {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(shape_paragraph(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: sizes.width_14k,
            height: sizes.height_1k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 0, 0)),
                line_width: Some(50),
                head_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Diamond,
                    size: arrow_size,
                    filled: false,
                }),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Normal,
                    size: arrow_size,
                    filled: true,
                }),
                ..Default::default()
            }),
        }));
    }

    push_shape_example(
        paras,
        "▸ 양방향 화살표 (head: Diamond 비움, tail: Arrow 채움)",
        Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: sizes.width_14k,
            height: sizes.height_1k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 80, 160)),
                line_width: Some(60),
                line_style: Some(LineStyle::Dash),
                head_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Diamond,
                    size: ArrowSize::Large,
                    filled: false,
                }),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Arrow,
                    size: ArrowSize::Large,
                    filled: true,
                }),
                ..Default::default()
            }),
        },
    );

    push_page_break(paras);
}

fn append_rotation_examples(paras: &mut Vec<Paragraph>) {
    push_shape_section(
        paras,
        "━━━ 5. 회전 (Rotation) ━━━",
        "ShapeStyle.rotation으로 도형을 시계 방향으로 회전합니다. \
         0°~360° 범위의 실수값을 지원하며, 도형의 중심점을 기준으로 회전합니다.",
    );

    let rotation_vertices: Vec<ShapePoint> = vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(4000, 0),
        ShapePoint::new(4000, 6000),
        ShapePoint::new(6000, 6000),
        ShapePoint::new(6000, 8000),
        ShapePoint::new(0, 8000),
        ShapePoint::new(0, 0),
    ];
    let rotation_samples: [(f32, (u8, u8, u8)); 4] =
        [(0.0, (100, 100, 100)), (45.0, (200, 0, 0)), (90.0, (0, 0, 200)), (135.0, (0, 150, 0))];
    for (angle, (red, green, blue)) in rotation_samples {
        push_shape_example(
            paras,
            &format!("▸ 다각형 회전 {angle:.0}°"),
            Control::Polygon {
                vertices: rotation_vertices.clone(),
                width: HwpUnit::new(6000).unwrap(),
                height: HwpUnit::new(8000).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                paragraphs: vec![empty()],
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(red, green, blue)),
                    fill_color: Some(Color::from_rgb(
                        200_u8.saturating_add(red / 5),
                        200_u8.saturating_add(green / 5),
                        200_u8.saturating_add(blue / 5),
                    )),
                    rotation: Some(angle),
                    ..Default::default()
                }),
            },
        );
    }

    push_page_break(paras);
}

fn append_flip_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 6. 반전 (Flip) ━━━",
        "Flip은 도형을 거울처럼 반전합니다. Horizontal(좌우), \
         Vertical(상하), Both(좌우+상하 동시) 3가지 모드가 있습니다. \
         비대칭 화살표 모양 다각형으로 반전 효과를 확인합니다. \
         인코더는 flip 속성과 함께 rotMatrix에 반전값을 반영합니다.",
    );

    let flip_vertices: Vec<ShapePoint> = vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(6000, 0),
        ShapePoint::new(8000, 2500),
        ShapePoint::new(6000, 5000),
        ShapePoint::new(1000, 5000),
        ShapePoint::new(1000, 8000),
        ShapePoint::new(0, 8000),
        ShapePoint::new(0, 0),
    ];
    let flip_samples: [(Flip, &str, (u8, u8, u8)); 4] = [
        (Flip::None, "None — 원본 (반전 없음)", (100, 100, 100)),
        (Flip::Horizontal, "Horizontal — 좌우 반전", (0, 0, 200)),
        (Flip::Vertical, "Vertical — 상하 반전", (200, 0, 0)),
        (Flip::Both, "Both — 좌우+상하 반전 (180° 회전과 동일)", (0, 150, 0)),
    ];
    for (flip, label, (red, green, blue)) in flip_samples {
        push_shape_example(
            paras,
            &format!("▸ {label}"),
            Control::Polygon {
                vertices: flip_vertices.clone(),
                width: sizes.width_8k,
                height: sizes.width_8k,
                horz_offset: 1,
                vert_offset: 0,
                paragraphs: vec![empty()],
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(red, green, blue)),
                    fill_color: Some(Color::from_rgb(
                        200_u8.saturating_add(red / 5),
                        200_u8.saturating_add(green / 5),
                        200_u8.saturating_add(blue / 5),
                    )),
                    flip: Some(flip),
                    ..Default::default()
                }),
            },
        );
    }

    push_page_break(paras);
}

fn append_gradient_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 7. 그라데이션 채우기 (Gradient Fill) ━━━",
        "Fill::Gradient로 그라데이션을 적용합니다. \
         GradientType: Linear(직선형), Radial(방사형), Square(사각형), \
         Conical(원뿔형). angle로 방향을, colors로 색상 정지점을 지정합니다.",
    );

    push_shape_example(
        paras,
        "▸ Linear 90° — 좌→우 (빨강→파랑)",
        Control::Ellipse {
            center: ShapePoint::new(4000, 4000),
            axis1: ShapePoint::new(8000, 4000),
            axis2: ShapePoint::new(4000, 8000),
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("좌→우", CS_WHITE, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Linear,
                    angle: 90,
                    colors: vec![
                        (Color::from_rgb(255, 0, 0), 0),
                        (Color::from_rgb(0, 0, 255), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Linear 0° — 위→아래 (노랑→초록)",
        Control::Ellipse {
            center: ShapePoint::new(4000, 4000),
            axis1: ShapePoint::new(8000, 4000),
            axis2: ShapePoint::new(4000, 8000),
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("위→아래", CS_BOLD, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Linear,
                    angle: 0,
                    colors: vec![
                        (Color::from_rgb(255, 255, 0), 0),
                        (Color::from_rgb(0, 128, 0), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Linear 45° — 대각선 (빨→파, 사각형)",
        Control::Polygon {
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(8000, 0),
                ShapePoint::new(8000, 8000),
                ShapePoint::new(0, 8000),
                ShapePoint::new(0, 0),
            ],
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("대각선 45°", CS_WHITE, PS_RIGHT)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Linear,
                    angle: 45,
                    colors: vec![
                        (Color::from_rgb(255, 0, 0), 0),
                        (Color::from_rgb(0, 0, 255), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Radial — 방사형 (중심에서 바깥으로, 흰→보라)",
        Control::Ellipse {
            center: ShapePoint::new(4000, 4000),
            axis1: ShapePoint::new(8000, 4000),
            axis2: ShapePoint::new(4000, 8000),
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("방사형", CS_BOLD, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Radial,
                    angle: 0,
                    colors: vec![
                        (Color::from_rgb(255, 255, 255), 0),
                        (Color::from_rgb(128, 0, 128), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Square — 사각형 그라데이션 (중심→모서리, 흰→남색)",
        Control::Polygon {
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(8000, 0),
                ShapePoint::new(8000, 8000),
                ShapePoint::new(0, 8000),
                ShapePoint::new(0, 0),
            ],
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("사각형", CS_WHITE, PS_LEFT)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Square,
                    angle: 0,
                    colors: vec![
                        (Color::from_rgb(255, 255, 255), 0),
                        (Color::from_rgb(0, 0, 128), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Conical — 원뿔형 (빨→파, 2색)",
        Control::Ellipse {
            center: ShapePoint::new(4000, 4000),
            axis1: ShapePoint::new(8000, 4000),
            axis2: ShapePoint::new(4000, 8000),
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("원뿔형", CS_WHITE, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Conical,
                    angle: 0,
                    colors: vec![
                        (Color::from_rgb(255, 0, 0), 0),
                        (Color::from_rgb(0, 0, 255), 100),
                    ],
                }),
                ..Default::default()
            }),
        },
    );

    push_page_break(paras);
}

fn append_pattern_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 8. 패턴 채우기 (Pattern Fill) ━━━",
        "Fill::Pattern으로 해칭 패턴을 적용합니다. 전경색(fg_color)과 \
         배경색(bg_color)을 지정하며, 6가지 PatternType이 있습니다: \
         Horizontal(수평선), Vertical(수직선), BackSlash(역사선), \
         Slash(사선), Cross(십자), CrossDiagonal(X자).",
    );

    let patterns: [(PatternType, &str, (u8, u8, u8), (u8, u8, u8)); 6] = [
        (PatternType::Horizontal, "Horizontal — 수평선", (0, 0, 200), (230, 230, 255)),
        (PatternType::Vertical, "Vertical — 수직선", (200, 0, 0), (255, 230, 230)),
        (PatternType::BackSlash, "BackSlash — 역사선 (\\)", (0, 150, 0), (230, 255, 230)),
        (PatternType::Slash, "Slash — 사선 (/)", (150, 0, 150), (255, 230, 255)),
        (PatternType::Cross, "Cross — 십자 (+)", (0, 0, 0), (240, 240, 240)),
        (PatternType::CrossDiagonal, "CrossDiagonal — X자 (×)", (128, 64, 0), (255, 245, 230)),
    ];
    for (pattern_type, label, (fg_red, fg_green, fg_blue), (bg_red, bg_green, bg_blue)) in patterns
    {
        push_shape_example(
            paras,
            &format!("▸ {label}"),
            Control::Polygon {
                vertices: vec![
                    ShapePoint::new(4000, 0),
                    ShapePoint::new(8000, 4000),
                    ShapePoint::new(4000, 8000),
                    ShapePoint::new(0, 4000),
                    ShapePoint::new(4000, 0),
                ],
                width: sizes.width_8k,
                height: sizes.height_8k,
                horz_offset: 0,
                vert_offset: 0,
                paragraphs: vec![empty()],
                caption: None,
                style: Some(ShapeStyle {
                    fill: Some(Fill::Pattern {
                        pattern_type,
                        fg_color: Color::from_rgb(fg_red, fg_green, fg_blue),
                        bg_color: Color::from_rgb(bg_red, bg_green, bg_blue),
                    }),
                    ..Default::default()
                }),
            },
        );
    }

    push_page_break(paras);
}

fn append_composite_examples(paras: &mut Vec<Paragraph>, sizes: ShapeSizes) {
    push_shape_section(
        paras,
        "━━━ 9. 복합 스타일 조합 ━━━",
        "여러 스타일 옵션을 동시에 적용한 복합 도형입니다. \
         Fill::Solid, 그라데이션+회전, 패턴+반전, 파선+화살표 등의 조합을 시연합니다.",
    );

    push_shape_example(
        paras,
        "▸ Fill::Solid — 단색 채우기 (주황색 타원)",
        Control::Ellipse {
            center: ShapePoint::new(4000, 4000),
            axis1: ShapePoint::new(8000, 4000),
            axis2: ShapePoint::new(4000, 8000),
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("Solid Fill", CS_NORMAL, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Solid { color: Color::from_rgb(255, 165, 0) }),
                line_color: Some(Color::from_rgb(200, 100, 0)),
                line_width: Some(40),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ 그라데이션 + 회전 60° (Linear, 보라→청록 + 60° 회전)",
        Control::Ellipse {
            center: ShapePoint::new(5000, 3000),
            axis1: ShapePoint::new(10000, 3000),
            axis2: ShapePoint::new(5000, 6000),
            width: sizes.width_10k,
            height: sizes.height_6k,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![p("Gradient+Rotation", CS_NORMAL, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Linear,
                    angle: 135,
                    colors: vec![
                        (Color::from_rgb(128, 0, 255), 0),
                        (Color::from_rgb(0, 200, 200), 100),
                    ],
                }),
                rotation: Some(60.0),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ 패턴(CrossDiagonal) + 수직 반전 (오각형)",
        Control::Polygon {
            vertices: vec![
                ShapePoint::new(4000, 0),
                ShapePoint::new(8000, 3000),
                ShapePoint::new(7000, 8000),
                ShapePoint::new(1000, 8000),
                ShapePoint::new(0, 3000),
                ShapePoint::new(4000, 0),
            ],
            width: sizes.width_8k,
            height: sizes.height_8k,
            horz_offset: 100,
            vert_offset: 100,
            paragraphs: vec![p("Pattern+Flip", CS_NORMAL, PS_CENTER)],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Pattern {
                    pattern_type: PatternType::CrossDiagonal,
                    fg_color: Color::from_rgb(200, 0, 0),
                    bg_color: Color::from_rgb(255, 240, 240),
                }),
                flip: Some(Flip::Vertical),
                line_width: Some(40),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ 파선(DashDot) + 양방향 화살표 (head: Oval, tail: Concave)",
        Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: sizes.width_14k,
            height: sizes.height_1k,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 100, 0)),
                line_width: Some(60),
                line_style: Some(LineStyle::DashDot),
                head_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Oval,
                    size: ArrowSize::Large,
                    filled: true,
                }),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Concave,
                    size: ArrowSize::Large,
                    filled: true,
                }),
                ..Default::default()
            }),
        },
    );

    push_shape_example(
        paras,
        "▸ Control::horizontal_line() — 편의 생성자 (기본 수평선)",
        Control::horizontal_line(sizes.width_14k),
    );

    push_shape_example(
        paras,
        "▸ Radial 그라데이션 + 파선 테두리 + 두꺼운 선 (별 모양 다각형)",
        Control::Polygon {
            vertices: vec![
                ShapePoint::new(5000, 0),
                ShapePoint::new(6200, 3500),
                ShapePoint::new(10000, 3500),
                ShapePoint::new(7000, 5800),
                ShapePoint::new(8100, 9500),
                ShapePoint::new(5000, 7200),
                ShapePoint::new(1900, 9500),
                ShapePoint::new(3000, 5800),
                ShapePoint::new(0, 3500),
                ShapePoint::new(3800, 3500),
                ShapePoint::new(5000, 0),
            ],
            width: sizes.width_10k,
            height: HwpUnit::new(9500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![empty()],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Gradient {
                    gradient_type: GradientType::Radial,
                    angle: 0,
                    colors: vec![
                        (Color::from_rgb(255, 255, 0), 0),
                        (Color::from_rgb(255, 100, 0), 100),
                    ],
                }),
                line_color: Some(Color::from_rgb(200, 0, 0)),
                line_width: Some(60),
                line_style: Some(LineStyle::Dash),
                ..Default::default()
            }),
        },
    );
}

pub(crate) fn gen_15_shapes_advanced() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "15. 고급 도형 종합 레퍼런스",
        "Arc(3종), Curve, ConnectLine, 선 스타일(5종), 화살표(6종), \
         회전(4단계), 반전(3종), 그라데이션(4종), 패턴(6종) 등 \
         HwpForge가 지원하는 모든 도형 옵션을 종합 시연합니다.",
    );
    let sizes: ShapeSizes = ShapeSizes::new();

    append_arc_examples(&mut paras, sizes);
    append_curve_examples(&mut paras, sizes);
    append_line_style_examples(&mut paras, sizes);
    append_arrow_examples(&mut paras, sizes);
    append_rotation_examples(&mut paras);
    append_flip_examples(&mut paras, sizes);
    append_gradient_examples(&mut paras, sizes);
    append_pattern_examples(&mut paras, sizes);
    append_composite_examples(&mut paras, sizes);

    let mut section = Section::with_paragraphs(paras, PageSettings::a4());
    section.header = Some(HeaderFooter::new(
        vec![p("15. 고급 도형 종합 레퍼런스 — HwpForge", CS_SMALL, PS_LEFT)],
        ApplyPageType::Both,
    ));
    section.footer = Some(HeaderFooter::new(
        vec![p("생성일: 2026-03-08", CS_SMALL, PS_RIGHT)],
        ApplyPageType::Both,
    ));
    section.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    let mut doc = Document::new();
    doc.add_section(section);
    encode_and_save("15_shapes_advanced.hwpx", &store, &doc, &images);
}
