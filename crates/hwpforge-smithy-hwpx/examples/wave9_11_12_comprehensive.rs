//! Wave 9/11/12 종합 테스트: 페이지 레이아웃, 도형 확장, 참조/주석 전체 API 검증
//!
//! 한글에서 열어서 각 기능이 정상 렌더링되는지 확인합니다.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example wave9_11_12_comprehensive
//!
//! Output:
//!   wave9_11_12_output.hwpx (프로젝트 루트)

use hwpforge_core::control::{ArrowStyle, Control, DutmalPosition, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{
    BeginNum, LineNumberShape, MasterPage, PageBorderFillEntry, PageNumber, Section, Visibility,
};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, ArcType, ArrowSize, ArrowType, BookmarkType, CharShapeIndex, Color,
    CurveSegmentType, FieldType, Flip, GutterType, HwpUnit, ParaShapeIndex, RefContentType,
    RefType, ShowMode,
};
use hwpforge_smithy_hwpx::style_store::{
    HwpxBorderFill, HwpxBorderLine, HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── CharShape indices ───────────────────────────────────────────────
const CS_NORMAL: usize = 0;
const CS_TITLE: usize = 1;
const CS_SMALL: usize = 2;
const CS_RED: usize = 3;
const CS_BLUE: usize = 4;

// ── ParaShape indices ───────────────────────────────────────────────
const PS_BODY: usize = 0;
const PS_CENTER: usize = 1;
const PS_LEFT: usize = 2;

// ── Helpers ─────────────────────────────────────────────────────────

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn ctrl_para(ctrl: Control, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn multi_run_para(runs: Vec<Run>, ps: usize) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(ps))
}

fn empty_para() -> Paragraph {
    Paragraph::with_runs(
        vec![Run::text("", CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_BODY),
    )
}

// ── Style Store ─────────────────────────────────────────────────────

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    // Fonts
    store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
    store.push_font(HwpxFont::new(1, "함초롬바탕", "HANGUL"));

    // CS 0: Normal 10pt
    store.push_char_shape(HwpxCharShape::default());

    // CS 1: Title 16pt bold
    let mut cs_title = HwpxCharShape::default();
    cs_title.height = HwpUnit::new(1600).unwrap();
    cs_title.bold = true;
    store.push_char_shape(cs_title);

    // CS 2: Small 8pt
    let mut cs_small = HwpxCharShape::default();
    cs_small.height = HwpUnit::new(800).unwrap();
    store.push_char_shape(cs_small);

    // CS 3: Red bold
    let mut cs_red = HwpxCharShape::default();
    cs_red.text_color = Color::from_rgb(200, 30, 30);
    cs_red.bold = true;
    store.push_char_shape(cs_red);

    // CS 4: Blue
    let mut cs_blue = HwpxCharShape::default();
    cs_blue.text_color = Color::from_rgb(30, 30, 200);
    store.push_char_shape(cs_blue);

    // PS 0: Body (양쪽 정렬, 160%)
    let mut ps_body = HwpxParaShape::default();
    ps_body.alignment = Alignment::Justify;
    ps_body.line_spacing = 160;
    store.push_para_shape(ps_body);

    // PS 1: Center
    let mut ps_center = HwpxParaShape::default();
    ps_center.alignment = Alignment::Center;
    ps_center.line_spacing = 160;
    store.push_para_shape(ps_center);

    // PS 2: Left with indent
    let mut ps_left = HwpxParaShape::default();
    ps_left.alignment = Alignment::Left;
    ps_left.line_spacing = 160;
    store.push_para_shape(ps_left);

    // Push 3 borderFills explicitly.
    // id=1: NONE (page border default — no visible border)
    // id=2: NONE + fill (char background default)
    // id=3: THICK BRIGHT RED for page border visibility test
    store.push_border_fill(HwpxBorderFill::default_page_border());
    store.push_border_fill(HwpxBorderFill::default_char_background());
    let mut bf3 = HwpxBorderFill::default_table_border();
    let border_line = HwpxBorderLine {
        line_type: "SOLID".into(),
        width: "0.4 mm".into(),
        color: "#FF0000".into(),
    };
    bf3.left = border_line.clone();
    bf3.right = border_line.clone();
    bf3.top = border_line.clone();
    bf3.bottom = border_line;
    store.push_border_fill(bf3);

    store
}

// ═══════════════════════════════════════════════════════════════════
// Section 1: Wave 9 — Gutter + Mirror Margins
// ═══════════════════════════════════════════════════════════════════

fn section_gutter_mirror() -> Section {
    let ps = PageSettings {
        gutter: HwpUnit::from_mm(10.0).unwrap(),
        gutter_type: GutterType::LeftOnly,
        ..PageSettings::a4()
    };
    Section::with_paragraphs(
        vec![
            text_para("Wave 9: Gutter + Mirror Margins", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("이 섹션은 Gutter=10mm, GutterType=LeftOnly 설정입니다.", CS_NORMAL, PS_BODY),
            text_para(
                "좌측에 10mm 제본 여백이 추가되어 본문 영역이 좁아집니다.",
                CS_NORMAL,
                PS_BODY,
            ),
            empty_para(),
            text_para("Gutter 종류별 설명:", CS_RED, PS_LEFT),
            text_para("  - LeftOnly: 좌측에만 제본 여백", CS_NORMAL, PS_LEFT),
            text_para("  - LeftRight: 좌우 양쪽 제본 여백", CS_NORMAL, PS_LEFT),
            text_para("  - TopOnly: 상단에만 제본 여백", CS_NORMAL, PS_LEFT),
            text_para("  - TopBottom: 상하 양쪽 제본 여백", CS_NORMAL, PS_LEFT),
        ],
        ps,
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: Wave 9 — Visibility + LineNumberShape
// ═══════════════════════════════════════════════════════════════════

fn section_visibility_linenumber() -> Section {
    let vis = Visibility {
        hide_first_header: true,
        hide_first_footer: false,
        hide_first_master_page: false,
        hide_first_page_num: true,
        hide_first_empty_line: false,
        show_line_number: true,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowOdd,
    };
    let lns = LineNumberShape {
        restart_type: 0, // Continuous
        count_by: 5,
        distance: HwpUnit::new(850).unwrap(),
        start_number: 1,
    };
    // 한글 counts visual rendered lines (not paragraphs).
    // Each paragraph below = 1 visual line, so line numbers align predictably.
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Visibility + LineNumberShape (countBy=5)", CS_TITLE, PS_CENTER), // 1
            text_para("hideFirstHeader=true, hideFirstPageNum=true", CS_NORMAL, PS_BODY), // 2
            text_para("border=SHOW_ALL, fill=SHOW_ODD", CS_NORMAL, PS_BODY),            // 3
            text_para("가나다라마바사아자차카타파하", CS_NORMAL, PS_BODY),              // 4
            text_para("← 줄 번호 5 (countBy=5이므로 이 줄에 표시)", CS_RED, PS_BODY),   // 5
            text_para("ABCDEFGHIJKLMNOPQRSTUVWXYZ", CS_NORMAL, PS_BODY),                // 6
            text_para("0123456789 +-*/=", CS_NORMAL, PS_BODY),                          // 7
            text_para("The quick brown fox jumps over the lazy dog.", CS_NORMAL, PS_BODY), // 8
            text_para("한글 줄 번호는 시각적 줄 기준으로 카운트됩니다.", CS_NORMAL, PS_BODY), // 9
            text_para("← 줄 번호 10 (여기에 표시)", CS_RED, PS_BODY),                   // 10
            text_para("줄 번호 간격(distance)은 850 HwpUnit입니다.", CS_NORMAL, PS_BODY), // 11
            text_para("법률 문서에서 줄 번호를 자주 사용합니다.", CS_NORMAL, PS_BODY),  // 12
            text_para("줄 번호는 왼쪽 여백에 표시됩니다.", CS_NORMAL, PS_BODY),         // 13
            text_para("restartType=0 (연속), startNumber=1", CS_NORMAL, PS_BODY),       // 14
            text_para("← 줄 번호 15 (여기에 표시)", CS_RED, PS_BODY),                   // 15
        ],
        PageSettings::a4(),
    );
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);
    section
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Wave 9 — PageBorderFill + BeginNum
// ═══════════════════════════════════════════════════════════════════

fn section_border_fill_begin_num() -> Section {
    // borderFillIDRef=4 references the user-added thick borderFill (0.4mm SOLID).
    // Default borderFills 1-3 have thin/invisible borders.
    // Use borderFillIDRef=3 (default_table_border = SOLID 0.12mm black).
    // Experiment: borderFillIDRef=4 failed lookup in 한글 — testing if id=3 works.
    let entries = vec![
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
    ];
    let cs = CharShapeIndex::new(CS_NORMAL);
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Wave 9: PageBorderFill + BeginNum", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("페이지 테두리(PageBorderFill)가 보여야 합니다.", CS_NORMAL, PS_BODY),
            text_para("페이지 번호가 10부터 시작합니다 (하단 확인).", CS_NORMAL, PS_BODY),
            // Footnote: should start at 5) due to BeginNum footnote=5
            Paragraph::with_runs(
                vec![
                    Run::text("이 문장에는 각주가 있습니다", cs),
                    Run::control(
                        Control::footnote(vec![text_para(
                            "각주 내용: 번호가 5부터 시작해야 합니다.",
                            CS_NORMAL,
                            PS_BODY,
                        )]),
                        cs,
                    ),
                    Run::text(".", cs),
                ],
                ParaShapeIndex::new(PS_BODY),
            ),
            // Endnote: should start at 3) due to BeginNum endnote=3
            Paragraph::with_runs(
                vec![
                    Run::text("이 문장에는 미주가 있습니다", cs),
                    Run::control(
                        Control::endnote(vec![text_para(
                            "미주 내용: 번호가 3부터 시작해야 합니다.",
                            CS_NORMAL,
                            PS_BODY,
                        )]),
                        cs,
                    ),
                    Run::text(".", cs),
                ],
                ParaShapeIndex::new(PS_BODY),
            ),
        ],
        PageSettings::a4(),
    );
    section.page_border_fills = Some(entries);
    section.begin_num =
        Some(BeginNum { page: 10, footnote: 5, endnote: 3, pic: 1, tbl: 1, equation: 1 });
    // Add page number display to make BeginNum visible (should show "10")
    section.page_number = Some(PageNumber::bottom_center());
    section
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Wave 9 — MasterPage (배경)
// ═══════════════════════════════════════════════════════════════════

fn section_master_page() -> Section {
    let master = MasterPage::new(
        ApplyPageType::Both,
        vec![text_para("[ CONFIDENTIAL / 대외비 ]", CS_SMALL, PS_CENTER)],
    );
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Wave 9: MasterPage (배경 페이지)", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("이 섹션에는 MasterPage가 설정되어 있습니다.", CS_NORMAL, PS_BODY),
            text_para(
                "배경에 '[ CONFIDENTIAL / 대외비 ]' 텍스트가 표시되어야 합니다.",
                CS_NORMAL,
                PS_BODY,
            ),
            text_para("MasterPage는 양면(BOTH)에 적용됩니다.", CS_NORMAL, PS_BODY),
        ],
        PageSettings::a4(),
    );
    section.master_pages = Some(vec![master]);
    section
}

// ═══════════════════════════════════════════════════════════════════
// Section 5: Wave 11 — Arc (3가지 타입)
// ═══════════════════════════════════════════════════════════════════

fn section_arcs() -> Section {
    let w = HwpUnit::from_mm(40.0).unwrap();
    let h = HwpUnit::from_mm(30.0).unwrap();

    // Arc 1: Normal (열린 호)
    let arc_normal = Control::arc(ArcType::Normal, w, h);

    // Arc 2: Pie (부채꼴)
    let arc_pie = Control::arc(ArcType::Pie, w, h);

    // Arc 3: Chord (활꼴)
    let arc_chord = Control::arc(ArcType::Chord, w, h);

    Section::with_paragraphs(
        vec![
            text_para("Wave 11: Arc (호) — 3가지 타입", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("1. Normal (열린 호):", CS_RED, PS_LEFT),
            ctrl_para(arc_normal, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("2. Pie (부채꼴/섹터):", CS_RED, PS_LEFT),
            ctrl_para(arc_pie, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("3. Chord (활꼴):", CS_RED, PS_LEFT),
            ctrl_para(arc_chord, CS_NORMAL, PS_CENTER),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 6: Wave 11 — Curve (베지어/폴리라인)
// ═══════════════════════════════════════════════════════════════════

fn section_curves() -> Section {
    // Curve 1: 직선 세그먼트 (지그재그)
    let zigzag = Control::curve(vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(3000, 6000),
        ShapePoint::new(6000, 0),
        ShapePoint::new(9000, 6000),
        ShapePoint::new(12000, 0),
    ])
    .unwrap();

    // Curve 2: 베지어 곡선 (S자 형태)
    let mut bezier = Control::curve(vec![
        ShapePoint::new(0, 5000),
        ShapePoint::new(3000, 0),
        ShapePoint::new(6000, 10000),
        ShapePoint::new(9000, 5000),
    ])
    .unwrap();
    // Set segment types to Curve for bezier
    if let Control::Curve { ref mut segment_types, .. } = bezier {
        *segment_types =
            vec![CurveSegmentType::Curve, CurveSegmentType::Curve, CurveSegmentType::Curve];
    }

    // Curve 3: 혼합 (직선 + 곡선)
    let mut mixed = Control::curve(vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(4000, 0),
        ShapePoint::new(6000, 4000),
        ShapePoint::new(10000, 4000),
    ])
    .unwrap();
    if let Control::Curve { ref mut segment_types, .. } = mixed {
        *segment_types =
            vec![CurveSegmentType::Line, CurveSegmentType::Curve, CurveSegmentType::Line];
    }

    Section::with_paragraphs(
        vec![
            text_para("Wave 11: Curve (곡선) — 3가지 패턴", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("1. 지그재그 (직선 세그먼트):", CS_RED, PS_LEFT),
            ctrl_para(zigzag, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("2. S자 베지어 곡선:", CS_RED, PS_LEFT),
            ctrl_para(bezier, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("3. 혼합 (직선 + 곡선):", CS_RED, PS_LEFT),
            ctrl_para(mixed, CS_NORMAL, PS_CENTER),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 7: Wave 11 — ConnectLine + 화살표
// ═══════════════════════════════════════════════════════════════════

fn section_connect_lines() -> Section {
    // ConnectLine 1: 단순 직선
    let cl_simple =
        Control::connect_line(ShapePoint::new(0, 0), ShapePoint::new(10000, 5000)).unwrap();

    // ConnectLine 2: 화살표 스타일
    let mut cl_arrow =
        Control::connect_line(ShapePoint::new(0, 0), ShapePoint::new(12000, 0)).unwrap();
    if let Control::ConnectLine { ref mut style, .. } = cl_arrow {
        *style = Some(ShapeStyle {
            head_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Arrow,
                size: ArrowSize::Medium,
                filled: true,
            }),
            tail_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Concave,
                size: ArrowSize::Large,
                filled: true,
            }),
            ..ShapeStyle::default()
        });
    }

    // ConnectLine 3: 빨간 두꺼운 선 + 양방향 화살표
    let mut cl_bidir =
        Control::connect_line(ShapePoint::new(0, 3000), ShapePoint::new(14000, 3000)).unwrap();
    if let Control::ConnectLine { ref mut style, .. } = cl_bidir {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(200, 30, 30)),
            line_width: Some(40),
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

    Section::with_paragraphs(
        vec![
            text_para("Wave 11: ConnectLine + Arrow Styles", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("1. 단순 연결선:", CS_RED, PS_LEFT),
            ctrl_para(cl_simple, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("2. 화살표 (앞: Arrow/Medium, 뒤: Stealth/Large):", CS_RED, PS_LEFT),
            ctrl_para(cl_arrow, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("3. 빨간 양방향 다이아몬드:", CS_RED, PS_LEFT),
            ctrl_para(cl_bidir, CS_NORMAL, PS_CENTER),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 8: Wave 11 — 기존 도형 + 회전/뒤집기/채우기
// ═══════════════════════════════════════════════════════════════════

fn section_shape_styles() -> Section {
    let w = HwpUnit::from_mm(35.0).unwrap();
    let h = HwpUnit::from_mm(25.0).unwrap();

    // Ellipse with rotation 45도
    let mut ell_rotated = Control::ellipse(w, h);
    if let Control::Ellipse { ref mut style, .. } = ell_rotated {
        *style = Some(ShapeStyle {
            rotation: Some(45.0), // 45도
            line_color: Some(Color::from_rgb(30, 100, 200)),
            line_width: Some(30),
            ..ShapeStyle::default()
        });
    }

    // Ellipse with horizontal flip
    let mut ell_flipped = Control::ellipse(w, h);
    if let Control::Ellipse { ref mut style, .. } = ell_flipped {
        *style = Some(ShapeStyle {
            flip: Some(Flip::Horizontal),
            line_color: Some(Color::from_rgb(200, 100, 30)),
            ..ShapeStyle::default()
        });
    }

    // Polygon (오각형) with Solid fill
    let pentagon = Control::polygon(vec![
        ShapePoint::new(5000, 0),
        ShapePoint::new(10000, 3800),
        ShapePoint::new(8100, 10000),
        ShapePoint::new(1900, 10000),
        ShapePoint::new(0, 3800),
    ])
    .unwrap();

    // Line with head/tail arrows and rotation
    let mut line_styled = Control::line(ShapePoint::new(0, 0), ShapePoint::new(14000, 0)).unwrap();
    if let Control::Line { ref mut style, .. } = line_styled {
        *style = Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 150, 0)),
            line_width: Some(25),
            head_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Open,
                size: ArrowSize::Medium,
                filled: true,
            }),
            tail_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Arrow,
                size: ArrowSize::Large,
                filled: true,
            }),
            ..ShapeStyle::default()
        });
    }

    Section::with_paragraphs(
        vec![
            text_para("Wave 11: Shape Styles (회전/뒤집기/화살표)", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para("1. 타원 — 45도 회전 (파란 테두리):", CS_RED, PS_LEFT),
            ctrl_para(ell_rotated, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("2. 타원 — 수평 뒤집기 (주황 테두리):", CS_RED, PS_LEFT),
            ctrl_para(ell_flipped, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("3. 오각형 다각형:", CS_RED, PS_LEFT),
            ctrl_para(pentagon, CS_NORMAL, PS_CENTER),
            empty_para(),
            text_para("4. 선 — 녹색 + 양쪽 화살표 (Open → Arrow):", CS_RED, PS_LEFT),
            ctrl_para(line_styled, CS_NORMAL, PS_CENTER),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 9: Wave 12 — Bookmark + CrossRef + Field
// ═══════════════════════════════════════════════════════════════════

fn section_bookmarks_refs_fields() -> Section {
    // Bookmark (point)
    let bm_point = Control::bookmark("중요위치");

    // Bookmark (span start — 범위 시작)
    let bm_span_start = Control::Bookmark {
        name: "핵심내용".to_string(),
        bookmark_type: BookmarkType::SpanStart,
    };

    // Bookmark (span end)
    let bm_span_end = Control::Bookmark {
        name: "핵심내용".to_string(),
        bookmark_type: BookmarkType::SpanEnd,
    };

    // CrossRef (참조)
    let cross_ref = Control::cross_ref("중요위치", RefType::Bookmark, RefContentType::Page);

    // Field: ClickHere (누름틀)
    let field_click = Control::field("여기를 클릭하세요");

    // Field: Date
    let field_date = Control::Field {
        field_type: FieldType::Date,
        hint_text: Some("날짜".to_string()),
        help_text: Some("문서 작성일".to_string()),
    };

    // Field: PageNumber (쪽번호)
    let field_page =
        Control::Field { field_type: FieldType::PageNum, hint_text: None, help_text: None };

    Section::with_paragraphs(
        vec![
            text_para("Wave 12: Bookmark + CrossRef + Field", CS_TITLE, PS_CENTER),
            empty_para(),
            // Point bookmark
            text_para("▶ 포인트 책갈피 (아래에 '중요위치' 책갈피 삽입):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text("이 위치에 책갈피가 있습니다", CharShapeIndex::new(CS_NORMAL)),
                    Run::control(bm_point, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(" ← 여기", CharShapeIndex::new(CS_BLUE)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // Span bookmark
            text_para("▶ 범위 책갈피 ('핵심내용' 범위):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::control(bm_span_start, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(
                        "이 텍스트는 '핵심내용' 책갈피 범위 안에 있습니다.",
                        CharShapeIndex::new(CS_BLUE),
                    ),
                    Run::control(bm_span_end, CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // Cross-reference
            text_para("▶ 상호참조 ('중요위치' 페이지번호 참조):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text("중요위치 책갈피는 ", CharShapeIndex::new(CS_NORMAL)),
                    Run::control(cross_ref, CharShapeIndex::new(CS_BLUE)),
                    Run::text("쪽에 있습니다.", CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // Fields
            text_para("▶ 누름틀 (ClickHere):", CS_RED, PS_LEFT),
            ctrl_para(field_click, CS_NORMAL, PS_BODY),
            empty_para(),
            text_para("▶ 날짜 필드 (SUMMERY → $modifiedtime):", CS_RED, PS_LEFT),
            ctrl_para(field_date, CS_NORMAL, PS_BODY),
            empty_para(),
            text_para("▶ 쪽번호 필드 (autoNum PAGE):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text("현재 페이지: ", CharShapeIndex::new(CS_NORMAL)),
                    Run::control(field_page, CharShapeIndex::new(CS_BLUE)),
                    Run::text(" 쪽", CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 10: Wave 12 — Memo + IndexMark
// ═══════════════════════════════════════════════════════════════════

fn section_memo_indexmark() -> Section {
    // Memo 1: 간단한 메모
    let memo_simple = Control::memo(
        vec![text_para("이 부분을 검토해주세요.", CS_NORMAL, PS_BODY)],
        "김검토",
        "2026-03-05",
    );

    // Memo 2: 긴 메모
    let memo_long = Control::memo(
        vec![
            text_para("수정 필요 사항:", CS_RED, PS_LEFT),
            text_para("1. 수치 데이터 재확인 필요", CS_NORMAL, PS_LEFT),
            text_para("2. 참고문헌 추가 필요", CS_NORMAL, PS_LEFT),
            text_para("3. 그래프 업데이트 요청", CS_NORMAL, PS_LEFT),
        ],
        "박수정",
        "2026-03-05",
    );

    // IndexMark 1: primary only
    let idx1 = Control::index_mark("한글문서");

    // IndexMark 2: primary + secondary
    let idx2 = Control::IndexMark {
        primary: "문서형식".to_string(),
        secondary: Some("HWPX".to_string()),
    };

    // IndexMark 3: another entry
    let idx3 = Control::IndexMark {
        primary: "문서형식".to_string(),
        secondary: Some("HWP5".to_string()),
    };

    Section::with_paragraphs(
        vec![
            text_para("Wave 12: Memo + IndexMark", CS_TITLE, PS_CENTER),
            empty_para(),
            // Memo 1
            text_para("▶ 간단한 메모 (김검토):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text(
                        "이 문장에 메모가 첨부되어 있습니다.",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                    Run::control(memo_simple, CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // Memo 2
            text_para("▶ 상세 메모 (박수정):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text(
                        "데이터 분석 결과를 여기에 기술합니다.",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                    Run::control(memo_long, CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // IndexMark
            text_para("▶ 찾아보기 표시 (IndexMark):", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text("한글문서", CharShapeIndex::new(CS_BLUE)),
                    Run::control(idx1, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(
                        "는 대한민국의 대표적인 워드프로세서입니다.",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                ],
                PS_BODY,
            ),
            multi_run_para(
                vec![
                    Run::text("HWPX 형식", CharShapeIndex::new(CS_BLUE)),
                    Run::control(idx2, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(
                        "은 XML 기반 국가표준(KS X 6101)입니다.",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                ],
                PS_BODY,
            ),
            multi_run_para(
                vec![
                    Run::text("HWP5 형식", CharShapeIndex::new(CS_BLUE)),
                    Run::control(idx3, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(
                        "은 바이너리 기반 레거시 형식입니다.",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                ],
                PS_BODY,
            ),
        ],
        PageSettings::a4(),
    )
}

// ═══════════════════════════════════════════════════════════════════
// Section 11: 기존 API + Wave 9/11/12 복합 테스트
// ═══════════════════════════════════════════════════════════════════

fn section_combined() -> Section {
    let ps = PageSettings {
        gutter: HwpUnit::from_mm(15.0).unwrap(),
        gutter_type: GutterType::LeftRight,
        ..PageSettings::a4()
    };
    let vis = Visibility {
        show_line_number: true,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowAll,
        ..Visibility::default()
    };
    let lns = LineNumberShape {
        restart_type: 1, // Section
        count_by: 10,
        distance: HwpUnit::new(1200).unwrap(),
        start_number: 1,
    };

    // Hyperlink + Bookmark in same paragraph
    let hyperlink = Control::hyperlink("HwpForge GitHub", "https://github.com/ai-screams/HwpForge");
    let bm = Control::bookmark("깃헙링크");

    // Equation
    let eq = Control::equation("{a^2 + b^2} = c^2");

    // Dutmal
    let dutmal_top = Control::dutmal("大韓民國", "대한민국");
    let mut dutmal_bottom = Control::dutmal("漢字", "한자");
    if let Control::Dutmal { ref mut position, .. } = dutmal_bottom {
        *position = DutmalPosition::Bottom;
    }

    // Footnote
    let footnote = Control::footnote(vec![text_para(
        "이것은 Wave 9/11/12 종합 테스트의 각주입니다.",
        CS_NORMAL,
        PS_BODY,
    )]);

    let mut section = Section::with_paragraphs(
        vec![
            text_para("종합 테스트: 모든 Wave 기능 복합", CS_TITLE, PS_CENTER),
            empty_para(),
            text_para(
                "이 섹션은 Wave 9 (페이지 레이아웃) + Wave 11 (도형) + Wave 12 (참조) 기능을 복합 사용합니다.",
                CS_NORMAL,
                PS_BODY,
            ),
            empty_para(),
            // Hyperlink + Bookmark
            text_para("▶ 하이퍼링크 + 책갈피:", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::control(hyperlink, CharShapeIndex::new(CS_BLUE)),
                    Run::text(" ", CharShapeIndex::new(CS_NORMAL)),
                    Run::control(bm, CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // Equation
            text_para("▶ 수식 (피타고라스 정리):", CS_RED, PS_LEFT),
            ctrl_para(eq, CS_NORMAL, PS_CENTER),
            empty_para(),
            // Dutmal (top + bottom)
            text_para("▶ 덧말 — 위/아래:", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::control(dutmal_top, CharShapeIndex::new(CS_NORMAL)),
                    Run::text("  /  ", CharShapeIndex::new(CS_NORMAL)),
                    Run::control(dutmal_bottom, CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_CENTER,
            ),
            empty_para(),
            // Footnote
            text_para("▶ 각주:", CS_RED, PS_LEFT),
            multi_run_para(
                vec![
                    Run::text(
                        "이 문장에는 각주가 달려 있습니다",
                        CharShapeIndex::new(CS_NORMAL),
                    ),
                    Run::control(footnote, CharShapeIndex::new(CS_NORMAL)),
                    Run::text(".", CharShapeIndex::new(CS_NORMAL)),
                ],
                PS_BODY,
            ),
            empty_para(),
            // ConnectLine with arrows
            text_para("▶ 연결선 (화살표):", CS_RED, PS_LEFT),
            {
                let mut cl = Control::connect_line(
                    ShapePoint::new(0, 2000),
                    ShapePoint::new(15000, 2000),
                )
                .unwrap();
                if let Control::ConnectLine { ref mut style, .. } = cl {
                    *style = Some(ShapeStyle {
                        line_color: Some(Color::from_rgb(100, 50, 150)),
                        line_width: Some(20),
                        head_arrow: Some(ArrowStyle {
                            arrow_type: ArrowType::Arrow,
                            size: ArrowSize::Medium, filled: true,
                        }),
                        tail_arrow: Some(ArrowStyle {
                            arrow_type: ArrowType::Arrow,
                            size: ArrowSize::Medium, filled: true,
                        }),
                        ..ShapeStyle::default()
                    });
                }
                ctrl_para(cl, CS_NORMAL, PS_CENTER)
            },
            empty_para(),
            text_para(
                "=== Wave 9/11/12 종합 테스트 완료 ===",
                CS_TITLE,
                PS_CENTER,
            ),
        ],
        ps,
    );
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);
    section.begin_num = Some(BeginNum::default());
    section
}

// ═══════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════

/// Helper: encode a single section into a standalone HWPX file.
fn write_single(
    name: &str,
    section: Section,
    style_store: &HwpxStyleStore,
    image_store: &ImageStore,
) {
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validation should pass");
    let bytes =
        HwpxEncoder::encode(&validated, style_store, image_store).expect("encode should succeed");
    let path = format!("temp/diag_{name}.hwpx");
    std::fs::write(&path, &bytes).expect("write should succeed");
    println!("  {path} ({} bytes)", bytes.len());
}

fn main() {
    println!("=== Wave 9/11/12 Diagnostic: Individual Section Files ===\n");

    let style_store = build_style_store();
    let image_store = ImageStore::new();

    // Create temp/ directory
    std::fs::create_dir_all("temp").ok();

    // Generate individual files for binary-search crash diagnosis
    println!("Generating individual section files:");
    write_single("01_gutter", section_gutter_mirror(), &style_store, &image_store);
    write_single("02_visibility", section_visibility_linenumber(), &style_store, &image_store);
    write_single("03_borderfill", section_border_fill_begin_num(), &style_store, &image_store);
    write_single("04_masterpage", section_master_page(), &style_store, &image_store);
    write_single("05_arcs", section_arcs(), &style_store, &image_store);
    write_single("06_curves", section_curves(), &style_store, &image_store);
    write_single("07_connectline", section_connect_lines(), &style_store, &image_store);
    write_single("08_shapestyles", section_shape_styles(), &style_store, &image_store);
    write_single("09_bookmarks", section_bookmarks_refs_fields(), &style_store, &image_store);
    write_single("10_memo", section_memo_indexmark(), &style_store, &image_store);
    write_single("11_combined", section_combined(), &style_store, &image_store);

    // Also generate the combined file
    let mut doc = Document::new();
    doc.add_section(section_gutter_mirror());
    doc.add_section(section_visibility_linenumber());
    doc.add_section(section_border_fill_begin_num());
    doc.add_section(section_master_page());
    doc.add_section(section_arcs());
    doc.add_section(section_curves());
    doc.add_section(section_connect_lines());
    doc.add_section(section_shape_styles());
    doc.add_section(section_bookmarks_refs_fields());
    doc.add_section(section_memo_indexmark());
    doc.add_section(section_combined());

    let validated = doc.validate().expect("validation should pass");
    let bytes =
        HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode should succeed");
    std::fs::write("wave9_11_12_output.hwpx", &bytes).expect("write should succeed");
    println!("\n  wave9_11_12_output.hwpx ({} bytes) — all 11 sections", bytes.len());

    println!("\n각 파일을 한글에서 열어서 어떤 섹션이 크래시하는지 확인하세요.");
    println!("크래시하는 파일 번호를 알려주시면 해당 섹션을 수정하겠습니다.");
}
