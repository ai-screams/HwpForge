//! HWPX 문서 구조 완전 가이드 — HwpForge 전체 API 데모
//!
//! HWPX 문서 포맷의 내부 구조를 설명하는 기술 참조 문서를 생성합니다.
//! 문서 자체가 HWPX 포맷에 대한 해설이며, 동시에 HwpForge의 모든 API를
//! 하나의 예제에서 시연합니다.
//!
//! 4개 섹션 구성:
//!   1. HWPX 문서 구조 개요 (A4 세로, 머리글/바닥글/페이지번호)
//!   2. 텍스트 서식 시스템 (A4 세로, visibility/줄번호)
//!   3. 도형과 그래픽 요소 (A4 가로, 다단)
//!   4. 차트, 수식, 고급 기능 (A4 세로, 마스터페이지)
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example hwpx_complete_guide
//!
//! Output:
//!   temp/hwpx_complete_guide.hwpx

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{
    ArrowStyle, Control, DutmalAlign, DutmalPosition, Fill, LineStyle, ShapePoint, ShapeStyle,
};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageStore};
use hwpforge_core::numbering::NumberingDef;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{
    BeginNum, HeaderFooter, LineNumberShape, MasterPage, PageBorderFillEntry, PageNumber, Section,
    Visibility,
};
use hwpforge_core::tab::TabDef;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, ArcType, ArrowSize, ArrowType, CharShapeIndex, Color,
    CurveSegmentType, FieldType, Flip, GradientType, GutterType, HwpUnit, NumberFormatType,
    PageNumberPosition, ParaShapeIndex, PatternType, RefContentType, RefType, ShowMode, StyleIndex,
};
use hwpforge_smithy_hwpx::style_store::{
    HwpxBorderFill, HwpxBorderLine, HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ═══════════════════════════════════════════════════════════════════════════
// CharShape 인덱스 상수
// ═══════════════════════════════════════════════════════════════════════════

const CS_NORMAL: usize = 0; // 10pt 기본
const CS_TITLE: usize = 1; // 18pt 굵은 제목
const CS_HEADING: usize = 2; // 14pt 굵은 파랑 (소제목)
const CS_SMALL: usize = 3; // 8pt 작은 글씨 (캡션/각주)
const CS_RED_BOLD: usize = 4; // 10pt 빨강 굵게 (강조)
const CS_BLUE: usize = 5; // 10pt 파랑 (링크/코드)
const CS_GREEN_ITALIC: usize = 6; // 10pt 녹색 기울임
const CS_GRAY: usize = 7; // 10pt 회색 (워터마크)

// ═══════════════════════════════════════════════════════════════════════════
// ParaShape 인덱스 상수
// ═══════════════════════════════════════════════════════════════════════════

const PS_BODY: usize = 0; // 양쪽정렬, 160%
const PS_CENTER: usize = 1; // 가운데정렬
const PS_LEFT: usize = 2; // 왼쪽정렬
const PS_RIGHT: usize = 3; // 오른쪽정렬
const PS_DISTRIBUTE: usize = 4; // 배분정렬

// ═══════════════════════════════════════════════════════════════════════════
// 헬퍼 함수
// ═══════════════════════════════════════════════════════════════════════════

/// 단일 텍스트 문단
fn p(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

/// 빈 줄
fn empty() -> Paragraph {
    p("", CS_NORMAL, PS_BODY)
}

/// 컨트롤 단독 문단
fn ctrl_p(ctrl: Control, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

/// 다중 Run 문단
fn runs_p(runs: Vec<Run>, ps: usize) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(ps))
}

/// 스타일 적용 문단
fn styled_p(text: &str, cs: usize, ps: usize, style_id: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
        .with_style(StyleIndex::new(style_id))
}

/// CharShapeIndex 단축
fn csi(idx: usize) -> CharShapeIndex {
    CharShapeIndex::new(idx)
}

/// 텍스트 셀 (단일 텍스트)
fn text_cell(text: &str, width_mm: f64, cs: usize, ps: usize) -> TableCell {
    TableCell::new(vec![p(text, cs, ps)], HwpUnit::from_mm(width_mm).unwrap())
}

/// 배경색 텍스트 셀
fn colored_cell(text: &str, width_mm: f64, cs: usize, ps: usize, r: u8, g: u8, b: u8) -> TableCell {
    let mut cell = text_cell(text, width_mm, cs, ps);
    cell.background = Some(Color::from_rgb(r, g, b));
    cell
}

// ═══════════════════════════════════════════════════════════════════════════
// 스타일 스토어 구성
// ═══════════════════════════════════════════════════════════════════════════

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    // ── 폰트 ──────────────────────────────────────────────────────
    store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
    store.push_font(HwpxFont::new(1, "함초롬바탕", "HANGUL"));

    // ── CharShape 0: 기본 10pt ────────────────────────────────────
    store.push_char_shape(HwpxCharShape::default());

    // ── CharShape 1: 제목 18pt 굵게 ──────────────────────────────
    let mut cs1 = HwpxCharShape::default();
    cs1.height = HwpUnit::new(1800).unwrap();
    cs1.bold = true;
    store.push_char_shape(cs1);

    // ── CharShape 2: 소제목 14pt 굵게 파랑 ──────────────────────
    let mut cs2 = HwpxCharShape::default();
    cs2.height = HwpUnit::new(1400).unwrap();
    cs2.bold = true;
    cs2.text_color = Color::from_rgb(0, 70, 160);
    store.push_char_shape(cs2);

    // ── CharShape 3: 작은 8pt ─────────────────────────────────────
    let mut cs3 = HwpxCharShape::default();
    cs3.height = HwpUnit::new(800).unwrap();
    store.push_char_shape(cs3);

    // ── CharShape 4: 빨강 굵게 (강조) ────────────────────────────
    let mut cs4 = HwpxCharShape::default();
    cs4.text_color = Color::from_rgb(200, 30, 30);
    cs4.bold = true;
    store.push_char_shape(cs4);

    // ── CharShape 5: 파랑 (링크/코드) ────────────────────────────
    let mut cs5 = HwpxCharShape::default();
    cs5.text_color = Color::from_rgb(30, 30, 200);
    store.push_char_shape(cs5);

    // ── CharShape 6: 녹색 기울임 ─────────────────────────────────
    let mut cs6 = HwpxCharShape::default();
    cs6.text_color = Color::from_rgb(0, 130, 60);
    cs6.italic = true;
    store.push_char_shape(cs6);

    // ── CharShape 7: 회색 (워터마크) ─────────────────────────────
    let mut cs7 = HwpxCharShape::default();
    cs7.text_color = Color::from_rgb(180, 180, 180);
    cs7.height = HwpUnit::new(1400).unwrap();
    store.push_char_shape(cs7);

    // ── ParaShape 0: 본문 (양쪽정렬 160%) ────────────────────────
    let mut ps0 = HwpxParaShape::default();
    ps0.alignment = Alignment::Justify;
    ps0.line_spacing = 160;
    store.push_para_shape(ps0);

    // ── ParaShape 1: 가운데 ──────────────────────────────────────
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    ps1.line_spacing = 160;
    store.push_para_shape(ps1);

    // ── ParaShape 2: 왼쪽 ────────────────────────────────────────
    let mut ps2 = HwpxParaShape::default();
    ps2.alignment = Alignment::Left;
    ps2.line_spacing = 160;
    store.push_para_shape(ps2);

    // ── ParaShape 3: 오른쪽 ──────────────────────────────────────
    let mut ps3 = HwpxParaShape::default();
    ps3.alignment = Alignment::Right;
    ps3.line_spacing = 160;
    store.push_para_shape(ps3);

    // ── ParaShape 4: 배분정렬 ────────────────────────────────────
    let mut ps4 = HwpxParaShape::default();
    ps4.alignment = Alignment::Distribute;
    ps4.line_spacing = 160;
    store.push_para_shape(ps4);

    // ── BorderFill 1: 기본 페이지 테두리 (없음) ──────────────────
    store.push_border_fill(HwpxBorderFill::default_page_border());

    // ── BorderFill 2: 기본 글자 배경 (없음) ──────────────────────
    store.push_border_fill(HwpxBorderFill::default_char_background());

    // ── BorderFill 3: 표 테두리 (검은 실선) ──────────────────────
    store.push_border_fill(HwpxBorderFill::default_table_border());

    // ── BorderFill 4: 빨간 실선 테두리 (페이지 테두리용) ─────────
    let mut bf4 = HwpxBorderFill::default_page_border();
    bf4.id = 4;
    let red_line = HwpxBorderLine {
        line_type: "SOLID".into(),
        width: "0.4 mm".into(),
        color: "#FF0000".into(),
    };
    bf4.left = red_line.clone();
    bf4.right = red_line.clone();
    bf4.top = red_line.clone();
    bf4.bottom = red_line;
    store.push_border_fill(bf4);

    // ── 개요 번호매기기 ──────────────────────────────────────────
    store.push_numbering(NumberingDef::default_outline());

    // ── 탭 속성 ──────────────────────────────────────────────────
    for tab in TabDef::defaults() {
        store.push_tab(tab);
    }

    store
}

// ═══════════════════════════════════════════════════════════════════════════
// 섹션 1: HWPX 문서 구조 개요
// ═══════════════════════════════════════════════════════════════════════════

fn section1_document_structure() -> Section {
    // 머리글/바닥글/페이지번호가 있는 A4 세로

    let mut paras: Vec<Paragraph> = vec![
        // ── 제목 ──
        styled_p(
            "HWPX 문서 구조 완전 가이드",
            CS_TITLE,
            PS_CENTER,
            0, // 바탕 스타일
        ),
        empty(),
    ];

    // ── 마스코트 이미지 + 캡션 ──
    let mut mascot_img = Image::from_path(
        "BinData/image1.png",
        HwpUnit::from_mm(35.0).unwrap(),
        HwpUnit::from_mm(35.0).unwrap(),
    );
    mascot_img.caption = Some(Caption::new(
        vec![p("[그림 1] HwpForge 마스코트 (오리너구리)", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::image(mascot_img, csi(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    ));
    paras.push(empty());

    // ── 오리너구리 소개글 ──
    paras.push(runs_p(
        vec![
            Run::text("HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 ", csi(CS_NORMAL)),
            Run::text("순수 Rust 라이브러리", csi(CS_RED_BOLD)),
            Run::text("입니다. 프로젝트 마스코트인 ", csi(CS_NORMAL)),
            Run::text("오리너구리(Platypus)", csi(CS_RED_BOLD)),
            Run::text(
                "는 HWPX 포맷의 독특한 특성을 상징합니다 — XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.",
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 북마크: HWPX정의 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("HWPX정의"), csi(CS_NORMAL)),
            Run::control(Control::index_mark("HWPX"), csi(CS_NORMAL)),
            Run::text("1. HWPX 문서 포맷이란?", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    // ── HWPX 설명 + 하이퍼링크 + 각주 ──
    paras.push(runs_p(
        vec![
            Run::text("HWPX는 대한민국 국가표준 ", csi(CS_NORMAL)),
            Run::control(Control::index_mark("KS X 6101"), csi(CS_NORMAL)),
            Run::text("KS X 6101", csi(CS_BLUE)),
            Run::control(
                Control::footnote(vec![p(
                    "KS X 6101: 한국산업표준(Korean Industrial Standards)에서 제정한 문서 파일 형식 표준. 2014년 최초 제정, 2021년 개정.",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(
                "에 정의된 XML 기반 문서 포맷입니다. ZIP 컨테이너 안에 여러 XML 파일이 구조화되어 저장됩니다.",
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 하이퍼링크 ──
    paras.push(runs_p(
        vec![
            Run::text("상세 사양은 ", csi(CS_NORMAL)),
            Run::control(
                Control::hyperlink("한국정보통신기술협회(TTA)", "https://www.tta.or.kr"),
                csi(CS_BLUE),
            ),
            Run::text(" 홈페이지에서 확인할 수 있습니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 북마크: 헤더구조 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("헤더구조"), csi(CS_NORMAL)),
            Run::text("2. ZIP 컨테이너 파일 구성", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p(
        "HWPX 파일은 확장자가 .hwpx인 ZIP 아카이브입니다. 내부에는 다음과 같은 XML 파일들이 포함됩니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 표: ZIP 파일 구성 ──
    let table_width = HwpUnit::from_mm(170.0).unwrap();
    let col_w1 = 55.0; // 파일명
    let col_w2 = 60.0; // 설명
    let col_w3 = 55.0; // 미디어타입

    // 헤더 행 (파란 배경)
    let header_row = TableRow::new(vec![
        colored_cell("파일 경로", col_w1, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
        colored_cell("설명", col_w2, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
        colored_cell("Media-Type", col_w3, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
    ]);

    // 데이터 행
    let row1 = TableRow::new(vec![
        text_cell("META-INF/manifest.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("패키지 매니페스트 (파일 목록)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("text/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row2 = TableRow::new(vec![
        text_cell("Contents/content.hpf", col_w1, CS_SMALL, PS_LEFT),
        text_cell("콘텐츠 목차 (OPF)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row3 = TableRow::new(vec![
        text_cell("Contents/header.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("스타일 정의 (폰트, 문단, 글자)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row4 = TableRow::new(vec![
        text_cell("Contents/section0.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("본문 첫 번째 구획 (paragraphs)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row5 = TableRow::new(vec![
        text_cell("Contents/section1.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("본문 두 번째 구획 (선택적)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    // col_span 행: BinData 설명
    let mut bindata_cell = text_cell(
        "BinData/ — 이미지, OLE 등 바이너리 데이터 폴더 (Content.hpf에 등록, Chart XML은 미등록)",
        col_w1 + col_w2 + col_w3,
        CS_GREEN_ITALIC,
        PS_LEFT,
    );
    bindata_cell.col_span = 3;
    let row6 = TableRow::new(vec![bindata_cell]);

    let mut tbl = Table::new(vec![header_row, row1, row2, row3, row4, row5, row6]);
    tbl.width = Some(table_width);
    tbl.caption = Some(Caption::new(
        vec![p("표 1. HWPX ZIP 컨테이너 내부 파일 구성", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));

    paras.push(runs_p(vec![Run::table(tbl, csi(CS_NORMAL))], PS_CENTER));
    paras.push(empty());

    // ── 북마크: 섹션구조 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("섹션구조"), csi(CS_NORMAL)),
            Run::control(
                Control::IndexMark {
                    primary: "OWPML".to_string(),
                    secondary: Some("섹션 구조".to_string()),
                },
                csi(CS_NORMAL),
            ),
            Run::text("3. 섹션(Section) 구조", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p(
        "HWPX 문서는 하나 이상의 섹션으로 구성됩니다. 각 섹션은 독립적인 페이지 설정(용지 크기, 여백, 방향)을 가질 수 있어, 세로 페이지와 가로 페이지를 하나의 문서에 혼합할 수 있습니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    paras.push(p(
        "각 섹션의 XML은 <hp:sec> 루트 아래 <hp:p>(문단) 요소들로 구성됩니다. 문단 안에는 <hp:run>(텍스트 런), <hp:ctrl>(컨트롤), <hp:tbl>(표) 등이 포함됩니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 제목 4 ──
    paras.push(p("4. header.xml 스타일 시스템", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("header.xml에는 문서 전체의 스타일 정의가 담깁니다: ", csi(CS_NORMAL)),
            Run::text("fontface(폰트)", csi(CS_RED_BOLD)),
            Run::text(", ", csi(CS_NORMAL)),
            Run::text("charShape(글자 모양)", csi(CS_RED_BOLD)),
            Run::text(", ", csi(CS_NORMAL)),
            Run::text("paraShape(문단 모양)", csi(CS_RED_BOLD)),
            Run::text(". 본문의 각 요소는 인덱스(IDRef)로 이 정의를 참조합니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 각주 추가 설명 ──
    paras.push(runs_p(
        vec![
            Run::text(
                "스타일 정의 인덱스는 0부터 시작하며, Modern 스타일셋 기준으로 기본 charShape 7개, paraShape 20개가 자동 생성됩니다",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote(vec![p(
                    "한글 2022(Modern 스타일셋)의 기본 스타일: charShape 0-6 (바탕~개요10), paraShape 0-19 (바탕~개요10). 사용자 정의 스타일은 이후 인덱스부터 시작합니다.",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    paras.push(p(
        "이 문서는 HwpForge 라이브러리로 생성되었으며, 4개 섹션에 걸쳐 문서 포맷의 각 요소를 실제로 사용하면서 설명합니다.",
        CS_GREEN_ITALIC,
        PS_BODY,
    ));

    // ── 섹션 구성 ──
    let mut section = Section::with_paragraphs(paras, PageSettings::a4());

    // 머리글: 모든 페이지
    section.header = Some(HeaderFooter::all_pages(vec![p(
        "HWPX 문서 구조 완전 가이드 — HwpForge",
        CS_SMALL,
        PS_CENTER,
    )]));

    // 바닥글: 모든 페이지
    section.footer = Some(HeaderFooter::all_pages(vec![p(
        "Copyright 2026 HwpForge Project. All rights reserved.",
        CS_SMALL,
        PS_CENTER,
    )]));

    // 페이지 번호: 하단 가운데, "- N -" 형식
    section.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "- ",
    ));

    section
}

// ═══════════════════════════════════════════════════════════════════════════
// 섹션 2: 텍스트 서식 시스템
// ═══════════════════════════════════════════════════════════════════════════

fn section2_text_formatting() -> Section {
    let mut paras: Vec<Paragraph> = vec![
        // ── 제목 ──
        p("텍스트 서식 시스템", CS_TITLE, PS_CENTER),
        empty(),
        // ── 정렬 데모 ──
        p("1. 문단 정렬 (Paragraph Alignment)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "양쪽 정렬(Justify): 본문에서 가장 일반적으로 사용되는 정렬입니다. 양쪽 여백에 맞춰 글자 간격이 자동 조절됩니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        p(
            "가운데 정렬(Center): 제목이나 캡션에 주로 사용합니다.",
            CS_NORMAL,
            PS_CENTER,
        ),
        p(
            "왼쪽 정렬(Left): 코드나 목록에 적합합니다.",
            CS_NORMAL,
            PS_LEFT,
        ),
        p(
            "오른쪽 정렬(Right): 날짜, 서명 등에 사용합니다.",
            CS_NORMAL,
            PS_RIGHT,
        ),
        p(
            "배분 정렬(Distribute): 글자를 균등하게 분배합니다.",
            CS_NORMAL,
            PS_DISTRIBUTE,
        ),
        empty(),
        // ── 덧말(Dutmal) 데모 ──
        p("2. 덧말 (Dutmal / Ruby Text)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "덧말은 한자 위나 아래에 한글 읽기를 표시하는 기능입니다:",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
    ];

    // 위쪽 덧말
    let dutmal_top = Control::dutmal("大韓民國", "대한민국");
    // 아래쪽 덧말
    let mut dutmal_bottom = Control::dutmal("漢字", "한자");
    if let Control::Dutmal { ref mut position, .. } = dutmal_bottom {
        *position = DutmalPosition::Bottom;
    }
    // 오른쪽 덧말 + 왼쪽정렬
    let mut dutmal_right = Control::dutmal("情報", "정보");
    if let Control::Dutmal { ref mut position, ref mut align, .. } = dutmal_right {
        *position = DutmalPosition::Right;
        *align = DutmalAlign::Left;
    }

    paras.push(runs_p(
        vec![
            Run::text("위쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_top, csi(CS_NORMAL)),
            Run::text("    아래쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_bottom, csi(CS_NORMAL)),
            Run::text("    오른쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_right, csi(CS_NORMAL)),
        ],
        PS_CENTER,
    ));
    paras.push(empty());

    // ── 글자겹침(Compose) ──
    paras.push(p("3. 글자겹침 (Compose)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("글자겹침 기능: ", csi(CS_NORMAL)),
            Run::control(Control::compose("12"), csi(CS_NORMAL)),
            Run::text("  (숫자 1과 2를 겹침)", csi(CS_SMALL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 필드(Field) 데모 ──
    paras.push(p("4. 필드 (Field)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // ClickHere (누름틀)
    paras.push(runs_p(
        vec![
            Run::text("누름틀(ClickHere): ", csi(CS_NORMAL)),
            Run::control(Control::field("이름을 입력하세요"), csi(CS_BLUE)),
        ],
        PS_BODY,
    ));

    // Date 필드
    paras.push(runs_p(
        vec![
            Run::text("날짜 필드(Date): ", csi(CS_NORMAL)),
            Run::control(
                Control::Field {
                    field_type: FieldType::Date,
                    hint_text: Some("날짜".to_string()),
                    help_text: Some("문서 작성 날짜를 표시합니다.".to_string()),
                },
                csi(CS_BLUE),
            ),
        ],
        PS_BODY,
    ));

    // PageNum 필드
    paras.push(runs_p(
        vec![
            Run::text("쪽 번호 필드(autoNum): 현재 ", csi(CS_NORMAL)),
            Run::control(
                Control::Field { field_type: FieldType::PageNum, hint_text: None, help_text: None },
                csi(CS_BLUE),
            ),
            Run::text("쪽", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 미주(Endnote) 데모 ──
    paras.push(p("5. 미주 (Endnote)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text(
                "글자 모양(charShape)은 폰트, 크기, 색상, 굵기, 기울임, 밑줄, 취소선 등을 정의합니다",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::endnote(vec![p(
                    "charShape 속성 목록: height(크기), textColor(색상), bold(굵기), italic(기울임), underlineType(밑줄), strikeoutShape(취소선), emphasis(강조점), ratio(장평), spacing(자간), relSz(상대크기), offset(세로위치), useKerning(커닝), useFontSpace(폰트 자간).",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 메모(Memo) ──
    paras.push(p("6. 메모 (Memo)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("이 문단에는 검토 메모가 첨부되어 있습니다.", csi(CS_NORMAL)),
            Run::control(
                Control::memo(
                    vec![
                        p("검토 의견:", CS_RED_BOLD, PS_LEFT),
                        p("charShape 설명을 표 형태로 정리하면 더 좋겠습니다.", CS_NORMAL, PS_LEFT),
                        p("다음 버전에 반영 부탁드립니다.", CS_NORMAL, PS_LEFT),
                    ],
                    "김검토",
                    "2026-03-06",
                ),
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 상호참조(CrossRef) ──
    paras.push(p("7. 상호참조 (CrossRef)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("HWPX 문서 정의는 섹션 1의 ", csi(CS_NORMAL)),
            Run::control(
                Control::cross_ref("HWPX정의", RefType::Bookmark, RefContentType::Page),
                csi(CS_BLUE),
            ),
            Run::text("쪽을 참조하세요. ZIP 파일 구조는 ", csi(CS_NORMAL)),
            Run::control(
                Control::CrossRef {
                    target_name: "헤더구조".to_string(),
                    ref_type: RefType::Bookmark,
                    content_type: RefContentType::Page,
                    as_hyperlink: true,
                },
                csi(CS_BLUE),
            ),
            Run::text("쪽에 설명되어 있습니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 정렬별 글자 스타일 시연 ──
    paras.push(p("8. 글자 서식 변화 시연", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("기본 ", csi(CS_NORMAL)),
            Run::text("굵게 ", csi(CS_RED_BOLD)),
            Run::text("파랑 ", csi(CS_BLUE)),
            Run::text("기울임 녹색 ", csi(CS_GREEN_ITALIC)),
            Run::text("작은 글씨 ", csi(CS_SMALL)),
            Run::text("제목 크기 ", csi(CS_TITLE)),
            Run::text("회색 워터마크", csi(CS_GRAY)),
        ],
        PS_BODY,
    ));

    // ── 섹션 설정: Visibility + 줄번호 ──
    let vis = Visibility {
        hide_first_header: true,
        hide_first_footer: false,
        hide_first_master_page: false,
        hide_first_page_num: false,
        hide_first_empty_line: false,
        show_line_number: true,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowAll,
    };
    let lns = LineNumberShape {
        restart_type: 2, // per section
        count_by: 5,
        distance: HwpUnit::new(850).unwrap(),
        start_number: 1,
    };

    let mut section = Section::with_paragraphs(paras, PageSettings::a4());
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);

    section
}

// ═══════════════════════════════════════════════════════════════════════════
// 섹션 3: 도형과 그래픽 요소
// ═══════════════════════════════════════════════════════════════════════════

fn section3_shapes_and_graphics() -> Section {
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

// ═══════════════════════════════════════════════════════════════════════════
// 섹션 4: 차트, 수식, 고급 기능
// ═══════════════════════════════════════════════════════════════════════════

fn section4_charts_equations_advanced() -> Section {
    let mut paras: Vec<Paragraph> = vec![
        p("차트, 수식, 고급 기능", CS_TITLE, PS_CENTER),
        empty(),
        // ── 4.1 수식(Equation) ──
        p("4.1 수식 (Equation — HancomEQN)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "HWPX의 수식은 HancomEQN 스크립트 형식을 사용합니다. MathML이 아닌 자체 문법입니다:",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
        // 수식 1: 분수
        p("분수:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("{a + b} over {c + d}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 2: 제곱근
        p("제곱근:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("root {2} of {x^2 + y^2}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 3: 적분
        p("적분:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("int _{0} ^{inf} e^{-x^2} dx = {sqrt {pi}} over {2}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 4: 행렬
        p("행렬:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("left ( matrix {a # b ## c # d} right )"),
            CS_NORMAL,
            PS_CENTER,
        ),
        empty(),
        // ── 4.2 차트(Chart) ──
        p("4.2 차트 (Chart — OOXML)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "HWPX는 OOXML(Office Open XML) 차트 형식을 사용합니다. Chart XML은 ZIP 내 별도 파일로 저장되며, content.hpf 매니페스트에는 등록하지 않습니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
        // 차트 1: 세로막대 (Column, Clustered, 제목+범례)
        p("세로막대 차트 (Column, Clustered):", CS_NORMAL, PS_LEFT),
    ];
    let col_data = ChartData::category(
        &["1분기", "2분기", "3분기", "4분기"],
        &[("매출", &[120.0, 180.0, 150.0, 210.0]), ("비용", &[80.0, 100.0, 95.0, 130.0])],
    );
    let mut col_chart = Control::chart(ChartType::Column, col_data);
    if let Control::Chart { ref mut title, ref mut legend, .. } = col_chart {
        *title = Some("분기별 매출/비용".to_string());
        *legend = LegendPosition::Bottom;
    }
    paras.push(ctrl_p(col_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 2: 원형 (Pie)
    paras.push(p("원형 차트 (Pie):", CS_NORMAL, PS_LEFT));
    let pie_data = ChartData::category(
        &["한국", "미국", "일본", "기타"],
        &[("시장점유율", &[35.0, 28.0, 22.0, 15.0])],
    );
    let mut pie_chart = Control::chart(ChartType::Pie, pie_data);
    if let Control::Chart { ref mut title, ref mut explosion, .. } = pie_chart {
        *title = Some("시장 점유율".to_string());
        *explosion = Some(15);
    }
    paras.push(ctrl_p(pie_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 3: 꺾은선 (Line, markers)
    paras.push(p("꺾은선 차트 (Line):", CS_NORMAL, PS_LEFT));
    let line_data = ChartData::category(
        &["1월", "2월", "3월", "4월", "5월", "6월"],
        &[
            ("서울", &[2.0, 4.0, 10.0, 17.0, 22.0, 26.0]),
            ("부산", &[5.0, 7.0, 12.0, 18.0, 23.0, 27.0]),
        ],
    );
    let mut line_chart = Control::chart(ChartType::Line, line_data);
    if let Control::Chart { ref mut title, ref mut show_markers, ref mut grouping, .. } = line_chart
    {
        *title = Some("월별 평균 기온".to_string());
        *show_markers = Some(true);
        *grouping = ChartGrouping::Standard;
    }
    paras.push(ctrl_p(line_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 4: 분산형 (Scatter)
    paras.push(p("분산형 차트 (Scatter):", CS_NORMAL, PS_LEFT));
    let scatter_data = ChartData::xy(&[(
        "측정값",
        &[1.0, 2.5, 3.0, 4.5, 5.0, 6.5, 7.0, 8.5],
        &[2.3, 3.1, 4.8, 5.2, 7.1, 6.8, 8.9, 9.5],
    )]);
    let mut scatter_chart = Control::chart(ChartType::Scatter, scatter_data);
    if let Control::Chart { ref mut title, .. } = scatter_chart {
        *title = Some("X-Y 상관 분석".to_string());
    }
    paras.push(ctrl_p(scatter_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 4.3 고급 표 서식 ──
    paras.push(p("4.3 고급 표 서식", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "표는 col_span으로 셀 병합, background로 배경색 지정이 가능합니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // 복합 표: HWPX 요소 분류
    let mut merged_title = TableCell::new(
        vec![p("HWPX 요소 분류표", CS_RED_BOLD, PS_CENTER)],
        HwpUnit::from_mm(170.0).unwrap(),
    );
    merged_title.col_span = 3;
    merged_title.background = Some(Color::from_rgb(240, 240, 200));

    let th_row = TableRow::new(vec![merged_title]);

    let th2_row = TableRow::new(vec![
        colored_cell("분류", 40.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
        colored_cell("요소명", 65.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
        colored_cell("설명", 65.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
    ]);

    let r1 = TableRow::new(vec![
        colored_cell("구조", 40.0, CS_NORMAL, PS_CENTER, 250, 255, 250),
        text_cell("Section, Paragraph, Run", 65.0, CS_SMALL, PS_LEFT),
        text_cell("문서의 기본 골격 (섹션→문단→런)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r2 = TableRow::new(vec![
        colored_cell("서식", 40.0, CS_NORMAL, PS_CENTER, 250, 250, 255),
        text_cell("CharShape, ParaShape, Style", 65.0, CS_SMALL, PS_LEFT),
        text_cell("글자/문단 모양 정의 (header.xml)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r3 = TableRow::new(vec![
        colored_cell("객체", 40.0, CS_NORMAL, PS_CENTER, 255, 250, 245),
        text_cell("Table, Image, TextBox, Chart", 65.0, CS_SMALL, PS_LEFT),
        text_cell("인라인 또는 부동 객체", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r4 = TableRow::new(vec![
        colored_cell("도형", 40.0, CS_NORMAL, PS_CENTER, 255, 245, 250),
        text_cell("Line, Ellipse, Polygon, Arc, Curve", 65.0, CS_SMALL, PS_LEFT),
        text_cell("벡터 드로잉 객체 (shape common block)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r5 = TableRow::new(vec![
        colored_cell("주석", 40.0, CS_NORMAL, PS_CENTER, 245, 250, 255),
        text_cell("Footnote, Endnote, Memo, Bookmark", 65.0, CS_SMALL, PS_LEFT),
        text_cell("참조 및 주석 체계", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r6 = TableRow::new(vec![
        colored_cell("필드", 40.0, CS_NORMAL, PS_CENTER, 255, 255, 240),
        text_cell("Hyperlink, Field, CrossRef, IndexMark", 65.0, CS_SMALL, PS_LEFT),
        text_cell("fieldBegin/fieldEnd 패턴 인코딩", 65.0, CS_SMALL, PS_LEFT),
    ]);

    let mut adv_table = Table::new(vec![th_row, th2_row, r1, r2, r3, r4, r5, r6]);
    adv_table.width = Some(HwpUnit::from_mm(170.0).unwrap());
    adv_table.caption = Some(Caption::new(
        vec![p("표 2. HWPX 문서 요소 분류", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));

    paras.push(runs_p(vec![Run::table(adv_table, csi(CS_NORMAL))], PS_CENTER));
    paras.push(empty());

    // ── 4.4 페이지 테두리 + 시작 번호 ──
    paras.push(p("4.4 페이지 테두리 (PageBorderFill) + BeginNum", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "이 섹션에는 페이지 테두리(borderFillIDRef=3, 검은 실선)가 설정되어 있으며, 페이지 번호는 1부터 새로 시작합니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 4.5 종합 요약 ──
    paras.push(p("4.5 종합 요약", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "이 문서는 HwpForge 라이브러리의 전체 API를 사용하여 생성되었습니다. 4개 섹션에 걸쳐 다음 기능들을 시연했습니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // 기능 목록
    paras.push(p(
        "구조: Document, Section, Paragraph, Run, Table, Image(Store)",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "섹션: Header, Footer, PageNumber, ColumnSettings, Visibility, LineNumberShape",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "섹션: PageBorderFill, MasterPage, BeginNum, Gutter, Landscape",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "도형: Line, Ellipse, Polygon, Arc, Curve, ConnectLine, TextBox",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "스타일: ShapeStyle (rotation, flip, fill, arrow), Caption (4방향)",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p("채우기: Solid, Gradient (Linear), Pattern (HorizontalLine)", CS_NORMAL, PS_LEFT));
    paras.push(p("차트: Column, Pie, Line, Scatter (OOXML 형식)", CS_NORMAL, PS_LEFT));
    paras.push(p("수식: fraction, root, integral, matrix (HancomEQN)", CS_NORMAL, PS_LEFT));
    paras.push(p("텍스트: Dutmal (3방향), Compose (글자겹침)", CS_NORMAL, PS_LEFT));
    paras.push(p("참조: Bookmark (Point/Span), CrossRef, Hyperlink", CS_NORMAL, PS_LEFT));
    paras.push(p("필드: ClickHere, Date, PageNum", CS_NORMAL, PS_LEFT));
    paras.push(p("주석: Footnote, Endnote, Memo, IndexMark", CS_NORMAL, PS_LEFT));
    paras.push(p("정렬: Left, Center, Right, Justify, Distribute", CS_NORMAL, PS_LEFT));
    paras.push(p(
        "스타일스토어: Font, CharShape(8종), ParaShape(5종), BorderFill(4종), Numbering, Tab",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p("=== HWPX 문서 구조 완전 가이드 끝 ===", CS_TITLE, PS_CENTER));

    // ── 섹션 설정 ──
    let mut section = Section::with_paragraphs(paras, PageSettings::a4());

    // 페이지 테두리
    section.page_border_fills = Some(vec![
        PageBorderFillEntry {
            apply_type: "BOTH".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
        PageBorderFillEntry {
            apply_type: "EVEN".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
        PageBorderFillEntry {
            apply_type: "ODD".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
    ]);

    // 시작 번호 리셋
    section.begin_num =
        Some(BeginNum { page: 1, footnote: 1, endnote: 1, pic: 1, tbl: 1, equation: 1 });

    // 마스터페이지 (워터마크)
    section.master_pages = Some(vec![MasterPage::new(
        ApplyPageType::Both,
        vec![p("[ DRAFT / 초안 ]", CS_GRAY, PS_CENTER)],
    )]);

    // 페이지 번호
    section.page_number = Some(PageNumber::bottom_center());

    section
}

// ═══════════════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════════════

fn main() {
    println!("HWPX 문서 구조 완전 가이드 생성 중...\n");

    // 스타일 스토어 구성
    let store = build_style_store();
    let mut image_store = ImageStore::new();
    let mascot_bytes = std::fs::read("assets/mascot.png").expect("assets/mascot.png not found");
    image_store.insert("image1.png", mascot_bytes);

    // 문서 구성: 4개 섹션
    let mut doc = Document::new();
    doc.add_section(section1_document_structure());
    doc.add_section(section2_text_formatting());
    doc.add_section(section3_shapes_and_graphics());
    doc.add_section(section4_charts_equations_advanced());

    // 검증
    let validated = doc.validate().expect("문서 검증 실패");

    // 인코딩
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store).expect("HWPX 인코딩 실패");

    // 파일 저장
    std::fs::create_dir_all("temp").expect("temp 디렉토리 생성 실패");
    let output_path = "temp/hwpx_complete_guide.hwpx";
    std::fs::write(output_path, &bytes).expect("파일 저장 실패");

    println!("생성 완료: {} ({} bytes)", output_path, bytes.len());
    println!();
    println!("섹션 구성:");
    println!("  1. HWPX 문서 구조 개요 (A4 세로, 머리글/바닥글/페이지번호)");
    println!("  2. 텍스트 서식 시스템 (A4 세로, Visibility/줄번호)");
    println!("  3. 도형과 그래픽 요소 (A4 가로, 다단 2열)");
    println!("  4. 차트, 수식, 고급 기능 (A4 세로, 마스터페이지/페이지테두리)");
    println!();
    println!("사용된 API:");
    println!("  CharShape: 8종 (기본/제목/소제목/작은/빨강/파랑/녹색/회색)");
    println!("  ParaShape: 5종 (양쪽/가운데/왼쪽/오른쪽/배분)");
    println!("  BorderFill: 4종 (기본페이지/글자배경/표테두리/빨간테두리)");
    println!("  Controls: TextBox, Hyperlink, Footnote, Endnote, Line, Ellipse,");
    println!("            Polygon, Equation, Chart, Dutmal, Compose, Arc, Curve,");
    println!("            ConnectLine, Bookmark, CrossRef, Field, Memo, IndexMark");
    println!("  Charts: Column, Pie, Line, Scatter");
    println!("  Fills: Solid, Gradient, Pattern");
    println!();
    println!("한글에서 열어서 정상 렌더링을 확인하세요.");
}
