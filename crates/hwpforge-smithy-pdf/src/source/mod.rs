//! 배치소스 — 조판 캐시 재생 (HWPUNIT i32 정수 산술).
//!
//! 이 층은 **계산하지 않고 재생한다**: 줄바꿈·줄 위치·쪽 배치는 전부
//! 캐시([`hwpforge_core::layout::LayoutCache`]) 의 수치다. 이 층이 하는
//! 일은 세 가지뿐이다 (규칙 문서 §1-§2):
//!
//! 1. **쪽분할**: 최상위 lineseg 의 `v == 0 (첫 줄 제외) ∨ v < prev` → 새 쪽
//! 2. **줄분할**: `textpos` (UTF-16 코드유닛) 로 문단 텍스트를 줄로 절단
//! 3. **baseline**: `body_top + v + lineseg.baseline` (전부 HWPUNIT 정수합)
//!
//! pt 변환은 여기서 하지 않는다 — paint 경계([`crate::paint::Pt::from_hwpunit`])
//! 에서 1회만. admission(계획 §3 행렬)도 이 층이 수행한다: 렌더에 필요한
//! 검사를 렌더 직전에 하는 것이 fail-closed 의 실체다.

use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_foundation::{Alignment, CharShapeIndex};

use crate::text::align::LineBox;
use crate::{PartialCachePolicy, PdfError, PdfInput, PdfOptions, PdfResult, PdfWarning};

/// 한 줄 안에서 같은 문자 스타일을 공유하는 텍스트 구간.
#[derive(Debug, Clone, PartialEq)]
pub struct LineRun {
    /// 구간 텍스트.
    pub text: String,
    /// 문자 스타일 (폰트명·크기 조회 키).
    pub char_shape: CharShapeIndex,
}

/// 배치가 끝난 한 줄.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidLine {
    /// 위치 보고용 경로 (`s{섹션}/p{문단}/l{줄}`).
    pub location: String,
    /// 줄 텍스트 구간들 (run 경계 분할 — 시각 순서).
    pub runs: Vec<LineRun>,
    /// baseline 세로 위치 (HWPUNIT, 쪽 상단 원점 — body_top + v + baseline).
    pub baseline_y: i32,
    /// 줄 가로 상자 (HWPUNIT — body 좌변 반영, 정렬 미적용 상태).
    pub line_box: LineBox,
    /// 문단의 마지막 줄인지 (JUSTIFY 마지막 줄 규칙).
    pub is_last_line: bool,
    /// 문단 정렬.
    pub alignment: Alignment,
}

/// 한 쪽의 배치 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// 쪽 폭 (HWPUNIT).
    pub width: i32,
    /// 쪽 높이 (HWPUNIT).
    pub height: i32,
    /// 줄들 (문서 순서).
    pub lines: Vec<LaidLine>,
}

/// 캐시 재생 결과.
#[derive(Debug)]
pub struct ReplayLayout {
    /// 쪽들 (문서 순서).
    pub pages: Vec<PageLayout>,
    /// 수집된 경고 (스킵 문단·regular 외 run·미지원 컨트롤).
    pub warnings: Vec<PdfWarning>,
}

/// 문서 전체를 캐시 재생으로 배치한다 (admission 포함).
///
/// # Errors
///
/// 계획 §3 행렬: 표 포함([`PdfError::UnsupportedContent`]) · 렌더 가능 캐시
/// 0 섹션([`PdfError::NoRenderableCache`]) · Reject 정책 하 캐시 결손
/// ([`PdfError::MissingLayoutCache`]) · textpos 정합 위반([`PdfError::InvalidCache`]).
pub fn replay_layout(input: &PdfInput<'_>, options: &PdfOptions) -> PdfResult<ReplayLayout> {
    let mut pages = Vec::new();
    let mut warnings = Vec::new();

    for (section_idx, section) in input.document.sections().iter().enumerate() {
        replay_section(input, options, section, section_idx, &mut pages, &mut warnings)?;
    }
    Ok(ReplayLayout { pages, warnings })
}

fn replay_section(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    section: &Section,
    section_idx: usize,
    pages: &mut Vec<PageLayout>,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    let ps = &section.page_settings;
    let page_width = ps.width.as_i32();
    let page_height = ps.height.as_i32();
    // body 원점 (규칙 §0): top = margin_top + header_margin, left = margin_left.
    let body_top = ps.margin_top.as_i32() + ps.header_margin.as_i32();
    let body_left = ps.margin_left.as_i32();

    // 구역은 새 쪽에서 시작한다.
    let new_page = |pages: &mut Vec<PageLayout>| {
        pages.push(PageLayout { width: page_width, height: page_height, lines: Vec::new() })
    };
    new_page(pages);

    let mut prev_v: Option<i32> = None;
    let mut missing: Vec<String> = Vec::new();
    let mut renderable = 0usize;

    for (para_idx, para) in section.paragraphs.iter().enumerate() {
        let location = format!("s{section_idx}/p{para_idx}");

        // 표 = W2 거부 (다쪽 표 뒤 문단의 page ordinal 을 v 만으로 알 수 없다 — Codex H1).
        if para.runs.iter().any(|r| matches!(r.content, RunContent::Table(_))) {
            return Err(PdfError::UnsupportedContent { kind: "table", location });
        }

        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            missing.push(location.clone());
            warnings.push(PdfWarning::ParagraphSkipped { location });
            continue;
        };
        renderable += 1;

        // fail-closed 가드 (독립 리뷰 열린질문): 첫 텍스트 run 이전에 비텍스트
        // run(컨트롤·이미지)이 있으면 textpos 좌표를 신뢰할 수 없다 — HWPX
        // 디코더의 선행 컨트롤 정규화는 secPr·ctrl 만 차감하므로(수식·그림
        // 등은 스트림 유닛 미차감) 무음 오절단 대신 깨끗하게 거부한다.
        let first_text = para.runs.iter().position(|r| r.content.plain_text().is_some());
        if let Some(ft) = first_text {
            if para.runs[..ft].iter().any(|r| r.content.plain_text().is_none()) {
                return Err(PdfError::InvalidCache {
                    detail: format!(
                        "{location}: leading non-text run before first text run — textpos \
                         normalization does not cover this control kind (W2 scope)"
                    ),
                });
            }
        }

        // regular-only 게이트 (Codex H2): bold/italic run 은 경고.
        for run in &para.runs {
            let bold = input.styles.char_bold(run.char_shape_id).unwrap_or(false);
            let italic = input.styles.char_italic(run.char_shape_id).unwrap_or(false);
            if bold || italic {
                warnings.push(PdfWarning::NonRegularRun { location: location.clone() });
                break;
            }
        }

        let text = para.text_content();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        validate_textpos(cache, utf16.len(), &location)?;

        // run 별 UTF-16 구간 (문자 스타일 매핑용).
        let run_spans = run_utf16_spans(para, warnings, &location);
        let alignment = input.styles.para_alignment(para.para_shape_id).unwrap_or(Alignment::Left);

        let line_count = cache.lines.len();
        for (line_idx, seg) in cache.lines.iter().enumerate() {
            // 쪽분할 (규칙 §2): 첫 줄 제외 v==0 ∨ v<prev → 새 쪽.
            let v = seg.vertpos;
            if let Some(prev) = prev_v {
                if v == 0 || v < prev {
                    new_page(pages);
                }
            }
            prev_v = Some(v);

            let start = seg.textpos as usize;
            let end =
                cache.lines.get(line_idx + 1).map_or(utf16.len(), |next| next.textpos as usize);
            let runs = slice_line_runs(&utf16, &run_spans, start, end);

            let page = pages.last_mut().expect("page pushed at section start");
            page.lines.push(LaidLine {
                location: format!("{location}/l{line_idx}"),
                runs,
                baseline_y: body_top + v + seg.baseline,
                line_box: LineBox { horzpos: body_left + seg.horzpos, horzsize: seg.horzsize },
                is_last_line: line_idx + 1 == line_count,
                alignment,
            });
        }
    }

    if renderable == 0 {
        return Err(PdfError::NoRenderableCache { section: section_idx });
    }
    if !missing.is_empty() && options.partial_cache == PartialCachePolicy::Reject {
        return Err(PdfError::MissingLayoutCache {
            count: missing.len(),
            first: missing.remove(0),
        });
    }
    Ok(())
}

/// textpos 형식 정합: 단조증가 + 텍스트 길이 이내 (규칙 §3 — 스테일 방어 아님).
fn validate_textpos(
    cache: &hwpforge_core::layout::LayoutCache,
    utf16_len: usize,
    location: &str,
) -> PdfResult<()> {
    let mut prev = None;
    for seg in &cache.lines {
        let tp = seg.textpos as usize;
        if tp > utf16_len {
            return Err(PdfError::InvalidCache {
                detail: format!("{location}: textpos {tp} > text length {utf16_len} (UTF-16)"),
            });
        }
        if let Some(p) = prev {
            if tp < p {
                return Err(PdfError::InvalidCache {
                    detail: format!("{location}: textpos not monotonic ({tp} < {p})"),
                });
            }
        }
        prev = Some(tp);
    }
    Ok(())
}

/// run 별 UTF-16 [start, end) 구간과 문자 스타일. 텍스트가 아닌 run
/// (컨트롤·이미지)은 0 폭으로 취급하고 경고를 남긴다 (W5 전 미지원).
fn run_utf16_spans(
    para: &hwpforge_core::paragraph::Paragraph,
    warnings: &mut Vec<PdfWarning>,
    location: &str,
) -> Vec<(usize, usize, CharShapeIndex)> {
    let mut spans = Vec::with_capacity(para.runs.len());
    let mut pos = 0usize;
    for run in &para.runs {
        match run.content.plain_text() {
            Some(text) => {
                let len = text.encode_utf16().count();
                spans.push((pos, pos + len, run.char_shape_id));
                pos += len;
            }
            None => {
                // 표는 이미 거부됐고, 여기 오는 것은 컨트롤/이미지 — 흐름 폭 0.
                warnings.push(PdfWarning::NonTextRunDropped { location: location.to_string() });
            }
        }
    }
    spans
}

/// [start, end) UTF-16 구간을 run 경계로 잘라 텍스트 구간들을 만든다.
fn slice_line_runs(
    utf16: &[u16],
    run_spans: &[(usize, usize, CharShapeIndex)],
    start: usize,
    end: usize,
) -> Vec<LineRun> {
    let mut out = Vec::new();
    for &(rs, re, cs) in run_spans {
        let s = start.max(rs);
        let e = end.min(re);
        if s >= e {
            continue;
        }
        let text = String::from_utf16_lossy(&utf16[s..e]);
        out.push(LineRun { text, char_shape: cs });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::document::Document;
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::StyleLookup;
    use hwpforge_foundation::{HwpUnit, ParaShapeIndex};

    struct NoopStyles;
    impl StyleLookup for NoopStyles {}

    fn seg(textpos: u32, vertpos: i32) -> LineSeg {
        LineSeg {
            textpos,
            vertpos,
            vertsize: 1000,
            textheight: 1000,
            baseline: 850,
            spacing: 600,
            horzpos: 0,
            horzsize: 48188,
            flags: 0,
        }
    }

    fn para_with_cache(text: &str, lines: Vec<LineSeg>) -> Paragraph {
        let mut p = Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(lines));
        p
    }

    fn doc_of(paragraphs: Vec<Paragraph>) -> Document<hwpforge_core::document::Validated> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
        doc.validate().expect("validate")
    }

    fn replay(
        doc: &Document<hwpforge_core::document::Validated>,
        options: &PdfOptions,
    ) -> PdfResult<ReplayLayout> {
        replay_layout(&PdfInput { document: doc, styles: &NoopStyles }, options)
    }

    // A4 기본 여백 (PageSettings::a4()): top/left 관측 기반이 아니라 코드 값 사용.
    fn a4_body_top() -> i32 {
        let ps = PageSettings::a4();
        ps.margin_top.as_i32() + ps.header_margin.as_i32()
    }

    #[test]
    fn baseline_is_body_top_plus_v_plus_baseline() {
        // 규칙 §1: baseline = body_top + v + lineseg.baseline (전부 정수합).
        let doc = doc_of(vec![para_with_cache("본문", vec![seg(0, 0), seg(2, 1600)])]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 1);
        let lines = &layout.pages[0].lines;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].baseline_y, a4_body_top() + 850);
        assert_eq!(lines[1].baseline_y, a4_body_top() + 1600 + 850);
        assert!(!lines[0].is_last_line && lines[1].is_last_line);
    }

    #[test]
    fn v_reset_starts_new_page() {
        // 규칙 §2: v < prev → 새 쪽 (headerfooter 42줄/쪽 실측 패턴의 최소형).
        let doc = doc_of(vec![
            para_with_cache("첫쪽", vec![seg(0, 0), seg(2, 1600)]),
            para_with_cache("둘째쪽", vec![seg(0, 0)]), // v 리셋
        ]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 2);
        assert_eq!(layout.pages[0].lines.len(), 2);
        assert_eq!(layout.pages[1].lines.len(), 1);
    }

    #[test]
    fn textpos_splits_line_text_by_utf16() {
        // 규칙 §1: 줄분할 = textpos (UTF-16), 뒤따르는 공백은 앞 줄 소속.
        let doc = doc_of(vec![para_with_cache("가나다 라마바", vec![seg(0, 0), seg(4, 1600)])]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let lines = &layout.pages[0].lines;
        assert_eq!(lines[0].runs[0].text, "가나다 ");
        assert_eq!(lines[1].runs[0].text, "라마바");
    }

    #[test]
    fn table_document_is_rejected_in_w2() {
        // Codex H1: 다쪽 표 page-ordinal 미보장 → W2 는 표 자체를 거부.
        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("셀", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_pt(100.0).unwrap(),
        );
        let mut host = para_with_cache("표 호스트", vec![seg(0, 0)]);
        host.add_run(Run::table(
            Table::new(vec![TableRow::new(vec![cell])]),
            CharShapeIndex::new(0),
        ));
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::UnsupportedContent { kind: "table", .. }));
    }

    #[test]
    fn missing_cache_warns_and_skips_by_default() {
        // 계획 §3: 기본 WarnAndSkip — 에픽 "fill/set-cell 지원(경고)" 계약.
        let mut no_cache = Paragraph::with_runs(
            vec![Run::text("편집됨", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        no_cache.layout_cache = None;
        let doc = doc_of(vec![para_with_cache("정상", vec![seg(0, 0)]), no_cache]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages[0].lines.len(), 1);
        assert!(layout.warnings.iter().any(
            |w| matches!(w, PdfWarning::ParagraphSkipped { location } if location == "s0/p1")
        ));
    }

    #[test]
    fn missing_cache_rejects_under_reject_policy() {
        let mut no_cache = Paragraph::with_runs(
            vec![Run::text("편집됨", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        no_cache.layout_cache = None;
        let doc = doc_of(vec![para_with_cache("정상", vec![seg(0, 0)]), no_cache]);
        let options =
            PdfOptions { partial_cache: PartialCachePolicy::Reject, ..Default::default() };
        let err = replay(&doc, &options).unwrap_err();
        assert!(matches!(err, PdfError::MissingLayoutCache { count: 1, .. }));
    }

    #[test]
    fn section_without_any_renderable_cache_is_rejected() {
        // 계획 §3 하한: layout_carry scan-실패 fail-open 산출물·순수 생성물 방어.
        let doc = doc_of(vec![Paragraph::with_runs(
            vec![Run::text("무캐시", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::NoRenderableCache { section: 0 }));
    }

    #[test]
    fn non_monotonic_or_overflowing_textpos_is_invalid_cache() {
        let doc = doc_of(vec![para_with_cache("가나", vec![seg(0, 0), seg(9, 1600)])]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }));

        let doc2 = doc_of(vec![para_with_cache("가나다라", vec![seg(3, 0), seg(1, 1600)])]);
        let err2 = replay(&doc2, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err2, PdfError::InvalidCache { .. }));
    }

    #[test]
    fn leading_non_text_run_before_text_is_rejected() {
        // 독립 리뷰 열린질문 상환: 디코더의 textpos 정규화가 다루지 않는
        // 선행 컨트롤(각주·그림 등)은 무음 오절단 대신 fail-closed 거부.
        let mut para = para_with_cache("본문", vec![seg(0, 0)]);
        para.runs.insert(
            0,
            Run::control(
                hwpforge_core::control::Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text("각주", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]),
                CharShapeIndex::new(0),
            ),
        );
        let doc = doc_of(vec![para]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
    }

    #[test]
    fn trailing_non_text_run_is_dropped_with_warning() {
        // 텍스트 뒤의 컨트롤 run 은 경고와 함께 생략 (문단 자체는 렌더).
        let mut para = para_with_cache("본문", vec![seg(0, 0)]);
        para.add_run(Run::control(
            hwpforge_core::control::Control::footnote(vec![Paragraph::with_runs(
                vec![Run::text("각주", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )]),
            CharShapeIndex::new(0),
        ));
        let doc = doc_of(vec![para]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages[0].lines.len(), 1);
        assert!(layout.warnings.iter().any(|w| matches!(w, PdfWarning::NonTextRunDropped { .. })));
    }

    #[test]
    fn line_box_includes_body_left_offset() {
        let doc = doc_of(vec![para_with_cache("가", vec![seg(0, 0)])]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let ps = PageSettings::a4();
        assert_eq!(layout.pages[0].lines[0].line_box.horzpos, ps.margin_left.as_i32());
        assert_eq!(layout.pages[0].lines[0].line_box.horzsize, 48188);
    }
}
