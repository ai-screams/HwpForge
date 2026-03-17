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
use hwpforge_core::control::{Control, LineStyle, ShapePoint, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
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
            horz_offset: 0,
            vert_offset: 0,
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
    let mut store: HwpxStyleStore = HwpxStyleStore::with_default_fonts("함초롬돋움");
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

    let line_styles: &[(LineStyle, &str)] = &[
        (LineStyle::Solid, "실선 (SOLID)"),
        (LineStyle::Dash, "긴 점선 (DASH)"),
        (LineStyle::Dot, "점선 (DOT)"),
        (LineStyle::DashDot, "일점쇄선 (DASH_DOT)"),
    ];
    for (style_variant, label) in line_styles {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some(Color::BLACK),
                fill_color: None,
                line_width: Some(100),
                line_style: Some(*style_variant),
                ..Default::default()
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 3. 선 굵기 비교 (0.1mm ~ 1.5mm) ──
    paragraphs.push(p("3. 선 굵기 비교"));

    let widths: &[(u32, &str)] = &[
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
                line_color: Some(Color::from_rgb(0x33, 0x33, 0x33)),
                fill_color: None,
                line_width: Some(w),
                line_style: Some(LineStyle::Solid),
                ..Default::default()
            }),
        ));
    }
    paragraphs.push(p(""));

    // ── 4. 색상 라인 ──
    paragraphs.push(p("4. 색상 라인"));

    let colors: &[(Color, &str)] = &[
        (Color::from_rgb(0xFF, 0x00, 0x00), "빨강 (#FF0000)"),
        (Color::from_rgb(0xFF, 0x8C, 0x00), "주황 (#FF8C00)"),
        (Color::from_rgb(0xFF, 0xD7, 0x00), "금색 (#FFD700)"),
        (Color::from_rgb(0x00, 0x80, 0x00), "초록 (#008000)"),
        (Color::from_rgb(0x00, 0x00, 0xFF), "파랑 (#0000FF)"),
        (Color::from_rgb(0x4B, 0x00, 0x82), "남색 (#4B0082)"),
        (Color::from_rgb(0x80, 0x00, 0x80), "보라 (#800080)"),
    ];
    for &(color, label) in colors {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some(color),
                fill_color: None,
                line_width: Some(150),
                line_style: Some(LineStyle::Solid),
                ..Default::default()
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
            line_color: Some(Color::from_rgb(0xFF, 0x00, 0x00)),
            fill_color: None,
            line_width: Some(100),
            line_style: Some(LineStyle::Solid),
            ..Default::default()
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
            line_color: Some(Color::from_rgb(0x00, 0x00, 0xFF)),
            fill_color: None,
            line_width: Some(100),
            line_style: Some(LineStyle::Solid),
            ..Default::default()
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
            line_color: Some(Color::from_rgb(0x00, 0x80, 0x00)),
            fill_color: None,
            line_width: Some(150),
            line_style: Some(LineStyle::Solid),
            ..Default::default()
        }),
    ));
    paragraphs.push(p(""));

    // ── 7. 색상 + 스타일 조합 ──
    paragraphs.push(p("7. 색상+스타일 조합"));

    let combos: &[(Color, LineStyle, u32, &str)] = &[
        (Color::from_rgb(0xFF, 0x00, 0x00), LineStyle::Dash, 150, "빨강 대시"),
        (Color::from_rgb(0x00, 0x00, 0xFF), LineStyle::Dot, 100, "파랑 점선"),
        (Color::from_rgb(0x00, 0x80, 0x00), LineStyle::DashDot, 200, "초록 일점쇄선"),
        (Color::from_rgb(0xFF, 0x8C, 0x00), LineStyle::Solid, 300, "주황 굵은 실선"),
        (Color::from_rgb(0x80, 0x00, 0x80), LineStyle::Dash, 250, "보라 굵은 대시"),
    ];
    for &(color, style, width, label) in combos {
        paragraphs.push(p(&format!("  {label}")));
        paragraphs.push(line_para(
            ShapePoint::new(0, 0),
            ShapePoint::new(35000, 0),
            35000,
            100,
            Some(ShapeStyle {
                line_color: Some(color),
                fill_color: None,
                line_width: Some(width),
                line_style: Some(style),
                ..Default::default()
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
                line_color: Some(Color::from_rgb(0x55, 0x55, 0x55)),
                fill_color: None,
                line_width: Some(80),
                line_style: Some(LineStyle::Solid),
                ..Default::default()
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
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(Caption::new(
                    vec![p("그림 1. 구분선 (빨간 굵은 대시)")],
                    CaptionSide::Bottom,
                )),
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(0xCC, 0x00, 0x00)),
                    fill_color: None,
                    line_width: Some(200),
                    line_style: Some(LineStyle::Dash),
                    ..Default::default()
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
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(Caption::new(
                    vec![p("그림 2. 대각선 (파란 점선)")],
                    CaptionSide::Bottom,
                )),
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(0x00, 0x00, 0xCC)),
                    fill_color: None,
                    line_width: Some(150),
                    line_style: Some(LineStyle::Dot),
                    ..Default::default()
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
            line_color: Some(Color::from_rgb(0xCC, 0x00, 0x00)),
            fill_color: None,
            line_width: Some(200),
            line_style: Some(LineStyle::Solid),
            ..Default::default()
        }),
    ));
    // 아래쪽 선: 가는 빨강
    paragraphs.push(line_para(
        ShapePoint::new(0, 0),
        ShapePoint::new(42520, 0),
        42520,
        100,
        Some(ShapeStyle {
            line_color: Some(Color::from_rgb(0xCC, 0x00, 0x00)),
            fill_color: None,
            line_width: Some(50),
            line_style: Some(LineStyle::Solid),
            ..Default::default()
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
