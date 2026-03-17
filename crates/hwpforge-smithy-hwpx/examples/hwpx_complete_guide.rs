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

#[path = "hwpx_complete_guide_parts/mod.rs"]
mod hwpx_complete_guide_parts;

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::numbering::NumberingDef;
use hwpforge_core::tab::TabDef;
use hwpforge_foundation::{Alignment, Color, HwpUnit};
use hwpforge_smithy_hwpx::style_store::{
    HwpxBorderFill, HwpxBorderLine, HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
};
use hwpforge_smithy_hwpx::HwpxEncoder;

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

fn main() {
    println!("HWPX 문서 구조 완전 가이드 생성 중...\n");

    // 스타일 스토어 구성
    let store = build_style_store();
    let mut image_store = ImageStore::new();
    let mascot_bytes =
        std::fs::read("assets/mascot-main.png").expect("assets/mascot-main.png not found");
    image_store.insert("image1.png", mascot_bytes);

    // 문서 구성: 4개 섹션
    let mut doc = Document::new();
    doc.add_section(hwpx_complete_guide_parts::section1_document_structure());
    doc.add_section(hwpx_complete_guide_parts::section2_text_formatting());
    doc.add_section(hwpx_complete_guide_parts::section3_shapes_and_graphics());
    doc.add_section(hwpx_complete_guide_parts::section4_charts_equations_advanced());

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
