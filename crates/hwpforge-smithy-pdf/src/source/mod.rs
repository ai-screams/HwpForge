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
use hwpforge_foundation::{Alignment, ApplyPageType, CharShapeIndex, Color};

use crate::text::align::LineBox;
use crate::{PartialCachePolicy, PdfError, PdfInput, PdfOptions, PdfResult, PdfWarning};

mod table;

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

/// 셀 배경 사각형 (HWPUNIT, 쪽 좌상단 원점).
#[derive(Debug, Clone, PartialEq)]
pub struct LaidRect {
    /// 위치 보고용 경로.
    pub location: String,
    /// 좌변 x.
    pub x: i32,
    /// 상단 y.
    pub y: i32,
    /// 폭.
    pub width: i32,
    /// 높이.
    pub height: i32,
    /// 채움색.
    pub color: Color,
}

/// 괘선 선분 (HWPUNIT, 쪽 좌상단 원점 — 경계선 중앙 기준).
#[derive(Debug, Clone, PartialEq)]
pub struct LaidBorder {
    /// 위치 보고용 경로.
    pub location: String,
    /// 시작점 (x, y).
    pub from: (i32, i32),
    /// 끝점 (x, y).
    pub to: (i32, i32),
    /// 선 굵기.
    pub width: i32,
    /// 선 색.
    pub color: Color,
}

/// 합성 쪽번호 (캐시 재생이 아니라 §8c 실측 규칙으로 합성).
///
/// 가로 = 페이지 폭 중앙(장식 포함 자연폭 기준 — 여백 무관, rules-pagenum
/// Δ0.08pt) · 세로 = baseline 을 `anchor_bottom − hhea descent × size` 에
/// 앵커 (렌더 층이 폰트 메트릭으로 확정 — 한컴 콘텐트 스트림 실측 783.00pt,
/// 모델 오차 ≤0.16pt).
#[derive(Debug, Clone, PartialEq)]
pub struct LaidPageNumber {
    /// 위치 보고용 경로.
    pub location: String,
    /// 표시 문자열 (장식 포함 — 예: `- 1 -`).
    pub text: String,
    /// 문자 스타일 (전용 "쪽 번호" 스타일의 charPr — 부재 시 문서 기본 0).
    pub char_shape: CharShapeIndex,
    /// em 하단 앵커 (HWPUNIT — `H − margin.bottom`, §8c).
    pub anchor_bottom: i32,
}

/// 한 쪽의 배치 결과.
///
/// z-order 계약: `rects`(셀 배경) → `borders`(괘선) → `lines`(글리프) 순으로
/// 그린다 (각 Vec 내부는 문서 순서). `page_number` 는 글리프와 같은 층이다
/// (겹칠 본문이 없는 밴드 밖 영역).
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// 쪽 폭 (HWPUNIT).
    pub width: i32,
    /// 쪽 높이 (HWPUNIT).
    pub height: i32,
    /// 셀 배경 사각형들.
    pub rects: Vec<LaidRect>,
    /// 괘선 선분들.
    pub borders: Vec<LaidBorder>,
    /// 줄들 (문서 순서).
    pub lines: Vec<LaidLine>,
    /// 합성 쪽번호 (지원 position/포맷일 때만).
    pub page_number: Option<LaidPageNumber>,
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

    // 표시 번호는 구역을 넘어 이어진다 (beginNum.page 0 = 이전 구역 계속).
    let mut counter = DisplayCounter::new();
    for (section_idx, section) in input.document.sections().iter().enumerate() {
        // pageStartsOn != BOTH 는 미실측 페이지네이션 속성 (한컴이 빈 쪽을
        // 삽입할 수 있음) — 머리말/꼬리말·쪽번호 유무와 무관하게 BOTH 거동으로
        // 재생하고 무조건 표면화한다 (§9 b0 — 독립 리뷰 M2 상환).
        if section
            .begin_num
            .as_ref()
            .is_some_and(|b| b.page_starts_on != hwpforge_core::section::PageStartsOn::Both)
        {
            warnings.push(PdfWarning::PageStartsOnFallback { section: section_idx });
        }
        let first_page = pages.len();
        let mut events: Vec<PageNumberEvent> = Vec::new();
        let mut hides: Vec<PageHideEvent> = Vec::new();
        replay_section(
            input,
            options,
            section,
            section_idx,
            &mut pages,
            &mut events,
            &mut hides,
            &mut warnings,
        )?;
        overlay_headers_footers(
            input,
            options,
            section,
            section_idx,
            first_page,
            &hides,
            &mut pages,
            &mut warnings,
        )?;
        counter.enter_section(section.begin_num.as_ref());
        // W2: 쪽 배정 직전에 그 쪽의 재시작 이벤트를 문서 순서로 적용한다
        // (같은 쪽 다중 = last-wins). F1 실측: 컨트롤이 놓인 쪽부터 새 번호
        // (1쪽 앵커 = `7,8`, 2쪽 앵커 = `1,7`).
        let numbers: Vec<u32> = (first_page..pages.len())
            .map(|page_idx| {
                for ev in events.iter().filter(|e| e.page == page_idx) {
                    counter.restart(ev.number);
                }
                counter.assign_page()
            })
            .collect();
        emit_page_numbers(
            input,
            section,
            section_idx,
            first_page,
            &numbers,
            &hides,
            &mut pages,
            &mut warnings,
        );
    }
    Ok(ReplayLayout { pages, warnings })
}

/// 쪽번호 표시 카운터 (W1 reducer).
///
/// 구역 시작값(`BeginNum`)과 쪽 단위 이벤트(중간 재시작 `nwno` W2, 감춤
/// `pghd` W3)가 **같은 카운터 상태**를 문서 순서로 변경한다 — 단일
/// `display_start + offset` 산술로는 중간 재시작을 표현할 수 없어 쪽
/// 단위 배정으로 바꾼다 (이 wave 에서는 기존 거동과 동일 — 골든 잠금).
#[derive(Debug)]
struct DisplayCounter {
    next: u32,
}

impl DisplayCounter {
    fn new() -> Self {
        Self { next: 1 }
    }

    /// 구역 진입 — `beginNum.page` `n>0` = 재시작, `0`/부재 = 이어서
    /// (OWPML §10.6.2).
    fn enter_section(&mut self, begin_num: Option<&hwpforge_core::section::BeginNum>) {
        if let Some(n) = begin_num.map(|b| b.page).filter(|&n| n > 0) {
            self.next = n;
        }
    }

    /// 물리 쪽 하나에 표시 번호를 배정하고 전진한다. 감춤(hide_first 등)은
    /// **표시만** 생략하고 전진은 막지 않는다 (F2-① PDF 실측: `1, _, 3`).
    fn assign_page(&mut self) -> u32 {
        let n = self.next;
        self.next += 1;
        n
    }

    /// `nwno` 재시작 이벤트 — 해당 쪽 번호 배정 **직전에** 카운터를 덮어쓴다.
    fn restart(&mut self, n: u32) {
        self.next = n;
    }
}

/// `nwno` 쪽번호 재시작 이벤트 (W2) — 컨트롤이 놓인 줄이 배치된 **물리 쪽**
/// (전역 인덱스)에 앵커된다. 문서 순서로 수집되어 같은 쪽 다중 이벤트는
/// last-wins 로 적용된다.
#[derive(Debug, Clone, Copy)]
struct PageNumberEvent {
    /// 전역 물리 쪽 인덱스 (`pages` 벡터 기준).
    page: usize,
    /// 새 표시 번호.
    number: u32,
}

/// `pghd` 감춤 이벤트 (W3) — 컨트롤이 놓인 물리 쪽의 렌더 대상 3종
/// (쪽번호/머리말/꼬리말)만 담는다. 바탕쪽/테두리/배경은 렌더 자체가 없어
/// carry-only. 같은 쪽 다중 이벤트는 적용부에서 OR 병합된다 (F2 계약:
/// 감춤은 카운터 전진을 막지 않는다 — `1, _, 3`).
#[derive(Debug, Clone, Copy)]
struct PageHideEvent {
    /// 전역 물리 쪽 인덱스 (`pages` 벡터 기준).
    page: usize,
    /// 쪽번호 표시 억제.
    page_num: bool,
    /// 머리말 밴드 억제.
    header: bool,
    /// 꼬리말 밴드 억제.
    footer: bool,
}

/// 렌더 replay 가 자체 소비하는 0폭 marker 컨트롤인지 (textpos 무영향 —
/// 한컴 HWPX 실측: `<hp:ctrl>` 은 linesegarray textpos 를 소비하지 않는다).
/// 선행 비텍스트 fail-closed 가드와 `NonTextRunDropped` 경고에서 제외된다.
fn is_replay_consumed_marker(content: &RunContent) -> bool {
    matches!(content, RunContent::Control(c)
    if matches!(
        **c,
        hwpforge_core::control::Control::NewNumber { .. }
            | hwpforge_core::control::Control::PageHiding { .. }
    ))
}

/// 쪽번호 합성 (§8c 실측 — BOTTOM_CENTER + DIGIT 만, 그 외 = 경고+생략).
///
/// 표시 문자열은 sideChar 장식을 어간 공백으로 감싼다 (`- 1 -` — 한컴 PDF
/// 실측 어간 5.08pt = 10pt 공백 폭). 스타일은 전용 "쪽 번호"(Page Number)
/// CHAR 스타일의 charPr — 부재 시 문서 기본 charPr(0) + 경고.
#[allow(clippy::too_many_arguments)]
fn emit_page_numbers(
    input: &PdfInput<'_>,
    section: &Section,
    section_idx: usize,
    first_page: usize,
    numbers: &[u32],
    hides: &[PageHideEvent],
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) {
    use hwpforge_foundation::{NumberFormatType, PageNumberPosition};
    let Some(pn) = section.page_number.as_ref() else { return };
    if pn.position == PageNumberPosition::None {
        return; // 표시 안 함 — 경고 아님.
    }
    if pn.position != PageNumberPosition::BottomCenter {
        warnings.push(PdfWarning::PageNumberSkipped { section: section_idx, what: "position" });
        return;
    }
    if pn.number_format != NumberFormatType::Digit {
        warnings.push(PdfWarning::PageNumberSkipped { section: section_idx, what: "format" });
        return;
    }
    let char_shape = input
        .styles
        .char_style_shape("쪽 번호")
        .or_else(|| input.styles.char_style_shape("Page Number"))
        .unwrap_or_else(|| {
            warnings.push(PdfWarning::PageNumberStyleFallback { section: section_idx });
            CharShapeIndex::new(0)
        });
    let ps = &section.page_settings;
    let anchor_bottom = ps.height.as_i32() - ps.margin_bottom.as_i32();
    let hide_first = section.visibility.as_ref().is_some_and(|v| v.hide_first_page_num);
    for (offset, page) in pages[first_page..].iter_mut().enumerate() {
        if hide_first && offset == 0 {
            continue; // 번호 자체는 진행하고 표시만 생략.
        }
        // W3: pghd 감춤 쪽 — 표시만 생략 (F2-① 실측 `1, _, 3`: 카운터는
        // reducer 가 이미 전진시켰다). secd 첫쪽 감춤과 자연 OR.
        if hides.iter().any(|h| h.page == first_page + offset && h.page_num) {
            continue;
        }
        let n = numbers[offset];
        let text = if pn.decoration.is_empty() {
            n.to_string()
        } else {
            format!("{d} {n} {d}", d = pn.decoration)
        };
        page.page_number = Some(LaidPageNumber {
            location: format!("s{section_idx}/pagenum/{n}"),
            text,
            char_shape,
            anchor_bottom,
        });
    }
}

/// 머리말/꼬리말 오버레이 (규칙 §5 R6 + W5 §8 실측).
///
/// 본문 쪽이 확정된 **뒤** 섹션의 각 쪽에 밴드-상대 캐시를 재생한다. 밴드는
/// 앵커일 뿐이다: 클립도, 본문 리플로도 없다 (rules-header-overflow 실측 —
/// `v+vertsize` 가 밴드를 넘으면 [`PdfWarning::BandOverflow`] 만 표면화).
/// ODD/EVEN 선택은 물리 쪽 서수(1-기반) 홀짝이다 (odd-even fixture 실측).
#[allow(clippy::too_many_arguments)]
fn overlay_headers_footers(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    section: &Section,
    section_idx: usize,
    first_page: usize,
    hides: &[PageHideEvent],
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    if section.headers.is_empty() && section.footers.is_empty() {
        return Ok(());
    }
    let ps = &section.page_settings;
    let page_height = ps.height.as_i32();
    let body_left = ps.margin_left.as_i32();

    // 다중 섹션 + parity 는 미실측 (물리/구역 서수 어느 쪽 홀짝인지) — 거부.
    if section_idx > 0 {
        for (kind, list) in [("header", &section.headers), ("footer", &section.footers)] {
            if list.iter().any(|h| h.apply_page_type != ApplyPageType::Both) {
                return Err(PdfError::AmbiguousHeaderFooter {
                    kind,
                    detail: format!(
                        "s{section_idx}: ODD/EVEN parity in a non-first section is unmeasured \
                         (physical vs section ordinal)"
                    ),
                });
            }
        }
    }

    let vis = section.visibility.as_ref();
    let bands = [
        (
            "header",
            "h",
            &section.headers,
            ps.margin_top.as_i32(),
            ps.header_margin.as_i32(),
            vis.is_some_and(|v| v.hide_first_header),
        ),
        (
            "footer",
            "f",
            &section.footers,
            page_height - ps.margin_bottom.as_i32() - ps.footer_margin.as_i32(),
            ps.footer_margin.as_i32(),
            vis.is_some_and(|v| v.hide_first_footer),
        ),
    ];
    for (kind, tag, list, band_top, band_height, hide_first) in bands {
        // 같은 항목이 매 쪽 반복되므로 초과/정렬 경고는 항목당 1회.
        let mut warned_overflow = vec![false; list.len()];
        let mut warned_valign = vec![false; list.len()];
        for (offset, page) in pages[first_page..].iter_mut().enumerate() {
            let physical = first_page + offset + 1;
            let odd = physical % 2 == 1;
            let matched: Vec<usize> = list
                .iter()
                .enumerate()
                .filter(|(_, h)| match h.apply_page_type {
                    ApplyPageType::Both => true,
                    ApplyPageType::Odd => odd,
                    ApplyPageType::Even => !odd,
                    // non_exhaustive 미래 variant — 디코더가 미지값을 BOTH 로
                    // 경고 폴백하므로 여기 도달 불가. 도달하면 미매치 처리.
                    _ => false,
                })
                .map(|(i, _)| i)
                .collect();
            if matched.len() > 1 {
                let kinds: Vec<ApplyPageType> =
                    matched.iter().map(|&i| list[i].apply_page_type).collect();
                return Err(PdfError::AmbiguousHeaderFooter {
                    kind,
                    detail: format!(
                        "s{section_idx} page {physical}: {} candidates match ({kinds:?}) — \
                         Hancom priority unmeasured",
                        matched.len()
                    ),
                });
            }
            let Some(&hf_idx) = matched.first() else { continue };
            if hide_first && offset == 0 {
                continue;
            }
            // W3: pghd 감춤 쪽 — 해당 밴드만 억제 (F2-③ 실측: 2쪽 머리말
            // 소거, 1·3쪽 유지). secd 첫쪽 감춤과 자연 OR.
            if hides.iter().any(|h| {
                h.page == first_page + offset
                    && ((kind == "header" && h.header) || (kind == "footer" && h.footer))
            }) {
                continue;
            }
            replay_band_item(
                input,
                options,
                &list[hf_idx],
                BandGeom { kind, tag, hf_idx, section_idx, band_top, band_height, body_left },
                page,
                warnings,
                &mut warned_overflow[hf_idx],
                &mut warned_valign[hf_idx],
            )?;
        }
    }
    Ok(())
}

/// 밴드 재생 기하 (인자 다발 — [`replay_band_item`] 전용).
struct BandGeom {
    kind: &'static str,
    tag: &'static str,
    hf_idx: usize,
    section_idx: usize,
    band_top: i32,
    band_height: i32,
    body_left: i32,
}

/// 머리말/꼬리말 한 항목을 한 쪽에 재생한다 (본문과 동일한 lineseg 재생,
/// 원점만 밴드 top — rules-header-multi 실측: 다문단 = v 누적, 특수 로직 없음).
#[allow(clippy::too_many_arguments)]
fn replay_band_item(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    hf: &hwpforge_core::section::HeaderFooter,
    geom: BandGeom,
    page: &mut PageLayout,
    warnings: &mut Vec<PdfWarning>,
    warned_overflow: &mut bool,
    warned_valign: &mut bool,
) -> PdfResult<()> {
    let BandGeom { kind, tag, hf_idx, section_idx, band_top, band_height, body_left } = geom;
    let item_location = format!("s{section_idx}/{tag}{hf_idx}");
    if hf.vert_align != hwpforge_foundation::VerticalAlign::Top && !*warned_valign {
        *warned_valign = true;
        warnings.push(PdfWarning::VertAlignFallback { location: item_location.clone() });
    }
    for (para_idx, para) in hf.paragraphs.iter().enumerate() {
        let location = format!("{item_location}/p{para_idx}");
        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            if options.partial_cache == PartialCachePolicy::Reject {
                return Err(PdfError::MissingLayoutCache { count: 1, first: location });
            }
            warnings.push(PdfWarning::ParagraphSkipped { location });
            continue;
        };
        // fail-closed 가드 (본문 경로와 동일 — 독립 리뷰 M1 상환): 첫 텍스트
        // run 이전의 비텍스트 run(로고 이미지 등)은 textpos 좌표를 신뢰할 수
        // 없게 만든다 — 무음 오절단 대신 거부.
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

        let text = para.text_content();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        validate_textpos(cache, utf16.len(), &location)?;
        let run_spans = run_utf16_spans(para, warnings, &location);
        let alignment = input.styles.para_alignment(para.para_shape_id).unwrap_or(Alignment::Left);
        let line_count = cache.lines.len();
        for (line_idx, seg) in cache.lines.iter().enumerate() {
            if seg.vertpos + seg.vertsize > band_height && !*warned_overflow {
                *warned_overflow = true;
                warnings.push(PdfWarning::BandOverflow { kind, location: location.clone() });
            }
            let start = seg.textpos as usize;
            let end =
                cache.lines.get(line_idx + 1).map_or(utf16.len(), |next| next.textpos as usize);
            let runs = slice_line_runs(&utf16, &run_spans, start, end);
            page.lines.push(LaidLine {
                location: format!("{location}/l{line_idx}"),
                runs,
                baseline_y: band_top + seg.vertpos + seg.baseline,
                line_box: LineBox { horzpos: body_left + seg.horzpos, horzsize: seg.horzsize },
                is_last_line: line_idx + 1 == line_count,
                alignment,
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replay_section(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    section: &Section,
    section_idx: usize,
    pages: &mut Vec<PageLayout>,
    events: &mut Vec<PageNumberEvent>,
    hides: &mut Vec<PageHideEvent>,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    let ps = &section.page_settings;
    let page_width = ps.width.as_i32();
    let page_height = ps.height.as_i32();
    // body 원점 (규칙 §0): top = margin_top + header_margin, left = margin_left.
    let body_top = ps.margin_top.as_i32() + ps.header_margin.as_i32();
    let body_left = ps.margin_left.as_i32();
    let body_height =
        page_height - body_top - ps.margin_bottom.as_i32() - ps.footer_margin.as_i32();

    // 구역은 새 쪽에서 시작한다.
    let new_page = |pages: &mut Vec<PageLayout>| {
        pages.push(PageLayout {
            width: page_width,
            height: page_height,
            rects: Vec::new(),
            borders: Vec::new(),
            lines: Vec::new(),
            page_number: None,
        })
    };
    new_page(pages);

    let mut prev_v: Option<i32> = None;
    let mut missing: Vec<String> = Vec::new();
    let mut renderable = 0usize;
    // 직전 표의 "다음 문단 v" 캐시 앵커 검산 (게이트2 C1 — 계산 페이지네이션
    // 을 캐시가 반증하면 fatal).
    let mut pending_table_anchor: Option<(i32, i32, String)> = None;

    for (para_idx, para) in section.paragraphs.iter().enumerate() {
        let location = format!("s{section_idx}/p{para_idx}");

        // W3: 명시적 쪽 나누기 (ParaHeader divide_sort bit2 / HWPX
        // pageBreak="1") — F2 실측: 한컴은 쪽나눔 문단의 lineseg v 를
        // 리셋하지 않아 (연속 600) `v==0 ∨ v<prev` 규칙이 잡지 못한다.
        // 플래그가 유일한 신호. 현재 쪽이 비어 있으면 이중 쪽나눔 방지.
        if para.page_break
            && pages.last().is_some_and(|p| {
                !p.lines.is_empty() || !p.rects.is_empty() || !p.borders.is_empty()
            })
        {
            new_page(pages);
            prev_v = None;
            // 새 쪽 — 표 앵커 검산은 쪽-상대 v 비교라 무효.
            pending_table_anchor = None;
        }

        // ── 표 host 문단 (W3c — 검증된 프로파일 재생) ─────────────
        let table_count =
            para.runs.iter().filter(|r| matches!(r.content, RunContent::Table(_))).count();
        if table_count > 0 {
            if table_count > 1 {
                return Err(PdfError::UnsupportedContent {
                    kind: "multiple tables in one paragraph",
                    location,
                });
            }
            let mixed_text = para
                .runs
                .iter()
                .filter_map(|r| r.content.plain_text())
                .any(|t| !t.trim().is_empty());
            if mixed_text {
                return Err(PdfError::UnsupportedContent {
                    kind: "table mixed with visible text",
                    location,
                });
            }
            let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
                return Err(PdfError::MissingLayoutCache { count: 1, first: location });
            };
            let host = &cache.lines[0];
            let host_v = host.vertpos;
            let broke = prev_v.is_some_and(|p| host_v == 0 || host_v < p);
            if broke {
                new_page(pages);
            }
            check_table_anchor(&mut pending_table_anchor, host_v, broke)?;

            let table = para
                .runs
                .iter()
                .find_map(|r| match &r.content {
                    RunContent::Table(t) => Some(t.as_ref()),
                    _ => None,
                })
                .ok_or_else(|| PdfError::InternalInvariant {
                    detail: format!("{location}: table run vanished after count"),
                })?;
            let geom = table::SectionGeom { body_top, body_left, body_height };
            let outcome = table::replay_table(
                input,
                table,
                &location,
                &geom,
                host_v,
                host.horzpos,
                &new_page,
                pages,
                warnings,
            )?;
            // 흐름 앵커 (over-split 수정 — 두 모델 실측):
            // ① 글자취급(인라인) 표 = host lineseg 가 표를 담아 vertsize ≈
            //   표높이 — 흐름 = host_v + vertsize (기재부 corpus 실측: 계산치
            //   의 outMargin 합산이 실제 간격보다 +190HU 과대 → 다음 문단을
            //   새 쪽으로 오판했다).
            // ② 앵커형 표 = host lineseg 는 순수 줄높이(vertsize ≪ 표높이) —
            //   흐름 = 계산치 host_v+om+Σ행높이+om (rules-table 실측 정확 일치).
            // 판별 = host.vertsize ≥ 계산 총높이. 분할 표는 항상 계산 흐름
            // (캐시가 연속 조각을 표현 못 함 — prev_v 를 host v 로 두면 후속
            // 문단의 작아진 v 가 "새 쪽"으로 오판되는 기존 근거 유지).
            let flow_next = if !outcome.split && host.vertsize >= outcome.total_height {
                host_v + host.vertsize
            } else {
                outcome.expected_next_v
            };
            pending_table_anchor = Some((flow_next, outcome.anchor_slack, location.clone()));
            prev_v = Some(flow_next);
            renderable += 1;
            continue;
        }

        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            missing.push(location.clone());
            warnings.push(PdfWarning::ParagraphSkipped { location });
            // 앵커는 표 "바로 다음" 문단에만 결합한다 — 그 문단이 스킵되면
            // 이후 문단의 v 는 앵커 식과 무관하므로 폐기 (오발 fatal 방지).
            pending_table_anchor = None;
            continue;
        };
        renderable += 1;

        // 표 뒤 첫 renderable 문단이면 캐시 앵커 검산 (같은 쪽에서만 유효).
        let first_v = cache.lines[0].vertpos;
        let first_breaks = prev_v.is_some_and(|p| first_v == 0 || first_v < p);
        check_table_anchor(&mut pending_table_anchor, first_v, first_breaks)?;

        // fail-closed 가드 (독립 리뷰 열린질문): 첫 텍스트 run 이전에 비텍스트
        // run(컨트롤·이미지)이 있으면 textpos 좌표를 신뢰할 수 없다 — HWPX
        // 디코더의 선행 컨트롤 정규화는 secPr·ctrl 만 차감하므로(수식·그림
        // 등은 스트림 유닛 미차감) 무음 오절단 대신 깨끗하게 거부한다.
        // W2 예외: replay 가 자체 소비하는 0폭 marker(nwno)는 textpos 를
        // 소비하지 않는다 (F1b 한컴 실측: ctrl 선행 + linesegarray textpos=0).
        let first_text = para.runs.iter().position(|r| r.content.plain_text().is_some());
        if let Some(ft) = first_text {
            if para.runs[..ft]
                .iter()
                .any(|r| r.content.plain_text().is_none() && !is_replay_consumed_marker(&r.content))
            {
                return Err(PdfError::InvalidCache {
                    detail: format!(
                        "{location}: leading non-text run before first text run — textpos \
                         normalization does not cover this control kind (W2 scope)"
                    ),
                });
            }
        }

        // W2/W3: 이 문단의 nwno(쪽 종류) 재시작·pghd 감춤을 가시 텍스트
        // 오프셋과 함께 수집 — 아래 줄 루프에서 해당 오프셋이 놓이는 물리
        // 쪽에 앵커한다.
        let mut restarts: Vec<(usize, u32)> = Vec::new();
        let mut hide_marks: Vec<(usize, PageHideEvent)> = Vec::new();
        {
            use hwpforge_core::control::Control;
            let mut pos = 0usize;
            for run in &para.runs {
                match run.content.plain_text() {
                    Some(t) => pos += t.encode_utf16().count(),
                    None => {
                        if let RunContent::Control(c) = &run.content {
                            match **c {
                                Control::NewNumber {
                                    kind: hwpforge_core::control::NewNumberKind::Page,
                                    number,
                                } => restarts.push((pos, number)),
                                Control::PageHiding {
                                    hide_header,
                                    hide_footer,
                                    hide_page_num,
                                    ..
                                } => hide_marks.push((
                                    pos,
                                    // page 는 줄 루프에서 확정 — 임시 0.
                                    PageHideEvent {
                                        page: 0,
                                        page_num: hide_page_num,
                                        header: hide_header,
                                        footer: hide_footer,
                                    },
                                )),
                                _ => {}
                            }
                        }
                    }
                }
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

            // W2: 이 줄 텍스트 구간에 놓인 nwno 재시작을 현재 물리 쪽에 앵커.
            // 마지막 줄은 문단 끝 오프셋(컨트롤이 꼬리에 있는 경우)까지 흡수.
            let is_last = line_idx + 1 == line_count;
            for &(_, number) in
                restarts.iter().filter(|&&(off, _)| off >= start && (off < end || is_last))
            {
                events.push(PageNumberEvent { page: pages.len() - 1, number });
            }
            for &(_, hide) in
                hide_marks.iter().filter(|&&(off, _)| off >= start && (off < end || is_last))
            {
                hides.push(PageHideEvent { page: pages.len() - 1, ..hide });
            }

            let page = pages.last_mut().ok_or_else(|| PdfError::InternalInvariant {
                detail: "section replay has no current page".to_string(),
            })?;
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

/// 표 다음 문단의 캐시 v 로 계산 페이지네이션을 검산한다 (게이트2 C1).
///
/// 다음 문단이 새 쪽에서 시작하면 앵커를 걸 수 없어 건너뛴다 (v 는
/// 쪽-상대라 비교 불가) — 그 밖에 불일치 = 계산이 캐시와 모순 = fatal.
fn check_table_anchor(
    pending: &mut Option<(i32, i32, String)>,
    v: i32,
    broke_page: bool,
) -> PdfResult<()> {
    if let Some((expected, slack, table_loc)) = pending.take() {
        if !broke_page && (v < expected || v - expected >= slack) {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{table_loc}: paragraph after table has cached v={v} but computed table \
                     pagination expects {expected} — refusing to output mismatched layout"
                ),
            });
        }
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
                // W2: replay 가 소비하는 marker(nwno)는 드롭이 아니므로 제외.
                if !is_replay_consumed_marker(&run.content) {
                    warnings.push(PdfWarning::NonTextRunDropped { location: location.to_string() });
                }
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

    // ── W5-a 머리말/꼬리말 오버레이 ──────────────────────────────

    use hwpforge_core::section::{BeginNum, HeaderFooter, PageStartsOn, Visibility};
    use hwpforge_foundation::ApplyPageType;

    fn hf_item(apply: ApplyPageType, texts: &[&str]) -> HeaderFooter {
        let paras = texts.iter().map(|t| para_with_cache(t, vec![seg(0, 0)])).collect();
        HeaderFooter::new(paras, apply)
    }

    // ── W1: DisplayCounter reducer (계약 — 감춤은 전진을 막지 않는다) ─────

    #[test]
    fn display_counter_continues_and_restarts_across_sections() {
        let mut c = DisplayCounter::new();
        c.enter_section(None);
        assert_eq!((c.assign_page(), c.assign_page()), (1, 2));
        // page=0 = 이전 구역 이어서 (§10.6.2)
        c.enter_section(Some(&BeginNum { page: 0, ..BeginNum::default() }));
        assert_eq!(c.assign_page(), 3);
        // n>0 = 재시작
        c.enter_section(Some(&BeginNum { page: 10, ..BeginNum::default() }));
        assert_eq!((c.assign_page(), c.assign_page()), (10, 11));
        // 재시작 뒤 다음 구역 이어서
        c.enter_section(None);
        assert_eq!(c.assign_page(), 12);
    }

    fn doc_with_bands(
        body: Vec<Paragraph>,
        headers: Vec<HeaderFooter>,
        footers: Vec<HeaderFooter>,
    ) -> Document<hwpforge_core::document::Validated> {
        let mut section = Section::with_paragraphs(body, PageSettings::a4());
        section.headers = headers;
        section.footers = footers;
        let mut doc = Document::new();
        doc.add_section(section);
        doc.validate().expect("validate")
    }

    /// v==0 리셋으로 n쪽 본문을 만든다.
    fn body_pages(n: usize) -> Vec<Paragraph> {
        (0..n).map(|i| para_with_cache(&format!("본문{i}"), vec![seg(0, 0)])).collect()
    }

    fn band_lines<'a>(page: &'a PageLayout, tag: &str) -> Vec<&'a LaidLine> {
        page.lines.iter().filter(|l| l.location.contains(tag)).collect()
    }

    #[test]
    fn header_baseline_is_band_relative_not_body() {
        // R6: 머리말 원점 = margin.top (body_top 아님 — rules-headerfooter 실측).
        let doc =
            doc_with_bands(body_pages(1), vec![hf_item(ApplyPageType::Both, &["머리말"])], vec![]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let ps = PageSettings::a4();
        let header = band_lines(&layout.pages[0], "/h0/");
        assert_eq!(header.len(), 1);
        assert_eq!(header[0].baseline_y, ps.margin_top.as_i32() + 850);
        assert_ne!(header[0].baseline_y, a4_body_top() + 850, "본문 원점과 달라야 함");
    }

    #[test]
    fn footer_baseline_anchors_at_body_bottom() {
        // R6: 꼬리말 밴드 top = H − margin.bottom − margin.footer.
        let doc =
            doc_with_bands(body_pages(1), vec![], vec![hf_item(ApplyPageType::Both, &["꼬리말"])]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let ps = PageSettings::a4();
        let band_top = ps.height.as_i32() - ps.margin_bottom.as_i32() - ps.footer_margin.as_i32();
        let footer = band_lines(&layout.pages[0], "/f0/");
        assert_eq!(footer.len(), 1);
        assert_eq!(footer[0].baseline_y, band_top + 850);
    }

    #[test]
    fn both_header_repeats_on_every_page() {
        let doc =
            doc_with_bands(body_pages(3), vec![hf_item(ApplyPageType::Both, &["매쪽"])], vec![]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 3);
        for page in &layout.pages {
            assert_eq!(band_lines(page, "/h0/").len(), 1);
        }
    }

    #[test]
    fn odd_even_headers_select_by_physical_parity() {
        // odd-even fixture 실측: 1쪽=ODD·2쪽=EVEN·3쪽=ODD (1-기반 물리 서수).
        let doc = doc_with_bands(
            body_pages(3),
            vec![hf_item(ApplyPageType::Odd, &["홀수"]), hf_item(ApplyPageType::Even, &["짝수"])],
            vec![],
        );
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<String> = layout
            .pages
            .iter()
            .map(|p| {
                let lines: Vec<&LaidLine> =
                    p.lines.iter().filter(|l| l.location.contains("/h")).collect();
                assert_eq!(lines.len(), 1, "쪽마다 정확히 한 머리말");
                lines[0].runs[0].text.clone()
            })
            .collect();
        assert_eq!(texts, vec!["홀수", "짝수", "홀수"]);
    }

    #[test]
    fn both_plus_odd_header_is_ambiguous() {
        // 게이트2 H2: 중복 매치 = 한컴 우선순위 미실측 — 첫 항목 선택 금지.
        let doc = doc_with_bands(
            body_pages(1),
            vec![hf_item(ApplyPageType::Both, &["매쪽"]), hf_item(ApplyPageType::Odd, &["홀수"])],
            vec![],
        );
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::AmbiguousHeaderFooter { kind: "header", .. }), "{err:?}");
    }

    #[test]
    fn multi_paragraph_header_accumulates_vertpos() {
        // rules-header-multi 실측: 문단2 vertpos=1600 — 밴드-상대 누적 재생.
        let mut hf = HeaderFooter::new(
            vec![
                para_with_cache("첫째", vec![seg(0, 0)]),
                para_with_cache("둘째", vec![seg(0, 1600)]),
            ],
            ApplyPageType::Both,
        );
        hf.text_height = HwpUnit::new(2835).expect("2835 HU");
        let doc = doc_with_bands(body_pages(1), vec![hf], vec![]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let ps = PageSettings::a4();
        let header = band_lines(&layout.pages[0], "/h0/");
        assert_eq!(header.len(), 2);
        assert_eq!(header[0].baseline_y, ps.margin_top.as_i32() + 850);
        assert_eq!(header[1].baseline_y, ps.margin_top.as_i32() + 1600 + 850);
    }

    #[test]
    fn band_overflow_renders_unclipped_with_warning() {
        // rules-header-overflow 실측: 무클립 재생 + 경고 (fatal 아님).
        let hf = HeaderFooter::new(
            vec![
                para_with_cache("일", vec![seg(0, 0)]),
                para_with_cache("이", vec![seg(0, 1600)]),
                para_with_cache("삼", vec![seg(0, 3200)]),
                para_with_cache("사", vec![seg(0, 4800)]),
            ],
            ApplyPageType::Both,
        );
        let doc = doc_with_bands(body_pages(2), vec![hf], vec![]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        for page in &layout.pages {
            assert_eq!(band_lines(page, "/h0/").len(), 4, "초과분 포함 전량 재생");
        }
        let overflow: Vec<&PdfWarning> = layout
            .warnings
            .iter()
            .filter(|w| matches!(w, PdfWarning::BandOverflow { kind: "header", .. }))
            .collect();
        assert_eq!(overflow.len(), 1, "항목당 1회만 경고 (쪽 반복 스팸 금지)");
    }

    #[test]
    fn hide_first_header_skips_first_page_only() {
        let mut section = Section::with_paragraphs(body_pages(2), PageSettings::a4());
        section.headers = vec![hf_item(ApplyPageType::Both, &["머리말"])];
        section.visibility = Some(Visibility { hide_first_header: true, ..Visibility::default() });
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(band_lines(&layout.pages[0], "/h0/").is_empty(), "1쪽 생략");
        assert_eq!(band_lines(&layout.pages[1], "/h0/").len(), 1, "2쪽부터 재생");
    }

    #[test]
    fn header_cache_missing_follows_partial_cache_policy() {
        // WarnAndSkip(기본): 경고 후 생략 / Reject: fatal.
        let uncached = HeaderFooter::new(
            vec![Paragraph::with_runs(
                vec![Run::text("무캐시", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            ApplyPageType::Both,
        );
        let doc = doc_with_bands(body_pages(1), vec![uncached], vec![]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(band_lines(&layout.pages[0], "/h0/").is_empty());
        assert!(layout.warnings.iter().any(
            |w| matches!(w, PdfWarning::ParagraphSkipped { location } if location.contains("/h0/"))
        ));

        let opts = PdfOptions { partial_cache: PartialCachePolicy::Reject, ..Default::default() };
        let err = replay(&doc, &opts).unwrap_err();
        assert!(matches!(err, PdfError::MissingLayoutCache { .. }), "{err:?}");
    }

    #[test]
    fn parity_header_in_later_section_is_rejected() {
        // 다중 섹션 parity = 물리/구역 서수 미실측 — 거부 (게이트2 H2).
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(body_pages(1), PageSettings::a4()));
        let mut s2 = Section::with_paragraphs(body_pages(1), PageSettings::a4());
        s2.headers = vec![hf_item(ApplyPageType::Odd, &["홀수"])];
        doc.add_section(s2);
        let doc = doc.validate().expect("validate");
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::AmbiguousHeaderFooter { .. }), "{err:?}");
    }

    #[test]
    fn unmeasured_vert_align_and_page_starts_on_are_surfaced() {
        let mut hf = hf_item(ApplyPageType::Both, &["머리말"]);
        hf.vert_align = hwpforge_foundation::VerticalAlign::Center;
        let mut section = Section::with_paragraphs(body_pages(1), PageSettings::a4());
        section.headers = vec![hf];
        section.begin_num =
            Some(BeginNum { page_starts_on: PageStartsOn::Even, ..BeginNum::default() });
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        // TOP/BOTH 거동으로 재생은 되고 경고만 표면화.
        assert_eq!(band_lines(&layout.pages[0], "/h0/").len(), 1);
        assert!(layout.warnings.iter().any(|w| matches!(w, PdfWarning::VertAlignFallback { .. })));
        assert!(layout
            .warnings
            .iter()
            .any(|w| matches!(w, PdfWarning::PageStartsOnFallback { section: 0 })));
    }

    #[test]
    fn header_leading_non_text_run_is_rejected() {
        // 독립 리뷰 M1 상환: 밴드 경로도 본문과 동일한 fail-closed —
        // 로고 이미지 등으로 시작하는 머리말은 textpos 오절단 대신 거부.
        let mut hf = hf_item(ApplyPageType::Both, &["머리말"]);
        hf.paragraphs[0].runs.insert(
            0,
            Run::control(
                hwpforge_core::control::Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text("각주", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]),
                CharShapeIndex::new(0),
            ),
        );
        let doc = doc_with_bands(body_pages(1), vec![hf], vec![]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
    }

    #[test]
    fn page_starts_on_warns_even_without_headers_or_footers() {
        // 독립 리뷰 M2 상환: pageStartsOn 은 페이지네이션 속성 — 머리말/꼬리말
        // 유무와 무관하게 무조건 표면화돼야 한다 (§9 b0).
        let mut section = Section::with_paragraphs(body_pages(1), PageSettings::a4());
        section.begin_num =
            Some(BeginNum { page_starts_on: PageStartsOn::Even, ..BeginNum::default() });
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(layout
            .warnings
            .iter()
            .any(|w| matches!(w, PdfWarning::PageStartsOnFallback { section: 0 })));
    }

    // ── W5-b 쪽번호 합성 ─────────────────────────────────────────

    use hwpforge_core::section::PageNumber;
    use hwpforge_foundation::{NumberFormatType, PageNumberPosition};

    fn section_with_pagenum(body: Vec<Paragraph>, pn: PageNumber) -> Section {
        let mut s = Section::with_paragraphs(body, PageSettings::a4());
        s.page_number = Some(pn);
        s
    }

    fn bottom_digit() -> PageNumber {
        PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit)
    }

    #[test]
    fn page_number_synthesized_with_decoration_and_sequence() {
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(
            body_pages(3),
            PageNumber::with_decoration(
                PageNumberPosition::BottomCenter,
                NumberFormatType::Digit,
                "-",
            ),
        ));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<&str> = layout
            .pages
            .iter()
            .map(|p| p.page_number.as_ref().expect("pagenum").text.as_str())
            .collect();
        // §8c 실측: 장식은 어간 공백으로 감싼 "- n -" 형태.
        assert_eq!(texts, vec!["- 1 -", "- 2 -", "- 3 -"]);
        let ps = PageSettings::a4();
        let anchor = ps.height.as_i32() - ps.margin_bottom.as_i32();
        assert_eq!(layout.pages[0].page_number.as_ref().unwrap().anchor_bottom, anchor);
        // NoopStyles 엔 스타일 테이블이 없다 — 기본 charPr(0) 폴백 경고는 1회.
        assert_eq!(
            layout
                .warnings
                .iter()
                .filter(|w| matches!(w, PdfWarning::PageNumberStyleFallback { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn page_number_without_decoration_is_bare_digit() {
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(body_pages(1), bottom_digit()));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages[0].page_number.as_ref().unwrap().text, "1");
    }

    #[test]
    fn page_number_style_comes_from_dedicated_char_style() {
        // §8c: 출처 = "쪽 번호" CHAR 스타일 (문서 기본 아님 — fixture 실측).
        struct PageNumStyles;
        impl StyleLookup for PageNumStyles {
            fn char_style_shape(&self, name: &str) -> Option<CharShapeIndex> {
                (name == "쪽 번호").then(|| CharShapeIndex::new(7))
            }
        }
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(body_pages(1), bottom_digit()));
        let doc = doc.validate().expect("validate");
        let layout = replay_layout(
            &PdfInput { document: &doc, styles: &PageNumStyles },
            &PdfOptions::default(),
        )
        .expect("replay");
        let pn = layout.pages[0].page_number.as_ref().unwrap();
        assert_eq!(pn.char_shape, CharShapeIndex::new(7));
        assert!(
            !layout
                .warnings
                .iter()
                .any(|w| matches!(w, PdfWarning::PageNumberStyleFallback { .. })),
            "전용 스타일 보유 시 폴백 경고 없음"
        );
    }

    #[test]
    fn page_number_restart_and_continue_across_sections() {
        // beginNum.page: 0 = 이전 구역 계속, n>0 = n 재시작 (스펙 §10.6.2).
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(body_pages(2), bottom_digit()));
        let mut s1 = section_with_pagenum(body_pages(2), bottom_digit());
        s1.begin_num = Some(BeginNum { page: 10, ..BeginNum::default() });
        doc.add_section(s1);
        doc.add_section(section_with_pagenum(body_pages(1), bottom_digit()));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<&str> =
            layout.pages.iter().map(|p| p.page_number.as_ref().unwrap().text.as_str()).collect();
        assert_eq!(texts, vec!["1", "2", "10", "11", "12"]);
    }

    // ── W2: nwno 재시작 이벤트 (F1/F1b PDF 실측 정답지) ────────────────────

    /// [NewNumber ctrl run + 텍스트 run] 문단 (F1b 한컴 실측 run 형태 — ctrl
    /// 이 텍스트 앞, textpos 는 가시 텍스트만 계수).
    fn para_with_restart(text: &str, number: u32, lines: Vec<LineSeg>) -> Paragraph {
        use hwpforge_core::control::{Control, NewNumberKind};
        let mut p = Paragraph::with_runs(
            vec![
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number },
                    CharShapeIndex::new(0),
                ),
                Run::text(text, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(lines));
        p
    }

    #[test]
    fn new_number_on_second_page_restarts_from_there() {
        // F1b 정답지: 1쪽 = 1, 2쪽(컨트롤 앵커) = 7.
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![
                para_with_cache("1쪽 본문", vec![seg(0, 0)]),
                para_with_restart("2쪽 본문", 7, vec![seg(0, 0)]),
            ],
            PageSettings::a4(),
        ));
        doc.sections_mut()[0].page_number = Some(bottom_digit());
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<&str> =
            layout.pages.iter().map(|p| p.page_number.as_ref().unwrap().text.as_str()).collect();
        assert_eq!(texts, vec!["1", "7"], "중간 쪽 재시작 (F1b `1, 7`)");
        // 소비된 marker 는 드롭 경고를 내지 않는다.
        assert!(
            !layout.warnings.iter().any(|w| matches!(w, PdfWarning::NonTextRunDropped { .. })),
            "{:?}",
            layout.warnings
        );
    }

    #[test]
    fn new_number_on_first_page_renumbers_whole_document() {
        // F1 정답지: 1쪽 앵커 → 전문서 재번호 `7, 8`.
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![
                para_with_restart("1쪽 본문", 7, vec![seg(0, 0)]),
                para_with_cache("2쪽 본문", vec![seg(0, 0)]),
            ],
            PageSettings::a4(),
        ));
        doc.sections_mut()[0].page_number = Some(bottom_digit());
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<&str> =
            layout.pages.iter().map(|p| p.page_number.as_ref().unwrap().text.as_str()).collect();
        assert_eq!(texts, vec!["7", "8"]);
    }

    #[test]
    fn new_number_same_page_last_wins_and_non_page_kind_ignored() {
        use hwpforge_core::control::{Control, NewNumberKind};
        // 같은 쪽 다중 재시작 = 문서 순서 last-wins; 쪽 외 kind 는 렌더 무시.
        let mut p = Paragraph::with_runs(
            vec![
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number: 5 },
                    CharShapeIndex::new(0),
                ),
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number: 9 },
                    CharShapeIndex::new(0),
                ),
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Footnote, number: 2 },
                    CharShapeIndex::new(0),
                ),
                Run::text("본문", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![p, para_with_cache("다음 쪽", vec![seg(0, 0)])],
            PageSettings::a4(),
        ));
        doc.sections_mut()[0].page_number = Some(bottom_digit());
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<&str> =
            layout.pages.iter().map(|p| p.page_number.as_ref().unwrap().text.as_str()).collect();
        assert_eq!(texts, vec!["9", "10"], "last-wins + Footnote kind 무시");
    }

    #[test]
    fn explicit_page_break_splits_pages_even_with_equal_vertpos() {
        // F2 실측: 한컴 쪽나눔 문단은 lineseg v 를 리셋하지 않는다 (전부
        // 600) — pageBreak 플래그가 유일한 신호. 등v 3문단 = 3쪽.
        let mk = |text: &str, brk: bool| {
            let mut p = para_with_cache(text, vec![seg(0, 600)]);
            p.page_break = brk;
            p
        };
        let doc = doc_of(vec![mk("1쪽", false), mk("2쪽", true), mk("3쪽", true)]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 3, "등v 에서도 쪽나눔 플래그로 분할");
        for (i, page) in layout.pages.iter().enumerate() {
            assert_eq!(page.lines.len(), 1, "p{i} 줄 1개씩");
        }
    }

    #[test]
    fn page_break_on_first_content_does_not_create_leading_blank_page() {
        // 구역 시작이 이미 새 쪽 — 첫 문단의 쪽나눔 플래그로 빈 쪽을 만들지
        // 않는다.
        let mut p = para_with_cache("본문", vec![seg(0, 600)]);
        p.page_break = true;
        let doc = doc_of(vec![p]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 1);
    }

    // ── W3: pghd 감춤 이벤트 (F2-①/③ PDF 실측 정답지) ────────────────────

    /// [PageHiding ctrl run + 텍스트 run] 문단 (F2 한컴 실측 run 형태).
    fn para_with_hiding(
        text: &str,
        hide_page_num: bool,
        hide_header: bool,
        lines: Vec<LineSeg>,
    ) -> Paragraph {
        use hwpforge_core::control::Control;
        let mut p = Paragraph::with_runs(
            vec![
                Run::control(
                    Control::PageHiding {
                        hide_header,
                        hide_footer: false,
                        hide_master_page: false,
                        hide_border: false,
                        hide_fill: false,
                        hide_page_num,
                    },
                    CharShapeIndex::new(0),
                ),
                Run::text(text, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(lines));
        p
    }

    #[test]
    fn page_hiding_suppresses_number_display_but_counter_advances() {
        // F2-① 정답지: 쪽번호 `1, _, 3` — 2쪽 표시만 소거, 카운터는 전진.
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![
                para_with_cache("1쪽", vec![seg(0, 0)]),
                para_with_hiding("2쪽 — 감추기", true, false, vec![seg(0, 0)]),
                para_with_cache("3쪽", vec![seg(0, 0)]),
            ],
            PageSettings::a4(),
        ));
        doc.sections_mut()[0].page_number = Some(bottom_digit());
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let texts: Vec<Option<&str>> =
            layout.pages.iter().map(|p| p.page_number.as_ref().map(|n| n.text.as_str())).collect();
        assert_eq!(texts, vec![Some("1"), None, Some("3")], "F2-① `1, _, 3`");
    }

    #[test]
    fn page_hiding_suppresses_header_band_on_its_page_only() {
        // F2-③ 정답지: 2쪽 머리말만 소거 — 1·3쪽 머리말 유지.
        let mut doc = Document::new();
        let mut section = Section::with_paragraphs(
            vec![
                para_with_cache("1쪽", vec![seg(0, 0)]),
                para_with_hiding("2쪽 — 감추기", true, true, vec![seg(0, 0)]),
                para_with_cache("3쪽", vec![seg(0, 0)]),
            ],
            PageSettings::a4(),
        );
        section.headers = vec![hf_item(ApplyPageType::Both, &["머리말"])];
        doc.add_section(section);
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let headers_per_page: Vec<usize> =
            layout.pages.iter().map(|p| band_lines(p, "/h0/").len()).collect();
        assert_eq!(headers_per_page, vec![1, 0, 1], "2쪽 머리말만 억제");
        // 감춤 marker 는 드롭 경고를 내지 않는다.
        assert!(
            !layout.warnings.iter().any(|w| matches!(w, PdfWarning::NonTextRunDropped { .. })),
            "{:?}",
            layout.warnings
        );
    }

    #[test]
    fn page_number_hide_first_skips_display_but_counts() {
        let mut section = section_with_pagenum(body_pages(2), bottom_digit());
        section.visibility =
            Some(Visibility { hide_first_page_num: true, ..Visibility::default() });
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(layout.pages[0].page_number.is_none(), "1쪽 표시 생략");
        assert_eq!(layout.pages[1].page_number.as_ref().unwrap().text, "2", "번호 자체는 진행");
    }

    #[test]
    fn page_number_unmeasured_position_or_format_warns_and_skips() {
        // TOP_* 등 미실측 position — 경고 + 생략.
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(
            body_pages(1),
            PageNumber::new(PageNumberPosition::TopCenter, NumberFormatType::Digit),
        ));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(layout.pages[0].page_number.is_none());
        assert!(layout
            .warnings
            .iter()
            .any(|w| matches!(w, PdfWarning::PageNumberSkipped { what: "position", .. })));

        // 비 DIGIT 포맷 — 경고 + 생략.
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(
            body_pages(1),
            PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::RomanCapital),
        ));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(layout.pages[0].page_number.is_none());
        assert!(layout
            .warnings
            .iter()
            .any(|w| matches!(w, PdfWarning::PageNumberSkipped { what: "format", .. })));

        // position None = 표시 안 함 — 경고도 없음.
        let mut doc = Document::new();
        doc.add_section(section_with_pagenum(
            body_pages(1),
            PageNumber::new(PageNumberPosition::None, NumberFormatType::Digit),
        ));
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(layout.pages[0].page_number.is_none());
        assert!(!layout.warnings.iter().any(|w| matches!(w, PdfWarning::PageNumberSkipped { .. })));
    }

    // ── W3c 표 admission (검증된 프로파일 밖 = fail-closed) ──────

    fn cell_with_cache(text: &str) -> TableCell {
        TableCell::new(
            vec![para_with_cache(text, vec![seg(0, 0)])],
            HwpUnit::from_pt(100.0).unwrap(),
        )
    }

    fn one_cell_cached_table() -> Table {
        Table::new(vec![TableRow::new(vec![cell_with_cache("셀")])])
            .with_layout_cache(hwpforge_core::table::TableLayoutCache::new(None, true))
    }

    /// 텍스트 없는 host 문단 (표 run + host 캐시 1줄).
    fn table_host(table: Table, host_v: i32) -> Paragraph {
        let mut p = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(vec![seg(0, host_v)]));
        p
    }

    #[test]
    fn unsplit_table_flow_uses_host_cache_not_computed_margins() {
        // 기재부 corpus over-split 실측 (p64→p65): 미분할 표의 계산치
        // (outMargin 합산 = host_v + om.top + 표높이 + om.bottom)가 캐시
        // 흐름(host_v + vertsize + spacing)보다 커서, 다음 문단 v 를
        // "새 쪽"으로 오판해 한 쪽을 둘로 쪼갰다. 흐름 앵커 = host lineseg.
        let mut table = one_cell_cached_table();
        table.out_margin = Some(hwpforge_core::table::TableMargin {
            left: HwpUnit::new(283).expect("283"),
            right: HwpUnit::new(283).expect("283"),
            top: HwpUnit::new(283).expect("283"),
            bottom: HwpUnit::new(283).expect("283"),
        });
        let mut host = table_host(table, 0);
        // 캐시 진실: host lineseg vertsize = 표높이 1000 (corpus 실측: vertsize
        // = sz.h, outMargin 미포함).
        host.layout_cache.as_mut().expect("host cache").lines[0].vertsize = 1000;
        // 다음 문단 = 캐시 흐름 그대로 (1000 + 문단 간격 376) — 계산치
        // 1566(= om 283 + 표높이 1000 + om 283) 보다 작아 구 코드는 새 쪽으로
        // 오판했다 (corpus p64→p65 와 동일 비율).
        let next = para_with_cache("다음 문단", vec![seg(0, 1376)]);
        let doc = doc_of(vec![host, next]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 1, "미분할 표 뒤 문단은 같은 쪽에 남아야 함");
    }

    #[test]
    fn anchored_table_flow_uses_computed_margins() {
        // 앵커형(비-글자취급) 표: host lineseg 는 순수 줄높이(1000 ≪ 표높이)
        // — 흐름 = host_v + om + Σ행높이 + om (rules-table 실물 wire 정확 일치
        // 모델). 인라인 판별(vertsize ≥ 표높이)에 걸리지 않아야 한다.
        let tall_cell = TableCell::new(
            vec![para_with_cache("셀", vec![seg(0, 0), seg(1, 1500)])],
            HwpUnit::from_pt(100.0).unwrap(),
        );
        let mut table = Table::new(vec![TableRow::new(vec![tall_cell])])
            .with_layout_cache(hwpforge_core::table::TableLayoutCache::new(None, true));
        table.out_margin = Some(hwpforge_core::table::TableMargin {
            left: HwpUnit::new(283).expect("283"),
            right: HwpUnit::new(283).expect("283"),
            top: HwpUnit::new(283).expect("283"),
            bottom: HwpUnit::new(283).expect("283"),
        });
        // H = 셀 extent 2500 (= 1500 + 1000) → 흐름 = 0 + 283 + 2500 + 283 = 3066.
        let next = para_with_cache("다음", vec![seg(0, 3066)]);
        let doc = doc_of(vec![table_host(table, 0), next]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 1, "앵커형 계산 흐름과 캐시가 정합");
    }

    #[test]
    fn table_mixed_with_visible_text_is_rejected() {
        let mut host = para_with_cache("표 호스트", vec![seg(0, 0)]);
        host.add_run(Run::table(one_cell_cached_table(), CharShapeIndex::new(0)));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).unwrap_err();
        assert!(
            matches!(
                err,
                PdfError::UnsupportedContent { kind: "table mixed with visible text", .. }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn table_without_caches_is_rejected() {
        // 표 layout_cache 없음 (합성 문서) → fatal.
        let bare = Table::new(vec![TableRow::new(vec![cell_with_cache("셀")])]);
        let err = replay(&doc_of(vec![table_host(bare, 0)]), &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::MissingLayoutCache { .. }), "{err:?}");

        // 셀 문단 캐시 결손 → 표 단위 fatal (게이트2 C4 — WarnAndSkip 아님).
        let uncached_cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("결손", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_pt(100.0).unwrap(),
        );
        let table = Table::new(vec![TableRow::new(vec![uncached_cell])])
            .with_layout_cache(hwpforge_core::table::TableLayoutCache::new(None, true));
        let err = replay(&doc_of(vec![table_host(table, 0)]), &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::MissingLayoutCache { .. }), "{err:?}");
    }

    #[test]
    fn table_profile_violations_are_rejected() {
        use hwpforge_core::table::TableLayoutCache;
        // 비기본 pos
        let t = Table::new(vec![TableRow::new(vec![cell_with_cache("셀")])])
            .with_layout_cache(TableLayoutCache::new(None, false));
        let err = replay(&doc_of(vec![table_host(t, 0)]), &PdfOptions::default()).unwrap_err();
        assert!(
            matches!(err, PdfError::UnsupportedContent { kind: "non-default table position", .. }),
            "{err:?}"
        );
        // cellSpacing ≠ 0
        let t = one_cell_cached_table().with_cell_spacing(HwpUnit::from_pt(1.0).unwrap());
        let err = replay(&doc_of(vec![table_host(t, 0)]), &PdfOptions::default()).unwrap_err();
        assert!(
            matches!(err, PdfError::UnsupportedContent { kind: "nonzero table cellSpacing", .. }),
            "{err:?}"
        );
        // 중첩 표 = 지원 (재귀 평면 배치 — blank-HPC p2 실물 대응).
        let mut inner_host = para_with_cache("", vec![seg(0, 0)]);
        inner_host.add_run(Run::table(one_cell_cached_table(), CharShapeIndex::new(0)));
        let nested = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![inner_host],
            HwpUnit::from_pt(100.0).unwrap(),
        )])])
        .with_layout_cache(TableLayoutCache::new(None, true));
        let layout =
            replay(&doc_of(vec![table_host(nested, 0)]), &PdfOptions::default()).expect("nested");
        let texts: Vec<String> = layout.pages[0]
            .lines
            .iter()
            .flat_map(|l| l.runs.iter().map(|r| r.text.clone()))
            .collect();
        assert!(texts.iter().any(|t| t == "셀"), "안쪽 표 텍스트 재생: {texts:?}");
    }

    #[test]
    fn merged_cell_deficit_goes_to_last_spanned_row() {
        use hwpforge_core::table::TableLayoutCache;
        // 2행 격자: (0,1) rowspan2 높이 요구 5000 > 행 최소합 2000 —
        // 부족분은 마지막 스팬 행 몰빵 (rules-rowspan-deficit 실측 2026-08-07)
        // → 행높이 [1000, 4000], 총 5000. 검산 앵커가 총높이를 잠근다.
        let make = || {
            let tall = TableCell::with_span(
                vec![para_with_cache("병합", vec![seg(0, 0)])],
                HwpUnit::from_pt(100.0).unwrap(),
                1,
                2,
            )
            .with_height(HwpUnit::from_pt(50.0).unwrap());
            Table::new(vec![
                TableRow::new(vec![cell_with_cache("A"), tall]),
                TableRow::new(vec![cell_with_cache("C")]),
            ])
            .with_layout_cache(TableLayoutCache::new(None, true))
        };
        // 총높이 5000 앵커 = 성공 + 배분 경고 + 기하 직접 잠금:
        // "C"(행1 셀) baseline = body_top + 행0 높이(1000, 최소 유지) + 850
        // — 부족분이 행0 이 아니라 행1 하단으로 갔다는 증명 (독립리뷰 M1).
        let follow = para_with_cache("후속", vec![seg(0, 5000)]);
        // host lineseg vertsize = 표 흐름 소비(총높이 5000) — corpus 실측 규칙
        // (미분할 표의 흐름 앵커 = 캐시). 이전 합성값 1000 은 실물과 다른 거짓말.
        let mut host = table_host(make(), 0);
        host.layout_cache.as_mut().expect("host cache").lines[0].vertsize = 5000;
        let doc = doc_of(vec![host, follow]);
        let layout = replay(&doc, &PdfOptions::default()).expect("deficit replay");
        assert!(
            layout.warnings.iter().any(|w| matches!(w, PdfWarning::TableDeficitDistributed { .. })),
            "{:?}",
            layout.warnings
        );
        let body_top = hwpforge_core::page::PageSettings::a4().margin_top.as_i32()
            + hwpforge_core::page::PageSettings::a4().header_margin.as_i32();
        let c_line = layout.pages[0]
            .lines
            .iter()
            .find(|l| l.runs.iter().any(|r| r.text == "C"))
            .expect("C line");
        assert_eq!(c_line.baseline_y, body_top + 1000 + 850, "행0 은 최소높이 유지");
        // 어긋난 앵커(6000 > 기대 5000, 쪽분할 아님) = 캐시 모순 fatal.
        // (기대보다 작은 v 는 쪽분할로 해석돼 앵커를 못 건다 — 문서화된 사각.)
        let stale = para_with_cache("후속", vec![seg(0, 6000)]);
        let mut host2 = table_host(make(), 0);
        host2.layout_cache.as_mut().expect("host cache").lines[0].vertsize = 5000;
        let doc = doc_of(vec![host2, stale]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
    }

    #[test]
    fn synthetic_table_replays_and_anchor_verifies() {
        // 1셀 표 (행높이 = 셀 extent 1000) + 정확한 앵커의 후속 문단.
        let host_v = 1600;
        let follow = para_with_cache("후속", vec![seg(0, host_v + 1000)]);
        let doc = doc_of(vec![table_host(one_cell_cached_table(), host_v), follow]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert_eq!(layout.pages.len(), 1, "단일 조각 = 쪽 분할 없음");
        assert!(
            !layout
                .warnings
                .iter()
                .any(|w| matches!(w, PdfWarning::TablePaginationComputed { .. })),
            "비분할 표는 계산 페이지네이션 경고가 없어야 한다"
        );
        let texts: Vec<String> = layout.pages[0]
            .lines
            .iter()
            .flat_map(|l| l.runs.iter().map(|r| r.text.clone()))
            .collect();
        assert!(texts.iter().any(|t| t == "셀"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "후속"), "{texts:?}");
    }

    #[test]
    fn table_anchor_mismatch_is_fatal() {
        let host_v = 1600;
        // 기대 앵커 = host_v + 1000 인데 캐시가 다른 값을 주장 → 계산 불신.
        let follow = para_with_cache("후속", vec![seg(0, host_v + 4321)]);
        let doc = doc_of(vec![table_host(one_cell_cached_table(), host_v), follow]);
        let err = replay(&doc, &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
    }

    #[test]
    fn table_saved_sz_mismatch_is_fatal() {
        use hwpforge_core::table::TableLayoutCache;
        // 재저장 sz(5000) ≠ 계산 첫 조각 높이(1000) → fatal (게이트2 H7).
        let table = Table::new(vec![TableRow::new(vec![cell_with_cache("셀")])])
            .with_layout_cache(TableLayoutCache::new(Some(HwpUnit::new(5000).unwrap()), true));
        let err = replay(&doc_of(vec![table_host(table, 0)]), &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
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
