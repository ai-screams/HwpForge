//! E2 `fill` 델타 API — 이름 붙은 누름틀(ClickHere)을 값 맵으로 채운다.
//!
//! 에이전트가 섹션 JSON 전체를 왕복하지 않고 `이름 → 값` 델타만으로
//! 문서를 채우는 상위 API. 내부적으로 preserve-first 패처
//! ([`crate::HwpxPatcher`])를 사용하므로 채워진 섹션 XML 외의 모든 ZIP
//! 엔트리는 바이트 그대로 보존된다.
//!
//! # 정책 (설계 토론 확정 — `.docs/planning/2026-07-10-agent-editing-architecture.md`)
//!
//! - **전량 preflight 후 전량 적용** (all-or-nothing): 요청 값 중 하나라도
//!   검증에 실패하면 아무 것도 쓰지 않는다.
//! - **이름 중복 거부**: 같은 `name` 의 누름틀이 여러 개면
//!   [`FillError::DuplicateFieldName`] — 자동으로 전부 채우지 않는다.
//! - **빈 값 거부**: 빈 `display_text` 는 힌트-폴백·모호-다운그레이드
//!   sentinel 로 이미 과적되어 있으므로 [`FillError::EmptyValue`].
//! - **미채움/모호 필드 거부**: `display_text` 가 빈 필드는 patch 텍스트
//!   슬롯이 없어(슬롯 거울 불변식) 채울 수 없다 —
//!   [`FillError::UnfillableField`]. 병합-run(한컴 재저장) 필드가 여기
//!   해당한다.

use std::collections::BTreeMap;
use std::ops::Range;

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_foundation::FieldType;

use crate::decoder::{ParagraphPath, PathSeg};
use crate::error::{HwpxError, HwpxResult};
use crate::{HwpxDecoder, HwpxPatcher};

/// 문서 안의 누름틀 한 개에 대한 발견가능성 정보 (`fields` CLI/MCP 표면).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FieldInfo {
    /// 필드 이름 (`fieldBegin name="…"`). 이름 없는 누름틀은 `None` —
    /// 이름으로 지정할 수 없으므로 fill 대상이 아니다.
    pub name: Option<String>,
    /// 힌트(placeholder) 텍스트 (`Direction` 파라미터).
    pub hint: Option<String>,
    /// 현재 본문 값. 미채움 네이티브 필드는 힌트와 동일한 문자열이다.
    pub current: String,
    /// 필드가 속한 섹션 인덱스.
    pub section: usize,
    /// `fill` 로 채울 수 있는지. `false` = 본문이 비어 patch 슬롯이 없는
    /// 상태(병합-run 모호 필드 또는 빈 본문) — 한컴에서 재저장하거나
    /// `from-json --base` 재생성 경로가 필요하다.
    pub fillable: bool,
}

/// `fill` 로 실제 채워진 필드 하나의 기록.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FilledField {
    /// 필드 이름.
    pub name: String,
    /// 필드가 속한 섹션 인덱스.
    pub section: usize,
    /// 채우기 전 본문 (힌트 또는 이전 값) — 감사/롤백 참고용.
    pub previous: String,
}

/// [`HwpxFiller::fill`] 의 성공 결과.
#[derive(Debug, Clone)]
pub struct FillOutcome {
    /// 채워진 HWPX 패키지 바이트. 건드리지 않은 엔트리는 원본과 동일하다.
    pub bytes: Vec<u8>,
    /// 채워진 필드 목록 (요청 순서가 아니라 문서 순서).
    pub filled: Vec<FilledField>,
}

/// `fill` preflight/적용 실패.
#[derive(Debug)]
pub enum FillError {
    /// 빈 값은 채울 수 없다 — 빈 `display_text` 는 힌트-폴백 sentinel.
    EmptyValue {
        /// 문제의 필드 이름.
        name: String,
    },
    /// 요청한 이름의 누름틀이 문서에 없다.
    UnknownField {
        /// 요청한 이름.
        name: String,
        /// 문서에 존재하는 이름 붙은 누름틀 목록 (문서 순서, 중복 제거).
        available: Vec<String>,
    },
    /// 같은 이름의 누름틀이 여러 개 — 모호하므로 거부.
    DuplicateFieldName {
        /// 중복된 이름.
        name: String,
        /// 발견된 개수.
        count: usize,
    },
    /// 본문이 비어 patch 슬롯이 없는 필드 (병합-run 모호 또는 빈 본문).
    UnfillableField {
        /// 필드 이름.
        name: String,
        /// 필드가 속한 섹션.
        section: usize,
    },
    /// 디코드/패치 하위 오류.
    Workflow(HwpxError),
}

impl std::fmt::Display for FillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyValue { name } => {
                write!(f, "field '{name}': empty value is not fillable (빈 값 채우기 미지원 — 비우기는 별도 연산)")
            }
            Self::UnknownField { name, available } => {
                write!(f, "field '{name}' not found; available: [{}]", available.join(", "))
            }
            Self::DuplicateFieldName { name, count } => {
                write!(f, "field '{name}' appears {count} times; ambiguous target (이름 중복 — 문서에서 이름을 유일하게 하세요)")
            }
            Self::UnfillableField { name, section } => {
                write!(
                    f,
                    "field '{name}' in section {section} has no patchable body (병합-run 모호 필드 또는 빈 본문 — 한컴 재저장 또는 from-json --base 필요)"
                )
            }
            Self::Workflow(error) => write!(f, "fill workflow error: {error}"),
        }
    }
}

impl std::error::Error for FillError {}

impl From<HwpxError> for FillError {
    fn from(error: HwpxError) -> Self {
        Self::Workflow(error)
    }
}

/// 이름 기반 누름틀 채우기 상위 API.
#[derive(Debug, Clone, Copy)]
pub struct HwpxFiller;

/// 순회 중 발견한 필드의 가변 참조와 위치.
// pub(crate): E6 stamp preflight 가 기존 필드 이름 충돌 검사에 같은
// 순회를 재사용한다 (필드 방문 커버리지의 단일 정의 유지).
pub(crate) struct FieldSlot<'a> {
    pub(crate) section: usize,
    /// 필드가 든 문단의 구역 안 구조 경로 — 디코더 [`PathSeg`] 규약
    /// (`BodyParagraph` → 컨테이너 segment → `NestedParagraph` …), run 번호는
    /// 넣지 않는다. fill 이 wire 위치와 맞출 때 쓰는 키다.
    pub(crate) path: &'a [PathSeg],
    pub(crate) control: &'a mut Control,
}

/// 인벤토리(디코드한 Core)가 본 누름틀 하나의 위치.
///
/// `path` 는 [`FieldSlot::path`], `name` 은 디코더가 준 이름(빈 이름 = `None`).
/// 같은 `path` 안의 순번은 두 쪽 모두 문단열 하나 안의 문서 순서라 같다 —
/// 순번은 [`ordinals`] 가 목록 순서에서 계산한다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldLocator {
    path: Vec<PathSeg>,
    name: Option<String>,
}

/// section XML 에서 찾은 누름틀 하나: 위치와 그것을 감싸는 가장 안쪽 문단
/// 번호([`WireScan`] 안).
#[derive(Debug)]
struct WireField {
    locator: FieldLocator,
    paragraph: usize,
}

/// [`scan_click_here_fields`] 의 결과: 필드들과, 문단마다 직접 자식 캐시
/// 구간·놓인 목록, 목록마다 그것을 품은 문단. 크기는 입력에 선형이다 —
/// 무효화할 캐시는 바꿀 필드를 고른 뒤 [`WireScan::stale_caches`] 가 한 번에
/// 모은다(W2 리뷰 MEDIUM: 필드마다 꼬리를 미리 모으면 필드 수의 제곱).
#[derive(Debug)]
struct WireScan {
    fields: Vec<WireField>,
    caches: Vec<Vec<Range<usize>>>,
    para_list: Vec<usize>,
    list_host: Vec<Option<usize>>,
    /// 무효화 수집이 걸은 사슬 단계와 살펴본 문단 수(선형성 시험용).
    #[cfg(test)]
    visits: std::cell::Cell<usize>,
}

impl WireScan {
    /// 값이 바뀐 필드 문단들이 낡게 만드는 캐시 — 정의는 patch 와 같은
    /// [`crate::cache_flow::stale_caches`] 하나다.
    fn stale_caches(&self, paragraphs: impl IntoIterator<Item = usize>) -> Vec<Range<usize>> {
        crate::cache_flow::stale_caches(
            &self.caches,
            &self.para_list,
            &self.list_host,
            paragraphs,
            &mut || {
                #[cfg(test)]
                self.visits.set(self.visits.get() + 1);
            },
        )
    }
}

/// 같은 `path` 안에서 각 항목의 순번 (0부터, 목록 순서). 경로마다 센 수를
/// 색인에 두어 항목 수에 선형이다(브랜치 리뷰 B-H2: 앞 항목을 매번 다시 세면
/// N²).
/// 이것을 실패시키는 것: 앞 항목 다시 세기(`locating_stays_linear` 의 작업량).
fn ordinals(paths: &[&[PathSeg]]) -> Vec<usize> {
    let mut seen: std::collections::HashMap<&[PathSeg], usize> =
        std::collections::HashMap::with_capacity(paths.len());
    paths
        .iter()
        .map(|path| {
            crate::cache_flow::tick();
            let count = seen.entry(path).or_insert(0);
            *count += 1;
            *count - 1
        })
        .collect()
}

/// 값이 바뀐 필드들이 낡게 만드는 `<hp:linesegarray>` 를 제거한다 — 둘러싼
/// `<hp:p>` 와 같은 흐름의 뒤 문단, host 사슬의 꼬리([`scan_click_here_fields`]). 캐시 요소는 디코더처럼 접두어 없는 이름(`linesegarray`)으로
/// 판정한다.
///
/// `inventory` 는 구역별로 디코드 Core 의 모든 ClickHere 위치(이름 없는 것
/// 포함, [`visit_section_fields`] 순서), `changed` 는 값이 바뀐 필드의
/// `(구역, inventory 안 번호)` 다. 구역마다 section XML 의 누름틀 위치를
/// [`scan_click_here_fields`] 로 읽어 [`locate_changed_paragraph_caches`] 로 맞춘다.
fn strip_linesegarray_for_changed_fields(
    bytes: &[u8],
    inventory: &BTreeMap<usize, Vec<FieldLocator>>,
    changed: &[(usize, usize)],
) -> HwpxResult<Vec<u8>> {
    if changed.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut pkg = crate::patch::RawPackage::read(bytes)?;
    let mut sections: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (section_idx, field_idx) in changed {
        sections.entry(*section_idx).or_default().push(*field_idx);
    }
    for (section_idx, field_indices) in sections {
        let path = crate::patch::section_path(section_idx);
        let expected = inventory.get(&section_idx).map(Vec::as_slice).unwrap_or_default();
        let xml = pkg.read_text_entry(&path)?;
        let removals =
            locate_changed_paragraph_caches(&xml, section_idx, expected, &field_indices)?;
        // 구간은 문단마다 한 번만 모이고([`WireScan::stale_caches`]) 한 번에
        // 써진다 — 구간마다 뒤를 당기면 문단 수의 제곱 바이트 이동(B-H2).
        // 캐시 없는 문단은 removals 에 아무 것도 더하지 않는다 = no-op.
        let xml = crate::cache_flow::splice(
            &xml,
            removals.into_iter().map(|range| (range, String::new())).collect(),
        )?;
        pkg.replace_text_entry(&path, xml);
    }
    pkg.write()
}

/// 인벤토리의 `field_indices` 번 필드를 감싸는 문단의 캐시 구간을 돌려준다.
///
/// section XML 의 누름틀 위치 집합이 인벤토리 위치 집합과 **같은지** 먼저
/// 검사한다 — (경로, 같은 경로 안 순번, 이름)이 하나라도 어긋나면 디코더가
/// 본 필드와 wire 의 필드를 짝지을 근거가 없으므로 오류다(낡은 캐시를 남기거나
/// 엉뚱한 문단의 캐시를 지우는 대신 fail-closed).
///
/// # Errors
///
/// 위치 집합이 다르면 [`HwpxError::InvalidStructure`], XML 을 읽지 못하면
/// [`HwpxError::XmlParse`].
fn locate_changed_paragraph_caches(
    xml: &str,
    section_idx: usize,
    expected: &[FieldLocator],
    field_indices: &[usize],
) -> HwpxResult<Vec<Range<usize>>> {
    let scan = scan_click_here_fields(xml)?;
    let wire = &scan.fields;
    let expected_paths: Vec<&[PathSeg]> = expected.iter().map(|f| f.path.as_slice()).collect();
    let wire_paths: Vec<&[PathSeg]> = wire.iter().map(|f| f.locator.path.as_slice()).collect();
    let expected_ordinals = ordinals(&expected_paths);
    let wire_ordinals = ordinals(&wire_paths);

    let mismatch =
        |what: &str, locator: &FieldLocator, ordinal: usize| HwpxError::InvalidStructure {
            detail: format!(
                "section {section_idx}: ClickHere field {:?} at {} #{ordinal} {what} \
             (decoded {} field(s), section XML {}); cannot locate the filled paragraph",
                locator.name,
                ParagraphPath(locator.path.clone()),
                expected.len(),
                wire.len(),
            ),
        };

    // 짝: 인벤토리 i ↔ wire j (경로·순번이 같고 이름이 같음). (경로, 순번) 색인
    // 한 번으로 찾는다 — wire 를 항목마다 훑으면 N²(B-H2).
    let mut wire_at: std::collections::HashMap<(&[PathSeg], usize), usize> =
        std::collections::HashMap::with_capacity(wire.len());
    for (j, path) in wire_paths.iter().enumerate() {
        crate::cache_flow::tick();
        wire_at.entry((*path, wire_ordinals[j])).or_insert(j);
    }
    let mut partner: Vec<usize> = Vec::with_capacity(expected.len());
    let mut paired = vec![false; wire.len()];
    for (i, field) in expected.iter().enumerate() {
        crate::cache_flow::tick();
        match wire_at.get(&(field.path.as_slice(), expected_ordinals[i])).copied() {
            Some(j) if wire[j].locator.name == field.name => {
                partner.push(j);
                paired[j] = true;
            }
            Some(j) => {
                return Err(mismatch(
                    &format!("is named {:?} in the section XML", wire[j].locator.name),
                    field,
                    expected_ordinals[i],
                ))
            }
            None => {
                return Err(mismatch(
                    "is missing from the section XML",
                    field,
                    expected_ordinals[i],
                ))
            }
        }
    }
    if wire.len() != expected.len() {
        let extra = paired.iter().position(|p| !p).unwrap_or_default();
        return Err(mismatch(
            "is in the section XML but not in the decoded document",
            &wire[extra].locator,
            wire_ordinals[extra],
        ));
    }

    let paragraphs = field_indices
        .iter()
        .map(|&i| {
            partner.get(i).map(|&j| wire[j].paragraph).ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("section {section_idx}: filled field index {i} out of range"),
            })
        })
        .collect::<HwpxResult<Vec<_>>>()?;
    Ok(scan.stale_caches(paragraphs))
}

/// [`scan_click_here_fields`] 의 요소 스택 한 칸.
enum Frame {
    /// 문단을 직접 자식으로 갖는 자리(구역 루트·범위 안 `subList`).
    ParagraphList {
        nested: bool,
        next: usize,
        /// 목록 번호(`lists` 의 자리).
        list: usize,
    },
    /// `hp:p` — `index` 는 문단 구간 목록 번호.
    Paragraph {
        index: usize,
    },
    /// 문단의 직접 자식 `linesegarray` — `paragraph` 의 캐시 구간으로 기록한다.
    Cache {
        paragraph: usize,
        start: usize,
    },
    /// `hp:run` — 디코더처럼 run 안에서만 `fieldBegin`/`fieldEnd` 를 짝짓는다.
    Run {
        pending: Option<PendingBegin>,
    },
    /// `hp:ctrl` — 닫힐 때 디코더 순서(begin 다음 end)로 run 에 반영한다.
    Ctrl {
        begin: Option<PendingBegin>,
        end: bool,
    },
    Table {
        next_row: usize,
    },
    TableRow {
        row: usize,
        next_cell: usize,
    },
    /// 그림: `hp:img@binaryItemIDRef` 가 없으면 디코더가 그림째(캡션 포함)
    /// 버리므로, 닫힐 때 `mark` 뒤에 쌓인 필드를 되돌린다.
    Picture {
        mark: usize,
        has_image: bool,
    },
    /// 도형: `draw_text` = 내부 문단을 Core 가 담는지(사각형·다각형·호가
    /// 아닌 타원).
    Shape {
        draw_text: bool,
    },
    /// `subList` 하나를 담는 컨테이너(셀·캡션·`drawText`·각주·미주).
    SubListOwner,
    /// 인벤토리 범위 밖(묶음·머리말·꼬리말·메모·`switch`·미지 요소 등).
    Skip,
}

/// run 안에서 짝을 기다리는 `fieldBegin`.
struct PendingBegin {
    click_here: bool,
    name: Option<String>,
    paragraph: usize,
}

/// section XML 에서 인벤토리 범위 안의 `CLICK_HERE` 누름틀을 문서 순서로
/// 모은다.
///
/// 범위는 [`visit_section_fields`] 가 방문하는 컨테이너와 같다. 요소는
/// 디코더(serde)처럼 접두어 없는 이름으로 맞춘다:
///
/// | wire (부모 → 자식 → `subList` → `p`) | segment | `visit_*` 갈래 |
/// | -- | -- | -- |
/// | 구역 루트 → `p` | `BodyParagraph(i)` | `visit_section_fields` |
/// | `tbl` → `tr` → `tc` | `TableCell { row, cell }` | `RunContent::Table` 셀 |
/// | `tbl` → `caption` | `Caption` | `RunContent::Table` 캡션 |
/// | `pic`(`img@binaryItemIDRef` 있음) → `caption` | `Caption` | `RunContent::Image` |
/// | `rect`·`polygon`·`ellipse`(`hasArcPr≠1`) → `drawText` | `TextBox` | `TextBox`·`Polygon`·`Ellipse` 내부 |
/// | `rect`·`ellipse`·`polygon`·`line`·`curve`·`connectLine` → `caption` | `Caption` | 도형 캡션 |
/// | `ctrl` → `footNote` / `endNote` | `Footnote` / `Endnote` | `Footnote` / `Endnote` |
/// | `subList` → `p` | `NestedParagraph(i)` | `visit_nested` |
///
/// 표에 없는 자리(`hp:container` 묶음, 머리말·꼬리말, 메모 본문,
/// `hp:switch`, 호 내부 등)의 필드는 Core 인벤토리에 없으므로 후보에서 뺀다.
///
/// 짝짓기는 디코더(`convert_run`)와 같다: 같은 `hp:run` 안에서 `fieldBegin`
/// 다음 `fieldEnd` 가 와야 필드가 되고, 짝이 오기 전 새 `fieldBegin` 은 앞의
/// 것을 대체한다. 이름은 디코더처럼 XML 1.0 속성값 정규화 결과(문자 참조
/// `&#9;` 는 탭, 날 탭은 공백)이고 빈 이름은 `None` 이다.
fn scan_click_here_fields(xml: &str) -> HwpxResult<WireScan> {
    use crate::patch::{buffer_position, event_start};
    use quick_xml::events::{BytesStart, Event};
    use quick_xml::{Reader, XmlVersion};

    let xml_error = |detail: String| HwpxError::XmlParse { file: "section.xml".into(), detail };
    let attr_value = |start: &BytesStart<'_>, key: &str| -> HwpxResult<Option<String>> {
        for attr in start.attributes() {
            let attr = attr.map_err(|e| xml_error(e.to_string()))?;
            if attr.key.as_ref() == key {
                let value = attr
                    .normalized_value(XmlVersion::Explicit1_0)
                    .map_err(|e| xml_error(e.to_string()))?;
                return Ok(Some(value.into_owned()));
            }
        }
        Ok(None)
    };

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf: Vec<u8> = Vec::new();
    let mut stack: Vec<(Frame, bool)> = Vec::new();
    let mut path: Vec<PathSeg> = Vec::new();
    // 문단 번호 → 그 문단의 직접 자식 캐시 구간들.
    let mut paragraphs: Vec<Vec<Range<usize>>> = Vec::new();
    // 문단 번호 → 그 문단이 놓인 목록 번호, 목록 번호 → 그 목록을 품은 문단.
    let mut para_list: Vec<usize> = Vec::new();
    let mut list_host: Vec<Option<usize>> = Vec::new();
    let mut found: Vec<(FieldLocator, usize)> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(|e| xml_error(e.to_string()))?;
        let (start, is_empty) = match &event {
            Event::Start(start) => (start, false),
            Event::Empty(start) => (start, true),
            Event::End(_) => {
                let end = buffer_position(&reader)?;
                close_frame(&mut stack, &mut path, &mut paragraphs, &mut found, end);
                buf.clear();
                continue;
            }
            Event::Eof => break,
            _ => {
                buf.clear();
                continue;
            }
        };
        let end = buffer_position(&reader)?;
        let begin = event_start(end, start.len(), is_empty)?;
        let local = start.local_name();
        let local = local.as_ref();
        let innermost_paragraph = stack.iter().rev().find_map(|(frame, _)| match frame {
            Frame::Paragraph { index } => Some(*index),
            _ => None,
        });

        let mut seg: Option<PathSeg> = None;
        let frame = match stack.last_mut().map(|(frame, _)| frame) {
            None => {
                list_host.push(None);
                Frame::ParagraphList { nested: false, next: 0, list: list_host.len() - 1 }
            }
            Some(Frame::ParagraphList { nested, next, list }) if local == "p" => {
                seg = Some(if *nested {
                    PathSeg::NestedParagraph(*next)
                } else {
                    PathSeg::BodyParagraph(*next)
                });
                *next += 1;
                paragraphs.push(Vec::new());
                para_list.push(*list);
                Frame::Paragraph { index: paragraphs.len() - 1 }
            }
            Some(Frame::Paragraph { .. }) if local == "run" => Frame::Run { pending: None },
            Some(Frame::Paragraph { index }) if local == "linesegarray" => {
                Frame::Cache { paragraph: *index, start: begin }
            }
            Some(Frame::Run { .. }) => match local {
                "ctrl" => Frame::Ctrl { begin: None, end: false },
                "tbl" => Frame::Table { next_row: 0 },
                "pic" => Frame::Picture { mark: found.len(), has_image: false },
                "rect" | "polygon" => Frame::Shape { draw_text: true },
                // `hasArcPr="1"` 타원은 호(Control::Arc) — 내부 문단이 없다.
                "ellipse" => Frame::Shape {
                    draw_text: attr_value(start, "hasArcPr")?
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0)
                        != 1,
                },
                "line" | "curve" | "connectLine" => Frame::Shape { draw_text: false },
                _ => Frame::Skip,
            },
            Some(Frame::Ctrl { begin: pending, end: has_end }) => match local {
                "fieldBegin" => {
                    let paragraph = innermost_paragraph
                        .ok_or_else(|| xml_error("fieldBegin outside a paragraph".into()))?;
                    *pending = Some(PendingBegin {
                        click_here: attr_value(start, "type")?.as_deref() == Some("CLICK_HERE"),
                        name: attr_value(start, "name")?.filter(|name| !name.is_empty()),
                        paragraph,
                    });
                    Frame::Skip
                }
                "fieldEnd" => {
                    *has_end = true;
                    Frame::Skip
                }
                "footNote" => {
                    seg = Some(PathSeg::Footnote);
                    Frame::SubListOwner
                }
                "endNote" => {
                    seg = Some(PathSeg::Endnote);
                    Frame::SubListOwner
                }
                _ => Frame::Skip,
            },
            Some(Frame::Table { next_row }) => match local {
                "tr" => {
                    *next_row += 1;
                    Frame::TableRow { row: *next_row - 1, next_cell: 0 }
                }
                "caption" => {
                    seg = Some(PathSeg::Caption);
                    Frame::SubListOwner
                }
                _ => Frame::Skip,
            },
            Some(Frame::TableRow { row, next_cell }) if local == "tc" => {
                seg = Some(PathSeg::TableCell { row: *row, cell: *next_cell });
                *next_cell += 1;
                Frame::SubListOwner
            }
            Some(Frame::Picture { has_image, .. }) => match local {
                "img" => {
                    if attr_value(start, "binaryItemIDRef")?.is_some_and(|v| !v.is_empty()) {
                        *has_image = true;
                    }
                    Frame::Skip
                }
                "caption" => {
                    seg = Some(PathSeg::Caption);
                    Frame::SubListOwner
                }
                _ => Frame::Skip,
            },
            Some(Frame::Shape { draw_text }) => match local {
                "drawText" if *draw_text => {
                    seg = Some(PathSeg::TextBox);
                    Frame::SubListOwner
                }
                "caption" => {
                    seg = Some(PathSeg::Caption);
                    Frame::SubListOwner
                }
                _ => Frame::Skip,
            },
            Some(Frame::SubListOwner) if local == "subList" => {
                list_host.push(innermost_paragraph);
                Frame::ParagraphList { nested: true, next: 0, list: list_host.len() - 1 }
            }
            Some(_) => Frame::Skip,
        };
        if let Some(seg) = seg {
            path.push(seg);
        }
        stack.push((frame, seg.is_some()));
        if is_empty {
            close_frame(&mut stack, &mut path, &mut paragraphs, &mut found, end);
        }
        buf.clear();
    }

    Ok(WireScan {
        fields: found
            .into_iter()
            .map(|(locator, paragraph)| WireField { locator, paragraph })
            .collect(),
        caches: paragraphs,
        para_list,
        list_host,
        #[cfg(test)]
        visits: std::cell::Cell::new(0),
    })
}

/// [`scan_click_here_fields`] 에서 요소 하나가 닫힐 때의 처리.
fn close_frame(
    stack: &mut Vec<(Frame, bool)>,
    path: &mut Vec<PathSeg>,
    paragraphs: &mut [Vec<Range<usize>>],
    found: &mut Vec<(FieldLocator, usize)>,
    end: usize,
) {
    let Some((frame, pushed)) = stack.pop() else {
        return;
    };
    if pushed {
        path.pop();
    }
    match frame {
        // 디코더(serde)처럼 접두어 없는 이름으로 판정한다 — `p:linesegarray` 도
        // 같은 캐시다.
        Frame::Cache { paragraph, start } => paragraphs[paragraph].push(start..end),
        Frame::Ctrl { begin, end: has_end } => {
            let Some((Frame::Run { pending }, _)) = stack.last_mut() else {
                return;
            };
            if begin.is_some() {
                *pending = begin;
            }
            if has_end {
                if let Some(field) = pending.take() {
                    if field.click_here {
                        found.push((
                            FieldLocator { path: path.clone(), name: field.name },
                            field.paragraph,
                        ));
                    }
                }
            }
        }
        Frame::Picture { mark, has_image: false } => found.truncate(mark),
        _ => {}
    }
}

impl HwpxFiller {
    /// 문서의 모든 누름틀을 문서 순서로 나열한다 (발견가능성 표면).
    ///
    /// # Errors
    ///
    /// 패키지 디코드에 실패하면 [`HwpxError`] 를 반환한다.
    pub fn list_fields(base: &[u8]) -> HwpxResult<Vec<FieldInfo>> {
        Self::list_fields_with_diagnostics(base)
            .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// 모든 누름틀을 나열하되 **디코더 경고를 함께** 돌려준다.
    ///
    /// 투영 결과는 [`HwpxFiller::list_fields`] 와 동일하다 — 그쪽이 이
    /// 함수의 얇은 래퍼라 둘이 어긋날 수 없다.
    ///
    /// # Errors
    ///
    /// 패키지 디코드에 실패하면 [`HwpxError`] 를 반환한다.
    pub fn list_fields_with_diagnostics(
        base: &[u8],
    ) -> HwpxResult<crate::diagnostics::WithDecodeWarnings<Vec<FieldInfo>>> {
        let mut decoded = HwpxDecoder::decode(base)?;
        let warnings = decoded.warnings.clone();
        let mut fields = Vec::new();
        for (section_idx, section) in decoded.document.sections_mut().iter_mut().enumerate() {
            visit_section_fields(section, section_idx, &mut |slot| {
                if let Control::Field {
                    field_type: FieldType::ClickHere,
                    hint_text,
                    name,
                    display_text,
                    ..
                } = &*slot.control
                {
                    fields.push(FieldInfo {
                        name: name.clone(),
                        hint: hint_text.clone(),
                        current: display_text.clone(),
                        section: slot.section,
                        fillable: name.is_some() && !display_text.is_empty(),
                    });
                }
            });
        }
        Ok(crate::diagnostics::WithDecodeWarnings::new(fields, warnings))
    }

    /// 이름 → 값 맵으로 누름틀을 채운 새 패키지 바이트를 만든다.
    ///
    /// 전량 preflight 후 전량 적용 — 어느 하나라도 실패하면 아무 것도
    /// 쓰지 않고 [`FillError`] 를 반환한다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    pub fn fill(base: &[u8], values: &BTreeMap<String, String>) -> Result<FillOutcome, FillError> {
        Self::fill_with_diagnostics(base, values)
            .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// 누름틀을 채우되 **디코더 경고를 함께** 돌려준다.
    ///
    /// 바이트·`filled` 목록은 [`HwpxFiller::fill`] 과 동일하다 — 그쪽이 이
    /// 함수의 얇은 래퍼라 둘이 어긋날 수 없다. 이 함수 역시
    /// [`HwpxFiller::fill_with_codec_diagnostics`] 의 얇은 래퍼로, 인코드
    /// 채널(값에서 지운 제어 문자 경고)만 버린다.
    ///
    /// # 어느 디코드인가
    ///
    /// 이름 해석용 **입력 디코드 한 번**의 경고만 싣는다. 뒤따르는
    /// preserve 패치는 섹션마다 같은 base 를 다시 디코드하지만 그것은 이미
    /// 보고한 디코드의 재실행이므로 합치면 한 경고가 섹션 수만큼 불어난다.
    /// 그래서 경고 개수는 편집이 건드린 섹션 수와 무관하게 일정하다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    pub fn fill_with_diagnostics(
        base: &[u8],
        values: &BTreeMap<String, String>,
    ) -> Result<crate::diagnostics::WithDecodeWarnings<FillOutcome>, FillError> {
        Self::fill_with_codec_diagnostics(base, values).map(|diagnosed| {
            crate::diagnostics::WithDecodeWarnings::new(diagnosed.value, diagnosed.decode_warnings)
        })
    }

    /// 누름틀을 채우되 **디코더 경고와 인코드 경고를 함께** 돌려준다.
    ///
    /// 바이트·`filled` 목록은 [`HwpxFiller::fill`] 과 동일하다.
    /// `decode_warnings` 는 [`HwpxFiller::fill_with_diagnostics`] 와 같은 입력
    /// 디코드 한 번의 경고다.
    ///
    /// 채우기는 preserve-first 라 패키지를 재인코드하지 않는다.
    /// `encode_warnings` 는 값 자체에 대한 것뿐이다: XML 1.0 이 담을 수 없는
    /// 문자(TAB·LF·CR 을 뺀 C0 제어 문자 — 단 `<hp:fwSpace/>` 로 쓰는
    /// U+001F 는 남긴다 — 와 U+FFFE·U+FFFF)는 값에서 지우고, 지운 값마다
    /// [`EncodeWarning::XmlForbiddenCharsRemoved`](crate::EncodeWarning::XmlForbiddenCharsRemoved)
    /// 하나(`location` = `field 'NAME'`)를 싣는다. 지운 뒤의 값이 쓰기·"값이
    /// 바뀌었나" 비교·`filled` 보고에 모두 쓰이므로, 지운 뒤 현재 값과 같으면
    /// 그 필드는 채우지 않은 것으로 남는다(경고만, 바이트 그대로). 지운 뒤
    /// 비면 [`FillError::EmptyValue`]. 현재 값과 원문 그대로 같은 요청은
    /// 정리하지 않는다 — 원본이 이미 그런 문자를 담고 있어도 경고 없는 no-op 이다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::collections::BTreeMap;
    /// use hwpforge_smithy_hwpx::HwpxFiller;
    ///
    /// let base = std::fs::read("form.hwpx")?;
    /// let values = BTreeMap::from([("성명".to_string(), "홍길동".to_string())]);
    /// let diagnosed = HwpxFiller::fill_with_codec_diagnostics(&base, &values)?;
    /// for warning in &diagnosed.encode_warnings {
    ///     eprintln!("{warning}");
    /// }
    /// std::fs::write("filled.hwpx", &diagnosed.value.bytes)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn fill_with_codec_diagnostics(
        base: &[u8],
        values: &BTreeMap<String, String>,
    ) -> Result<crate::diagnostics::WithCodecWarnings<FillOutcome>, FillError> {
        // ── preflight 1: 값 자체 검증 (XML 이 담을 수 없는 문자를 지운 뒤) ──
        for (name, value) in values {
            if value.chars().all(crate::wire_xml::is_edit_unrepresentable_char) {
                return Err(FillError::EmptyValue { name: name.clone() });
            }
        }

        let mut decoded = HwpxDecoder::decode(base)?;
        let decode_warnings = decoded.warnings.clone();

        // ── preflight 2: 이름 해석 (개수·채움가능성) ──
        let mut inventory: Vec<(String, usize, String)> = Vec::new(); // (name, section, display)
        for (section_idx, section) in decoded.document.sections_mut().iter_mut().enumerate() {
            visit_section_fields(section, section_idx, &mut |slot| {
                if let Control::Field {
                    field_type: FieldType::ClickHere,
                    name: Some(name),
                    display_text,
                    ..
                } = &*slot.control
                {
                    inventory.push((name.clone(), slot.section, display_text.clone()));
                }
            });
        }

        for name in values.keys() {
            let matches: Vec<&(String, usize, String)> =
                inventory.iter().filter(|(n, _, _)| n == name).collect();
            match matches.len() {
                0 => {
                    let mut available: Vec<String> = Vec::new();
                    for (n, _, _) in &inventory {
                        if !available.contains(n) {
                            available.push(n.clone());
                        }
                    }
                    return Err(FillError::UnknownField { name: name.clone(), available });
                }
                1 => {
                    let (_, section, display) = matches[0];
                    if display.is_empty() {
                        return Err(FillError::UnfillableField {
                            name: name.clone(),
                            section: *section,
                        });
                    }
                }
                count => return Err(FillError::DuplicateFieldName { name: name.clone(), count }),
            }
        }

        // 값마다 입구에서 한 번 정리한다 — 이 값 하나가 쓰기·비교·보고에 쓰인다.
        // 현재 값과 원문 그대로 같은 요청은 정리하지 않는다: 원본이 이미 담고
        // 있는 문자를 같은 값으로 다시 요청한 것은 변경이 아니다(no-op 계약).
        let mut encode_warnings = Vec::new();
        let mut cleaned: BTreeMap<&str, String> = BTreeMap::new();
        for (name, value) in values {
            let Some((_, section, current)) = inventory.iter().find(|(n, _, _)| n == name) else {
                continue;
            };
            let value = if value == current {
                value.clone()
            } else {
                crate::wire_xml::sanitize_edit_text(
                    value,
                    &crate::patch::section_path(*section),
                    &format!("field '{name}'"),
                    &mut encode_warnings,
                )
            };
            cleaned.insert(name.as_str(), value);
        }

        // ── 적용: 대상 필드 mutate + 섹션별 preserve-first patch 체이닝 ──
        let original_sections: Vec<Section> = decoded.document.sections().to_vec();
        let mut filled: Vec<FilledField> = Vec::new();
        let mut touched: Vec<usize> = Vec::new();
        // 구역별 모든 ClickHere 위치 (이름 없는 것 포함 — wire 와 순번을 맞추는
        // 데 필요하다) + 값이 바뀐 필드의 (구역, 그 목록 안 번호).
        let mut locators: BTreeMap<usize, Vec<FieldLocator>> = BTreeMap::new();
        let mut changed_fields: Vec<(usize, usize)> = Vec::new();
        for (section_idx, section) in decoded.document.sections_mut().iter_mut().enumerate() {
            visit_section_fields(section, section_idx, &mut |slot| {
                let path = slot.path.to_vec();
                if let Control::Field {
                    field_type: FieldType::ClickHere, name, display_text, ..
                } = &mut *slot.control
                {
                    let section_locators = locators.entry(slot.section).or_default();
                    section_locators.push(FieldLocator { path, name: name.clone() });
                    let field_idx = section_locators.len() - 1;
                    let Some(name) = name else {
                        return;
                    };
                    if let Some(value) = cleaned.get(name.as_str()) {
                        if value == display_text {
                            // 동일 값 = 완전 no-op — mutate/FilledField/
                            // touched/캐시 무효화 전부 생략 (§1g v5 변경 5).
                            return;
                        }
                        filled.push(FilledField {
                            name: name.clone(),
                            section: slot.section,
                            previous: std::mem::replace(display_text, value.clone()),
                        });
                        changed_fields.push((slot.section, field_idx));
                        if !touched.contains(&slot.section) {
                            touched.push(slot.section);
                        }
                    }
                }
            });
        }

        let mut bytes = base.to_vec();
        for section_idx in touched {
            // preservation 은 원본 섹션 상태로 export 해야 sha/슬롯이 맞는다.
            // 앞선 patch 는 다른 섹션 파일만 바꾸므로 현재 bytes 의 이 섹션
            // XML 은 base 와 동일하다 — 체이닝이 안전한 근거.
            let preservation = HwpxPatcher::export_section_preservation(
                &bytes,
                section_idx,
                &original_sections[section_idx],
            )?;
            bytes = HwpxPatcher::patch_section_preserving(
                &bytes,
                section_idx,
                &decoded.document.sections()[section_idx],
                None,
                Some(&preservation),
            )?;
        }

        // W1b (§1g v5 변경 5): 값이 바뀐 필드의 둘러싼 문단에서
        // linesegarray 를 제거한다 — 동일 UTF-16 길이라도 글자 폭이 다르면
        // 줄바꿈/기하가 stale 이므로 "길이 변경 시"가 아니라 **값 변경 시
        // 무조건**이다. 문단당 정확히 1회 제거 (이후 재조판은 한글 몫).
        bytes = strip_linesegarray_for_changed_fields(&bytes, &locators, &changed_fields)?;

        Ok(crate::diagnostics::WithCodecWarnings::new(
            FillOutcome { bytes, filled },
            decode_warnings,
            encode_warnings,
        ))
    }
}

/// 섹션 안의 모든 [`Control`] 을 patch 텍스트-슬롯 순회와 동일한 커버리지로
/// 방문한다 (`collect_semantic_control_slots` / `redact_control` 의 거울 —
/// 여기 없는 컨테이너(예: `Group`)의 필드는 슬롯도 없으므로 fill 대상이
/// 아니어야 일관된다).
///
/// 각 필드에는 디코더가 경고 경로에 쓰는 것과 같은 segment 로 만든 문단
/// 경로를 함께 넘긴다([`FieldSlot::path`]).
pub(crate) fn visit_section_fields(
    section: &mut Section,
    section_idx: usize,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    let mut path = Vec::new();
    visit_paragraphs(&mut section.paragraphs, false, section_idx, &mut path, f);
}

fn visit_paragraphs(
    paragraphs: &mut [Paragraph],
    nested: bool,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    for (index, paragraph) in paragraphs.iter_mut().enumerate() {
        path.push(if nested {
            PathSeg::NestedParagraph(index)
        } else {
            PathSeg::BodyParagraph(index)
        });
        for run in &mut paragraph.runs {
            match &mut run.content {
                RunContent::Control(control) => visit_control(control, section_idx, path, f),
                RunContent::Table(table) => {
                    if let Some(caption) = table.caption.as_mut() {
                        visit_nested(
                            &mut caption.paragraphs,
                            PathSeg::Caption,
                            section_idx,
                            path,
                            f,
                        );
                    }
                    for (row_index, row) in table.rows.iter_mut().enumerate() {
                        for (cell_index, cell) in row.cells.iter_mut().enumerate() {
                            let seg = PathSeg::TableCell { row: row_index, cell: cell_index };
                            visit_nested(&mut cell.paragraphs, seg, section_idx, path, f);
                        }
                    }
                }
                RunContent::Image(image) => {
                    if let Some(caption) = image.caption.as_mut() {
                        visit_nested(
                            &mut caption.paragraphs,
                            PathSeg::Caption,
                            section_idx,
                            path,
                            f,
                        );
                    }
                }
                _ => {}
            }
        }
        path.pop();
    }
}

/// 컨테이너 segment `seg` 아래의 문단열을 방문한다.
fn visit_nested(
    paragraphs: &mut [Paragraph],
    seg: PathSeg,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    path.push(seg);
    visit_paragraphs(paragraphs, true, section_idx, path, f);
    path.pop();
}

fn visit_control(
    control: &mut Control,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    f(&mut FieldSlot { section: section_idx, path: path.as_slice(), control });
    match control {
        Control::TextBox { paragraphs, caption, .. }
        | Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            visit_nested(paragraphs, PathSeg::TextBox, section_idx, path, f);
            if let Some(caption) = caption.as_mut() {
                visit_nested(&mut caption.paragraphs, PathSeg::Caption, section_idx, path, f);
            }
        }
        Control::Footnote { paragraphs, .. } => {
            visit_nested(paragraphs, PathSeg::Footnote, section_idx, path, f);
        }
        Control::Endnote { paragraphs, .. } => {
            visit_nested(paragraphs, PathSeg::Endnote, section_idx, path, f);
        }
        Control::Rect { caption: Some(caption), .. }
        | Control::Line { caption: Some(caption), .. }
        | Control::Arc { caption: Some(caption), .. }
        | Control::Curve { caption: Some(caption), .. }
        | Control::ConnectLine { caption: Some(caption), .. } => {
            visit_nested(&mut caption.paragraphs, PathSeg::Caption, section_idx, path, f);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    //! 채운 누름틀의 문단 캐시를 wire 에서 찾는 경로(R1-4)의 테스트.
    //!
    //! 캐시 표지: [`add_caches`] 가 모든 `</hp:p>` 앞에 `vertpos` = 9000 + (닫히는
    //! 순서) 인 `linesegarray` 를 넣는다. 안쪽 문단이 먼저 닫히므로 번호는
    //! 구조에서 정해진다 — 각 테스트 주석에 어느 번호가 어느 문단인지 적었다.

    use super::*;
    use crate::patch::RawPackage;
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    /// 한컴 저장 누름틀 하나(`user_email`, 캐시 `vertpos="0"`)가 있는 문서.
    const NAMED: &[u8] = include_bytes!("../../../tests/fixtures/fields/clickhere_named.hwpx");

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn field(name: &str, current: &str) -> RunContent {
        RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some(current.to_string()),
            help_text: None,
            name: Some(name.to_string()),
            display_text: current.to_string(),
        }))
    }

    fn text(value: &str) -> RunContent {
        RunContent::Text(value.to_string())
    }

    fn para(contents: Vec<RunContent>) -> Paragraph {
        let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
        for content in contents {
            paragraph.runs.push(Run { content, char_shape_id: CharShapeIndex::new(0) });
        }
        paragraph
    }

    fn control(control: Control) -> RunContent {
        RunContent::Control(Box::new(control))
    }

    fn size(value: i32) -> HwpUnit {
        HwpUnit::new(value).expect("unit")
    }

    fn encode(paragraphs: Vec<Paragraph>, images: &ImageStore) -> Vec<u8> {
        let mut doc = Document::new();
        let mut section = Section::new(PageSettings::default());
        section.paragraphs = paragraphs;
        doc.add_section(section);
        let validated = doc.validate().expect("validate");
        let styles = crate::style_store_for_preset("default").expect("preset");
        crate::HwpxEncoder::encode(&validated, &styles, images).expect("encode")
    }

    fn section_xml(bytes: &[u8]) -> String {
        RawPackage::read(bytes)
            .expect("read")
            .read_text_entry("Contents/section0.xml")
            .expect("xml")
    }

    fn with_section_xml(bytes: &[u8], xml: String) -> Vec<u8> {
        let mut pkg = RawPackage::read(bytes).expect("read");
        pkg.replace_text_entry("Contents/section0.xml", xml);
        pkg.write().expect("write")
    }

    /// 모든 `</hp:p>` 앞에 닫히는 순서 번호를 `vertpos` 로 단 캐시를 넣는다.
    fn add_caches(xml: &str) -> String {
        let mut out = String::with_capacity(xml.len());
        for (k, piece) in xml.split("</hp:p>").enumerate() {
            if k > 0 {
                out.push_str("</hp:p>");
            }
            out.push_str(piece);
            if xml[..].matches("</hp:p>").count() > k {
                out.push_str(&format!(
                    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" \
                     textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                     horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray>",
                    9000 + k
                ));
            }
        }
        out
    }

    /// 남아 있는 캐시 표지 번호 (9000 기준 오프셋).
    fn caches(xml: &str) -> Vec<usize> {
        (0..64).filter(|k| xml.contains(&format!("vertpos=\"{}\"", 9000 + k))).collect()
    }

    fn fill_xml(bytes: &[u8], pairs: &[(&str, &str)]) -> Result<String, FillError> {
        let outcome = HwpxFiller::fill(bytes, &values(pairs))?;
        HwpxDecoder::decode(&outcome.bytes).expect("filled output decodes");
        Ok(section_xml(&outcome.bytes))
    }

    fn named_with(xml_edit: impl Fn(&str) -> String) -> Vec<u8> {
        let xml = section_xml(NAMED);
        let edited = xml_edit(&xml);
        assert_ne!(edited, xml, "test edit must change the fixture");
        with_section_xml(NAMED, edited)
    }

    #[test]
    fn equivalent_name_spellings_reach_the_filled_paragraph() {
        // 이것을 실패시키는 것: 이름을 문자열 needle(`name="…"`)로 찾기 —
        // 엔티티·작은따옴표·`=` 공백·속성 안 `>` 를 못 찾아 캐시가 남는다.
        // 이름을 정규화 없이 raw 로 비교 → 엔티티 행이 위치 불일치 오류.
        // `normalized_value` 대신 엔티티만 푸는 `quick_xml::escape::unescape`(속성값
        // 공백 정규화 없음) → 날 탭 행이 위치 불일치 오류(디코더는 날 탭을
        // 공백으로 읽는다).
        let cases: [(&str, &str); 10] = [
            ("name=\"user_email\"", "user_email"),
            ("name=\"a&amp;b\"", "a&b"),
            ("name=\"a&#38;b\"", "a&b"),
            ("name=\"a&quot;b\"", "a\"b"),
            ("name=\"a&lt;b\"", "a<b"),
            ("name=\"a>b\"", "a>b"),
            ("name='user_email'", "user_email"),
            ("name = \"user_email\"", "user_email"),
            ("name=\"a&#9;b\"", "a\tb"),
            ("name=\"a\tb\"", "a b"),
        ];
        for (wire, decoded) in cases {
            let bytes = if wire == "name=\"user_email\"" {
                NAMED.to_vec()
            } else {
                named_with(|xml| xml.replacen("name=\"user_email\"", wire, 1))
            };
            let listed = HwpxFiller::list_fields(&bytes).expect("list");
            assert_eq!(listed[0].name.as_deref(), Some(decoded), "fixture premise for {wire}");
            let xml = fill_xml(&bytes, &[(decoded, "채움")])
                .unwrap_or_else(|e| panic!("{wire}: fill failed: {e}"));
            assert!(!xml.contains("<hp:linesegarray"), "{wire}: filled paragraph cache must go");
        }
    }

    #[test]
    fn same_named_hyperlink_before_the_field_keeps_its_cache() {
        // 앞 문단 = 같은 이름의 HYPERLINK(캐시 vertpos 0), 뒤 문단 = 실제
        // 누름틀(캐시 vertpos 777).
        // 이것을 실패시키는 것: `type="CLICK_HERE"` 필터 제거 → wire 쪽 후보가
        // 둘이 되어 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let start = xml.find("<hp:run charPrIDRef=\"0\"><hp:ctrl><hp:fieldBegin").unwrap();
            let end = xml[start..].find("</hp:run>").unwrap() + start + "</hp:run>".len();
            let run = xml[start..end].replace("1000000000", "1000000001");
            let new_para = format!(
                "<hp:p id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" \
                 columnBreak=\"0\" merged=\"0\">{run}<hp:linesegarray><hp:lineseg textpos=\"0\" \
                 vertpos=\"777\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" \
                 spacing=\"600\" horzpos=\"0\" horzsize=\"42520\" flags=\"393216\"/>\
                 </hp:linesegarray></hp:p>"
            );
            xml.replacen(
                "type=\"CLICK_HERE\" name=\"user_email\"",
                "type=\"HYPERLINK\" name=\"user_email\"",
                1,
            )
            .replace("</hs:sec>", &format!("{new_para}</hs:sec>"))
        });
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(xml.contains("vertpos=\"0\""), "hyperlink paragraph cache must stay");
        assert!(!xml.contains("vertpos=\"777\""), "filled paragraph cache must go");
    }

    #[test]
    fn cell_cache_inside_the_filled_paragraph_is_left_alone() {
        // 9000 = 셀 문단(먼저 닫힘), 9001 = 필드와 표가 든 바깥 문단.
        // 이것을 실패시키는 것: 캐시를 그것의 부모 문단이 아니라 가장 바깥
        // 문단에 귀속 → 셀 캐시 9000 이 지워진다.
        let cell = TableCell::new(vec![para(vec![text("셀")])], size(4000));
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let doc = encode(
            vec![para(vec![field("f", "입력"), RunContent::Table(Box::new(table))])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let xml = fill_xml(&bytes, &[("f", "값")]).expect("fill");
        assert_eq!(caches(&xml), vec![0]);
    }

    #[test]
    fn two_filled_fields_in_one_paragraph_remove_its_cache_once() {
        // 9000 = 필드 둘이 든 문단, 9001 = 다음 문단.
        // 이것을 실패시키는 것: 같은 구간을 두 번 모으기(`stale_caches` 가 구간을
        // 겹쳐 내면 두 번째 제거가 다른 바이트를 깎는다).
        let doc = encode(
            vec![para(vec![field("a", "가"), field("b", "나")]), para(vec![text("다음")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let xml = fill_xml(&bytes, &[("a", "1"), ("b", "2")]).expect("fill");
        // W2: 다음 문단(9001)도 채운 문단 뒤 같은 흐름이라 누적 vertpos 가
        // 낡는다 — 전에는 남겼던 것이 틀렸다(r1-3).
        assert_eq!(caches(&xml), Vec::<usize>::new());
        assert!(xml.contains("<hp:t>다음</hp:t>"), "the next paragraph's text is intact");
    }

    /// W2 (design 3.5 tests 1, 3): filling a field in the middle of three
    /// cell paragraphs drops that paragraph's cache, the cell paragraph after
    /// it, the table's host paragraph and the body paragraph after the host;
    /// the first cell paragraph and the body paragraph before keep theirs.
    /// A value of the same length does the same (line counts are not judged
    /// — that would need layout).
    /// 이것을 실패시키는 것: 꼬리 대신 문단 하나만(9003 이 남는다) · host 사슬
    /// 생략(9004·9005 가 남는다) · 목록 처음부터(9001 이 간다).
    #[test]
    fn cell_fill_drops_the_flow_tail_and_the_host_chain() {
        let cell = TableCell::new(
            vec![para(vec![text("c0")]), para(vec![field("mid", "칸")]), para(vec![text("c2")])],
            size(4000),
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let doc = encode(
            vec![
                para(vec![text("앞")]),
                para(vec![RunContent::Table(Box::new(table))]),
                para(vec![text("뒤")]),
            ],
            &ImageStore::default(),
        );
        // close order: 앞 9000, c0 9001, mid 9002, c2 9003, host 9004, 뒤 9005.
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        for value in ["여러 줄로 길어진 값 여러 줄로 길어진 값 여러 줄로 길어진 값", "값"]
        {
            let xml = fill_xml(&bytes, &[("mid", value)]).expect("fill");
            assert_eq!(caches(&xml), vec![0, 1], "{value}");
        }
    }

    /// Branch review B-H2: locating the filled paragraphs and writing the
    /// removals stays linear in the field count — the per-path ordinals, the
    /// pairing with the section XML and the one-pass splice — not only the
    /// cache collection.
    /// 이것을 실패시키는 것: 순번을 앞 항목 다시 세기로, 짝짓기를 wire 전체 훑기로
    /// (작업량 N²) · 구간마다 따로 지우기(splice 를 구간마다 부름).
    #[test]
    fn locating_stays_linear() {
        const N: usize = 2000;
        let paragraphs: Vec<Paragraph> =
            (0..N).map(|i| para(vec![field(&format!("f{i}"), "값")])).collect();
        let doc = encode(paragraphs, &ImageStore::default());
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let locators: Vec<FieldLocator> = (0..N)
            .map(|i| FieldLocator {
                path: vec![PathSeg::BodyParagraph(i)],
                name: Some(format!("f{i}")),
            })
            .collect();
        let inventory = BTreeMap::from([(0, locators)]);
        let changed: Vec<(usize, usize)> = (0..N).map(|i| (0, i)).collect();
        crate::cache_flow::WORK.with(|w| w.set(0));
        let out =
            strip_linesegarray_for_changed_fields(&bytes, &inventory, &changed).expect("strip");
        let work = crate::cache_flow::WORK.with(std::cell::Cell::get);
        assert!(caches(&section_xml(&out)).is_empty());
        assert!(work <= 10 * N, "{work} work units for {N} fields");
    }

    /// W2 review MEDIUM: the scan keeps one cache list per paragraph (not a
    /// flow tail per field), and filling the last of N fields collects one
    /// range — the first collects N.
    /// 이것을 실패시키는 것: 필드마다 꼬리를 미리 모으기(스캔 구간 수가 N²/2).
    #[test]
    fn scan_and_collection_stay_linear() {
        const N: usize = 2000;
        let paragraphs: Vec<Paragraph> =
            (0..N).map(|i| para(vec![field(&format!("f{i}"), "값")])).collect();
        let doc = encode(paragraphs, &ImageStore::default());
        let xml = add_caches(&section_xml(&doc));
        let scan = scan_click_here_fields(&xml).expect("scan");
        assert_eq!(scan.fields.len(), N);
        assert_eq!(scan.caches.iter().map(Vec::len).sum::<usize>(), N);
        assert_eq!(scan.stale_caches([scan.fields[N - 1].paragraph]).len(), 1);
        assert_eq!(scan.stale_caches([scan.fields[0].paragraph]).len(), N);
        let all: Vec<usize> = scan.fields.iter().map(|f| f.paragraph).collect();
        // S22 r1: the collection itself stays linear, not only its result —
        // one walk up each paragraph's host chain and one pass over the
        // caches. 이것을 실패시키는 것: 바뀐 문단마다 캐시 전체를 다시 훑기
        // (같은 결과, 살펴본 수 N²).
        scan.visits.set(0);
        assert_eq!(scan.stale_caches(all).len(), N);
        assert!(scan.visits.get() <= 3 * N, "{} visits for {N} fields", scan.visits.get());
    }

    /// W2 review: per container, filling the middle of three paragraphs
    /// drops exactly that paragraph, the one after it, the host paragraph
    /// and the body paragraph after the host — every other cache stays.
    /// 이것을 실패시키는 것: host 사슬 생략 · 꼬리 대신 문단 하나만 · 목록
    /// 처음부터 — 컨테이너마다.
    #[test]
    fn each_container_drops_its_flow_tail_and_host_chain() {
        let inner =
            || vec![para(vec![text("x0")]), para(vec![field("mid", "칸")]), para(vec![text("x2")])];
        let containers: Vec<(&str, RunContent)> = vec![
            (
                "cell",
                RunContent::Table(Box::new(Table::new(vec![TableRow::new(vec![TableCell::new(
                    inner(),
                    size(4000),
                )])]))),
            ),
            ("text box", control(Control::text_box(inner(), size(4000), size(2000)))),
            (
                "table caption",
                RunContent::Table(Box::new(
                    Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![para(vec![text("칸")])],
                        size(4000),
                    )])])
                    .with_caption(Caption::new(inner(), CaptionSide::Bottom)),
                )),
            ),
            ("footnote", control(Control::footnote(inner()))),
        ];
        for (name, content) in containers {
            let doc = encode(
                vec![para(vec![text("앞")]), para(vec![content]), para(vec![text("뒤")])],
                &ImageStore::default(),
            );
            let xml = add_caches(&section_xml(&doc));
            // Close order of a paragraph = `</hp:p>` count before it closes.
            let leaf =
                |needle: &str| xml[..xml.find(needle).expect(needle)].matches("</hp:p>").count();
            let host = leaf("<hp:t>뒤</hp:t>") - 1;
            let dropped =
                [leaf("name=\"mid\""), leaf("<hp:t>x2</hp:t>"), host, leaf("<hp:t>뒤</hp:t>")];
            let all = caches(&xml);
            let expected: Vec<usize> =
                all.iter().copied().filter(|k| !dropped.contains(k)).collect();
            let bytes = with_section_xml(&doc, xml.clone());
            let filled = fill_xml(&bytes, &[("mid", "여러 줄로 길어진 값")])
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(caches(&filled), expected, "{name}");
            assert!(expected.contains(&leaf("<hp:t>x0</hp:t>")), "{name}: x0 keeps its cache");
        }
    }

    #[test]
    fn filled_paragraph_without_cache_is_a_no_op_for_the_cache() {
        // 이것을 실패시키는 것: "캐시 없음"(직접 자식 캐시 0개)을 오류로 처리.
        let doc = encode(vec![para(vec![field("f", "입력")])], &ImageStore::default());
        let xml = fill_xml(&doc, &[("f", "값")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn same_named_field_inside_a_group_is_out_of_scope() {
        // 묶음 안 글상자 문단 = 9000, 묶음이 든 문단 = 9001, 본문 필드 문단 = 9002.
        // fill 인벤토리는 묶음 내부를 보지 않으므로 이름 `dup` 은 하나다.
        // 이것을 실패시키는 것: `hp:container` 를 범위 밖으로 두지 않기(예: run
        // 처럼 자식 도형을 따라 들어감) → wire 후보가 둘이 되어 위치 불일치 오류.
        let inner = Control::text_box(vec![para(vec![field("dup", "안")])], size(4000), size(2000));
        let group = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![hwpforge_core::control::GroupChild::Shape(inner)],
            width: size(4000),
            height: size(2000),
            placement: None,
            inst_id: None,
        };
        let doc = encode(
            vec![para(vec![control(group)]), para(vec![field("dup", "밖")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the group interior is not in the inventory");
        let xml = fill_xml(&bytes, &[("dup", "값")]).expect("fill");
        assert_eq!(caches(&xml), vec![0, 1]);
    }

    #[test]
    fn paragraph_attributes_split_across_lines_are_found() {
        // 이것을 실패시키는 것: 문단 시작을 `<hp:p` + 공백/`>` 문자열로 찾던
        // 옛 스캐너 — 개행으로 나뉜 속성(`<hp:p\n id=…>`)을 문단으로 보지 못해
        // 캐시가 남는다.
        let bytes = named_with(|xml| xml.replace("<hp:p id=", "<hp:p\n id="));
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn ellipse_inner_and_caption_fields_match_by_structure_not_order() {
        // 인벤토리는 타원 내부 문단(A)을 캡션(B)보다 먼저 방문하고, XML 은
        // 캡션을 먼저 쓴다 — 문서 순서로는 [B, A], 인벤토리 순서로는 [A, B].
        // 이것을 실패시키는 것: 구조 키 대신 문서 순서 목록으로 짝짓기 →
        // 이름이 엇갈려 정상 문서를 위치 불일치로 거부한다.
        let mut ellipse =
            Control::ellipse_with_text(size(6000), size(3000), vec![para(vec![field("A", "가")])]);
        if let Control::Ellipse { caption, .. } = &mut ellipse {
            *caption = Some(Caption::new(vec![para(vec![field("B", "나")])], CaptionSide::Bottom));
        }
        let doc = encode(vec![para(vec![control(ellipse)])], &ImageStore::default());
        let xml = section_xml(&doc);
        let caption_at = xml.find("<hp:caption").expect("caption");
        let inner_at = xml.find("<hp:drawText").expect("drawText");
        assert!(caption_at < inner_at, "premise: the caption is written before the inner text");
        // 9000 = 캡션 문단, 9001 = 내부 문단, 9002 = 타원이 든 본문 문단.
        let bytes = with_section_xml(&doc, add_caches(&xml));
        let only_a = fill_xml(&bytes, &[("A", "1")]).expect("fill A");
        // W2: 내부 문단(9001)의 host 는 타원이 든 본문 문단(9002) — 상자가 글에
        // 맞춰 자랄 수 있으므로 host 사슬로 함께 지운다(전에는 남겼다). 캡션(9000)은
        // 다른 흐름이라 남는다.
        assert_eq!(caches(&only_a), vec![0]);
        let filled = HwpxFiller::fill(&bytes, &values(&[("A", "1"), ("B", "2")])).expect("fill");
        assert_eq!(caches(&section_xml(&filled.bytes)), Vec::<usize>::new());
        let listed = HwpxFiller::list_fields(&filled.bytes).expect("list");
        let current: Vec<(Option<&str>, &str)> =
            listed.iter().map(|f| (f.name.as_deref(), f.current.as_str())).collect();
        assert_eq!(current, vec![(Some("A"), "1"), (Some("B"), "2")]);
    }

    #[test]
    fn click_here_without_an_end_in_its_run_is_out_of_scope() {
        // 한컴 코퍼스의 누름틀은 대개 `fieldEnd` 가 다른 run(다른 문단)에 있고
        // 디코더는 그런 필드를 Core 로 올리지 않는다. 채울 필드 뒤 문단(캐시
        // 777)에 그런 같은 이름의 `fieldBegin` 을 둔다.
        // 이것을 실패시키는 것: run 안 짝짓기 없이 `fieldBegin` 마다 필드로
        // 세기 → wire 후보가 둘이 되어 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:fieldBegin").unwrap();
            let close =
                xml[open..].find("</hp:fieldBegin>").unwrap() + open + "</hp:fieldBegin>".len();
            let begin = xml[open..close].replace("1000000000", "1000000009");
            let new_para = format!(
                "<hp:p id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" \
                 columnBreak=\"0\" merged=\"0\"><hp:run charPrIDRef=\"0\"><hp:ctrl>{begin}\
                 </hp:ctrl></hp:run><hp:run charPrIDRef=\"0\"><hp:t>본문</hp:t></hp:run>\
                 <hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"777\" vertsize=\"1000\" \
                 textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                 horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray></hp:p>"
            );
            xml.replace("</hs:sec>", &format!("{new_para}</hs:sec>"))
        });
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the decoder drops the unpaired begin");
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        // W2: 777 문단은 채운 문단 뒤 같은 흐름이라 이제 캐시를 잃는다(전에는
        // 남겼던 것이 틀렸다). 이 시험의 이빨(짝 없는 begin 을 필드로 세면 위치
        // 불일치 오류)은 위 `fill_xml(...).expect` 가 진다.
        assert!(!xml.contains("vertpos=\"777\""), "the flow tail goes");
        assert!(!xml.contains("vertpos=\"0\""), "filled paragraph cache must go");
    }

    #[test]
    fn a_second_begin_in_the_run_replaces_the_pending_one() {
        // 같은 run 에서 `fieldBegin`(ghost) 다음 `fieldBegin`(user_email) 이 오면
        // 디코더는 뒤의 것과 `fieldEnd` 를 짝짓는다.
        // 이것을 실패시키는 것: 짝을 기다리는 begin 이 있으면 새 begin 을 무시
        // → wire 이름이 ghost 가 되어 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:ctrl><hp:fieldBegin").unwrap();
            let ghost = "<hp:ctrl><hp:fieldBegin id=\"1000000007\" type=\"CLICK_HERE\" \
                         name=\"ghost\" editable=\"1\" dirty=\"0\" zorder=\"-1\" \
                         fieldid=\"627272811\"/></hp:ctrl>";
            format!("{}{ghost}{}", &xml[..open], &xml[open..])
        });
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the decoder pairs the second begin");
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn picture_caption_fields_follow_the_image_reference() {
        // 그림 캡션 문단 = 9000, 그림이 든 문단 = 9001, 본문 필드 문단 = 9002.
        // 이것을 실패시키는 것: (a) `binaryItemIDRef` 와 무관하게 그림 캡션을
        // 늘 범위 밖으로 두기 → 정상 그림의 캡션 필드 `cap` 이 wire 에서 빠져
        // 위치 불일치 오류. (b) 빈 `binaryItemIDRef`(디코더가 그림째 버림)의 캡션
        // 필드를 되돌리지 않기 → wire 후보가 하나 남아 위치 불일치 오류.
        let mut images = ImageStore::new();
        images.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
        let image = Image::new("BinData/image1.png", size(2000), size(2000), ImageFormat::Png)
            .with_caption(Caption::new(vec![para(vec![field("cap", "캡")])], CaptionSide::Bottom));
        let doc = encode(
            vec![para(vec![RunContent::Image(image)]), para(vec![field("body", "본")])],
            &images,
        );
        let xml = add_caches(&section_xml(&doc));
        let bytes = with_section_xml(&doc, xml.clone());
        let filled = fill_xml(&bytes, &[("cap", "1")]).expect("fill caption field");
        // W2: 캡션이 자라면 그림이 든 문단(9001, host)과 그 뒤 본문 문단(9002)이
        // 밀린다 — 전에는 둘 다 남겼다(r1-3).
        assert_eq!(caches(&filled), Vec::<usize>::new());

        let start = xml.find("binaryItemIDRef=\"").expect("img ref") + "binaryItemIDRef=\"".len();
        let end = xml[start..].find('"').unwrap() + start;
        let dropped = with_section_xml(&doc, format!("{}{}", &xml[..start], &xml[end..]));
        let listed = HwpxFiller::list_fields(&dropped).expect("list");
        assert_eq!(listed.len(), 1, "premise: the decoder drops the picture and its caption");
        let filled = fill_xml(&dropped, &[("body", "1")]).expect("fill body field");
        assert_eq!(caches(&filled), vec![0, 1]);
    }

    #[test]
    fn arc_draw_text_is_out_of_scope() {
        // `hasArcPr="1"` 타원은 호(Control::Arc)라 디코더가 내부 문단을 버린다.
        // 이것을 실패시키는 것: `hasArcPr` 를 보지 않고 타원 `drawText` 를 늘
        // 범위 안으로 두기 → wire 후보가 하나 많아 위치 불일치 오류.
        let ellipse =
            Control::ellipse_with_text(size(6000), size(3000), vec![para(vec![field("in", "가")])]);
        let doc = encode(
            vec![para(vec![control(ellipse)]), para(vec![field("body", "본")])],
            &ImageStore::default(),
        );
        let xml = section_xml(&doc);
        assert!(xml.contains("hasArcPr=\"0\""), "premise: the encoder writes hasArcPr");
        let bytes = with_section_xml(
            &doc,
            add_caches(&xml.replacen("hasArcPr=\"0\"", "hasArcPr=\"1\"", 1)),
        );
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the arc's inner text is not in the inventory");
        let filled = fill_xml(&bytes, &[("body", "1")]).expect("fill");
        // 9000 = 호 내부 문단, 9001 = 호가 든 문단, 9002 = 본문 필드 문단.
        assert_eq!(caches(&filled), vec![0, 1]);
    }

    #[test]
    fn every_container_fill_visits_maps_to_its_own_paragraph() {
        // fill 인벤토리가 방문하는 컨테이너마다 필드 하나를 두고 하나씩 채운다.
        // 기대 캐시(독립 판정): 각 필드 문단에는 필드만 있으므로 그 문단은
        // `fieldBegin` 뒤 첫 `</hp:p>` 에서 닫힌다 — 표지 번호 = 그 앞의
        // `</hp:p>` 개수.
        // 이것을 실패시키는 것: 범위 표의 갈래 하나를 범위 밖으로 바꾸기
        // (`tc`·표 `caption`·`footNote`·`endNote`·`rect`/`polygon` 의 `drawText`·
        // 도형 `caption`·`subList`) → 그 필드가 wire 에서 빠져 위치 불일치 오류.
        let with_caption = |mut control: Control, name: &str| {
            let caption = Caption::new(vec![para(vec![field(name, "캡")])], CaptionSide::Bottom);
            match &mut control {
                Control::TextBox { caption: c, .. }
                | Control::Polygon { caption: c, .. }
                | Control::Rect { caption: c, .. }
                | Control::Line { caption: c, .. }
                | Control::Curve { caption: c, .. } => *c = Some(caption),
                _ => panic!("captionable shape"),
            }
            control
        };
        let point = hwpforge_core::control::ShapePoint::new;
        let mut polygon =
            Control::polygon(vec![point(0, 0), point(3000, 0), point(0, 3000)]).expect("polygon");
        if let Control::Polygon { paragraphs, .. } = &mut polygon {
            *paragraphs = vec![para(vec![field("poly", "다")])];
        }
        let cell = |name: &str| TableCell::new(vec![para(vec![field(name, "칸")])], size(4000));
        let inner = Table::new(vec![TableRow::new(vec![cell("inner_cell")])]);
        let outer_cell =
            TableCell::new(vec![para(vec![RunContent::Table(Box::new(inner))])], size(4000));
        let table = Table::new(vec![TableRow::new(vec![cell("cell"), outer_cell])]).with_caption(
            Caption::new(vec![para(vec![field("table_caption", "표")])], CaptionSide::Bottom),
        );
        let doc = encode(
            vec![
                para(vec![field("body", "본")]),
                para(vec![RunContent::Table(Box::new(table))]),
                para(vec![control(Control::footnote(vec![para(vec![field("foot", "각")])]))]),
                para(vec![control(Control::endnote(vec![para(vec![field("end", "미")])]))]),
                para(vec![control(with_caption(
                    Control::text_box(vec![para(vec![field("box", "글")])], size(4000), size(2000)),
                    "box_caption",
                ))]),
                para(vec![control(with_caption(polygon, "poly_caption"))]),
                para(vec![control(with_caption(
                    Control::line(point(0, 0), point(3000, 0)).expect("line"),
                    "line_caption",
                ))]),
                para(vec![control(with_caption(
                    Control::curve(vec![point(0, 0), point(1500, 1500), point(3000, 0)])
                        .expect("curve"),
                    "curve_caption",
                ))]),
            ],
            &ImageStore::default(),
        );
        let xml = add_caches(&section_xml(&doc));
        let bytes = with_section_xml(&doc, xml.clone());
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        let names: Vec<String> = listed.iter().filter_map(|f| f.name.clone()).collect();
        assert_eq!(names.len(), 12, "premise: every field is in the inventory: {names:?}");
        let all: Vec<usize> = caches(&xml);
        for name in &names {
            let at = xml.find(&format!("name=\"{name}\"")).expect("field on the wire");
            let own = xml[..at].matches("</hp:p>").count();
            let filled = fill_xml(&bytes, &[(name.as_str(), "값")])
                .unwrap_or_else(|e| panic!("{name}: fill failed: {e}"));
            // W2: 흐름 꼬리와 host 사슬도 가므로 정확한 남은 집합은 그 규칙의
            // 전용 시험(`cell_fill_drops_the_flow_tail_and_the_host_chain`)이 맡고,
            // 여기서는 짝지은 문단의 캐시가 가고 첫 본문 문단(9000, `body`)은
            // 다른 필드일 때 남는지만 본다.
            let left = caches(&filled);
            assert!(!left.contains(&own), "{name}: its own paragraph cache goes: {left:?}");
            assert!(left.iter().all(|k| all.contains(k)), "{name}");
            if name != "body" {
                assert!(left.contains(&0), "{name}: the body paragraph before it keeps its cache");
            }
        }
    }

    #[test]
    fn cache_with_another_prefix_is_still_the_paragraph_cache() {
        // 디코더(serde)는 접두어 없는 이름으로 읽으므로 `p:linesegarray` 도
        // 문단 캐시다.
        // 이것을 실패시키는 것: 캐시를 접두어 포함 이름(`hp:linesegarray`)으로만
        // 판정 → `p:` 캐시가 남는다.
        let bytes = named_with(|xml| {
            xml.replacen(
                "<hs:sec ",
                "<hs:sec xmlns:p=\"http://www.hancom.co.kr/hwpml/2011/paragraph\" ",
                1,
            )
            .replace("hp:linesegarray", "p:linesegarray")
            .replace("<hp:lineseg ", "<p:lineseg ")
        });
        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        assert!(
            decoded.document.sections()[0].paragraphs.iter().any(|p| p.layout_cache.is_some()),
            "premise: the decoder reads the prefixed cache"
        );
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("linesegarray"), "prefixed cache must go: {xml}");
    }

    #[test]
    fn self_closing_field_begin_is_a_fill_target() {
        // 이것을 실패시키는 것: `Empty` 이벤트의 `fieldBegin` 을 무시(Start 만
        // 처리) → wire 에서 필드가 빠져 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:fieldBegin").unwrap();
            let head_end = xml[open..].find('>').unwrap() + open;
            let close = xml[open..].find("</hp:fieldBegin>").unwrap() + open;
            format!("{}/>{}", &xml[..head_end], &xml[close + "</hp:fieldBegin>".len()..])
        });
        assert!(
            section_xml(&bytes).contains("metaTag=\"\"/>"),
            "premise: fieldBegin is self-closing"
        );
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the decoder pairs the self-closing begin");
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn named_field_after_an_unnamed_one_keeps_its_ordinal() {
        // 9000 = 이름 없는 필드와 `b` 가 든 문단, 9001 = 다음 문단.
        // 이것을 실패시키는 것: 이름 없는 ClickHere 를 wire 쪽(또는 인벤토리 쪽)
        // 목록에서 빼기 → 같은 경로 안 순번이 어긋나 위치 불일치 오류.
        let unnamed = RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("무명".to_string()),
            help_text: None,
            name: None,
            display_text: "무명".to_string(),
        }));
        let doc = encode(
            vec![para(vec![unnamed, field("b", "나")]), para(vec![text("다음")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 2, "premise: the unnamed field is listed");
        let xml = fill_xml(&bytes, &[("b", "값")]).expect("fill");
        // W2: 다음 문단(9001)은 흐름 꼬리라 함께 간다(전에는 남겼다). 순번 시험의
        // 이빨은 위치 불일치 오류(`expect`)가 진다.
        assert_eq!(caches(&xml), Vec::<usize>::new());
    }

    #[test]
    fn wire_and_inventory_mismatch_is_rejected() {
        // 이것을 실패시키는 것: (경로, 순번, 이름) 집합 동일성 검사를 빼고
        // 이름만으로 고르기 → 아래 세 입력이 Ok 가 된다.
        let xml = section_xml(NAMED);
        let at = |name: &str| FieldLocator {
            path: vec![PathSeg::BodyParagraph(0)],
            name: Some(name.to_string()),
        };
        assert!(locate_changed_paragraph_caches(&xml, 0, &[at("user_email")], &[0]).is_ok());
        let renamed = locate_changed_paragraph_caches(&xml, 0, &[at("other")], &[0]);
        assert!(matches!(renamed, Err(HwpxError::InvalidStructure { .. })), "{renamed:?}");
        let missing = locate_changed_paragraph_caches(&xml, 0, &[at("user_email"), at("x")], &[0]);
        assert!(matches!(missing, Err(HwpxError::InvalidStructure { .. })), "{missing:?}");
        let extra = locate_changed_paragraph_caches(&xml, 0, &[], &[]);
        assert!(matches!(extra, Err(HwpxError::InvalidStructure { .. })), "{extra:?}");
        let moved = FieldLocator { path: vec![PathSeg::BodyParagraph(1)], ..at("user_email") };
        let moved = locate_changed_paragraph_caches(&xml, 0, &[moved], &[0]);
        assert!(matches!(moved, Err(HwpxError::InvalidStructure { .. })), "{moved:?}");
    }

    #[test]
    fn strip_reports_a_mismatch_instead_of_skipping_the_cache() {
        // 이것을 실패시키는 것: 위치 불일치를 무시하고 그 구역을 건너뛰기(옛
        // `None` = no-op 동작) → Ok 가 되어 낡은 캐시가 남은 채 출고된다.
        let mut inventory = BTreeMap::new();
        inventory.insert(
            0,
            vec![FieldLocator {
                path: vec![PathSeg::BodyParagraph(0)],
                name: Some("other".to_string()),
            }],
        );
        let result = strip_linesegarray_for_changed_fields(NAMED, &inventory, &[(0, 0)]);
        assert!(matches!(result, Err(HwpxError::InvalidStructure { .. })), "{result:?}");
    }
}
