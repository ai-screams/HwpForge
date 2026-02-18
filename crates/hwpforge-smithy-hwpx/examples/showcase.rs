//! Comprehensive showcase: exercises ALL implemented APIs and generates a .hwpx file.
//!
//! This example creates a multi-section document that tests every feature:
//! - Rich text (bold, italic, colored, underline)
//! - Tables with caption
//! - Images with caption + binary data (ImageStore)
//! - Header / Footer / Page numbers
//! - Footnotes and endnotes
//! - TextBox with caption
//! - Line / Ellipse / Polygon shapes with captions
//! - Multi-column layout
//! - Multiple sections with different page settings
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example showcase
//!
//! Output:
//!   showcase_output.hwpx (in the project root)

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::ColumnSettings;
use hwpforge_core::control::{Control, ShapePoint};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, CharShapeIndex, Color, HwpUnit, LineSpacingType, NumberFormatType,
    PageNumberPosition, ParaShapeIndex, UnderlineType,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn make_caption(text: &str, side: CaptionSide) -> Caption {
    Caption {
        side,
        gap: HwpUnit::new(850).unwrap(),
        width: None,
        paragraphs: vec![text_para(text, 0, 0)],
    }
}

/// Minimal 1x1 red PNG (67 bytes).
fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // 8-bit RGB
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT chunk
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, // compressed
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, // pixel data
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND chunk
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

// ---------------------------------------------------------------------------
// Style Store Setup
// ---------------------------------------------------------------------------

fn build_style_store() -> HwpxStyleStore {
    let mut store: HwpxStyleStore = HwpxStyleStore::new();

    // Fonts
    store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
    store.push_font(HwpxFont::new(1, "Arial", "LATIN"));

    // CharShape 0: Normal (10pt, black)
    let cs0: HwpxCharShape = HwpxCharShape::default();
    store.push_char_shape(cs0);

    // CharShape 1: Bold + Red (12pt)
    let mut cs1: HwpxCharShape = HwpxCharShape::default();
    cs1.height = HwpUnit::from_pt(12.0).unwrap();
    cs1.text_color = Color::from_rgb(200, 0, 0);
    cs1.bold = true;
    store.push_char_shape(cs1);

    // CharShape 2: Italic + Blue + Underline (10pt)
    let mut cs2: HwpxCharShape = HwpxCharShape::default();
    cs2.text_color = Color::from_rgb(0, 0, 200);
    cs2.italic = true;
    cs2.underline_type = UnderlineType::Bottom;
    store.push_char_shape(cs2);

    // CharShape 3: Title (16pt, bold)
    let mut cs3: HwpxCharShape = HwpxCharShape::default();
    cs3.height = HwpUnit::from_pt(16.0).unwrap();
    cs3.bold = true;
    store.push_char_shape(cs3);

    // ParaShape 0: Left align, 160% line spacing
    let ps0: HwpxParaShape = HwpxParaShape::default();
    store.push_para_shape(ps0);

    // ParaShape 1: Center align
    let mut ps1: HwpxParaShape = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    store.push_para_shape(ps1);

    // ParaShape 2: Justify with indent
    let mut ps2: HwpxParaShape = HwpxParaShape::default();
    ps2.alignment = Alignment::Justify;
    ps2.indent = HwpUnit::new(400).unwrap();
    ps2.line_spacing = 180;
    ps2.line_spacing_type = LineSpacingType::Percentage;
    ps2.spacing_after = HwpUnit::new(200).unwrap();
    store.push_para_shape(ps2);

    store
}

// ---------------------------------------------------------------------------
// Section 1: Rich Text + Table + Footnote/Endnote
// ---------------------------------------------------------------------------

fn build_section1() -> Section {
    let mut paragraphs: Vec<Paragraph> = Vec::new();

    // Title paragraph (CharShape 3 = 16pt bold, ParaShape 1 = center)
    paragraphs.push(Paragraph::with_runs(
        vec![Run::text("HwpForge 쇼케이스 문서", CharShapeIndex::new(3))],
        ParaShapeIndex::new(1),
    ));

    // Normal paragraph
    paragraphs.push(text_para("이 문서는 HwpForge의 모든 구현된 API를 테스트합니다.", 0, 0));

    // Mixed styles paragraph
    paragraphs.push(Paragraph::with_runs(
        vec![
            Run::text("일반 텍스트, ", CharShapeIndex::new(0)),
            Run::text("굵은 빨간색", CharShapeIndex::new(1)),
            Run::text(", ", CharShapeIndex::new(0)),
            Run::text("기울임 파란색 밑줄", CharShapeIndex::new(2)),
            Run::text(" — 스타일 혼합 테스트.", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    ));

    // Paragraph with footnote
    paragraphs.push(Paragraph::with_runs(
        vec![
            Run::text("각주가 포함된 문장입니다", CharShapeIndex::new(0)),
            Run::control(
                Control::Footnote {
                    inst_id: Some(1),
                    paragraphs: vec![text_para("이것은 각주(footnote) 내용입니다.", 0, 0)],
                },
                CharShapeIndex::new(0),
            ),
            Run::text(".", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    ));

    // Paragraph with endnote
    paragraphs.push(Paragraph::with_runs(
        vec![
            Run::text("미주가 포함된 문장입니다", CharShapeIndex::new(0)),
            Run::control(
                Control::Endnote {
                    inst_id: Some(1),
                    paragraphs: vec![text_para("이것은 미주(endnote) 내용입니다.", 0, 0)],
                },
                CharShapeIndex::new(0),
            ),
            Run::text(".", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    ));

    // Table with caption (3 rows x 2 cols)
    let table: Table = {
        let header_row: TableRow = TableRow {
            cells: vec![
                TableCell::new(vec![text_para("항목", 1, 1)], HwpUnit::new(21260).unwrap()),
                TableCell::new(vec![text_para("내용", 1, 1)], HwpUnit::new(21260).unwrap()),
            ],
            height: None,
        };
        let row1: TableRow = TableRow {
            cells: vec![
                TableCell::new(vec![text_para("프로젝트", 0, 0)], HwpUnit::new(21260).unwrap()),
                TableCell::new(vec![text_para("HwpForge", 0, 0)], HwpUnit::new(21260).unwrap()),
            ],
            height: None,
        };
        let row2: TableRow = TableRow {
            cells: vec![
                TableCell::new(vec![text_para("버전", 0, 0)], HwpUnit::new(21260).unwrap()),
                TableCell::new(vec![text_para("v1.0-alpha", 0, 0)], HwpUnit::new(21260).unwrap()),
            ],
            height: None,
        };
        let mut t: Table = Table::new(vec![header_row, row1, row2]);
        t.caption = Some(make_caption("표 1. 프로젝트 현황", CaptionSide::Bottom));
        t
    };
    paragraphs.push(Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    ));

    // Build section with header, footer, page number
    let mut section: Section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    section.header = Some(HeaderFooter::new(
        vec![text_para("HwpForge 쇼케이스 — 머리글", 0, 1)],
        ApplyPageType::Both,
    ));

    section.footer = Some(HeaderFooter::new(
        vec![text_para("Copyright © 2026 HwpForge Project", 0, 1)],
        ApplyPageType::Both,
    ));

    section.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    section
}

// ---------------------------------------------------------------------------
// Section 2: Image + TextBox + Shapes + Multi-column
// ---------------------------------------------------------------------------

fn build_section2() -> Section {
    let mut paragraphs: Vec<Paragraph> = Vec::new();

    // Section title
    paragraphs.push(Paragraph::with_runs(
        vec![Run::text("섹션 2: 이미지, 글상자, 도형", CharShapeIndex::new(3))],
        ParaShapeIndex::new(1),
    ));

    // Image with caption
    let mut img: Image = Image::new(
        "BinData/showcase_image.png",
        HwpUnit::from_mm(60.0).unwrap(),
        HwpUnit::from_mm(40.0).unwrap(),
        ImageFormat::Png,
    );
    img.caption = Some(make_caption("그림 1. HwpForge 아키텍처 다이어그램", CaptionSide::Bottom));
    paragraphs.push(Paragraph::with_runs(
        vec![Run::image(img, CharShapeIndex::new(0))],
        ParaShapeIndex::new(1),
    ));

    // TextBox with caption
    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: vec![
                    text_para("글상자 제목", 1, 1),
                    text_para(
                        "글상자 안의 본문 텍스트입니다. 다양한 내용을 포함할 수 있습니다.",
                        0,
                        0,
                    ),
                ],
                width: HwpUnit::from_mm(80.0).unwrap(),
                height: HwpUnit::from_mm(30.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(make_caption("글상자 1. 설명", CaptionSide::Bottom)),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));

    // Line shape with caption
    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Line {
                start: ShapePoint::new(0, 0),
                end: ShapePoint::new(14000, 0),
                width: HwpUnit::new(14000).unwrap(),
                height: HwpUnit::new(100).unwrap(),
                caption: Some(make_caption("선 1. 구분선", CaptionSide::Bottom)),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));

    // Ellipse shape with text and caption
    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Ellipse {
                center: ShapePoint::new(5000, 5000),
                axis1: ShapePoint::new(10000, 5000),
                axis2: ShapePoint::new(5000, 8000),
                width: HwpUnit::new(10000).unwrap(),
                height: HwpUnit::new(6000).unwrap(),
                paragraphs: vec![text_para("타원 내부 텍스트", 0, 1)],
                caption: Some(make_caption("그림 2. 타원 도형", CaptionSide::Bottom)),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));

    // Polygon (triangle) with text and caption
    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Polygon {
                vertices: vec![
                    ShapePoint::new(5000, 0),
                    ShapePoint::new(10000, 8000),
                    ShapePoint::new(0, 8000),
                ],
                width: HwpUnit::new(10000).unwrap(),
                height: HwpUnit::new(8000).unwrap(),
                paragraphs: vec![text_para("삼각형", 0, 1)],
                caption: Some(make_caption("그림 3. 삼각형 도형", CaptionSide::Bottom)),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));

    let mut section: Section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    // Multi-column layout (2 columns)
    section.column_settings =
        Some(ColumnSettings::equal_columns(2, HwpUnit::from_mm(4.0).unwrap()).unwrap());

    section
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== HwpForge API 쇼케이스 ===\n");

    // 1. Build style store
    let store: HwpxStyleStore = build_style_store();
    println!(
        "[1] 스타일 스토어: {} fonts, {} char shapes, {} para shapes",
        store.font_count(),
        store.char_shape_count(),
        store.para_shape_count(),
    );

    // 2. Build image store with a tiny PNG
    let mut image_store: ImageStore = ImageStore::new();
    image_store.insert("showcase_image.png".to_string(), tiny_png());
    println!("[2] 이미지 스토어: {} images", image_store.len());

    // 3. Build document with 2 sections
    let mut doc: Document = Document::new();
    doc.add_section(build_section1());
    doc.add_section(build_section2());

    let sec1: &Section = &doc.sections()[0];
    let sec2: &Section = &doc.sections()[1];
    println!(
        "[3] 문서 구성: {} sections, S1={} paras, S2={} paras",
        doc.sections().len(),
        sec1.paragraphs.len(),
        sec2.paragraphs.len(),
    );
    println!(
        "    S1: header={}, footer={}, page_num={}, columns={}",
        sec1.header.is_some(),
        sec1.footer.is_some(),
        sec1.page_number.is_some(),
        sec1.column_settings.is_some(),
    );
    println!(
        "    S2: header={}, footer={}, page_num={}, columns={}",
        sec2.header.is_some(),
        sec2.footer.is_some(),
        sec2.page_number.is_some(),
        sec2.column_settings.is_some(),
    );

    // 4. Validate
    let validated = doc.validate().expect("validation failed");
    println!("[4] 문서 검증 완료 (Draft → Validated)");

    // 5. Encode to HWPX
    let bytes: Vec<u8> =
        HwpxEncoder::encode(&validated, &store, &image_store).expect("encode failed");
    let output_path: &str = "showcase_output.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");
    println!("[5] HWPX 파일 생성: {output_path} ({} bytes)", bytes.len());

    // 6. Decode back (roundtrip verification)
    let decoded = HwpxDecoder::decode(&bytes).expect("decode failed");
    let d = &decoded.document;
    println!("[6] 라운드트립 디코딩 완료:");
    println!("    Sections: {}", d.sections().len());

    // Section 1 verification
    let s1: &Section = &d.sections()[0];
    println!("    Section 1:");
    println!("      Paragraphs: {}", s1.paragraphs.len());
    println!("      Header: {}", s1.header.is_some());
    println!("      Footer: {}", s1.footer.is_some());
    println!("      PageNumber: {}", s1.page_number.is_some());

    // Check first paragraph text
    if let Some(text) = s1.paragraphs[0].runs[0].content.as_text() {
        println!("      First paragraph: \"{text}\"");
    }

    // Check table
    let table_run =
        s1.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.content.as_table().is_some());
    if let Some(run) = table_run {
        let table: &Table = run.content.as_table().unwrap();
        println!(
            "      Table: {}x{} cells, caption={}",
            table.rows.len(),
            table.rows[0].cells.len(),
            table.caption.is_some(),
        );
        if let Some(cap) = &table.caption {
            let text: String = cap
                .paragraphs
                .iter()
                .flat_map(|p| p.runs.iter())
                .filter_map(|r| r.content.as_text())
                .collect::<Vec<_>>()
                .join("");
            println!("      Table caption: \"{text}\" (side={:?})", cap.side);
        }
    }

    // Check footnote
    let has_footnote: bool = s1
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .any(|r| matches!(r.content.as_control(), Some(ctrl) if ctrl.is_footnote()));
    println!("      Has footnote: {has_footnote}");

    // Check endnote
    let has_endnote: bool = s1
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .any(|r| matches!(r.content.as_control(), Some(ctrl) if ctrl.is_endnote()));
    println!("      Has endnote: {has_endnote}");

    // Section 2 verification
    let s2: &Section = &d.sections()[1];
    println!("    Section 2:");
    println!("      Paragraphs: {}", s2.paragraphs.len());
    println!("      Columns: {:?}", s2.column_settings.as_ref().map(|c| c.columns.len()));

    // Check image with caption
    let image_run = s2.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.content.is_image());
    if let Some(run) = image_run {
        if let Some(img) = run.content.as_image() {
            println!(
                "      Image: {} ({}x{}, caption={})",
                img.path,
                img.width.as_i32(),
                img.height.as_i32(),
                img.caption.is_some(),
            );
        }
    }

    // Check shapes
    let shape_types: Vec<&str> = s2
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_control())
        .map(|c| match c {
            Control::TextBox { .. } => "TextBox",
            Control::Line { .. } => "Line",
            Control::Ellipse { .. } => "Ellipse",
            Control::Polygon { .. } => "Polygon",
            _ => "Other",
        })
        .collect();
    println!("      Shapes: {shape_types:?}");

    // Check binary image in store
    println!("    Image store: {} images", decoded.image_store.len());

    // 7. Style store verification
    println!("[7] 스타일 스토어 검증:");
    println!("    Fonts: {}", decoded.style_store.font_count());
    println!("    CharShapes: {}", decoded.style_store.char_shape_count());
    println!("    ParaShapes: {}", decoded.style_store.para_shape_count());

    println!("\n=== 쇼케이스 완료! ===");
    println!("파일을 한글(Hancom Office)에서 열어 확인하세요: {output_path}");
}
