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
        lines[..4].iter().flat_map(|l| l.runs.iter()).map(|r| r.text.as_str()).collect();
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
    assert_eq!(layout.pages[0].lines.len(), 42);
    assert_eq!(layout.pages[1].lines.len(), 18);
}
