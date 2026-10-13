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

// ── F 셀 영역 채우기 (rules-table 의 첫 표에 영역을 얹는 합성) ──

/// rules-table 을 디코드해 첫 표의 칸 (0,0) 에 `zone_fill`·`zone_left_line` 인
/// borderFill 영역을 얹고 재생한다.
fn replay_with_zone(
    zone_fill: &str,
    zone_left_line: &str,
) -> Option<hwpforge_smithy_pdf::source::ReplayLayout> {
    use hwpforge_smithy_hwpx::style_store::HwpxBorderLine;
    let left = zone_left_line.to_string();
    replay_with_zone_fill(zone_fill, true, &move |f| {
        f.left = HwpxBorderLine { line_type: left.clone(), ..HwpxBorderLine::default() }
    })
}

/// [`replay_with_zone`] with the zone's border fill edited by `edit`, and
/// left out of the store when `register` is false.
fn replay_with_zone_fill(
    zone_fill: &str,
    register: bool,
    edit: &dyn Fn(&mut hwpforge_smithy_hwpx::style_store::HwpxBorderFill),
) -> Option<hwpforge_smithy_pdf::source::ReplayLayout> {
    use hwpforge_core::run::RunContent;
    use hwpforge_core::table::CellZone;
    use hwpforge_smithy_hwpx::style_store::{HwpxBorderFill, HwpxFill};
    let bytes = fixture("rules-table.hwpx")?;
    let mut decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let id = decoded.style_store.iter_border_fills().map(|b| b.id).max().unwrap_or(0) + 1;
    let mut fill = HwpxBorderFill::default_page_border();
    fill.id = id;
    fill.fill = Some(HwpxFill::WinBrush {
        face_color: zone_fill.into(),
        hatch_color: "#000000".into(),
        alpha: "0".into(),
    });
    edit(&mut fill);
    if register {
        decoded.style_store.push_border_fill(fill);
    }
    // The cell under the zone gets its own blue fill, so a test can tell the
    // cell's fill from none.
    let mut blue = HwpxBorderFill::default_page_border();
    blue.id = id + 1;
    blue.fill = Some(HwpxFill::WinBrush {
        face_color: "#0000FF".into(),
        hatch_color: "#000000".into(),
        alpha: "0".into(),
    });
    decoded.style_store.push_border_fill(blue);
    let mut zoned = false;
    decoded.document.for_each_paragraph_mut(|p| {
        for run in &mut p.runs {
            if let RunContent::Table(t) = &mut run.content {
                if !zoned {
                    t.cell_zones = vec![CellZone::new(0, 0, 0, 0, id)];
                    t.rows[0].cells[0].border_fill_id = Some(id + 1);
                    zoned = true;
                }
            }
        }
    });
    assert!(zoned, "rules-table has a table");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    Some(replay_layout(&input, &PdfOptions::default()).expect("replay"))
}

/// F: a cell inside a zone is filled with the zone's border fill; cells
/// outside keep theirs; a zone whose border fill has a line warns (zone
/// lines are not drawn).
/// 이것을 실패시키는 것: 영역 무시(칸 (0,0) 이 빨강이 아님) · 영역을 모든 칸에
/// (칸 (0,1) 도 빨강) · 선 있는 영역 경고 삭제 · 선 없는 영역에도 경고.
#[test]
fn cell_zone_fill_replaces_the_cell_fill() {
    use hwpforge_foundation::Color;
    let Some(layout) = replay_with_zone("#FF0000", "NONE") else { return };
    let red = Color::from_rgb(255, 0, 0);
    let rect = |suffix: &str| {
        layout.pages[0].rects.iter().find(|r| r.location.ends_with(suffix)).map(|r| r.color)
    };
    assert_eq!(rect("t0r0c0"), Some(red), "{:?}", layout.pages[0].rects);
    assert_ne!(rect("t0r0c1"), Some(red));
    let zone_warn = |l: &hwpforge_smithy_pdf::source::ReplayLayout| {
        l.warnings
            .iter()
            .filter(|w| {
                matches!(
                    w,
                    hwpforge_smithy_pdf::PdfWarning::UnsupportedTableStyle {
                        what: "cell zone border",
                        ..
                    }
                )
            })
            .count()
    };
    assert_eq!(zone_warn(&layout), 0);
    let lined = replay_with_zone("#FF0000", "SOLID").expect("fixture");
    assert_eq!(zone_warn(&lined), 1);
}

fn zone_warnings(l: &hwpforge_smithy_pdf::source::ReplayLayout, what: &str) -> usize {
    l.warnings
        .iter()
        .filter(|w| {
            matches!(w, hwpforge_smithy_pdf::PdfWarning::UnsupportedTableStyle { what: x, .. } if *x == what)
        })
        .count()
}

/// Review r1 MEDIUM-4: a zone whose border fill has only a diagonal (no
/// edge lines) still warns that zone lines are not drawn.
/// 이것을 실패시키는 것: 대각선 판정을 경고 조건에서 빼기.
#[test]
fn cell_zone_with_only_a_diagonal_warns() {
    let Some(layout) =
        replay_with_zone_fill("#FF0000", true, &|f| f.slash.border_type = "CENTER".into())
    else {
        return;
    };
    assert_eq!(zone_warnings(&layout, "cell zone border"), 1);
}

/// Review r1 HIGH-2: a zone whose border fill is not in the store leaves
/// the cell's own fill and is reported, instead of erasing the background.
/// 이것을 실패시키는 것: 조회 실패 분기 삭제(배경 사라짐) · 경고 삭제.
#[test]
fn cell_zone_with_a_missing_border_fill_keeps_the_cell_fill() {
    let Some(missing) = replay_with_zone_fill("#FF0000", false, &|_| {}) else { return };
    let rect = |l: &hwpforge_smithy_pdf::source::ReplayLayout| {
        l.pages[0].rects.iter().find(|r| r.location.ends_with("t0r0c0")).map(|r| r.color)
    };
    assert_eq!(rect(&missing), Some(hwpforge_foundation::Color::from_rgb(0, 0, 255)));
    assert_eq!(zone_warnings(&missing, "cell zone without its border fill"), 1);
}
