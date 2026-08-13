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
//! ⚠️ smithy-hwpx 의 동명 모듈과 **to_core 방향 동형** — conformance
//! vector 테스트를 양쪽에 복제해 drift 를 방지한다 (Core 승격은 계층
//! 누수라 기각, §1g v2 disposition #2). **`to_wire` 는 이 crate 에
//! 없다**: HWP5 writer 가 없어 역변환 수요가 없음 (v4 disposition #10
//! YAGNI — 필요해지는 시점에 hwpx 모듈에서 conformance 와 함께 복제).

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

#[cfg(test)]
mod tests {
    use super::*;

    /// conformance vector — smithy-hwpx 의 동명 모듈 테스트와 to_core
    /// 방향에서 동일하게 유지할 것 (동형성 게이트).
    fn marker_ledger_p1() -> WireTextMap {
        // rules-marker-ledger p1: 텍스트 17유닛 + nwno(8,0)×2 + 텍스트.
        // raw lineseg [0, 76, 132] → core [0, 60, 116].
        let mut b = WireMapBuilder::new();
        b.advance_identity(17);
        b.push_span(8, 0);
        b.push_span(8, 0);
        b.advance_identity(148 - 33);
        b.finish().expect("valid map")
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
