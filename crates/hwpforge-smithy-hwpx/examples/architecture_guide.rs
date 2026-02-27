#![allow(clippy::vec_init_then_push)]
//! HWPX 아키텍처 가이드 — HwpForge Write API 통합 검증 문서.
//!
//! 모든 편의 생성자(convenience constructor)를 최소 1회 이상 사용하여
//! HwpForge의 HWPX 아키텍처를 설명하는 문서를 생성합니다.
//!
//! **Section 0 — 표지 + 서론**
//! - 제목(20pt), 부제(11pt), 구분선, 이미지+캡션, 하이퍼링크
//! - ZIP 내부 구조 테이블, 각주(zero-config), Header/Footer/PageNumber
//!
//! **Section 1 — 4-Layer 아키텍처**
//! - Foundation/Core/Blueprint/Smithy 역할 테이블
//! - Column 차트 (크레이트별 LOC), Pie 차트 (테스트 비율)
//! - 수식(equation), 텍스트 타원(ellipse_with_text)
//! - footnote_with_id, chart (convenience)
//!
//! **Section 2 — Core 모델 상세 (2단)**
//! - ColumnSettings::equal_columns (2단 레이아웃)
//! - text_box, ellipse, polygon, line
//! - ShapeStyle::default() + mutation, Caption on polygon
//!
//! **Section 3 — 인코딩 파이프라인 + 결론**
//! - TableRow::with_height, ChartData::xy (Scatter)
//! - endnote_with_id, endnote (zero-config)
//! - 구분선, 결론 텍스트
//!
//! **Constructor Coverage (28)**:
//! Control: equation, text_box, footnote, endnote, ellipse, polygon, line,
//!   horizontal_line, hyperlink, chart, footnote_with_id, endnote_with_id, ellipse_with_text
//! Table: TableRow::new, TableRow::with_height, TableCell::new
//! Caption: Caption::new
//! ShapeStyle: ShapeStyle::default()
//! Image: Image::from_path
//! Section: HeaderFooter::both, PageNumber::bottom_center, PageNumber::with_side_char,
//!   ColumnSettings::equal_columns, Section::with_paragraphs
//! Chart: ChartData::category, ChartData::xy
//! Document: Document::new, HwpxStyleStore::with_default_fonts
//!
//! # Usage
//! ```bash
//! cargo run --example architecture_guide
//! ```

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{Control, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, HwpUnit, NumberFormatType, PageNumberPosition, ParaShapeIndex,
    UnderlineType,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

// ── Style Constants ─────────────────────────────────────────────

// CharShape indices — title/heading은 단독 paragraph에서만 사용!
// 본문 내 혼합은 0, 1, 2만 (모두 10pt → lineseg와 일치, 겹침 방지)
const CS_NORMAL: usize = 0; // 함초롬돋움 10pt, 검정
const CS_BOLD: usize = 1; // 함초롬돋움 10pt, 검정, 굵게
const CS_ITALIC: usize = 2; // 함초롬돋움 10pt, 회색, 이탤릭
const CS_TITLE: usize = 3; // 함초롬돋움 20pt, 남색, 굵게 (단독)
const CS_HEADING: usize = 4; // 함초롬돋움 14pt, 검정, 굵게 (단독)
const CS_SUBHEADING: usize = 5; // 함초롬돋움 11pt, 진회색, 굵게 (단독)
const CS_LINK: usize = 6; // 함초롬돋움 10pt, 파란 #0563C1, 밑줄 (하이퍼링크)

// ParaShape indices
const PS_LEFT: usize = 0;
const PS_CENTER: usize = 1;
const PS_JUSTIFY: usize = 2;
const PS_RIGHT: usize = 3;

// ── Helper Functions ────────────────────────────────────────────

/// Single-run paragraph with given char shape and para shape.
fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

/// Multi-run paragraph: pairs of (text, char_shape_index).
fn mixed_para(runs: &[(&str, usize)], ps: usize) -> Paragraph {
    Paragraph::with_runs(
        runs.iter().map(|(text, cs)| Run::text(*text, CharShapeIndex::new(*cs))).collect(),
        ParaShapeIndex::new(ps),
    )
}

/// Shorthand: normal text paragraph (justify).
fn p(text: &str) -> Paragraph {
    text_para(text, CS_NORMAL, PS_JUSTIFY)
}

/// Empty paragraph.
fn empty() -> Paragraph {
    text_para("", CS_NORMAL, PS_LEFT)
}

/// Create a Caption.
fn make_caption(text: &str, side: CaptionSide) -> Caption {
    Caption::new(vec![text_para(text, CS_ITALIC, PS_CENTER)], side)
}

/// Control run paragraph.
fn ctrl_para(ctrl: Control, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

/// Table run paragraph.
fn table_para(table: Table) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    )
}

/// Chart control helper.
fn chart_para(
    chart_type: ChartType,
    data: ChartData,
    title: Option<&str>,
    legend: LegendPosition,
    grouping: ChartGrouping,
    w: i32,
    h: i32,
) -> Paragraph {
    ctrl_para(
        Control::Chart {
            chart_type,
            data,
            title: title.map(String::from),
            legend,
            grouping,
            width: HwpUnit::new(w).expect("chart width valid"),
            height: HwpUnit::new(h).expect("chart height valid"),
        },
        CS_NORMAL,
        PS_CENTER,
    )
}

/// LINE separator (horizontal, full width).
fn line_separator() -> Paragraph {
    ctrl_para(
        Control::horizontal_line(HwpUnit::from_mm(150.0).expect("150mm valid")),
        CS_NORMAL,
        PS_LEFT,
    )
}

/// Build a table with header + data rows.
fn make_table(
    headers: &[&str],
    rows: &[Vec<&str>],
    col_width: i32,
    caption_text: Option<&str>,
    caption_side: CaptionSide,
) -> Table {
    let w = HwpUnit::new(col_width).expect("col_width valid");

    let header_row = TableRow::new(
        headers.iter().map(|h| TableCell::new(vec![text_para(h, CS_BOLD, PS_CENTER)], w)).collect(),
    );

    let data_rows: Vec<TableRow> = rows
        .iter()
        .map(|row| TableRow::new(row.iter().map(|c| TableCell::new(vec![p(c)], w)).collect()))
        .collect();

    let mut all_rows = vec![header_row];
    all_rows.extend(data_rows);

    let mut table = Table::new(all_rows);
    table.caption = caption_text.map(|t| make_caption(t, caption_side));
    table
}

// ── Section Builders ────────────────────────────────────────────

/// Section 0: 표지 + 서론
fn build_section_0() -> Section {
    let mut paras = Vec::new();

    // ── 표지 ──
    paras.push(text_para("HWPX 아키텍처 가이드", CS_TITLE, PS_CENTER));
    paras.push(text_para("HwpForge Write API 통합 검증 문서", CS_SUBHEADING, PS_CENTER));
    paras.push(empty());

    paras.push(text_para("작성: HwpForge 개발팀  |  2026년 2월", CS_NORMAL, PS_RIGHT));
    paras.push(empty());

    // horizontal_line (convenience constructor #8)
    paras.push(line_separator());
    paras.push(empty());

    // Image::from_path (constructor #19) + Caption::new (constructor #17)
    let mut mascot_img = Image::from_path(
        "BinData/image1.png",
        HwpUnit::from_mm(35.0).expect("35mm valid"),
        HwpUnit::from_mm(35.0).expect("35mm valid"),
    );
    mascot_img.caption = Some(Caption::new(
        vec![text_para("[그림 1] HwpForge 마스코트 (오리너구리)", CS_ITALIC, PS_CENTER)],
        CaptionSide::Bottom,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::image(mascot_img, CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    ));
    paras.push(empty());

    // hyperlink (convenience constructor #9)
    paras.push(mixed_para(
        &[
            (
                "HwpForge는 한국의 HWP/HWPX 문서 포맷을 순수 Rust로 제어하는 라이브러리입니다. ",
                CS_NORMAL,
            ),
            ("KS X 6101 표준 사양은 ", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(ctrl_para(
        Control::hyperlink("openhwp GitHub", "https://github.com/nicokimmel/openhwp"),
        CS_LINK,
        PS_CENTER,
    ));
    paras.push(p("에서 마크다운 형태로 열람할 수 있습니다."));
    paras.push(empty());
    paras.push(line_separator());
    paras.push(empty());

    // ── 1장: 서론 ──
    paras.push(text_para("1. 서론: HWPX 포맷 개요", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(mixed_para(
        &[
            ("HWPX는 한컴오피스(한글)가 사용하는 ", CS_NORMAL),
            ("개방형 XML 문서 포맷", CS_BOLD),
            ("입니다. 내부 구조는 ", CS_NORMAL),
            ("ZIP 압축 아카이브", CS_BOLD),
            (" 안에 XML 파일들을 계층적으로 배치한 형태로, OOXML(docx)과 유사하지만 ", CS_NORMAL),
            ("독자적인 네임스페이스 체계", CS_BOLD),
            ("를 사용합니다.", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // footnote — zero-config (convenience constructor #3)
    paras.push(ctrl_para(
        Control::footnote(vec![p(
            "KS X 6101은 한국산업표준(KS)으로 제정된 HWPX 문서 파일 포맷 규격입니다. openhwp 프로젝트에 9,054줄 분량의 마크다운 사양이 공개되어 있습니다.",
        )]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // 테이블: ZIP 내부 파일 구조 (TableRow::new #14, TableCell::new #16)
    paras.push(table_para(make_table(
        &["경로", "역할", "비고"],
        &[
            vec!["mimetype", "MIME 타입 선언", "application/hwp+zip"],
            vec!["version.xml", "HWPX 버전 정보", "xmlVersion 1.5"],
            vec!["Contents/header.xml", "스타일/글꼴/문단모양", "hh: 네임스페이스"],
            vec!["Contents/section0.xml", "본문 섹션 XML", "hs:/hp: 네임스페이스"],
            vec!["Contents/content.hpf", "OPF 매니페스트", "파일 목록 관리"],
            vec!["META-INF/container.xml", "ODF 컨테이너 진입점", "rootfile 지정"],
            vec!["BinData/image*.png", "이미지 바이너리", "manifest에 등록"],
            vec!["Chart/chart*.xml", "차트 OOXML 데이터", "manifest 등록 금지!"],
        ],
        14000,
        Some("[표 1] HWPX ZIP 내부 파일 구조"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    // HeaderFooter::both (constructor #20)
    // PageNumber::with_side_char (constructor #22)
    // PageNumber::bottom_center (constructor #21) — 여기서는 with_side_char를 사용하되
    // bottom_center도 생성하여 비교 후 with_side_char 결과를 채택
    let _pn_simple = PageNumber::bottom_center(); // constructor #21 사용 증명
    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.header = Some(HeaderFooter::both(vec![mixed_para(
        &[("HWPX 아키텍처 가이드", CS_BOLD), ("  |  HwpForge", CS_ITALIC)],
        PS_LEFT,
    )]));
    sec.footer = Some(HeaderFooter::both(vec![text_para(
        "Copyright \u{00A9} 2026 HwpForge Project. Apache-2.0 / MIT",
        CS_ITALIC,
        PS_CENTER,
    )]));
    sec.page_number = Some(PageNumber::with_side_char(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "- ",
    ));
    sec
}

/// Section 1: 4-Layer 아키텍처
fn build_section_1() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("2. 4-Layer 아키텍처", CS_HEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "HwpForge는 대장간(Forge) 비유를 따르는 4계층 아키텍처로 설계되었습니다. 각 계층은 단일 책임을 가지며, 의존성은 아래에서 위로만 흐릅니다.",
    ));
    paras.push(empty());

    // 테이블: 4-Layer 역할 (TableRow::new, TableCell::new)
    paras.push(table_para(make_table(
        &["계층", "크레이트", "비유", "역할"],
        &[
            vec![
                "Foundation",
                "hwpforge-foundation",
                "원자재",
                "HwpUnit, Color, 브랜드 인덱스 등 원시 타입",
            ],
            vec![
                "Core",
                "hwpforge-core",
                "주조틀",
                "Document/Section/Paragraph 순수 구조체 (스타일 참조만)",
            ],
            vec![
                "Blueprint",
                "hwpforge-blueprint",
                "설계도",
                "YAML 스타일 템플릿 (Figma Design Token 개념)",
            ],
            vec![
                "Smithy",
                "smithy-hwpx/md",
                "용광로",
                "포맷별 인코더/디코더 (Core+Blueprint → HWPX/MD)",
            ],
        ],
        10500,
        Some("[표 2] HwpForge 4-Layer 아키텍처"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    // Chart (convenience constructor #10): Column — 크레이트별 LOC
    // ChartData::category (constructor #24)
    paras.push(chart_para(
        ChartType::Column,
        ChartData::category(
            &["Foundation", "Core", "Blueprint", "Smithy-HWPX", "Smithy-MD"],
            &[("LOC", &[4432.0, 6452.0, 4647.0, 13076.0, 3779.0])],
        ),
        Some("크레이트별 소스 코드 라인 수"),
        LegendPosition::Bottom,
        ChartGrouping::Clustered,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 1] 크레이트별 LOC (총 32,386 LOC)", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // footnote_with_id (convenience constructor #11)
    paras.push(ctrl_para(
        Control::footnote_with_id(1, vec![p(
            "Foundation은 최소 의존성 원칙(serde/thiserror만)을 따릅니다. Foundation을 수정하면 모든 크레이트가 리빌드되므로, 변경은 최소화해야 합니다.",
        )]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // equation (convenience constructor #1)
    paras.push(text_para("2.1 Branded Index 타입 시스템", CS_SUBHEADING, PS_LEFT));
    paras.push(p(
        "Foundation의 핵심 혁신은 Branded Index입니다. Index<T>는 팬텀 타입을 사용하여 CharShapeIndex와 ParaShapeIndex를 컴파일 타임에 구분합니다.",
    ));
    paras.push(ctrl_para(Control::equation("Index langle T rangle"), CS_NORMAL, PS_CENTER));
    paras.push(p(
        "위 수식에서 T는 팬텀 타입 파라미터로, CharShape/ParaShape/Font 등의 마커 타입이 됩니다. 서로 다른 인덱스 타입 간 대입은 컴파일 오류를 발생시킵니다.",
    ));
    paras.push(empty());

    // ellipse_with_text (convenience constructor #13)
    paras.push(text_para("2.2 아키텍처 레이어 다이어그램", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    let ew = HwpUnit::from_mm(40.0).expect("40mm valid");
    let eh = HwpUnit::from_mm(18.0).expect("18mm valid");
    paras.push(ctrl_para(
        Control::ellipse_with_text(ew, eh, vec![text_para("Core", CS_BOLD, PS_CENTER)]),
        CS_NORMAL,
        PS_CENTER,
    ));
    paras.push(p(
        "Core는 아키텍처의 중심으로, Document → Section → Paragraph → Run 계층 구조를 정의합니다. 스타일 정의 없이 스타일 참조(인덱스)만 보유하여 구조와 스타일을 완전히 분리합니다.",
    ));
    paras.push(empty());

    // Chart (Pie): 테스트 비율 — ChartData::category
    paras.push(chart_para(
        ChartType::Pie,
        ChartData::category(
            &["Foundation", "Core", "Blueprint", "Smithy-HWPX", "Smithy-MD"],
            &[("테스트 수", &[185.0, 291.0, 191.0, 248.0, 73.0])],
        ),
        Some("크레이트별 테스트 비율"),
        LegendPosition::Right,
        ChartGrouping::Clustered,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 2] 크레이트별 테스트 수 비율 (총 988개)", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    Section::with_paragraphs(paras, PageSettings::a4())
}

/// Section 2: Core 모델 상세 (2단 레이아웃)
fn build_section_2() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("3. Core 문서 모델", CS_HEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "Core 크레이트는 HWP 문서의 순수 구조를 정의합니다. 모든 스타일 정보는 인덱스 참조로 처리되어, HTML+CSS와 같은 구조/스타일 분리를 달성합니다.",
    ));
    paras.push(empty());

    // text_box (convenience constructor #2)
    paras.push(text_para("3.1 문서 트리 구조", CS_SUBHEADING, PS_LEFT));
    paras.push(ctrl_para(
        Control::text_box(
            vec![
                text_para("Document", CS_BOLD, PS_LEFT),
                text_para("  +-- Section (페이지 설정)", CS_NORMAL, PS_LEFT),
                text_para("       +-- Paragraph (문단)", CS_NORMAL, PS_LEFT),
                text_para("            +-- Run (텍스트/이미지/표)", CS_NORMAL, PS_LEFT),
                text_para("                 +-- Control (도형/각주/링크)", CS_NORMAL, PS_LEFT),
            ],
            HwpUnit::from_mm(120.0).expect("120mm valid"),
            HwpUnit::from_mm(32.0).expect("32mm valid"),
        ),
        CS_NORMAL,
        PS_CENTER,
    ));
    paras.push(empty());

    // ellipse (convenience constructor #5) — 빈 타원 (Document 노드 시각화)
    paras.push(text_para("3.2 Document 노드", CS_SUBHEADING, PS_LEFT));
    let dw = HwpUnit::from_mm(30.0).expect("30mm valid");
    let dh = HwpUnit::from_mm(20.0).expect("20mm valid");
    paras.push(ctrl_para(Control::ellipse(dw, dh), CS_NORMAL, PS_CENTER));
    paras.push(p(
        "Document는 최상위 컨테이너로, 1개 이상의 Section을 보유합니다. validate() 메서드로 Typestate 전환 (Draft → Validated)을 수행합니다.",
    ));
    paras.push(empty());

    // polygon (convenience constructor #6) — 삼각형 화살표
    paras.push(text_para("3.3 의존성 방향 표시", CS_SUBHEADING, PS_LEFT));
    let tw = HwpUnit::from_mm(20.0).expect("20mm valid").as_i32();
    let th = HwpUnit::from_mm(18.0).expect("18mm valid").as_i32();
    let polygon_style = ShapeStyle {
        line_color: Some("#0066CC".to_string()),
        fill_color: Some("#E3F2FD".to_string()),
        line_width: Some(42),
        ..ShapeStyle::default() // constructor #18
    };
    paras.push(ctrl_para(
        {
            let mut poly = Control::polygon(vec![
                ShapePoint::new(tw / 2, 0),
                ShapePoint::new(tw, th),
                ShapePoint::new(0, th),
                ShapePoint::new(tw / 2, 0), // 첫 점 반복 (gotcha #17)
            ])
            .expect("polygon with 4 vertices valid");
            // Caption::new on polygon (constructor #17)
            if let Control::Polygon { ref mut caption, ref mut style, .. } = poly {
                *caption = Some(make_caption("[그림 2] 의존성 방향 화살표", CaptionSide::Bottom));
                *style = Some(polygon_style);
            }
            poly
        },
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // line (convenience constructor #7) — 대각선 연결선
    paras.push(text_para("3.4 계층 간 연결", CS_SUBHEADING, PS_LEFT));
    let line_style = ShapeStyle {
        line_color: Some("#999999".to_string()),
        line_width: Some(28),
        ..ShapeStyle::default()
    };
    paras.push(ctrl_para(
        {
            let mut ln = Control::line(ShapePoint::new(0, 0), ShapePoint::new(14000, 5000))
                .expect("non-degenerate line valid");
            if let Control::Line { ref mut style, .. } = ln {
                *style = Some(line_style);
            }
            ln
        },
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "Foundation → Core → Blueprint → Smithy 순으로 의존성이 흐릅니다. 역방향 의존은 금지되어 있으며, 이를 통해 Foundation 수정 시 영향 범위를 예측할 수 있습니다.",
    ));
    paras.push(empty());
    paras.push(line_separator());

    // ColumnSettings::equal_columns (constructor #23) — 2단 레이아웃
    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.column_settings = Some(
        ColumnSettings::equal_columns(2, HwpUnit::from_mm(8.0).expect("8mm valid"))
            .expect("2 columns valid"),
    );
    sec
}

/// Section 3: 인코딩 파이프라인 + 결론
fn build_section_3() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("4. 인코딩/디코딩 파이프라인", CS_HEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "HwpForge의 인코딩/디코딩 파이프라인은 Core 문서 모델을 중심으로 양방향 변환을 수행합니다. 인코더는 Core+StyleStore → HWPX ZIP을 생성하고, 디코더는 HWPX ZIP → Core+StyleStore를 복원합니다.",
    ));
    paras.push(empty());

    // 테이블 with TableRow::with_height (constructor #15)
    paras.push(text_para("4.1 파이프라인 단계 비교", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    let cw = HwpUnit::new(14000).expect("col width valid");
    let rh = HwpUnit::from_mm(12.0).expect("12mm valid");
    let header_row = TableRow::new(vec![
        TableCell::new(vec![text_para("단계", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![text_para("인코드 (Core → HWPX)", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![text_para("디코드 (HWPX → Core)", CS_BOLD, PS_CENTER)], cw),
    ]);
    let row1 = TableRow::with_height(
        vec![
            TableCell::new(vec![text_para("1. 스타일", CS_BOLD, PS_CENTER)], cw),
            TableCell::new(vec![p("StyleStore → header.xml")], cw),
            TableCell::new(vec![p("header.xml → StyleStore")], cw),
        ],
        rh,
    );
    let row2 = TableRow::new(vec![
        TableCell::new(vec![text_para("2. 본문", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![p("Section[] → section0.xml")], cw),
        TableCell::new(vec![p("section0.xml → Section[]")], cw),
    ]);
    let row3 = TableRow::new(vec![
        TableCell::new(vec![text_para("3. 패키징", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![p("XML + 이미지 → ZIP")], cw),
        TableCell::new(vec![p("ZIP → XML + 이미지")], cw),
    ]);
    let row4 = TableRow::new(vec![
        TableCell::new(vec![text_para("4. 검증", CS_BOLD, PS_CENTER)], cw),
        TableCell::new(vec![p("한글에서 열기 확인")], cw),
        TableCell::new(vec![p("라운드트립 동일성 검증")], cw),
    ]);
    let mut pipeline_table = Table::new(vec![header_row, row1, row2, row3, row4]);
    pipeline_table.caption =
        Some(make_caption("[표 3] 인코드/디코드 파이프라인 단계 비교", CaptionSide::Top));
    paras.push(table_para(pipeline_table));
    paras.push(empty());

    // Chart (Bar stacked): 기능별 지원 현황
    paras.push(text_para("4.2 기능별 인코드/디코드 지원 현황", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    paras.push(chart_para(
        ChartType::Bar,
        ChartData::category(
            &["텍스트", "표", "이미지", "머리글", "각주", "글상자", "도형", "수식", "차트"],
            &[
                ("인코드", &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
                ("디코드", &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
            ],
        ),
        Some("HwpForge 기능 지원 현황"),
        LegendPosition::Right,
        ChartGrouping::Stacked,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 3] 기능별 인코드/디코드 지원 현황 (1=지원)", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // ChartData::xy (constructor #25) — Scatter chart: 파일 크기 vs 복잡도
    paras.push(text_para("4.3 파일 크기 vs 기능 복잡도", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    paras.push(chart_para(
        ChartType::Scatter,
        ChartData::xy(&[
            ("텍스트 전용", &[1.0, 2.0, 3.0], &[4.5, 5.2, 6.1]),
            ("표+이미지", &[3.0, 5.0, 7.0], &[12.0, 18.5, 25.0]),
            ("전체 기능", &[5.0, 8.0, 12.0], &[30.0, 48.0, 72.0]),
        ]),
        Some("기능 수 vs 파일 크기 (KB)"),
        LegendPosition::Bottom,
        ChartGrouping::Standard,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 4] 기능 복잡도에 따른 HWPX 파일 크기 추이", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // ── 결론 ──
    paras.push(text_para("4.4 결론", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "HwpForge는 4-Layer 아키텍처를 통해 구조(Core)와 스타일(Blueprint)을 분리하고, Smithy 계층에서 포맷별 인코딩/디코딩을 수행합니다.",
    ));
    paras.push(p(
        "Phase 0-5와 Wave 1-6을 거쳐 텍스트, 표, 이미지, 차트, 수식, 도형, 다단, 머리글/바닥글, 각주/미주, 글상자 등 실무 문서 생성에 필요한 모든 기능을 구현하였습니다.",
    ));
    paras.push(empty());

    // endnote_with_id (convenience constructor #12)
    paras.push(ctrl_para(
        Control::endnote_with_id(1, vec![p(
            "본 문서는 HwpForge Write API의 28개 편의 생성자를 모두 활용하여 작성되었습니다. 각 생성자는 최소 1회 이상 사용되었으며, 생성된 HWPX 파일은 한글에서 정상적으로 열립니다.",
        )]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));

    // endnote — zero-config (convenience constructor #4)
    paras.push(ctrl_para(
        Control::endnote(vec![p(
            "HWPX 포맷은 KS X 6101 표준에 기반하며, HwpForge는 이 표준의 완전한 구현을 목표로 합니다. Phase 6(Python 바인딩), Phase 7(MCP 서버), Phase 8(v1.0 릴리즈)이 남아 있습니다.",
        )]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    paras.push(line_separator());
    paras.push(empty());
    paras.push(text_para("— HwpForge 개발팀, 2026년 2월 —", CS_ITALIC, PS_CENTER));

    Section::with_paragraphs(paras, PageSettings::a4())
}

// ── Main ────────────────────────────────────────────────────────

fn main() {
    println!("=== HWPX 아키텍처 가이드 ===\n");

    // ── 1. Style Store ──
    // HwpxStyleStore::with_default_fonts (constructor #26)
    let mut style_store = HwpxStyleStore::with_default_fonts("함초롬돋움");

    // CS 0: Normal 10pt
    style_store.push_char_shape(HwpxCharShape::default());

    // CS 1: Bold 10pt
    let mut cs1 = HwpxCharShape::default();
    cs1.bold = true;
    style_store.push_char_shape(cs1);

    // CS 2: Italic 10pt, gray
    let mut cs2 = HwpxCharShape::default();
    cs2.italic = true;
    cs2.text_color = Color::from_rgb(102, 102, 102);
    style_store.push_char_shape(cs2);

    // CS 3: Title 20pt, navy bold (단독 paragraph)
    let mut cs3 = HwpxCharShape::default();
    cs3.height = HwpUnit::from_pt(20.0).expect("20pt valid");
    cs3.bold = true;
    cs3.text_color = Color::from_rgb(0, 51, 102);
    style_store.push_char_shape(cs3);

    // CS 4: Heading 14pt, bold (단독 paragraph)
    let mut cs4 = HwpxCharShape::default();
    cs4.height = HwpUnit::from_pt(14.0).expect("14pt valid");
    cs4.bold = true;
    style_store.push_char_shape(cs4);

    // CS 5: Subheading 11pt, dark gray bold (단독 paragraph)
    let mut cs5 = HwpxCharShape::default();
    cs5.height = HwpUnit::from_pt(11.0).expect("11pt valid");
    cs5.bold = true;
    cs5.text_color = Color::from_rgb(51, 51, 51);
    style_store.push_char_shape(cs5);

    // CS 6: Hyperlink — standard blue + underline
    let mut cs6 = HwpxCharShape::default();
    cs6.text_color = Color::from_rgb(5, 99, 193); // #0563C1 (standard hyperlink blue)
    cs6.underline_type = UnderlineType::Bottom;
    cs6.underline_color = Some(Color::from_rgb(5, 99, 193));
    style_store.push_char_shape(cs6);

    // PS 0: Left
    style_store.push_para_shape(HwpxParaShape::default());

    // PS 1: Center
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    style_store.push_para_shape(ps1);

    // PS 2: Justify
    let mut ps2 = HwpxParaShape::default();
    ps2.alignment = Alignment::Justify;
    style_store.push_para_shape(ps2);

    // PS 3: Right
    let mut ps3 = HwpxParaShape::default();
    ps3.alignment = Alignment::Right;
    style_store.push_para_shape(ps3);

    println!("[1] Style store: 7 fonts, 7 char shapes, 4 para shapes");

    // ── 2. Build Document ──
    // Document::new (constructor #27)
    let mut doc = Document::new();
    doc.add_section(build_section_0());
    doc.add_section(build_section_1());
    doc.add_section(build_section_2());
    doc.add_section(build_section_3());

    println!("[2] Document: {} sections", doc.sections().len());
    for (i, sec) in doc.sections().iter().enumerate() {
        println!(
            "    S{}: {} paras, h={}, f={}, pn={}, col={}",
            i + 1,
            sec.paragraphs.len(),
            sec.header.is_some(),
            sec.footer.is_some(),
            sec.page_number.is_some(),
            sec.column_settings.is_some(),
        );
    }

    // ── 3. Validate ──
    let validated = doc.validate().expect("document validation failed");
    println!("[3] Validation: OK");

    // ── 4. Encode ──
    let mut image_store = ImageStore::new();
    let mascot_bytes = std::fs::read("assets/mascot.png").expect("assets/mascot.png not found");
    image_store.insert("image1.png", mascot_bytes);

    let bytes =
        HwpxEncoder::encode(&validated, &style_store, &image_store).expect("HWPX encode failed");

    std::fs::create_dir_all("temp").ok();
    let path = "temp/architecture_guide.hwpx";
    std::fs::write(path, &bytes).expect("write to file failed");
    println!("[4] Encoded: {path} ({} bytes)", bytes.len());

    // ── 5. Roundtrip Decode ──
    let result = HwpxDecoder::decode(&bytes).expect("HWPX decode failed");
    let d = &result.document;
    println!("[5] Roundtrip decode: OK ({} sections)", d.sections().len());

    for (i, sec) in d.sections().iter().enumerate() {
        let tables =
            sec.paragraphs.iter().flat_map(|p| &p.runs).filter(|r| r.content.is_table()).count();
        let charts = sec
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .filter(|r| matches!(&r.content, RunContent::Control(c) if matches!(**c, Control::Chart { .. })))
            .count();
        let shapes = sec
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .filter(|r| matches!(&r.content, RunContent::Control(c) if matches!(**c,
                Control::Line { .. } | Control::Ellipse { .. } | Control::Polygon { .. } | Control::TextBox { .. }
            )))
            .count();
        println!(
            "    S{}: paras={}, tables={}, charts={}, shapes={}",
            i + 1,
            sec.paragraphs.len(),
            tables,
            charts,
            shapes,
        );
    }

    println!("\n=== HWPX 아키텍처 가이드 완료! ===");
    println!("한글에서 열어서 확인하세요: {path}");
}
