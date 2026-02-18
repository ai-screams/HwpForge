//! Feature isolation test: generates individual HWPX files per feature.
//!
//! Each file tests exactly ONE feature in isolation to identify which
//! feature causes the "파일을 읽거나 저장하는데 오류가 있습니다" error dialog.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example feature_isolation
//!
//! Output:
//!   temp/01_text_only.hwpx
//!   temp/02_rich_text.hwpx
//!   temp/03_table.hwpx
//!   temp/04_table_caption.hwpx
//!   temp/05_header_footer.hwpx
//!   temp/06_page_number.hwpx
//!   temp/07_footnote.hwpx
//!   temp/08_endnote.hwpx
//!   temp/09_textbox.hwpx
//!   temp/10_textbox_caption.hwpx
//!   temp/11_line.hwpx
//!   temp/12_ellipse.hwpx
//!   temp/13_polygon.hwpx
//!   temp/14_multi_column.hwpx
//!   temp/15_image.hwpx

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
    Alignment, ApplyPageType, CharShapeIndex, Color, HwpUnit, NumberFormatType,
    PageNumberPosition, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Helpers ──────────────────────────────────────────────────────

fn p(text: &str) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::text(text, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )
}

fn caption(text: &str) -> Caption {
    Caption {
        side: CaptionSide::Bottom,
        gap: HwpUnit::new(850).unwrap(),
        width: None,
        paragraphs: vec![p(text)],
    }
}

/// Minimal style store: 1 font (×7 lang groups), 1 char shape, 1 para shape.
fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();

    // Fonts: mirror across all 7 language groups (한글 requires this)
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }

    // CharShape 0: default (10pt, black)
    store.push_char_shape(HwpxCharShape::default());

    // ParaShape 0: left, 160% line spacing
    store.push_para_shape(HwpxParaShape::default());

    store
}

/// Extended store: adds bold/red style (cs=1, ps=1 center).
fn extended_store() -> HwpxStyleStore {
    let mut store = minimal_store();

    // CharShape 1: bold + red (12pt)
    let mut cs1 = HwpxCharShape::default();
    cs1.height = HwpUnit::from_pt(12.0).unwrap();
    cs1.bold = true;
    cs1.text_color = Color::from_rgb(200, 0, 0);
    store.push_char_shape(cs1);

    // ParaShape 1: center
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    store.push_para_shape(ps1);

    store
}

fn empty_images() -> ImageStore {
    ImageStore::new()
}

fn encode_and_save(name: &str, store: &HwpxStyleStore, doc: &Document, images: &ImageStore) {
    let path = format!("temp/{name}");
    let validated = doc.clone().validate().expect("validation");
    let bytes = HwpxEncoder::encode(&validated, store, images).expect("encode");
    std::fs::write(&path, &bytes).expect("write");
    println!("  ✅ {} ({} bytes)", path, bytes.len());
}

/// Minimal 1x1 red PNG (67 bytes).
fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
        0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
        0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC,
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

// ── Test generators ─────────────────────────────────────────────

fn main() {
    println!("=== 기능별 격리 테스트 HWPX 생성 ===\n");

    std::fs::create_dir_all("temp").unwrap();

    // 01. Text only
    {
        let store = minimal_store();
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![p("안녕하세요. 기본 텍스트입니다."), p("두 번째 문단입니다.")],
            PageSettings::a4(),
        ));
        encode_and_save("01_text_only.hwpx", &store, &doc, &empty_images());
    }

    // 02. Rich text (bold, color)
    {
        let store = extended_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![
                Run::text("일반 ", CharShapeIndex::new(0)),
                Run::text("굵은빨강 ", CharShapeIndex::new(1)),
                Run::text("다시 일반", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        encode_and_save("02_rich_text.hwpx", &store, &doc, &empty_images());
    }

    // 03. Table (no caption)
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let cell_w = HwpUnit::new(21260).unwrap();
        let table = Table::new(vec![
            TableRow {
                cells: vec![
                    TableCell::new(vec![p("A1")], cell_w),
                    TableCell::new(vec![p("B1")], cell_w),
                ],
                height: None,
            },
            TableRow {
                cells: vec![
                    TableCell::new(vec![p("A2")], cell_w),
                    TableCell::new(vec![p("B2")], cell_w),
                ],
                height: None,
            },
        ]);
        let para = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![p("표 테스트:"), para],
            PageSettings::a4(),
        ));
        encode_and_save("03_table.hwpx", &store, &doc, &empty_images());
    }

    // 04. Table + caption
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let cell_w = HwpUnit::new(21260).unwrap();
        let mut table = Table::new(vec![TableRow {
            cells: vec![
                TableCell::new(vec![p("셀1")], cell_w),
                TableCell::new(vec![p("셀2")], cell_w),
            ],
            height: None,
        }]);
        table.caption = Some(caption("표 1. 캡션 테스트"));
        let para = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        encode_and_save("04_table_caption.hwpx", &store, &doc, &empty_images());
    }

    // 05. Header + Footer
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let mut sec = Section::with_paragraphs(vec![p("머리글/바닥글 테스트")], PageSettings::a4());
        sec.header = Some(HeaderFooter::new(
            vec![p("머리글 텍스트")],
            ApplyPageType::Both,
        ));
        sec.footer = Some(HeaderFooter::new(
            vec![p("바닥글 텍스트")],
            ApplyPageType::Both,
        ));
        doc.add_section(sec);
        encode_and_save("05_header_footer.hwpx", &store, &doc, &empty_images());
    }

    // 06. Page number
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let mut sec = Section::with_paragraphs(vec![p("페이지 번호 테스트")], PageSettings::a4());
        sec.page_number = Some(PageNumber::new(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
        ));
        doc.add_section(sec);
        encode_and_save("06_page_number.hwpx", &store, &doc, &empty_images());
    }

    // 07. Footnote
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![
                Run::text("각주가 있는 텍스트", CharShapeIndex::new(0)),
                Run::control(
                    Control::Footnote {
                        inst_id: Some(1),
                        paragraphs: vec![p("각주 내용입니다.")],
                    },
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        encode_and_save("07_footnote.hwpx", &store, &doc, &empty_images());
    }

    // 08. Endnote
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![
                Run::text("미주가 있는 텍스트", CharShapeIndex::new(0)),
                Run::control(
                    Control::Endnote {
                        inst_id: Some(1),
                        paragraphs: vec![p("미주 내용입니다.")],
                    },
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        encode_and_save("08_endnote.hwpx", &store, &doc, &empty_images());
    }

    // 09. TextBox (no caption)
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    paragraphs: vec![p("글상자 내용")],
                    width: HwpUnit::from_mm(80.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![p("글상자 테스트:"), para],
            PageSettings::a4(),
        ));
        encode_and_save("09_textbox.hwpx", &store, &doc, &empty_images());
    }

    // 10. TextBox + caption
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    paragraphs: vec![p("글상자 내용")],
                    width: HwpUnit::from_mm(80.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: Some(caption("글상자 1. 캡션")),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        encode_and_save("10_textbox_caption.hwpx", &store, &doc, &empty_images());
    }

    // 11. Line shape
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![Run::control(
                Control::Line {
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(14000, 0),
                    width: HwpUnit::new(14000).unwrap(),
                    height: HwpUnit::new(100).unwrap(),
                    caption: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![p("선 테스트:"), para],
            PageSettings::a4(),
        ));
        encode_and_save("11_line.hwpx", &store, &doc, &empty_images());
    }

    // 12. Ellipse shape
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    center: ShapePoint::new(5000, 3000),
                    axis1: ShapePoint::new(10000, 3000),
                    axis2: ShapePoint::new(5000, 6000),
                    width: HwpUnit::new(10000).unwrap(),
                    height: HwpUnit::new(6000).unwrap(),
                    paragraphs: vec![p("타원")],
                    caption: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![p("타원 테스트:"), para],
            PageSettings::a4(),
        ));
        encode_and_save("12_ellipse.hwpx", &store, &doc, &empty_images());
    }

    // 13. Polygon shape
    {
        let store = minimal_store();
        let mut doc = Document::new();
        let para = Paragraph::with_runs(
            vec![Run::control(
                Control::Polygon {
                    vertices: vec![
                        ShapePoint::new(5000, 0),
                        ShapePoint::new(10000, 8000),
                        ShapePoint::new(0, 8000),
                    ],
                    width: HwpUnit::new(10000).unwrap(),
                    height: HwpUnit::new(8000).unwrap(),
                    paragraphs: vec![p("삼각형")],
                    caption: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![p("다각형 테스트:"), para],
            PageSettings::a4(),
        ));
        encode_and_save("13_polygon.hwpx", &store, &doc, &empty_images());
    }

    // 14. Multi-column (3 columns, natural overflow + columnBreak)
    {
        let store = minimal_store();
        let mut doc = Document::new();

        // Column 1: lots of text for natural overflow
        let long_text = "가나다라마바사아자차카타파하. \
            이것은 첫 번째 열에 들어갈 긴 텍스트입니다. \
            다단 레이아웃에서 텍스트가 자연스럽게 넘쳐 흐르는지 확인합니다. \
            한글 문서에서 다단은 신문(NEWSPAPER) 스타일로 작동합니다. \
            첫 번째 열이 가득 차면 자동으로 두 번째 열로 넘어가야 합니다.";
        let long_text2 = "추가 문단입니다. 첫 번째 열에 충분한 텍스트를 넣어서 \
            자연스럽게 두 번째 열로 넘어가는 것을 테스트합니다. \
            이 문단도 첫 번째 열에 배치되어야 합니다.";
        let long_text3 = "세 번째 문단: 아직도 첫 번째 열에 있을 수 있습니다. \
            텍스트가 충분히 길면 자연스럽게 다음 열로 넘어갑니다.";

        // Column 2: forced via columnBreak
        let mut col2_para = p("두 번째 열 시작 (columnBreak 사용)");
        col2_para.column_break = true;
        let col2_para2 = p("두 번째 열의 추가 내용입니다.");

        // Column 3: forced via another columnBreak
        let mut col3_para = p("세 번째 열 시작 (columnBreak 사용)");
        col3_para.column_break = true;
        let col3_para2 = p("세 번째 열의 마지막 내용.");

        let mut sec = Section::with_paragraphs(
            vec![
                p(long_text),
                p(long_text2),
                p(long_text3),
                col2_para,
                col2_para2,
                col3_para,
                col3_para2,
            ],
            PageSettings::a4(),
        );
        sec.column_settings =
            Some(ColumnSettings::equal_columns(3, HwpUnit::from_mm(4.0).unwrap()).unwrap());
        doc.add_section(sec);
        encode_and_save("14_multi_column.hwpx", &store, &doc, &empty_images());
    }

    // 15. Image (two images: generated tiny PNG + real duck PNG)
    {
        let store = minimal_store();
        let mut doc = Document::new();

        // Image 1: tiny generated 1x1 PNG
        let img1 = Image::new(
            "BinData/tiny_generated.png",
            HwpUnit::from_mm(20.0).unwrap(),
            HwpUnit::from_mm(20.0).unwrap(),
            ImageFormat::Png,
        );
        let para1 = Paragraph::with_runs(
            vec![Run::image(img1, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        // Image 2: real duck character PNG from tests/fixtures/
        let duck_bytes = std::fs::read("tests/fixtures/main-charactor.png")
            .expect("tests/fixtures/main-charactor.png should exist");
        let img2 = Image::new(
            "BinData/main-charactor.png",
            HwpUnit::from_mm(50.0).unwrap(),
            HwpUnit::from_mm(50.0).unwrap(),
            ImageFormat::Png,
        );
        let para2 = Paragraph::with_runs(
            vec![Run::image(img2, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        doc.add_section(Section::with_paragraphs(
            vec![p("이미지 테스트 (2개):"), para1, p("오리 마법사:"), para2],
            PageSettings::a4(),
        ));
        let mut images = ImageStore::new();
        images.insert("tiny_generated.png".to_string(), tiny_png());
        images.insert("main-charactor.png".to_string(), duck_bytes);
        encode_and_save("15_image.hwpx", &store, &doc, &images);
    }

    println!("\n=== 총 15개 파일 생성 완료 ===");
    println!("temp/ 폴더에서 한글로 하나씩 열어서 확인하세요.");
    println!("에러 다이얼로그 여부를 기록해주세요:");
    println!("  ✅ = 정상 (에러 없음)");
    println!("  ⚠️  = 에러 다이얼로그 (내용은 보임)");
    println!("  ❌ = 크래시 또는 열리지 않음");
}
