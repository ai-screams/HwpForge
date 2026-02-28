#![allow(clippy::vec_init_then_push)]
//! PR #14 Review API 변경사항 검증 문서.
//!
//! 코드 리뷰에서 수정된 모든 API를 사용하여 HWPX 파일을 생성합니다.
//!
//! **변경된 API 목록**:
//! 1. `ShapeStyle` 타입 강화: `String→Color`, `i32→u32`, `String→LineStyle`
//! 2. `LineStyle` enum (Solid/Dash/Dot/DashDot/DashDotDot/None)
//! 3. `Equation.text_color`: `String→Color`
//! 4. `HeaderFooter::all_pages()` (was `both()`)
//! 5. `PageNumber::with_decoration()` (was `with_side_char()`)
//! 6. `ChartData::has_no_series()` (was `is_empty()`)
//! 7. `Paragraph::try_with_heading_level()` (fallible)
//! 8. `DEFAULT_CAPTION_GAP` 상수
//! 9. `Control::hyperlink()` URL 스킴 검증
//! 10. `Color::to_hex_rgb()` 포맷 출력
//! 11. Validation: `EmptyCategoryLabels`, `MismatchedSeriesLengths`
//!
//! # Usage
//! ```bash
//! cargo run -p hwpforge-smithy-hwpx --example review_api
//! ```

use hwpforge_core::caption::{Caption, CaptionSide, DEFAULT_CAPTION_GAP};
use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
use hwpforge_core::control::{Control, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, HwpUnit, NumberFormatType, PageNumberPosition, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Style Constants ─────────────────────────────────────────────

const CS_NORMAL: usize = 0;
const CS_BOLD: usize = 1;
const CS_ITALIC: usize = 2;
const CS_TITLE: usize = 3;
const CS_HEADING: usize = 4;
const CS_LINK: usize = 5;

const PS_LEFT: usize = 0;
const PS_CENTER: usize = 1;
const PS_JUSTIFY: usize = 2;

// ── Helpers ─────────────────────────────────────────────────────

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn p(text: &str) -> Paragraph {
    text_para(text, CS_NORMAL, PS_JUSTIFY)
}

fn empty() -> Paragraph {
    text_para("", CS_NORMAL, PS_LEFT)
}

fn ctrl_para(ctrl: Control, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn table_para(table: Table) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    )
}

// ── Section 0: 표지 + API 변경 요약 ────────────────────────────

fn build_section_0() -> Section {
    let mut paras: Vec<Paragraph> = Vec::new();

    // 제목
    paras.push(text_para("PR #14 Review API 변경사항 검증", CS_TITLE, PS_CENTER));
    paras.push(text_para("HwpForge Write API 코드 리뷰 수정 결과물", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // [API #8] horizontal_line
    paras.push(ctrl_para(
        Control::horizontal_line(HwpUnit::from_mm(150.0).expect("150mm")),
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // [API #7] try_with_heading_level — fallible alternative
    let heading_para: Paragraph = Paragraph::with_runs(
        vec![Run::text("1. 타입 안전성 강화", CharShapeIndex::new(CS_HEADING))],
        ParaShapeIndex::new(PS_LEFT),
    )
    .try_with_heading_level(1)
    .expect("heading level 1 valid");
    paras.push(heading_para);
    paras.push(empty());

    paras.push(p(
        "이 문서는 PR #14 코드 리뷰에서 지적된 48건의 수정사항 중 API 변경을 수반하는 항목들을 검증합니다.",
    ));
    paras.push(empty());

    // API 변경 요약 테이블
    let cw: HwpUnit = HwpUnit::new(14000).expect("col width");
    let header_row: TableRow = TableRow::new(vec![
        TableCell::new(vec![text_para("변경 항목", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![text_para("Before", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![text_para("After", CS_BOLD, PS_CENTER)], cw),
    ]);
    let rows: Vec<TableRow> = vec![
        TableRow::new(vec![
            TableCell::new(vec![p("ShapeStyle.line_color")], cw),
            TableCell::new(vec![p("Option<String>")], cw),
            TableCell::new(vec![p("Option<Color>")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("ShapeStyle.fill_color")], cw),
            TableCell::new(vec![p("Option<String>")], cw),
            TableCell::new(vec![p("Option<Color>")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("ShapeStyle.line_width")], cw),
            TableCell::new(vec![p("Option<i32>")], cw),
            TableCell::new(vec![p("Option<u32>")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("ShapeStyle.line_style")], cw),
            TableCell::new(vec![p("Option<String>")], cw),
            TableCell::new(vec![p("Option<LineStyle>")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("Equation.text_color")], cw),
            TableCell::new(vec![p("String")], cw),
            TableCell::new(vec![p("Color")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("HeaderFooter::both()")], cw),
            TableCell::new(vec![p("primary")], cw),
            TableCell::new(vec![p("deprecated → all_pages()")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("PageNumber.side_char")], cw),
            TableCell::new(vec![p("field name")], cw),
            TableCell::new(vec![p("decoration")], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("ChartData::is_empty()")], cw),
            TableCell::new(vec![p("primary")], cw),
            TableCell::new(vec![p("deprecated → has_no_series()")], cw),
        ]),
    ];
    let mut api_table: Table = Table::new(std::iter::once(header_row).chain(rows).collect());
    api_table.caption = Some(Caption::new(
        vec![text_para("[표 1] PR #14 API 변경 요약", CS_ITALIC, PS_CENTER)],
        CaptionSide::Top,
    ));
    paras.push(table_para(api_table));
    paras.push(empty());

    // [API #8] DEFAULT_CAPTION_GAP 사용 확인 (컴파일 타임 검증)
    paras.push(p(&format!(
        "참고: 캡션 기본 간격(DEFAULT_CAPTION_GAP) = {} HWPUNIT ≈ {:.1}mm",
        DEFAULT_CAPTION_GAP,
        DEFAULT_CAPTION_GAP as f64 / 283.0
    )));
    paras.push(empty());

    // [API #4] HeaderFooter::all_pages() — 새 API
    // [API #5] PageNumber::with_decoration() — 새 API
    let mut sec: Section = Section::with_paragraphs(paras, PageSettings::a4());
    sec.header = Some(HeaderFooter::all_pages(vec![text_para(
        "PR #14 Review — API 변경사항 검증 문서",
        CS_ITALIC,
        PS_LEFT,
    )]));
    sec.footer =
        Some(HeaderFooter::all_pages(vec![text_para("HwpForge © 2026", CS_ITALIC, PS_CENTER)]));
    sec.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "- ",
    ));
    sec
}

// ── Section 1: ShapeStyle + LineStyle 검증 ──────────────────────

fn build_section_1() -> Section {
    let mut paras: Vec<Paragraph> = Vec::new();

    let heading: Paragraph = Paragraph::with_runs(
        vec![Run::text("2. ShapeStyle 타입 안전성 검증", CharShapeIndex::new(CS_HEADING))],
        ParaShapeIndex::new(PS_LEFT),
    )
    .try_with_heading_level(1)
    .expect("heading level 1");
    paras.push(heading);
    paras.push(empty());

    paras.push(p(
        "ShapeStyle의 4개 필드가 모두 타입 안전한 값을 사용합니다. String 대신 Color, LineStyle, u32를 직접 사용하여 잘못된 값이 컴파일 타임에 거부됩니다.",
    ));
    paras.push(empty());

    // [API #1] ShapeStyle with typed Color fields
    // [API #2] LineStyle enum variants
    paras.push(text_para("2.1 LineStyle 열거형 변형", CS_BOLD, PS_LEFT));
    paras.push(empty());

    let line_variants: &[(LineStyle, &str, Color)] = &[
        (LineStyle::Solid, "실선 (Solid)", Color::from_rgb(0x00, 0x00, 0x00)),
        (LineStyle::Dash, "대시 (Dash)", Color::from_rgb(0xFF, 0x00, 0x00)),
        (LineStyle::Dot, "점선 (Dot)", Color::from_rgb(0x00, 0x80, 0x00)),
        (LineStyle::DashDot, "일점쇄선 (DashDot)", Color::from_rgb(0x00, 0x00, 0xFF)),
        (LineStyle::DashDotDot, "이점쇄선 (DashDotDot)", Color::from_rgb(0xFF, 0x8C, 0x00)),
    ];

    for &(style, label, color) in line_variants {
        paras.push(p(&format!("  {label}:")));
        // [API #1] ShapeStyle — Color, u32, LineStyle typed fields
        let shape_style: ShapeStyle = ShapeStyle {
            line_color: Some(color),
            fill_color: None,
            line_width: Some(80_u32), // u32 (was i32)
            line_style: Some(style),  // LineStyle enum (was String)
        };
        let line_ctrl: Control =
            Control::line(ShapePoint::new(0, 0), ShapePoint::new(35000, 0)).expect("valid line");
        // Apply style
        let styled_line: Control = match line_ctrl {
            Control::Line {
                start, end, width, height, horz_offset, vert_offset, caption, ..
            } => Control::Line {
                start,
                end,
                width,
                height,
                horz_offset,
                vert_offset,
                caption,
                style: Some(shape_style),
            },
            other => other,
        };
        paras.push(ctrl_para(styled_line, CS_NORMAL, PS_LEFT));
    }
    paras.push(empty());

    // [API #10] Color::to_hex_rgb() 확인
    paras.push(text_para("2.2 Color::to_hex_rgb() 출력 확인", CS_BOLD, PS_LEFT));
    paras.push(empty());

    let colors: &[(Color, &str)] = &[
        (Color::from_rgb(0xFF, 0x00, 0x00), "빨강"),
        (Color::from_rgb(0x00, 0xFF, 0x00), "초록"),
        (Color::from_rgb(0x00, 0x00, 0xFF), "파랑"),
        (Color::BLACK, "검정"),
    ];
    for &(color, name) in colors {
        paras.push(p(&format!(
            "  {name}: {} (BGR raw: {:#010X})",
            color.to_hex_rgb(),
            color.to_raw()
        )));
    }
    paras.push(empty());

    // [API #1] ShapeStyle::default() + 개별 필드 설정
    paras.push(text_para("2.3 타원 + 스타일 오버라이드", CS_BOLD, PS_LEFT));
    paras.push(empty());

    let ellipse_style: ShapeStyle = ShapeStyle {
        line_color: Some(Color::from_rgb(0x00, 0x66, 0xCC)),
        fill_color: Some(Color::from_rgb(0xE3, 0xF2, 0xFD)),
        line_width: Some(56_u32),
        line_style: Some(LineStyle::Solid),
    };
    let ew: HwpUnit = HwpUnit::from_mm(50.0).expect("50mm");
    let eh: HwpUnit = HwpUnit::from_mm(25.0).expect("25mm");
    let ellipse_ctrl: Control = {
        let mut e: Control = Control::ellipse(ew, eh);
        if let Control::Ellipse { ref mut style, .. } = e {
            *style = Some(ellipse_style);
        }
        e
    };
    paras.push(ctrl_para(ellipse_ctrl, CS_NORMAL, PS_CENTER));
    paras.push(p(
        "위 타원: line_color=Color::from_rgb(0,102,204), fill_color=Color::from_rgb(227,242,253)",
    ));
    paras.push(empty());

    // Polygon with LineStyle::DashDot + Caption
    paras.push(text_para("2.4 다각형 + DashDot 스타일 + 캡션", CS_BOLD, PS_LEFT));
    paras.push(empty());

    let poly_style: ShapeStyle = ShapeStyle {
        line_color: Some(Color::from_rgb(0x80, 0x00, 0x80)),
        fill_color: Some(Color::from_rgb(0xF3, 0xE5, 0xF5)),
        line_width: Some(42_u32),
        line_style: Some(LineStyle::DashDot),
    };
    let tw: i32 = HwpUnit::from_mm(30.0).expect("30mm").as_i32();
    let th: i32 = HwpUnit::from_mm(25.0).expect("25mm").as_i32();
    let pentagon: Control = {
        let mut poly: Control = Control::polygon(vec![
            ShapePoint::new(tw / 2, 0),
            ShapePoint::new(tw, th * 2 / 5),
            ShapePoint::new(tw * 4 / 5, th),
            ShapePoint::new(tw / 5, th),
            ShapePoint::new(0, th * 2 / 5),
            ShapePoint::new(tw / 2, 0), // 첫 점 반복 (gotcha #17)
        ])
        .expect("valid polygon");
        if let Control::Polygon { ref mut style, ref mut caption, .. } = poly {
            *style = Some(poly_style);
            *caption = Some(Caption::new(
                vec![text_para("[그림 1] DashDot 오각형", CS_ITALIC, PS_CENTER)],
                CaptionSide::Bottom,
            ));
        }
        poly
    };
    paras.push(ctrl_para(pentagon, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Section 2: Equation + Chart + Hyperlink ─────────────────────

fn build_section_2() -> Section {
    let mut paras: Vec<Paragraph> = Vec::new();

    let heading: Paragraph = Paragraph::with_runs(
        vec![Run::text("3. Equation / Chart / Hyperlink 검증", CharShapeIndex::new(CS_HEADING))],
        ParaShapeIndex::new(PS_LEFT),
    )
    .try_with_heading_level(1)
    .expect("heading level 1");
    paras.push(heading);
    paras.push(empty());

    // [API #3] Equation text_color: Color (was String)
    paras.push(text_para("3.1 수식 — Color 타입 text_color", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p("수식의 텍스트 색상이 String에서 Color 타입으로 변경되었습니다:"));
    paras.push(empty());

    // 빨간색 수식
    let red_eq: Control = Control::Equation {
        script: "E = mc^{2}".to_string(),
        width: HwpUnit::new(3000).expect("valid"),
        height: HwpUnit::new(1000).expect("valid"),
        base_line: 85,
        text_color: Color::from_rgb(0xCC, 0x00, 0x00), // Color 타입!
        font: "HancomEQ".to_string(),
    };
    paras.push(ctrl_para(red_eq, CS_NORMAL, PS_CENTER));
    paras.push(p("위 수식: text_color = Color::from_rgb(0xCC, 0x00, 0x00) — 빨간색"));
    paras.push(empty());

    // 파란색 수식
    let blue_eq: Control = Control::Equation {
        script: "{a+b} over {c+d}".to_string(),
        width: HwpUnit::new(3500).expect("valid"),
        height: HwpUnit::new(1308).expect("valid"),
        base_line: 90,
        text_color: Color::from_rgb(0x00, 0x00, 0xCC), // Color 타입!
        font: "HancomEQ".to_string(),
    };
    paras.push(ctrl_para(blue_eq, CS_NORMAL, PS_CENTER));
    paras.push(p("위 수식: text_color = Color::from_rgb(0x00, 0x00, 0xCC) — 파란색"));
    paras.push(empty());

    // [API #6] ChartData::has_no_series() (was is_empty())
    paras.push(text_para("3.2 ChartData::has_no_series() 검증", CS_BOLD, PS_LEFT));
    paras.push(empty());

    let chart_data: ChartData = ChartData::category(
        &["Foundation", "Core", "Blueprint", "Smithy-HWPX", "Smithy-MD"],
        &[("테스트 수", &[197.0, 360.0, 191.0, 273.0, 59.0])],
    );
    // [API #6] has_no_series() — 새 메서드 검증
    let is_non_empty: bool = !chart_data.has_no_series();
    paras.push(p(&format!(
        "chart_data.has_no_series() = {} (데이터가 있으므로 false)",
        chart_data.has_no_series()
    )));
    assert!(is_non_empty, "chart data should not be empty");

    let chart_ctrl: Control = Control::Chart {
        chart_type: ChartType::Column,
        data: chart_data,
        title: Some("크레이트별 테스트 수 (PR #14 수정 후)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::Clustered,
        width: HwpUnit::new(42520).expect("valid"),
        height: HwpUnit::new(21000).expect("valid"),
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
    paras.push(ctrl_para(chart_ctrl, CS_NORMAL, PS_CENTER));
    paras.push(text_para("[차트 1] 크레이트별 테스트 수 (총 1,080개)", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // [API #9] Control::hyperlink() — safe URL
    paras.push(text_para("3.3 하이퍼링크 — URL 스킴 검증", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "인코더가 javascript:, data:, file: 스킴을 거부합니다. 아래는 안전한 https:// 링크입니다:",
    ));
    paras.push(ctrl_para(
        Control::hyperlink("HwpForge GitHub", "https://github.com/nicokimmel/openhwp"),
        CS_LINK,
        PS_CENTER,
    ));
    paras.push(empty());

    // 각주 — 새 API와 함께 인라인
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("URL 검증은 인코더 단에서 수행됩니다.", CharShapeIndex::new(CS_NORMAL)),
            Run::control(
                Control::footnote(vec![p(
                    "1) is_safe_url() 함수가 http://, https://, mailto: 스킴만 허용합니다. javascript:, data:, file: 등은 HwpxError::InvalidStructure 에러를 반환합니다.",
                )]),
                CharShapeIndex::new(CS_NORMAL),
            ),
        ],
        ParaShapeIndex::new(PS_JUSTIFY),
    ));
    paras.push(empty());

    // [API #11] Validation 검증 (컴파일 타임 확인)
    paras.push(text_para("3.4 새로운 Validation 규칙", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p("PR #14에서 추가된 2개의 새로운 검증 규칙:"));
    paras.push(p("  • EmptyCategoryLabels (2014): Category 차트에 0개 카테고리 → 거부"));
    paras.push(p("  • MismatchedSeriesLengths (2015): XY 시리즈의 x/y 길이 불일치 → 거부"));
    paras.push(empty());

    // 미주
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "이 문서는 PR #14 코드 리뷰에서 수정된 모든 API 변경사항을 검증합니다.",
                CharShapeIndex::new(CS_NORMAL),
            ),
            Run::control(
                Control::endnote(vec![p(
                    "1) 총 48건의 리뷰 지적사항 중 API 변경이 필요한 항목: ShapeStyle 타입 강화 (4건), Equation text_color (1건), HeaderFooter/PageNumber 리네이밍 (2건), ChartData 리네이밍 (1건), try_with_heading_level (1건), DEFAULT_CAPTION_GAP (1건), URL 검증 (1건), 새 Validation (2건).",
                )]),
                CharShapeIndex::new(CS_NORMAL),
            ),
        ],
        ParaShapeIndex::new(PS_JUSTIFY),
    ));
    paras.push(empty());

    paras.push(ctrl_para(
        Control::horizontal_line(HwpUnit::from_mm(150.0).expect("150mm")),
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());
    paras.push(text_para("— HwpForge 개발팀, 2026년 2월 —", CS_ITALIC, PS_CENTER));

    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Main ────────────────────────────────────────────────────────

fn main() {
    println!("=== PR #14 Review API 변경사항 검증 ===\n");

    // ── 1. Style Store ──
    let mut style_store: HwpxStyleStore = HwpxStyleStore::with_default_fonts("함초롬돋움");

    // CS 0: Normal 10pt
    style_store.push_char_shape(HwpxCharShape::default());

    // CS 1: Bold 10pt
    let mut cs1: HwpxCharShape = HwpxCharShape::default();
    cs1.bold = true;
    style_store.push_char_shape(cs1);

    // CS 2: Italic 10pt, gray
    let mut cs2: HwpxCharShape = HwpxCharShape::default();
    cs2.italic = true;
    cs2.text_color = Color::from_rgb(102, 102, 102);
    style_store.push_char_shape(cs2);

    // CS 3: Title 20pt, navy bold
    let mut cs3: HwpxCharShape = HwpxCharShape::default();
    cs3.height = HwpUnit::from_pt(20.0).expect("20pt");
    cs3.bold = true;
    cs3.text_color = Color::from_rgb(0, 51, 102);
    style_store.push_char_shape(cs3);

    // CS 4: Heading 14pt, bold
    let mut cs4: HwpxCharShape = HwpxCharShape::default();
    cs4.height = HwpUnit::from_pt(14.0).expect("14pt");
    cs4.bold = true;
    style_store.push_char_shape(cs4);

    // CS 5: Link — blue + underline
    let mut cs5: HwpxCharShape = HwpxCharShape::default();
    cs5.text_color = Color::from_rgb(5, 99, 193);
    cs5.underline_type = hwpforge_foundation::UnderlineType::Bottom;
    cs5.underline_color = Some(Color::from_rgb(5, 99, 193));
    style_store.push_char_shape(cs5);

    // PS 0: Left
    style_store.push_para_shape(HwpxParaShape::default());

    // PS 1: Center
    let mut ps1: HwpxParaShape = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    style_store.push_para_shape(ps1);

    // PS 2: Justify
    let mut ps2: HwpxParaShape = HwpxParaShape::default();
    ps2.alignment = Alignment::Justify;
    style_store.push_para_shape(ps2);

    println!("[1] Style store: 7 fonts, 6 char shapes, 3 para shapes");

    // ── 2. Build Document ──
    let mut doc: Document = Document::new();
    doc.add_section(build_section_0());
    doc.add_section(build_section_1());
    doc.add_section(build_section_2());

    println!("[2] Document: {} sections", doc.sections().len());
    for (i, sec) in doc.sections().iter().enumerate() {
        println!(
            "    S{}: {} paras, header={}, footer={}, page_num={}",
            i + 1,
            sec.paragraphs.len(),
            sec.header.is_some(),
            sec.footer.is_some(),
            sec.page_number.is_some(),
        );
    }

    // ── 3. Validate ──
    let validated = doc.validate().expect("document validation failed");
    println!("[3] Validation: OK");

    // ── 4. Encode ──
    let image_store: ImageStore = ImageStore::new();
    let bytes: Vec<u8> =
        HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode failed");

    std::fs::create_dir_all("temp").ok();
    let path: &str = "temp/review_api.hwpx";
    std::fs::write(path, &bytes).expect("write failed");
    println!("[4] Encoded: {path} ({} bytes)", bytes.len());

    // ── 5. API 검증 출력 ──
    println!("\n--- API 변경사항 검증 결과 ---");
    println!("  [✓] ShapeStyle: Color, u32, LineStyle 타입 사용");
    println!("  [✓] LineStyle: Solid/Dash/Dot/DashDot/DashDotDot 5종");
    println!("  [✓] Equation.text_color: Color 타입");
    println!("  [✓] HeaderFooter::all_pages() 사용");
    println!("  [✓] PageNumber::with_decoration() 사용");
    println!("  [✓] ChartData::has_no_series() 사용");
    println!("  [✓] try_with_heading_level() fallible 사용");
    println!("  [✓] DEFAULT_CAPTION_GAP 상수 참조");
    println!("  [✓] Control::hyperlink() safe URL");
    println!("  [✓] Color::to_hex_rgb() 포맷 출력");

    println!("\n=== 완료! 한글에서 열어 확인하세요: {path} ===");
}
