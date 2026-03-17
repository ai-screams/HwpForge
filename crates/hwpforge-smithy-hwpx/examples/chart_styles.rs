//! Chart showcase: generates a single HWPX with all 18 chart types.
//!
//! Demonstrates every `ChartType` variant with sample data so you can
//! visually verify rendering in 한글.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example chart_styles
//!
//! Output:
//!   temp/chart_styles.hwpx

use hwpforge_core::chart::{ChartData, ChartType};
use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{PageNumber, Section};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, NumberFormatType, PageNumberPosition, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Helpers ──────────────────────────────────────────────────────

const CS0: CharShapeIndex = CharShapeIndex::new(0);
const PS0: ParaShapeIndex = ParaShapeIndex::new(0);

fn text_para(s: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(s, CS0)], PS0)
}

fn empty_para() -> Paragraph {
    text_para("")
}

fn chart_para(ctrl: Control) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CS0)], PS0)
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

// ── Sample data ─────────────────────────────────────────────────

fn category_data() -> ChartData {
    ChartData::category(
        &["1월", "2월", "3월", "4월"],
        &[("서울", &[120.0, 150.0, 180.0, 200.0]), ("부산", &[80.0, 100.0, 130.0, 160.0])],
    )
}

fn single_series_data() -> ChartData {
    ChartData::category(&["A", "B", "C", "D", "E"], &[("비율", &[35.0, 25.0, 20.0, 12.0, 8.0])])
}

fn xy_data() -> ChartData {
    ChartData::xy(&[
        ("실험1", &[1.0, 2.0, 3.0, 4.0, 5.0], &[2.1, 4.3, 5.8, 8.1, 9.7]),
        ("실험2", &[1.0, 2.0, 3.0, 4.0, 5.0], &[1.5, 3.0, 4.5, 6.0, 7.5]),
    ])
}

fn stock_data() -> ChartData {
    ChartData::category(
        &["월", "화", "수", "목", "금"],
        &[
            ("고가", &[105.0, 108.0, 107.0, 110.0, 112.0]),
            ("저가", &[98.0, 100.0, 99.0, 103.0, 105.0]),
            ("종가", &[102.0, 105.0, 103.0, 108.0, 110.0]),
        ],
    )
}

fn surface_data() -> ChartData {
    ChartData::category(
        &["X1", "X2", "X3", "X4"],
        &[
            ("Y1", &[10.0, 20.0, 30.0, 25.0]),
            ("Y2", &[15.0, 25.0, 35.0, 30.0]),
            ("Y3", &[20.0, 30.0, 40.0, 35.0]),
        ],
    )
}

// ── Main ─────────────────────────────────────────────────────────

fn main() {
    println!("=== Chart Type Showcase (18종) ===\n");
    std::fs::create_dir_all("temp").unwrap();

    let store = build_store();
    let images = ImageStore::new();

    let charts: Vec<(&str, ChartType, ChartData)> = vec![
        // ── Bar / Column 계열 ──
        ("1. 가로 막대 (Bar)", ChartType::Bar, category_data()),
        ("2. 세로 막대 (Column)", ChartType::Column, category_data()),
        ("3. 가로 막대 3D (Bar3D)", ChartType::Bar3D, category_data()),
        ("4. 세로 막대 3D (Column3D)", ChartType::Column3D, category_data()),
        // ── Line 계열 ──
        ("5. 꺾은선 (Line)", ChartType::Line, category_data()),
        ("6. 꺾은선 3D (Line3D)", ChartType::Line3D, category_data()),
        // ── Pie 계열 ──
        ("7. 원형 (Pie)", ChartType::Pie, single_series_data()),
        ("8. 원형 3D (Pie3D)", ChartType::Pie3D, single_series_data()),
        ("9. 도넛형 (Doughnut)", ChartType::Doughnut, single_series_data()),
        ("10. 원형 대 원형 (OfPie)", ChartType::OfPie, single_series_data()),
        // ── Area 계열 ──
        ("11. 영역 (Area)", ChartType::Area, category_data()),
        ("12. 영역 3D (Area3D)", ChartType::Area3D, category_data()),
        // ── XY 계열 ──
        ("13. 분산형 (Scatter)", ChartType::Scatter, xy_data()),
        ("14. 거품형 (Bubble)", ChartType::Bubble, xy_data()),
        // ── 기타 ──
        ("15. 방사형 (Radar)", ChartType::Radar, category_data()),
        ("16. 표면 (Surface)", ChartType::Surface, surface_data()),
        ("17. 표면 3D (Surface3D)", ChartType::Surface3D, surface_data()),
        ("18. 주식형 (Stock)", ChartType::Stock, stock_data()),
    ];

    let mut paras: Vec<Paragraph> = Vec::new();
    paras.push(text_para("HwpForge 차트(Chart) API 종합 데모 — 18종 전체"));
    paras.push(empty_para());

    for (label, chart_type, data) in charts {
        paras.push(text_para(label));
        paras.push(chart_para(Control::chart(chart_type, data)));
        paras.push(empty_para());
    }

    let mut section = Section::with_paragraphs(paras, PageSettings::a4());
    section.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    let mut doc = Document::new();
    doc.add_section(section);

    let validated = doc.validate().unwrap();
    let bytes = HwpxEncoder::encode(&validated, &store, &images).unwrap();

    let out_path = "temp/chart_styles.hwpx";
    std::fs::write(out_path, &bytes).unwrap();
    println!("Written to {out_path} ({} bytes)", bytes.len());
    println!("한글에서 열어서 18종 차트를 확인하세요!");
}
