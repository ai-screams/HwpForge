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

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_foundation::FieldType;

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
    pub(crate) control: &'a mut Control,
}

/// 값이 바뀐 필드들의 둘러싼 `<hp:p>` 에서 `<hp:linesegarray>` 를 제거한다.
///
/// 이름 유일성은 fill preflight(DuplicateFieldName 거부)가 보장하므로
/// 필드 이름으로 raw XML 에서 위치를 찾는 것이 무모호하다. 같은 문단의
/// 다중 slot 은 첫 제거 후 자연히 no-op (문단당 정확히 1개 edit —
/// §1g v5 변경 5).
fn strip_linesegarray_for_changed_fields(
    bytes: &[u8],
    changed: &[(usize, String)],
) -> crate::error::HwpxResult<Vec<u8>> {
    if changed.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut pkg = crate::patch::RawPackage::read(bytes)?;
    let mut sections: std::collections::BTreeMap<usize, Vec<&str>> =
        std::collections::BTreeMap::new();
    for (section_idx, name) in changed {
        sections.entry(*section_idx).or_default().push(name.as_str());
    }
    for (section_idx, names) in sections {
        let path = crate::patch::section_path(section_idx);
        let mut xml = pkg.read_text_entry(&path)?;
        for name in names {
            if let Some(stripped) = strip_paragraph_linesegarray_for_field(&xml, name) {
                xml = stripped;
            }
        }
        pkg.replace_text_entry(&path, xml);
    }
    pkg.write()
}

/// `name` 필드의 fieldBegin 을 찾아 그 문단의 linesegarray 를 제거한 XML 을
/// 돌려준다. 필드/linesegarray 부재 시 `None` (무캐시 문단 = no-op).
fn strip_paragraph_linesegarray_for_field(xml: &str, name: &str) -> Option<String> {
    // fieldBegin 태그 중 name 속성이 일치하는 첫 위치.
    let needle = format!("name=\"{name}\"");
    let mut search = 0usize;
    let field_pos = loop {
        let at = xml[search..].find("<hp:fieldBegin")? + search;
        let tag_end = xml[at..].find('>')? + at;
        if xml[at..tag_end].contains(&needle) {
            break at;
        }
        search = tag_end + 1;
    };
    let (p_start, p_end) = enclosing_paragraph_span(xml, field_pos)?;
    let para = &xml[p_start..p_end];
    let lsa_rel = para.find("<hp:linesegarray")?;
    let lsa_start = p_start + lsa_rel;
    let first_gt = xml[lsa_start..].find('>')? + lsa_start;
    let lsa_end = if xml.as_bytes().get(first_gt.checked_sub(1)?) == Some(&b'/') {
        first_gt + 1
    } else {
        let close = xml[lsa_start..p_end].find("</hp:linesegarray>")?;
        lsa_start + close + "</hp:linesegarray>".len()
    };
    let mut out = String::with_capacity(xml.len());
    out.push_str(&xml[..lsa_start]);
    out.push_str(&xml[lsa_end..]);
    Some(out)
}

/// `pos` 를 포함하는 가장 안쪽 `<hp:p …>…</hp:p>` 구간.
///
/// 중첩 문단(셀 등)은 안쪽이 먼저 닫히므로, pos 이후 처음 닫히는 열림이
/// 곧 innermost 다.
fn enclosing_paragraph_span(xml: &str, pos: usize) -> Option<(usize, usize)> {
    let bytes = xml.as_bytes();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0usize;
    loop {
        let open = xml[i..].find("<hp:p").map(|o| i + o);
        let close = xml[i..].find("</hp:p>").map(|o| i + o);
        let take_open = match (open, close) {
            (None, None) => return None,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(o), Some(c)) => o < c,
        };
        if take_open {
            let o = open.expect("checked");
            // `<hp:pic` 등 배제 — 다음 문자가 공백/`>` 인 경우만 문단.
            if matches!(bytes.get(o + 5), Some(b' ') | Some(b'>')) {
                stack.push(o);
            }
            i = o + 5;
        } else {
            let c = close.expect("checked");
            if let Some(s) = stack.pop() {
                if s < pos && pos < c {
                    return Some((s, c + "</hp:p>".len()));
                }
            }
            i = c + "</hp:p>".len();
        }
    }
}

impl HwpxFiller {
    /// 문서의 모든 누름틀을 문서 순서로 나열한다 (발견가능성 표면).
    ///
    /// # Errors
    ///
    /// 패키지 디코드에 실패하면 [`HwpxError`] 를 반환한다.
    pub fn list_fields(base: &[u8]) -> HwpxResult<Vec<FieldInfo>> {
        let mut decoded = HwpxDecoder::decode(base)?;
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
        Ok(fields)
    }

    /// 이름 → 값 맵으로 누름틀을 채운 새 패키지 바이트를 만든다.
    ///
    /// 전량 preflight 후 전량 적용 — 어느 하나라도 실패하면 아무 것도
    /// 쓰지 않고 [`FillError`] 를 반환한다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조.
    pub fn fill(base: &[u8], values: &BTreeMap<String, String>) -> Result<FillOutcome, FillError> {
        // ── preflight 1: 값 자체 검증 ──
        for (name, value) in values {
            if value.is_empty() {
                return Err(FillError::EmptyValue { name: name.clone() });
            }
        }

        let mut decoded = HwpxDecoder::decode(base)?;

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

        // ── 적용: 대상 필드 mutate + 섹션별 preserve-first patch 체이닝 ──
        let original_sections: Vec<Section> = decoded.document.sections().to_vec();
        let mut filled: Vec<FilledField> = Vec::new();
        let mut touched: Vec<usize> = Vec::new();
        let mut changed_fields: Vec<(usize, String)> = Vec::new();
        for (section_idx, section) in decoded.document.sections_mut().iter_mut().enumerate() {
            visit_section_fields(section, section_idx, &mut |slot| {
                if let Control::Field {
                    field_type: FieldType::ClickHere,
                    name: Some(name),
                    display_text,
                    ..
                } = &mut *slot.control
                {
                    if let Some(value) = values.get(name.as_str()) {
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
                        changed_fields.push((slot.section, name.clone()));
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
        bytes = strip_linesegarray_for_changed_fields(&bytes, &changed_fields)?;

        Ok(FillOutcome { bytes, filled })
    }
}

/// 섹션 안의 모든 [`Control`] 을 patch 텍스트-슬롯 순회와 동일한 커버리지로
/// 방문한다 (`collect_semantic_control_slots` / `redact_control` 의 거울 —
/// 여기 없는 컨테이너(예: `Group`)의 필드는 슬롯도 없으므로 fill 대상이
/// 아니어야 일관된다).
pub(crate) fn visit_section_fields(
    section: &mut Section,
    section_idx: usize,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    visit_paragraphs(&mut section.paragraphs, section_idx, f);
}

fn visit_paragraphs(
    paragraphs: &mut [Paragraph],
    section_idx: usize,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    for paragraph in paragraphs {
        for run in &mut paragraph.runs {
            match &mut run.content {
                RunContent::Control(control) => visit_control(control, section_idx, f),
                RunContent::Table(table) => {
                    if let Some(caption) = table.caption.as_mut() {
                        visit_paragraphs(&mut caption.paragraphs, section_idx, f);
                    }
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            visit_paragraphs(&mut cell.paragraphs, section_idx, f);
                        }
                    }
                }
                RunContent::Image(image) => {
                    if let Some(caption) = image.caption.as_mut() {
                        visit_paragraphs(&mut caption.paragraphs, section_idx, f);
                    }
                }
                _ => {}
            }
        }
    }
}

fn visit_control(
    control: &mut Control,
    section_idx: usize,
    f: &mut impl FnMut(&mut FieldSlot<'_>),
) {
    f(&mut FieldSlot { section: section_idx, control });
    match control {
        Control::TextBox { paragraphs, caption, .. }
        | Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            visit_paragraphs(paragraphs, section_idx, f);
            if let Some(caption) = caption.as_mut() {
                visit_paragraphs(&mut caption.paragraphs, section_idx, f);
            }
        }
        Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
            visit_paragraphs(paragraphs, section_idx, f);
        }
        Control::Rect { caption: Some(caption), .. }
        | Control::Line { caption: Some(caption), .. }
        | Control::Arc { caption: Some(caption), .. }
        | Control::Curve { caption: Some(caption), .. }
        | Control::ConnectLine { caption: Some(caption), .. } => {
            visit_paragraphs(&mut caption.paragraphs, section_idx, f);
        }
        _ => {}
    }
}
