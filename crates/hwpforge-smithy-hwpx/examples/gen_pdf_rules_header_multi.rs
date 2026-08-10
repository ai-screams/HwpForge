//! W5-a 머리말 다문단·overflow 시각 게이트 스캐폴드 —
//! `rules-header-multi`(2문단, 경계) + `rules-header-overflow`(4문단, 진초과).
//!
//! R6 실측은 지금까지 **한 문단짜리** 머리말만 증명했다 (Codex H1/H2). 이
//! 스캐폴드들은 기본 머리말 밴드(10mm=28.35pt)를 경계까지/넘어서 채우는
//! 다문단 머리말로, 한컴이 밴드를 자동 확장하는지 / 잘라내는지 / 본문을
//! 밀어내는지를 재저장 wire(margin·lineseg) + PDF 로 실측한다. 꼬리말
//! 1문단도 함께 넣어 같은 문서에서 꼬리말 텍스트 좌표를 재확인한다.
//!
//! Usage: `cargo run -p hwpforge-smithy-hwpx --example gen_pdf_rules_header_multi`
//! (워크스페이스 루트에서 실행 — 산출물은
//! `examples/hwp5_review/_verify/pdf-rules-scaffold/rules-header-{multi,overflow}.hwpx`)

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn generate(name: &str, header_lines: &[&str]) {
    let store = style_store_for_preset("latest").expect("latest preset");
    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    let header = HeaderFooter::all_pages(
        header_lines
            .iter()
            .map(|line| Paragraph::with_runs(vec![Run::text(*line, cs)], ps))
            .collect(),
    );
    let footer = HeaderFooter::all_pages(vec![Paragraph::with_runs(
        vec![Run::text("꼬리말 한 줄 — 좌표 재확인", cs)],
        ps,
    )]);

    let filler = "국가 연구개발 혁신을 위한 고성능컴퓨팅 자원 활용 계획서 표준 양식 검증 문장을 길게 이어 붙여 줄바꿈이 여러 번 일어나게 한다. The quick brown fox jumps over the lazy dog 1234567890 — 한영 혼합 폭 검증 문장.";
    let paragraphs: Vec<Paragraph> = (1..=24)
        .map(|n| Paragraph::with_runs(vec![Run::text(format!("문단 {n:02}: {filler}"), cs)], ps))
        .collect();

    let mut section = Section::with_paragraphs(paragraphs, PageSettings::a4());
    section.headers.push(header);
    section.footers.push(footer);

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");
    let dir = "examples/hwp5_review/_verify/pdf-rules-scaffold";
    std::fs::create_dir_all(dir).expect("mkdir");
    let path = format!("{dir}/{name}.hwpx");
    std::fs::write(&path, &bytes).expect("write");
    println!("스캐폴드 생성: {path} ({} bytes)", bytes.len());
}

fn main() {
    // 10pt 두 줄: em 은 밴드(28.35pt) 안에 들지만 spacing 포함 높이는 초과 — 경계 케이스.
    generate(
        "rules-header-multi",
        &["머리말 첫째 문단 — 밴드 실측", "머리말 둘째 문단 — overflow 경계"],
    );
    // 10pt 네 줄(v+vertsize=5800HU ≫ 2835HU): 진짜 밴드 초과 — 한컴의 자름/겹침 거동 실측.
    generate(
        "rules-header-overflow",
        &[
            "머리말 첫째 문단 — 진초과 실측",
            "머리말 둘째 문단 — 밴드 하단 통과",
            "머리말 셋째 문단 — 본문 영역 진입",
            "머리말 넷째 문단 — 겹침 또는 자름",
        ],
    );
    println!();
    println!("한컴오피스에서 할 일 (재저장 = 조판 캐시 생성):");
    println!("  1. 파일 열기 → 머리말 문단들이 어떻게 보이는지 확인 (잘림/확장 그대로 두기)");
    println!("  2. 다른 이름으로 저장 → 같은 폴더에 같은 이름 (.hwpx)");
    println!("  3. PDF 로 인쇄 → 같은 폴더에 같은 이름 (.pdf)");
}
