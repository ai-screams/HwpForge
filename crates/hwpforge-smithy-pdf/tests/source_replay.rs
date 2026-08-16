//! W2c 통합 게이트 — 실물 fixture(한컴 재저장 캐시) 재생 좌표 검증.
//!
//! W0 실측값(`.docs/algorithms/pdf-cache-replay-rules.md`)과 대조한다:
//! rules-justify 는 pagePr margin top=5669·header=4252 → body_top=9921HU,
//! 2문단 × 4줄, 전 줄 hsize=48188HU, 줄 스텝 1600HU, baseline 850HU.

use std::path::PathBuf;

use hwpforge_smithy_pdf::source::replay_layout;
use hwpforge_smithy_pdf::{PdfInput, PdfOptions};

fn fixture(name: &str) -> Option<Vec<u8>> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/pdf-rules").join(name);
    std::fs::read(path).ok()
}

#[test]
fn rules_justify_replays_hancom_saved_cache_exactly() {
    // fixture-optional: pdf-rules 쌍이 없는 체크아웃에서는 건너뜀.
    let Some(bytes) = fixture("rules-justify.hwpx") else { return };
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };

    let layout = replay_layout(&input, &PdfOptions::default()).expect("replay");

    // 1쪽 (W0: 8줄 전부 1쪽).
    assert_eq!(layout.pages.len(), 1);
    let lines = &layout.pages[0].lines;
    assert_eq!(lines.len(), 8, "2문단 × 4줄");

    // body_top = margin_top(5669) + header(4252) = 9921HU (W0 §0 검증치).
    // 줄 baseline: body_top + v + 850, v = 0·1600·3200·4800 · 6400…11200.
    for (i, line) in lines.iter().enumerate() {
        let expected_v = 1600 * i as i32;
        assert_eq!(line.baseline_y, 9921 + expected_v + 850, "line {i}");
        assert_eq!(line.line_box.horzsize, 48188, "line {i}");
        // body_left = margin_left(5669) + horzpos(0).
        assert_eq!(line.line_box.horzpos, 5669, "line {i}");
    }

    // 문단 경계: 4째 줄과 8째 줄만 is_last_line.
    let last_flags: Vec<bool> = lines.iter().map(|l| l.is_last_line).collect();
    assert_eq!(last_flags, vec![false, false, false, true, false, false, false, true]);

    // md 프리셋 기본 정렬 = JUSTIFY (W0 실측: 두 문단 모두 양쪽정렬 렌더).
    assert!(lines.iter().all(|l| l.alignment == hwpforge_foundation::Alignment::Justify));

    // 텍스트 재조립 = 원문 (줄분할 무손실).
    let para0: String =
        lines[..4].iter().flat_map(|l| l.text_runs()).map(|r| r.text.as_str()).collect();
    assert!(para0.starts_with("본 과제는 한국어 공문서"), "{para0:?}");
    assert!(layout.warnings.is_empty(), "{:?}", layout.warnings);
}

#[test]
fn rules_headerfooter_splits_42_lines_per_page() {
    // W0 실측: 본문 60문단 → 1쪽 42줄 + 2쪽 18줄 (본문 높이 671.86pt ÷ 16pt 스텝).
    let Some(bytes) = fixture("rules-headerfooter.hwpx") else { return };
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };

    let layout = replay_layout(&input, &PdfOptions::default()).expect("replay");
    assert_eq!(layout.pages.len(), 2);
    // W5-a: 머리말/꼬리말 오버레이가 줄에 합류 — 본문 카운트는 불변이어야 한다.
    let body = |p: &hwpforge_smithy_pdf::source::PageLayout| {
        p.lines.iter().filter(|l| !l.location.contains("/h") && !l.location.contains("/f")).count()
    };
    let band = |p: &hwpforge_smithy_pdf::source::PageLayout, tag: &str| {
        p.lines.iter().filter(|l| l.location.contains(tag)).count()
    };
    assert_eq!(body(&layout.pages[0]), 42);
    assert_eq!(body(&layout.pages[1]), 18);
    // R6 실측: BOTH 머리말/꼬리말 각 1줄 — 매쪽 반복.
    for page in &layout.pages {
        assert_eq!(band(page, "/h0/"), 1, "머리말 1줄");
        assert_eq!(band(page, "/f0/"), 1, "꼬리말 1줄");
    }
}

// ── W3c 표 게이트 — 3쪽 분할 + 제목행 반복 (rules-pagespan3 쌍) ──

fn replay_fixture(name: &str) -> Option<hwpforge_smithy_pdf::source::ReplayLayout> {
    let bytes = fixture(name)?;
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    Some(replay_layout(&input, &PdfOptions::default()).expect("replay"))
}

/// 쪽의 표 데이터 첫 줄 텍스트 (번호 열) 를 찾는다.
fn first_table_number_on_page(page: &hwpforge_smithy_pdf::source::PageLayout) -> Option<String> {
    page.lines
        .iter()
        .filter(|l| l.location.contains("/t0r"))
        .filter(|l| !l.location.contains("r0c"))
        .find_map(|l| l.text_runs().next().map(|r| r.text.clone()))
}

#[test]
fn pagespan3_replays_three_page_split_with_exact_anchor() {
    let Some(layout) = replay_fixture("rules-pagespan3.hwpx") else { return };
    // 한컴 PDF 실측: 4쪽 (표가 1~3쪽, 후속 본문이 3~4쪽).
    assert_eq!(layout.pages.len(), 4, "쪽수 = 한컴 4쪽");
    // 분할 = 계산임을 정직하게 경고.
    assert!(
        layout
            .warnings
            .iter()
            .any(|w| matches!(w, hwpforge_smithy_pdf::PdfWarning::TablePaginationComputed { .. })),
        "{:?}",
        layout.warnings
    );
    // 분할점: p2 첫 데이터 행 = 44, p3 = 93 (pdftotext 실측 고정 — 게이트2 H8).
    assert_eq!(first_table_number_on_page(&layout.pages[1]).as_deref(), Some("44"));
    assert_eq!(first_table_number_on_page(&layout.pages[2]).as_deref(), Some("93"));
    // p4 는 표 없음 (후속 본문만).
    assert!(layout.pages[3].lines.iter().all(|l| !l.location.contains("/t0r")));
    // 후속 본문이 실제로 재생됐는지.
    let all_text: String = layout
        .pages
        .iter()
        .flat_map(|p| p.lines.iter())
        .flat_map(|l| l.text_runs())
        .map(|r| r.text.as_str())
        .collect();
    assert!(all_text.contains("표가 끝난 뒤의 첫 번째 후속 본문"), "후속 본문 소실");
    // 괘선/배경이 방출됐는지 (모든 표 쪽).
    assert!(!layout.pages[0].borders.is_empty(), "p1 괘선");
    assert!(!layout.pages[2].borders.is_empty(), "p3 괘선");
}

#[test]
fn pagespan3_repeat_inserts_header_on_every_continuation() {
    let Some(layout) = replay_fixture("rules-pagespan3-repeat.hwpx") else { return };
    assert_eq!(layout.pages.len(), 4, "쪽수 = 한컴 4쪽");
    // 연속 조각(2·3·4쪽) 전부 상단 제목행 삽입 (2026-08-06 pdftotext 실측).
    for page_idx in [1usize, 2, 3] {
        let has_repeated_header = layout.pages[page_idx]
            .lines
            .iter()
            .any(|l| l.location.contains("/rep") && l.text_runs().any(|r| r.text == "번호"));
        assert!(has_repeated_header, "p{} 반복 제목행 없음", page_idx + 1);
    }
    // 분할점: p2=44 · p3=92 · p4=140 (pdftotext 실측 고정).
    assert_eq!(first_table_number_on_page(&layout.pages[1]).as_deref(), Some("44"));
    assert_eq!(first_table_number_on_page(&layout.pages[2]).as_deref(), Some("92"));
    assert_eq!(first_table_number_on_page(&layout.pages[3]).as_deref(), Some("140"));
}

#[test]
fn rules_table_single_page_checksum_passes() {
    // 비분할 표: Σ행높이 == 재저장 sz 검산이 replay 안에서 통과해야 한다.
    let Some(layout) = replay_fixture("rules-table.hwpx") else { return };
    assert_eq!(layout.pages.len(), 1, "1쪽 표 fixture");
    assert!(
        !layout
            .warnings
            .iter()
            .any(|w| matches!(w, hwpforge_smithy_pdf::PdfWarning::TablePaginationComputed { .. })),
        "비분할 표는 계산 경고 없음"
    );
    assert!(!layout.pages[0].rects.is_empty() || !layout.pages[0].borders.is_empty());
}

/// blank-HPC 실전 프로브 (수동 — 리뷰 영역 미추적 파일이라 fixture-optional).
#[test]
#[ignore = "manual probe against untracked blank-HPC review artifact"]
fn probe_blank_hpc_replay() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/blank-hpc-application-2026.hwpx");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("blank-HPC not present — skip");
        return;
    };
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    match replay_layout(&input, &PdfOptions::default()) {
        Ok(layout) => {
            let mut warn_kinds = std::collections::BTreeMap::new();
            for w in &layout.warnings {
                *warn_kinds
                    .entry(format!("{w:?}").split('{').next().unwrap().trim().to_string())
                    .or_insert(0usize) += 1;
            }
            eprintln!("PAGES = {} (한컴 실측 9)", layout.pages.len());
            eprintln!("warnings = {warn_kinds:?}");
        }
        Err(e) => eprintln!("REJECTED: {e}"),
    }
}
