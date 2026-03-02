#![allow(clippy::vec_init_then_push)]
//! HWPX 포맷 분석 보고서 — HwpForge 프로젝트 기술 문서.
//!
//! full_report_2.hwpx의 내용을 재현하면서 HwpForge의 모든 구현 API를 활용합니다.
//!
//! **Section 0 — 표지 + 1장: HWPX 포맷 개요**
//! - 표지: 제목(20pt), 부제, 구분선(LINE), 작성자/날짜/문서번호
//! - 마스코트 이미지(오리너구리) + 캡션 + 소개글
//! - 1장: HWPX 설명 + 각주 + 테이블(ZIP 구조)
//! - Header / Footer / PageNumber
//!
//! **Section 1 — 2장: XML 구조 및 네임스페이스 체계**
//! - 테이블 x2 (네임스페이스, 파일 역할)
//! - 글상자 (코드 예시)
//! - 차트 x2 (Column: Phase별 LOC, Bar: 기능 지원)
//! - 각주
//!
//! **Section 2 — 3장: 구현 주의사항 (Gotchas)**
//! - 2단 레이아웃 (다단)
//! - 수식 (이차방정식 공식)
//! - 타원 + 캡션 (BGR 주의)
//! - 다각형(삼각형) + 캡션 (경고)
//! - 구분선(LINE)
//!
//! **Section 3 — 4장: 구현 현황 및 결론**
//! - 테이블 (Phase 현황 7x5)
//! - 차트 x2 (Line: 테스트 추이, Pie: LOC 비율)
//! - 미주
//! - 구분선(LINE)
//!
//! **API Coverage**: Text, Table x4, Image, Chart x4, Equation, Line x3,
//! Ellipse+Caption, Polygon+Caption, TextBox, 2-column, Footnote x2,
//! Endnote, Header, Footer, PageNumber, ShapeStyle = 17 API types
//!
//! # Usage
//! ```bash
//! cargo run --example full_report
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
            width: HwpUnit::new(w).unwrap(),
            height: HwpUnit::new(h).unwrap(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
        CS_NORMAL,
        PS_CENTER,
    )
}

/// LINE separator (horizontal, full width).
fn line_separator() -> Paragraph {
    ctrl_para(
        Control::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(42520, 0),
            width: HwpUnit::from_mm(150.0).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0x99, 0x99, 0x99)),
                fill_color: None,
                line_width: Some(28),
                line_style: None,
            }),
        },
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
    let w = HwpUnit::new(col_width).unwrap();

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

/// Section 0: 표지 + 1장 HWPX 포맷 개요
fn build_section_0() -> Section {
    let mut paras = Vec::new();

    // ── 표지 ──
    paras.push(text_para("HWPX 포맷 분석 보고서", CS_TITLE, PS_CENTER));
    paras.push(text_para("HwpForge 프로젝트 기술 문서", CS_SUBHEADING, PS_CENTER));
    paras.push(empty());
    paras.push(line_separator());
    paras.push(empty());
    paras.push(text_para("작성: HwpForge 개발팀", CS_NORMAL, PS_RIGHT));
    paras.push(text_para("작성일: 2026년 2월 27일 — 버전 1.0", CS_NORMAL, PS_RIGHT));
    paras.push(text_para("문서번호: HWPFORGE-2026-TECH-001", CS_NORMAL, PS_RIGHT));
    paras.push(empty());

    // 마스코트 이미지 + 캡션 (작은 아이콘 크기)
    let mut mascot_img = Image::from_path(
        "BinData/image1.png",
        HwpUnit::from_mm(35.0).unwrap(),
        HwpUnit::from_mm(35.0).unwrap(),
    );
    mascot_img.caption =
        Some(make_caption("[그림 1] HwpForge 마스코트 (오리너구리)", CaptionSide::Bottom));
    paras.push(Paragraph::with_runs(
        vec![Run::image(mascot_img, CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    ));
    paras.push(empty());

    // 오리너구리 소개글
    paras.push(mixed_para(
        &[
            ("HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 ", CS_NORMAL),
            ("순수 Rust 라이브러리", CS_BOLD),
            ("입니다. 프로젝트 마스코트인 ", CS_NORMAL),
            ("오리너구리(Platypus)", CS_BOLD),
            ("는 HWPX 포맷의 독특한 특성을 상징합니다 — 포유류이면서 알을 낳고, 부리와 독침을 가진 것처럼, HWPX도 XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(empty());
    paras.push(line_separator());
    paras.push(empty());

    // ── 1장: HWPX 포맷 개요 ──
    paras.push(text_para("1. HWPX 포맷 개요", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 본문 + 각주
    paras.push(mixed_para(
        &[
            ("HWPX는 한컴오피스(한글)가 사용하는 ", CS_NORMAL),
            ("개방형 XML 문서 포맷", CS_BOLD),
            ("으로, ", CS_NORMAL),
            ("KS X 6101", CS_BOLD),
        ],
        PS_JUSTIFY,
    ));
    paras.push(ctrl_para(
        Control::footnote_with_id(1, vec![
                p("KS X 6101은 한국산업표준(KS)으로 제정된 한글 문서 파일 포맷 규격입니다. 한국표준정보망(KSSN)을 통해 열람 가능하며, openhwp 프로젝트에 9,054줄 분량의 마크다운 사양이 공개되어 있습니다."),
            ]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(p(
        " 표준을 기반으로 합니다. 내부 구조는 ZIP 압축 아카이브 안에 XML 파일들을 계층적으로 배치한 형태입니다.",
    ));
    paras.push(p(
        "HWPX는 Microsoft Office의 OOXML과 유사한 구조를 가지며, ZIP → XML → 네임스페이스 계층으로 이루어져 있습니다. HwpForge는 이 포맷의 완전한 인코드/디코드를 순수 Rust로 구현하였습니다.",
    ));
    paras.push(empty());

    // 테이블 1: ZIP 구조
    paras.push(table_para(make_table(
        &["경로", "역할"],
        &[
            vec!["mimetype", "MIME 타입 선언"],
            vec!["version.xml", "HWPX 버전 정보"],
            vec!["Contents/header.xml", "스타일·글꼴·문단모양 정의"],
            vec!["Contents/section0.xml", "본문 섹션 XML"],
            vec!["Contents/content.hpf", "OPF 매니페스트"],
            vec!["META-INF/container.xml", "ODF 컨테이너 진입점"],
            vec!["BinData/image*.png", "이미지 바이너리"],
            vec!["Chart/chart*.xml", "차트 데이터 (manifest 등록 금지!)"],
        ],
        21000,
        Some("[표 1] HWPX ZIP 내부 파일 구조"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    paras.push(mixed_para(
        &[
            ("특히 ", CS_NORMAL),
            ("Chart/*.xml", CS_BOLD),
            (" 파일은 content.hpf에 등록하면 ", CS_NORMAL),
            ("한글이 크래시", CS_BOLD),
            ("합니다. ZIP에만 존재해야 합니다.", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(empty());

    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.header = Some(HeaderFooter::all_pages(vec![mixed_para(
        &[("HWPX 포맷 분석 보고서", CS_BOLD), ("  |  HwpForge 기술 문서 v1.0", CS_ITALIC)],
        PS_LEFT,
    )]));
    sec.footer = Some(HeaderFooter::all_pages(vec![text_para(
        "Copyright \u{00A9} 2026 HwpForge Project. Apache-2.0 / MIT",
        CS_ITALIC,
        PS_CENTER,
    )]));
    sec.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "- ",
    ));
    sec
}

/// Section 1: 2장 XML 구조 및 네임스페이스 체계
fn build_section_1() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("2. XML 구조 및 네임스페이스 체계", CS_HEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "HWPX의 XML은 6개의 네임스페이스 접두어로 구분됩니다. HwpForge는 serde의 rename 기능을 활용하여 각 네임스페이스를 Rust 구조체에 자동으로 매핑합니다.",
    ));
    paras.push(empty());

    // 2.1 네임스페이스
    paras.push(text_para("2.1 XML 네임스페이스 목록", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());

    paras.push(table_para(make_table(
        &["접두어", "URI", "주요 용도"],
        &[
            vec!["hh:", "urn:...hwpmlHead", "header.xml 스타일/글꼴"],
            vec!["hp:", "urn:...hwpmlPara", "문단, 표, 도형"],
            vec!["hc:", "urn:...hwpmlCore", "도형 기하 (startPt, pt)"],
            vec!["hs:", "urn:...hwpmlSect", "섹션 설정 (페이지, 다단)"],
            vec!["ha:", "urn:...hwpmlApp", "앱 설정"],
            vec!["hm:", "urn:...hwpmlMaster", "마스터 페이지"],
        ],
        14000,
        Some("[표 2] HWPX XML 네임스페이스 체계"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    // xmlns 래핑 + 각주
    paras.push(mixed_para(
        &[
            ("네임스페이스 선언(xmlns)은 각 XML 파일의 ", CS_NORMAL),
            ("루트 요소에만", CS_BOLD),
            (" 작성합니다.", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(ctrl_para(
        Control::footnote_with_id(2, vec![p(
                "HwpForge는 루트 요소를 수동으로 생성하고 내부 콘텐츠만 serde로 직렬화하는 'xmlns 래핑 패턴'을 사용합니다.",
            )]),
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // 2.2 파일별 역할
    paras.push(text_para("2.2 파일별 역할 상세", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());

    paras.push(table_para(make_table(
        &["파일", "루트 요소", "HwpForge 모듈"],
        &[
            vec!["Contents/header.xml", "hh:head", "encoder/header.rs"],
            vec!["Contents/section0.xml", "hs:sec", "encoder/section.rs"],
            vec!["Contents/content.hpf", "opf:package", "encoder/package.rs"],
            vec!["META-INF/container.xml", "container", "encoder/package.rs"],
            vec!["Chart/chart*.xml", "c:chartSpace", "encoder/chart.rs"],
        ],
        14000,
        Some("[표 3] HWPX 파일별 역할 및 HwpForge 모듈"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    // 글상자: serde rename 코드 예시
    paras.push(text_para("Dual Serde Rename 패턴:", CS_BOLD, PS_LEFT));
    paras.push(ctrl_para(
        Control::TextBox {
            paragraphs: vec![
                text_para(r#"#[serde(rename(serialize = "hh:refList","#, CS_NORMAL, PS_LEFT),
                text_para(r#"                deserialize = "refList"))]"#, CS_NORMAL, PS_LEFT),
                text_para("pub ref_list: HxRefList,", CS_ITALIC, PS_LEFT),
            ],
            width: HwpUnit::from_mm(120.0).unwrap(),
            height: HwpUnit::from_mm(20.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0xCC, 0xCC, 0xCC)),
                fill_color: Some(Color::from_rgb(0xF5, 0xF5, 0xF5)),
                line_width: None,
                line_style: None,
            }),
        },
        CS_NORMAL,
        PS_CENTER,
    ));
    paras.push(empty());

    // 2.3 Phase별 코드량
    paras.push(text_para("2.3 Phase별 구현 규모 (코드 라인 수)", CS_SUBHEADING, PS_LEFT));
    paras.push(p("아래 차트는 HwpForge의 Phase 0-5에 걸친 소스 코드 증가 추이를 보여줍니다."));
    paras.push(empty());

    // Chart 1: Column — Phase별 LOC
    paras.push(chart_para(
        ChartType::Column,
        ChartData::category(
            &["Foundation", "Core", "Blueprint", "Decoder", "Encoder", "smithy-md"],
            &[("LOC", &[4432.0, 5554.0, 4647.0, 3666.0, 10349.0, 3757.0])],
        ),
        Some("Phase별 소스 코드 라인 수"),
        LegendPosition::Bottom,
        ChartGrouping::Clustered,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 1] Phase별 소스 코드 라인 수 (Phase 0-5)", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // Chart 2: Bar stacked — 기능 지원
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
    paras.push(text_para("[차트 2] 기능별 인코드/디코드 지원 현황", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    Section::with_paragraphs(paras, PageSettings::a4())
}

/// Section 2: 3장 구현 주의사항 (Gotchas) — 2단 레이아웃
fn build_section_2() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("3. 구현 주의사항 (Gotchas)", CS_HEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p("HwpForge 구현 과정에서 발견한 핵심 주의사항을 정리합니다."));
    paras.push(empty());

    // ── 3.1 XML 인코딩 (왼쪽 단에 배치) ──
    paras.push(text_para("3.1 XML 인코딩 주의사항", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());

    // (1) BGR
    paras.push(text_para("(1) BGR 색상 순서", CS_BOLD, PS_LEFT));
    paras.push(mixed_para(
        &[
            ("HWP/HWPX 포맷은 ", CS_NORMAL),
            ("BGR (Blue-Green-Red)", CS_BOLD),
            (" 바이트 순서를 사용합니다. 빨강(Red)을 표현하려면 ", CS_NORMAL),
            ("0x0000FF", CS_BOLD),
            ("로 저장해야 합니다 (RGB에서는 0xFF0000). RGB로 착각하면 파란색이 됩니다.", CS_NORMAL),
        ],
        PS_JUSTIFY,
    ));
    paras.push(p(
        "HwpForge는 Color::from_rgb(r, g, b) 생성자를 통해 내부적으로 BGR 변환을 처리합니다. 절대로 raw 16진수 값을 직접 사용하지 마십시오.",
    ));
    paras.push(empty());

    // (2) ctrl 순서
    paras.push(text_para("(2) ctrl 요소 순서 규칙", CS_BOLD, PS_LEFT));
    paras.push(p(
        "hp:sec (섹션) 내부의 ctrl 요소는 반드시 secPr → colPr → header → footer → pageNum 순서로 배치해야 합니다. 순서가 틀리면 한글이 섹션 설정을 무시하거나 크래시합니다.",
    ));
    paras.push(empty());

    // (3) hc: namespace
    paras.push(text_para("(3) 도형 기하에는 hc: 네임스페이스", CS_BOLD, PS_LEFT));
    paras.push(mixed_para(
        &[
            ("선(Line) 도형의 ", CS_NORMAL),
            ("startPt/endPt", CS_BOLD),
            ("와 다각형(Polygon)의 ", CS_NORMAL),
            ("pt", CS_BOLD),
            (" 요소는 반드시 ", CS_NORMAL),
            ("hc:", CS_BOLD),
            (
                " 네임스페이스를 사용해야 합니다. hp:를 사용하면 한글에서 파싱 오류가 발생합니다.",
                CS_NORMAL,
            ),
        ],
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // (4) HwpUnit
    paras.push(text_para("(4) HwpUnit 단위계", CS_BOLD, PS_LEFT));
    paras.push(p(
        "HWPX의 모든 길이 단위는 HWPUNIT으로, 1pt = 100 HWPUNIT, 1mm ≈ 283 HWPUNIT 관계를 가집니다. HwpUnit::from_pt() 또는 HwpUnit::from_mm()로 변환합니다.",
    ));
    paras.push(empty());

    // (5) Chart manifest 금지
    paras.push(text_para("(5) Chart XML manifest 등록 금지", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Chart/*.xml 파일은 ZIP 아카이브에만 존재해야 하며, content.hpf 매니페스트에 등록하면 한글이 크래시합니다. 또한 차트 데이터의 <c:f> formula 참조가 없으면 빈 차트가 표시됩니다.",
    ));
    paras.push(empty());

    // (6) TextBox는 Control이 아님
    paras.push(text_para("(6) TextBox는 hp:rect 구조", CS_BOLD, PS_LEFT));
    paras.push(p(
        "HWPX에서 글상자(TextBox)는 <hp:rect> + <hp:drawText> 구조입니다. Control 요소가 아니며, 도형의 일종으로 처리해야 합니다. 꼭짓점(pt0~pt3)은 hc: 네임스페이스를 사용합니다.",
    ));
    paras.push(empty());

    // ── 3.2 도형 및 수식 (오른쪽 단에 배치) ──
    paras.push(text_para("3.2 도형 및 수식 주의사항", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());

    // (7) 수식
    paras.push(text_para("(7) 수식: HancomEQN 포맷", CS_BOLD, PS_LEFT));
    paras.push(p(
        "HWPX 수식은 MathML이 아닌 HancomEQN 스크립트를 사용합니다. 수식에는 shape common 블록이 없습니다 (offset, orgSz, curSz 등 없음). flowWithText=1, outMargin=56이 기본값입니다.",
    ));
    paras.push(ctrl_para(
        Control::equation("x= {-b +-  root {2} of {b ^{2} -4ac}} over {2a}"),
        CS_NORMAL,
        PS_CENTER,
    ));
    paras.push(empty());

    // (6) 타원 — BGR 주의
    paras.push(text_para("(6) 타원 도형 — 주의 영역", CS_BOLD, PS_LEFT));
    let ew = HwpUnit::from_mm(30.0).unwrap().as_i32();
    let eh = HwpUnit::from_mm(20.0).unwrap().as_i32();
    paras.push(ctrl_para(
        Control::Ellipse {
            center: ShapePoint::new(ew / 2, eh / 2),
            axis1: ShapePoint::new(ew, eh / 2),
            axis2: ShapePoint::new(ew / 2, eh),
            width: HwpUnit::new(ew).unwrap(),
            height: HwpUnit::new(eh).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![text_para("BGR!", CS_BOLD, PS_CENTER)],
            caption: Some(make_caption("[그림 2] BGR 색상 주의 영역", CaptionSide::Right)),
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0xFF, 0x00, 0x00)),
                fill_color: Some(Color::from_rgb(0xFF, 0xEE, 0xEE)),
                line_width: Some(56),
                line_style: None,
            }),
        },
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // (8) 타원의 기하 구조
    paras.push(p(
        "타원은 center, axis1, axis2 세 점으로 정의됩니다. 정원(circle)은 width=height로 설정합니다. shadow alpha는 도형별로 다릅니다: rect=178, line=0, ellipse=0.",
    ));
    paras.push(empty());

    // (9) 다각형 — 경고 삼각형
    paras.push(text_para("(9) 다각형 — 첫 점 반복 필수", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Polygon 꼭짓점 마지막에 첫 점을 반복해야 닫힌 도형이 됩니다. 반복하지 않으면 한글에서 마지막 변이 표시되지 않습니다.",
    ));
    let tw = HwpUnit::from_mm(25.0).unwrap().as_i32();
    let th = HwpUnit::from_mm(22.0).unwrap().as_i32();
    paras.push(ctrl_para(
        Control::Polygon {
            vertices: vec![
                ShapePoint::new(tw / 2, 0),
                ShapePoint::new(tw, th),
                ShapePoint::new(0, th),
                ShapePoint::new(tw / 2, 0), // 첫 점 반복!
            ],
            width: HwpUnit::new(tw).unwrap(),
            height: HwpUnit::new(th).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![text_para("주의!", CS_BOLD, PS_CENTER)],
            caption: Some(make_caption("[그림 3] 경고 삼각형 (첫 점 반복)", CaptionSide::Bottom)),
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0xFF, 0x66, 0x00)),
                fill_color: Some(Color::from_rgb(0xFF, 0xF3, 0xE0)),
                line_width: Some(42),
                line_style: None,
            }),
        },
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // (10) shapeComment 필수
    paras.push(text_para("(10) shapeComment 필수", CS_BOLD, PS_LEFT));
    paras.push(p(
        "모든 도형에는 <hp:shapeComment> 요소가 필수입니다. 사각형은 '사각형입니다.', 선은 '선입니다.', 타원은 '타원입니다.' 등 형태별 고정 문자열을 사용합니다.",
    ));
    paras.push(empty());

    // (11) serde 필드 순서 = XML 순서
    paras.push(text_para("(11) serde 필드 순서", CS_BOLD, PS_LEFT));
    paras.push(p(
        "quick-xml의 serde 직렬화에서 Rust 구조체의 필드 선언 순서가 XML 요소 순서를 결정합니다. 한글은 요소 순서에 민감하므로 golden fixture와 동일한 순서를 유지해야 합니다.",
    ));
    paras.push(empty());
    paras.push(line_separator());

    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.column_settings =
        Some(ColumnSettings::equal_columns(2, HwpUnit::from_mm(8.0).unwrap()).unwrap());
    sec
}

/// Section 3: 4장 구현 현황 및 결론
fn build_section_3() -> Section {
    let mut paras = Vec::new();

    paras.push(text_para("4. 구현 현황 및 결론", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // 4.1 Phase 현황
    paras.push(text_para("4.1 Phase별 구현 현황", CS_SUBHEADING, PS_LEFT));
    paras.push(p("HwpForge v1.0 기준 총 37,052 LOC, 988개 테스트, 8개 크레이트입니다."));
    paras.push(empty());

    paras.push(table_para(make_table(
        &["Phase", "크레이트", "상태", "테스트", "LOC"],
        &[
            vec!["0", "foundation", "완료 (90+)", "224", "4,432"],
            vec!["1", "core", "완료 (94)", "331", "5,554"],
            vec!["2", "blueprint", "완료 (90)", "200", "4,647"],
            vec!["3", "decoder", "완료 (96)", "110", "3,666"],
            vec!["4", "encoder", "완료 (95)", "226", "10,349"],
            vec!["5", "smithy-md", "완료 (91)", "73", "3,757"],
            vec!["Wave1-6", "확장 기능", "완료", "—", "~4,648"],
        ],
        8400,
        Some("[표 4] HwpForge Phase별 구현 현황"),
        CaptionSide::Top,
    )));
    paras.push(empty());

    // 4.2 테스트 추이
    paras.push(text_para("4.2 테스트 수 성장 추이", CS_SUBHEADING, PS_LEFT));
    paras.push(p("TDD 방식으로 개발되어 Phase 진행에 따라 테스트가 꾸준히 증가했습니다."));
    paras.push(empty());

    // Chart 3: Line
    paras.push(chart_para(
        ChartType::Line,
        ChartData::category(
            &["P0", "P1", "P2", "P3", "P4", "P5", "Wave"],
            &[("누적 테스트", &[224.0, 555.0, 755.0, 865.0, 905.0, 978.0, 988.0])],
        ),
        Some("Phase별 누적 테스트 수 추이"),
        LegendPosition::Bottom,
        ChartGrouping::Standard,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 3] Phase별 누적 테스트 수 추이", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // Chart 4: Pie — LOC 비율
    paras.push(chart_para(
        ChartType::Pie,
        ChartData::category(
            &["foundation", "core", "blueprint", "smithy-hwpx", "smithy-md", "Wave"],
            &[("LOC", &[4432.0, 5554.0, 4647.0, 14015.0, 3757.0, 4648.0])],
        ),
        Some("크레이트별 LOC 비율"),
        LegendPosition::Right,
        ChartGrouping::Clustered,
        42520,
        21000,
    ));
    paras.push(text_para("[차트 4] 크레이트별 코드량(LOC) 비율", CS_ITALIC, PS_CENTER));
    paras.push(empty());

    // 4.3 향후 과제
    paras.push(text_para("4.3 향후 과제", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    for (phase, desc) in [
        ("Phase 6", ": Python 바인딩 (PyO3) 및 CLI"),
        ("Phase 7", ": MCP 서버 통합 — Claude Code 직접 연동"),
        ("Phase 8", ": 종합 테스트 및 v1.0 릴리즈"),
        ("Phase 9", ": HWPX 고급 기능 (OLE, 양식, 변경추적)"),
        ("Phase 10", ": smithy-hwp5 — HWP5 바이너리 읽기"),
    ] {
        paras.push(mixed_para(&[(phase, CS_BOLD), (desc, CS_NORMAL)], PS_JUSTIFY));
    }
    paras.push(empty());

    // 4.4 결론
    paras.push(text_para("4.4 결론", CS_SUBHEADING, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "HwpForge는 순수 Rust로 HWPX 포맷의 완전한 인코드/디코드를 구현한 최초의 오픈소스 프로젝트입니다.",
    ));
    paras.push(p(
        "Wave 1-6에서는 이미지, 머리글/바닥글, 각주/미주, 글상자, 다단, 도형, 캡션, 수식, 차트까지 확장하여 실무 문서 생성에 필요한 대부분의 기능을 갖추었습니다.",
    ));
    // 미주
    paras.push(ctrl_para(
        Control::endnote_with_id(1, vec![p(
                "본 보고서는 HwpForge의 모든 구현 API를 활용하여 작성되었습니다. 텍스트, 표, 이미지, 차트, 수식, 도형, 다단, 머리글/바닥글, 각주/미주, 글상자 등 17개 API 유형을 포함합니다.",
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
    println!("=== HWPX 포맷 분석 보고서 ===\n");

    // ── 1. Style Store ──
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
    cs3.height = HwpUnit::from_pt(20.0).unwrap();
    cs3.bold = true;
    cs3.text_color = Color::from_rgb(0, 51, 102);
    style_store.push_char_shape(cs3);

    // CS 4: Heading 14pt, bold (단독 paragraph)
    let mut cs4 = HwpxCharShape::default();
    cs4.height = HwpUnit::from_pt(14.0).unwrap();
    cs4.bold = true;
    style_store.push_char_shape(cs4);

    // CS 5: Subheading 11pt, dark gray bold (단독 paragraph)
    let mut cs5 = HwpxCharShape::default();
    cs5.height = HwpUnit::from_pt(11.0).unwrap();
    cs5.bold = true;
    cs5.text_color = Color::from_rgb(51, 51, 51);
    style_store.push_char_shape(cs5);

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

    println!("[1] Style store: 7 fonts, 6 char shapes, 4 para shapes");

    // ── 2. Build Document ──
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
    let validated = doc.validate().expect("validation failed");
    println!("[3] Validation: OK");

    // ── 4. Encode ──
    let mut image_store = ImageStore::new();
    let mascot_bytes = std::fs::read("assets/mascot.png").expect("assets/mascot.png not found");
    image_store.insert("image1.png", mascot_bytes);

    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode failed");

    std::fs::create_dir_all("temp").ok();
    let path = "temp/full_report.hwpx";
    std::fs::write(path, &bytes).expect("write failed");
    println!("[4] Encoded: {path} ({} bytes)", bytes.len());

    // ── 5. Roundtrip Decode ──
    let result = HwpxDecoder::decode(&bytes).expect("decode failed");
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

    println!("\n=== HWPX 포맷 분석 보고서 완료! ===");
    println!("한글에서 열어서 확인하세요: {path}");
}
