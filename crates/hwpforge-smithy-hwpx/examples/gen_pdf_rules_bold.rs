//! W4 bold 시각 게이트 스캐폴드 — `rules-bold` (함초롬바탕 보통/진하게).
//!
//! 함초롬바탕은 한컴 번들에 **실물 Bold face(HANBatangB)** 가 있는 확증
//! 계열이라, 이 스캐폴드를 한컴에서 재저장(조판 캐시 생성) + PDF 인쇄하면
//! W4c 의 bold face 선택 렌더를 한컴 출력과 직접 대조할 수 있다.
//!
//! Usage: `cargo run -p hwpforge-smithy-hwpx --example gen_pdf_rules_bold`
//! (워크스페이스 루트에서 실행 — 산출물은
//! `examples/hwp5_review/_verify/pdf-rules-scaffold/rules-bold.hwpx`)

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() {
    // "latest" 프리셋 = 함초롬바탕 10pt A4 (7축 모두 함초롬바탕 — 축 균일).
    let mut store = style_store_for_preset("latest").expect("latest preset");
    let mut bold_shape = store.char_shape(CharShapeIndex::new(0)).expect("char shape 0").clone();
    bold_shape.bold = true;
    let bold = store.push_char_shape(bold_shape);
    let regular = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    // 같은 문장을 보통/진하게로 반복 — 폭·잉크 차이를 줄 단위로 대조.
    let line = "다람쥐 헌 쳇바퀴에 타고파 Quartz glyphs 0123456789";
    let paragraphs = vec![
        Paragraph::with_runs(vec![Run::text(format!("보통: {line}"), regular)], ps),
        Paragraph::with_runs(vec![Run::text(format!("진하게: {line}"), bold)], ps),
        // 한 문단 안 보통↔진하게 run 혼합 (run 경계 폭 배분 대조).
        Paragraph::with_runs(
            vec![
                Run::text("혼합: 보통 다음 ", regular),
                Run::text("진하게 Bold 123", bold),
                Run::text(" 다시 보통 tail.", regular),
            ],
            ps,
        ),
        Paragraph::with_runs(
            vec![Run::text("진하게 장문단: 국가 연구개발 혁신을 위한 고성능컴퓨팅 자원 활용 계획서 표준 양식 검증 문장을 길게 이어 붙여 줄바꿈이 최소 두 번 일어나게 한다. The quick brown fox jumps over the lazy dog 1234567890 — 한영 혼합 폭 검증.", bold)],
            ps,
        ),
    ];

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");
    let dir = "examples/hwp5_review/_verify/pdf-rules-scaffold";
    std::fs::create_dir_all(dir).expect("mkdir");
    let path = format!("{dir}/rules-bold.hwpx");
    std::fs::write(&path, &bytes).expect("write");
    println!("스캐폴드 생성: {path} ({} bytes)", bytes.len());
    println!();
    println!("한컴오피스에서 할 일 (재저장 = 조판 캐시 생성):");
    println!("  1. 파일 열기 → 진하게 문단이 실제 굵게 보이는지 확인");
    println!("  2. 다른 이름으로 저장 → tests/fixtures/pdf-rules/rules-bold.hwpx");
    println!("  3. PDF 로 인쇄 → tests/fixtures/pdf-rules/rules-bold.pdf");
}
