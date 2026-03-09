//! 도형 위치 지정, 차트 세부 변형, TOC titleMark 테스트
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example positioning_and_toc
//!
//! Output:
//!   temp/positioning_and_toc.hwpx

use hwpforge_core::chart::{
    BarShape, ChartData, ChartGrouping, ChartType, LegendPosition, OfPieType, RadarStyle,
    ScatterStyle, StockVariant,
};
use hwpforge_core::control::{Control, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{Alignment, CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Style indices ──────────────────────────────────────────────
const CS_NORMAL: CharShapeIndex = CharShapeIndex::new(0);
const CS_HEADING: CharShapeIndex = CharShapeIndex::new(1);
const PS_BODY: ParaShapeIndex = ParaShapeIndex::new(0);
const PS_CENTER: ParaShapeIndex = ParaShapeIndex::new(1);

// ── Helpers ────────────────────────────────────────────────────

fn p(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CS_NORMAL)], PS_BODY)
}

fn heading(text: &str, level: u8) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CS_HEADING)], PS_BODY).with_heading_level(level)
}

fn ctrl_para(ctrl: Control) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CS_NORMAL)], PS_CENTER)
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }

    // CS 0: normal (10pt, black)
    store.push_char_shape(HwpxCharShape::default());

    // CS 1: heading (14pt, bold, dark blue)
    let mut cs_h: HwpxCharShape = HwpxCharShape::default();
    cs_h.height = HwpUnit::from_pt(14.0).unwrap();
    cs_h.bold = true;
    cs_h.text_color = Color::from_rgb(0, 51, 102);
    store.push_char_shape(cs_h);

    // PS 0: body (left, 160% line)
    store.push_para_shape(HwpxParaShape::default());

    // PS 1: center
    let mut ps_c: HwpxParaShape = HwpxParaShape::default();
    ps_c.alignment = Alignment::Center;
    store.push_para_shape(ps_c);

    store
}

// ── Section 1: TOC titleMark ──────────────────────────────────

fn section_toc() -> Section {
    let paras: Vec<Paragraph> = vec![
        heading("1. 서론", 1),
        p("이 문서는 HwpForge의 새로운 기능을 테스트합니다."),
        p("heading_level이 설정된 문단은 titleMark가 삽입되어 한글에서 자동 목차 생성이 가능합니다."),
        p(""),
        heading("1.1 배경", 2),
        p("HWPX Write API v1.0의 마지막 미완성 항목 3가지를 구현했습니다:"),
        p("  - Line/Polygon 절대 위치 지정 (horz_offset/vert_offset)"),
        p("  - Chart 71-variant 전체 호환 (9개 sub-option 필드)"),
        p("  - TOC titleMark 지원 (heading_level → hp:titleMark)"),
        p(""),
        heading("1.2 목적", 2),
        p("이 테스트 파일은 위 3가지 기능이 한글에서 정상 동작하는지 확인합니다."),
        p(""),
        heading("2. 도형 위치 테스트", 1),
        p("다음 섹션에서 Line과 Polygon의 절대 위치 지정을 테스트합니다."),
        p(""),
        heading("3. 차트 서브 옵션 테스트", 1),
        p("다양한 차트 서브 타입(폭발 파이, 마커 라인, 레이더 등)을 테스트합니다."),
    ];
    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Section 2: Line/Polygon Positioning ───────────────────────

fn section_positioning() -> Section {
    let mut paras: Vec<Paragraph> = vec![
        heading("2. 도형 절대 위치 테스트", 1),
        p(""),
        p("아래 선은 inline (horz_offset=0, vert_offset=0):"),
    ];

    // Inline line (default positioning)
    let line_inline: Control =
        Control::line(ShapePoint::new(0, 0), ShapePoint::new(14000, 0)).expect("valid line");
    paras.push(ctrl_para(line_inline));
    paras.push(p(""));

    // Line with absolute offset
    paras.push(p("아래 선은 절대 위치 (horz_offset=5000, vert_offset=2000):"));
    let line_abs: Control = Control::Line {
        start: ShapePoint::new(0, 0),
        end: ShapePoint::new(14000, 0),
        width: HwpUnit::from_pt(140.0).unwrap(),
        height: HwpUnit::new(100).expect("valid"),
        horz_offset: 5000,
        vert_offset: 2000,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0xFF, 0x00, 0x00)),
            fill_color: None,
            line_width: Some(100),
            line_style: None,
            ..Default::default()
        }),
    };
    paras.push(ctrl_para(line_abs));
    paras.push(p(""));

    // Polygon inline (triangle)
    paras.push(p("아래 삼각형은 inline:"));
    let triangle: Control = Control::polygon(vec![
        ShapePoint::new(0, 5000),
        ShapePoint::new(5000, 0),
        ShapePoint::new(10000, 5000),
    ])
    .expect("valid polygon");
    paras.push(ctrl_para(triangle));
    paras.push(p(""));

    // Polygon with absolute offset (diamond)
    paras.push(p("아래 마름모는 절대 위치 (horz_offset=8000, vert_offset=1000):"));
    let diamond: Control = Control::Polygon {
        vertices: vec![
            ShapePoint::new(5000, 0),
            ShapePoint::new(10000, 5000),
            ShapePoint::new(5000, 10000),
            ShapePoint::new(0, 5000),
        ],
        width: HwpUnit::new(10000).expect("valid"),
        height: HwpUnit::new(10000).expect("valid"),
        horz_offset: 8000,
        vert_offset: 1000,
        paragraphs: vec![],
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0x00, 0x64, 0xC8)),
            fill_color: Some(Color::from_rgb(0xC8, 0xE6, 0xFF)),
            line_width: None,
            line_style: None,
            ..Default::default()
        }),
    };
    paras.push(ctrl_para(diamond));

    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Section 3: Chart Sub-variants ─────────────────────────────

fn section_charts() -> Section {
    let mut paras: Vec<Paragraph> = vec![heading("3. 차트 서브 옵션 테스트", 1), p("")];

    let cat3: &[&str] = &["Q1", "Q2", "Q3"];
    let cat5: &[&str] = &["항목A", "항목B", "항목C", "항목D", "항목E"];

    // ① Exploded Pie
    paras.push(p("① 폭발 파이 차트 (explosion=25):"));
    let pie_data: ChartData = ChartData::category(cat3, &[("매출", &[30.0, 45.0, 25.0])]);
    let pie_chart: Control = Control::Chart {
        chart_type: ChartType::Pie,
        data: pie_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("폭발 파이 차트".to_string()),
        legend: LegendPosition::Right,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: Some(25),
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(pie_chart));
    paras.push(p(""));

    // ② Line with markers
    paras.push(p("② 꺾은선 + 표식 (show_markers=true):"));
    let line_data: ChartData = ChartData::category(
        cat3,
        &[("시리즈1", &[10.0, 25.0, 18.0]), ("시리즈2", &[15.0, 12.0, 30.0])],
    );
    let line_chart: Control = Control::Chart {
        chart_type: ChartType::Line,
        data: line_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("꺾은선 + 표식".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::Standard,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: Some(true),
        stock_variant: None,
    };
    paras.push(ctrl_para(line_chart));
    paras.push(p(""));

    // ③ Filled Radar
    paras.push(p("③ 채워진 방사형 (radar_style=Filled):"));
    let radar_data: ChartData = ChartData::category(
        cat5,
        &[("팀A", &[80.0, 90.0, 70.0, 85.0, 60.0]), ("팀B", &[65.0, 75.0, 95.0, 70.0, 80.0])],
    );
    let radar_chart: Control = Control::Chart {
        chart_type: ChartType::Radar,
        data: radar_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("채워진 방사형".to_string()),
        legend: LegendPosition::Right,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: Some(RadarStyle::Filled),
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(radar_chart));
    paras.push(p(""));

    // ④ Scatter smooth (scatter charts require XY data, NOT category data)
    paras.push(p("④ 분산형 부드러운 곡선 (scatter_style=SmoothMarker):"));
    let scatter_data: ChartData =
        ChartData::xy(&[("데이터", &[1.0, 2.0, 3.0, 4.0, 5.0], &[5.0, 15.0, 12.0, 18.0, 10.0])]);
    let scatter_chart: Control = Control::Chart {
        chart_type: ChartType::Scatter,
        data: scatter_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("분산형 부드러운 곡선".to_string()),
        legend: LegendPosition::Right,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: Some(ScatterStyle::SmoothMarker),
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(scatter_chart));
    paras.push(p(""));

    // ⑤ 3D Column with Cylinder shape
    paras.push(p("⑤ 3D 세로 막대 원기둥 (bar_shape=Cylinder):"));
    let col3d_data: ChartData = ChartData::category(cat3, &[("수익", &[100.0, 150.0, 120.0])]);
    let col3d_chart: Control = Control::Chart {
        chart_type: ChartType::Column3D,
        data: col3d_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("3D 원기둥 막대".to_string()),
        legend: LegendPosition::Right,
        grouping: ChartGrouping::Clustered,
        bar_shape: Some(BarShape::Cylinder),
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(col3d_chart));
    paras.push(p(""));

    // ⑥ Bar-of-Pie
    paras.push(p("⑥ 원형 대 막대 (of_pie_type=Bar):"));
    let ofpie_data: ChartData =
        ChartData::category(cat5, &[("비율", &[40.0, 25.0, 15.0, 12.0, 8.0])]);
    let ofpie_chart: Control = Control::Chart {
        chart_type: ChartType::OfPie,
        data: ofpie_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("원형 대 막대".to_string()),
        legend: LegendPosition::Right,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: Some(OfPieType::Bar),
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(ofpie_chart));
    paras.push(p(""));

    // ⑦ Stock HLC (basic: High-Low-Close, no volume)
    paras.push(p("⑦ 주식 HLC 차트 (기본 stock_variant=None):"));
    let hlc_data: ChartData = ChartData::category(
        &["1월", "2월", "3월", "4월"],
        &[
            ("고가", &[110.0, 120.0, 115.0, 130.0]),
            ("저가", &[90.0, 95.0, 88.0, 100.0]),
            ("종가", &[105.0, 110.0, 100.0, 125.0]),
        ],
    );
    let hlc_chart: Control = Control::Chart {
        chart_type: ChartType::Stock,
        data: hlc_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("주식 HLC 차트".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    paras.push(ctrl_para(hlc_chart));
    paras.push(p(""));

    // ⑧ Stock VHLC (Volume + High-Low-Close, composite plotArea)
    paras.push(p("⑧ 주식 VHLC 차트 (stock_variant=Vhlc):"));
    let vhlc_data: ChartData = ChartData::category(
        &["1월", "2월", "3월", "4월"],
        &[
            ("거래량", &[1000.0, 1500.0, 1200.0, 1800.0]),
            ("고가", &[110.0, 120.0, 115.0, 130.0]),
            ("저가", &[90.0, 95.0, 88.0, 100.0]),
            ("종가", &[105.0, 110.0, 100.0, 125.0]),
        ],
    );
    let vhlc_chart: Control = Control::Chart {
        chart_type: ChartType::Stock,
        data: vhlc_data,
        width: HwpUnit::new(28000).expect("valid"),
        height: HwpUnit::new(18000).expect("valid"),
        title: Some("주식 VHLC 차트".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: Some(StockVariant::Vhlc),
    };
    paras.push(ctrl_para(vhlc_chart));

    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Main ───────────────────────────────────────────────────────

fn main() {
    let store: HwpxStyleStore = build_store();

    let mut doc = Document::new();
    doc.add_section(section_toc());
    doc.add_section(section_positioning());
    doc.add_section(section_charts());
    let validated = doc.validate().expect("validation failed");

    std::fs::create_dir_all("temp").expect("create temp dir");

    let images = ImageStore::new();
    HwpxEncoder::encode_file("temp/positioning_and_toc.hwpx", &validated, &store, &images)
        .expect("encode failed");

    println!("Created: temp/positioning_and_toc.hwpx");
    println!("Open in 한글 to verify:");
    println!("  - Section 1: heading_level → titleMark (삽입 > 차례에서 TOC 생성 가능)");
    println!("  - Section 2: Line/Polygon with horz_offset/vert_offset");
    println!("  - Section 3: 8 chart sub-variants (explosion, markers, radar, scatter, cylinder, ofPie, stock HLC, stock VHLC)");
}
