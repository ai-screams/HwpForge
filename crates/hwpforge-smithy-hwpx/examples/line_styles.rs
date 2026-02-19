//! Line style showcase: generates a single HWPX with diverse line variations.
//!
//! Demonstrates:
//! - Horizontal / vertical / diagonal lines
//! - Various stroke styles (solid, dash, dot, dash_dot)
//! - Different line widths (thin → thick)
//! - Colored lines (red, blue, green, orange, purple)
//! - Lines with captions
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example line_styles
//!
//! Output:
//!   temp/line_styles.hwpx

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{Control, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Helpers ──────────────────────────────────────────────────────

fn p(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn line_run(
    start: ShapePoint,
    end: ShapePoint,
    width: i32,
    height: i32,
    style: Option<ShapeStyle>,
) -> Run {
    Run::control(
        Control::Line {
            start,
            end,
            width: HwpUnit::new(width).unwrap(),
            height: HwpUnit::new(height).unwrap(),
            caption: None,
            style,
        },
        CharShapeIndex::new(0),
    )
}

fn line_para(
    start: ShapePoint,
    end: ShapePoint,
    width: i32,
    height: i32,
    style: Option<ShapeStyle>,
) -> Paragraph {
    Paragraph::with_runs(vec![line_run(start, end, width, height, style)], ParaShapeIndex::new(0))
}

fn build_store() -> HwpxStyleStore {
    let mut store: HwpxStyleStore = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

// ── Main ─────────────────────────────────────────────────────────

#[allow(clippy::vec_init_then_push)]
fn main() {
    println!("=== Line Style Showcase ===\n");
    std::fs::create_dir_all("temp").unwrap();

    let store: HwpxStyleStore = build_store();
    let images: ImageStore = ImageStore::new();

    let mut paragraphs: Vec<Paragraph> = Vec::new();

    // ── 1. 기본 수평선 (Default style) ──
    paragraphs.push(p("1. 기본 수평선 (기본 스타일, 검정 실선)"));
    paragraphs.push(line_para(ShapePoint::new(0, 0), ShapePoint::new(42520, 0), 42520, 100, None));
    paragraphs.push(p(""));

    // ── 2. 선 스타일 비교 (Solid / Dash / Dot / DashDot) ──
    paragraphs.push(p("2. 선 스타일 비교"));

    let line_styles: &[(&str, &str)] = &[
        ("SOLID", "실선 (SOLID)"),
        ("DASH", "긴 점선 (DASH)"),
        ("DOT", "점선 (DOT)"),
        ("DASH_DOT", "일점쇄선 (DASH_DOT)"),
    ];
    for &(style_name, label) in line_styles {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some("#000000".to_string()),
                fill_color: None,
                line_width: Some(100),
                line_style: Some(style_name.to_string()),
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 3. 선 굵기 비교 (0.1mm ~ 1.5mm) ──
    paragraphs.push(p("3. 선 굵기 비교"));

    let widths: &[(i32, &str)] = &[
        (28, "극세 (0.1mm)"),
        (56, "세 (0.2mm)"),
        (100, "보통 (0.35mm)"),
        (200, "굵은 (0.7mm)"),
        (283, "매우 굵은 (1.0mm)"),
        (425, "초굵은 (1.5mm)"),
    ];
    for &(w, label) in widths {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some("#333333".to_string()),
                fill_color: None,
                line_width: Some(w),
                line_style: Some("SOLID".to_string()),
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 4. 색상 라인 ──
    paragraphs.push(p("4. 색상 라인"));

    let colors: &[(&str, &str)] = &[
        ("#FF0000", "빨강"),
        ("#FF8C00", "주황"),
        ("#FFD700", "금색"),
        ("#008000", "초록"),
        ("#0000FF", "파랑"),
        ("#4B0082", "남색"),
        ("#800080", "보라"),
    ];
    for &(color, label) in colors {
        paragraphs.push(p(&format!("  {label} ({color})")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some(color.to_string()),
                fill_color: None,
                line_width: Some(150),
                line_style: Some("SOLID".to_string()),
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 5. 대각선 (좌상→우하, 우상→좌하) ──
    paragraphs.push(p("5. 대각선"));

    // 좌상 → 우하 (↘)
    paragraphs.push(p("  좌상→우하 대각선 (빨강)"));
    paragraphs.push(line_para(
        ShapePoint::new(0, 0),
        ShapePoint::new(20000, 10000),
        20000,
        10000,
        Some(ShapeStyle {
            line_color: Some("#FF0000".to_string()),
            fill_color: None,
            line_width: Some(100),
            line_style: Some("SOLID".to_string()),
        }),
    ));

    // 우상 → 좌하 (↙)
    paragraphs.push(p("  우상→좌하 대각선 (파랑)"));
    paragraphs.push(line_para(
        ShapePoint::new(20000, 0),
        ShapePoint::new(0, 10000),
        20000,
        10000,
        Some(ShapeStyle {
            line_color: Some("#0000FF".to_string()),
            fill_color: None,
            line_width: Some(100),
            line_style: Some("SOLID".to_string()),
        }),
    ));
    paragraphs.push(p(""));

    // ── 6. 수직선 ──
    paragraphs.push(p("6. 수직선"));
    paragraphs.push(line_para(
        ShapePoint::new(0, 0),
        ShapePoint::new(0, 14000),
        100,
        14000,
        Some(ShapeStyle {
            line_color: Some("#008000".to_string()),
            fill_color: None,
            line_width: Some(150),
            line_style: Some("SOLID".to_string()),
        }),
    ));
    paragraphs.push(p(""));

    // ── 7. 색상 + 스타일 조합 ──
    paragraphs.push(p("7. 색상+스타일 조합"));

    let combos: &[(&str, &str, i32, &str)] = &[
        ("#FF0000", "DASH", 150, "빨강 대시"),
        ("#0000FF", "DOT", 100, "파랑 점선"),
        ("#008000", "DASH_DOT", 200, "초록 일점쇄선"),
        ("#FF8C00", "SOLID", 300, "주황 굵은 실선"),
        ("#800080", "DASH", 250, "보라 굵은 대시"),
    ];
    for &(color, style, width, label) in combos {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some(color.to_string()),
                fill_color: None,
                line_width: Some(width),
                line_style: Some(style.to_string()),
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 8. 짧은 선분들 (길이 비교) ──
    paragraphs.push(p("8. 길이 비교 (짧은선 → 긴선)"));

    let lengths: &[(i32, &str)] =
        &[(5000, "짧은선"), (15000, "중간선"), (30000, "긴선"), (42520, "전체폭")];
    for &(len, label) in lengths {
        paragraphs.push(p(&format!("  {label} ({:.1}mm)", len as f64 / 283.0)));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(len, 0),
            len,
            100,
            Some(ShapeStyle {
                line_color: Some("#555555".to_string()),
                fill_color: None,
                line_width: Some(80),
                line_style: Some("SOLID".to_string()),
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 9. 캡션이 있는 선 ──
    paragraphs.push(p("9. 캡션이 있는 선"));
    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Line {
                start: ShapePoint::new(0, 0),
                end: ShapePoint::new(30000, 0),
                width: HwpUnit::new(30000).unwrap(),
                height: HwpUnit::new(100).unwrap(),
                caption: Some(Caption {
                    side: CaptionSide::Bottom,
                    gap: HwpUnit::new(850).unwrap(),
                    width: None,
                    paragraphs: vec![p("그림 1. 구분선 (빨간 굵은 대시)")],
                }),
                style: Some(ShapeStyle {
                    line_color: Some("#CC0000".to_string()),
                    fill_color: None,
                    line_width: Some(200),
                    line_style: Some("DASH".to_string()),
                }),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));
    paragraphs.push(p(""));

    paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            Control::Line {
                start: ShapePoint::new(0, 0),
                end: ShapePoint::new(30000, 5000),
                width: HwpUnit::new(30000).unwrap(),
                height: HwpUnit::new(5000).unwrap(),
                caption: Some(Caption {
                    side: CaptionSide::Bottom,
                    gap: HwpUnit::new(850).unwrap(),
                    width: None,
                    paragraphs: vec![p("그림 2. 대각선 (파란 점선)")],
                }),
                style: Some(ShapeStyle {
                    line_color: Some("#0000CC".to_string()),
                    fill_color: None,
                    line_width: Some(150),
                    line_style: Some("DOT".to_string()),
                }),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));
    paragraphs.push(p(""));

    // ── 10. 장식용 이중선 효과 (두 선 나란히) ──
    paragraphs.push(p("10. 장식용 이중선 효과"));
    // 위쪽 선: 굵은 빨강
    paragraphs.push(line_para(
        ShapePoint::new(0, 0),
        ShapePoint::new(42520, 0),
        42520,
        100,
        Some(ShapeStyle {
            line_color: Some("#CC0000".to_string()),
            fill_color: None,
            line_width: Some(200),
            line_style: Some("SOLID".to_string()),
        }),
    ));
    // 아래쪽 선: 가는 빨강
    paragraphs.push(line_para(
        ShapePoint::new(0, 0),
        ShapePoint::new(42520, 0),
        42520,
        100,
        Some(ShapeStyle {
            line_color: Some("#CC0000".to_string()),
            fill_color: None,
            line_width: Some(50),
            line_style: Some("SOLID".to_string()),
        }),
    ));

    // Build document
    let mut doc: Document = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));

    let validated = doc.validate().expect("validation failed");
    let bytes: Vec<u8> = HwpxEncoder::encode(&validated, &store, &images).expect("encode failed");

    let output_path: &str = "temp/line_styles.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");

    println!("  Generated: {output_path} ({} bytes)", bytes.len());
    println!("\n한글(Hancom Office)에서 열어 확인하세요!");
}
