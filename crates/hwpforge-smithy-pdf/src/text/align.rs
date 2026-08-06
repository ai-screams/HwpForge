//! R2 정렬 배분 — W0 실측 확정 규칙 (`pdf-cache-replay-rules.md` §4).
//!
//! | 정렬 | 규칙 (실측 근거) |
//! |---|---|
//! | LEFT | x = horzpos (자연폭 그대로) |
//! | RIGHT | 우변 밀착 — 표B Δ0.12pt |
//! | CENTER | 잉여 절반 — 표B ±1pt(베어링), 라운딩 미세보정은 게이트에서 역보정 |
//! | JUSTIFY | 잉여를 **공백에만 균등** (자간 배분 없음 — 공백 1개에 46.14pt 몰빵 실측), 마지막 줄 자연폭 |
//!
//! 산술 단위: **HWPUNIT (f64)** — lineseg 정수값에서 파생된 분수 폭.
//! paint 경계 전까지 pt 로 바꾸지 않는다.

use hwpforge_foundation::Alignment;

/// 한 줄의 배치 입력 (lineseg 에서 온 값 — 정렬 미반영 상태).
#[derive(Debug, Clone, Copy)]
pub struct LineBox {
    /// 줄 가로 시작 (lineseg `horzpos`, HWPUNIT).
    pub horzpos: i32,
    /// 줄 폭 (lineseg `horzsize`, HWPUNIT).
    pub horzsize: i32,
}

/// 셰이핑으로 구한 자연 상태.
#[derive(Debug, Clone, Copy)]
pub struct NaturalLine {
    /// 자연폭 (HWPUNIT — 공백 0.5em 반영).
    pub width: f64,
    /// 줄 안 공백 수 (JUSTIFY 배분 대상).
    pub space_count: usize,
}

/// 배분 결과.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePlacement {
    /// 줄 시작 x (HWPUNIT — 정렬 반영).
    pub origin_x: f64,
    /// 공백 하나당 추가 폭 (HWPUNIT — JUSTIFY 외에는 0).
    pub extra_per_space: f64,
    /// 배분이 근사임을 알리는 경고 필요 여부 (배분 정렬·공백 0 JUSTIFY).
    pub needs_warning: bool,
}

/// 정렬 배분을 계산한다.
///
/// - `is_last_line`: 문단의 마지막 lineseg 여부 — JUSTIFY 는 마지막 줄을
///   자연폭(LEFT)으로 둔다 (W0 실측: rules-justify 4째 줄 459.50 < 538.44).
/// - 자연폭이 `horzsize` 를 넘으면 늘이지 않는다 (extra 0, LEFT 기점) —
///   음수 배분은 발명이며 실측 근거가 없다.
/// - [`Alignment::Distribute`]/[`Alignment::DistributeSpace`] 는 W0 미실측 —
///   자연폭 + 경고 (위치는 정확, 스트레치만 포기 — no-fake-support).
pub fn place_line(
    alignment: Alignment,
    line: LineBox,
    natural: NaturalLine,
    is_last_line: bool,
) -> LinePlacement {
    let horzpos = f64::from(line.horzpos);
    let excess = f64::from(line.horzsize) - natural.width;

    match alignment {
        Alignment::Left => {
            LinePlacement { origin_x: horzpos, extra_per_space: 0.0, needs_warning: false }
        }
        Alignment::Right => LinePlacement {
            origin_x: horzpos + excess.max(0.0),
            extra_per_space: 0.0,
            needs_warning: false,
        },
        Alignment::Center => LinePlacement {
            origin_x: horzpos + (excess.max(0.0)) / 2.0,
            extra_per_space: 0.0,
            needs_warning: false,
        },
        Alignment::Justify => {
            if is_last_line || excess <= 0.0 {
                return LinePlacement {
                    origin_x: horzpos,
                    extra_per_space: 0.0,
                    needs_warning: false,
                };
            }
            if natural.space_count == 0 {
                // 공백 0 JUSTIFY 줄 — 미실측 (규칙 문서 §4). 자연폭 + 경고.
                return LinePlacement {
                    origin_x: horzpos,
                    extra_per_space: 0.0,
                    needs_warning: true,
                };
            }
            LinePlacement {
                origin_x: horzpos,
                extra_per_space: excess / natural.space_count as f64,
                needs_warning: false,
            }
        }
        // 배분 정렬 2종 — W0 미실측: 자연폭 + 경고 (위치 정확, 스트레치 포기).
        _ => LinePlacement { origin_x: horzpos, extra_per_space: 0.0, needs_warning: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // W0 실측 (rules-table 표B): 셀 내부폭 hsize=94.68pt=9468HU,
    // '가운데 정렬 텍스트' 자연폭 90pt=9000HU (8자×10pt + 공백2×5pt).
    const CELL: LineBox = LineBox { horzpos: 0, horzsize: 9468 };

    #[test]
    fn left_keeps_natural_origin() {
        let p =
            place_line(Alignment::Left, CELL, NaturalLine { width: 9000.0, space_count: 2 }, false);
        assert_eq!(p, LinePlacement { origin_x: 0.0, extra_per_space: 0.0, needs_warning: false });
    }

    #[test]
    fn right_is_flush_to_right_edge() {
        // W0: 오른쪽 = 콘텐츠 우변 밀착 (실측 Δ0.12pt = 베어링)
        let p = place_line(
            Alignment::Right,
            CELL,
            NaturalLine { width: 9000.0, space_count: 2 },
            false,
        );
        assert_eq!(p.origin_x, 468.0);
        assert_eq!(p.extra_per_space, 0.0);
    }

    #[test]
    fn center_takes_half_excess() {
        // W0: 가운데 = 잉여 절반 (9468−9000)/2 = 234HU = 2.34pt
        let p = place_line(
            Alignment::Center,
            CELL,
            NaturalLine { width: 9000.0, space_count: 2 },
            false,
        );
        assert_eq!(p.origin_x, 234.0);
    }

    #[test]
    fn justify_distributes_excess_equally_over_spaces_only() {
        // W0 (rules-justify): 본문 hsize 48188HU, 잉여를 공백 수로 균등 나눔.
        // '분배 규칙을' — 공백 1개에 몰빵 46.14pt-5pt=잉여 전량 실측.
        let p = place_line(
            Alignment::Justify,
            LineBox { horzpos: 0, horzsize: 9468 },
            NaturalLine { width: 5354.0, space_count: 1 },
            false,
        );
        assert_eq!(p.origin_x, 0.0);
        assert_eq!(p.extra_per_space, 9468.0 - 5354.0);
        assert!(!p.needs_warning);

        // 공백 3개 균등 (rules-justify 2째 줄 12.18pt×3 실측 패턴)
        let p3 = place_line(
            Alignment::Justify,
            LineBox { horzpos: 0, horzsize: 9468 },
            NaturalLine { width: 9000.0, space_count: 3 },
            false,
        );
        assert_eq!(p3.extra_per_space, 156.0);
    }

    #[test]
    fn justify_last_line_stays_natural() {
        // W0: 마지막 줄 = 자연폭 (rules-justify 4째 줄 끝 459.50 < 538.44)
        let p = place_line(
            Alignment::Justify,
            CELL,
            NaturalLine { width: 9000.0, space_count: 2 },
            true,
        );
        assert_eq!(p, LinePlacement { origin_x: 0.0, extra_per_space: 0.0, needs_warning: false });
    }

    #[test]
    fn justify_without_spaces_warns_and_stays_natural() {
        // 규칙 문서 §4 미실측 항목 — fail-open 아님(위치 정확), 스트레치만 포기.
        let p = place_line(
            Alignment::Justify,
            CELL,
            NaturalLine { width: 9000.0, space_count: 0 },
            false,
        );
        assert_eq!(p.origin_x, 0.0);
        assert_eq!(p.extra_per_space, 0.0);
        assert!(p.needs_warning);
    }

    #[test]
    fn overflowing_line_is_never_stretched_or_shifted_negative() {
        // 자연폭 > horzsize: 늘이거나 음수 기점으로 밀지 않는다 (발명 금지).
        for alignment in [Alignment::Right, Alignment::Center, Alignment::Justify] {
            let p = place_line(
                alignment,
                LineBox { horzpos: 100, horzsize: 1000 },
                NaturalLine { width: 1500.0, space_count: 2 },
                false,
            );
            assert_eq!(p.origin_x, 100.0, "{alignment:?}");
            assert_eq!(p.extra_per_space, 0.0, "{alignment:?}");
        }
    }

    #[test]
    fn distribute_alignments_warn_and_stay_natural() {
        // W0 미실측 — 자연폭 + 경고 (no-fake-support).
        let p = place_line(
            Alignment::Distribute,
            CELL,
            NaturalLine { width: 9000.0, space_count: 2 },
            false,
        );
        assert!(p.needs_warning);
        assert_eq!(p.origin_x, 0.0);
    }
}
