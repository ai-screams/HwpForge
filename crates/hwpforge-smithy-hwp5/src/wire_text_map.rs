//! Wire↔Core 텍스트 좌표 맵 (W1b) — 이 crate 내부 전용.
//!
//! wire 좌표(HWP5 raw 스트림 = 한컴 HWPX linesegarray textpos 규약)와
//! Core 정준 좌표(`Paragraph::text_content()` UTF-16 축) 사이의 변환을
//! 담당한다. 각 경계에서 누적된 wire/Core 길이 차이를 적용한다.
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
//! ⚠️ smithy-hwpx 의 동명 모듈과 **to_core 방향 동형** — conformance
//! vector 테스트를 양쪽에 복제해 drift 를 방지한다 (Core 승격은 계층
//! 누수라 기각). **`to_wire` 는 이 crate 에
//! 없다**: HWP5 writer 가 없어 역변환 수요가 없음 (v4 disposition #10
//! YAGNI — 필요해지는 시점에 hwpx 모듈에서 conformance 와 함께 복제).

use std::fmt;

#[cfg(test)]
thread_local! {
    /// 변환이 검사한 span 수 — 테스트가 시간 대신 일의 양으로 상한을 건다
    /// (스레드별).
    pub(crate) static SPAN_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// span 하나를 검사했다고 센다 (테스트 빌드에서만).
fn visit_span() {
    #[cfg(test)]
    SPAN_VISITS.with(|visits| visits.set(visits.get() + 1));
}

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
    /// 각 span 시작의 Core 위치(span 순서, 비감소) — 좌표 변환은 이것과
    /// `spans` 의 wire 시작을 이진 탐색한다(줄마다 span 을 앞에서부터
    /// 훑지 않는다).
    core_start: Vec<u32>,
}

impl WireTextMap {
    /// wire 축 총길이 (현재 테스트 전용 — lib 소비자가 생기면 게이트 해제).
    #[cfg(test)]
    pub(crate) fn wire_end(&self) -> u32 {
        self.wire_end
    }

    /// Core 축 총길이.
    pub(crate) fn core_end(&self) -> u32 {
        self.core_end
    }

    /// wire 좌표 → Core 좌표.
    pub(crate) fn to_core(&self, wire: u32) -> Result<u32, CacheMapError> {
        if wire > self.wire_end {
            return Err(CacheMapError::OutOfRange { pos: wire, end: self.wire_end });
        }
        // The last span that starts before `wire` decides: inside it is a
        // contraction; at or after its end the rest is one to one (a `wire`
        // at a span's start counts with the gap before it, as before).
        // 이것을 실패시키는 것: span 을 앞에서부터 훑기
        // (`conversions_after_many_spans_stay_logarithmic`).
        let before = self.spans.partition_point(|span| {
            visit_span();
            span.wire_start < wire
        });
        let Some(last) = before.checked_sub(1) else {
            return Ok(wire);
        };
        let span = &self.spans[last];
        let span_end = span.wire_start + span.wire_len;
        if wire < span_end {
            return Err(CacheMapError::InsideContraction { wire });
        }
        Ok(self.core_start[last] + span.core_len + (wire - span_end))
    }

    /// 오름차순 질의용 커서 — [`Self::to_core`] 와 같은 값을 내되, 앞 질의가
    /// 지나간 span 을 다시 훑지 않는다. 질의가 오름차순이면 질의 R 개·span S
    /// 개에 `O(R + S)`. 뒤로 가는 질의는 처음부터 다시 훑는다(정확성 유지).
    pub(crate) fn cursor(&self) -> WireToCoreCursor<'_> {
        WireToCoreCursor { map: self, next_span: 0, wire_cursor: 0, core_cursor: 0 }
    }
}

/// [`WireTextMap::cursor`] 가 만드는 오름차순 wire → Core 변환기.
#[derive(Debug)]
pub(crate) struct WireToCoreCursor<'a> {
    map: &'a WireTextMap,
    /// 아직 지나지 않은 첫 span.
    next_span: usize,
    /// `next_span` 앞까지 소비한 wire·Core 길이.
    wire_cursor: u32,
    core_cursor: u32,
}

impl WireToCoreCursor<'_> {
    /// wire 좌표 → Core 좌표 ([`WireTextMap::to_core`] 와 같은 결과).
    pub(crate) fn core_at(&mut self, wire: u32) -> Result<u32, CacheMapError> {
        if wire > self.map.wire_end {
            return Err(CacheMapError::OutOfRange { pos: wire, end: self.map.wire_end });
        }
        if wire < self.wire_cursor {
            self.next_span = 0;
            self.wire_cursor = 0;
            self.core_cursor = 0;
        }
        while let Some(span) = self.map.spans.get(self.next_span) {
            visit_span();
            if wire <= span.wire_start {
                return Ok(self.core_cursor + (wire - self.wire_cursor));
            }
            let span_end = span.wire_start + span.wire_len;
            if wire < span_end {
                return Err(CacheMapError::InsideContraction { wire });
            }
            self.core_cursor += (span.wire_start - self.wire_cursor) + span.core_len;
            self.wire_cursor = span_end;
            self.next_span += 1;
        }
        Ok(self.core_cursor + (wire - self.wire_cursor))
    }

    /// Like [`Self::core_at`], but a position inside a contraction gives the
    /// Core position at the contraction's end, with `true`. The cursor stays
    /// at that span, so a later query inside the same span or after it does
    /// not rescan from the start (a cursor moved past the span would reset
    /// on the next, smaller query).
    pub(crate) fn core_at_or_after(&mut self, wire: u32) -> Result<(u32, bool), CacheMapError> {
        match self.core_at(wire) {
            Ok(core) => Ok((core, false)),
            Err(CacheMapError::InsideContraction { .. }) => {
                // `core_at` stopped on the span holding `wire` without
                // moving past it.
                let span = &self.map.spans[self.next_span];
                Ok((self.core_cursor + (span.wire_start - self.wire_cursor) + span.core_len, true))
            }
            Err(e) => Err(e),
        }
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
        let mut core_start = Vec::with_capacity(self.spans.len());
        for span in &self.spans {
            if span.wire_start < expect_wire || span.wire_len == 0 || span.wire_len == span.core_len
            {
                return Err(CacheMapError::InvalidSpans);
            }
            let start = expect_core
                .checked_add(span.wire_start - expect_wire)
                .ok_or(CacheMapError::InvalidSpans)?;
            core_start.push(start);
            expect_core = start.checked_add(span.core_len).ok_or(CacheMapError::InvalidSpans)?;
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
            core_start,
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
    /// 종류.
    pub kind: hwpforge_core::layout::WireTokenKind,
}

/// 문단의 영-core 토큰 목록, wire 순서 (S22). 좌표 맵과 나란히 기록하고 맵은
/// 바꾸지 않는다.
///
/// 줄마다 하는 조회(같은 Core 위치의 토큰, 필드 안인지)는 색인으로 한다 —
/// 줄 수 × 토큰 수로 다시 훑지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TokenLedger {
    tokens: Vec<ZeroToken>,
    /// Core 위치별 토큰 번호, wire 순서.
    by_core: std::collections::BTreeMap<u32, Vec<usize>>,
    /// 아직 닫히지 않은 필드 시작 컨트롤의 wire 시작, 안쪽이 끝.
    open: Vec<u32>,
    /// 닫힌 필드 구간 `(시작 컨트롤의 시작, 끝 컨트롤의 끝)`, 닫힌 순서.
    fields: Vec<(u32, u32)>,
    /// `fields` 를 시작 순으로 정렬하고 끝을 앞에서부터의 최댓값으로 바꾼 것 —
    /// 첫 조회 때 만든다.
    field_index: std::cell::OnceCell<Vec<(u32, u32)>>,
    /// 조회가 살펴본 토큰·구간 수(선형성 시험용).
    #[cfg(test)]
    visits: std::cell::Cell<usize>,
}

impl TokenLedger {
    /// 토큰 하나를 더한다 (wire 순서대로).
    pub(crate) fn push(&mut self, token: ZeroToken) {
        use hwpforge_core::layout::WireTokenKind::{FieldBegin, FieldEnd};
        match token.kind {
            FieldBegin => self.open.push(token.wire_start),
            FieldEnd => {
                if let Some(start) = self.open.pop() {
                    // 필드 끝 컨트롤은 8 유닛.
                    self.fields.push((start, token.wire_start + 8));
                    self.field_index = std::cell::OnceCell::new();
                }
            }
            _ => {}
        }
        self.by_core.entry(token.core).or_default().push(self.tokens.len());
        self.tokens.push(token);
    }

    /// `wire` 가 필드 안(필드 시작 < `wire` < 필드 끝 컨트롤의 끝, 8 유닛)인지
    /// 판정한다. 그런 줄은 캐시를 버린다.
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

    /// wire 위치 `wire`(Core 위치 `core`)에서 시작하는 줄의 경계. 그 위치의
    /// 토큰 앞에서 시작하면(토큰이 없거나 첫 토큰 앞) `None` — 가장 이른
    /// 자리라 좌표만으로 정해진다. 몇 개를 지난 뒤면 지난 종류들과 다음 종류.
    pub(crate) fn boundary_at(
        &self,
        core: u32,
        wire: u32,
    ) -> Option<hwpforge_core::layout::LineBoundary> {
        // Borrow the position's index and binary search for the
        // tokens before `wire`; read only that prefix and the token after it.
        let here = self.by_core.get(&core).map_or(&[][..], Vec::as_slice);
        let k = here.partition_point(|&i| self.tokens[i].wire_start < wire);
        (k > 0).then(|| {
            #[cfg(test)]
            self.visits.set(self.visits.get() + k + 1);
            hwpforge_core::layout::LineBoundary::new(
                here[..k].iter().map(|&i| self.tokens[i].kind).collect(),
                here.get(k).map(|&i| self.tokens[i].kind),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk from the front `to_core` used before, as the oracle.
    fn walked_to_core(map: &WireTextMap, wire: u32) -> Result<u32, CacheMapError> {
        if wire > map.wire_end {
            return Err(CacheMapError::OutOfRange { pos: wire, end: map.wire_end });
        }
        let (mut wire_cursor, mut core_cursor) = (0u32, 0u32);
        for span in &map.spans {
            if wire <= span.wire_start {
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

    /// The binary searches give the walk's answer at every position,
    /// over maps that mix zero-width tokens, tabs, runs of tokens, and text.
    /// 이것을 실패시키는 것: 탐색 경계를 한 칸 어긋나게(`<` 를 `<=` 로) 하는 것.
    #[test]
    fn binary_search_matches_the_walk_everywhere() {
        let maps: Vec<WireTextMap> = vec![
            WireMapBuilder::new().finish().unwrap(),
            {
                let mut b = WireMapBuilder::new();
                b.push_span(8, 0);
                b.push_span(8, 0);
                b.advance_identity(3);
                b.push_span(8, 1);
                b.advance_identity(2);
                b.push_span(8, 0);
                b.push_span(1, 0);
                b.advance_identity(4);
                b.push_span(8, 0);
                b.finish().unwrap()
            },
            {
                let mut b = WireMapBuilder::new();
                b.advance_identity(5);
                b.push_span(8, 1);
                b.push_span(8, 1);
                b.push_span(8, 0);
                b.finish().unwrap()
            },
        ];
        for map in maps {
            for wire in 0..=map.wire_end + 2 {
                assert_eq!(map.to_core(wire), walked_to_core(&map, wire), "wire {wire}");
            }
        }
    }

    /// 16,000 zero-width tokens, one character, then 16,000 lines
    /// after it — each conversion searches the spans, it does not walk them.
    /// 이것을 실패시키는 것: 좌표 변환을 앞에서부터 span 을 훑는 것으로
    /// 되돌리기(검사한 span 수가 N² 로 는다).
    #[test]
    fn conversions_after_many_spans_stay_logarithmic() {
        let n = 16_000u32;
        let mut b = WireMapBuilder::new();
        for _ in 0..n {
            b.push_span(8, 0);
        }
        b.advance_identity(1);
        let map = b.finish().unwrap();
        SPAN_VISITS.with(|visits| visits.set(0));
        for _ in 0..n {
            assert_eq!(map.to_core(8 * n + 1), Ok(1));
        }
        let visits = SPAN_VISITS.with(std::cell::Cell::get);
        assert!(visits <= 40 * u64::from(n), "{visits} spans checked for {n} lines");
    }

    /// The per-line lookups use the ledger's index — N lines over
    /// a paragraph of 3N tokens look at O(N) tokens and spans in all, not N².
    /// 이것을 실패시키는 것: `at` 를 전체 토큰 필터로, 또는 `inside_field` 를
    /// 질의마다 전체 토큰 순회로 되돌리는 것(살펴본 수가 N² 로 는다).
    /// N tokens and N lines at one Core position (bookmarks with no
    /// text, every line at 0) — each lookup reads only the prefix it needs,
    /// not the whole position.
    /// 이것을 실패시키는 것: 조회마다 그 위치의 토큰 전체를 복제·순회하는 것
    /// (살펴본 수 N²).
    #[test]
    fn lookups_at_one_crowded_position_stay_linear() {
        use hwpforge_core::layout::WireTokenKind::Bookmark;
        let n = 4096u32;
        let mut ledger = TokenLedger::default();
        for i in 0..n {
            ledger.push(ZeroToken { core: 0, wire_start: 8 * i, kind: Bookmark });
        }
        for _ in 0..n {
            assert_eq!(ledger.boundary_at(0, 0), None);
            assert_eq!(ledger.boundary_at(0, 8).map(|b| b.skip.len()), Some(1));
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
            ledger.push(ZeroToken { core: i, wire_start: w, kind: FieldBegin });
            ledger.push(ZeroToken { core: i, wire_start: w + 8, kind: FieldEnd });
            ledger.push(ZeroToken { core: i, wire_start: w + 16, kind: Bookmark });
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

    /// conformance vector — smithy-hwpx 의 동명 모듈 테스트와 to_core
    /// 방향에서 동일하게 유지할 것 (동형성 게이트).
    fn marker_ledger_p1() -> WireTextMap {
        // 한컴 새 번호 문단 실측: 텍스트 17유닛 + nwno(8,0)×2 + 텍스트.
        // raw lineseg [0, 76, 132] → core [0, 60, 116].
        let mut b = WireMapBuilder::new();
        b.advance_identity(17);
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(148 - 33);
        b.finish().expect("valid map")
    }

    /// The cursor gives `to_core`'s answer at every wire position, walked
    /// forward and then backward (a backward query starts over).
    ///
    /// 이것을 실패시키는 것: 뒤로 가는 질의에서 커서를 되감지 않는 것, span 을
    /// 지날 때 Core 누적을 빼먹는 것.
    #[test]
    fn cursor_matches_to_core_forward_and_backward() {
        let mut b = WireMapBuilder::new();
        b.advance_identity(3);
        b.push_span(8, 1);
        b.advance_identity(2);
        b.push_span(8, 0);
        b.push_span(1, 0);
        b.advance_identity(4);
        let map = b.finish().expect("valid map");
        let all: Vec<u32> = (0..=map.wire_end + 1).collect();
        let mut cursor = map.cursor();
        for &wire in all.iter().chain(all.iter().rev()) {
            assert_eq!(cursor.core_at(wire), map.to_core(wire), "wire {wire}");
        }
    }

    #[test]
    fn empty_paragraph_succeeds_only_at_zero() {
        let map = WireMapBuilder::new().finish().expect("empty map");
        assert_eq!(map.wire_end(), 0);
        assert_eq!(map.core_end(), 0);
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(1), Err(CacheMapError::OutOfRange { pos: 1, end: 0 }));
    }

    #[test]
    fn control_only_paragraph_collapses_to_core_zero() {
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 8);
        assert_eq!(map.core_end(), 0);
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(8), Ok(0));
        assert_eq!(map.to_core(3), Err(CacheMapError::InsideContraction { wire: 3 }));
    }

    #[test]
    fn leading_marker_prelude_normalizes_to_core_zero() {
        // HWP5 첫 문단 프렐류드 (secd+cold = 16유닛) 후 텍스트 4유닛.
        let mut b = WireMapBuilder::new();
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(4);
        let map = b.finish().expect("valid");
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(8), Ok(0));
        assert_eq!(map.to_core(16), Ok(0));
        assert_eq!(map.to_core(20), Ok(4));
    }

    #[test]
    fn golden_marker_ledger_p1_normalization() {
        let map = marker_ledger_p1();
        assert_eq!(map.to_core(0), Ok(0));
        assert_eq!(map.to_core(76), Ok(60));
        assert_eq!(map.to_core(132), Ok(116));
    }

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
        assert_eq!(map.to_core(17), Ok(17));
        assert_eq!(map.to_core(25), Ok(17));
        assert_eq!(map.to_core(33), Ok(17));
    }

    #[test]
    fn tab_span_maps_core_contribution() {
        // "ab" + tab(8,1) + "cd".
        let mut b = WireMapBuilder::new();
        b.advance_identity(2);
        b.push_span(8, 1);
        b.advance_identity(2);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 12);
        assert_eq!(map.core_end(), 5);
        assert_eq!(map.to_core(2), Ok(2));
        assert_eq!(map.to_core(10), Ok(3));
        assert_eq!(map.to_core(12), Ok(5));
        assert_eq!(map.to_core(5), Err(CacheMapError::InsideContraction { wire: 5 }));
    }

    #[test]
    fn single_unit_silent_consumption() {
        // "a" + 0x1E(1,0) + "b".
        let mut b = WireMapBuilder::new();
        b.advance_identity(1);
        b.push_span(1, 0);
        b.advance_identity(1);
        let map = b.finish().expect("valid");
        assert_eq!(map.to_core(1), Ok(1));
        assert_eq!(map.to_core(2), Ok(1));
        assert_eq!(map.to_core(3), Ok(2));
    }

    #[test]
    fn identity_sized_span_is_absorbed() {
        let mut b = WireMapBuilder::new();
        b.push_span(1, 1);
        b.advance_identity(3);
        let map = b.finish().expect("valid");
        assert_eq!(map.wire_end(), 4);
        assert_eq!(map.core_end(), 4);
        assert_eq!(map.to_core(2), Ok(2));
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
}
