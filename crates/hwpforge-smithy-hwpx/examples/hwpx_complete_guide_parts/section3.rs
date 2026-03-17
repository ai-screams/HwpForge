use super::shared::{
    ctrl_p, empty, p, CS_HEADING, CS_NORMAL, CS_SMALL, CS_TITLE, PS_BODY, PS_CENTER, PS_LEFT,
};
use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{ArrowStyle, Control, Fill, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    ArcType, ArrowSize, ArrowType, Color, CurveSegmentType, Flip, GradientType, GutterType,
    HwpUnit, PatternType,
};

pub(crate) fn section3_shapes_and_graphics() -> Section {
    // 가로(landscape) 페이지: landscape: true, width/height는 세로 기준 유지
    let landscape = PageSettings {
        landscape: true,
        gutter: HwpUnit::from_mm(10.0).unwrap(),
        gutter_type: GutterType::LeftOnly,
        ..PageSettings::a4()
    };

    let mut paras: Vec<Paragraph> = vec![
        p("도형과 그래픽 요소", CS_TITLE, PS_CENTER),
        empty(),
        p(
            "이 섹션은 가로(landscape) 방향이며, Gutter 10mm가 적용되어 있습니다. HWPX의 다양한 도형 요소를 시연합니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
        // ── 3.1 선(Line) ──
        p("3.1 선 (Line)", CS_HEADING, PS_LEFT),
        empty(),
        // 선 1: 기본 실선
        p("실선 (기본):", CS_NORMAL, PS_LEFT),
    ];
    let line1 = Control::line(ShapePoint::new(0, 0), ShapePoint::new(15000, 0)).unwrap();
    paras.push(ctrl_p(line1, CS_NORMAL, PS_LEFT));

    // 선 2: 점선 + 화살표
    paras.push(p("점선 + 화살표:", CS_NORMAL, PS_LEFT));
    let mut line2 = Control::line(ShapePoint::new(0, 0), ShapePoint::new(15000, 0)).unwrap();
    if let Control::Line { ref mut style, .. } = line2 {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 100, 200)),
            line_width: Some(25),
            line_style: Some(LineStyle::Dot),
            head_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Arrow,
                size: ArrowSize::Medium,
                filled: true,
            }),
            tail_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Normal,
                size: ArrowSize::Large,
                filled: true,
            }),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(line2, CS_NORMAL, PS_LEFT));

    // 선 3: 빨간 쇄선(DashDot)
    paras.push(p("쇄선(DashDot) 빨강:", CS_NORMAL, PS_LEFT));
    let mut line3 = Control::line(ShapePoint::new(0, 0), ShapePoint::new(15000, 0)).unwrap();
    if let Control::Line { ref mut style, .. } = line3 {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(200, 30, 30)),
            line_width: Some(30),
            line_style: Some(LineStyle::DashDot),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(line3, CS_NORMAL, PS_LEFT));
    paras.push(empty());

    // ── 3.2 타원(Ellipse) ──
    paras.push(p("3.2 타원 (Ellipse)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 타원 + 텍스트 내부 + 솔리드 채우기
    let w = HwpUnit::from_mm(50.0).unwrap();
    let h = HwpUnit::from_mm(30.0).unwrap();
    let mut ell =
        Control::ellipse_with_text(w, h, vec![p("타원 내부 텍스트", CS_SMALL, PS_CENTER)]);
    if let Control::Ellipse { ref mut style, ref mut caption, .. } = ell {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(30, 100, 200)),
            line_width: Some(30),
            fill: Some(Fill::Solid { color: Color::from_rgb(230, 240, 255) }),
            ..ShapeStyle::default()
        });
        *caption = Some(Caption::new(
            vec![p("그림 1. 텍스트가 포함된 타원", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        ));
    }
    paras.push(ctrl_p(ell, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.3 다각형(Polygon) ──
    paras.push(p("3.3 다각형 (Polygon)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 삼각형 + 그라디언트 채우기
    paras.push(p("삼각형 (그라디언트 채우기):", CS_NORMAL, PS_LEFT));
    let mut tri = Control::polygon(vec![
        ShapePoint::new(5000, 0),
        ShapePoint::new(10000, 8660),
        ShapePoint::new(0, 8660),
    ])
    .unwrap();
    if let Control::Polygon { ref mut style, .. } = tri {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 100, 0)),
            line_width: Some(25),
            fill: Some(Fill::Gradient {
                gradient_type: GradientType::Linear,
                angle: 45,
                colors: vec![
                    (Color::from_rgb(255, 200, 200), 0),
                    (Color::from_rgb(200, 200, 255), 100),
                ],
            }),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(tri, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 오각형 + 패턴 채우기 + 캡션
    paras.push(p("오각형 (패턴 채우기):", CS_NORMAL, PS_LEFT));
    let mut pent = Control::polygon(vec![
        ShapePoint::new(5000, 0),
        ShapePoint::new(10000, 3800),
        ShapePoint::new(8100, 10000),
        ShapePoint::new(1900, 10000),
        ShapePoint::new(0, 3800),
    ])
    .unwrap();
    if let Control::Polygon { ref mut style, ref mut caption, .. } = pent {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(100, 50, 150)),
            fill: Some(Fill::Pattern {
                pattern_type: PatternType::Horizontal,
                fg_color: Color::from_rgb(100, 50, 150),
                bg_color: Color::from_rgb(240, 230, 255),
            }),
            ..ShapeStyle::default()
        });
        *caption = Some(Caption::new(
            vec![p("그림 2. 패턴 채우기 오각형", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        ));
    }
    paras.push(ctrl_p(pent, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.4 호(Arc) 3종 ──
    paras.push(p("3.4 호 (Arc) — 3가지 타입", CS_HEADING, PS_LEFT));
    paras.push(empty());

    let arc_w = HwpUnit::from_mm(35.0).unwrap();
    let arc_h = HwpUnit::from_mm(25.0).unwrap();

    paras.push(p("Normal (열린 호):", CS_NORMAL, PS_LEFT));
    paras.push(ctrl_p(Control::arc(ArcType::Normal, arc_w, arc_h), CS_NORMAL, PS_CENTER));
    paras.push(p("Pie (부채꼴):", CS_NORMAL, PS_LEFT));
    paras.push(ctrl_p(Control::arc(ArcType::Pie, arc_w, arc_h), CS_NORMAL, PS_CENTER));
    paras.push(p("Chord (활꼴):", CS_NORMAL, PS_LEFT));
    paras.push(ctrl_p(Control::arc(ArcType::Chord, arc_w, arc_h), CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.5 곡선(Curve) ──
    paras.push(p("3.5 곡선 (Curve)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 베지어 S자 곡선
    paras.push(p("베지어 S자 곡선:", CS_NORMAL, PS_LEFT));
    let mut bezier = Control::curve(vec![
        ShapePoint::new(0, 5000),
        ShapePoint::new(3000, 0),
        ShapePoint::new(6000, 10000),
        ShapePoint::new(9000, 5000),
    ])
    .unwrap();
    if let Control::Curve { ref mut segment_types, .. } = bezier {
        *segment_types =
            vec![CurveSegmentType::Curve, CurveSegmentType::Curve, CurveSegmentType::Curve];
    }
    paras.push(ctrl_p(bezier, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.6 연결선(ConnectLine) + 화살표 ──
    paras.push(p("3.6 연결선 (ConnectLine)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p("양방향 다이아몬드 화살표:", CS_NORMAL, PS_LEFT));
    let mut cl =
        Control::connect_line(ShapePoint::new(0, 2000), ShapePoint::new(14000, 2000)).unwrap();
    if let Control::ConnectLine { ref mut style, .. } = cl {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(150, 50, 50)),
            line_width: Some(30),
            head_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Diamond,
                size: ArrowSize::Large,
                filled: true,
            }),
            tail_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Diamond,
                size: ArrowSize::Large,
                filled: true,
            }),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(cl, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.7 글상자(TextBox) ──
    paras.push(p("3.7 글상자 (TextBox)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    let tb_w = HwpUnit::from_mm(100.0).unwrap();
    let tb_h = HwpUnit::from_mm(30.0).unwrap();
    let mut tb = Control::text_box(
        vec![
            p("이것은 글상자(TextBox) 안의 문단입니다.", CS_NORMAL, PS_BODY),
            p(
                "HWPX에서 글상자는 <hp:rect> + <hp:drawText> 구조로 인코딩됩니다. 별도의 Control 요소가 아닌 도형 객체입니다.",
                CS_SMALL,
                PS_BODY,
            ),
        ],
        tb_w,
        tb_h,
    );
    if let Control::TextBox { ref mut style, ref mut caption, .. } = tb {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 80, 160)),
            line_width: Some(25),
            fill: Some(Fill::Solid { color: Color::from_rgb(245, 248, 255) }),
            ..ShapeStyle::default()
        });
        *caption = Some(Caption::new(
            vec![p("그림 3. 스타일이 적용된 글상자", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        ));
    }
    paras.push(ctrl_p(tb, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 3.8 ShapeStyle: 회전, 뒤집기 ──
    paras.push(p("3.8 도형 스타일 — 회전/뒤집기", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 타원 45도 회전
    paras.push(p("타원 45도 회전:", CS_NORMAL, PS_LEFT));
    let mut ell_rot =
        Control::ellipse(HwpUnit::from_mm(35.0).unwrap(), HwpUnit::from_mm(25.0).unwrap());
    if let Control::Ellipse { ref mut style, .. } = ell_rot {
        *style = Some(ShapeStyle {
            rotation: Some(45.0),
            line_color: Some(Color::from_rgb(200, 100, 0)),
            line_width: Some(25),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(ell_rot, CS_NORMAL, PS_CENTER));

    // 타원 수평 뒤집기
    paras.push(p("타원 수평 뒤집기:", CS_NORMAL, PS_LEFT));
    let mut ell_flip =
        Control::ellipse(HwpUnit::from_mm(35.0).unwrap(), HwpUnit::from_mm(25.0).unwrap());
    if let Control::Ellipse { ref mut style, .. } = ell_flip {
        *style = Some(ShapeStyle {
            flip: Some(Flip::Horizontal),
            line_color: Some(Color::from_rgb(0, 150, 100)),
            line_width: Some(25),
            fill: Some(Fill::Solid { color: Color::from_rgb(220, 255, 240) }),
            ..ShapeStyle::default()
        });
    }
    paras.push(ctrl_p(ell_flip, CS_NORMAL, PS_CENTER));

    // ── 다단 설정 ──
    let mut section = Section::with_paragraphs(paras, landscape);
    section.column_settings =
        Some(ColumnSettings::equal_columns(2, HwpUnit::from_mm(8.0).unwrap()).unwrap());

    section
}
