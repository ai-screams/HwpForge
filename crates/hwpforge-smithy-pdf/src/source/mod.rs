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
/// 줄 안의 원자 하나 — 텍스트 run·인라인 이미지·인라인 글상자 (W2a §3 D4 · W4).
#[derive(Debug, Clone, PartialEq)]
pub enum LineAtom {
    /// 셰이핑 대상 텍스트 run.
    Text(LineRun),
    /// 인라인 이미지 — W2a 에선 synthetic 테스트만 생성하고 production
    /// producer 는 admission 이 막는다 (개방 = W2b).
    Image(LineImage),
    /// 인라인(글자취급) 글상자 — W4 w2 에선 타입만 정의하고 production
    /// producer(admission)는 w3 이 연다. 렌더는 w3 이 [`LineTextBox`] 를
    /// [`crate::paint::PaintItem::Clipped`] 로 낮춘다.
    TextBox(LineTextBox),
}

/// 줄 안 인라인 이미지 원자.
#[derive(Debug, Clone, PartialEq)]
pub struct LineImage {
    /// canonical 스토어 키 (`StyleLookup::image_data` 조회 키).
    pub canonical_key: String,
    /// 표시 폭 (HWPUNIT).
    pub width: i32,
    /// 표시 높이 (HWPUNIT).
    pub height: i32,
}

/// 줄 안 인라인(글자취급) 글상자 원자 (W4 w2).
///
/// 글상자는 이미지처럼 호스트 줄에서 인라인 **폭**([`Self::width`])을
/// 소비하는 원자다. 내부는 박스-내용 상대 좌표의 연속 줄들
/// ([`Self::inner_lines`])이고, 테두리·채움([`Self::style`])과 세로
/// 정렬([`Self::vert_align`])을 가진다 (설계 §8a 실측).
///
/// # w3 이 소비할 계약
///
/// 1. **원점·폭 소비**: 호스트 줄의 x 커서가 [`Self::width`] 만큼 전진한다
///    (인라인 이미지와 동일 — [`LineImage::width`] 선례). 박스 좌상단 =
///    (전진 전 x, 호스트 줄 top).
/// 2. **박스 clip**: 박스 사각형([`Self::width`]×[`Self::height`])으로
///    [`crate::paint::PaintItem::Clipped`] 를 세운다 — overflow 실측(한컴은
///    넘친 줄을 캐시에 방출하고 렌더에서 절단)이라 상시 clip.
/// 3. **내부 줄 replay**: [`Self::inner_lines`] 각 줄을 **박스 원점 +
///    textMargin(기본 283, gotcha #29) + vertAlign 시프트** 를 더해 절대
///    배치한다 (셀 replay 선례 — [`crate::source::table`]). 내부 캐시 v축은
///    문단 경계를 넘어 연속이다 (§8a). 각 줄의 `seg`([`hwpforge_core::layout::LineSeg`])·
///    `alignment`·`is_last_line` 이 텍스트/이미지 배치와 JUSTIFY 규칙을 준다.
/// 4. **vertAlign 시프트**: 내부 캐시는 vertAlign 을 반영하지 않는다(3종
///    모두 내부 vertpos 0 — §8f 실측) — TOP/CENTER/BOTTOM 은 렌더가
///    콘텐츠 높이 대비 박스 높이 여백으로 시프트한다 (셀 valign 선례).
///
/// # 명시 제외 (W4 백로그 — admission fail-closed 유지)
///
/// 내부 원자가 다시 글상자·표인 중첩은 W4 범위 밖이다. w3 의 admission 이
/// 그런 캐시를 거부한다 (fixture 부재 — §8f ⑤). 타입 자체는
/// [`TextBoxLine::atoms`] 가 [`LineAtom`] 를 담아 재귀 표현이 가능하다.
#[derive(Debug, Clone, PartialEq)]
pub struct LineTextBox {
    /// 박스 폭 (HWPUNIT) — 호스트 줄에서 소비하는 인라인 폭.
    pub width: i32,
    /// 박스 높이 (HWPUNIT) — clip 영역 높이.
    pub height: i32,
    /// 테두리·채움 스타일 (Core `ShapeStyle` 에서 증류 — `None` 이면 박스
    /// 페인트 없음, 내부 콘텐츠만).
    pub style: Option<TextBoxStyle>,
    /// 내부 콘텐츠 세로 정렬 (TOP/CENTER/BOTTOM — 내부 캐시 미반영, 렌더가
    /// 시프트).
    pub vert_align: hwpforge_foundation::VerticalAlign,
    /// 내부 줄들 (박스-내용 상대, 문단 경계를 넘어 연속하는 v축 — §8a).
    pub inner_lines: Vec<TextBoxLine>,
}

/// 글상자 내부 한 줄 (박스-내용 상대 좌표 — w3 렌더가 박스 원점·textMargin·
/// vertAlign 시프트를 더해 절대 배치한다).
///
/// `seg` 는 디코드 캐시의 [`hwpforge_core::layout::LineSeg`] 를 **그대로**
/// 나른다 (설계 §8g "내부 lineseg replay = 그대로 재생"). `textpos` 는
/// 원자가 이미 줄별로 분할됐으므로 렌더에서 쓰이지 않지만, 캐시 실측값을
/// 왜곡 없이 보존한다.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBoxLine {
    /// 이 줄의 조판 캐시 기하 (박스-내용 상대 — vertpos/baseline/horzpos/
    /// horzsize/vertsize/textheight).
    pub seg: hwpforge_core::layout::LineSeg,
    /// 줄 원자들 (텍스트/이미지 — 재귀 표현. 중첩 글상자·표는 W4 제외,
    /// admission fail-closed).
    pub atoms: Vec<LineAtom>,
    /// 이 줄이 속한 (내부) 문단의 정렬.
    pub alignment: Alignment,
    /// (내부) 문단의 마지막 줄인지 (JUSTIFY 마지막 줄 규칙).
    pub is_last_line: bool,
}

/// 글상자 테두리·채움 (Core `ShapeStyle` 증류 — Paint IR `Rect`(채움)·
/// `Line`(테두리)로 낮출 원색·굵기만).
///
/// `line_style`(dash/dot 등)은 담지 않는다 — Paint `Line` 이 실선만
/// 그리므로(설계 §8g w4) 담아도 죽은 데이터다.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBoxStyle {
    /// 테두리 색 (`None` = 테두리 없음).
    pub line_color: Option<Color>,
    /// 채움 색 (`None` = 채움 없음).
    pub fill_color: Option<Color>,
    /// 테두리 굵기 (HWPUNIT — `line_color` 가 있을 때만 의미).
    pub line_width: i32,
}

/// 배치가 끝난 한 줄.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidLine {
    /// 위치 보고용 경로 (`s{섹션}/p{문단}/l{줄}`).
    pub location: String,
    /// 이 줄을 구성하는 원자들 (run 경계 분할 — 시각 순서, 텍스트/이미지 혼합).
    pub atoms: Vec<LineAtom>,
    /// 줄 상자 상단 y (HWPUNIT, 페이지 원점) — 인라인 이미지의 top 앵커
    /// (W0a 실측: 이미지 top = 줄 top). `baseline_y − seg.baseline` 과 동치.
    pub top_y: i32,
    /// baseline 세로 위치 (HWPUNIT, 쪽 상단 원점 — body_top + v + baseline).
    pub baseline_y: i32,
    /// 줄 가로 상자 (HWPUNIT — body 좌변 반영, 정렬 미적용 상태).
    pub line_box: LineBox,
    /// 문단의 마지막 줄인지 (JUSTIFY 마지막 줄 규칙).
    pub is_last_line: bool,
    /// 문단 정렬.
    pub alignment: Alignment,
}

impl LaidLine {
    /// 텍스트 원자만 문서 순서로 돌려준다 (검사/테스트 편의).
    pub fn text_runs(&self) -> impl Iterator<Item = &LineRun> {
        self.atoms.iter().filter_map(|a| match a {
            LineAtom::Text(r) => Some(r),
            LineAtom::Image(_) | LineAtom::TextBox(_) => None,
        })
    }
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

/// 문단의 nwno(쪽 종류) 재시작·pghd 감춤을 **가시 텍스트 오프셋**과 함께
/// 수집한다 (오프셋 → 줄 → 물리 쪽 앵커의 입력). cache admission 과 분리해
/// 문단 진입 시 항상 호출된다 — table-host/cacheless 경로도 이벤트를 놓치지
/// 않는다 (독립 리뷰 High #3·#4).
fn collect_page_events(
    para: &hwpforge_core::paragraph::Paragraph,
) -> (Vec<(usize, u32)>, Vec<(usize, PageHideEvent)>) {
    use hwpforge_core::control::Control;
    let mut restarts: Vec<(usize, u32)> = Vec::new();
    let mut hide_marks: Vec<(usize, PageHideEvent)> = Vec::new();
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
                        Control::PageHiding { hide_header, hide_footer, hide_page_num, .. } => {
                            hide_marks.push((
                                pos,
                                // page 는 앵커 시점에 확정 — 임시 0.
                                PageHideEvent {
                                    page: 0,
                                    page_num: hide_page_num,
                                    header: hide_header,
                                    footer: hide_footer,
                                },
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    (restarts, hide_marks)
}

/// 글상자 내부 텍스트 여백 (HWPUNIT) — HWPX `<hp:textMargin>` 기본값
/// (gotcha #29). Core `Control::TextBox` 는 여백을 carry 하지 않으므로 기본을
/// 쓴다 (리뷰 Low-2). 비기본 여백 문서의 거동은 regime 에 따라 다르다:
/// content+margin-dominated(내용이 박스를 채움)에선 host-줄 predicate
/// 불일치로 fail-closed 되지만, **box-dominated**(`box.height ≥
/// content+2×여백)에선 조용히 admitted 되어 내부 세로 offset 이 실제
/// 여백과의 차만큼(bounded) 어긋난다. 근본 해소 = Core textMargin carry
/// (백로그 — 에픽 문서 §8h).
pub(crate) const TEXTBOX_TEXT_MARGIN: i32 = 283;

/// 줄이 **원자 지배 profile**(삼중 일치: vertsize==textheight==원자 기대
/// 높이)인지 — 인라인 이미지·글상자가 공유하는 host-줄 predicate. 경계 귀속
/// 판별과 후검사가 이 동일 predicate 를 쓴다 (§7 r2 fold-in — 순환 아님:
/// 독립 입력의 방어적 중복).
pub(crate) fn line_matches_object_height(
    seg: &hwpforge_core::layout::LineSeg,
    height: i32,
) -> bool {
    seg.vertsize == height && seg.textheight == height
}

/// 줄이 이미지 지배 profile 인지 (이미지 기대 높이 = `image_height`).
/// [`line_matches_object_height`] 의 이미지 이름 alias — 기존 호출부 유지.
pub(crate) fn line_matches_image_height(
    seg: &hwpforge_core::layout::LineSeg,
    image_height: i32,
) -> bool {
    line_matches_object_height(seg, image_height)
}

/// body admission 이 허용하는 글자취급(treat_as_char) 인라인 이미지인지
/// (W2b §4 D1 — placement None/false 는 보수 거부, 앵커형 렌더 = W5).
pub(crate) fn is_admitted_inline_image(content: &RunContent) -> bool {
    matches!(content, RunContent::Image(img)
        if img.placement.as_ref().is_some_and(|p| p.treat_as_char))
}

/// body admission 이 허용하는 글자취급(treat_as_char) 인라인 글상자인지
/// (W4 §8g — `placement == None` 은 레거시 인라인 기본값(treat_as_char)이라
/// 허용, `Some` 은 `treat_as_char` 일 때만. 앵커형(`treat_as_char=false`)은
/// 보수 거부 → 앵커 렌더 = W5).
pub(crate) fn is_admitted_inline_textbox(content: &RunContent) -> bool {
    matches!(content, RunContent::Control(c)
        if matches!(&**c, hwpforge_core::control::Control::TextBox { placement, .. }
            if placement.as_ref().is_none_or(|p| p.treat_as_char)))
}

/// 글상자 내부 콘텐츠 세로 범위 = `max(내부 줄 vertpos + vertsize)` (checked).
///
/// admission host-줄 predicate(`max(box.height, extent + 상하 textMargin)`)와
/// 렌더 vertAlign 시프트(`interior = box.height − 2×textMargin`, `interior −
/// extent`)가 이 **단일 산식**을 공유한다 — 두 단계가 drift 하지 않도록.
pub(crate) fn textbox_content_extent(
    inner_lines: &[TextBoxLine],
    location: &str,
) -> PdfResult<i32> {
    let mut extent = 0;
    for line in inner_lines {
        let bottom = line.seg.vertpos.checked_add(line.seg.vertsize).ok_or_else(|| {
            PdfError::InvalidCache {
                detail: format!("{location}: text box inner line bottom overflows i32"),
            }
        })?;
        extent = extent.max(bottom);
    }
    Ok(extent)
}

/// 글상자 host 줄 기대 높이 = `max(box.height, content_extent + 상하 textMargin)`.
///
/// fixture 실측(§8f ①): basic 11339(=box.height) · valign 7087(=box.height) ·
/// overflow 12766(=extent 12200 + 566 > box 4252). 불일치 = 미측정 profile →
/// 호출부가 `InvalidCache` fail-closed.
pub(crate) fn textbox_expected_host_height(box_height: i32, content_extent: i32) -> PdfResult<i32> {
    let with_margins = content_extent.checked_add(2 * TEXTBOX_TEXT_MARGIN).ok_or_else(|| {
        PdfError::InvalidCache {
            detail: "text box content extent + margins overflows i32".to_string(),
        }
    })?;
    Ok(box_height.max(with_margins))
}

/// 렌더 replay(`collect_page_events`)가 실제 소비하는 0폭 marker 인지.
///
/// W1b (§1g v5 변경 1): allowlist 는 **실제 consumer 와 동일한 match** —
/// `NewNumber` 는 `Page` kind 만 replay 가 소비한다 (각주/그림 번호
/// 재시작 등 다른 kind 는 소비자가 없으므로 허용하면 무음 드롭).
/// admission(문단 전체 검사)과 `NonTextRunDropped` 경고에서 제외된다.
fn is_replay_consumed_marker(content: &RunContent) -> bool {
    matches!(content, RunContent::Control(c)
    if matches!(
        **c,
        hwpforge_core::control::Control::NewNumber {
            kind: hwpforge_core::control::NewNumberKind::Page,
            ..
        } | hwpforge_core::control::Control::PageHiding { .. }
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
        // W1b admission (§1g v5 변경 1): 밴드는 대응 consumer 가 없어
        // allowlist 가 **비어 있다** — 문단 전체에서 비텍스트 run 은 전부
        // W2(밴드 이미지 렌더) 전 InvalidCache.
        if para.runs.iter().any(|r| r.content.plain_text().is_none()) {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: non-text run in band paragraph is not renderable \
                     before W2 (band allowlist is empty)"
                ),
            });
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
                atoms: runs.into_iter().map(LineAtom::Text).collect(),
                top_y: band_top + seg.vertpos,
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

        // W2/W3 이벤트는 **cache admission 과 무관하게** 문단 진입 시 수집한다
        // (독립 리뷰 High #3·#4: table-host/cacheless 경로의 `continue` 가
        // 이벤트를 무음 유실시켰다).
        let (restarts, hide_marks) = collect_page_events(para);

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
            // 이미지 에픽 게이트 2 Critical#5 선행 수리: `[Image, Table]` 등
            // 비텍스트 혼합 host 는 지금까지 표만 재생하고 나머지 객체를
            // **무진단 폐기**했다 — 미지원 종류는 fail-closed 로 거부한다.
            // (재시작/감춤 marker 는 host 쪽 이벤트로 소비되므로 예외.)
            if para.runs.iter().any(|r| {
                !matches!(r.content, RunContent::Table(_))
                    && r.content.plain_text().is_none()
                    && !is_replay_consumed_marker(&r.content)
            }) {
                return Err(PdfError::UnsupportedContent {
                    kind: "non-text content in table-host paragraph",
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

            // 독립 리뷰 High #4: `[marker, Table]` host 문단의 재시작/감춤은
            // host lineseg 가 귀속된 쪽(표 재생 전 현재 쪽)에 앵커한다.
            let host_page = pages.len() - 1;
            for &(_, number) in &restarts {
                events.push(PageNumberEvent { page: host_page, number });
            }
            for &(_, hide) in &hide_marks {
                hides.push(PageHideEvent { page: host_page, ..hide });
            }

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
            // 독립 리뷰 High #3: 스킵 문단의 재시작/감춤은 물리 쪽을 결정할
            // 수 없다 — 근사 앵커로 날조하지 않고 유실을 특정 경고로 표면화.
            if !restarts.is_empty() || !hide_marks.is_empty() {
                warnings.push(PdfWarning::PageEventLost { location: location.clone() });
            }
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

        // W1b admission (§1g v5 변경 1): 검사는 선두 prefix 가 아니라
        // **문단 전체** — W1b 좌표 정규화로 marker 문단의 textpos 는
        // 신뢰 가능해졌지만, 미렌더 원자(이미지·각주·수식·미지 컨트롤)는
        // 위치 무관 W2 전 InvalidCache 다 (trailing 이미지가 0폭으로
        // 무음 누락되는 경로 차단). 허용 = replay 가 실제 소비하는 0폭
        // marker(Page 재시작·감추기)뿐.
        // W2b (§4 D1): body 는 **글자취급(treat_as_char) 인라인 이미지**를
        // 추가 허용한다 — placement None/false 는 보수 거부 유지 (앵커형
        // 렌더 = W5). 그 외 미렌더 원자는 여전히 InvalidCache.
        // W4 (§8g): body 는 글자취급(treat_as_char) 인라인 **글상자**도
        // 추가 허용한다 — 앵커형(treat_as_char=false)은 여전히 거부(W5).
        if para.runs.iter().any(|r| {
            r.content.plain_text().is_none()
                && !is_replay_consumed_marker(&r.content)
                && !is_admitted_inline_image(&r.content)
                && !is_admitted_inline_textbox(&r.content)
        }) {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: non-text run is not renderable before W2 \
                     (only replay-consumed page markers are admitted)"
                ),
            });
        }

        let text = para.text_content();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        validate_textpos(cache, utf16.len(), &location)?;

        // W2b/W4 (§4 D2 · §8g): 이미지·글상자 문단은 원자 지배 profile 로
        // 줄별 원자를 귀속하고 host 줄 높이 삼중 일치를 검사한다. 이미지와
        // 글상자를 한 문단에 섞은 캐시는 미측정 — fail-closed.
        let has_admitted_images = para.runs.iter().any(|r| is_admitted_inline_image(&r.content));
        let has_admitted_textbox = para.runs.iter().any(|r| is_admitted_inline_textbox(&r.content));
        if has_admitted_images && has_admitted_textbox {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: paragraph mixes an inline image and a text box — \
                     unmeasured profile (unsupported)"
                ),
            });
        }
        let line_atoms_override = if has_admitted_images {
            // 이미지 문단: image.height == vertsize == textheight (W0a profile).
            let atoms = build_inline_image_line_atoms(para, cache, &location)?;
            for (li, line) in atoms.iter().enumerate() {
                for atom in line {
                    if let LineAtom::Image(img) = atom {
                        let seg = &cache.lines[li];
                        if !line_matches_image_height(seg, img.height) {
                            return Err(PdfError::InvalidCache {
                                detail: format!(
                                    "{location}/l{li}: image height {} != line vertsize {} / \
                                     textheight {} — unmeasured height profile",
                                    img.height, seg.vertsize, seg.textheight
                                ),
                            });
                        }
                    }
                }
            }
            Some(atoms)
        } else if has_admitted_textbox {
            // 글상자 문단: host 줄 vertsize == textheight ==
            // max(box.height, content_extent + 상하 textMargin) (§8f ①).
            let atoms = build_inline_textbox_line_atoms(input, para, cache, &location)?;
            for (li, line) in atoms.iter().enumerate() {
                for atom in line {
                    if let LineAtom::TextBox(tb) = atom {
                        let seg = &cache.lines[li];
                        let extent = textbox_content_extent(&tb.inner_lines, &location)?;
                        let expected = textbox_expected_host_height(tb.height, extent)?;
                        if !line_matches_object_height(seg, expected) {
                            return Err(PdfError::InvalidCache {
                                detail: format!(
                                    "{location}/l{li}: text box host line vertsize {} / \
                                     textheight {} != expected {expected} \
                                     (max(box.height {}, content-extent {extent} + \
                                     2×{TEXTBOX_TEXT_MARGIN})) — unmeasured profile",
                                    seg.vertsize, seg.textheight, tb.height
                                ),
                            });
                        }
                    }
                }
            }
            Some(atoms)
        } else {
            None
        };

        // run 별 UTF-16 구간 (문자 스타일 매핑용 — 이미지 문단은 helper 가
        // 원자를 소유하므로 불요 + admitted 이미지에 NonTextRunDropped
        // 경고를 내지 않기 위해 건너뛴다).
        let run_spans = if line_atoms_override.is_none() {
            run_utf16_spans(para, warnings, &location)
        } else {
            Vec::new()
        };
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
            let line_atoms = match &line_atoms_override {
                Some(per_line) => per_line[line_idx].clone(),
                None => slice_line_runs(&utf16, &run_spans, start, end)
                    .into_iter()
                    .map(LineAtom::Text)
                    .collect(),
            };

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
                atoms: line_atoms,
                top_y: body_top + v,
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

/// 문단의 원자(텍스트 span·인라인 객체)를 캐시 줄별로 귀속하는 공용 엔진
/// (W2b §4 D2 · W4 §8g — 인라인 이미지·글상자가 이 skeleton 을 공유한다).
///
/// 단일 pass: run 순서대로 가시(UTF-16) 커서를 전진시키며 텍스트는 줄
/// 경계로 분할한다. `classify` 가 `Some((height, atom))` 을 돌려준 run 은
/// **폭 0 인 인라인 객체**로, 커서가 속한 줄에 문서 순서 그대로 삽입한다
/// (`height` = 그 객체의 host 줄 기대 높이 — 경계 판별용). `None` 은 텍스트
/// /marker 로 취급한다. 줄 구간 = non-last `[start, end)` · **last
/// `[start, end]`** (trailing 객체·객체-only 문단 귀속 — §4 H3).
///
/// fail-closed: 객체 offset 이 **첫 줄이 아닌** 줄 시작과 같으면 host 줄
/// 높이 profile 이 유일하게 매치되는 쪽으로 귀속하고, 0/2 매치는
/// `InvalidCache` (추측 금지 — W1b 영폭 축약 사각).
///
/// 전제: `cache.lines` 의 textpos 는 단조 (본문 경로는 `validate_textpos`
/// 가 선행 보장).
pub(crate) fn build_inline_object_line_atoms(
    para: &hwpforge_core::paragraph::Paragraph,
    cache: &hwpforge_core::layout::LayoutCache,
    location: &str,
    mut classify: impl FnMut(&hwpforge_core::run::Run) -> PdfResult<Option<(i32, LineAtom)>>,
) -> PdfResult<Vec<Vec<LineAtom>>> {
    let line_count = cache.lines.len();
    let mut atoms: Vec<Vec<LineAtom>> = vec![Vec::new(); line_count];
    if line_count == 0 {
        return Ok(atoms);
    }
    let starts: Vec<usize> = cache.lines.iter().map(|l| l.textpos as usize).collect();
    // 독립 리뷰 M1 상환: 동일 textpos 중복(영폭 줄)은 단일 pass 분할이
    // 뒤 줄을 건너뛰어 텍스트 꼬리를 무음 유실한다 — 객체 가드와
    // 대칭으로 fail-closed (validate_textpos 는 == 를 허용하므로 여기가
    // 유일 검출점).
    if starts.windows(2).any(|w| w[0] == w[1]) {
        return Err(PdfError::InvalidCache {
            detail: format!(
                "{location}: duplicate line textpos — zero-width line cannot be \
                 attributed in an inline-object paragraph"
            ),
        });
    }
    // offset 을 포함하는 줄 인덱스 (마지막 start <= offset).
    let line_of = |offset: usize| -> usize {
        match starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    };

    let mut cursor = 0usize;
    // 동일 boundary offset 다중 객체의 판별 결과 추적 — 서로 다른 후보
    // 줄로 갈리면 줄 순회 방출이 문서 순서를 뒤집는다 (r2 High#1).
    let mut boundary_target: Option<(usize, usize)> = None;
    for run in &para.runs {
        if let Some((height, atom)) = classify(run)? {
            let li = if let Some(i) = (1..line_count).find(|&i| starts[i] == cursor) {
                // 경계 (offset == non-first 줄 시작): 후보 = 줄 i−1(끝)·
                // 줄 i(시작). 높이-profile 삼중 일치가 **유일**한 쪽으로
                // 귀속 (캐시가 기록한 실측 — §7 v2), 0/2개 매치 =
                // fail-closed (추측 금지).
                let prev = line_matches_object_height(&cache.lines[i - 1], height);
                let next = line_matches_object_height(&cache.lines[i], height);
                let target = match (prev, next) {
                    (true, false) => i - 1,
                    (false, true) => i,
                    (false, false) => {
                        return Err(PdfError::InvalidCache {
                            detail: format!(
                                "{location}: inline object offset {cursor} on a line \
                                 boundary matches neither candidate line's height profile \
                                 (zero-match) — attribution ambiguous"
                            ),
                        });
                    }
                    (true, true) => {
                        return Err(PdfError::InvalidCache {
                            detail: format!(
                                "{location}: inline object offset {cursor} on a line \
                                 boundary matches both candidate lines' height profile \
                                 (two-match) — attribution ambiguous"
                            ),
                        });
                    }
                };
                if let Some((c, t)) = boundary_target {
                    if c == cursor && t != target {
                        return Err(PdfError::InvalidCache {
                            detail: format!(
                                "{location}: multiple inline objects at boundary offset \
                                 {cursor} resolve to different lines — document order \
                                 would invert across line traversal"
                            ),
                        });
                    }
                }
                boundary_target = Some((cursor, target));
                target
            } else {
                line_of(cursor)
            };
            atoms[li].push(atom);
            continue;
        }
        let Some(text) = run.content.plain_text() else {
            continue; // 0폭 marker 류 — 원자 없음 (admission 이 종류를 거른다).
        };
        let units: Vec<u16> = text.encode_utf16().collect::<Vec<u16>>();
        if units.is_empty() {
            continue;
        }
        let (rs, re) = (cursor, cursor + units.len());
        let mut li = line_of(rs);
        while li < line_count {
            let seg_start = rs.max(starts[li]);
            let seg_end = if li + 1 < line_count { re.min(starts[li + 1]) } else { re };
            if seg_start >= seg_end {
                break;
            }
            let slice = String::from_utf16(&units[seg_start - rs..seg_end - rs]).map_err(|_| {
                PdfError::InvalidCache {
                    detail: format!("{location}: line boundary splits a surrogate pair"),
                }
            })?;
            atoms[li].push(LineAtom::Text(LineRun { text: slice, char_shape: run.char_shape_id }));
            li += 1;
        }
        cursor = re;
    }
    Ok(atoms)
}

/// 인라인 이미지 문단의 원자를 캐시 줄별로 귀속한다 (W2b §4 D2).
///
/// [`build_inline_object_line_atoms`] 에 이미지 classifier(기대 높이 =
/// `image.height`)를 결선한 얇은 래퍼다.
pub(crate) fn build_inline_image_line_atoms(
    para: &hwpforge_core::paragraph::Paragraph,
    cache: &hwpforge_core::layout::LayoutCache,
    location: &str,
) -> PdfResult<Vec<Vec<LineAtom>>> {
    build_inline_object_line_atoms(para, cache, location, |run| {
        Ok(match &run.content {
            RunContent::Image(img) => Some((
                img.height.as_i32(),
                LineAtom::Image(LineImage {
                    canonical_key: img.path.clone(),
                    width: img.width.as_i32(),
                    height: img.height.as_i32(),
                }),
            )),
            _ => None,
        })
    })
}

/// 인라인 글상자 문단의 원자를 캐시 줄별로 귀속한다 (W4 §8g).
///
/// [`build_inline_object_line_atoms`] 에 글상자 classifier 를 결선한다 —
/// 각 글상자 원자의 host 줄 기대 높이 = `max(box.height, content_extent +
/// 상하 textMargin)` ([`textbox_expected_host_height`]). 앵커형(비-admitted)
/// 글상자 run 은 `None` 으로 무시하나(선행 admission 가드가 이미 그 문단을
/// 거부), 내부 캐시/중첩 객체 결손은 [`build_line_text_box`] 가 fail-closed.
pub(crate) fn build_inline_textbox_line_atoms(
    input: &PdfInput<'_>,
    para: &hwpforge_core::paragraph::Paragraph,
    cache: &hwpforge_core::layout::LayoutCache,
    location: &str,
) -> PdfResult<Vec<Vec<LineAtom>>> {
    build_inline_object_line_atoms(para, cache, location, |run| {
        if !is_admitted_inline_textbox(&run.content) {
            return Ok(None);
        }
        let RunContent::Control(ctrl) = &run.content else {
            return Ok(None); // is_admitted_inline_textbox 가 이미 Control 로 좁힘.
        };
        let tb = build_line_text_box(input, ctrl, location)?;
        let extent = textbox_content_extent(&tb.inner_lines, location)?;
        let expected = textbox_expected_host_height(tb.height, extent)?;
        Ok(Some((expected, LineAtom::TextBox(tb))))
    })
}

/// 글상자 내부 문단의 원자를 캐시 줄별로 귀속한다 (W5 w1a — §9g).
///
/// [`build_inline_object_line_atoms`] 에 **글상자-내부** classifier 를 결선한
/// 얇은 래퍼다. body 의 [`build_inline_image_line_atoms`] 와 달리 텍스트/
/// marker 외의 run 을 조용히 흘리지 않고 종류별로 **fail-closed** 분기한다:
///
/// - 글자취급 인라인 이미지(`placement == None` 인 레거시 인라인 기본값 또는
///   `treat_as_char`)는 host 줄 기대 높이 = `image.height` 인 원자로 귀속한다
///   — body 인라인 이미지와 동일 profile·admission 관례([`is_admitted_inline_image`]
///   가 body 에서, [`is_admitted_inline_textbox`] 의 `None` 허용이 글상자에서).
/// - **앵커형** 이미지(`treat_as_char == false`)는 `"anchored object inside
///   text box"` 로 거부한다 — W5 w1b(앵커 렌더) 대상임을 CLI cause 집계에서
///   식별 가능하게 kind 를 분리한다 (§9b w1b).
/// - 중첩 글상자·표·기타 컨트롤은 `"nested object inside text box"` 를 유지한다
///   (fixture 부재 — §8f ⑤).
///
/// 전제: `cache.lines` 의 textpos 는 단조 (호출부 [`build_line_text_box`] 가
/// `validate_textpos` 로 선행 보장).
pub(crate) fn build_textbox_inner_line_atoms(
    para: &hwpforge_core::paragraph::Paragraph,
    cache: &hwpforge_core::layout::LayoutCache,
    location: &str,
) -> PdfResult<Vec<Vec<LineAtom>>> {
    build_inline_object_line_atoms(para, cache, location, |run| match &run.content {
        RunContent::Image(img) if img.placement.as_ref().is_none_or(|p| p.treat_as_char) => {
            Ok(Some((
                img.height.as_i32(),
                LineAtom::Image(LineImage {
                    canonical_key: img.path.clone(),
                    width: img.width.as_i32(),
                    height: img.height.as_i32(),
                }),
            )))
        }
        RunContent::Image(_) => Err(PdfError::UnsupportedContent {
            kind: "anchored object inside text box",
            location: location.to_string(),
        }),
        other if other.plain_text().is_some() => Ok(None),
        _ => Err(PdfError::UnsupportedContent {
            kind: "nested object inside text box",
            location: location.to_string(),
        }),
    })
}

/// Core `Control::TextBox` 를 렌더용 [`LineTextBox`] 로 증류한다 (W4 §8g · W5 w1a).
///
/// 내부 문단들의 조판 캐시([`hwpforge_core::layout::LineSeg`])를 **그대로**
/// 나른 [`TextBoxLine`] 로 옮기고, 테두리·채움([`TextBoxStyle`])과 세로정렬을
/// 캐리한다. 내부 줄 원자는 body 공용 엔진([`build_textbox_inner_line_atoms`])
/// 으로 귀속하므로 **글자취급 인라인 이미지**를 담는다 (W5 w1a — §9g). 앵커형
/// 이미지·중첩 글상자/표·캐시 결손은 fail-closed (§8f ⑤).
fn build_line_text_box(
    input: &PdfInput<'_>,
    ctrl: &hwpforge_core::control::Control,
    location: &str,
) -> PdfResult<LineTextBox> {
    let hwpforge_core::control::Control::TextBox {
        paragraphs,
        width,
        height,
        style,
        text_vertical_align,
        ..
    } = ctrl
    else {
        return Err(PdfError::InternalInvariant {
            detail: format!("{location}: build_line_text_box called on non-TextBox control"),
        });
    };
    let mut inner_lines = Vec::new();
    for (pi, para) in paragraphs.iter().enumerate() {
        let inner_loc = format!("{location}/tb/p{pi}");
        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            return Err(PdfError::InvalidCache {
                detail: format!("{inner_loc}: text box inner paragraph has no layout cache"),
            });
        };
        let utf16: Vec<u16> = para.text_content().encode_utf16().collect();
        validate_textpos(cache, utf16.len(), &inner_loc)?;
        // W5 w1a (§9g): 내부 인라인 이미지를 body 공용 엔진으로 줄별 귀속한다
        // (visible textpos 축 — 디코더가 wire marker 유닛을 이미 정규화, 실측
        // [0, 23] = raw 31 − pic 8유닛). 앵커형/중첩은 helper 가 typed fail-closed.
        let atoms_per_line = build_textbox_inner_line_atoms(para, cache, &inner_loc)?;
        let alignment = input.styles.para_alignment(para.para_shape_id).unwrap_or(Alignment::Left);
        let line_count = cache.lines.len();
        for (li, (seg, line_atoms)) in cache.lines.iter().zip(atoms_per_line).enumerate() {
            // 이미지 지배 줄 삼중 일치 (body 와 동일 계약 — §4 D2): image.height
            // == vertsize == textheight. 불일치 = 미측정 profile → fail-closed.
            for atom in &line_atoms {
                if let LineAtom::Image(img) = atom {
                    if !line_matches_object_height(seg, img.height) {
                        return Err(PdfError::InvalidCache {
                            detail: format!(
                                "{inner_loc}/l{li}: inner image height {} != line vertsize {} / \
                                 textheight {} — unmeasured height profile",
                                img.height, seg.vertsize, seg.textheight
                            ),
                        });
                    }
                }
            }
            inner_lines.push(TextBoxLine {
                seg: *seg,
                atoms: line_atoms,
                alignment,
                is_last_line: li + 1 == line_count,
            });
        }
    }
    Ok(LineTextBox {
        width: width.as_i32(),
        height: height.as_i32(),
        style: style.as_ref().map(distill_textbox_style),
        vert_align: *text_vertical_align,
        inner_lines,
    })
}

/// Core `ShapeStyle` 에서 [`TextBoxStyle`] 로 원색·굵기만 증류한다
/// (dash/rotation 등은 Paint `Line` 이 실선만 그리므로 제외 — §8g w4).
fn distill_textbox_style(style: &hwpforge_core::control::ShapeStyle) -> TextBoxStyle {
    TextBoxStyle {
        line_color: style.line_color,
        fill_color: style.fill_color,
        line_width: style.line_width.map_or(0, |w| w.min(i32::MAX as u32) as i32),
    }
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

    // ── W4 w2: LineAtom::TextBox 구축/접근 계약 (w3 소비 형태) ──────

    /// 글상자 원자가 박스 크기·스타일·vertAlign 과 내부 줄(lineseg + 재귀
    /// 원자)을 손실 없이 나르고, 호스트 줄의 텍스트 run 으로 새지 않는지
    /// 잠근다 — w3 이 이 형태를 소비한다.
    #[test]
    fn line_text_box_carries_size_style_and_recursive_inner_atoms() {
        let inner_text = TextBoxLine {
            seg: seg(0, 0),
            atoms: vec![LineAtom::Text(LineRun {
                text: "글상자 안 텍스트".to_string(),
                char_shape: CharShapeIndex::new(0),
            })],
            alignment: Alignment::Left,
            is_last_line: false,
        };
        // 내부 v축은 문단 경계를 넘어 연속 (0 → 1000, §8a) — 두 번째 줄은
        // 이미지 원자 (재귀 표현).
        let inner_image = TextBoxLine {
            seg: seg(0, 1000),
            atoms: vec![LineAtom::Image(LineImage {
                canonical_key: "BinData/inner.png".to_string(),
                width: 500,
                height: 500,
            })],
            alignment: Alignment::Center,
            is_last_line: true,
        };
        let tb = LineTextBox {
            width: 22677,
            height: 11339,
            style: Some(TextBoxStyle {
                line_color: Some(Color::from_rgb(0, 0, 255)),
                fill_color: Some(Color::from_rgb(255, 244, 200)),
                line_width: 100,
            }),
            vert_align: hwpforge_foundation::VerticalAlign::Center,
            inner_lines: vec![inner_text, inner_image],
        };

        // 호스트 줄에 인라인 원자로 얹혀도 텍스트 run 으로 세지 않는다.
        let line = LaidLine {
            location: "s0/p0/l0".to_string(),
            atoms: vec![LineAtom::TextBox(tb)],
            top_y: 0,
            baseline_y: 850,
            line_box: LineBox { horzpos: 0, horzsize: 22677 },
            is_last_line: true,
            alignment: Alignment::Left,
        };
        assert_eq!(line.text_runs().count(), 0, "글상자는 호스트 줄의 텍스트 run 이 아니다");

        let LineAtom::TextBox(tb) = &line.atoms[0] else { panic!("expected TextBox atom") };
        assert_eq!(tb.width, 22677);
        assert_eq!(tb.height, 11339);
        assert_eq!(tb.vert_align, hwpforge_foundation::VerticalAlign::Center);
        let style = tb.style.as_ref().expect("style carried");
        assert_eq!(style.line_width, 100);
        assert_eq!(style.line_color, Some(Color::from_rgb(0, 0, 255)));
        assert_eq!(style.fill_color, Some(Color::from_rgb(255, 244, 200)));

        assert_eq!(tb.inner_lines.len(), 2);
        assert_eq!(tb.inner_lines[0].seg.vertpos, 0);
        assert_eq!(tb.inner_lines[1].seg.vertpos, 1000, "내부 v축은 문단 경계를 넘어 연속");
        assert!(matches!(tb.inner_lines[0].atoms[0], LineAtom::Text(_)));
        assert!(matches!(tb.inner_lines[1].atoms[0], LineAtom::Image(_)), "내부 원자는 재귀");
        assert!(!tb.inner_lines[0].is_last_line);
        assert!(tb.inner_lines[1].is_last_line);
        assert_eq!(tb.inner_lines[1].alignment, Alignment::Center);
    }

    // ── W2b c2: body admission·높이 profile (§4 D1/D2) ──────────

    fn img_seg(textpos: u32, vertpos: i32) -> LineSeg {
        LineSeg { vertsize: 2000, textheight: 2000, ..seg(textpos, vertpos) }
    }

    #[test]
    fn admitted_inline_image_paragraph_replays_with_image_atom() {
        let mut para = Paragraph::with_runs(
            vec![Run::text("본문", CharShapeIndex::new(0)), inline_img("BinData/ok.png")],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let doc = doc_of(vec![para]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let kinds = atom_kinds(&layout.pages[0].lines[0].atoms);
        assert_eq!(kinds, ["text", "image"]);
        assert!(
            !layout.warnings.iter().any(|w| matches!(w, PdfWarning::NonTextRunDropped { .. })),
            "admitted 이미지는 드롭 경고 대상이 아니다: {:?}",
            layout.warnings
        );
    }

    #[test]
    fn image_height_mismatch_is_invalid_cache() {
        // 이미지 2000 vs 줄 1000 — 미측정 높이 profile 은 fail-closed.
        let mut para = Paragraph::with_runs(
            vec![Run::text("본문", CharShapeIndex::new(0)), inline_img("BinData/tall.png")],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let doc = doc_of(vec![para]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("mismatch");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("height")),
            "{err:?}"
        );
    }

    #[test]
    fn anchored_and_placement_none_images_stay_rejected() {
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::placement::ObjectPlacement;
        // treat_as_char=false (앵커형).
        let mut anchored = Image::new(
            "BinData/anch.png",
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            ImageFormat::Png,
        );
        let mut placement = ObjectPlacement::legacy_inline_defaults();
        placement.treat_as_char = false;
        anchored.placement = Some(placement);
        // placement=None (보수 거부).
        let bare = Image::new(
            "BinData/bare.png",
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            ImageFormat::Png,
        );
        for image in [anchored, bare] {
            let mut para = Paragraph::with_runs(
                vec![
                    Run::text("본문", CharShapeIndex::new(0)),
                    Run {
                        content: RunContent::Image(image),
                        char_shape_id: CharShapeIndex::new(0),
                    },
                ],
                ParaShapeIndex::new(0),
            );
            para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
            let doc = doc_of(vec![para]);
            let err = replay(&doc, &PdfOptions::default()).expect_err("rejected");
            assert!(
                matches!(&err, PdfError::InvalidCache { detail } if detail.contains("not renderable")),
                "{err:?}"
            );
        }
    }

    // ── W4 w3: 인라인 글상자 admission·host predicate·fail-closed ────────

    /// vertsize·textheight 를 지정하는 host 줄 seg (원자 지배 profile 검사용).
    fn sized_seg(textpos: u32, vertpos: i32, size: i32) -> LineSeg {
        LineSeg { vertsize: size, textheight: size, ..seg(textpos, vertpos) }
    }

    /// 글상자 내부 문단 하나 — 조판 캐시 + 텍스트 run (텍스트 길이 ≥ 최대
    /// textpos 여야 validate_textpos 통과).
    fn tb_inner(segs: Vec<LineSeg>, text: &str) -> Paragraph {
        let mut p = Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(segs));
        p
    }

    fn textbox_run(
        width: i32,
        height: i32,
        valign: hwpforge_foundation::VerticalAlign,
        inner: Vec<Paragraph>,
    ) -> Run {
        use hwpforge_core::control::Control;
        Run {
            content: RunContent::Control(Box::new(Control::TextBox {
                paragraphs: inner,
                width: HwpUnit::new(width).unwrap(),
                height: HwpUnit::new(height).unwrap(),
                placement: None,
                caption: None,
                style: None,
                text_vertical_align: valign,
            })),
            char_shape_id: CharShapeIndex::new(0),
        }
    }

    /// 순수 산술 잠금 — fixture 실측(§8f ①): basic 11339(=box.height) ·
    /// valign 7087(=box.height) · overflow 12766(=extent 12200 + 566 > box 4252).
    #[test]
    fn textbox_expected_host_height_matches_fixture_measurements() {
        use hwpforge_foundation::VerticalAlign;
        // basic: box 11339 이 content-extent 5800 + 566 을 이겨 그대로.
        assert_eq!(textbox_expected_host_height(11339, 5800).unwrap(), 11339);
        // valign: box 7087 이 content 1000 + 566 을 이김.
        assert_eq!(textbox_expected_host_height(7087, 1000).unwrap(), 7087);
        // overflow: content 12200 + 566 = 12766 이 box 4252 를 넘겨 확장.
        assert_eq!(textbox_expected_host_height(4252, 12200).unwrap(), 12766);
        // content_extent = 내부 줄 max(vertpos + vertsize).
        let inner = vec![
            TextBoxLine {
                seg: sized_seg(0, 3200, 1000),
                atoms: vec![],
                alignment: Alignment::Left,
                is_last_line: false,
            },
            TextBoxLine {
                seg: sized_seg(0, 4800, 1000),
                atoms: vec![],
                alignment: Alignment::Left,
                is_last_line: true,
            },
        ];
        assert_eq!(textbox_content_extent(&inner, "t").unwrap(), 5800);
        let _ = VerticalAlign::Top;
    }

    /// basic 프로파일: 2 내부 문단(3줄 + 1줄)·연속 v축, host 11339 매치 →
    /// admission 통과, 원자는 TextBox, LineTextBox 가 크기·valign·내부 줄을
    /// 손실 없이 나른다.
    #[test]
    fn textbox_basic_admits_and_carries_inner_lines() {
        use hwpforge_foundation::VerticalAlign;
        let inner = vec![
            tb_inner(vec![seg(0, 0), seg(1, 1600), seg(2, 3200)], "가나다"),
            tb_inner(vec![seg(0, 4800)], "가"),
        ];
        // content-extent = 4800 + 1000 = 5800 → expected host = max(11339, 6366) = 11339.
        let mut host = Paragraph::with_runs(
            vec![textbox_run(22677, 11339, VerticalAlign::Top, inner)],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 11339)]));
        let doc = doc_of(vec![host]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let atoms = &layout.pages[0].lines[0].atoms;
        assert_eq!(atom_kinds(atoms), ["textbox"]);
        let LineAtom::TextBox(tb) = &atoms[0] else { panic!("expected textbox atom") };
        assert_eq!(tb.width, 22677);
        assert_eq!(tb.height, 11339);
        assert_eq!(tb.vert_align, VerticalAlign::Top);
        // 내부 v축은 문단 경계를 넘어 연속 (0/1600/3200 → 4800).
        assert_eq!(tb.inner_lines.len(), 4);
        assert_eq!(tb.inner_lines[0].seg.vertpos, 0);
        assert_eq!(tb.inner_lines[3].seg.vertpos, 4800);
        // is_last_line 은 내부 **문단마다** 계산된다 (JUSTIFY 마지막 줄
        // 규칙): p0 중간 줄=false · p0 마지막 줄=true · p1 유일 줄=true.
        assert!(!tb.inner_lines[1].is_last_line, "p0 중간 줄은 마지막 아님");
        assert!(tb.inner_lines[2].is_last_line, "p0 마지막 줄 = 문단 마지막(비양쪽정렬)");
        assert!(tb.inner_lines[3].is_last_line, "p1 마지막 줄");
        assert!(matches!(tb.inner_lines[0].atoms[0], LineAtom::Text(_)));
        assert!(tb.style.is_none(), "style None 은 그대로 None");
    }

    /// valign(box 7087) 과 overflow(box 4252, content 12200) 두 프로파일이
    /// 모두 host predicate 를 통과한다.
    #[test]
    fn textbox_valign_and_overflow_profiles_admit() {
        use hwpforge_foundation::VerticalAlign;
        // valign: 단일 줄 content 1000 → expected 7087 (= box.height).
        let mut valign_host = Paragraph::with_runs(
            vec![textbox_run(
                17008,
                7087,
                VerticalAlign::Center,
                vec![tb_inner(vec![seg(0, 0)], "가")],
            )],
            ParaShapeIndex::new(0),
        );
        valign_host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 3200, 7087)]));
        let layout = replay(&doc_of(vec![valign_host]), &PdfOptions::default()).expect("valign");
        let LineAtom::TextBox(tb) = &layout.pages[0].lines[0].atoms[0] else { panic!() };
        assert_eq!(tb.vert_align, VerticalAlign::Center);

        // overflow: content-extent 12200 → expected 12766 (> box 4252).
        let mut ovf_host = Paragraph::with_runs(
            vec![textbox_run(
                17008,
                4252,
                VerticalAlign::Top,
                vec![tb_inner(vec![sized_seg(0, 11200, 1000)], "가")],
            )],
            ParaShapeIndex::new(0),
        );
        ovf_host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 12766)]));
        let layout = replay(&doc_of(vec![ovf_host]), &PdfOptions::default()).expect("overflow");
        assert_eq!(atom_kinds(&layout.pages[0].lines[0].atoms), ["textbox"]);
    }

    /// host 줄 vertsize/textheight 가 기대값과 다르면 미측정 profile →
    /// InvalidCache fail-closed (어느 값도 임의 우선하지 않는다).
    #[test]
    fn textbox_host_height_mismatch_is_invalid_cache() {
        use hwpforge_foundation::VerticalAlign;
        let mut host = Paragraph::with_runs(
            vec![textbox_run(
                17008,
                7087,
                VerticalAlign::Top,
                vec![tb_inner(vec![seg(0, 0)], "가")],
            )],
            ParaShapeIndex::new(0),
        );
        // expected = 7087 이지만 host vertsize=7000 → 불일치.
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7000)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("mismatch");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("text box host line")),
            "{err:?}"
        );
    }

    /// 글상자 내부 글자취급 인라인 이미지는 admission 통과 — 텍스트+이미지
    /// 혼합 줄에서 이미지가 지배 줄(vertsize==textheight==3402)에 문서 순서
    /// 그대로 귀속되고, 가시 축([0,3)/[3,4]) 텍스트 분할이 정확하다 (W5 w1a).
    #[test]
    fn textbox_inner_inline_image_admits_and_attributes() {
        use hwpforge_foundation::VerticalAlign;
        let mut inner = Paragraph::with_runs(
            vec![
                Run::text("앞", CharShapeIndex::new(0)),
                img_with_height("BinData/i.png", 3402),
                Run::text("뒤글자", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        // 줄 0 이미지 지배(3402), 줄 1 순수 텍스트. content-extent = max(3402,
        // 4002+1000) = 5002 → expected host = max(7087, 5568) = 7087.
        inner.layout_cache = Some(LayoutCache::new(vec![tall_seg(0, 0, 3402), seg(3, 4002)]));
        let mut host = Paragraph::with_runs(
            vec![textbox_run(17008, 7087, VerticalAlign::Top, vec![inner])],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7087)]));
        let layout =
            replay(&doc_of(vec![host]), &PdfOptions::default()).expect("admit inner image");
        let LineAtom::TextBox(tb) = &layout.pages[0].lines[0].atoms[0] else {
            panic!("expected textbox atom")
        };
        assert_eq!(tb.inner_lines.len(), 2);
        assert_eq!(atom_kinds(&tb.inner_lines[0].atoms), ["text", "image", "text"]);
        assert_eq!(atom_kinds(&tb.inner_lines[1].atoms), ["text"]);
        let LineAtom::Text(a) = &tb.inner_lines[0].atoms[0] else { panic!() };
        assert_eq!(a.text, "앞");
        let LineAtom::Image(img) = &tb.inner_lines[0].atoms[1] else { panic!() };
        assert_eq!((img.width, img.height), (2000, 3402));
        assert_eq!(img.canonical_key, "BinData/i.png");
        let LineAtom::Text(b) = &tb.inner_lines[0].atoms[2] else { panic!() };
        assert_eq!(b.text, "뒤글");
        let LineAtom::Text(c) = &tb.inner_lines[1].atoms[0] else { panic!() };
        assert_eq!(c.text, "자");
        // 내부 인라인 이미지는 드롭 경고 대상이 아니다.
        assert!(!layout.warnings.iter().any(|w| matches!(w, PdfWarning::NonTextRunDropped { .. })));
    }

    /// 글상자 내부 **앵커형**(treat_as_char=false) 이미지는 W5 w1b(앵커 렌더)
    /// 대상 — CLI cause 집계에서 식별 가능하게 typed kind 로 분리 거부한다.
    #[test]
    fn textbox_inner_anchored_image_is_typed_rejection() {
        use hwpforge_foundation::VerticalAlign;
        let mut inner = Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0)), anchored_img("BinData/anc.png")],
            ParaShapeIndex::new(0),
        );
        inner.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let mut host = Paragraph::with_runs(
            vec![textbox_run(17008, 7087, VerticalAlign::Top, vec![inner])],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7087)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("anchored inner");
        assert!(
            matches!(
                &err,
                PdfError::UnsupportedContent { kind: "anchored object inside text box", .. }
            ),
            "{err:?}"
        );
    }

    /// 중첩 글상자(글상자 안 글상자)·표·기타 컨트롤은 "nested object inside
    /// text box" 를 유지한다 (fixture 부재 — §8f ⑤, W5 w1a 무관).
    #[test]
    fn textbox_nested_control_is_fail_closed() {
        use hwpforge_foundation::VerticalAlign;
        let mut inner = Paragraph::with_runs(
            vec![textbox_run(
                8000,
                4000,
                VerticalAlign::Top,
                vec![tb_inner(vec![seg(0, 0)], "가")],
            )],
            ParaShapeIndex::new(0),
        );
        inner.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let mut host = Paragraph::with_runs(
            vec![textbox_run(17008, 7087, VerticalAlign::Top, vec![inner])],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7087)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("nested");
        assert!(
            matches!(
                &err,
                PdfError::UnsupportedContent { kind: "nested object inside text box", .. }
            ),
            "{err:?}"
        );
    }

    /// 내부 이미지가 host 줄 profile 과 다르면(이미지 3402 vs 줄 1000) 미측정
    /// → InvalidCache fail-closed (body 와 동일 삼중 일치 계약).
    #[test]
    fn textbox_inner_image_height_mismatch_is_invalid_cache() {
        use hwpforge_foundation::VerticalAlign;
        let mut inner = Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0)), img_with_height("BinData/m.png", 3402)],
            ParaShapeIndex::new(0),
        );
        // 줄 vertsize/textheight = 1000 (seg 기본) ≠ 이미지 3402.
        inner.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let mut host = Paragraph::with_runs(
            vec![textbox_run(17008, 7087, VerticalAlign::Top, vec![inner])],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7087)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("mismatch");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("inner image height")),
            "{err:?}"
        );
    }

    /// 앵커형(treat_as_char=false) 글상자는 admission 거부 (앵커 렌더 = W5).
    #[test]
    fn anchored_textbox_stays_rejected() {
        use hwpforge_core::control::Control;
        use hwpforge_core::placement::ObjectPlacement;
        use hwpforge_foundation::VerticalAlign;
        let mut placement = ObjectPlacement::legacy_inline_defaults();
        placement.treat_as_char = false;
        let run = Run {
            content: RunContent::Control(Box::new(Control::TextBox {
                paragraphs: vec![tb_inner(vec![seg(0, 0)], "가")],
                width: HwpUnit::new(17008).unwrap(),
                height: HwpUnit::new(7087).unwrap(),
                placement: Some(placement),
                caption: None,
                style: None,
                text_vertical_align: VerticalAlign::Top,
            })),
            char_shape_id: CharShapeIndex::new(0),
        };
        let mut host = Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
        host.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 1600, 7087)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("anchored");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("not renderable")),
            "{err:?}"
        );
    }

    /// 한 문단에 이미지와 글상자가 섞이면 미측정 profile → fail-closed.
    #[test]
    fn mixed_image_and_textbox_is_invalid_cache() {
        use hwpforge_foundation::VerticalAlign;
        let mut host = Paragraph::with_runs(
            vec![
                inline_img("BinData/x.png"),
                textbox_run(17008, 7087, VerticalAlign::Top, vec![tb_inner(vec![seg(0, 0)], "가")]),
            ],
            ParaShapeIndex::new(0),
        );
        host.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let err = replay(&doc_of(vec![host]), &PdfOptions::default()).expect_err("mixed");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("mixes")),
            "{err:?}"
        );
    }

    // ── W2b c1: build_inline_image_line_atoms 귀속 edge (§4 D2) ─────────

    fn inline_img(key: &str) -> Run {
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::placement::ObjectPlacement;
        let mut image = Image::new(
            key,
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            ImageFormat::Png,
        );
        image.placement = Some(ObjectPlacement::legacy_inline_defaults());
        Run { content: RunContent::Image(image), char_shape_id: CharShapeIndex::new(0) }
    }

    /// 앵커형(treat_as_char=false) 이미지 — 글상자 내부에서 W5 w1b 대상.
    fn anchored_img(key: &str) -> Run {
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::placement::ObjectPlacement;
        let mut image = Image::new(
            key,
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            ImageFormat::Png,
        );
        let mut placement = ObjectPlacement::legacy_inline_defaults();
        placement.treat_as_char = false;
        image.placement = Some(placement);
        Run { content: RunContent::Image(image), char_shape_id: CharShapeIndex::new(0) }
    }

    fn atom_kinds(atoms: &[LineAtom]) -> Vec<&'static str> {
        atoms
            .iter()
            .map(|a| match a {
                LineAtom::Text(_) => "text",
                LineAtom::Image(_) => "image",
                LineAtom::TextBox(_) => "textbox",
            })
            .collect()
    }

    #[test]
    fn atoms_mid_line_image_splits_text_in_document_order() {
        // "가나다" + 이미지 + "라마바사아자차" — 줄 [0, 6): 이미지는
        // 커서 3 에서 줄 0 중간, 텍스트는 경계 6 에서 분할.
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나다", CharShapeIndex::new(0)),
                inline_img("BinData/a.png"),
                Run::text("라마바사아자차", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(6, 1600)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["text", "image", "text"]);
        assert_eq!(atom_kinds(&atoms[1]), ["text"]);
        let LineAtom::Text(t0) = &atoms[0][0] else { panic!() };
        assert_eq!(t0.text, "가나다");
        let LineAtom::Text(t2) = &atoms[0][2] else { panic!() };
        assert_eq!(t2.text, "라마바");
        let LineAtom::Text(t1) = &atoms[1][0] else { panic!() };
        assert_eq!(t1.text, "사아자차");
    }

    /// 글상자-내부 helper 는 body 와 **동일 축(가시 textpos)** 으로 분할한다.
    /// textbox_inline_image-base fixture 실측 그대로: "그림 앞 "(5) + 이미지 +
    /// " 그림 뒤 …" 를 줄 경계 [0,23)/[23,42] 로 자른다 (디코드 캐시 실측
    /// [0, 23] = raw wire 31 − pic 8유닛). 축이 어긋나면(원시 31 로 자르면)
    /// 이 정확 텍스트 대조가 큰 소리로 실패한다.
    #[test]
    fn textbox_inner_atoms_split_matches_fixture_visible_axis() {
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("그림 앞 ", CharShapeIndex::new(0)),
                img_with_height("BinData/a.png", 3402),
                Run::text(
                    " 그림 뒤 — 이 문장은 글상자 폭에서 줄이 감기도록 길게 씁니다.",
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![tall_seg(0, 0, 3402), seg(23, 4002)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_textbox_inner_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["text", "image", "text"]);
        assert_eq!(atom_kinds(&atoms[1]), ["text"]);
        let LineAtom::Text(t0) = &atoms[0][0] else { panic!() };
        assert_eq!(t0.text, "그림 앞 ");
        let LineAtom::Text(t2) = &atoms[0][2] else { panic!() };
        assert_eq!(t2.text, " 그림 뒤 — 이 문장은 글상자 ");
        let LineAtom::Text(t1) = &atoms[1][0] else { panic!() };
        assert_eq!(t1.text, "폭에서 줄이 감기도록 길게 씁니다.");
    }

    #[test]
    fn atoms_trailing_image_belongs_to_last_line_inclusive() {
        // 텍스트 4유닛 + 문단 끝 이미지 (offset 4 == 마지막 줄 end) —
        // last 줄 [start, end] inclusive 귀속 (§4 H3).
        let mut para = Paragraph::with_runs(
            vec![Run::text("텍스트끝", CharShapeIndex::new(0)), inline_img("BinData/t.png")],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["text", "image"]);
    }

    #[test]
    fn atoms_image_only_paragraph_lands_on_first_line() {
        let mut para =
            Paragraph::with_runs(vec![inline_img("BinData/only.png")], ParaShapeIndex::new(0));
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["image"]);
    }

    #[test]
    fn atoms_leading_image_on_first_line_start_is_allowed() {
        // offset 0 == 첫 줄 시작 — 첫 줄은 축약 모호가 없다 (native
        // 불변식 to_wire(0)=0).
        let mut para = Paragraph::with_runs(
            vec![inline_img("BinData/lead.png"), Run::text("본문", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["image", "text"]);
    }

    #[test]
    fn atoms_image_on_non_first_line_start_is_invalid_cache() {
        // 이미지 offset 3 == 줄 2 시작 3 — W1b 영폭 축약상 앞/뒤 줄
        // 귀속 불가 → InvalidCache (§4 H3/H4 선계약).
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나다", CharShapeIndex::new(0)),
                inline_img("BinData/b.png"),
                Run::text("라마바", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(3, 1600)]));
        let cache = para.layout_cache.clone().unwrap();
        let err = build_inline_image_line_atoms(&para, &cache, "t").expect_err("ambiguous");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("ambiguous")),
            "{err:?}"
        );
    }

    fn tall_seg(textpos: u32, vertpos: i32, h: i32) -> LineSeg {
        LineSeg { vertsize: h, textheight: h, ..seg(textpos, vertpos) }
    }

    fn img_with_height(key: &str, h: i32) -> Run {
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::placement::ObjectPlacement;
        let mut image = Image::new(
            key,
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(h).unwrap(),
            ImageFormat::Png,
        );
        image.placement = Some(ObjectPlacement::legacy_inline_defaults());
        Run { content: RunContent::Image(image), char_shape_id: CharShapeIndex::new(0) }
    }

    #[test]
    fn atoms_boundary_unique_next_attributes_to_image_line() {
        // 대표 fixture r1c1 모양: "큰 그림 "(5) + 이미지(6000), 줄 2 가
        // tp=5·h=6000 — 유일 next 매치 → 줄 2 귀속 (렌더 성공 경로).
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("큰 그림 ", CharShapeIndex::new(0)),
                img_with_height("BinData/t.png", 6000),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), tall_seg(5, 1600, 6000)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("unique next");
        assert_eq!(atom_kinds(&atoms[0]), ["text"]);
        assert_eq!(atom_kinds(&atoms[1]), ["image"]);
    }

    #[test]
    fn atoms_boundary_unique_prev_attributes_to_previous_line() {
        // 이전 줄이 이미지 지배(2000)·다음 줄 텍스트(1000) — 유일 prev.
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나", CharShapeIndex::new(0)),
                img_with_height("BinData/p.png", 2000),
                Run::text("다라마", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![tall_seg(0, 0, 2000), seg(2, 2600)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("unique prev");
        assert_eq!(atom_kinds(&atoms[0]), ["text", "image"]);
        assert_eq!(atom_kinds(&atoms[1]), ["text"]);
    }

    #[test]
    fn atoms_boundary_two_match_is_invalid_cache() {
        // image=1000 · 양 줄 다 1000 — 2-candidate = 판별 불가.
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나다", CharShapeIndex::new(0)),
                img_with_height("BinData/two.png", 1000),
                Run::text("라마바", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(3, 1600)]));
        let cache = para.layout_cache.clone().unwrap();
        let err = build_inline_image_line_atoms(&para, &cache, "t").expect_err("two-match");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("two-match")),
            "{err:?}"
        );
    }

    #[test]
    fn atoms_boundary_multi_image_same_target_preserves_order() {
        // 같은 경계의 이미지 2개가 모두 next(6000) 로 판별 — 순서 보존.
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나다", CharShapeIndex::new(0)),
                img_with_height("BinData/one.png", 6000),
                img_with_height("BinData/two.png", 6000),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), tall_seg(3, 1600, 6000)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("same target");
        assert_eq!(atom_kinds(&atoms[1]), ["image", "image"]);
        let LineAtom::Image(first) = &atoms[1][0] else { panic!() };
        assert_eq!(first.canonical_key, "BinData/one.png");
    }

    #[test]
    fn atoms_boundary_multi_image_cross_target_is_invalid_cache() {
        // 같은 경계에서 이미지 1=next(6000)·이미지 2=prev(1000) — 줄 순회
        // 방출이 문서 순서를 뒤집으므로 거부 (r2 High#1).
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가나다", CharShapeIndex::new(0)),
                img_with_height("BinData/next.png", 6000),
                img_with_height("BinData/prev.png", 1000),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), tall_seg(3, 1600, 6000)]));
        let cache = para.layout_cache.clone().unwrap();
        let err = build_inline_image_line_atoms(&para, &cache, "t").expect_err("cross target");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("different lines")),
            "{err:?}"
        );
    }

    #[test]
    fn atoms_duplicate_line_textpos_is_invalid_cache() {
        // 독립 리뷰 M1: starts=[0,3,3] — 단일 pass 는 줄 2 를 건너뛰어
        // 꼬리 텍스트를 유실한다 → fail-closed.
        let mut para = Paragraph::with_runs(
            vec![Run::text("가나다라마바", CharShapeIndex::new(0)), inline_img("BinData/dup.png")],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(3, 1600), seg(3, 3200)]));
        let cache = para.layout_cache.clone().unwrap();
        let err = build_inline_image_line_atoms(&para, &cache, "t").expect_err("dup starts");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("duplicate")),
            "{err:?}"
        );
    }

    #[test]
    fn atoms_multiple_images_same_offset_preserve_run_order() {
        let mut para = Paragraph::with_runs(
            vec![
                Run::text("가", CharShapeIndex::new(0)),
                inline_img("BinData/one.png"),
                inline_img("BinData/two.png"),
                Run::text("나", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let cache = para.layout_cache.clone().unwrap();
        let atoms = build_inline_image_line_atoms(&para, &cache, "t").expect("attribution");
        assert_eq!(atom_kinds(&atoms[0]), ["text", "image", "image", "text"]);
        let LineAtom::Image(first) = &atoms[0][1] else { panic!() };
        let LineAtom::Image(second) = &atoms[0][2] else { panic!() };
        assert_eq!(first.canonical_key, "BinData/one.png");
        assert_eq!(second.canonical_key, "BinData/two.png");
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
        assert_eq!(lines[0].text_runs().next().unwrap().text, "가나다 ");
        assert_eq!(lines[1].text_runs().next().unwrap().text, "라마바");
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
                lines[0].text_runs().next().unwrap().text.clone()
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
    fn new_number_same_page_last_wins() {
        use hwpforge_core::control::{Control, NewNumberKind};
        // 같은 쪽 다중 재시작 = 문서 순서 last-wins (Page kind 만 —
        // allowlist 원자).
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
        assert_eq!(texts, vec!["9", "10"], "last-wins");
    }

    #[test]
    fn non_page_new_number_is_rejected_not_silently_ignored() {
        use hwpforge_core::control::{Control, NewNumberKind};
        // W1b (§1g v5 변경 1): replay 소비자가 없는 NewNumber kind 는
        // "무시"가 아니라 InvalidCache — 소비자와 allowlist 의 match 가
        // 갈라지면 무음 드롭이 부활한다 (R4 Critical).
        let mut p = Paragraph::with_runs(
            vec![
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
        doc.add_section(Section::with_paragraphs(vec![p], PageSettings::a4()));
        let doc = doc.validate().expect("validate");
        let err = replay(&doc, &PdfOptions::default()).expect_err("must reject");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("not renderable")),
            "got {err:?}"
        );
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
    fn cacheless_paragraph_with_restart_surfaces_page_event_lost() {
        // 독립 리뷰 High #3: 스킵 문단의 이벤트는 근사 앵커로 날조하지 않고
        // 유실을 특정 경고로 표면화한다.
        use hwpforge_core::control::{Control, NewNumberKind};
        let mut cacheless = Paragraph::with_runs(
            vec![
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number: 7 },
                    CharShapeIndex::new(0),
                ),
                Run::text("캐시 없는 문단", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        cacheless.layout_cache = None;
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![para_with_cache("본문", vec![seg(0, 0)]), cacheless],
            PageSettings::a4(),
        ));
        doc.sections_mut()[0].page_number = Some(bottom_digit());
        let doc = doc.validate().expect("validate");
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        assert!(
            layout.warnings.iter().any(|w| matches!(w, PdfWarning::PageEventLost { .. })),
            "{:?}",
            layout.warnings
        );
        // 이벤트는 적용되지 않는다 (앵커 불가 — 기존 번호 유지).
        assert_eq!(layout.pages[0].page_number.as_ref().unwrap().text, "1");
    }

    #[test]
    fn table_host_with_image_fails_closed_instead_of_silent_discard() {
        // 이미지 에픽 게이트 2 Critical#5: [Image, Table] host 는 이전엔 표만
        // 그리고 이미지를 경고 없이 버렸다 — fail-closed 로 잠근다.
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::table::Table;
        let img = Image::new(
            "BinData/x.png".to_string(),
            HwpUnit::from_pt(10.0).expect("unit"),
            HwpUnit::from_pt(10.0).expect("unit"),
            ImageFormat::Png,
        );
        let mut p = Paragraph::with_runs(
            vec![
                Run::image(img, CharShapeIndex::new(0)),
                Run::table(
                    Table::new(vec![TableRow::new(vec![cell_with_cache("셀")])]),
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        p.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let doc = doc_of(vec![para_with_cache("본문", vec![seg(0, 0)]), p]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("must fail closed");
        assert!(
            matches!(
                err,
                PdfError::UnsupportedContent {
                    kind: "non-text content in table-host paragraph",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn collect_page_events_sees_markers_in_table_host_paragraph() {
        // 독립 리뷰 High #4: 이벤트 수집은 admission/표 분기와 분리돼
        // [marker, Table] host 문단에서도 재시작·감춤을 놓치지 않는다
        // (host 쪽 앵커 부착은 replay_section 표 분기의 직선 코드).
        use hwpforge_core::control::{Control, NewNumberKind};
        use hwpforge_core::table::Table;
        let para = Paragraph::with_runs(
            vec![
                Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number: 7 },
                    CharShapeIndex::new(0),
                ),
                Run::control(
                    Control::PageHiding {
                        hide_header: false,
                        hide_footer: false,
                        hide_master_page: false,
                        hide_border: false,
                        hide_fill: false,
                        hide_page_num: true,
                    },
                    CharShapeIndex::new(0),
                ),
                Run::table(Table::new(Vec::new()), CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        let (restarts, hide_marks) = collect_page_events(&para);
        assert_eq!(restarts, vec![(0, 7)]);
        assert_eq!(hide_marks.len(), 1);
        assert!(hide_marks[0].1.page_num && !hide_marks[0].1.header);
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

    // ── W3 w2: 셀 인라인 이미지 admission (§7 v2 D1·replay) ─────

    fn img_cell_table(paragraphs: Vec<Paragraph>) -> Table {
        Table::new(vec![TableRow::new(vec![TableCell::new(
            paragraphs,
            HwpUnit::from_pt(100.0).unwrap(),
        )])])
        .with_layout_cache(hwpforge_core::table::TableLayoutCache::new(None, true))
    }

    #[test]
    fn cell_extent_uses_max_bottom_not_last_line() {
        // W3 w3 (§7 r2 fold-in): 앞줄 큰 이미지 bottom(5000)이 마지막
        // 줄 bottom(2600)보다 큰 캐시 — extent 는 max-bottom 이어야
        // 행높이 누락이 없다 (정상 캐시는 last==max 라 동치).
        let mut cell_para = Paragraph::with_runs(
            vec![Run::text("가나다 라마", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![
            tall_seg(0, 0, 5000),
            seg(4, 1600), // bottom 2600 < 5000
        ]));
        let cell = TableCell::new(vec![cell_para], HwpUnit::from_pt(100.0).unwrap());
        let doc = doc_of(vec![para_with_cache("x", vec![seg(0, 0)])]);
        let input = PdfInput { document: &doc, styles: &NoopStyles };
        let extent = crate::source::table::cell_content_extent(&input, &cell, "t", 0)
            .expect("extent")
            .expect("some");
        assert_eq!(extent, 5000, "max-bottom (구현 전엔 last=2600)");
    }

    #[test]
    fn cell_inline_image_paragraph_replays_with_image_atom() {
        let mut cell_para = Paragraph::with_runs(
            vec![Run::text("셀 ", CharShapeIndex::new(0)), inline_img("BinData/c.png")],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let cell_line =
            layout.pages[0].lines.iter().find(|l| l.location.contains("r0c0")).expect("cell line");
        assert_eq!(atom_kinds(&cell_line.atoms), ["text", "image"]);
    }

    #[test]
    fn cell_image_only_paragraph_reaches_helper() {
        // 빈 text_content 라도 admitted 이미지 문단은 continue 전에
        // helper 로 분기해야 한다 (r2 #4).
        let mut cell_para =
            Paragraph::with_runs(vec![inline_img("BinData/only.png")], ParaShapeIndex::new(0));
        cell_para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let layout = replay(&doc, &PdfOptions::default()).expect("replay");
        let cell_line =
            layout.pages[0].lines.iter().find(|l| l.location.contains("r0c0")).expect("cell line");
        assert_eq!(atom_kinds(&cell_line.atoms), ["image"]);
    }

    #[test]
    fn cell_anchored_image_stays_rejected() {
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::placement::ObjectPlacement;
        let mut anchored = Image::new(
            "BinData/anch.png",
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            hwpforge_foundation::HwpUnit::new(2000).unwrap(),
            ImageFormat::Png,
        );
        let mut placement = ObjectPlacement::legacy_inline_defaults();
        placement.treat_as_char = false;
        anchored.placement = Some(placement);
        let mut cell_para = Paragraph::with_runs(
            vec![Run {
                content: RunContent::Image(anchored),
                char_shape_id: CharShapeIndex::new(0),
            }],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("anchored rejected");
        assert!(
            matches!(&err, PdfError::UnsupportedContent { kind, .. }
                if *kind == "non-text content in table cell"),
            "{err:?}"
        );
    }

    #[test]
    fn cell_textbox_stays_fail_closed() {
        // 셀 내부 글상자는 W4 범위 밖 (fixture 부재 — §8g 백로그): body 는
        // 열렸어도 셀 admission(scan_cell_contents)은 닫힌 채여야 한다.
        use hwpforge_foundation::VerticalAlign;
        let mut cell_para = Paragraph::with_runs(
            vec![textbox_run(
                17008,
                7087,
                VerticalAlign::Top,
                vec![tb_inner(vec![seg(0, 0)], "가")],
            )],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 0, 7087)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("cell textbox rejected");
        assert!(
            matches!(&err, PdfError::UnsupportedContent { kind, .. }
                if *kind == "non-text content in table cell"),
            "{err:?}"
        );
    }

    /// W5 w1a 회귀 잠금: 글상자 **내부** 인라인 이미지 개방은 body/글상자
    /// 경로 한정 — 셀 안 글상자는 scan_cell_contents 가 여전히 셀 경계에서
    /// 거부한다 (내부 이미지 admittable 여부와 무관, 그 전에 컷).
    #[test]
    fn cell_textbox_with_inner_inline_image_stays_fail_closed() {
        use hwpforge_foundation::VerticalAlign;
        let mut inner = Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0)), img_with_height("BinData/i.png", 3402)],
            ParaShapeIndex::new(0),
        );
        inner.layout_cache = Some(LayoutCache::new(vec![tall_seg(0, 0, 3402)]));
        let mut cell_para = Paragraph::with_runs(
            vec![textbox_run(17008, 7087, VerticalAlign::Top, vec![inner])],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![sized_seg(0, 0, 7087)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("cell textbox rejected");
        assert!(
            matches!(&err, PdfError::UnsupportedContent { kind, .. }
                if *kind == "non-text content in table cell"),
            "{err:?}"
        );
    }

    #[test]
    fn cell_table_mixed_with_inline_image_rejected() {
        // `[Table+Image]` 혼합 = hosted-table replay 가 문단을 skip 해
        // 이미지가 무음 폐기됨 — 명시 거부 (r2 #2).
        let mut cell_para = Paragraph::with_runs(
            vec![
                Run::table(one_cell_cached_table(), CharShapeIndex::new(0)),
                inline_img("BinData/m.png"),
            ],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![img_seg(0, 0)]));
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("mixed rejected");
        assert!(
            matches!(&err, PdfError::UnsupportedContent { kind, .. }
                if *kind == "table mixed with inline image"),
            "{err:?}"
        );
    }

    #[test]
    fn cell_image_height_mismatch_is_fatal_even_with_warn_and_skip() {
        // 셀 캐시 오류는 행높이·앵커가 의존하므로 표-fatal — 기본
        // WarnAndSkip 에서도 문서 전체 Err (r2 #9, C4 계약).
        let mut cell_para = Paragraph::with_runs(
            vec![Run::text("셀 ", CharShapeIndex::new(0)), inline_img("BinData/tall.png")],
            ParaShapeIndex::new(0),
        );
        cell_para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)])); // vertsize 1000 ≠ 2000
        let host = table_host(img_cell_table(vec![cell_para]), 0);
        let doc = doc_of(vec![host]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("fatal");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("height")),
            "{err:?}"
        );
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
            .flat_map(|l| l.text_runs().map(|r| r.text.clone()))
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
            .find(|l| l.text_runs().any(|r| r.text == "C"))
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
            .flat_map(|l| l.text_runs().map(|r| r.text.clone()))
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
    fn trailing_non_text_run_is_rejected_before_w2() {
        // W1b (§1g v5 변경 1): 검사는 문단 전체 — trailing 각주도 W2 전
        // InvalidCache 다 (종전 "경고+생략"은 미렌더 원자의 무음 누락).
        let mut para = para_with_cache("본문", vec![seg(0, 0)]);
        para.add_run(Run::control(
            hwpforge_core::control::Control::footnote(vec![Paragraph::with_runs(
                vec![Run::text("각주", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )]),
            CharShapeIndex::new(0),
        ));
        let doc = doc_of(vec![para]);
        let err = replay(&doc, &PdfOptions::default()).expect_err("must reject");
        assert!(
            matches!(&err, PdfError::InvalidCache { detail } if detail.contains("not renderable")),
            "got {err:?}"
        );
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
