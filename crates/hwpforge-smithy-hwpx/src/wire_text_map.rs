//! Wire↔Core 텍스트 좌표 맵 (W1b) — 이 crate 내부 전용.
//!
//! wire 좌표(HWP5 raw 스트림 = 한컴 HWPX linesegarray textpos 규약)와
//! Core 정준 좌표(`Paragraph::text_content()` UTF-16 축) 사이의 변환을
//! 담당한다. 설계 정본: `.docs/planning/2026-08-13-image-textbox-epic.md`
//! §1g v5 (Codex 4라운드 확정 수학 법칙).
//!
//! ## 모델
//!
//! 문단의 wire 스트림은 **비-1:1 span** (marker `(8,0)`, tab `(8,1)`,
//! 무음 소비 `(1,0)` 등) 과 그 사이의 **1:1 구간** (일반 텍스트) 으로
//! 구성된다. 맵은 비-1:1 span 만 보유하며 (1:1 span 은 push 시점에
//! identity 로 흡수), 총길이 `wire_end`/`core_end` 를 seal 시점에 고정한다.
//!
//! ## 법칙 (v5 확정 — proptest 로 잠금)
//!
//! - 입력 domain = inclusive `[0, wire_end]` / `[0, core_end]`, 밖은 Err.
//! - `to_core(0) = 0` · `to_wire(0) = 0` 무조건 성립 (canonicalization —
//!   native 291파일·13,598 linesegarray 전수에서 첫 lineseg textpos=0
//!   100% 실측).
//! - 1:1 **열린 구간에서만** 양방향 합성이 identity.
//! - 비-1:1 span 의 wire strict interior 는 `to_core = Err`.
//! - 동일 core 좌표에 wire preimage 2개 이상이면 `to_wire = Err`
//!   (단 core 0 은 canonicalization 으로 항상 wire 0).
//!
//! ⚠️ smithy-hwp5 의 동명 모듈과 **동형** — conformance vector 테스트를
//! 양쪽에 복제해 drift 를 방지한다 (Core 로의 승격은 계층 누수라 기각,
//! §1g v2 disposition #2).

use std::fmt;

/// 비-1:1 wire 소비 구간 하나.
///
/// `wire_len != core_len` 이 불변식이다 (1:1 은 span 이 아니라 구간).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireSpan {
    /// wire 좌표계에서의 시작 위치.
    pub wire_start: u32,
    /// wire 유닛 소비량 (≥ 1).
    pub wire_len: u32,
    /// Core 축 기여량 (marker=0, tab=1 등).
    pub core_len: u32,
}

/// 좌표 변환/맵 구성 실패 사유.
///
/// 캐시 fail-closed 경고(`DecodeWarning`/`EncodeWarning`)의 payload 로
/// 전파된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CacheMapError {
    /// 질의 좌표가 `[0, wire_end]` / `[0, core_end]` 밖.
    OutOfRange {
        /// 질의 좌표.
        pos: u32,
        /// 해당 축의 총길이.
        end: u32,
    },
    /// `to_core` 질의가 비-1:1 span 의 strict interior 에 위치.
    InsideContraction {
        /// 질의 wire 좌표.
        wire: u32,
    },
    /// `to_wire` 질의 core 좌표의 wire preimage 가 2개 이상 (core 0 제외).
    AmbiguousPreimage {
        /// 질의 core 좌표.
        core: u32,
    },
    /// span 정렬/중첩/불변식 위반 또는 checked 산술 overflow.
    InvalidSpans,
}

impl fmt::Display for CacheMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { pos, end } => {
                write!(f, "coordinate {pos} outside [0, {end}]")
            }
            Self::InsideContraction { wire } => {
                write!(f, "wire {wire} inside a non-1:1 span interior")
            }
            Self::AmbiguousPreimage { core } => {
                write!(f, "core {core} has multiple wire preimages")
            }
            Self::InvalidSpans => write!(f, "span set violates map invariants"),
        }
    }
}

/// seal 된 wire↔Core 좌표 맵.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireTextMap {
    /// 비-1:1 span 들 (wire_start 오름차순, 비중첩).
    spans: Vec<WireSpan>,
    /// wire 축 총길이.
    wire_end: u32,
    /// Core 축 총길이.
    core_end: u32,
}

impl WireTextMap {
    /// wire 축 총길이.
    pub(crate) fn wire_end(&self) -> u32 {
        self.wire_end
    }

    /// Core 축 총길이.
    pub(crate) fn core_end(&self) -> u32 {
        self.core_end
    }

    /// wire 좌표 → Core 좌표.
    ///
    /// **문단부호 sentinel (wire끝+1)**: Square 어울림 앵커가 행을 좌/우로
    /// 쪼갠 문단에서, 텍스트 소진 후의 오른쪽 빈 세그먼트 lineseg 를
    /// 한컴이 문단부호 좌표(= wire끝+1)로 방출한다 — byte 확정 실측 2건
    /// (에픽 문서 §10h: 52/[0,51] · 267/[0,266]). Core 끝으로 수용한다.
    /// 그 외 초과는 기존대로 [`CacheMapError::OutOfRange`] (임의 클램프
    /// 금지). **HWPX 실측 한정** — HWP5 동형 사본에는 native 실측 전
    /// 이식 금지. 왕복 비대칭: 재인코드([`Self::to_wire`])는 축약점
    /// 규약(earliest preimage)대로 방출하므로 이 sentinel 은 byte
    /// 복제되지 않는다 (known 정규화 — §11b F2).
    pub(crate) fn to_core(&self, wire: u32) -> Result<u32, CacheMapError> {
        if self.wire_end.checked_add(1) == Some(wire) {
            return Ok(self.core_end);
        }
        if wire > self.wire_end {
            return Err(CacheMapError::OutOfRange { pos: wire, end: self.wire_end });
        }
        let mut wire_cursor = 0u32;
        let mut core_cursor = 0u32;
        for span in &self.spans {
            if wire <= span.wire_start {
                // 1:1 구간 (span 시작점 포함 — 시작점의 core 값은 구간
                // 공식과 동일).
                return Ok(core_cursor + (wire - wire_cursor));
            }
            let span_end = span.wire_start + span.wire_len;
            if wire < span_end {
                return Err(CacheMapError::InsideContraction { wire });
            }
            core_cursor += (span.wire_start - wire_cursor) + span.core_len;
            wire_cursor = span_end;
        }
        Ok(core_cursor + (wire - wire_cursor))
    }

    /// Core 좌표 → wire 좌표.
    ///
    /// **축약점(영-core span 경계, preimage ≥ 2)은 earliest preimage**
    /// (marker 시작) — W3 실측 개정 (에픽 문서 §7): native 셀 fixture
    /// 2건의 raw tp 가 모두 marker 시작이고, 첫 lineseg tp=0 전수
    /// 불변식(13,598건)도 동일 규약이다 (W1b 의 Err 는 미측정 시점의
    /// fail-closed — 측정이 규약을 확정함). `AmbiguousPreimage` 는
    /// 다중-core span 내부(현 span 종류엔 없음 — 방어)에만 남는다.
    pub(crate) fn to_wire(&self, core: u32) -> Result<u32, CacheMapError> {
        if core > self.core_end {
            return Err(CacheMapError::OutOfRange { pos: core, end: self.core_end });
        }
        if core == 0 {
            return Ok(0);
        }
        let mut wire_cursor = 0u32;
        let mut core_cursor = 0u32;
        for span in &self.spans {
            let gap_core_end = core_cursor + (span.wire_start - wire_cursor);
            if core < gap_core_end {
                // 1:1 열린 구간 — 유일 preimage.
                return Ok(wire_cursor + (core - core_cursor));
            }
            if core == gap_core_end {
                // 영-core span 경계 = 축약점: earliest preimage(= span
                // 시작 = 직전 1:1 구간의 그 core 위치)로 확정 — 실측
                // 규약 (한컴은 경계 lineseg 를 marker 시작에 쓴다).
                // core 기여 span (tab 등)도 span 시작이 유일 preimage 라
                // 같은 식이 성립한다.
                return Ok(span.wire_start);
            }
            let span_core_end = gap_core_end + span.core_len;
            if core < span_core_end {
                // span core 기여의 strict interior (core_len ≥ 2 인 경우만
                // 도달 가능 — 현재 알려진 span 종류엔 없음, 방어적 Err).
                return Err(CacheMapError::AmbiguousPreimage { core });
            }
            core_cursor = span_core_end;
            wire_cursor = span.wire_start + span.wire_len;
        }
        Ok(wire_cursor + (core - core_cursor))
    }
}

/// [`WireTextMap`] 증분 구성기.
///
/// 변환기(디코더/projection/인코더 방출부)가 **wire 순서대로** 소비를
/// 기록한다: 1:1 텍스트는 [`advance_identity`], 비-1:1 소비는
/// [`push_span`]. [`finish`] 가 불변식 검증 후 seal 한다.
///
/// field pending(begin~end 사이 core 기여 확정 지연)은 변환기 측 책임 —
/// 이 빌더는 확정된 소비만 순서대로 받는다 (v5 lifecycle: FieldEnd 에서
/// wire 순서대로 commit).
///
/// [`advance_identity`]: WireMapBuilder::advance_identity
/// [`push_span`]: WireMapBuilder::push_span
/// [`finish`]: WireMapBuilder::finish
#[derive(Debug, Default)]
pub(crate) struct WireMapBuilder {
    spans: Vec<WireSpan>,
    wire_cursor: u32,
    core_cursor: u32,
    overflowed: bool,
}

impl WireMapBuilder {
    /// 새 빌더 (빈 문단 = 그대로 `finish` 하면 `(0,0)` 맵).
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 지금까지 기록한 `(wire, core)` 길이.
    pub(crate) fn cursors(&self) -> (u32, u32) {
        (self.wire_cursor, self.core_cursor)
    }

    /// 1:1 구간 전진 (일반 텍스트 UTF-16 `units`).
    pub(crate) fn advance_identity(&mut self, units: u32) {
        match (self.wire_cursor.checked_add(units), self.core_cursor.checked_add(units)) {
            (Some(w), Some(c)) => {
                self.wire_cursor = w;
                self.core_cursor = c;
            }
            _ => self.overflowed = true,
        }
    }

    /// 비-1:1 소비 기록 (`wire_len != core_len`).
    ///
    /// `wire_len == core_len` 이면 identity 로 흡수한다 (span 최소성).
    pub(crate) fn push_span(&mut self, wire_len: u32, core_len: u32) {
        if wire_len == core_len {
            self.advance_identity(wire_len);
            return;
        }
        if wire_len == 0 {
            // core 만 늘어나는 span 은 존재하지 않음 (markpen 은 (0,0) —
            // 어느 축도 소비하지 않으므로 기록 자체가 불필요).
            self.overflowed = true;
            return;
        }
        let span = WireSpan { wire_start: self.wire_cursor, wire_len, core_len };
        match (self.wire_cursor.checked_add(wire_len), self.core_cursor.checked_add(core_len)) {
            (Some(w), Some(c)) => {
                self.wire_cursor = w;
                self.core_cursor = c;
                self.spans.push(span);
            }
            _ => self.overflowed = true,
        }
    }

    /// 불변식 검증 후 seal.
    ///
    /// 검증: overflow 부재 · span 정렬/비중첩 · `wire_len != core_len` ·
    /// 누적 합 == 커서 (구성 오류 방어).
    pub(crate) fn finish(self) -> Result<WireTextMap, CacheMapError> {
        if self.overflowed {
            return Err(CacheMapError::InvalidSpans);
        }
        let mut expect_wire = 0u32;
        let mut expect_core = 0u32;
        for span in &self.spans {
            if span.wire_start < expect_wire || span.wire_len == 0 || span.wire_len == span.core_len
            {
                return Err(CacheMapError::InvalidSpans);
            }
            expect_core = expect_core
                .checked_add(span.wire_start - expect_wire)
                .and_then(|c| c.checked_add(span.core_len))
                .ok_or(CacheMapError::InvalidSpans)?;
            expect_wire =
                span.wire_start.checked_add(span.wire_len).ok_or(CacheMapError::InvalidSpans)?;
        }
        // trailing 1:1 구간 반영.
        let tail = self.wire_cursor.checked_sub(expect_wire).ok_or(CacheMapError::InvalidSpans)?;
        expect_core = expect_core.checked_add(tail).ok_or(CacheMapError::InvalidSpans)?;
        if expect_core != self.core_cursor {
            return Err(CacheMapError::InvalidSpans);
        }
        Ok(WireTextMap {
            spans: self.spans,
            wire_end: self.wire_cursor,
            core_end: self.core_cursor,
        })
    }
}

/// 영-core wire 토큰 하나 (S22) — 줄 경계의 종류 열을 적는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZeroToken {
    /// 토큰이 놓인 Core 위치.
    pub core: u32,
    /// wire 시작 위치.
    pub wire_start: u32,
    /// wire 길이.
    pub wire_len: u32,
    /// 종류.
    pub kind: hwpforge_core::layout::WireTokenKind,
}

/// 문단의 영-core 토큰 목록, wire 순서 (S22). 좌표 맵([`WireTextMap`])과
/// 나란히 기록하고 맵 자체는 바꾸지 않는다 — 경계 없는 줄은 지금처럼
/// 변환된다.
///
/// 줄마다 하는 조회(같은 Core 위치의 토큰, 필드 안인지)는 색인으로 한다 —
/// 줄 수 × 토큰 수로 다시 훑지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TokenLedger {
    tokens: Vec<ZeroToken>,
    /// Core 위치별 토큰 번호, wire 순서.
    by_core: std::collections::BTreeMap<u32, Vec<usize>>,
    /// 아직 닫히지 않은 필드 시작 토큰의 wire 시작, 안쪽이 끝.
    open: Vec<u32>,
    /// 닫힌 필드 구간 `(시작 토큰의 시작, 끝 토큰의 끝)`, 닫힌 순서.
    fields: Vec<(u32, u32)>,
    /// `fields` 를 시작 순으로 정렬하고 끝을 앞에서부터의 최댓값으로 바꾼 것 —
    /// 첫 조회 때 만든다.
    field_index: std::cell::OnceCell<Vec<(u32, u32)>>,
    /// 조회가 살펴본 토큰·구간 수(선형성 시험용).
    #[cfg(test)]
    visits: std::cell::Cell<usize>,
}

impl TokenLedger {
    /// 토큰 하나를 더한다 (wire 순서대로). 바로 앞 토큰에 이어지는 `Text`
    /// (필드 본문의 글자·탭이 여러 조각으로 오는 경우)는 그 토큰에 합친다 —
    /// 인코더는 접힌 필드 본문을 한 토큰으로 적는다.
    pub(crate) fn push(&mut self, token: ZeroToken) {
        use hwpforge_core::layout::WireTokenKind::{FieldBegin, FieldEnd, Text};
        if let Some(last) = self.tokens.last_mut() {
            if token.kind == Text
                && last.kind == Text
                && last.core == token.core
                && last.wire_start + last.wire_len == token.wire_start
            {
                last.wire_len += token.wire_len;
                return;
            }
        }
        match token.kind {
            FieldBegin => self.open.push(token.wire_start),
            FieldEnd => {
                if let Some(start) = self.open.pop() {
                    self.fields.push((start, token.wire_start + token.wire_len));
                    self.field_index = std::cell::OnceCell::new();
                }
            }
            _ => {}
        }
        self.by_core.entry(token.core).or_default().push(self.tokens.len());
        self.tokens.push(token);
    }

    /// `wire` 가 필드 안(필드 시작 토큰의 시작 < `wire` < 필드 끝 토큰의 끝)인지
    /// (S22 설계 3.3 — 한컴 쌍둥이 22,356 줄 중 0). 그런 줄은 세 경로 모두
    /// 캐시를 버린다.
    ///
    /// 시작과 끝이 이 문단에 다 있는 필드만 본다. 문단을 가로지르는 필드(한컴
    /// 보도자료 서식의 "본문" 누름틀 — 한컴 저장 HWPX 3,517 문서 중 858)는
    /// 시작·끝 토큰만 영-core 이고 본문 글자가 1:1 이라 줄 자리가 Core 글자와
    /// 맞는다(S22 r1 실측: 시작·끝이 든 문단 1,719 개의 둘째 줄부터 3,199 줄 중
    /// 불일치 10, 모두 필드와 무관 — 보조 평면 글자 8, `<hp:fwSpace/>` 뒤
    /// 공백뿐인 꼬리 2). 시작과 같은 run 에서 접힌 본문 안의 줄은 좌표 맵이
    /// 수축 안쪽으로 거부한다.
    pub(crate) fn inside_field(&self, wire: u32) -> bool {
        let index = self.field_index.get_or_init(|| {
            #[cfg(test)]
            self.visits.set(self.visits.get() + self.fields.len());
            let mut spans = self.fields.clone();
            spans.sort_unstable();
            let mut reach = 0;
            spans
                .into_iter()
                .map(|(start, end)| {
                    reach = reach.max(end);
                    (start, reach)
                })
                .collect()
        });
        // 시작이 `wire` 앞인 구간 중 가장 멀리 가는 끝이 `wire` 뒤면 안이다.
        let before = index.partition_point(|&(start, _)| start < wire);
        before > 0 && index[before - 1].1 > wire
    }

    /// wire 위치 `wire`(Core 위치 `core`)에서 시작하는 줄의 경계 — 그 위치의
    /// 토큰을 하나도 지나지 않으면 `None`(가장 이른 자리, 좌표만으로 정해짐).
    pub(crate) fn boundary_at(
        &self,
        core: u32,
        wire: u32,
    ) -> Option<hwpforge_core::layout::LineBoundary> {
        let here = self.at(core);
        // S22 r2: binary search for the tokens before `wire`, then read only
        // that prefix and the token after it.
        let k = here.partition_point(|&i| self.tokens[i].wire_start < wire);
        (k > 0).then(|| {
            self.count_visits(k + 1);
            hwpforge_core::layout::LineBoundary::new(
                here[..k].iter().map(|&i| self.tokens[i].kind).collect(),
                here.get(k).map(|&i| self.tokens[i].kind),
            )
        })
    }

    /// Core 위치 `core` 의 토큰 번호들, wire 순서 — 색인을 빌린다(복제하지
    /// 않는다).
    fn at(&self, core: u32) -> &[usize] {
        self.by_core.get(&core).map_or(&[], Vec::as_slice)
    }

    /// 조회가 살펴본 토큰 수를 센다(선형성 시험용).
    fn count_visits(&self, _n: usize) {
        #[cfg(test)]
        self.visits.set(self.visits.get() + _n);
    }

    /// 경계가 가리키는 줄 시작의 wire 위치. 종류 열이 맞지 않거나, 그 자리가
    /// 토큰 안(접힌 필드 본문 등 — `to_core` 가 받지 않는 자리)이면 `Err`.
    pub(crate) fn place(
        &self,
        map: &WireTextMap,
        core: u32,
        boundary: &hwpforge_core::layout::LineBoundary,
    ) -> Result<u32, String> {
        let here = self.at(core);
        let k = boundary.skip.len();
        let kinds = || -> Vec<_> { here.iter().map(|&i| self.tokens[i].kind).collect() };
        // S22 r2: only the skipped prefix and the token after it are read.
        self.count_visits(k.min(here.len()) + 1);
        let matches = here.len() >= k
            && here[..k].iter().zip(&boundary.skip).all(|(&i, kind)| self.tokens[i].kind == *kind);
        if !matches {
            return Err(format!(
                "line boundary at core {core}: skip {:?} does not match the tokens {:?}",
                boundary.skip,
                kinds()
            ));
        }
        let wire = match boundary.next {
            Some(next) => match here.get(k).map(|&i| &self.tokens[i]) {
                Some(t) if t.kind == next => t.wire_start,
                _ => {
                    return Err(format!(
                        "line boundary at core {core}: token {k} is not {next:?} ({:?})",
                        kinds()
                    ))
                }
            },
            None if k == here.len() => match k.checked_sub(1) {
                Some(last) => {
                    let t = &self.tokens[here[last]];
                    t.wire_start + t.wire_len
                }
                None => map.to_wire(core).map_err(|e| e.to_string())?,
            },
            None => {
                return Err(format!(
                    "line boundary at core {core}: {} token(s) left after skip ({:?})",
                    here.len() - k,
                    kinds()
                ))
            }
        };
        match map.to_core(wire) {
            Ok(c) if c == core => Ok(wire),
            _ => Err(format!("line boundary at core {core}: wire {wire} is inside a token")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S22 r1: the per-line lookups use the ledger's index — N lines over
    /// a paragraph of 3N tokens look at O(N) tokens and spans in all, not N².
    /// 이것을 실패시키는 것: `at` 를 전체 토큰 필터로, 또는 `inside_field` 를
    /// 질의마다 전체 토큰 순회로 되돌리는 것(살펴본 수가 N² 로 는다).
    /// S22 r2: N tokens and N lines at one Core position (bookmarks with no
    /// text, every line at 0) — each lookup reads only the prefix it needs,
    /// not the whole position.
    /// 이것을 실패시키는 것: 조회마다 그 위치의 토큰 전체를 복제·순회하는 것
    /// (살펴본 수 N²).
    #[test]
    fn lookups_at_one_crowded_position_stay_linear() {
        use hwpforge_core::layout::{LineBoundary, WireTokenKind::Bookmark};
        let n = 4096u32;
        let mut b = WireMapBuilder::new();
        let mut ledger = TokenLedger::default();
        for i in 0..n {
            ledger.push(ZeroToken { core: 0, wire_start: 8 * i, wire_len: 8, kind: Bookmark });
            b.push_span(8, 0);
        }
        let map = b.finish().expect("valid map");
        let after_one = LineBoundary::new(vec![Bookmark], Some(Bookmark));
        for _ in 0..n {
            assert_eq!(ledger.boundary_at(0, 0), None);
            assert_eq!(ledger.place(&map, 0, &after_one), Ok(8));
        }
        let visits = ledger.visits.get();
        assert!(visits <= 3 * n as usize, "{visits} lookups for {n} lines");
    }

    #[test]
    fn per_line_lookups_stay_linear() {
        use hwpforge_core::layout::WireTokenKind::{Bookmark, FieldBegin, FieldEnd};
        let n = 2048u32;
        let mut ledger = TokenLedger::default();
        // At Core i (wire 25i): a field (begin, end) then a bookmark, then
        // one text unit.
        for i in 0..n {
            let w = 25 * i;
            ledger.push(ZeroToken { core: i, wire_start: w, wire_len: 8, kind: FieldBegin });
            ledger.push(ZeroToken { core: i, wire_start: w + 8, wire_len: 8, kind: FieldEnd });
            ledger.push(ZeroToken { core: i, wire_start: w + 16, wire_len: 8, kind: Bookmark });
        }
        for i in 0..n {
            let w = 25 * i;
            let boundary = ledger.boundary_at(i, w + 16).expect("after the field");
            assert_eq!(boundary.skip, [FieldBegin, FieldEnd]);
            assert!(ledger.inside_field(w + 4), "inside field {i}");
            assert!(!ledger.inside_field(w + 16), "after field {i}");
        }
        let visits = ledger.visits.get();
        assert!(visits <= 5 * n as usize, "{visits} lookups for {n} lines");
    }

    /// conformance vector — smithy-hwp5 의 동명 모듈 테스트와 동일하게
    /// 유지할 것 (동형성 게이트).
    fn marker_ledger_p1() -> WireTextMap {
        // rules-marker-ledger p1: 텍스트 17유닛 + nwno(8,0)×2 + 텍스트.
        // raw lineseg [0, 76, 132] → core [0, 60, 116].
        let mut b = WireMapBuilder::new();
        b.advance_identity(17);
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(148 - 33); // 총 raw 길이 148 가정 (본문 잔여)
        b.finish().expect("valid map")
    }

    // ── 빈/퇴화 문단 ────────────────────────────────────────────

    #[test]
    fn empty_paragraph_succeeds_only_at_zero() {
        let map = WireMapBuilder::new().finish().expect("empty map");
        assert_eq!(map.wire_end(), 0);
        assert_eq!(map.core_end(), 0);
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_wire(0), Ok(0));
        // wire끝+1 = 문단부호 sentinel (§10h) — 빈 문단에도 문단부호는
        // 존재하므로 수용된다. +2 부터 거부.
        assert_eq!(map.to_core(1), Ok(0));
        assert_eq!(map.to_core(2), Err(CacheMapError::OutOfRange { pos: 2, end: 0 }));
        // to_wire 는 sentinel 미보유 (왕복 비대칭 — 인코드는 축약점 규약).
        assert_eq!(map.to_wire(1), Err(CacheMapError::OutOfRange { pos: 1, end: 0 }));
    }

    #[test]
    fn control_only_paragraph_collapses_to_core_zero() {
        // (wire_end>0, core_end=0): to_core(0)=0, to_core(wire_end)=0,
        // to_wire(0)=0 (canonicalization).
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 8);
        assert_eq!(map.core_end(), 0);
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(8), Ok(0));
        assert_eq!(map.to_core(3), Err(CacheMapError::InsideContraction { wire: 3 }));
        assert_eq!(map.to_wire(0), Ok(0));
    }

    // ── canonicalization (실측 불변식) ──────────────────────────

    #[test]
    fn leading_marker_canonicalizes_zero_both_ways() {
        // HWP5 첫 문단 프렐류드 (secd+cold = 16유닛) 후 텍스트 4유닛.
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(4);
        let map = b.finish().expect("valid");
        // to_core: 0 과 16(양 span 경계) 모두 core 0 — 경계는 합법.
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(8), Ok(0));
        assert_eq!(map.to_core(16), Ok(0));
        // to_wire(0) 은 preimage {0,8,16} 중 canonicalization 으로 0.
        assert_eq!(map.to_wire(0), Ok(0));
        assert_eq!(map.to_wire(4), Ok(20));
    }

    // ── 1:1 열린 구간 왕복 ──────────────────────────────────────

    #[test]
    fn open_interval_roundtrip_is_identity() {
        let map = marker_ledger_p1();
        // 골든: raw [0,76,132] ↔ core [0,60,116].
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(76), Ok(60));
        assert_eq!(map.to_core(132), Ok(116));
        assert_eq!(map.to_wire(60), Ok(76));
        assert_eq!(map.to_wire(116), Ok(132));
        // 왕복 항등 (1:1 구간).
        for w in [1u32, 16, 40, 76, 100, 132, 147] {
            let c = map.to_core(w).expect("in gap");
            assert_eq!(map.to_wire(c), Ok(w), "roundtrip at wire {w}");
        }
    }

    // ── strict interior / 축약점 ───────────────────────────────

    #[test]
    fn marker_interior_is_error() {
        let map = marker_ledger_p1();
        for w in [18u32, 20, 24, 26, 30, 32] {
            assert_eq!(
                map.to_core(w),
                Err(CacheMapError::InsideContraction { wire: w }),
                "wire {w}"
            );
        }
        // span 시작/끝 경계는 합법.
        assert_eq!(map.to_core(17), Ok(17));
        assert_eq!(map.to_core(25), Ok(17));
        assert_eq!(map.to_core(33), Ok(17));
    }

    #[test]
    fn mid_paragraph_collapse_point_resolves_to_earliest_preimage() {
        let map = marker_ledger_p1();
        // core 17 의 preimage = {17, 25, 33} (연속 영-core span) —
        // earliest(marker 시작) 확정 (W3 실측 개정).
        assert_eq!(map.to_wire(17), Ok(17));
        // 왕복: earliest 로 되돌려도 core 는 동일.
        assert_eq!(map.to_core(17), Ok(17));
    }

    #[test]
    fn trailing_zero_core_span_core_end_resolves_to_earliest() {
        // 텍스트 4 + marker(8,0) 로 끝나는 문단: core_end=4 의 preimage =
        // {4, 12} → Err.
        let mut b = WireMapBuilder::new();
        b.advance_identity(4);
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.core_end(), 4);
        // trailing 영-core span 의 core_end 도 earliest (marker 앞).
        assert_eq!(map.to_wire(4), Ok(4));
        assert_eq!(map.to_wire(3), Ok(3));
    }

    // ── 문단부호(+1) sentinel (§10h — Square 앵커 분할 행) ─────────

    #[test]
    fn paragraph_mark_sentinel_maps_to_core_end() {
        // evidence1 (짧은 제목 문단): secPr(8,0)+colPr(8,0)+text 27+pic(8,0)
        // → wire_end=51, core_end=27. 한컴 trailing 빈 세그먼트 tp=52.
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(27);
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 51);
        assert_eq!(map.core_end(), 27);
        assert_eq!(map.to_core(52), Ok(27));
        // sentinel 초과(+2 이상)는 여전히 거부 — 임의 클램프 금지.
        assert_eq!(map.to_core(53), Err(CacheMapError::OutOfRange { pos: 53, end: 51 }));
    }

    #[test]
    fn paragraph_mark_sentinel_long_paragraph() {
        // evidence2 (258자 문단 + 문단끝 pic): wire_end=266 → tp=267 수용.
        let mut b = WireMapBuilder::new();
        b.advance_identity(258);
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 266);
        assert_eq!(map.to_core(267), Ok(258));
        assert_eq!(map.to_core(268), Err(CacheMapError::OutOfRange { pos: 268, end: 266 }));
    }

    #[test]
    fn paragraph_mark_sentinel_roundtrip_normalizes_to_contraction_point() {
        // 왕복 비대칭 잠금 (§11b F2 Critical): sentinel 로 디코드한
        // core_end 를 재인코드하면 축약점 규약(earliest preimage = marker
        // 시작 = 43)대로 방출된다 — 원본 byte(+1 sentinel = 52)는 복제되지
        // 않는다 (known 정규화, 인코드 방향 byte 복제는 스코프 아님).
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(27);
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.to_core(52), Ok(27));
        assert_eq!(map.to_wire(27), Ok(43));
    }

    #[test]
    fn paragraph_mark_sentinel_identity_only_paragraph() {
        // 마커 없는 순수 텍스트 문단도 문단부호 좌표는 유효하다
        // (sentinel 은 문단부호 의미론 — 앵커 유무와 무관).
        let mut b = WireMapBuilder::new();
        b.advance_identity(10);
        let map = b.finish().expect("valid");
        assert_eq!(map.to_core(11), Ok(10));
        assert_eq!(map.to_core(12), Err(CacheMapError::OutOfRange { pos: 12, end: 10 }));
    }

    // ── tab (8,1) 비대칭 ────────────────────────────────────────

    #[test]
    fn tab_span_maps_core_contribution_uniquely() {
        // "ab" + tab(8,1) + "cd": wire [a=0,b=1,tab=2..10,c=10,d=11],
        // core [a=0,b=1,\t=2,c=3,d=4].
        let mut b = WireMapBuilder::new();
        b.advance_identity(2);
        b.push_span(8, 1);
        b.advance_identity(2);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 12);
        assert_eq!(map.core_end(), 5);
        assert_eq!(map.to_core(2), Ok(2)); // tab 시작 경계
        assert_eq!(map.to_core(10), Ok(3)); // tab 끝 경계
        assert_eq!(map.to_core(5), Err(CacheMapError::InsideContraction { wire: 5 }));
        // core 2 (tab 위치) 의 유일 preimage = tab wire 시작.
        assert_eq!(map.to_wire(2), Ok(2));
        assert_eq!(map.to_wire(3), Ok(10));
        assert_eq!(map.to_wire(4), Ok(11));
    }

    // ── 무음 소비 (1,0) ─────────────────────────────────────────

    #[test]
    fn single_unit_silent_consumption() {
        // "a" + 0x1E(1,0) + "b": wire [0,1..2,2], core [0,1].
        let mut b = WireMapBuilder::new();
        b.advance_identity(1);
        b.push_span(1, 0);
        b.advance_identity(1);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 3);
        assert_eq!(map.core_end(), 2);
        // (1,0) span 은 strict interior 가 없음 — 경계만 존재.
        assert_eq!(map.to_core(1), Ok(1));
        assert_eq!(map.to_core(2), Ok(1));
        // 축약점 {1,2} — earliest preimage (W3 실측 개정).
        assert_eq!(map.to_wire(1), Ok(1));
    }

    // ── 빌더 불변식 ─────────────────────────────────────────────

    #[test]
    fn identity_sized_span_is_absorbed() {
        let mut b = WireMapBuilder::new();
        b.push_span(1, 1); // lineBreak 등 — span 아님
        b.advance_identity(3);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 4);
        assert_eq!(map.core_end(), 4);
        assert_eq!(map.to_wire(2), Ok(2));
    }

    #[test]
    fn overflow_is_rejected_at_finish() {
        let mut b = WireMapBuilder::new();
        b.advance_identity(u32::MAX);
        b.push_span(8, 0);
        assert_eq!(b.finish(), Err(CacheMapError::InvalidSpans));
    }

    #[test]
    fn zero_wire_span_is_rejected() {
        let mut b = WireMapBuilder::new();
        b.push_span(0, 1);
        assert_eq!(b.finish(), Err(CacheMapError::InvalidSpans));
    }

    // ── property: 법칙 잠금 ─────────────────────────────────────

    proptest::proptest! {
        /// 임의 span 구성에서 1:1 구간 왕복 항등 + 경계 법칙.
        #[test]
        fn laws_hold_for_arbitrary_maps(
            segments in proptest::collection::vec(
                (0u32..3, 1u32..12, 0u32..2), 0..12,
            )
        ) {
            let mut b = WireMapBuilder::new();
            for (kind, len, core) in segments {
                match kind {
                    0 => b.advance_identity(len),
                    1 => b.push_span(8, core.min(1)),
                    _ => b.push_span(1, 0),
                }
            }
            let Ok(map) = b.finish() else { return Ok(()); };

            // 법칙 1: canonicalization.
            proptest::prop_assert_eq!(map.to_core(0), Ok(0));
            proptest::prop_assert_eq!(map.to_wire(0), Ok(0));

            // 법칙 2: 전 domain 에서 to_core 는 OutOfRange 를 내지 않고,
            // 성공 시 core domain 내 값.
            for w in 0..=map.wire_end() {
                match map.to_core(w) {
                    Ok(c) => {
                        proptest::prop_assert!(c <= map.core_end());
                        // 법칙 3: to_core 성공 + to_wire 성공이면 왕복 항등
                        // 또는 canonicalization(0)/경계 축약.
                        if let Ok(w2) = map.to_wire(c) {
                            let c2 = map.to_core(w2).expect("canonical wire valid");
                            proptest::prop_assert_eq!(c2, c);
                        }
                    }
                    Err(CacheMapError::InsideContraction { .. }) => {}
                    Err(e) => proptest::prop_assert!(false, "unexpected {e:?} at wire {w}"),
                }
            }

            // 법칙 4: to_wire 성공값은 wire domain 내 + 재변환 일치.
            for c in 0..=map.core_end() {
                if let Ok(w) = map.to_wire(c) {
                    proptest::prop_assert!(w <= map.wire_end());
                    proptest::prop_assert_eq!(map.to_core(w), Ok(c));
                }
            }
        }
    }
}
