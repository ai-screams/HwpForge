//! 문단 기능 및 주석 종합 테스트
//!
//! - **Numbering**: NumberingDef (10 levels), TabDef, HeadingType
//! - **Character**: EmphasisType (13 variants), ratio/spacing/rel_sz/char_offset
//! - **Annotations**: Control::Dutmal (position/align combos), Control::Compose
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example paragraph_and_annotations
//!
//! Output:
//!   temp/paragraph_and_annotations.hwpx

use hwpforge_core::control::{Control, DutmalAlign, DutmalPosition};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::numbering::NumberingDef;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::tab::TabDef;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, EmphasisType, HeadingType, HwpUnit, LineSpacingType,
    ParaShapeIndex, StyleIndex,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

// ---------------------------------------------------------------------------
// Constants — CharShape indices (push order in build_style_store)
// ---------------------------------------------------------------------------

const CS_NORMAL: usize = 0; // 10pt black (pushed first)
const CS_TITLE: usize = 1; // 16pt bold
const CS_EMPHASIS_DOT: usize = 2; // DotAbove emphasis
const CS_EMPHASIS_RING: usize = 3; // RingAbove emphasis
const CS_EMPHASIS_TILDE: usize = 4; // Tilde emphasis
const CS_WIDE_SPACING: usize = 5; // ratio=120, spacing=30
const CS_SMALL_REL: usize = 6; // rel_sz=80, char_offset=100
const CS_KERNING: usize = 7; // use_kerning + use_font_space
const CS_RED_BOLD: usize = 8; // red + bold
const CS_EMPHASIS_SIDE: usize = 9; // Side emphasis
const CS_EMPHASIS_COLON: usize = 10; // Colon emphasis

// ---------------------------------------------------------------------------
// Constants — ParaShape indices (push order in build_style_store)
// ---------------------------------------------------------------------------

const PS_BODY: usize = 0; // Justify, 160% (pushed first)
const PS_CENTER: usize = 1; // Center align
const PS_OUTLINE_LV1: usize = 2; // Outline heading level 1
const PS_OUTLINE_LV2: usize = 3; // Outline heading level 2
const PS_OUTLINE_LV3: usize = 4; // Outline heading level 3
const PS_TAB_LEFT: usize = 5; // tabPrIDRef=1 (auto left tab)
const PS_TAB_RIGHT: usize = 6; // tabPrIDRef=2 (auto right tab)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn styled_para(text: &str, cs: usize, ps: usize, style_id: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
        .with_style(StyleIndex::new(style_id))
}

// ---------------------------------------------------------------------------
// Style Store Setup
// ---------------------------------------------------------------------------

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    // ── Fonts ───────────────────────────────────────────────────
    store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
    store.push_font(HwpxFont::new(1, "함초롬바탕", "HANGUL"));

    // ── Wave 8: Numbering + Tab ────────────────────────────────
    store.push_numbering(NumberingDef::default_outline());
    for tab in TabDef::defaults() {
        store.push_tab(tab);
    }

    // ── CharShapes ───────────────────────────────────────────────

    // CS 0: Normal (10pt black — default)
    store.push_char_shape(HwpxCharShape::default());

    // CS 1: Title (16pt bold)
    let mut cs_title = HwpxCharShape::default();
    cs_title.height = HwpUnit::from_pt(16.0).unwrap();
    cs_title.bold = true;
    store.push_char_shape(cs_title);

    // CS 2: DotAbove emphasis (Wave 10)
    let mut cs_dot = HwpxCharShape::default();
    cs_dot.emphasis = EmphasisType::DotAbove;
    store.push_char_shape(cs_dot);

    // CS 3: RingAbove emphasis
    let mut cs_ring = HwpxCharShape::default();
    cs_ring.emphasis = EmphasisType::RingAbove;
    store.push_char_shape(cs_ring);

    // CS 4: Tilde emphasis
    let mut cs_tilde = HwpxCharShape::default();
    cs_tilde.emphasis = EmphasisType::Tilde;
    store.push_char_shape(cs_tilde);

    // CS 5: Wide spacing (ratio=120%, spacing=30)
    let mut cs_wide = HwpxCharShape::default();
    cs_wide.ratio = 120;
    cs_wide.spacing = 30;
    store.push_char_shape(cs_wide);

    // CS 6: Small relative size + offset (rel_sz=80%, char_offset=100)
    let mut cs_small = HwpxCharShape::default();
    cs_small.rel_sz = 80;
    cs_small.char_offset = 100;
    store.push_char_shape(cs_small);

    // CS 7: Kerning + font space
    let mut cs_kern = HwpxCharShape::default();
    cs_kern.use_kerning = true;
    cs_kern.use_font_space = true;
    store.push_char_shape(cs_kern);

    // CS 8: Red bold
    let mut cs_red = HwpxCharShape::default();
    cs_red.text_color = Color::from_rgb(200, 0, 0);
    cs_red.bold = true;
    store.push_char_shape(cs_red);

    // CS 9: Side emphasis
    let mut cs_side = HwpxCharShape::default();
    cs_side.emphasis = EmphasisType::Side;
    store.push_char_shape(cs_side);

    // CS 10: Colon emphasis
    let mut cs_colon = HwpxCharShape::default();
    cs_colon.emphasis = EmphasisType::Colon;
    store.push_char_shape(cs_colon);

    // ── ParaShapes ─────────────────────────────────────────────

    // PS 0: Body (Justify, 160% line spacing — default)
    store.push_para_shape(HwpxParaShape::default());

    // PS 1: Center align
    let mut ps_center = HwpxParaShape::default();
    ps_center.alignment = Alignment::Center;
    store.push_para_shape(ps_center);

    // PS 2: Outline heading level 1 (Wave 8: HeadingType)
    let mut ps_ol1 = HwpxParaShape::default();
    ps_ol1.alignment = Alignment::Left;
    ps_ol1.heading_type = HeadingType::Outline;
    ps_ol1.heading_id_ref = 1; // references numbering def id=1
    ps_ol1.heading_level = 1;
    ps_ol1.tab_pr_id_ref = 1; // auto left tab
    ps_ol1.spacing_after = HwpUnit::new(200).unwrap();
    ps_ol1.line_spacing = 160;
    ps_ol1.line_spacing_type = LineSpacingType::Percentage;
    store.push_para_shape(ps_ol1);

    // PS 3: Outline heading level 2
    let mut ps_ol2 = HwpxParaShape::default();
    ps_ol2.alignment = Alignment::Left;
    ps_ol2.heading_type = HeadingType::Outline;
    ps_ol2.heading_id_ref = 1;
    ps_ol2.heading_level = 2;
    ps_ol2.tab_pr_id_ref = 1;
    ps_ol2.margin_left = HwpUnit::new(800).unwrap();
    ps_ol2.spacing_after = HwpUnit::new(100).unwrap();
    ps_ol2.line_spacing = 160;
    ps_ol2.line_spacing_type = LineSpacingType::Percentage;
    store.push_para_shape(ps_ol2);

    // PS 4: Outline heading level 3
    let mut ps_ol3 = HwpxParaShape::default();
    ps_ol3.alignment = Alignment::Left;
    ps_ol3.heading_type = HeadingType::Outline;
    ps_ol3.heading_id_ref = 1;
    ps_ol3.heading_level = 3;
    ps_ol3.tab_pr_id_ref = 1;
    ps_ol3.margin_left = HwpUnit::new(1600).unwrap();
    ps_ol3.spacing_after = HwpUnit::new(100).unwrap();
    ps_ol3.line_spacing = 160;
    ps_ol3.line_spacing_type = LineSpacingType::Percentage;
    store.push_para_shape(ps_ol3);

    // PS 5: With auto left tab
    let mut ps_tab_l = HwpxParaShape::default();
    ps_tab_l.tab_pr_id_ref = 1;
    store.push_para_shape(ps_tab_l);

    // PS 6: With auto right tab
    let mut ps_tab_r = HwpxParaShape::default();
    ps_tab_r.tab_pr_id_ref = 2;
    store.push_para_shape(ps_tab_r);

    store
}

// ---------------------------------------------------------------------------
// Section 1: Wave 10 — EmphasisType 및 CharShape 확장 필드 테스트
// ---------------------------------------------------------------------------

fn build_section1() -> Section {
    let paragraphs = vec![
        // ── Title ──
        text_para("Wave 10: 강조점(EmphasisType) 및 문자 속성 확장 테스트", CS_TITLE, PS_CENTER),
        // ── Blank line ──
        text_para("", CS_NORMAL, PS_BODY),
        // ── EmphasisType variants ──
        text_para("[1] 강조점 테스트", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::text("· DotAbove 강조: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text(
                    "강조점이 글자 위에 점으로 표시됩니다",
                    CharShapeIndex::new(CS_EMPHASIS_DOT),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("· RingAbove 강조: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text(
                    "강조점이 글자 위에 동그라미로 표시됩니다",
                    CharShapeIndex::new(CS_EMPHASIS_RING),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("· Tilde 강조: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("강조점이 물결표로 표시됩니다", CharShapeIndex::new(CS_EMPHASIS_TILDE)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("· Side 강조: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("강조점이 옆에 점으로 표시됩니다", CharShapeIndex::new(CS_EMPHASIS_SIDE)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("· Colon 강조: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text(
                    "강조점이 콜론 형태로 표시됩니다",
                    CharShapeIndex::new(CS_EMPHASIS_COLON),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // ── Blank line ──
        text_para("", CS_NORMAL, PS_BODY),
        // ── Extended CharShape fields ──
        text_para("[2] 문자 속성 확장 필드 테스트", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::text("· 장평(ratio)=120%, 자간(spacing)=30: ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("이 텍스트는 넓게 퍼져 있습니다", CharShapeIndex::new(CS_WIDE_SPACING)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text(
                    "· 상대크기(rel_sz)=80%, 오프셋(offset)=100: ",
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text("이 텍스트는 작고 위로 올라갑니다", CharShapeIndex::new(CS_SMALL_REL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text(
                    "· 커닝(kerning) + 글꼴간격(font_space): ",
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(
                    "AV WAve Typography 커닝이 적용된 텍스트",
                    CharShapeIndex::new(CS_KERNING),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // ── Mixed emphasis in one paragraph ──
        text_para("", CS_NORMAL, PS_BODY),
        text_para("[3] 한 문단에서 여러 강조점 혼합", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::text("일반 텍스트 → ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("점(Dot)", CharShapeIndex::new(CS_EMPHASIS_DOT)),
                Run::text(" → ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("동그라미(Ring)", CharShapeIndex::new(CS_EMPHASIS_RING)),
                Run::text(" → ", CharShapeIndex::new(CS_NORMAL)),
                Run::text("물결(Tilde)", CharShapeIndex::new(CS_EMPHASIS_TILDE)),
                Run::text(" → 일반", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
    ];

    Section::with_paragraphs(paragraphs, PageSettings::a4())
}

// ---------------------------------------------------------------------------
// Section 2: Wave 8 — HeadingType, Outline, Numbering, Tab 테스트
// ---------------------------------------------------------------------------

fn build_section2() -> Section {
    let paragraphs = vec![
        // ── Title ──
        text_para(
            "Wave 8: 개요(Outline), 번호매기기(Numbering), 탭(Tab) 테스트",
            CS_TITLE,
            PS_CENTER,
        ),
        text_para("", CS_NORMAL, PS_BODY),
        // ── Outline headings with styleIDRef for 개요 1-3 ──
        // 개요 1 = style ID 2 (Modern), 개요 2 = style ID 3, 개요 3 = style ID 4
        text_para("[1] 개요 번호 (HeadingType::Outline)", CS_RED_BOLD, PS_BODY),
        styled_para("개요 수준 1: 첫 번째 대제목입니다", CS_NORMAL, PS_OUTLINE_LV1, 2),
        text_para(
            "개요 수준 1 아래의 본문 내용입니다. 이 문단은 들여쓰기 없이 표시됩니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        styled_para("개요 수준 2: 소제목 가", CS_NORMAL, PS_OUTLINE_LV2, 3),
        text_para(
            "개요 수준 2 아래의 본문 내용입니다. 약간의 들여쓰기가 적용됩니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        styled_para("개요 수준 3: 항목 1)", CS_NORMAL, PS_OUTLINE_LV3, 4),
        text_para("개요 수준 3 아래의 세부 내용입니다.", CS_NORMAL, PS_BODY),
        styled_para("개요 수준 3: 항목 2)", CS_NORMAL, PS_OUTLINE_LV3, 4),
        text_para("두 번째 세부 항목의 내용입니다.", CS_NORMAL, PS_BODY),
        styled_para("개요 수준 2: 소제목 나", CS_NORMAL, PS_OUTLINE_LV2, 3),
        text_para("또 다른 소제목 아래의 본문입니다.", CS_NORMAL, PS_BODY),
        styled_para("개요 수준 1: 두 번째 대제목입니다", CS_NORMAL, PS_OUTLINE_LV1, 2),
        text_para("두 번째 대제목 아래의 본문 내용입니다.", CS_NORMAL, PS_BODY),
        // ── Blank line ──
        text_para("", CS_NORMAL, PS_BODY),
        // ── Tab properties ──
        text_para("[2] 탭 속성 (TabDef) 테스트", CS_RED_BOLD, PS_BODY),
        text_para(
            "tabPrIDRef=1 (자동 왼쪽 탭): 이 문단은 auto left tab이 적용됩니다.",
            CS_NORMAL,
            PS_TAB_LEFT,
        ),
        text_para(
            "tabPrIDRef=2 (자동 오른쪽 탭): 이 문단은 auto right tab이 적용됩니다.",
            CS_NORMAL,
            PS_TAB_RIGHT,
        ),
        text_para("tabPrIDRef=0 (기본값): 이 문단은 기본 탭 설정입니다.", CS_NORMAL, PS_BODY),
    ];

    Section::with_paragraphs(paragraphs, PageSettings::a4())
}

// ---------------------------------------------------------------------------
// Section 3: Wave 13 — Dutmal (덧말) 및 Compose (글자겹침) 테스트
// ---------------------------------------------------------------------------

fn build_section3() -> Section {
    let paragraphs = vec![
        // ── Title ──
        text_para("Wave 13: 덧말(Dutmal) 및 글자겹침(Compose) 테스트", CS_TITLE, PS_CENTER),
        text_para("", CS_NORMAL, PS_BODY),
        // ── Dutmal: various position combos ──
        text_para("[1] 덧말(Dutmal) — 위치 변형 테스트", CS_RED_BOLD, PS_BODY),
        // Dutmal Top + Center (default)
        Paragraph::with_runs(
            vec![
                Run::text("덧말 위(Top) + 가운데: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::dutmal("대한민국", "Republic of Korea"),
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(" — 기본값입니다.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Dutmal Bottom + Center
        Paragraph::with_runs(
            vec![
                Run::text("덧말 아래(Bottom) + 가운데: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::Dutmal {
                        main_text: "서울특별시".into(),
                        sub_text: "Seoul".into(),
                        position: DutmalPosition::Bottom,
                        sz_ratio: 50,
                        align: DutmalAlign::Center,
                    },
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(" — 아래쪽 덧말.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Dutmal Top + Left
        Paragraph::with_runs(
            vec![
                Run::text("덧말 위(Top) + 왼쪽정렬: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::Dutmal {
                        main_text: "인공지능".into(),
                        sub_text: "AI".into(),
                        position: DutmalPosition::Top,
                        sz_ratio: 0,
                        align: DutmalAlign::Left,
                    },
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(" — 왼쪽 정렬 덧말.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Dutmal Top + Right
        Paragraph::with_runs(
            vec![
                Run::text("덧말 위(Top) + 오른쪽정렬: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::Dutmal {
                        main_text: "한글".into(),
                        sub_text: "Hangul".into(),
                        position: DutmalPosition::Top,
                        sz_ratio: 70,
                        align: DutmalAlign::Right,
                    },
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(" — 오른쪽 정렬 덧말.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Dutmal Right position
        Paragraph::with_runs(
            vec![
                Run::text("덧말 오른쪽(Right) 위치: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::Dutmal {
                        main_text: "프로그래밍".into(),
                        sub_text: "Programming".into(),
                        position: DutmalPosition::Right,
                        sz_ratio: 0,
                        align: DutmalAlign::Center,
                    },
                    CharShapeIndex::new(CS_NORMAL),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Dutmal Left position
        Paragraph::with_runs(
            vec![
                Run::text("덧말 왼쪽(Left) 위치: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::Dutmal {
                        main_text: "문서".into(),
                        sub_text: "Document".into(),
                        position: DutmalPosition::Left,
                        sz_ratio: 0,
                        align: DutmalAlign::Center,
                    },
                    CharShapeIndex::new(CS_NORMAL),
                ),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Multiple dutmal in one paragraph
        text_para("", CS_NORMAL, PS_BODY),
        text_para("[2] 한 문단에 여러 덧말", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::control(
                    Control::dutmal("HwpForge", "에이치더블유피포지"),
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text("는 ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::dutmal("HWPX", "한글 문서 형식"),
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text(" 파일을 생성하는 ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::dutmal("Rust", "러스트"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" 라이브러리입니다.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // ── Blank line ──
        text_para("", CS_NORMAL, PS_BODY),
        // ── Compose ──
        text_para("[3] 글자겹침(Compose) 테스트", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::text("글자겹침 예시 1: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("가"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" — 단일 글자 겹침", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("글자겹침 예시 2: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("12"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" — 두 글자 겹침 (숫자)", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        Paragraph::with_runs(
            vec![
                Run::text("글자겹침 예시 3: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("AB"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" — 영문 겹침", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // Multiple compose in one paragraph
        Paragraph::with_runs(
            vec![
                Run::text("연속 겹침: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("①"), CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("②"), CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("③"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" — 원문자 연속 겹침", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
        // ── Blank line ──
        text_para("", CS_NORMAL, PS_BODY),
        // ── Mixed Dutmal + Compose ──
        text_para("[4] 덧말 + 글자겹침 혼합 테스트", CS_RED_BOLD, PS_BODY),
        Paragraph::with_runs(
            vec![
                Run::text("혼합: ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::dutmal("제1조", "Article 1"), CharShapeIndex::new(CS_NORMAL)),
                Run::text(" ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("甲"), CharShapeIndex::new(CS_NORMAL)),
                Run::text("은 ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(Control::compose("乙"), CharShapeIndex::new(CS_NORMAL)),
                Run::text("에게 ", CharShapeIndex::new(CS_NORMAL)),
                Run::control(
                    Control::dutmal("계약금", "Contract deposit"),
                    CharShapeIndex::new(CS_NORMAL),
                ),
                Run::text("을 지급한다.", CharShapeIndex::new(CS_NORMAL)),
            ],
            ParaShapeIndex::new(PS_BODY),
        ),
    ];

    Section::with_paragraphs(paragraphs, PageSettings::a4())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== Wave 8/10/13 API 종합 테스트 ===\n");

    // 1. Build style store with all Wave 8/10/13 features
    let store = build_style_store();
    println!("[1] 스타일 스토어 구성 완료:");
    println!(
        "    Fonts: {}, CharShapes: {}, ParaShapes: {}",
        store.font_count(),
        store.char_shape_count(),
        store.para_shape_count(),
    );
    println!(
        "    Numberings: {}, Tabs: {}, Styles: {}",
        store.numbering_count(),
        store.tab_count(),
        store.style_count(),
    );

    // 2. Build document with 3 sections
    let image_store = ImageStore::new();
    let mut doc = Document::new();
    doc.add_section(build_section1()); // Wave 10: Emphasis + CharShape fields
    doc.add_section(build_section2()); // Wave 8: Heading/Outline/Numbering/Tab
    doc.add_section(build_section3()); // Wave 13: Dutmal + Compose

    println!("[2] 문서 구성: {} sections", doc.sections().len());
    for (i, sec) in doc.sections().iter().enumerate() {
        println!("    Section {}: {} paragraphs", i + 1, sec.paragraphs.len());
    }

    // 3. Validate
    let validated = doc.validate().expect("validation failed");
    println!("[3] 문서 검증 완료 (Draft → Validated)");

    // 4. Encode to HWPX
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store).expect("encode failed");
    std::fs::create_dir_all("temp").ok();
    let output_path = "temp/paragraph_and_annotations.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");
    println!("[4] HWPX 파일 생성: {output_path} ({} bytes)", bytes.len());

    // 5. Decode back (roundtrip verification)
    let decoded = HwpxDecoder::decode(&bytes).expect("decode failed");
    let d = &decoded.document;
    println!("[5] 라운드트립 디코딩 완료:");
    println!("    Sections: {}", d.sections().len());

    // Verify Section 1 (Wave 10 - Emphasis)
    let s1 = &d.sections()[0];
    println!("    Section 1 (Wave 10 EmphasisType):");
    println!("      Paragraphs: {}", s1.paragraphs.len());

    // Verify Section 2 (Wave 8 - Heading)
    let s2 = &d.sections()[1];
    println!("    Section 2 (Wave 8 Heading/Outline):");
    println!("      Paragraphs: {}", s2.paragraphs.len());
    // Check outline headings have style_id
    let styled_count = s2.paragraphs.iter().filter(|p| p.style_id.is_some()).count();
    println!("      Paragraphs with styleIDRef: {styled_count}");

    // Verify Section 3 (Wave 13 - Dutmal/Compose)
    let s3 = &d.sections()[2];
    println!("    Section 3 (Wave 13 Dutmal/Compose):");
    println!("      Paragraphs: {}", s3.paragraphs.len());
    let dutmal_count = s3
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter(|r| matches!(r.content.as_control(), Some(c) if c.is_dutmal()))
        .count();
    let compose_count = s3
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter(|r| matches!(r.content.as_control(), Some(c) if c.is_compose()))
        .count();
    println!("      Dutmal controls: {dutmal_count}");
    println!("      Compose controls: {compose_count}");

    // 6. Verify style store roundtrip
    println!("[6] 스타일 스토어 라운드트립 검증:");
    let ds = &decoded.style_store;
    println!("    Fonts: {}", ds.font_count());
    println!("    CharShapes: {}", ds.char_shape_count());
    println!("    ParaShapes: {}", ds.para_shape_count());
    println!("    Numberings: {}", ds.numbering_count());
    println!("    Tabs: {}", ds.tab_count());

    // Verify emphasis types survived roundtrip
    if ds.char_shape_count() > CS_EMPHASIS_DOT {
        let cs = ds.char_shape(CharShapeIndex::new(CS_EMPHASIS_DOT)).unwrap();
        println!("    CharShape[{}] emphasis: {:?}", CS_EMPHASIS_DOT, cs.emphasis);
    }
    if ds.char_shape_count() > CS_KERNING {
        let cs = ds.char_shape(CharShapeIndex::new(CS_KERNING)).unwrap();
        println!(
            "    CharShape[{}] kerning={}, font_space={}",
            CS_KERNING, cs.use_kerning, cs.use_font_space
        );
    }

    // Verify heading type survived roundtrip
    if ds.para_shape_count() > PS_OUTLINE_LV1 {
        let ps = ds.para_shape(ParaShapeIndex::new(PS_OUTLINE_LV1)).unwrap();
        println!(
            "    ParaShape[{}] heading_type={:?}, level={}, id_ref={}",
            PS_OUTLINE_LV1, ps.heading_type, ps.heading_level, ps.heading_id_ref
        );
    }

    println!("\n=== 테스트 완료! ===");
    println!("한글(Hancom Office)에서 파일을 열어 확인하세요: {output_path}");
    println!("\n확인 포인트:");
    println!("  Section 1: 강조점(DotAbove, RingAbove, Tilde, Side, Colon)이 글자 위에 표시되는지");
    println!("  Section 1: 장평/자간/상대크기/커닝 적용 확인");
    println!("  Section 2: 개요 1/2/3 수준이 번호매기기와 함께 표시되는지");
    println!("  Section 2: 탭 속성(auto left/right)이 적용되는지");
    println!("  Section 3: 덧말이 본문 위/아래/좌/우에 올바르게 표시되는지");
    println!("  Section 3: 글자겹침이 원형 프레임 안에 표시되는지");
}
