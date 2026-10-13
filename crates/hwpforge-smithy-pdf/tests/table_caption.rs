//! D (설계 4.2.1): 기본 흐름 표의 위·아래 캡션 재생. 근거 fixture =
//! `tests/fixtures/user_samples/sample-table-caption-fullsz.hwpx`(한컴 macOS
//! 재저장: 3×2 표 둘, 아래 캡션 — 표 1 `fullSz=0`, 표 2 `fullSz=1`). 한컴
//! PDF 실측: 캡션 1 x 59.52 pt·기준선 159.36 pt, 캡션 2 x 56.64 pt·기준선
//! 237.96 pt, 표 2 위 182.52 pt.

use std::path::PathBuf;

use hwpforge_core::caption::CaptionSide;
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::run::RunContent;
use hwpforge_core::table::Table;
use hwpforge_smithy_pdf::source::{replay_layout, LaidLine, ReplayLayout};
use hwpforge_smithy_pdf::{PdfError, PdfInput, PdfOptions, PdfWarning};

fn decoded() -> hwpforge_smithy_hwpx::HwpxDocument {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/user_samples/sample-table-caption-fullsz.hwpx");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode")
}

/// The `n`-th table of the document (host paragraphs in order).
fn table_mut(doc: &mut Document<Draft>, n: usize) -> &mut Table {
    doc.sections_mut()[0]
        .paragraphs
        .iter_mut()
        .flat_map(|p| p.runs.iter_mut())
        .filter_map(|r| match &mut r.content {
            RunContent::Table(t) => Some(t.as_mut()),
            _ => None,
        })
        .nth(n)
        .expect("table")
}

fn replay(edit: impl FnOnce(&mut Document<Draft>)) -> Result<ReplayLayout, PdfError> {
    let mut d = decoded();
    edit(&mut d.document);
    let validated = d.document.validate().expect("validate");
    replay_layout(
        &PdfInput { document: &validated, styles: &d.style_store },
        &PdfOptions::default(),
    )
}

fn caption_lines(layout: &ReplayLayout) -> Vec<&LaidLine> {
    layout.pages[0].lines.iter().filter(|l| l.location.contains("/caption/")).collect()
}

/// The first line of table `n`'s first cell (`s0/p{host}/t0r0c0`).
fn first_cell_line(layout: &ReplayLayout, host: usize) -> &LaidLine {
    let prefix = format!("s0/p{host}/t0r0c0");
    layout.pages[0].lines.iter().find(|l| l.location.starts_with(&prefix)).expect("cell line")
}

fn pt(hu: i32) -> f64 {
    f64::from(hu) / 100.0
}

/// 두 캡션의 x·기준선이 한컴 PDF 와 ±0.5 pt, 표 2 가 표 1 의 흐름 소비만큼
/// 내려가고 경고 없음.
/// 이것을 실패시키는 것: `fullSz` 무시(x 2.83 어긋남) · 캡션 위치에 om.bottom
/// 포함 · 흐름에서 gap·H_c 누락(표 2 앵커 검산 실패) · 캡션 미방출.
#[test]
fn bottom_captions_match_hancom() {
    let layout = replay(|_| {}).expect("replay");
    let lines = caption_lines(&layout);
    assert_eq!(
        lines.len(),
        2,
        "{:?}",
        layout.pages[0].lines.iter().map(|l| &l.location).collect::<Vec<_>>()
    );
    for (line, (x, baseline)) in lines.iter().zip([(59.52, 159.36), (56.64, 237.96)]) {
        assert!(
            (pt(line.line_box.horzpos) - x).abs() <= 0.5,
            "{} x {}",
            line.location,
            pt(line.line_box.horzpos)
        );
        assert!(
            (pt(line.baseline_y) - baseline).abs() <= 0.5,
            "{} y {}",
            line.location,
            pt(line.baseline_y)
        );
    }
    assert!(
        !layout.warnings.iter().any(|w| matches!(w, PdfWarning::TableCaptionWidthMismatch { .. })),
        "{:?}",
        layout.warnings
    );
}

/// 위 캡션은 표를 `H_c + gap` (1,000 + 850) 내리고 캡션 상자가 host_v +
/// om.top 에 놓인다; 흐름 소비는 같다(다음 표 앵커 그대로).
/// 이것을 실패시키는 것: 첫 조각 감산 생략 · 캡션 상자 위치 오류.
#[test]
fn top_caption_lowers_the_table() {
    let bottom = replay(|_| {}).expect("replay");
    let top = replay(|d| table_mut(d, 0).caption.as_mut().unwrap().side = CaptionSide::Top)
        .expect("replay");
    let host = 1; // 표 1 host 문단
    assert_eq!(first_cell_line(&top, host).top_y - first_cell_line(&bottom, host).top_y, 1850);
    let caption = caption_lines(&top)[0];
    // body_top 8,504 + host_v 1,600 + om.top 283 = 10,387 = 캡션 상자 위.
    assert_eq!(caption.top_y, 10_387);
}

/// 아래 캡션이 쪽 본문 끝을 넘으면 거부.
/// 이것을 실패시키는 것: 거부 삭제.
#[test]
fn bottom_caption_past_the_page_end_is_refused() {
    let err = replay(|d| {
        let caption = table_mut(d, 1).caption.as_mut().unwrap();
        caption.paragraphs[0].layout_cache.as_mut().unwrap().lines[0].vertsize = 80_000;
    })
    .expect_err("past the page end");
    assert!(
        matches!(err, PdfError::UnsupportedContent { kind: "table caption past page end", .. }),
        "{err}"
    );
}

/// 왼쪽 캡션·캐시 없는 캡션·캡션 번호·글자 밖 내용은 각각 거부.
/// 이것을 실패시키는 것: 측 검사 · 캐시 검사 · 번호 거부 · 글자 밖 거부 각각 삭제.
#[test]
fn unsupported_table_captions_are_refused() {
    let side = replay(|d| table_mut(d, 0).caption.as_mut().unwrap().side = CaptionSide::Left)
        .expect_err("left");
    assert!(
        matches!(side, PdfError::UnsupportedContent { kind: "table caption side", .. }),
        "{side}"
    );
    let cacheless =
        replay(|d| table_mut(d, 0).caption.as_mut().unwrap().paragraphs[0].layout_cache = None)
            .expect_err("no cache");
    assert!(matches!(cacheless, PdfError::MissingLayoutCache { .. }), "{cacheless}");
    let numbered = replay(|d| {
        use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, Control};
        use hwpforge_core::run::Run;
        let number = Control::AutoNumber {
            kind: AutoNumberKind::Table,
            num: 1,
            format: AutoNumberFormat::default(),
        };
        let p = &mut table_mut(d, 0).caption.as_mut().unwrap().paragraphs[0];
        let cs = p.runs[0].char_shape_id;
        p.runs.push(Run::control(number, cs));
    })
    .expect_err("number");
    assert!(
        matches!(numbered, PdfError::UnsupportedContent { kind: "caption number", .. }),
        "{numbered}"
    );
    let shape = replay(|d| {
        use hwpforge_core::control::{Control, ShapePoint};
        use hwpforge_core::run::Run;
        let line = Control::line(ShapePoint { x: 0, y: 0 }, ShapePoint { x: 100, y: 0 }).unwrap();
        let p = &mut table_mut(d, 0).caption.as_mut().unwrap().paragraphs[0];
        let cs = p.runs[0].char_shape_id;
        p.runs.push(Run::control(line, cs));
    })
    .expect_err("shape");
    assert!(
        matches!(
            shape,
            PdfError::UnsupportedContent { kind: "non-text content in table caption", .. }
        ),
        "{shape}"
    );
}

/// 저장 폭이 배치 식과 다르면 경고 하나.
/// 이것을 실패시키는 것: 경고 삭제 · 식에서 `fullSz` 여백 누락(표 2 에 오경고).
#[test]
fn caption_width_mismatch_warns() {
    let layout = replay(|d| {
        table_mut(d, 0).caption.as_mut().unwrap().last_width =
            Some(hwpforge_foundation::HwpUnit::new(1000).unwrap())
    })
    .expect("replay");
    let n = layout
        .warnings
        .iter()
        .filter(|w| matches!(w, PdfWarning::TableCaptionWidthMismatch { saved: 1000, .. }))
        .count();
    assert_eq!(n, 1, "{:?}", layout.warnings);
}

/// host 줄 `horzpos = 1000` 이면 표·캡션이 함께 10 pt 오른쪽, `fullSz` 캡션은
/// 그 자리에서 om.left 만큼 왼쪽.
/// 이것을 실패시키는 것: 캡션 x 에서 host.horzpos 누락.
#[test]
fn host_horzpos_moves_table_and_caption_together() {
    let base = replay(|_| {}).expect("replay");
    let moved = replay(|d| {
        for p in d.sections_mut()[0].paragraphs.iter_mut() {
            if p.runs.iter().any(|r| matches!(r.content, RunContent::Table(_))) {
                p.layout_cache.as_mut().expect("host cache").lines[0].horzpos = 1000;
            }
        }
    })
    .expect("replay");
    for (b, m) in caption_lines(&base).iter().zip(caption_lines(&moved)) {
        assert_eq!(m.line_box.horzpos - b.line_box.horzpos, 1000, "{}", m.location);
    }
    assert_eq!(
        first_cell_line(&moved, 1).line_box.horzpos - first_cell_line(&base, 1).line_box.horzpos,
        1000
    );
}

fn user_sample(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/user_samples/{name}"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn replay_converted(name: &str) -> Result<ReplayLayout, PdfError> {
    let options = hwpforge_convert::ConvertOptions::default().with_carry_layout_cache(true);
    let (hwpx, _) =
        hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&user_sample(name), options)
            .expect("convert");
    let d = hwpforge_smithy_hwpx::HwpxDecoder::decode(&hwpx).expect("decode");
    let validated = d.document.validate().expect("validate");
    replay_layout(
        &PdfInput { document: &validated, styles: &d.style_store },
        &PdfOptions::default(),
    )
}

/// The `.hwp` of the fullsz fixture, converted with its cache, places both
/// captions where Hancom's PDF has them, like the Hancom-saved HWPX.
/// 이것을 실패시키는 것: HWP5 캡션 디코드 삭제(캡션 0) · 변환에서 `fullSz` 소실.
#[test]
fn converted_hwp_captions_match_hancom() {
    let layout = replay_converted("sample-table-caption-fullsz.hwp").expect("replay");
    let lines = caption_lines(&layout);
    assert_eq!(lines.len(), 2);
    for (line, (x, baseline)) in lines.iter().zip([(59.52, 159.36), (56.64, 237.96)]) {
        assert!((pt(line.line_box.horzpos) - x).abs() <= 0.5, "{} x", line.location);
        assert!((pt(line.baseline_y) - baseline).abs() <= 0.5, "{} y", line.location);
    }
}

/// 분할 fixture 의 표 1(아래 캡션, 60 행)은 지금 모형의 첫 조각(64,241)이 한컴
/// 저장값(62,818)과 허용 밖이라 거부된다 — 아래 캡션의 첫 조각 예약 여부가
/// 미확정(설계 §15.8, 한컴 판정 탐침 대기). 몰래 다르게 그리지 않는다는 고정.
#[test]
fn split_fixture_with_a_bottom_caption_is_refused_until_measured() {
    let err = replay_converted("sample-table-caption-split.hwp").expect_err("refused");
    assert!(matches!(err, PdfError::InvalidCache { .. }), "{err}");
}

fn first_caption_line(d: &mut Document<Draft>) -> &mut hwpforge_core::layout::LineSeg {
    let caption = table_mut(d, 0).caption.as_mut().unwrap();
    &mut caption.paragraphs[0].layout_cache.as_mut().unwrap().lines[0]
}

fn invalid_cache(r: Result<ReplayLayout, PdfError>) -> bool {
    matches!(r, Err(PdfError::InvalidCache { ref detail }) if detail.contains("/caption"))
}

/// D review r1 HIGH-1: a caption line cache that does not start at the text
/// start, or whose line starts at the text end, is refused — before, the
/// caption vanished from a successful PDF ("여백 미포함 캡션" = 9 UTF-16).
/// 이것을 실패시키는 것: 첫 줄 0 검사 삭제 · 마지막 줄 끝 검사 삭제 · 끝 검사를
/// `>` 로(둘째 줄이 길이에서 시작해도 통과).
#[test]
fn caption_lines_must_cover_the_text() {
    assert!(replay(|_| {}).is_ok());
    assert!(invalid_cache(replay(|d| first_caption_line(d).textpos = 1)));
    assert!(invalid_cache(replay(|d| first_caption_line(d).textpos = 9)));
    // A second line starting at the text end (the first still at 0).
    assert!(invalid_cache(replay(|d| {
        let caption = table_mut(d, 0).caption.as_mut().unwrap();
        let lines = &mut caption.paragraphs[0].layout_cache.as_mut().unwrap().lines;
        let mut second = lines[0].clone();
        second.textpos = 9;
        second.vertpos += second.vertsize;
        lines.push(second);
    })));
}

/// D review r1 MEDIUM-3: caption line values past the unit range are an
/// invalid cache, not an overflow (debug panic, release wraparound); the
/// range end itself is accepted.
/// 이것을 실패시키는 것: 범위 검사 삭제(`i32::MAX` 세로 위치 → panic) ·
/// `<=` 를 `<` 로(경계 100,000,000 거부).
#[test]
fn caption_geometry_out_of_range_is_invalid_cache() {
    assert!(invalid_cache(replay(|d| first_caption_line(d).vertpos = i32::MAX)));
    assert!(invalid_cache(replay(|d| first_caption_line(d).horzpos = i32::MAX)));
    assert!(invalid_cache(replay(|d| first_caption_line(d).horzpos = 100_000_001)));
    assert!(invalid_cache(replay(|d| first_caption_line(d).baseline = i32::MIN)));
    let edge = replay(|d| first_caption_line(d).horzpos = 100_000_000).expect("range end");
    assert!(caption_lines(&edge)[0].line_box.horzpos > 100_000_000);
}

/// Sets table 1's caption text and line starts (each line a copy of the
/// first, one line height lower).
fn caption_text(d: &mut Document<Draft>, text: &str, starts: &[u32]) {
    let caption = table_mut(d, 0).caption.as_mut().unwrap();
    let p = &mut caption.paragraphs[0];
    p.runs.truncate(1);
    p.runs[0].content = RunContent::Text(text.to_string());
    let lines = &mut p.layout_cache.as_mut().unwrap().lines;
    let mut line = lines[0].clone();
    lines.clear();
    for &start in starts {
        line.textpos = start;
        lines.push(line.clone());
        line.vertpos += line.vertsize;
    }
}

/// Review r3 M1·M2: the empty line after a closing line break, an empty
/// line in the middle and an empty paragraph are accepted; a line starting
/// at the end without a closing break, or inside a surrogate pair, is an
/// invalid cache.
/// 이것을 실패시키는 것: 끝 줄바꿈 뒤 빈 줄 거부(`A\n` [0,2]) · 서로게이트
/// 검사 삭제("😀" [0,1] 통과) · 서로게이트 검사를 높은 쪽으로("😀😀" [0,2] 거부).
#[test]
fn caption_line_breaks_and_surrogates() {
    for (text, starts) in
        [("A\n", &[0u32, 2][..]), ("A\n\nB", &[0, 2, 3]), ("", &[0]), ("😀😀", &[0, 2])]
    {
        assert!(replay(|d| caption_text(d, text, starts)).is_ok(), "{text:?} {starts:?}");
    }
    for (text, starts) in [("AB", &[0u32, 2][..]), ("😀", &[0, 1]), ("😀😀", &[0, 3])] {
        assert!(invalid_cache(replay(|d| caption_text(d, text, starts))), "{text:?} {starts:?}");
    }
}
