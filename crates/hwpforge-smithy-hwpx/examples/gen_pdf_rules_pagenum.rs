//! W5-b 쪽번호 시각 게이트 스캐폴드 — `rules-pagenum` (스타일 출처 판별).
//!
//! `hp:pageNum` 요소는 charPr 참조가 없어, 한컴이 번호를 **문서 기본
//! charPr(0)** 으로 찍는지 **전용 "쪽 번호" CHAR 스타일**로 찍는지 wire 만으로는
//! 알 수 없다. 이 스캐폴드는 문서 기본 charPr(0)을 16pt 진하게로 변조해 둘을
//! 갈라놓는다 — 한컴 PDF 에서 번호가 16pt 굵게면 기본 스타일, 10pt 보통이면
//! 전용 스타일이 출처다.
//!
//! Usage: `cargo run -p hwpforge-smithy-hwpx --example gen_pdf_rules_pagenum`
//! (워크스페이스 루트에서 실행 — 산출물은
//! `examples/hwp5_review/_verify/pdf-rules-scaffold/rules-pagenum.hwpx`)

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{PageNumber, Section};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    CharShapeIndex, HwpUnit, NumberFormatType, PageNumberPosition, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() {
    let mut store = style_store_for_preset("latest").expect("latest preset");
    // 문서 기본 charPr(0) 자체를 변조 — 본문·기본 스타일 전부 16pt 진하게가 된다.
    let default_shape = store.char_shape_mut(CharShapeIndex::new(0)).expect("char shape 0");
    default_shape.height = HwpUnit::from_pt(16.0).expect("16pt");
    default_shape.bold = true;

    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);
    let filler = "국가 연구개발 혁신을 위한 고성능컴퓨팅 자원 활용 계획서 표준 양식 검증 문장을 길게 이어 붙여 줄바꿈이 여러 번 일어나게 한다. The quick brown fox jumps over the lazy dog 1234567890 — 한영 혼합 폭 검증 문장.";
    let paragraphs: Vec<Paragraph> = (1..=18)
        .map(|n| Paragraph::with_runs(vec![Run::text(format!("문단 {n:02}: {filler}"), cs)], ps))
        .collect();

    let mut section = Section::with_paragraphs(paragraphs, PageSettings::a4());
    // 쪽번호: 아래 가운데 + 숫자 + 줄표 장식 ("- 1 -" 형태).
    section.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "-",
    ));

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");
    let dir = "examples/hwp5_review/_verify/pdf-rules-scaffold";
    std::fs::create_dir_all(dir).expect("mkdir");
    let path = format!("{dir}/rules-pagenum.hwpx");
    std::fs::write(&path, &bytes).expect("write");
    println!("스캐폴드 생성: {path} ({} bytes)", bytes.len());
    println!();
    println!("한컴오피스에서 할 일 (재저장 = 조판 캐시 생성):");
    println!("  1. 파일 열기 → 본문이 16pt 굵게, 쪽 아래 가운데에 쪽번호가 보이는지 확인");
    println!("  2. 다른 이름으로 저장 → tests/fixtures/pdf-rules/rules-pagenum.hwpx");
    println!("  3. PDF 로 인쇄 → tests/fixtures/pdf-rules/rules-pagenum.pdf");
}
