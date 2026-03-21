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
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{Control, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, CharShapeIndex, Color, HwpUnit, NumberFormatType, PageNumberPosition,
    ParaShapeIndex, UnderlineType,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

#[path = "feature_isolation_large/mod.rs"]
mod feature_isolation_large;

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
    let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");

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
    feature_isolation_large::gen_13_equation();
    feature_isolation_large::gen_14_chart();
    feature_isolation_large::gen_15_shapes_advanced();

    println!("\n=== 15 files generated in temp/ ===");
}
