//! Enhanced feature showcase: generates 15 individual HWPX files with rich variations.
//!
//! Each file starts with a mascot intro (title + image + caption + description),
//! then demonstrates one feature category with multiple variations.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example feature_isolation
//!
//! Output:
//!   temp/01_text.hwpx
//!   temp/02_rich_text.hwpx
//!   temp/03_table.hwpx
//!   temp/04_header_footer.hwpx
//!   temp/05_footnote_endnote.hwpx
//!   temp/06_textbox.hwpx
//!   temp/07_line.hwpx
//!   temp/08_ellipse.hwpx
//!   temp/09_polygon.hwpx
//!   temp/10_multi_column.hwpx
//!   temp/11_image.hwpx
//!   temp/12_hyperlink.hwpx
//!   temp/13_equation.hwpx
//!   temp/14_chart.hwpx
//!   temp/15_shapes_advanced.hwpx

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::chart::{
    BarShape, ChartData, ChartGrouping, ChartType, LegendPosition, OfPieType, RadarStyle,
    ScatterStyle, StockVariant,
};
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{ArrowStyle, Control, Fill, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, ArcType, ArrowSize, ArrowType, CharShapeIndex, Color,
    CurveSegmentType, Flip, GradientType, HwpUnit, NumberFormatType, PageNumberPosition,
    ParaShapeIndex, PatternType, UnderlineType,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Constants ──────────────────────────────────────────────────────

const CS_NORMAL: u32 = 0;
const CS_BOLD: u32 = 1;
const CS_TITLE: u32 = 2;
const CS_RED: u32 = 3;
const CS_BLUE: u32 = 4;
const CS_SMALL: u32 = 5;
const CS_LINK: u32 = 6;
const CS_WHITE: u32 = 7;

const PS_LEFT: u32 = 0;
const PS_CENTER: u32 = 1;
const PS_RIGHT: u32 = 2;

// ── Helpers ────────────────────────────────────────────────────────

fn csi(idx: u32) -> CharShapeIndex {
    CharShapeIndex::new(idx as usize)
}

fn psi(idx: u32) -> ParaShapeIndex {
    ParaShapeIndex::new(idx as usize)
}

fn p(text: &str, cs: u32, ps: u32) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, csi(cs))], psi(ps))
}

fn empty() -> Paragraph {
    Paragraph::with_runs(vec![Run::text("", csi(CS_NORMAL))], psi(PS_LEFT))
}

/// Shared style store with 7 fonts, 6 char shapes, and 3 para shapes.
fn showcase_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    // 7 fonts (one per language group)
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "\u{d568}\u{cd08}\u{b868}\u{bc14}\u{d0d5}", lang));
    }

    // CS_NORMAL (0): 10pt, black
    store.push_char_shape(HwpxCharShape::default());

    // CS_BOLD (1): 12pt, bold, black
    let mut cs1 = HwpxCharShape::default();
    cs1.height = HwpUnit::from_pt(12.0).unwrap();
    cs1.bold = true;
    store.push_char_shape(cs1);

    // CS_TITLE (2): 16pt, bold, black
    let mut cs2 = HwpxCharShape::default();
    cs2.height = HwpUnit::from_pt(16.0).unwrap();
    cs2.bold = true;
    store.push_char_shape(cs2);

    // CS_RED (3): 10pt, red
    let mut cs3 = HwpxCharShape::default();
    cs3.text_color = Color::from_rgb(200, 0, 0);
    store.push_char_shape(cs3);

    // CS_BLUE (4): 10pt, blue
    let mut cs4 = HwpxCharShape::default();
    cs4.text_color = Color::from_rgb(0, 0, 200);
    store.push_char_shape(cs4);

    // CS_SMALL (5): 8pt, gray
    let mut cs5 = HwpxCharShape::default();
    cs5.height = HwpUnit::from_pt(8.0).unwrap();
    cs5.text_color = Color::from_rgb(128, 128, 128);
    store.push_char_shape(cs5);

    // CS_LINK (6): 10pt, blue (#0563C1) + underline — standard hyperlink style
    let mut cs6 = HwpxCharShape::default();
    cs6.text_color = Color::from_rgb(5, 99, 193);
    cs6.underline_type = UnderlineType::Bottom;
    cs6.underline_color = Some(Color::from_rgb(5, 99, 193));
    store.push_char_shape(cs6);

    // CS_WHITE (7): 10pt, white, bold — for dark gradient backgrounds
    let mut cs7 = HwpxCharShape::default();
    cs7.text_color = Color::from_rgb(255, 255, 255);
    cs7.bold = true;
    store.push_char_shape(cs7);

    // PS_LEFT (0): left alignment
    store.push_para_shape(HwpxParaShape::default());

    // PS_CENTER (1): center alignment
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    store.push_para_shape(ps1);

    // PS_RIGHT (2): right alignment
    let mut ps2 = HwpxParaShape::default();
    ps2.alignment = Alignment::Right;
    store.push_para_shape(ps2);

    store
}

/// Builds the mascot intro paragraphs and image store.
fn mascot_intro(title: &str, description: &str) -> (Vec<Paragraph>, ImageStore) {
    let mascot_bytes =
        std::fs::read("assets/mascot-main.png").expect("assets/mascot-main.png not found");
    let mut images = ImageStore::new();
    images.insert("image1.png", mascot_bytes);

    let mut img = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(60.0).unwrap(),
        HwpUnit::from_mm(60.0).unwrap(),
        ImageFormat::Png,
    );
    img.caption = Some(Caption::new(
        vec![p(
            "\u{c1e0}\u{bd80}\u{b9ac} (SoeBuri) \u{2014} \
             \u{d55c}\u{cef4} \u{bb38}\u{c11c}\u{b97c} \u{bd88}\u{c5d0} \
             \u{b2ec}\u{ad6c}\u{c5b4} \u{b2e8}\u{b2e8}\u{d558}\u{ac8c} \
             \u{bca8}\u{b824}\u{b0b4}\u{b294} \u{b300}\u{c7a5}\u{c7a5}\u{c774} \
             \u{c624}\u{b9ac}\u{b108}\u{ad6c}\u{b9ac}",
            CS_SMALL,
            PS_CENTER,
        )],
        CaptionSide::Bottom,
    ));
    let img_para = Paragraph::with_runs(vec![Run::image(img, csi(CS_NORMAL))], psi(PS_CENTER));

    let paras = vec![
        p(title, CS_TITLE, PS_CENTER),
        empty(),
        img_para,
        empty(),
        p(description, CS_NORMAL, PS_LEFT),
        empty(),
    ];
    (paras, images)
}

fn encode_and_save(name: &str, store: &HwpxStyleStore, doc: &Document, images: &ImageStore) {
    let path = format!("temp/{name}");
    let validated = doc.clone().validate().expect("validation");
    let bytes = HwpxEncoder::encode(&validated, store, images).expect("encode");
    std::fs::write(&path, &bytes).expect("write");
    println!("  OK {} ({} bytes)", path, bytes.len());
}

// ── Example generators ─────────────────────────────────────────────

fn gen_01_text() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "01. \u{ae30}\u{bcf8} \u{d14d}\u{c2a4}\u{d2b8}",
        "\u{ae30}\u{bcf8} \u{d14d}\u{c2a4}\u{d2b8} \u{cd9c}\u{b825} \
         \u{ae30}\u{b2a5}\u{c744} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Multiple paragraphs
    paras.push(p(
        "\u{ccab} \u{bc88}\u{c9f8} \u{bb38}\u{b2e8}\u{c785}\u{b2c8}\u{b2e4}. \
         \u{d55c}\u{ae00} \u{bb38}\u{c11c}\u{c758} \u{ae30}\u{bcf8} \
         \u{d14d}\u{c2a4}\u{d2b8} \u{cd9c}\u{b825}\u{c744} \u{d14c}\u{c2a4}\u{d2b8}\u{d569}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "\u{b450} \u{bc88}\u{c9f8} \u{bb38}\u{b2e8}\u{c785}\u{b2c8}\u{b2e4}. \
         \u{c624}\u{b978}\u{cabd} \u{c815}\u{b82c}\u{c744} \u{c801}\u{c6a9}\u{d588}\u{c2b5}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_RIGHT,
    ));
    paras.push(empty());

    // Korean + English + Chinese mixed text
    paras.push(p(
        "\u{d55c}\u{ae00} English \u{6f22}\u{5b57} \u{d63c}\u{d569} \
         \u{d14d}\u{c2a4}\u{d2b8}: Mixed-language text \u{d14c}\u{c2a4}\u{d2b8}\u{c785}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Very long paragraph (150+ chars)
    let long = "\u{c774}\u{ac83}\u{c740} \u{b9e4}\u{c6b0} \u{ae34} \
        \u{bb38}\u{b2e8}\u{c785}\u{b2c8}\u{b2e4}. \u{c6cc}\u{b4dc}\u{b7a9} \
        \u{ae30}\u{b2a5}\u{c744} \u{d14c}\u{c2a4}\u{d2b8}\u{d558}\u{ae30} \
        \u{c704}\u{d574} \u{c791}\u{c131}\u{b418}\u{c5c8}\u{c2b5}\u{b2c8}\u{b2e4}. \
        HwpForge\u{b294} Rust\u{b85c} \u{c791}\u{c131}\u{b41c} \u{d55c}\u{ae00} \
        \u{bb38}\u{c11c} \u{c0dd}\u{c131} \u{b77c}\u{c774}\u{be0c}\u{b7ec}\u{b9ac}\u{c785}\u{b2c8}\u{b2e4}. \
        AI \u{c5d0}\u{c774}\u{c804}\u{d2b8}\u{ac00} \u{d55c}\u{ae00} \u{bb38}\u{c11c}\u{b97c} \
        \u{d504}\u{b85c}\u{adf8}\u{b798}\u{b9e4}\u{d2f1}\u{d558}\u{ac8c} \u{c0dd}\u{c131}\u{d560} \
        \u{c218} \u{c788}\u{b3c4}\u{b85d} \u{c124}\u{acc4}\u{b418}\u{c5c8}\u{c2b5}\u{b2c8}\u{b2e4}. \
        \u{c774} \u{bb38}\u{b2e8}\u{c740} 150\u{c790} \u{c774}\u{c0c1}\u{c73c}\u{b85c} \
        \u{c791}\u{c131}\u{b418}\u{c5b4} \u{c790}\u{b3d9} \u{c904}\u{bc14}\u{afbc}\u{c774} \
        \u{c815}\u{c0c1}\u{c801}\u{c73c}\u{b85c} \u{b3d9}\u{c791}\u{d558}\u{b294}\u{c9c0} \
        \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.";
    paras.push(p(long, CS_NORMAL, PS_LEFT));

    // Page break between two paragraphs
    let mut page2 = p(
        "\u{c774} \u{bb38}\u{b2e8}\u{c740} \u{c0c8} \u{d398}\u{c774}\u{c9c0}\u{c5d0}\u{c11c} \
         \u{c2dc}\u{c791}\u{b429}\u{b2c8}\u{b2e4}. page_break = true \u{c124}\u{c815}.",
        CS_NORMAL,
        PS_LEFT,
    );
    page2.page_break = true;
    paras.push(page2);
    paras.push(p(
        "\u{d398}\u{c774}\u{c9c0} \u{be0c}\u{b808}\u{c774}\u{d06c} \u{c774}\u{d6c4}\u{c758} \
         \u{cd94}\u{ac00} \u{b0b4}\u{c6a9}\u{c785}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_LEFT,
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("01_text.hwpx", &store, &doc, &images);
}

fn gen_02_rich_text() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "02. \u{c11c}\u{c2dd} \u{d14d}\u{c2a4}\u{d2b8}",
        "\u{b2e4}\u{c591}\u{d55c} \u{ae00}\u{c790} \u{c11c}\u{c2dd}(CharShape) \
         \u{c870}\u{d569}\u{c744} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Bold text
    paras.push(p("\u{ad75}\u{c740} \u{ae00}\u{c528} (12pt Bold)", CS_BOLD, PS_LEFT));
    // Red text
    paras.push(p("\u{be68}\u{ac04} \u{ae00}\u{c528} (Red, 10pt)", CS_RED, PS_LEFT));
    // Blue text
    paras.push(p("\u{d30c}\u{b780} \u{ae00}\u{c528} (Blue, 10pt)", CS_BLUE, PS_LEFT));
    // Small gray text
    paras.push(p(
        "\u{c791}\u{c740} \u{d68c}\u{c0c9} \u{ae00}\u{c528} (Gray, 8pt)",
        CS_SMALL,
        PS_LEFT,
    ));
    // Title text
    paras.push(p(
        "\u{c81c}\u{baa9} \u{d06c}\u{ae30} \u{ae00}\u{c528} (16pt Bold)",
        CS_TITLE,
        PS_CENTER,
    ));
    paras.push(empty());

    // Mixed run paragraph: normal + bold + red + blue + normal
    let mixed = Paragraph::with_runs(
        vec![
            Run::text("\u{c77c}\u{bc18} ", csi(CS_NORMAL)),
            Run::text("\u{ad75}\u{c740} ", csi(CS_BOLD)),
            Run::text("\u{be68}\u{ac04} ", csi(CS_RED)),
            Run::text("\u{d30c}\u{b780} ", csi(CS_BLUE)),
            Run::text("\u{b2e4}\u{c2dc} \u{c77c}\u{bc18}", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    );
    paras.push(mixed);
    paras.push(empty());

    // Multiple styled paragraphs
    paras.push(p(
        "\u{c67c}\u{cabd} \u{c815}\u{b82c} + \u{ad75}\u{c740} \u{ae00}\u{c528}",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(p(
        "\u{c624}\u{b978}\u{cabd} \u{c815}\u{b82c} + \u{be68}\u{ac04} \u{ae00}\u{c528}",
        CS_RED,
        PS_RIGHT,
    ));
    paras.push(p(
        "\u{c911}\u{c559} \u{c815}\u{b82c} + \u{d30c}\u{b780} \u{ae00}\u{c528}",
        CS_BLUE,
        PS_CENTER,
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("02_rich_text.hwpx", &store, &doc, &images);
}

fn gen_03_table() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "03. \u{d45c}",
        "\u{b2e4}\u{c591}\u{d55c} \u{d45c} \u{b808}\u{c774}\u{c544}\u{c6c3}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Simple 2x3 table (no caption)
    let cw = HwpUnit::new(21260).unwrap();
    let t1 = Table::new(vec![
        TableRow::new(vec![
            TableCell::new(vec![p("A1", CS_NORMAL, PS_LEFT)], cw),
            TableCell::new(vec![p("B1", CS_NORMAL, PS_LEFT)], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("A2", CS_NORMAL, PS_LEFT)], cw),
            TableCell::new(vec![p("B2", CS_NORMAL, PS_LEFT)], cw),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("A3", CS_NORMAL, PS_LEFT)], cw),
            TableCell::new(vec![p("B3", CS_NORMAL, PS_LEFT)], cw),
        ]),
    ]);
    paras.push(p("\u{ac04}\u{b2e8}\u{d55c} 2x3 \u{d45c}:", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(vec![Run::table(t1, csi(CS_NORMAL))], psi(PS_LEFT)));
    paras.push(empty());

    // Wide 4x5 table with caption and header row
    let cw4 = HwpUnit::new(10630).unwrap();
    let mut rows = vec![TableRow::new(vec![
        TableCell::new(vec![p("\u{d56d}\u{baa9}", CS_BOLD, PS_CENTER)], cw4),
        TableCell::new(vec![p("\u{c124}\u{ba85}", CS_BOLD, PS_CENTER)], cw4),
        TableCell::new(vec![p("\u{be44}\u{ace0}", CS_BOLD, PS_CENTER)], cw4),
        TableCell::new(vec![p("\u{c0c1}\u{d0dc}", CS_BOLD, PS_CENTER)], cw4),
    ])];
    for i in 1..=4 {
        rows.push(TableRow::new(vec![
            TableCell::new(vec![p(&format!("\u{d56d}\u{baa9} {i}"), CS_NORMAL, PS_LEFT)], cw4),
            TableCell::new(
                vec![p(&format!("\u{c124}\u{ba85} \u{b0b4}\u{c6a9} {i}"), CS_NORMAL, PS_LEFT)],
                cw4,
            ),
            TableCell::new(vec![p(&format!("\u{cc38}\u{ace0} {i}"), CS_SMALL, PS_LEFT)], cw4),
            TableCell::new(
                vec![p(
                    if i % 2 == 0 { "\u{c644}\u{b8cc}" } else { "\u{c9c4}\u{d589}\u{c911}" },
                    CS_NORMAL,
                    PS_CENTER,
                )],
                cw4,
            ),
        ]));
    }
    let mut t2 = Table::new(rows);
    t2.caption = Some(Caption::new(
        vec![p(
            "\u{d45c} 1. \u{d504}\u{b85c}\u{c81d}\u{d2b8} \u{d604}\u{d669}",
            CS_SMALL,
            PS_CENTER,
        )],
        CaptionSide::Bottom,
    ));
    paras.push(p("4x5 Table (with Caption):", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(vec![Run::table(t2, csi(CS_NORMAL))], psi(PS_LEFT)));
    paras.push(empty());

    // Single-column narrow table (list-style)
    let full_w = HwpUnit::new(42520).unwrap();
    let t3 = Table::new(vec![
        TableRow::new(vec![TableCell::new(
            vec![p("1. \u{ccab} \u{bc88}\u{c9f8} \u{d56d}\u{baa9}", CS_NORMAL, PS_LEFT)],
            full_w,
        )]),
        TableRow::new(vec![TableCell::new(
            vec![p("2. \u{b450} \u{bc88}\u{c9f8} \u{d56d}\u{baa9}", CS_NORMAL, PS_LEFT)],
            full_w,
        )]),
        TableRow::new(vec![TableCell::new(
            vec![p("3. \u{c138} \u{bc88}\u{c9f8} \u{d56d}\u{baa9}", CS_NORMAL, PS_LEFT)],
            full_w,
        )]),
    ]);
    paras.push(p(
        "\u{b2e8}\u{c77c} \u{c5f4} \u{d45c} (\u{baa9}\u{b85d} \u{c2a4}\u{d0c0}\u{c77c}):",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(vec![Run::table(t3, csi(CS_NORMAL))], psi(PS_LEFT)));
    paras.push(empty());

    // Merged cells table (col_span + row_span)
    let cw3 = HwpUnit::new(14173).unwrap(); // ~50mm per column (3 columns)
    let t4 = Table::new(vec![
        // Row 0: header spanning all 3 columns
        TableRow::new(vec![TableCell::with_span(
            vec![p("2026년 분기별 실적 요약", CS_BOLD, PS_CENTER)],
            cw3,
            3,
            1,
        )]),
        // Row 1: sub-headers
        TableRow::new(vec![
            TableCell::new(vec![p("분기", CS_BOLD, PS_CENTER)], cw3),
            TableCell::new(vec![p("매출 (억원)", CS_BOLD, PS_CENTER)], cw3),
            TableCell::new(vec![p("비고", CS_BOLD, PS_CENTER)], cw3),
        ]),
        // Row 2-3: "상반기" row_span=2
        TableRow::new(vec![
            TableCell::with_span(vec![p("상반기", CS_NORMAL, PS_CENTER)], cw3, 1, 2),
            TableCell::new(vec![p("1Q: 125", CS_NORMAL, PS_CENTER)], cw3),
            TableCell::new(vec![p("전년 대비 +12%", CS_SMALL, PS_LEFT)], cw3),
        ]),
        TableRow::new(vec![
            // 상반기 row_span covers this row's first cell
            TableCell::new(vec![p("2Q: 143", CS_NORMAL, PS_CENTER)], cw3),
            TableCell::new(vec![p("신규 사업 반영", CS_SMALL, PS_LEFT)], cw3),
        ]),
        // Row 4-5: "하반기" row_span=2
        TableRow::new(vec![
            TableCell::with_span(vec![p("하반기", CS_NORMAL, PS_CENTER)], cw3, 1, 2),
            TableCell::new(vec![p("3Q: 158", CS_NORMAL, PS_CENTER)], cw3),
            TableCell::new(vec![p("최고 실적", CS_SMALL, PS_LEFT)], cw3),
        ]),
        TableRow::new(vec![
            // 하반기 row_span covers this row's first cell
            TableCell::new(vec![p("4Q: 131", CS_NORMAL, PS_CENTER)], cw3),
            TableCell::new(vec![p("계절적 감소", CS_SMALL, PS_LEFT)], cw3),
        ]),
        // Row 6: footer spanning all 3 columns
        TableRow::new(vec![TableCell::with_span(
            vec![p("연간 합계: 557억원", CS_BOLD, PS_CENTER)],
            cw3,
            3,
            1,
        )]),
    ]);
    paras.push(p("셀 병합 표 (col_span + row_span):", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(vec![Run::table(t4, csi(CS_NORMAL))], psi(PS_LEFT)));
    paras.push(empty());

    // Complex merged cells: mixed col_span and row_span
    let cw5 = HwpUnit::new(8504).unwrap(); // ~30mm per column (5 columns)
    let t5 = Table::new(vec![
        // Row 0: title spanning 5 columns
        TableRow::new(vec![TableCell::with_span(
            vec![p("부서별 평가 현황", CS_BOLD, PS_CENTER)],
            cw5,
            5,
            1,
        )]),
        // Row 1: group headers (2+3 col_span)
        TableRow::new(vec![
            TableCell::with_span(vec![p("부서 정보", CS_BOLD, PS_CENTER)], cw5, 2, 1),
            TableCell::with_span(vec![p("평가 결과", CS_BOLD, PS_CENTER)], cw5, 3, 1),
        ]),
        // Row 2: sub-headers
        TableRow::new(vec![
            TableCell::new(vec![p("부서", CS_BOLD, PS_CENTER)], cw5),
            TableCell::new(vec![p("인원", CS_BOLD, PS_CENTER)], cw5),
            TableCell::new(vec![p("업무", CS_BOLD, PS_CENTER)], cw5),
            TableCell::new(vec![p("협업", CS_BOLD, PS_CENTER)], cw5),
            TableCell::new(vec![p("혁신", CS_BOLD, PS_CENTER)], cw5),
        ]),
        // Row 3-4: 개발팀 row_span=2
        TableRow::new(vec![
            TableCell::with_span(vec![p("개발팀", CS_NORMAL, PS_CENTER)], cw5, 1, 2),
            TableCell::new(vec![p("15명", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("B+", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A+", CS_NORMAL, PS_CENTER)], cw5),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![p("8명", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A+", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("B", CS_NORMAL, PS_CENTER)], cw5),
        ]),
        // Row 5: 디자인팀
        TableRow::new(vec![
            TableCell::new(vec![p("디자인팀", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("6명", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A+", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A", CS_NORMAL, PS_CENTER)], cw5),
            TableCell::new(vec![p("A", CS_NORMAL, PS_CENTER)], cw5),
        ]),
    ]);
    paras.push(p("복합 병합 표 (5열, 그룹 헤더 + row_span):", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(vec![Run::table(t5, csi(CS_NORMAL))], psi(PS_LEFT)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("03_table.hwpx", &store, &doc, &images);
}

fn gen_04_header_footer() {
    let store = showcase_store();
    let (paras1, images) = mascot_intro(
        "04. \u{ba38}\u{b9ac}\u{ae00}/\u{bc14}\u{b2e5}\u{ae00}/\u{d398}\u{c774}\u{c9c0}\u{bc88}\u{d638}",
        "\u{c139}\u{c158}\u{bcf4} \u{b2e4}\u{b978} \u{ba38}\u{b9ac}\u{ae00}/\u{bc14}\u{b2e5}\u{ae00} \
         \u{c870}\u{d569}\u{c744} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Section 1: Header + Footer + PageNumber BottomCenter
    let mut sec1 = Section::with_paragraphs(paras1, PageSettings::a4());
    sec1.header = Some(HeaderFooter::new(
        vec![p(
            "\u{c139}\u{c158} 1 \u{ba38}\u{b9ac}\u{ae00} \u{2014} HwpForge \u{ae30}\u{b2a5} \u{c2dc}\u{c5f0}",
            CS_SMALL,
            PS_CENTER,
        )],
        ApplyPageType::Both,
    ));
    sec1.footer = Some(HeaderFooter::new(
        vec![p("Copyright 2026 HwpForge", CS_SMALL, PS_CENTER)],
        ApplyPageType::Both,
    ));
    sec1.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    // Section 2: Different header, TopRight page number
    let mut sec2 = Section::with_paragraphs(
        vec![
            p(
                "\u{c139}\u{c158} 2 \u{b0b4}\u{c6a9}",
                CS_BOLD,
                PS_LEFT,
            ),
            p(
                "\u{c774} \u{c139}\u{c158}\u{c740} \u{b2e4}\u{b978} \u{ba38}\u{b9ac}\u{ae00}\u{acfc} \
                 \u{c624}\u{b978}\u{cabd} \u{c0c1}\u{b2e8} \u{d398}\u{c774}\u{c9c0} \u{bc88}\u{d638}\u{b97c} \
                 \u{c0ac}\u{c6a9}\u{d569}\u{b2c8}\u{b2e4}.",
                CS_NORMAL,
                PS_LEFT,
            ),
        ],
        PageSettings::a4(),
    );
    sec2.header = Some(HeaderFooter::new(
        vec![p(
            "\u{c139}\u{c158} 2 \u{ba38}\u{b9ac}\u{ae00} \u{2014} \u{b2e4}\u{b978} \u{b0b4}\u{c6a9}",
            CS_SMALL,
            PS_RIGHT,
        )],
        ApplyPageType::Both,
    ));
    sec2.page_number = Some(PageNumber::new(PageNumberPosition::TopRight, NumberFormatType::Digit));

    // Section 3: Decorated page number (roman numerals)
    let mut sec3 = Section::with_paragraphs(
        vec![
            p(
                "\u{c139}\u{c158} 3 \u{b0b4}\u{c6a9}",
                CS_BOLD,
                PS_LEFT,
            ),
            p(
                "\u{b85c}\u{b9c8} \u{c22b}\u{c790} \u{d398}\u{c774}\u{c9c0} \u{bc88}\u{d638}\u{c640} \
                 \u{c7a5}\u{c2dd} \u{bb38}\u{c790}\u{b97c} \u{c0ac}\u{c6a9}\u{d569}\u{b2c8}\u{b2e4}.",
                CS_NORMAL,
                PS_LEFT,
            ),
        ],
        PageSettings::a4(),
    );
    sec3.header = Some(HeaderFooter::new(
        vec![p("HwpForge Documentation", CS_SMALL, PS_LEFT)],
        ApplyPageType::Both,
    ));
    sec3.footer = Some(HeaderFooter::new(
        vec![p("\u{bc14}\u{b2e5}\u{ae00} \u{c139}\u{c158} 3", CS_SMALL, PS_RIGHT)],
        ApplyPageType::Both,
    ));
    sec3.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::RomanCapital,
        "- ",
    ));

    let mut doc = Document::new();
    doc.add_section(sec1);
    doc.add_section(sec2);
    doc.add_section(sec3);
    encode_and_save("04_header_footer.hwpx", &store, &doc, &images);
}

fn gen_05_footnote_endnote() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "05. \u{ac01}\u{c8fc}/\u{bbf8}\u{c8fc}",
        "\u{ac01}\u{c8fc}\u{c640} \u{bbf8}\u{c8fc}\u{c758} \u{b2e4}\u{c591}\u{d55c} \
         \u{c870}\u{d569}\u{c744} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Single footnote
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c774} \u{bb38}\u{c7a5}\u{c5d0}\u{b294} \u{ac01}\u{c8fc}\u{ac00} \u{c788}\u{c2b5}\u{b2c8}\u{b2e4}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote_with_id(
                    1,
                    vec![p(
                        "\u{ac01}\u{c8fc} 1: HwpForge\u{b294} Rust\u{b85c} \u{c791}\u{c131}\u{b41c} \
                         \u{d55c}\u{ae00} \u{bb38}\u{c11c} \u{b77c}\u{c774}\u{be0c}\u{b7ec}\u{b9ac}\u{c785}\u{b2c8}\u{b2e4}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Two footnotes in same paragraph
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{ccab} \u{bc88}\u{c9f8} \u{ac01}\u{c8fc}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote_with_id(
                    2,
                    vec![p(
                        "\u{ac01}\u{c8fc} 2: \u{ccab} \u{bc88}\u{c9f8} \u{cc38}\u{ace0} \u{c790}\u{b8cc}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(
                "\u{c640} \u{b450} \u{bc88}\u{c9f8} \u{ac01}\u{c8fc}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote_with_id(
                    3,
                    vec![p(
                        "\u{ac01}\u{c8fc} 3: \u{b450} \u{bc88}\u{c9f8} \u{cc38}\u{ace0} \u{c790}\u{b8cc}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(
                "\u{ac00} \u{d55c} \u{bb38}\u{b2e8}\u{c5d0} \u{c788}\u{c2b5}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Endnote
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c774} \u{bb38}\u{c7a5}\u{c5d0}\u{b294} \u{bbf8}\u{c8fc}\u{ac00} \u{c788}\u{c2b5}\u{b2c8}\u{b2e4}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::endnote_with_id(
                    1,
                    vec![p(
                        "\u{bbf8}\u{c8fc} 1: \u{bb38}\u{c11c} \u{b9c8}\u{c9c0}\u{b9c9}\u{c5d0} \
                         \u{d45c}\u{c2dc}\u{b418}\u{b294} \u{cc38}\u{ace0} \u{c790}\u{b8cc}\u{c785}\u{b2c8}\u{b2e4}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Both footnote AND endnote in same paragraph
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{ac01}\u{c8fc}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote_with_id(
                    4,
                    vec![p(
                        "\u{ac01}\u{c8fc} 4: \u{ac01}\u{c8fc}\u{c640} \u{bbf8}\u{c8fc}\u{ac00} \
                         \u{d568}\u{aed8} \u{c0ac}\u{c6a9}\u{b41c} \u{c608}\u{c2dc}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(
                "\u{c640} \u{bbf8}\u{c8fc}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::endnote_with_id(
                    2,
                    vec![p(
                        "\u{bbf8}\u{c8fc} 2: \u{ac01}\u{c8fc}\u{c640} \u{bbf8}\u{c8fc}\u{b97c} \
                         \u{d568}\u{aed8} \u{c0ac}\u{c6a9}\u{d560} \u{c218} \u{c788}\u{c2b5}\u{b2c8}\u{b2e4}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(
                "\u{ac00} \u{d568}\u{aed8} \u{c788}\u{b294} \u{bb38}\u{b2e8}\u{c785}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Footnote with long content
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{ae34} \u{ac01}\u{c8fc} \u{b0b4}\u{c6a9} \u{d14c}\u{c2a4}\u{d2b8}",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote_with_id(
                    5,
                    vec![p(
                        "\u{ac01}\u{c8fc} 5: \u{c774}\u{ac83}\u{c740} \u{b9e4}\u{c6b0} \u{ae34} \
                         \u{ac01}\u{c8fc} \u{b0b4}\u{c6a9}\u{c785}\u{b2c8}\u{b2e4}. \u{c5ec}\u{b7ec} \
                         \u{bb38}\u{c7a5}\u{c73c}\u{b85c} \u{ad6c}\u{c131}\u{b418}\u{c5b4} \u{c788}\u{c73c}\u{ba70}, \
                         \u{ac01}\u{c8fc}\u{ac00} \u{ae38}\u{c5b4}\u{c9c8} \u{b54c} \u{c790}\u{b3d9}\u{c73c}\u{b85c} \
                         \u{c904}\u{bc14}\u{afbc}\u{c774} \u{b418}\u{b294}\u{c9c0} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}. \
                         HwpForge\u{b294} \u{c774}\u{b7ec}\u{d55c} \u{ae34} \u{ac01}\u{c8fc}\u{b3c4} \
                         \u{c815}\u{c0c1}\u{c801}\u{c73c}\u{b85c} \u{cc98}\u{b9ac}\u{d569}\u{b2c8}\u{b2e4}.",
                        CS_NORMAL,
                        PS_LEFT,
                    )],
                ),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("05_footnote_endnote.hwpx", &store, &doc, &images);
}

fn gen_06_textbox() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "06. \u{ae00}\u{c0c1}\u{c790}",
        "\u{b2e4}\u{c591}\u{d55c} \u{ae00}\u{c0c1}\u{c790} \u{c2a4}\u{d0c0}\u{c77c}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Basic textbox (no style, no caption)
    paras.push(p("\u{ae30}\u{bcf8} \u{ae00}\u{c0c1}\u{c790}:", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![p(
                    "\u{ae30}\u{bcf8} \u{ae00}\u{c0c1}\u{c790} \u{b0b4}\u{c6a9}\u{c785}\u{b2c8}\u{b2e4}.",
                    CS_NORMAL,
                    PS_LEFT,
                )],
                width: HwpUnit::from_mm(80.0).unwrap(),
                height: HwpUnit::from_mm(20.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: None,
            },
            csi(CS_NORMAL),
        )],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // TextBox with caption
    paras.push(p("TextBox with Caption:", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![p("This is a TextBox with a caption.", CS_NORMAL, PS_LEFT)],
                width: HwpUnit::from_mm(80.0).unwrap(),
                height: HwpUnit::from_mm(20.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(Caption::new(
                    vec![p("TextBox 1. Caption test", CS_SMALL, PS_CENTER)],
                    CaptionSide::Bottom,
                )),
                style: None,
            },
            csi(CS_NORMAL),
        )],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // TextBox with solid fill style (blue border, light blue fill)
    paras.push(p(
        "\u{c2a4}\u{d0c0}\u{c77c} \u{c801}\u{c6a9} \u{ae00}\u{c0c1}\u{c790}:",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![p(
                    "\u{d30c}\u{b780} \u{d14c}\u{b450}\u{b9ac} + \u{c5f0}\u{d55c} \u{d30c}\u{b780} \u{bc30}\u{acbd}",
                    CS_BLUE,
                    PS_CENTER,
                )],
                width: HwpUnit::from_mm(80.0).unwrap(),
                height: HwpUnit::from_mm(25.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(0, 0, 200)),
                    fill_color: Some(Color::from_rgb(200, 220, 255)),
                    line_width: Some(50),
                    ..Default::default()
                }),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // TextBox with red thick border
    paras.push(p(
        "\u{be68}\u{ac04} \u{b450}\u{ae4c}\u{c6b4} \u{d14c}\u{b450}\u{b9ac}:",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![p(
                    "\u{be68}\u{ac04} \u{b450}\u{ae4c}\u{c6b4} \u{d14c}\u{b450}\u{b9ac} \u{ae00}\u{c0c1}\u{c790}",
                    CS_RED,
                    PS_CENTER,
                )],
                width: HwpUnit::from_mm(80.0).unwrap(),
                height: HwpUnit::from_mm(20.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(200, 0, 0)),
                    line_width: Some(150),
                    ..Default::default()
                }),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Small (40mm x 15mm)
    paras.push(p("\u{d06c}\u{ae30} \u{be44}\u{ad50} (Small / Medium / Large):", CS_BOLD, PS_LEFT));
    for (label, w, h) in
        [("Small 40x15", 40.0, 15.0), ("Medium 80x30", 80.0, 30.0), ("Large 120x50", 120.0, 50.0)]
    {
        paras.push(Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    paragraphs: vec![p(label, CS_NORMAL, PS_CENTER)],
                    width: HwpUnit::from_mm(w).unwrap(),
                    height: HwpUnit::from_mm(h).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
                },
                csi(CS_NORMAL),
            )],
            psi(PS_LEFT),
        ));
    }
    paras.push(empty());

    // TextBox with multiple paragraphs inside
    paras.push(p("\u{c5ec}\u{b7ec} \u{bb38}\u{b2e8} \u{ae00}\u{c0c1}\u{c790}:", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![
                    p(
                        "\u{ccab} \u{bc88}\u{c9f8} \u{bb38}\u{b2e8}",
                        CS_BOLD,
                        PS_LEFT,
                    ),
                    p(
                        "\u{b450} \u{bc88}\u{c9f8} \u{bb38}\u{b2e8} \u{2014} \u{c77c}\u{bc18} \u{d14d}\u{c2a4}\u{d2b8}",
                        CS_NORMAL,
                        PS_LEFT,
                    ),
                    p(
                        "\u{c138} \u{bc88}\u{c9f8} \u{bb38}\u{b2e8} \u{2014} \u{be68}\u{ac04} \u{d14d}\u{c2a4}\u{d2b8}",
                        CS_RED,
                        PS_LEFT,
                    ),
                ],
                width: HwpUnit::from_mm(100.0).unwrap(),
                height: HwpUnit::from_mm(40.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(100, 100, 100)),
                    line_width: Some(30),
                    ..Default::default()
                }),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_LEFT),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("06_textbox.hwpx", &store, &doc, &images);
}

fn gen_07_line() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "07. \u{c120}",
        "\u{b2e4}\u{c591}\u{d55c} \u{c120} \u{c2a4}\u{d0c0}\u{c77c}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    let make_line =
        |sx: i32, sy: i32, ex: i32, ey: i32, w: i32, h: i32, style: Option<ShapeStyle>| {
            Paragraph::with_runs(
                vec![Run::control(
                    Control::Line {
                        start: ShapePoint::new(sx, sy),
                        end: ShapePoint::new(ex, ey),
                        width: HwpUnit::new(w).unwrap(),
                        height: HwpUnit::new(h).unwrap(),
                        horz_offset: 0,
                        vert_offset: 0,
                        caption: None,
                        style,
                    },
                    csi(CS_NORMAL),
                )],
                psi(PS_LEFT),
            )
        };

    // Horizontal line
    paras.push(p("\u{c218}\u{d3c9}\u{c120}:", CS_BOLD, PS_LEFT));
    paras.push(make_line(0, 0, 14000, 0, 14000, 100, None));
    paras.push(empty());

    // Vertical line
    paras.push(p("\u{c218}\u{c9c1}\u{c120}:", CS_BOLD, PS_LEFT));
    paras.push(make_line(0, 0, 0, 8000, 100, 8000, None));
    paras.push(empty());

    // Diagonal line
    paras.push(p("\u{b300}\u{ac01}\u{c120}:", CS_BOLD, PS_LEFT));
    paras.push(make_line(0, 0, 10000, 5000, 10000, 5000, None));
    paras.push(empty());

    // 5 line styles
    let styles_data: &[(&str, LineStyle)] = &[
        ("Solid", LineStyle::Solid),
        ("Dash", LineStyle::Dash),
        ("Dot", LineStyle::Dot),
        ("DashDot", LineStyle::DashDot),
        ("DashDotDot", LineStyle::DashDotDot),
    ];
    paras.push(p("\u{c120} \u{c2a4}\u{d0c0}\u{c77c} 5\u{c885}:", CS_BOLD, PS_LEFT));
    for (label, ls) in styles_data {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(make_line(
            0,
            0,
            14000,
            0,
            14000,
            100,
            Some(ShapeStyle { line_style: Some(*ls), line_width: Some(30), ..Default::default() }),
        ));
    }
    paras.push(empty());

    // 3 widths
    paras.push(p("\u{c120} \u{b450}\u{aed8} 3\u{c885}:", CS_BOLD, PS_LEFT));
    for (label, width) in [("Thin (20)", 20u32), ("Medium (50)", 50), ("Thick (100)", 100)] {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(make_line(
            0,
            0,
            14000,
            0,
            14000,
            100,
            Some(ShapeStyle { line_width: Some(width), ..Default::default() }),
        ));
    }
    paras.push(empty());

    // 4 colors
    paras.push(p("\u{c120} \u{c0c9}\u{c0c1} 4\u{c885}:", CS_BOLD, PS_LEFT));
    for (label, color) in [
        ("Red", Color::from_rgb(200, 0, 0)),
        ("Blue", Color::from_rgb(0, 0, 200)),
        ("Green", Color::from_rgb(0, 150, 0)),
        ("Orange", Color::from_rgb(255, 140, 0)),
    ] {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(make_line(
            0,
            0,
            14000,
            0,
            14000,
            100,
            Some(ShapeStyle {
                line_color: Some(color),
                line_width: Some(50),
                ..Default::default()
            }),
        ));
    }

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("07_line.hwpx", &store, &doc, &images);
}

fn gen_08_ellipse() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "08. \u{d0c0}\u{c6d0}",
        "\u{b2e4}\u{c591}\u{d55c} \u{d0c0}\u{c6d0} \u{c2a4}\u{d0c0}\u{c77c}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    let make_ellipse = |w: i32, h: i32, text: &str, style: Option<ShapeStyle>| {
        Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    center: ShapePoint::new(w / 2, h / 2),
                    axis1: ShapePoint::new(w, h / 2),
                    axis2: ShapePoint::new(w / 2, h),
                    width: HwpUnit::new(w).unwrap(),
                    height: HwpUnit::new(h).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: if text.is_empty() {
                        vec![empty()]
                    } else {
                        vec![p(text, CS_NORMAL, PS_CENTER)]
                    },
                    caption: None,
                    style,
                },
                csi(CS_NORMAL),
            )],
            psi(PS_LEFT),
        )
    };

    // Perfect circle
    paras.push(p("\u{c815}\u{c6d0} (8000x8000):", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(8000, 8000, "\u{c815}\u{c6d0}", None));
    paras.push(empty());

    // Wide ellipse
    paras.push(p("\u{b113}\u{c740} \u{d0c0}\u{c6d0} (12000x6000):", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(12000, 6000, "\u{b113}\u{c740} \u{d0c0}\u{c6d0}", None));
    paras.push(empty());

    // Tall ellipse
    paras.push(p("\u{b192}\u{c740} \u{d0c0}\u{c6d0} (6000x12000):", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(6000, 12000, "\u{b192}\u{c740} \u{d0c0}\u{c6d0}", None));
    paras.push(empty());

    // Ellipse with solid fill
    paras.push(p("\u{c0c9}\u{c0c1} \u{cc44}\u{c6b0}\u{ae30}:", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(
        10000,
        6000,
        "\u{be68}\u{ac04} \u{d14c}\u{b450}\u{b9ac}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(200, 0, 0)),
            fill_color: Some(Color::from_rgb(255, 200, 200)),
            line_width: Some(50),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Ellipse with dashed outline
    paras.push(p("\u{c810}\u{c120} \u{d14c}\u{b450}\u{b9ac}:", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(
        10000,
        6000,
        "\u{c810}\u{c120}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 0, 200)),
            line_style: Some(LineStyle::Dash),
            line_width: Some(50),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Ellipse with text inside
    paras.push(p("\u{c6d0} \u{b0b4}\u{bd80} \u{d14d}\u{c2a4}\u{d2b8}:", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(
        10000,
        8000,
        "\u{c6d0} \u{b0b4}\u{bd80} \u{d14d}\u{c2a4}\u{d2b8}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 150, 0)),
            fill_color: Some(Color::from_rgb(200, 255, 200)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Ellipse without text (empty paragraphs vec)
    paras.push(p("\u{d14d}\u{c2a4}\u{d2b8} \u{c5c6}\u{b294} \u{d0c0}\u{c6d0}:", CS_BOLD, PS_LEFT));
    paras.push(make_ellipse(
        8000,
        5000,
        "",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(128, 0, 128)),
            fill_color: Some(Color::from_rgb(230, 200, 255)),
            line_width: Some(80),
            ..Default::default()
        }),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("08_ellipse.hwpx", &store, &doc, &images);
}

fn gen_09_polygon() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "09. \u{b2e4}\u{ac01}\u{d615}",
        "\u{b2e4}\u{c591}\u{d55c} \u{b2e4}\u{ac01}\u{d615}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}. \u{ccab} \u{af3d}\u{c9d3}\u{c810} \
         \u{bc18}\u{bcf5} \u{d544}\u{c218}.",
    );

    let make_polygon =
        |verts: Vec<ShapePoint>, w: i32, h: i32, text: &str, style: Option<ShapeStyle>| {
            Paragraph::with_runs(
                vec![Run::control(
                    Control::Polygon {
                        vertices: verts,
                        width: HwpUnit::new(w).unwrap(),
                        height: HwpUnit::new(h).unwrap(),
                        horz_offset: 0,
                        vert_offset: 0,
                        paragraphs: vec![p(text, CS_NORMAL, PS_CENTER)],
                        caption: None,
                        style,
                    },
                    csi(CS_NORMAL),
                )],
                psi(PS_LEFT),
            )
        };

    // Triangle (3 vertices + closure)
    paras.push(p("\u{c0bc}\u{ac01}\u{d615}:", CS_BOLD, PS_LEFT));
    paras.push(make_polygon(
        vec![
            ShapePoint::new(5000, 0),
            ShapePoint::new(10000, 8000),
            ShapePoint::new(0, 8000),
            ShapePoint::new(5000, 0), // closure
        ],
        10000,
        8000,
        "\u{c0bc}\u{ac01}\u{d615}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(200, 0, 0)),
            fill_color: Some(Color::from_rgb(255, 220, 220)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Diamond (4 vertices + closure)
    paras.push(p("\u{b9c8}\u{b984}\u{baa8}:", CS_BOLD, PS_LEFT));
    paras.push(make_polygon(
        vec![
            ShapePoint::new(5000, 0),
            ShapePoint::new(10000, 5000),
            ShapePoint::new(5000, 10000),
            ShapePoint::new(0, 5000),
            ShapePoint::new(5000, 0), // closure
        ],
        10000,
        10000,
        "\u{b9c8}\u{b984}\u{baa8}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 0, 200)),
            fill_color: Some(Color::from_rgb(220, 220, 255)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Pentagon (5 vertices + closure)
    paras.push(p("\u{c624}\u{ac01}\u{d615}:", CS_BOLD, PS_LEFT));
    paras.push(make_polygon(
        vec![
            ShapePoint::new(5000, 0),
            ShapePoint::new(10000, 3800),
            ShapePoint::new(8100, 10000),
            ShapePoint::new(1900, 10000),
            ShapePoint::new(0, 3800),
            ShapePoint::new(5000, 0), // closure
        ],
        10000,
        10000,
        "\u{c624}\u{ac01}\u{d615}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 150, 0)),
            fill_color: Some(Color::from_rgb(220, 255, 220)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Hexagon (6 vertices + closure)
    paras.push(p("\u{c721}\u{ac01}\u{d615}:", CS_BOLD, PS_LEFT));
    paras.push(make_polygon(
        vec![
            ShapePoint::new(2500, 0),
            ShapePoint::new(7500, 0),
            ShapePoint::new(10000, 5000),
            ShapePoint::new(7500, 10000),
            ShapePoint::new(2500, 10000),
            ShapePoint::new(0, 5000),
            ShapePoint::new(2500, 0), // closure
        ],
        10000,
        10000,
        "\u{c721}\u{ac01}\u{d615}",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(255, 140, 0)),
            fill_color: Some(Color::from_rgb(255, 235, 200)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));
    paras.push(empty());

    // Arrow shape (7 vertices + closure)
    paras.push(p("\u{d654}\u{c0b4}\u{d45c} \u{baa8}\u{c591}:", CS_BOLD, PS_LEFT));
    paras.push(make_polygon(
        vec![
            ShapePoint::new(0, 3000),
            ShapePoint::new(7000, 3000),
            ShapePoint::new(7000, 0),
            ShapePoint::new(10000, 5000),
            ShapePoint::new(7000, 10000),
            ShapePoint::new(7000, 7000),
            ShapePoint::new(0, 7000),
            ShapePoint::new(0, 3000), // closure
        ],
        10000,
        10000,
        "",
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(128, 0, 128)),
            fill_color: Some(Color::from_rgb(230, 200, 255)),
            line_width: Some(30),
            ..Default::default()
        }),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("09_polygon.hwpx", &store, &doc, &images);
}

fn gen_10_multi_column() {
    let store = showcase_store();
    let (mut paras1, images) = mascot_intro(
        "10. \u{b2e4}\u{b2e8}",
        "\u{b2e4}\u{b2e8} \u{b808}\u{c774}\u{c544}\u{c6c3}\u{c744} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // Section 1: 2-column layout
    let long1 = "\u{c774}\u{ac83}\u{c740} 2\u{b2e8} \u{b808}\u{c774}\u{c544}\u{c6c3}\u{c758} \
        \u{ccab} \u{bc88}\u{c9f8} \u{c5f4}\u{c785}\u{b2c8}\u{b2e4}. \u{d14d}\u{c2a4}\u{d2b8}\u{ac00} \
        \u{ce49}\u{bd84}\u{d788} \u{ae38}\u{ba74} \u{c790}\u{c5f0}\u{c2a4}\u{b7fd}\u{ac8c} \
        \u{b2e4}\u{c74c} \u{c5f4}\u{b85c} \u{b118}\u{c5b4}\u{ac11}\u{b2c8}\u{b2e4}. \
        HwpForge\u{b294} \u{d55c}\u{ae00} \u{bb38}\u{c11c}\u{b97c} \u{d504}\u{b85c}\u{adf8}\u{b798}\u{b9e4}\u{d2f1}\u{d558}\u{ac8c} \
        \u{c0dd}\u{c131}\u{d560} \u{c218} \u{c788}\u{b294} Rust \u{b77c}\u{c774}\u{be0c}\u{b7ec}\u{b9ac}\u{c785}\u{b2c8}\u{b2e4}.";
    let long2 = "\u{cd94}\u{ac00} \u{bb38}\u{b2e8}\u{c785}\u{b2c8}\u{b2e4}. \u{ccab} \u{bc88}\u{c9f8} \
        \u{c5f4}\u{c744} \u{cc44}\u{c6b0}\u{ae30} \u{c704}\u{d55c} \u{d14d}\u{c2a4}\u{d2b8}\u{c785}\u{b2c8}\u{b2e4}. \
        \u{c790}\u{c5f0}\u{c2a4}\u{b7fd}\u{ac8c} \u{b2e4}\u{c74c} \u{c5f4}\u{b85c} \u{b118}\u{c5b4}\u{ac00}\u{b294}\u{c9c0} \
        \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}. \u{c774} \u{bb38}\u{b2e8}\u{c740} \u{b450} \u{bc88}\u{c9f8} \u{c5f4}\u{c5d0} \
        \u{bc30}\u{ce58}\u{b418}\u{c5b4}\u{c57c} \u{d569}\u{b2c8}\u{b2e4}.";
    paras1.push(p(long1, CS_NORMAL, PS_LEFT));
    paras1.push(p(long2, CS_NORMAL, PS_LEFT));

    let mut sec1 = Section::with_paragraphs(paras1, PageSettings::a4());
    sec1.column_settings =
        Some(ColumnSettings::equal_columns(2, HwpUnit::from_mm(4.0).unwrap()).unwrap());

    // Section 2: 3-column layout with columnBreak
    let mut paras2 = vec![
        p(
            "3\u{b2e8} \u{b808}\u{c774}\u{c544}\u{c6c3}",
            CS_TITLE,
            PS_CENTER,
        ),
        p(
            "\u{ccab} \u{bc88}\u{c9f8} \u{c5f4}\u{c758} \u{b0b4}\u{c6a9}\u{c785}\u{b2c8}\u{b2e4}. \
             \u{c790}\u{c5f0} \u{d750}\u{b984}\u{c73c}\u{b85c} \u{cc44}\u{c6cc}\u{c9c0}\u{b294} \u{d14d}\u{c2a4}\u{d2b8}.",
            CS_NORMAL,
            PS_LEFT,
        ),
    ];
    let mut col2 = p(
        "\u{b450} \u{bc88}\u{c9f8} \u{c5f4} (columnBreak \u{c0ac}\u{c6a9}). \
         \u{ac15}\u{c81c}\u{b85c} \u{b450} \u{bc88}\u{c9f8} \u{c5f4}\u{b85c} \u{c774}\u{b3d9}\u{d569}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_LEFT,
    );
    col2.column_break = true;
    paras2.push(col2);

    let mut col3 = p(
        "\u{c138} \u{bc88}\u{c9f8} \u{c5f4} (columnBreak \u{c0ac}\u{c6a9}). \
         \u{b9c8}\u{c9c0}\u{b9c9} \u{c5f4}\u{c758} \u{b0b4}\u{c6a9}\u{c785}\u{b2c8}\u{b2e4}.",
        CS_NORMAL,
        PS_LEFT,
    );
    col3.column_break = true;
    paras2.push(col3);

    let mut sec2 = Section::with_paragraphs(paras2, PageSettings::a4());
    sec2.column_settings =
        Some(ColumnSettings::equal_columns(3, HwpUnit::from_mm(4.0).unwrap()).unwrap());

    let mut doc = Document::new();
    doc.add_section(sec1);
    doc.add_section(sec2);
    encode_and_save("10_multi_column.hwpx", &store, &doc, &images);
}

fn gen_11_image() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "11. Image",
        "Image API: size variations, aspect ratios, caption positions. \
         All images use treat_as_char=1 (inline).",
    );

    // ── Size variations ──
    paras.push(p("Size Variations:", CS_TITLE, PS_LEFT));
    paras.push(empty());

    // Small (30mm x 30mm)
    paras.push(p("30mm x 30mm:", CS_BOLD, PS_LEFT));
    let img_small = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(30.0).unwrap(),
        HwpUnit::from_mm(30.0).unwrap(),
        ImageFormat::Png,
    );
    paras.push(Paragraph::with_runs(vec![Run::image(img_small, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Medium (60mm x 60mm)
    paras.push(p("60mm x 60mm:", CS_BOLD, PS_LEFT));
    let img_medium = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(60.0).unwrap(),
        HwpUnit::from_mm(60.0).unwrap(),
        ImageFormat::Png,
    );
    paras.push(Paragraph::with_runs(vec![Run::image(img_medium, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Wide aspect ratio (120mm x 50mm)
    paras.push(p("120mm x 50mm (wide):", CS_BOLD, PS_LEFT));
    let img_wide = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(120.0).unwrap(),
        HwpUnit::from_mm(50.0).unwrap(),
        ImageFormat::Png,
    );
    paras.push(Paragraph::with_runs(vec![Run::image(img_wide, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Tall aspect ratio (40mm x 80mm)
    paras.push(p("40mm x 80mm (tall):", CS_BOLD, PS_LEFT));
    let img_tall = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(40.0).unwrap(),
        HwpUnit::from_mm(80.0).unwrap(),
        ImageFormat::Png,
    );
    paras.push(Paragraph::with_runs(vec![Run::image(img_tall, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // ── Caption positions ──
    paras.push(p("Caption Positions:", CS_TITLE, PS_LEFT));
    paras.push(empty());

    // Caption bottom (default)
    paras.push(p("Caption Bottom:", CS_BOLD, PS_LEFT));
    let mut img_cap_bottom = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(50.0).unwrap(),
        HwpUnit::from_mm(50.0).unwrap(),
        ImageFormat::Png,
    );
    img_cap_bottom.caption = Some(Caption::new(
        vec![p("Fig 1. SoeBuri mascot (bottom caption)", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::image(img_cap_bottom, csi(CS_NORMAL))],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // Caption top
    paras.push(p("Caption Top:", CS_BOLD, PS_LEFT));
    let mut img_cap_top = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(50.0).unwrap(),
        HwpUnit::from_mm(50.0).unwrap(),
        ImageFormat::Png,
    );
    img_cap_top.caption = Some(Caption::new(
        vec![p("Fig 2. Top caption example", CS_SMALL, PS_CENTER)],
        CaptionSide::Top,
    ));
    paras.push(Paragraph::with_runs(vec![Run::image(img_cap_top, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Caption left
    paras.push(p("Caption Left:", CS_BOLD, PS_LEFT));
    let mut img_cap_left = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(50.0).unwrap(),
        HwpUnit::from_mm(50.0).unwrap(),
        ImageFormat::Png,
    );
    img_cap_left.caption =
        Some(Caption::new(vec![p("Fig 3. Left caption", CS_SMALL, PS_CENTER)], CaptionSide::Left));
    paras
        .push(Paragraph::with_runs(vec![Run::image(img_cap_left, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Caption right
    paras.push(p("Caption Right:", CS_BOLD, PS_LEFT));
    let mut img_cap_right = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(50.0).unwrap(),
        HwpUnit::from_mm(50.0).unwrap(),
        ImageFormat::Png,
    );
    img_cap_right.caption = Some(Caption::new(
        vec![p("Fig 4. Right caption", CS_SMALL, PS_CENTER)],
        CaptionSide::Right,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::image(img_cap_right, csi(CS_NORMAL))],
        psi(PS_CENTER),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("11_image.hwpx", &store, &doc, &images);
}

fn gen_12_hyperlink() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "12. \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c}",
        "\u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c} \u{ae30}\u{b2a5}\u{c744} \
         \u{b2e4}\u{c591}\u{d55c} \u{c2a4}\u{d0c0}\u{c77c}\u{acfc} \
         \u{c0ac}\u{c6a9} \u{c2dc}\u{b098}\u{b9ac}\u{c624}\u{b85c} \
         \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    // ── 1. Standard hyperlink (blue + underline) ─────────────────────
    paras.push(p(
        "1. \u{d45c}\u{c900} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c} (\u{d30c}\u{b780}\u{c0c9} + \u{bc11}\u{c904})",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Hyperlink {
                text: "HwpForge GitHub".to_string(),
                url: "https://github.com/ai-screams/HwpForge".to_string(),
            },
            csi(CS_LINK),
        )],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 2. Hyperlink without styling (plain black text) ──────────────
    paras.push(p(
        "2. \u{c2a4}\u{d0c0}\u{c77c} \u{c5c6}\u{b294} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c} (\u{ae30}\u{bcf8} \u{ac80}\u{c815}\u{c0c9})",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c2a4}\u{d0c0}\u{c77c} \u{c5c6}\u{c774} \u{b9c1}\u{d06c}\u{b9cc} \u{c801}\u{c6a9}: ",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::Hyperlink {
                    text: "github.com/ai-screams/HwpForge".to_string(),
                    url: "https://github.com/ai-screams/HwpForge".to_string(),
                },
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 3. Multiple hyperlinks in one paragraph ──────────────────────
    paras.push(p(
        "3. \u{d55c} \u{bb38}\u{b2e8}\u{c5d0} \u{c5ec}\u{b7ec} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c}",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("\u{cc38}\u{ace0}: ", csi(CS_NORMAL)),
            Run::control(
                Control::Hyperlink {
                    text: "Rust \u{acf5}\u{c2dd}".to_string(),
                    url: "https://www.rust-lang.org".to_string(),
                },
                csi(CS_LINK),
            ),
            Run::text(" | ", csi(CS_NORMAL)),
            Run::control(
                Control::Hyperlink {
                    text: "crates.io".to_string(),
                    url: "https://crates.io/crates/hwpforge".to_string(),
                },
                csi(CS_LINK),
            ),
            Run::text(" | ", csi(CS_NORMAL)),
            Run::control(
                Control::Hyperlink {
                    text: "HwpForge Docs".to_string(),
                    url: "https://ai-screams.github.io/HwpForge/".to_string(),
                },
                csi(CS_LINK),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 4. Inline hyperlink within sentence ──────────────────────────
    paras.push(p(
        "4. \u{bb38}\u{c7a5} \u{c911}\u{ac04}\u{c5d0} \u{c0bd}\u{c785}\u{b41c} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c}",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("\u{c790}\u{c138}\u{d55c} \u{b0b4}\u{c6a9}\u{c740} ", csi(CS_NORMAL)),
            Run::control(
                Control::Hyperlink {
                    text: "HwpForge \u{bb38}\u{c11c} \u{d398}\u{c774}\u{c9c0}".to_string(),
                    url: "https://github.com/ai-screams/HwpForge/wiki".to_string(),
                },
                csi(CS_LINK),
            ),
            Run::text("\u{b97c} \u{cc38}\u{ace0}\u{d558}\u{c138}\u{c694}.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 5. Email mailto link ─────────────────────────────────────────
    paras.push(p(
        "5. \u{c774}\u{ba54}\u{c77c} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c} (mailto:)",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("\u{bb38}\u{c758}\u{c0ac}\u{d56d}: ", csi(CS_NORMAL)),
            Run::control(
                Control::Hyperlink {
                    text: "pignuante@gmail.com".to_string(),
                    url: "mailto:pignuante@gmail.com".to_string(),
                },
                csi(CS_LINK),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 6. Long URL with short display text ──────────────────────────
    paras.push(p(
        "6. \u{ae34} URL\u{c744} \u{c9e7}\u{c740} \u{d14d}\u{c2a4}\u{d2b8}\u{b85c} \u{d45c}\u{c2dc}",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "HWPX \u{d3ec}\u{b9f7} \u{c2a4}\u{d399}\u{c740} ",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::Hyperlink {
                    text: "KS X 6101 \u{d45c}\u{c900} \u{bb38}\u{c11c}".to_string(),
                    url: "https://www.ksa.or.kr/ksa_kr/950/subview.do".to_string(),
                },
                csi(CS_LINK),
            ),
            Run::text(
                "\u{c5d0}\u{c11c} \u{d655}\u{c778}\u{d560} \u{c218} \u{c788}\u{c2b5}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 7. Red-styled hyperlink (custom color) ───────────────────────
    paras.push(p(
        "7. \u{c0ac}\u{c6a9}\u{c790} \u{c815}\u{c758} \u{c0c9}\u{c0c1} (\u{be68}\u{ac04}\u{c0c9})",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c8fc}\u{c758}: ",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::Hyperlink {
                    text: "\u{c774} \u{b9c1}\u{d06c}\u{b294} \u{be68}\u{ac04}\u{c0c9}\u{c73c}\u{b85c} \u{d45c}\u{c2dc}\u{b429}\u{b2c8}\u{b2e4}"
                        .to_string(),
                    url: "https://example.com/warning".to_string(),
                },
                csi(CS_RED),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // ── 8. Bold hyperlink ────────────────────────────────────────────
    paras.push(p("8. \u{ad75}\u{c740} \u{d558}\u{c774}\u{d37c}\u{b9c1}\u{d06c}", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Hyperlink {
                text: "HwpForge \u{d504}\u{b85c}\u{c81d}\u{d2b8} \u{d648}\u{d398}\u{c774}\u{c9c0}"
                    .to_string(),
                url: "https://github.com/ai-screams/HwpForge".to_string(),
            },
            csi(CS_BOLD),
        )],
        psi(PS_LEFT),
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("12_hyperlink.hwpx", &store, &doc, &images);
}

fn gen_13_equation() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "13. \u{c218}\u{c2dd}",
        "HancomEQN \u{c2a4}\u{d06c}\u{b9bd}\u{d2b8} \u{d615}\u{c2dd}\u{c758} \
         \u{b2e4}\u{c591}\u{d55c} \u{c218}\u{c2dd}\u{c744} \u{d655}\u{c778}\u{d569}\u{b2c8}\u{b2e4}. \
         \u{be14}\u{b85d} \u{c218}\u{c2dd}\u{acfc} \u{c778}\u{b77c}\u{c778} \u{c218}\u{c2dd}\u{c744} \
         \u{baa8}\u{b450} \u{d3ec}\u{d568}\u{d569}\u{b2c8}\u{b2e4}.",
    );

    let eq_w = HwpUnit::from_mm(50.0).unwrap();
    let eq_h = HwpUnit::from_mm(15.0).unwrap();
    let black = Color::from_rgb(0, 0, 0);

    let make_eq = |label: &str, script: &str| -> Vec<Paragraph> {
        vec![
            p(label, CS_BOLD, PS_LEFT),
            Paragraph::with_runs(
                vec![Run::control(
                    Control::Equation {
                        script: script.to_string(),
                        width: eq_w,
                        height: eq_h,
                        base_line: 850,
                        text_color: black,
                        font: "HancomEQN".to_string(),
                    },
                    csi(CS_NORMAL),
                )],
                psi(PS_CENTER),
            ),
            empty(),
        ]
    };

    // ── Block equations ──────────────────────────────────────────────

    paras.push(p("[\u{be14}\u{b85d} \u{c218}\u{c2dd}]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    // 1. Fraction
    paras.extend(make_eq("1. \u{bd84}\u{c218} (Fraction):", "{a+b} over {c+d}"));

    // 2. Square root
    paras.extend(make_eq("2. \u{c81c}\u{acf1}\u{adfc} (Square root):", "root {2} of {x^2 + y^2}"));

    // 3. Quadratic formula
    paras.extend(make_eq(
        "3. \u{adfc}\u{c758} \u{acf5}\u{c2dd} (Quadratic formula):",
        "x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}",
    ));

    // 4. Euler's identity
    paras.extend(make_eq(
        "4. \u{c624}\u{c77c}\u{b7ec} \u{d56d}\u{b4f1}\u{c2dd} (Euler's identity):",
        "e^{i pi} + 1 = 0",
    ));

    // 5. Integral (Gaussian)
    paras.extend(make_eq(
        "5. \u{c801}\u{bd84} (Gaussian integral):",
        "int _{0} ^{INF } {e ^{(-x ^{2} )}} dx = { root {2} of { pi }} over {2}",
    ));

    // 6. Summation (Basel problem)
    paras.extend(make_eq(
        "6. \u{ae09}\u{c218} (Basel problem):",
        "sum _{n=1} ^{INF } {1} over {n ^{2}} = { pi  ^{2}} over {6}",
    ));

    // 7. Limit
    paras.extend(make_eq("7. \u{adf9}\u{d55c} (Limit):", "lim _{x rarrow  0} {sin} (x)/x`=1"));

    // 8. Derivative definition
    paras.extend(make_eq(
        "8. \u{b3c4}\u{d568}\u{c218} \u{c815}\u{c758} (Derivative):",
        "f'(x)= lim _{h rarrow  0} {f(x+h)-f(x)} over {h}",
    ));

    // 9. Matrix (2x2)
    paras.extend(make_eq("9. \u{d589}\u{b82c} (2x2 Matrix):", "{matrix{a&b#c&d}}"));

    // 10. 3x3 Identity matrix
    paras.extend(make_eq(
        "10. \u{b2e8}\u{c704}\u{d589}\u{b82c} (3x3 Identity):",
        "I= {matrix{1&0&0#0&1&0#0&0&1}}",
    ));

    // 11. Trigonometry (law of sines)
    paras.extend(make_eq(
        "11. \u{c0ac}\u{c778} \u{bc95}\u{ce59} (Law of sines):",
        "{a} over {sin`A} = {b} over {sin`B} = {c} over {sin`C} =`2R",
    ));

    // 12. Newton's gravitation
    paras.extend(make_eq(
        "12. \u{b274}\u{d134} \u{b9cc}\u{c720}\u{c778}\u{b825} (Gravitation):",
        "F`=`G {m _{1} m _{2}} over {r ^{2}}",
    ));

    // 13. Binomial theorem
    paras.extend(make_eq(
        "13. \u{c774}\u{d56d}\u{c815}\u{b9ac} (Binomial theorem):",
        "(a+b) ^{n} = sum _{k=0} ^{n} {matrix{n#k}}`a ^{n-k} b ^{k}",
    ));

    // 14. Stirling's approximation
    paras.extend(make_eq(
        "14. \u{c2a4}\u{d138}\u{b9c1} \u{adfc}\u{c0ac} (Stirling):",
        "n!` APPROX  root {2} of {2 pi  n}` ({ {n} over {e} }) ^{n}",
    ));

    // ── Inline equations ─────────────────────────────────────────────

    paras.push(p("[\u{c778}\u{b77c}\u{c778} \u{c218}\u{c2dd}]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    let inline_w = HwpUnit::from_mm(25.0).unwrap();
    let inline_h = HwpUnit::from_mm(8.0).unwrap();

    let inline_eq = |script: &str| -> Run {
        Run::control(
            Control::Equation {
                script: script.to_string(),
                width: inline_w,
                height: inline_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )
    };

    // Inline 1: E=mc²
    paras.push(p(
        "15. \u{c778}\u{b77c}\u{c778} \u{c218}\u{c2dd} \u{c608}\u{c2dc}:",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c544}\u{c778}\u{c288}\u{d0c0}\u{c778}\u{c758} \u{c720}\u{ba85}\u{d55c} \u{acf5}\u{c2dd} ",
                csi(CS_NORMAL),
            ),
            inline_eq("E`=`mc ^{2}"),
            Run::text(
                " \u{c740} \u{c9c8}\u{b7c9}\u{acfc} \u{c5d0}\u{b108}\u{c9c0}\u{c758} \u{b4f1}\u{ac00}\u{c131}\u{c744} \u{b098}\u{d0c0}\u{b0c5}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Inline 2: quadratic equation
    paras.push(Paragraph::with_runs(
        vec![
            Run::text(
                "\u{c774}\u{cc28}\u{bc29}\u{c815}\u{c2dd} ",
                csi(CS_NORMAL),
            ),
            inline_eq("ax ^{2} +bx+c=0"),
            Run::text(
                " \u{c758} \u{d574}\u{b294} \u{adfc}\u{c758} \u{acf5}\u{c2dd}\u{c73c}\u{b85c} \u{ad6c}\u{d569}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Inline 3: pi approximation
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("\u{c6d0}\u{c8fc}\u{c728} ", csi(CS_NORMAL)),
            inline_eq("pi  APPROX 3.14159"),
            Run::text(
                " \u{b294} \u{bb34}\u{b9ac}\u{c218}\u{c785}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Inline 4: fraction in text
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("\u{d655}\u{b960} ", csi(CS_NORMAL)),
            inline_eq("P(A|B)= {P(B|A) CDOT P(A)} over {P(B)}"),
            Run::text(
                " \u{b294} \u{bca0}\u{c774}\u{c988} \u{c815}\u{b9ac}\u{c785}\u{b2c8}\u{b2e4}.",
                csi(CS_NORMAL),
            ),
        ],
        psi(PS_LEFT),
    ));

    // ── Page break → 동형암호 학술 소개 ──────────────────────────────
    paras.push(empty().with_page_break());

    // Title
    paras.push(p("동형암호(Homomorphic Encryption)의 수학적 기초", CS_TITLE, PS_CENTER));
    paras.push(empty());

    // ── 1. 서론 ──────────────────────────────────────────────────────
    paras.push(p("1. 서론", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "동형암호(Homomorphic Encryption, HE)는 암호화된 데이터에 대해 \
         복호화 없이 직접 연산을 수행할 수 있는 암호 체계이다. \
         기존의 공개키 암호 체계에서는 암호화된 데이터를 연산하기 위해 \
         반드시 복호화를 거쳐야 하므로, 클라우드 환경이나 제3자 서버에 \
         데이터를 위탁하는 경우 민감 정보의 노출 위험이 존재한다. \
         반면, 동형암호를 사용하면 암호문 상에서의 연산 결과가 \
         평문 연산 결과의 암호문과 동일하므로, 데이터 소유자의 비밀키 \
         없이도 서버가 연산을 대행할 수 있다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Definition: homomorphic property (addition)
    paras.push(p("정의 1. 동형성 (덧셈):", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "Dec(Enc(m _{1}) oplus Enc(m _{2})) = m _{1} + m _{2}".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // Definition: homomorphic property (multiplication)
    paras.push(p("정의 2. 동형성 (곱셈):", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "Dec(Enc(m _{1}) otimes Enc(m _{2})) = m _{1} cdot m _{2}".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    paras.push(p(
        "완전동형암호(Fully Homomorphic Encryption, FHE)의 수학적 가능성은 \
         2009년 Gentry에 의해 처음 증명되었으며, 이는 격자(lattice) 이론의 \
         난제인 오류 학습(Learning With Errors, LWE) 문제의 계산적 어려움에 \
         기반한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // ── 2. 수학적 기반: LWE 및 RLWE ─────────────────────────────────
    paras.push(p("2. 수학적 기반: LWE 및 RLWE", CS_BOLD, PS_LEFT));
    paras.push(empty());

    // 2.1 Polynomial ring
    paras.push(p("2.1. 다항식 환(Polynomial Ring)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "현대의 동형암호 체계는 대부분 Ring-LWE 문제에 기반하며, \
         다음의 다항식 환 위에서 동작한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "R _{q} = Z _{q} [X] / (X ^{n} + 1) ,`` n = 2 ^{k} ,` k in N".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // 2.2 LWE
    paras.push(p("2.2. LWE (Learning With Errors) 문제", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Regev(2005)가 제안한 LWE 문제는 다음과 같이 정의된다. \
         비밀 벡터 s와 소규모 오차 e에 대해, 표본 (a, b)가 주어질 때 \
         이를 균일 난수 쌍과 구별하는 문제이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "b = langle a , s rangle + e ,`` a in Z _{q} ^{n} ,` s in Z _{q} ^{n} ,` e ~ chi"
                    .to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // 2.3 RLWE
    paras.push(p("2.3. Ring-LWE (RLWE) 문제", CS_BOLD, PS_LEFT));
    paras.push(p(
        "RLWE는 LWE의 환(ring) 변형으로, 다항식 환 R_q 위에서 정의된다. \
         이상적 격자(ideal lattice) 문제의 최악 사례 어려움으로 환원되므로 \
         양자컴퓨터 공격에도 안전한 후양자(post-quantum) 암호로 분류된다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script:
                    "b = a cdot s + e ,`` a in _{R} R _{q} ,` e ~ chi _{sigma} ,` sigma approx 3.2"
                        .to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // ── 3. BFV 체계 ─────────────────────────────────────────────────
    paras.push(p("3. BFV 체계 (Brakerski/Fan/Vercauteren)", CS_BOLD, PS_LEFT));
    paras.push(empty());

    // 3.1 Key Generation
    paras.push(p("3.1. 키 생성 (Key Generation)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "공개키는 RLWE 표본으로 구성되며, 비밀키 s는 소규모 다항식이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "pk = ( -(a cdot s + e) , a ) ,`` a in _{R} R _{q} ,` e ~ chi".to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // 3.2 Encryption
    paras.push(p("3.2. 암호화 (Encryption)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "메시지 m을 평문 공간 R_t에서 암호화한다. \
         스케일 팩터 Δ = ⌊q/t⌋가 메시지를 암호문 공간으로 확장한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "c _{0} = pk _{0} cdot u + e _{1} + lfloor {q} over {t} rfloor cdot m"
                    .to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "c _{1} = pk _{1} cdot u + e _{2}".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // 3.3 Decryption
    paras.push(p("3.3. 복호화 (Decryption)", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "m = left [ lfloor {t} over {q} cdot [c _{0} + c _{1} cdot s] _{q} rfloor right ] _{t}"
                    .to_string(),
                width: HwpUnit::from_mm(60.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // 3.4 Homomorphic operations
    paras.push(p("3.4. 동형 연산", CS_BOLD, PS_LEFT));
    paras.push(p("동형 덧셈:", CS_NORMAL, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "c ^{add} = (c _{0} + c _{0} prime , c _{1} + c _{1} prime ) mod q"
                    .to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    paras.push(p("동형 곱셈 (재선형화 전):", CS_NORMAL, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "c _{2} ^{*} = lfloor {t} over {q} cdot c _{1} c _{1} prime rfloor"
                    .to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // ── 4. CKKS 체계 ────────────────────────────────────────────────
    paras.push(p("4. CKKS 체계 (Cheon/Kim/Kim/Song)", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "CKKS(2017)는 실수 및 복소수에 대한 근사 동형암호 체계이다. \
         스케일 팩터 Δ가 정밀도를 제어하며, 반올림 오차를 \
         암호문 잡음의 일부로 취급하는 것이 핵심이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // CKKS encryption
    paras.push(p("4.1. 암호화 및 복호화", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script:
                    "c = (c _{0} , c _{1} ) = [u cdot pk + ( Delta cdot m + e _{1} , e _{2} )] _{q}"
                        .to_string(),
                width: HwpUnit::from_mm(70.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // CKKS decryption (inline style)
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("복호화: ", csi(CS_NORMAL)),
            inline_eq("m approx {1} over {Delta} (c _{0} + c _{1} cdot s)"),
            Run::text(" 로 근사값을 복원한다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // CKKS rescaling
    paras.push(p("4.2. 재스케일링 (Rescaling)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "곱셈 후 스케일이 Δ²으로 증가하므로, 재스케일링으로 Δ로 복원한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script:
                    "RS(c) = lfloor {q prime} over {q} cdot c rfloor ,`` q prime = {q} over {Delta}"
                        .to_string(),
                width: HwpUnit::from_mm(60.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // ── 5. 잡음 성장 분석 ────────────────────────────────────────────
    paras.push(p("5. 잡음 성장 분석 (Noise Growth)", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "동형 연산 과정에서의 잡음 누적은 동형암호의 핵심 도전 과제이다. \
         동형 덧셈은 잡음이 선형적으로, 곱셈은 지수적으로 증가한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Noise after addition (inline)
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("덧셈 후 잡음: ", csi(CS_NORMAL)),
            inline_eq("|| v _{add} || leq || v _{1} || + || v _{2} ||"),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    // Multiplicative depth
    paras.push(p("지원 가능한 곱셈 깊이(multiplicative depth):", CS_NORMAL, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "L approx { log _{2} q } over { log _{2} (n cdot B _{err}) }".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // Noise variance (BFV)
    paras.push(p("BFV 초기 암호문 잡음 분산:", CS_NORMAL, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "V _{v} = t ^{2} left ( {1} over {12} + sigma ^{2} left ( {4n} over {3} + 1 right ) right )"
                    .to_string(),
                width: HwpUnit::from_mm(60.0).unwrap(),
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // ── 6. 부트스트래핑 ─────────────────────────────────────────────
    paras.push(p("6. 부트스트래핑 (Bootstrapping)", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "Gentry(2009)가 제안한 부트스트래핑은 잡음이 누적된 암호문을 \
         동형적으로 복호화 회로를 평가하여 잡음을 갱신(refresh)하는 기법이다. \
         이를 통해 무한한 깊이의 동형 연산이 이론적으로 가능해진다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Equation {
                script: "c ^{*} = Dec _{Enc(sk)} (c) approx Enc _{pk _{2}} (m)".to_string(),
                width: eq_w,
                height: eq_h,
                base_line: 850,
                text_color: black,
                font: "HancomEQN".to_string(),
            },
            csi(CS_NORMAL),
        )],
        psi(PS_CENTER),
    ));
    paras.push(empty());

    // ── 참고문헌 ─────────────────────────────────────────────────────
    paras.push(p("참고문헌", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "[1] C. Gentry, \"Fully Homomorphic Encryption Using Ideal Lattices,\" STOC 2009.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(p(
        "[2] Z. Brakerski, C. Gentry, V. Vaikuntanathan, \"(Leveled) Fully Homomorphic \
         Encryption without Bootstrapping,\" ITCS 2012.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(p(
        "[3] J. Fan, F. Vercauteren, \"Somewhat Practical Fully Homomorphic Encryption,\" \
         IACR ePrint 2012/144.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(p(
        "[4] J.H. Cheon, A. Kim, M. Kim, Y. Song, \"Homomorphic Encryption for Arithmetic \
         of Approximate Numbers,\" ASIACRYPT 2017.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(p(
        "[5] O. Regev, \"On Lattices, Learning with Errors, Random Linear Codes, \
         and Cryptography,\" STOC 2005.",
        CS_SMALL,
        PS_LEFT,
    ));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("13_equation.hwpx", &store, &doc, &images);
}

fn gen_14_chart() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "14. 차트",
        "OOXML 차트 18종을 다양한 데이터와 옵션으로 시연합니다. \
         각 페이지에 1~2개 차트를 배치하여 레이아웃을 정리했습니다.",
    );

    let large_w = HwpUnit::from_mm(140.0).unwrap();
    let large_h = HwpUnit::from_mm(90.0).unwrap();
    let med_w = HwpUnit::from_mm(120.0).unwrap();
    let med_h = HwpUnit::from_mm(65.0).unwrap();

    // Helper: chart with common defaults (legend, grouping only vary)
    #[allow(clippy::too_many_arguments)]
    fn make_chart(
        ct: ChartType,
        data: ChartData,
        w: HwpUnit,
        h: HwpUnit,
        title: &str,
        legend: LegendPosition,
    ) -> Control {
        Control::Chart {
            chart_type: ct,
            data,
            width: w,
            height: h,
            title: Some(title.to_string()),
            legend,
            grouping: Default::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        }
    }

    let chart_para = |ctrl: Control| -> Paragraph {
        Paragraph::with_runs(vec![Run::control(ctrl, csi(CS_NORMAL))], psi(PS_CENTER))
    };

    // ── Page 1: Column (Clustered vs Stacked) ────────────────────────
    paras.push(p("[세로 막대 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("1. Column — Clustered (기본):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Column,
        ChartData::category(
            &["Q1", "Q2", "Q3", "Q4"],
            &[
                ("매출", &[120.0, 180.0, 210.0, 250.0]),
                ("비용", &[90.0, 110.0, 130.0, 160.0]),
                ("이익", &[30.0, 70.0, 80.0, 90.0]),
            ],
        ),
        med_w,
        med_h,
        "분기별 매출/비용/이익 (억원)",
        LegendPosition::Bottom,
    )));
    paras.push(empty());

    paras.push(p("2. Column — Stacked:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Column,
        data: ChartData::category(
            &["2022", "2023", "2024", "2025"],
            &[
                ("국내", &[450.0, 520.0, 580.0, 640.0]),
                ("아시아", &[180.0, 230.0, 310.0, 380.0]),
                ("유럽", &[90.0, 120.0, 160.0, 200.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("연도별 지역 매출 구성".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::Stacked,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 2: Column3D (Cylinder) + PercentStacked ─────────────────
    paras.push(p("[3D 세로 막대 + 100% 누적]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("3. Column3D — Cylinder:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Column3D,
        data: ChartData::category(
            &["서울", "경기", "부산", "대전", "광주"],
            &[
                ("주거용", &[85.0, 72.0, 45.0, 28.0, 22.0]),
                ("상업용", &[42.0, 38.0, 25.0, 15.0, 12.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("지역별 건축 허가 (백건)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: Some(BarShape::Cylinder),
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p("4. Column — PercentStacked:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Column,
        data: ChartData::category(
            &["10대", "20대", "30대", "40대", "50대+"],
            &[
                ("모바일", &[95.0, 88.0, 75.0, 60.0, 40.0]),
                ("PC", &[3.0, 8.0, 20.0, 32.0, 45.0]),
                ("태블릿", &[2.0, 4.0, 5.0, 8.0, 15.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("연령대별 기기 사용 비율 (%)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::PercentStacked,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 3: Bar + Bar3D ──────────────────────────────────────────
    paras.push(p("[가로 막대 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("5. Bar — 프로그래밍 언어 인기도:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Bar,
        ChartData::category(
            &["Python", "JavaScript", "Java", "C++", "Rust", "Go"],
            &[("점유율(%)", &[28.0, 22.0, 16.0, 12.0, 8.0, 6.0])],
        ),
        med_w,
        med_h,
        "2025 프로그래밍 언어 인기도",
        LegendPosition::None,
    )));
    paras.push(empty());

    paras.push(p("6. Bar3D — Pyramid:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Bar3D,
        data: ChartData::category(
            &["전자", "자동차", "반도체", "조선", "바이오"],
            &[
                ("수출(조원)", &[180.0, 95.0, 130.0, 42.0, 28.0]),
                ("수입(조원)", &[60.0, 35.0, 80.0, 15.0, 22.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("산업별 수출입 (2025)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: Some(BarShape::Pyramid),
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 4: Line + Line3D ────────────────────────────────────────
    paras.push(p("[선형 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("7. Line — 월별 기온 변화 (마커):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Line,
        data: ChartData::category(
            &["1월", "3월", "5월", "7월", "9월", "11월"],
            &[
                ("서울", &[-2.4, 5.7, 18.6, 25.7, 21.2, 5.2]),
                ("부산", &[3.1, 8.9, 18.1, 25.0, 22.5, 9.8]),
                ("제주", &[5.8, 10.2, 18.5, 26.8, 23.1, 11.5]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("월별 평균 기온 (°C)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: Some(true),
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p("8. Line3D:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Line3D,
        ChartData::category(
            &["2021", "2022", "2023", "2024"],
            &[
                ("회원수(만)", &[120.0, 185.0, 260.0, 340.0]),
                ("MAU(만)", &[45.0, 92.0, 150.0, 220.0]),
            ],
        ),
        med_w,
        med_h,
        "서비스 성장 추이",
        LegendPosition::Bottom,
    )));
    paras.push(empty().with_page_break());

    // ── Page 5: Area (Stacked) + Area3D ──────────────────────────────
    paras.push(p("[영역 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("9. Area — Stacked 트래픽:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Area,
        data: ChartData::category(
            &["00시", "04시", "08시", "12시", "16시", "20시"],
            &[
                ("웹", &[120.0, 40.0, 380.0, 520.0, 490.0, 350.0]),
                ("앱", &[80.0, 25.0, 250.0, 410.0, 380.0, 290.0]),
                ("API", &[200.0, 180.0, 450.0, 600.0, 550.0, 400.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("시간대별 서버 트래픽 (req/s)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::Stacked,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p("10. Area3D — 에너지원별 발전량:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Area3D,
        ChartData::category(
            &["2020", "2022", "2024", "2026E"],
            &[
                ("원자력", &[160.0, 175.0, 190.0, 200.0]),
                ("태양광", &[20.0, 45.0, 80.0, 120.0]),
                ("풍력", &[10.0, 25.0, 50.0, 85.0]),
            ],
        ),
        med_w,
        med_h,
        "에너지원별 발전량 (TWh)",
        LegendPosition::Bottom,
    )));
    paras.push(empty().with_page_break());

    // ── Page 6: Pie + Pie3D ──────────────────────────────────────────
    paras.push(p("[원형 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("11. Pie — 부서별 예산:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Pie,
        ChartData::category(
            &["영업", "개발", "마케팅", "인사", "총무"],
            &[("예산(억)", &[35.0, 42.0, 18.0, 12.0, 8.0])],
        ),
        med_w,
        med_h,
        "부서별 예산 배분",
        LegendPosition::Right,
    )));
    paras.push(empty());

    paras.push(p("12. Pie3D — 시장 점유율:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Pie3D,
        ChartData::category(
            &["삼성", "애플", "샤오미", "오포", "기타"],
            &[("점유율(%)", &[20.0, 27.0, 14.0, 9.0, 30.0])],
        ),
        med_w,
        med_h,
        "글로벌 스마트폰 시장 점유율 (2025)",
        LegendPosition::Right,
    )));
    paras.push(empty().with_page_break());

    // ── Page 7: Doughnut + OfPie ─────────────────────────────────────
    paras.push(p("[도넛 / 원형 분리 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("13. Doughnut — OS 점유율:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Doughnut,
        ChartData::category(
            &["Windows", "macOS", "Linux", "ChromeOS"],
            &[("점유율(%)", &[72.0, 16.0, 8.0, 4.0])],
        ),
        med_w,
        med_h,
        "데스크톱 OS 점유율",
        LegendPosition::Right,
    )));
    paras.push(empty());

    paras.push(p("14. OfPie (Pie-of-Pie) — 기타 항목 분리:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::OfPie,
        data: ChartData::category(
            &["급여", "임대료", "마케팅", "서버비", "출장비", "교육비", "복리후생"],
            &[("비용(만원)", &[4500.0, 1200.0, 800.0, 600.0, 300.0, 200.0, 150.0])],
        ),
        width: med_w,
        height: med_h,
        title: Some("운영 비용 구성 (기타 분리)".to_string()),
        legend: LegendPosition::Right,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: Some(OfPieType::Pie),
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 8: Scatter (2종) ────────────────────────────────────────
    paras.push(p("[산점도]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("15. Scatter — Dots (점만):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Scatter,
        data: ChartData::xy(&[
            ("실험 A", &[1.0, 2.5, 3.2, 4.8, 6.0, 7.5], &[2.1, 5.2, 6.8, 9.5, 12.3, 15.0]),
            ("실험 B", &[1.0, 2.0, 3.5, 5.0, 6.5, 8.0], &[1.5, 3.8, 7.2, 10.1, 13.0, 16.8]),
        ]),
        width: med_w,
        height: med_h,
        title: Some("실험 데이터 비교".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: Some(ScatterStyle::Dots),
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p("16. Scatter — SmoothMarker (곡선+마커):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Scatter,
        data: ChartData::xy(&[(
            "sin(x)",
            &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, std::f64::consts::TAU],
            &[0.0, 0.48, 0.84, 1.0, 0.91, 0.60, 0.14, -0.35, -0.76, -0.98, -0.96, -0.71, 0.0],
        )]),
        width: med_w,
        height: med_h,
        title: Some("사인 함수 곡선".to_string()),
        legend: LegendPosition::None,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: Some(ScatterStyle::SmoothMarker),
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 9: Bubble + Radar (Filled) ──────────────────────────────
    paras.push(p("[버블 / 레이더 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("17. Bubble — 도시별 비교:", CS_BOLD, PS_LEFT));
    paras.push(chart_para(make_chart(
        ChartType::Bubble,
        ChartData::xy(&[
            ("서울", &[970.0], &[4200.0]),
            ("부산", &[340.0], &[3100.0]),
            ("인천", &[295.0], &[3300.0]),
            ("대구", &[240.0], &[2900.0]),
            ("대전", &[150.0], &[3000.0]),
        ]),
        med_w,
        med_h,
        "주요 도시 비교 (X=인구, Y=소득)",
        LegendPosition::Bottom,
    )));
    paras.push(empty());

    paras.push(p("18. Radar — Filled (역량 평가):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Radar,
        data: ChartData::category(
            &["기술력", "커뮤니케이션", "리더십", "문제해결", "협업", "창의성"],
            &[
                ("김철수", &[9.0, 7.0, 8.0, 9.0, 6.0, 8.0]),
                ("이영희", &[7.0, 9.0, 7.0, 6.0, 9.0, 7.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("팀원 역량 비교".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: Some(RadarStyle::Filled),
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── Page 10: Radar (Marker) + Stock (OHLC) ───────────────────────
    paras.push(p("[레이더(마커) / 주식 차트]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("19. Radar — Marker (제품 비교):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Radar,
        data: ChartData::category(
            &["디자인", "성능", "배터리", "카메라", "가격"],
            &[
                ("제품 A", &[8.0, 9.0, 7.0, 8.0, 6.0]),
                ("제품 B", &[7.0, 7.0, 9.0, 6.0, 9.0]),
                ("제품 C", &[9.0, 6.0, 6.0, 9.0, 7.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("스마트폰 비교 평가".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: Some(RadarStyle::Marker),
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p("20. Stock — OHLC (시가-고가-저가-종가):", CS_BOLD, PS_LEFT));
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Stock,
        data: ChartData::category(
            &["3/3", "3/4", "3/5", "3/6", "3/7"],
            &[
                ("시가", &[52000.0, 52500.0, 53000.0, 51500.0, 52800.0]),
                ("고가", &[53500.0, 54000.0, 53800.0, 53000.0, 54200.0]),
                ("저가", &[51000.0, 51800.0, 51500.0, 50500.0, 52000.0]),
                ("종가", &[52500.0, 53000.0, 51500.0, 52800.0, 53500.0]),
            ],
        ),
        width: large_w,
        height: large_h,
        title: Some("HWP전자 주가 (OHLC)".to_string()),
        legend: LegendPosition::None,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: Some(StockVariant::Ohlc),
    }));

    // ── Page 11+: 데이터 분석 보고서 ──────────────────────────────
    paras.push(empty().with_page_break());

    paras.push(p("AI 시대의 컴퓨터공학 전공자 취업 동향 분석", CS_TITLE, PS_CENTER));
    paras.push(p("2026년 3월 보고서", CS_SMALL, PS_CENTER));
    paras.push(empty());

    // ── 1. 개요 ──────────────────────────────────────────────────────
    paras.push(p("1. 개요", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "2022년 ChatGPT 출시 이후 생성형 AI(Generative AI)의 급격한 발전은 \
         소프트웨어 개발 산업 전반에 구조적 변화를 가져왔다. \
         AI 코딩 도구(GitHub Copilot, Cursor 등)의 보편화로 개발 생산성이 \
         크게 향상된 반면, 신입 개발자의 채용 시장은 긴축 기조를 보이고 있다. \
         본 보고서는 2020~2025년 데이터를 바탕으로 AI가 \
         컴퓨터공학 전공자의 취업률에 미치는 영향을 분석한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // ── 2. 연도별 CS 전공자 취업률 추이 ──────────────────────────────
    paras.push(p("2. 연도별 CS 전공자 취업률 추이", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "한국교육개발원 취업통계, Stanford AI Index 2025, \
         뉴욕 연방준비은행(Federal Reserve Bank of New York) 데이터를 종합한 \
         연도별 주요 지표는 다음과 같다. 2025년 기준 미국 CS 졸업생 실업률은 \
         6.1%로 전체 전공 중 7번째로 높으며, 한국 주요 대학 CS 취업률도 \
         서울대 83.8%(2023)→72.6%(2025) 등 큰 폭으로 하락하고 있다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Table 1: 연도별 취업률
    let cell = |text: &str, cs: u32| -> TableCell {
        TableCell::new(vec![p(text, cs, PS_CENTER)], HwpUnit::from_mm(28.0).unwrap())
    };

    let t1 = Table {
        rows: vec![
            TableRow {
                height: None,
                cells: vec![
                    cell("연도", CS_BOLD),
                    cell("취업률(%)", CS_BOLD),
                    cell("AI직무 비중(%)", CS_BOLD),
                    cell("평균연봉(만원)", CS_BOLD),
                    cell("채용공고수(만)", CS_BOLD),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2020", CS_NORMAL),
                    cell("67.2", CS_NORMAL),
                    cell("8.5", CS_NORMAL),
                    cell("3,850", CS_NORMAL),
                    cell("12.3", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2021", CS_NORMAL),
                    cell("69.8", CS_NORMAL),
                    cell("11.2", CS_NORMAL),
                    cell("4,200", CS_NORMAL),
                    cell("15.7", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2022", CS_NORMAL),
                    cell("72.5", CS_NORMAL),
                    cell("15.8", CS_NORMAL),
                    cell("4,680", CS_NORMAL),
                    cell("18.2", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2023", CS_NORMAL),
                    cell("68.1", CS_NORMAL),
                    cell("22.4", CS_NORMAL),
                    cell("4,950", CS_NORMAL),
                    cell("14.5", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2024", CS_NORMAL),
                    cell("64.3", CS_NORMAL),
                    cell("31.6", CS_NORMAL),
                    cell("5,120", CS_NORMAL),
                    cell("11.8", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell("2025", CS_NORMAL),
                    cell("61.7", CS_NORMAL),
                    cell("38.2", CS_NORMAL),
                    cell("5,350", CS_NORMAL),
                    cell("10.2", CS_NORMAL),
                ],
            },
        ],
        width: Some(HwpUnit::from_mm(140.0).unwrap()),
        caption: Some(Caption::new(
            vec![p("[표 1] 연도별 CS 전공자 취업 지표", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        )),
    };
    paras.push(Paragraph::with_runs(vec![Run::table(t1, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Chart: 취업률 vs AI직무 비중 추이
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Line,
        data: ChartData::category(
            &["2020", "2021", "2022", "2023", "2024", "2025"],
            &[
                ("취업률(%)", &[67.2, 69.8, 72.5, 68.1, 64.3, 61.7]),
                ("AI직무 비중(%)", &[8.5, 11.2, 15.8, 22.4, 31.6, 38.2]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("취업률 vs AI직무 비중 추이".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: Some(true),
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p(
        "2022년까지 상승 추세를 보이던 취업률은 2023년을 기점으로 하락 전환하였다. \
         Stanford Digital Economy Lab(2025.11)에 따르면, 22~25세 AI 노출 직군 \
         취업자는 2022년 정점 대비 20% 감소하였으며, 미국 엔트리레벨 테크 채용은 \
         2023→2024년 67% 급감하였다(Stanford). 한국에서도 SW 개발직 채용 공고 중 \
         신입 비율이 2022년 53.5%에서 2024년 37.4%로 16.1%p 감소하였다(한국노동연구원).",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty().with_page_break());

    // ── 3. 직무별 채용 시장 분석 ─────────────────────────────────────
    paras.push(p("3. 직무별 채용 시장 분석", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "Veritone Q1 2025 분석에 따르면, AI/ML 엔지니어 채용은 전년 대비 \
         41.8% 증가하였고, 생성형 AI 기술을 명시한 채용 공고는 2023년 16,000건에서 \
         2024년 66,000건으로 4배 폭증하였다(Stanford AI Index/Lightcast). \
         반면 BLS(미 노동통계국)는 컴퓨터 프로그래머 고용이 향후 10년간 \
         10% 감소할 것으로 전망한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Table 2: 직무별 비교
    let cell2 = |text: &str, cs: u32| -> TableCell {
        TableCell::new(vec![p(text, cs, PS_CENTER)], HwpUnit::from_mm(23.0).unwrap())
    };

    let t2 = Table {
        rows: vec![
            TableRow {
                height: None,
                cells: vec![
                    cell2("직무", CS_BOLD),
                    cell2("공고수", CS_BOLD),
                    cell2("전년대비", CS_BOLD),
                    cell2("평균연봉", CS_BOLD),
                    cell2("경쟁률", CS_BOLD),
                    cell2("요구경력", CS_BOLD),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("AI/ML 엔지니어", CS_NORMAL),
                    cell2("18,500", CS_NORMAL),
                    cell2("+42%", CS_BLUE),
                    cell2("6,800만", CS_NORMAL),
                    cell2("8.2:1", CS_NORMAL),
                    cell2("3년+", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("데이터 엔지니어", CS_NORMAL),
                    cell2("12,300", CS_NORMAL),
                    cell2("+28%", CS_BLUE),
                    cell2("5,900만", CS_NORMAL),
                    cell2("6.5:1", CS_NORMAL),
                    cell2("2년+", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("백엔드 개발", CS_NORMAL),
                    cell2("22,100", CS_NORMAL),
                    cell2("-12%", CS_RED),
                    cell2("5,200만", CS_NORMAL),
                    cell2("15.3:1", CS_NORMAL),
                    cell2("2년+", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("프론트엔드", CS_NORMAL),
                    cell2("15,800", CS_NORMAL),
                    cell2("-18%", CS_RED),
                    cell2("4,800만", CS_NORMAL),
                    cell2("18.7:1", CS_NORMAL),
                    cell2("1년+", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("DevOps/SRE", CS_NORMAL),
                    cell2("8,900", CS_NORMAL),
                    cell2("+15%", CS_BLUE),
                    cell2("6,200만", CS_NORMAL),
                    cell2("5.1:1", CS_NORMAL),
                    cell2("3년+", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell2("SI/SM", CS_NORMAL),
                    cell2("9,200", CS_NORMAL),
                    cell2("-25%", CS_RED),
                    cell2("3,600만", CS_NORMAL),
                    cell2("4.2:1", CS_NORMAL),
                    cell2("무관", CS_NORMAL),
                ],
            },
        ],
        width: Some(HwpUnit::from_mm(138.0).unwrap()),
        caption: Some(Caption::new(
            vec![p("[표 2] 2025년 IT 직무별 채용 현황", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        )),
    };
    paras.push(Paragraph::with_runs(vec![Run::table(t2, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Chart: 직무별 전년대비 증감
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Bar,
        data: ChartData::category(
            &["AI/ML", "데이터", "백엔드", "프론트", "DevOps", "SI/SM"],
            &[("전년대비(%)", &[42.0, 28.0, -12.0, -18.0, 15.0, -25.0])],
        ),
        width: med_w,
        height: med_h,
        title: Some("직무별 채용 증감률 (전년대비 %)".to_string()),
        legend: LegendPosition::None,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── 4. AI 도구 활용 역량과 채용 상관관계 ─────────────────────────
    paras.push(p("4. AI 도구 활용 역량과 채용 상관관계", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "Stack Overflow 2025 Developer Survey(49,000명+, 177개국)에 따르면, \
         개발자의 84%가 AI 도구를 사용하며 51%는 매일 사용한다. \
         PwC 2025 Global AI Jobs Barometer는 AI 스킬 보유자의 임금 프리미엄이 \
         56%에 달한다고 보고했다. 특히 Anthropic 내부 설문에서 \
         AI 활용 엔지니어의 생산성 향상은 50%로 나타났다(1년 전 20%에서 상승).",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Table 3: AI 도구 활용 수준별 취업 지표
    let cell3 = |text: &str, cs: u32| -> TableCell {
        TableCell::new(vec![p(text, cs, PS_CENTER)], HwpUnit::from_mm(35.0).unwrap())
    };

    let t3 = Table {
        rows: vec![
            TableRow {
                height: None,
                cells: vec![
                    cell3("AI 활용 수준", CS_BOLD),
                    cell3("면접 통과율", CS_BOLD),
                    cell3("연봉 프리미엄", CS_BOLD),
                    cell3("취업 소요기간", CS_BOLD),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell3("미활용", CS_NORMAL),
                    cell3("22%", CS_NORMAL),
                    cell3("기준", CS_NORMAL),
                    cell3("평균 5.2개월", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell3("기본 활용", CS_NORMAL),
                    cell3("31%", CS_NORMAL),
                    cell3("+8%", CS_NORMAL),
                    cell3("평균 3.8개월", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell3("적극 활용", CS_NORMAL),
                    cell3("40%", CS_NORMAL),
                    cell3("+15%", CS_NORMAL),
                    cell3("평균 2.5개월", CS_NORMAL),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell3("AI 프로젝트 경험", CS_NORMAL),
                    cell3("52%", CS_NORMAL),
                    cell3("+28%", CS_NORMAL),
                    cell3("평균 1.8개월", CS_NORMAL),
                ],
            },
        ],
        width: Some(HwpUnit::from_mm(140.0).unwrap()),
        caption: Some(Caption::new(
            vec![p("[표 3] AI 도구 활용 수준별 취업 성과 (2025)", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        )),
    };
    paras.push(Paragraph::with_runs(vec![Run::table(t3, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Radar chart: 채용 시 요구되는 역량 비교
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Radar,
        data: ChartData::category(
            &["알고리즘", "시스템설계", "AI/ML", "커뮤니케이션", "문제해결", "코딩테스트"],
            &[
                ("2022 요구역량", &[9.0, 7.0, 4.0, 6.0, 8.0, 9.0]),
                ("2025 요구역량", &[7.0, 8.0, 9.0, 8.0, 9.0, 6.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("채용 시 요구역량 변화 (2022 vs 2025)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: Default::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: Some(RadarStyle::Filled),
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty().with_page_break());

    // ── 5. 결론 및 시사점 ────────────────────────────────────────────
    paras.push(p("5. 결론 및 시사점", CS_BOLD, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "WEF Future of Jobs Report 2025에 따르면, 2030년까지 1억 7,000만 개의 \
         신규 일자리가 창출되나 9,200만 개가 소멸하여 순 7,800만 개 증가가 예상된다. \
         그러나 Goldman Sachs(2025)는 AI 완전 채택 시 미국 고용의 6~7%가 위협받을 수 \
         있다고 경고한다. 특히 Anthropic CEO Dario Amodei는 5년 내 엔트리레벨 \
         화이트칼라 직무의 50%가 소멸할 수 있다고 전망하였다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Table 4: 요약 시사점
    let cell4w = |text: &str, cs: u32| -> TableCell {
        TableCell::new(vec![p(text, cs, PS_LEFT)], HwpUnit::from_mm(45.0).unwrap())
    };
    let cell4n = |text: &str, cs: u32| -> TableCell {
        TableCell::new(vec![p(text, cs, PS_LEFT)], HwpUnit::from_mm(95.0).unwrap())
    };

    let t4 = Table {
        rows: vec![
            TableRow {
                height: None,
                cells: vec![cell4w("항목", CS_BOLD), cell4n("시사점", CS_BOLD)],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell4w("취업률 전망", CS_NORMAL),
                    cell4n(
                        "2026년 CS 전공 취업률 60% 전후 예상. \
                         AI 직무 제외 시 50%대 하락 가능성",
                        CS_NORMAL,
                    ),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell4w("필수 역량 변화", CS_NORMAL),
                    cell4n(
                        "코딩테스트 비중 감소, AI/ML 활용 능력 및 \
                         시스템 설계 역량 중요도 상승",
                        CS_NORMAL,
                    ),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell4w("연봉 양극화", CS_NORMAL),
                    cell4n(
                        "AI 직무 평균연봉 6,800만원 vs SI 직무 3,600만원 \
                         — 1.9배 격차 확대 추세",
                        CS_NORMAL,
                    ),
                ],
            },
            TableRow {
                height: None,
                cells: vec![
                    cell4w("교육과정 대응", CS_NORMAL),
                    cell4n(
                        "대학 교육과정에 AI/ML 필수화 시급. \
                         현재 상위 20개교 중 AI 트랙 운영 비율 75%",
                        CS_NORMAL,
                    ),
                ],
            },
        ],
        width: Some(HwpUnit::from_mm(140.0).unwrap()),
        caption: Some(Caption::new(
            vec![p("[표 4] 주요 시사점 요약", CS_SMALL, PS_CENTER)],
            CaptionSide::Bottom,
        )),
    };
    paras.push(Paragraph::with_runs(vec![Run::table(t4, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());

    // Final stacked area chart: 향후 전망
    paras.push(chart_para(Control::Chart {
        chart_type: ChartType::Area,
        data: ChartData::category(
            &["2023", "2024", "2025", "2026E", "2027E"],
            &[
                ("AI/ML 직무", &[22.0, 32.0, 38.0, 45.0, 52.0]),
                ("전통 개발 직무", &[55.0, 48.0, 42.0, 38.0, 33.0]),
                ("기타 IT 직무", &[23.0, 20.0, 20.0, 17.0, 15.0]),
            ],
        ),
        width: med_w,
        height: med_h,
        title: Some("IT 채용 시장 직무 구성 전망 (%)".to_string()),
        legend: LegendPosition::Bottom,
        grouping: ChartGrouping::PercentStacked,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }));
    paras.push(empty());

    paras.push(p(
        "향후 AI 도구의 발전은 개발자의 역할을 '코드 작성자'에서 \
         'AI 오케스트레이터'로 전환시킬 것으로 전망된다. \
         컴퓨터공학 전공자에게는 기초 CS 역량(자료구조, 알고리즘, \
         운영체제)을 바탕으로 AI 시스템의 설계·평가·통합 능력을 \
         갖추는 것이 취업 경쟁력의 핵심이 될 것이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p(
        "※ 본 데이터는 한국교육개발원, GitHub Developer Survey 2025, \
         Stack Overflow Annual Survey, 사람인/원티드 채용 데이터를 종합 분석한 것임.",
        CS_SMALL,
        PS_LEFT,
    ));

    // ── Header / Footer / PageNumber ────────────────────────────────
    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.header = Some(HeaderFooter::new(
        vec![p("14. 차트 쇼케이스 — HwpForge", CS_SMALL, PS_LEFT)],
        ApplyPageType::Both,
    ));
    sec.footer = Some(HeaderFooter::new(
        vec![p("생성일: 2026-03-08", CS_SMALL, PS_RIGHT)],
        ApplyPageType::Both,
    ));
    sec.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    let mut doc = Document::new();
    doc.add_section(sec);
    encode_and_save("14_chart.hwpx", &store, &doc, &images);
}

fn gen_15_shapes_advanced() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "15. 고급 도형 종합 레퍼런스",
        "Arc(3종), Curve, ConnectLine, 선 스타일(5종), 화살표(6종), \
         회전(4단계), 반전(3종), 그라데이션(4종), 패턴(6종) 등 \
         HwpForge가 지원하는 모든 도형 옵션을 종합 시연합니다.",
    );

    // Common sizes
    let w8 = HwpUnit::new(8000).unwrap();
    let w10 = HwpUnit::new(10000).unwrap();
    let w14 = HwpUnit::new(14000).unwrap();
    let h6 = HwpUnit::new(6000).unwrap();
    let h8 = HwpUnit::new(8000).unwrap();

    let h1 = HwpUnit::new(1000).unwrap();

    // Helper: shape in paragraph
    let shape = |ctrl: Control| -> Paragraph {
        Paragraph::with_runs(vec![Run::control(ctrl, csi(CS_NORMAL))], psi(PS_LEFT))
    };

    // ═══════════════════════════════════════════════════════════════
    // 1. Arc (호) 도형 — ArcType 3종
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 1. Arc (호) 도형 ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Arc는 타원 위의 호를 그리는 도형입니다. ArcType에 따라 \
         Normal(열린 호), Pie(부채꼴 — 중심까지 선), Chord(활꼴 — 양 끝점 연결) \
         세 가지 형태로 렌더링됩니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Arc Normal
    paras.push(p("▸ Normal — 열린 호 (빨간 선, 두께 50)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Arc {
        arc_type: ArcType::Normal,
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        start1: ShapePoint::new(8000, 4000),
        end1: ShapePoint::new(4000, 0),
        start2: ShapePoint::new(4000, 8000),
        end2: ShapePoint::new(0, 4000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(200, 0, 0)),
            line_width: Some(50),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Arc Pie
    paras.push(p(
        "▸ Pie — 부채꼴 (파란 채움, 중심에서 호 양쪽 끝까지 직선 연결)",
        CS_BOLD,
        PS_LEFT,
    ));
    paras.push(shape(Control::Arc {
        arc_type: ArcType::Pie,
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        start1: ShapePoint::new(8000, 4000),
        end1: ShapePoint::new(4000, 0),
        start2: ShapePoint::new(4000, 8000),
        end2: ShapePoint::new(0, 4000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 0, 200)),
            fill_color: Some(Color::from_rgb(200, 220, 255)),
            line_width: Some(30),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Arc Chord
    paras.push(p("▸ Chord — 활꼴 (녹색 채움, 호 양쪽 끝점을 직선 연결)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Arc {
        arc_type: ArcType::Chord,
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        start1: ShapePoint::new(8000, 4000),
        end1: ShapePoint::new(4000, 0),
        start2: ShapePoint::new(4000, 8000),
        end2: ShapePoint::new(0, 4000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 150, 0)),
            fill_color: Some(Color::from_rgb(220, 255, 220)),
            line_width: Some(30),
            ..Default::default()
        }),
    }));
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 2. Curve & ConnectLine
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 2. Curve & ConnectLine ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Curve는 베지어 곡선으로 부드러운 곡선을 표현합니다. \
         CurveSegmentType::Curve(곡선)와 Line(직선)을 혼합할 수 있습니다. \
         ConnectLine은 두 도형을 연결하는 선입니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Bezier S-curve
    paras.push(p("▸ 베지어 S-곡선 (보라색, 두께 50)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Curve {
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
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(128, 0, 128)),
            line_width: Some(50),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Wave pattern (more points)
    paras.push(p("▸ 파동 곡선 (6개 제어점, 청록색, 두께 40)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Curve {
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
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0, 150, 150)),
            line_width: Some(40),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Mixed segments (Line + Curve)
    paras.push(p("▸ 혼합 세그먼트 (Line → Curve → Line, 주황색)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Curve {
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
        width: w8,
        height: h6,
        horz_offset: 0,
        vert_offset: 0,
        caption: None,
        style: Some(ShapeStyle {
            line_color: Some(Color::from_rgb(230, 120, 0)),
            line_width: Some(40),
            line_style: Some(LineStyle::Dash),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // ConnectLine — Straight
    paras.push(p("▸ ConnectLine (STRAIGHT, 양방향 화살표)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::ConnectLine {
        start: ShapePoint::new(0, 0),
        end: ShapePoint::new(10000, 4000),
        control_points: vec![ShapePoint::new(5000, 0), ShapePoint::new(5000, 4000)],
        connect_type: "STRAIGHT".to_string(),
        width: w10,
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
    }));
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 3. Line 스타일 — LineStyle 5종
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 3. 선 스타일 (LineStyle) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "LineStyle은 선의 패턴을 결정합니다. \
         Solid(실선), Dash(파선), Dot(점선), DashDot(일점쇄선), \
         DashDotDot(이점쇄선) 5가지가 있습니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    let line_styles: Vec<(LineStyle, &str, (u8, u8, u8))> = vec![
        (LineStyle::Solid, "Solid — 실선 (기본값)", (0, 0, 0)),
        (LineStyle::Dash, "Dash — 파선", (200, 0, 0)),
        (LineStyle::Dot, "Dot — 점선", (0, 0, 200)),
        (LineStyle::DashDot, "DashDot — 일점쇄선", (0, 150, 0)),
        (LineStyle::DashDotDot, "DashDotDot — 이점쇄선", (200, 100, 0)),
    ];
    for (ls, label, (r, g, b)) in &line_styles {
        paras.push(p(&format!("▸ {label}"), CS_BOLD, PS_LEFT));
        paras.push(shape(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: w14,
            height: h1,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(*r, *g, *b)),
                line_width: Some(50),
                line_style: Some(*ls),
                ..Default::default()
            }),
        }));
        paras.push(empty());
    }

    // Line width comparison
    paras.push(p("▸ 선 두께 비교: 20 / 50 / 100 / 200 (HWPUNIT)", CS_BOLD, PS_LEFT));
    for (width, label) in [(20, "20"), (50, "50"), (100, "100"), (200, "200")] {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(shape(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: w14,
            height: h1,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(50, 50, 50)),
                line_width: Some(width),
                ..Default::default()
            }),
        }));
    }
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 4. Arrow 스타일 — ArrowType 6종 × ArrowSize 3종
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 4. 화살표 스타일 (ArrowType) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "ArrowType은 6종: Normal(삼각형), Arrow(화살촉), Concave(오목), \
         Diamond(마름모), Oval(원형), Open(열린 삼각형). \
         각각 Small/Medium/Large 3가지 크기와 filled(채움)/unfilled(비움) 지정 가능. \
         head_arrow(시작점)와 tail_arrow(끝점)에 독립 설정됩니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    let arrow_types: Vec<(ArrowType, &str, (u8, u8, u8))> = vec![
        (ArrowType::Normal, "Normal — 삼각형 (filled)", (0, 0, 0)),
        (ArrowType::Arrow, "Arrow — 화살촉 (filled)", (200, 0, 0)),
        (ArrowType::Concave, "Concave — 오목 화살표 (filled)", (0, 0, 200)),
        (ArrowType::Diamond, "Diamond — 마름모 (filled)", (0, 150, 0)),
        (ArrowType::Oval, "Oval — 원형 (filled)", (150, 0, 150)),
        (ArrowType::Open, "Open — 열린 삼각형 (unfilled)", (200, 100, 0)),
    ];
    for (at, label, (r, g, b)) in &arrow_types {
        paras.push(p(&format!("▸ {label}"), CS_BOLD, PS_LEFT));
        paras.push(shape(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: w14,
            height: h1,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(*r, *g, *b)),
                line_width: Some(50),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: *at,
                    size: ArrowSize::Medium,
                    filled: *at != ArrowType::Open,
                }),
                ..Default::default()
            }),
        }));
        paras.push(empty());
    }

    // Arrow size comparison
    paras.push(p("▸ 크기 비교: Small / Medium / Large", CS_BOLD, PS_LEFT));
    for (sz, label) in
        [(ArrowSize::Small, "Small"), (ArrowSize::Medium, "Medium"), (ArrowSize::Large, "Large")]
    {
        paras.push(p(&format!("  {label}:"), CS_SMALL, PS_LEFT));
        paras.push(shape(Control::Line {
            start: ShapePoint::new(0, 500),
            end: ShapePoint::new(14000, 500),
            width: w14,
            height: h1,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(0, 0, 0)),
                line_width: Some(50),
                head_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Diamond,
                    size: sz,
                    filled: false,
                }),
                tail_arrow: Some(ArrowStyle {
                    arrow_type: ArrowType::Normal,
                    size: sz,
                    filled: true,
                }),
                ..Default::default()
            }),
        }));
    }
    paras.push(empty());

    // Both-end arrows (head + tail different)
    paras.push(p("▸ 양방향 화살표 (head: Diamond 비움, tail: Arrow 채움)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Line {
        start: ShapePoint::new(0, 500),
        end: ShapePoint::new(14000, 500),
        width: w14,
        height: h1,
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
    }));
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 5. 회전 (Rotation) — 0°, 45°, 90°, 135°
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 5. 회전 (Rotation) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "ShapeStyle.rotation으로 도형을 시계 방향으로 회전합니다. \
         0°~360° 범위의 실수값을 지원하며, 도형의 중심점을 기준으로 회전합니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // L-shaped polygon: asymmetric in both axes, so rotation is clearly visible.
    //   (0,0)──(4000,0)
    //   │          │
    //   │          │
    //   (0,3000)──(4000,3000)
    //              │
    //              │
    //              (4000,6000)──(6000,6000)
    //                           │
    //              (4000,8000)──(6000,8000)
    //
    let rot_verts = vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(4000, 0),
        ShapePoint::new(4000, 6000),
        ShapePoint::new(6000, 6000),
        ShapePoint::new(6000, 8000),
        ShapePoint::new(0, 8000),
        ShapePoint::new(0, 0),
    ];

    for (angle, (r, g, b)) in
        [(0.0_f32, (100, 100, 100)), (45.0, (200, 0, 0)), (90.0, (0, 0, 200)), (135.0, (0, 150, 0))]
    {
        paras.push(p(&format!("▸ 다각형 회전 {angle:.0}°"), CS_BOLD, PS_LEFT));
        paras.push(shape(Control::Polygon {
            vertices: rot_verts.clone(),
            width: HwpUnit::new(6000).unwrap(),
            height: HwpUnit::new(8000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![empty()],
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(r, g, b)),
                fill_color: Some(Color::from_rgb(200 + (r / 5), 200 + (g / 5), 200 + (b / 5))),
                rotation: Some(angle),
                ..Default::default()
            }),
        }));
        paras.push(empty());
    }
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 6. 반전 (Flip) — Horizontal, Vertical, Both
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 6. 반전 (Flip) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Flip은 도형을 거울처럼 반전합니다. Horizontal(좌우), \
         Vertical(상하), Both(좌우+상하 동시) 3가지 모드가 있습니다. \
         비대칭 화살표 모양 다각형으로 반전 효과를 확인합니다. \
         인코더는 flip 속성과 함께 rotMatrix에 반전값을 반영합니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Flag-shaped polygon: asymmetric in BOTH axes.
    // The flag points right and the "pole" extends downward,
    // making horizontal AND vertical flip clearly visible.
    //
    //   (0,0)──────(6000,0)
    //   │              ╲
    //   │               (8000,2500)  ← head shifted upward
    //   │              ╱
    //   (0,5000)──(6000,5000)
    //   │
    //   │  ← pole (only on left-bottom)
    //   │
    //   (0,8000)
    //
    let arrow_verts = vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(6000, 0),
        ShapePoint::new(8000, 2500),
        ShapePoint::new(6000, 5000),
        ShapePoint::new(1000, 5000),
        ShapePoint::new(1000, 8000),
        ShapePoint::new(0, 8000),
        ShapePoint::new(0, 0),
    ];

    for (flip, label, (r, g, b)) in [
        (Flip::None, "None — 원본 (반전 없음)", (100, 100, 100)),
        (Flip::Horizontal, "Horizontal — 좌우 반전", (0, 0, 200)),
        (Flip::Vertical, "Vertical — 상하 반전", (200, 0, 0)),
        (Flip::Both, "Both — 좌우+상하 반전 (180° 회전과 동일)", (0, 150, 0)),
    ] {
        paras.push(p(&format!("▸ {label}"), CS_BOLD, PS_LEFT));
        paras.push(shape(Control::Polygon {
            vertices: arrow_verts.clone(),
            width: w8,
            height: w8,
            horz_offset: 1,
            vert_offset: 0,
            paragraphs: vec![empty()],
            caption: None,
            style: Some(ShapeStyle {
                line_color: Some(Color::from_rgb(r, g, b)),
                fill_color: Some(Color::from_rgb(200 + (r / 5), 200 + (g / 5), 200 + (b / 5))),
                flip: Some(flip),
                ..Default::default()
            }),
        }));
        paras.push(empty());
    }
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 7. 그라데이션 채우기 — GradientType 4종
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 7. 그라데이션 채우기 (Gradient Fill) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Fill::Gradient로 그라데이션을 적용합니다. \
         GradientType: Linear(직선형), Radial(방사형), Square(사각형), \
         Conical(원뿔형). angle로 방향을, colors로 색상 정지점을 지정합니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Linear 90° = left→right in HWPX (angle measured from vertical axis)
    paras.push(p("▸ Linear 90° — 좌→우 (빨강→파랑)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        paragraphs: vec![p("좌→우", CS_WHITE, PS_CENTER)],
        caption: None,
        style: Some(ShapeStyle {
            fill: Some(Fill::Gradient {
                gradient_type: GradientType::Linear,
                angle: 90,
                colors: vec![(Color::from_rgb(255, 0, 0), 0), (Color::from_rgb(0, 0, 255), 100)],
            }),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Linear 0° = top→bottom in HWPX
    paras.push(p("▸ Linear 0° — 위→아래 (노랑→초록)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        paragraphs: vec![p("위→아래", CS_BOLD, PS_CENTER)],
        caption: None,
        style: Some(ShapeStyle {
            fill: Some(Fill::Gradient {
                gradient_type: GradientType::Linear,
                angle: 0,
                colors: vec![(Color::from_rgb(255, 255, 0), 0), (Color::from_rgb(0, 128, 0), 100)],
            }),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Linear 45° — diagonal, 2-color (사각형으로 대각선 확인)
    paras.push(p("▸ Linear 45° — 대각선 (빨→파, 사각형)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Polygon {
        vertices: vec![
            ShapePoint::new(0, 0),
            ShapePoint::new(8000, 0),
            ShapePoint::new(8000, 8000),
            ShapePoint::new(0, 8000),
            ShapePoint::new(0, 0),
        ],
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        paragraphs: vec![p("대각선 45°", CS_WHITE, PS_RIGHT)],
        caption: None,
        style: Some(ShapeStyle {
            fill: Some(Fill::Gradient {
                gradient_type: GradientType::Linear,
                angle: 45,
                colors: vec![(Color::from_rgb(255, 0, 0), 0), (Color::from_rgb(0, 0, 255), 100)],
            }),
            ..Default::default()
        }),
    }));
    paras.push(empty());

    // Radial
    paras.push(p("▸ Radial — 방사형 (중심에서 바깥으로, 흰→보라)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        width: w8,
        height: h8,
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
    }));
    paras.push(empty());

    // Square
    paras.push(p("▸ Square — 사각형 그라데이션 (중심→모서리, 흰→남색)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Polygon {
        vertices: vec![
            ShapePoint::new(0, 0),
            ShapePoint::new(8000, 0),
            ShapePoint::new(8000, 8000),
            ShapePoint::new(0, 8000),
            ShapePoint::new(0, 0),
        ],
        width: w8,
        height: h8,
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
    }));
    paras.push(empty());

    // Conical — 2-color to verify basic support
    paras.push(p("▸ Conical — 원뿔형 (빨→파, 2색)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        width: w8,
        height: h8,
        horz_offset: 0,
        vert_offset: 0,
        paragraphs: vec![p("원뿔형", CS_WHITE, PS_CENTER)],
        caption: None,
        style: Some(ShapeStyle {
            fill: Some(Fill::Gradient {
                gradient_type: GradientType::Conical,
                angle: 0,
                colors: vec![(Color::from_rgb(255, 0, 0), 0), (Color::from_rgb(0, 0, 255), 100)],
            }),
            ..Default::default()
        }),
    }));
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 8. 패턴 채우기 — PatternType 6종
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 8. 패턴 채우기 (Pattern Fill) ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Fill::Pattern으로 해칭 패턴을 적용합니다. 전경색(fg_color)과 \
         배경색(bg_color)을 지정하며, 6가지 PatternType이 있습니다: \
         Horizontal(수평선), Vertical(수직선), BackSlash(역사선), \
         Slash(사선), Cross(십자), CrossDiagonal(X자).",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    let patterns: Vec<(PatternType, &str, (u8, u8, u8), (u8, u8, u8))> = vec![
        (PatternType::Horizontal, "Horizontal — 수평선", (0, 0, 200), (230, 230, 255)),
        (PatternType::Vertical, "Vertical — 수직선", (200, 0, 0), (255, 230, 230)),
        (PatternType::BackSlash, "BackSlash — 역사선 (\\)", (0, 150, 0), (230, 255, 230)),
        (PatternType::Slash, "Slash — 사선 (/)", (150, 0, 150), (255, 230, 255)),
        (PatternType::Cross, "Cross — 십자 (+)", (0, 0, 0), (240, 240, 240)),
        (PatternType::CrossDiagonal, "CrossDiagonal — X자 (×)", (128, 64, 0), (255, 245, 230)),
    ];
    for (pt, label, (fr, fg, fb), (br, bg, bb)) in &patterns {
        paras.push(p(&format!("▸ {label}"), CS_BOLD, PS_LEFT));
        // Diamond shape for each pattern
        paras.push(shape(Control::Polygon {
            vertices: vec![
                ShapePoint::new(4000, 0),
                ShapePoint::new(8000, 4000),
                ShapePoint::new(4000, 8000),
                ShapePoint::new(0, 4000),
                ShapePoint::new(4000, 0),
            ],
            width: w8,
            height: h8,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![empty()],
            caption: None,
            style: Some(ShapeStyle {
                fill: Some(Fill::Pattern {
                    pattern_type: *pt,
                    fg_color: Color::from_rgb(*fr, *fg, *fb),
                    bg_color: Color::from_rgb(*br, *bg, *bb),
                }),
                ..Default::default()
            }),
        }));
        paras.push(empty());
    }
    paras.push(empty().with_page_break());

    // ═══════════════════════════════════════════════════════════════
    // 9. Solid 채우기 + 복합 스타일
    // ═══════════════════════════════════════════════════════════════
    paras.push(p("━━━ 9. 복합 스타일 조합 ━━━", CS_BOLD, PS_LEFT));
    paras.push(p(
        "여러 스타일 옵션을 동시에 적용한 복합 도형입니다. \
         Fill::Solid, 그라데이션+회전, 패턴+반전, 파선+화살표 등의 조합을 시연합니다.",
        CS_SMALL,
        PS_LEFT,
    ));
    paras.push(empty());

    // Solid fill ellipse
    paras.push(p("▸ Fill::Solid — 단색 채우기 (주황색 타원)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(4000, 4000),
        axis1: ShapePoint::new(8000, 4000),
        axis2: ShapePoint::new(4000, 8000),
        width: w8,
        height: h8,
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
    }));
    paras.push(empty());

    // Gradient + Rotation
    paras.push(p("▸ 그라데이션 + 회전 60° (Linear, 보라→청록 + 60° 회전)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Ellipse {
        center: ShapePoint::new(5000, 3000),
        axis1: ShapePoint::new(10000, 3000),
        axis2: ShapePoint::new(5000, 6000),
        width: w10,
        height: h6,
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
    }));
    paras.push(empty());

    // Pattern + Flip
    paras.push(p("▸ 패턴(CrossDiagonal) + 수직 반전 (오각형)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Polygon {
        vertices: vec![
            ShapePoint::new(4000, 0),
            ShapePoint::new(8000, 3000),
            ShapePoint::new(7000, 8000),
            ShapePoint::new(1000, 8000),
            ShapePoint::new(0, 3000),
            ShapePoint::new(4000, 0),
        ],
        width: w8,
        height: h8,
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
    }));
    paras.push(empty());

    // Dashed line with both-end different arrows + rotation
    paras.push(p("▸ 파선(DashDot) + 양방향 화살표 (head: Oval, tail: Concave)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::Line {
        start: ShapePoint::new(0, 500),
        end: ShapePoint::new(14000, 500),
        width: w14,
        height: h1,
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
    }));
    paras.push(empty());

    // Convenience constructor: horizontal_line
    paras.push(p("▸ Control::horizontal_line() — 편의 생성자 (기본 수평선)", CS_BOLD, PS_LEFT));
    paras.push(shape(Control::horizontal_line(w14)));
    paras.push(empty());

    // Gradient polygon with thick dashed border
    paras.push(p(
        "▸ Radial 그라데이션 + 파선 테두리 + 두꺼운 선 (별 모양 다각형)",
        CS_BOLD,
        PS_LEFT,
    ));
    // 5-point star
    paras.push(shape(Control::Polygon {
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
        width: w10,
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
    }));

    let mut sec = Section::with_paragraphs(paras, PageSettings::a4());
    sec.header = Some(HeaderFooter::new(
        vec![p("15. 고급 도형 종합 레퍼런스 — HwpForge", CS_SMALL, PS_LEFT)],
        ApplyPageType::Both,
    ));
    sec.footer = Some(HeaderFooter::new(
        vec![p("생성일: 2026-03-08", CS_SMALL, PS_RIGHT)],
        ApplyPageType::Both,
    ));
    sec.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    let mut doc = Document::new();
    doc.add_section(sec);
    encode_and_save("15_shapes_advanced.hwpx", &store, &doc, &images);
}

// ── Main ───────────────────────────────────────────────────────────

fn main() {
    println!("=== HwpForge Feature Showcase ===\n");
    std::fs::create_dir_all("temp").ok();

    gen_01_text();
    gen_02_rich_text();
    gen_03_table();
    gen_04_header_footer();
    gen_05_footnote_endnote();
    gen_06_textbox();
    gen_07_line();
    gen_08_ellipse();
    gen_09_polygon();
    gen_10_multi_column();
    gen_11_image();
    gen_12_hyperlink();
    gen_13_equation();
    gen_14_chart();
    gen_15_shapes_advanced();

    println!("\n=== 15 files generated in temp/ ===");
}
