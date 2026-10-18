//! Packed paragraph values at the HWPX/HWP5 boundary.
use crate::{HwpxError, HwpxResult};
use hwpforge_core::{CharCount, ParagraphMeasure};
use hwpforge_foundation::{HwpUnit, LineSpacingType};

pub(crate) const MEASURE_NAMESPACE: &str = "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar";

pub(crate) fn invalid(detail: impl Into<String>) -> HwpxError {
    HwpxError::InvalidStructure { detail: format!("Contents/header.xml: {}", detail.into()) }
}

fn unpack(raw: i64) -> HwpxResult<ParagraphMeasure> {
    // 이것을 실패시키는 것: 변이 1 — signed >>1을 /2로 교체.
    let value = i32::try_from(raw >> 1).map_err(|_| invalid("packed measure overflow"))?;
    // 이것을 실패시키는 것: 변이 2·3 — CHAR 비트 제거 또는 HWPUNIT 강제.
    if raw & 1 != 0 {
        Ok(ParagraphMeasure::from_char_count(CharCount::from_ticks(value)))
    } else {
        Ok(ParagraphMeasure::from_hwp_unit(HwpUnit::new(value)?))
    }
}

/// Decodes a signed packed margin, retaining the unit bit (including negative CHAR).
pub fn unpack_margin(raw: i32) -> HwpxResult<ParagraphMeasure> {
    unpack(i64::from(raw))
}
/// Decodes HWP5 tab bits: signed HWPUNIT or unsigned CHAR, retaining the unit bit.
pub fn unpack_tab(raw: u32) -> HwpxResult<ParagraphMeasure> {
    // Hancom's FSC twin and MOE/MSS counterparts retain negative HWPUNIT positions.
    unpack(if raw & 1 == 0 { i64::from(raw as i32) } else { i64::from(raw) })
}

pub(crate) fn unpack_xml_tab(raw: i64) -> HwpxResult<ParagraphMeasure> {
    if raw < 0 {
        let signed = i32::try_from(raw).map_err(|_| invalid("signed tab packing overflow"))?;
        if signed & 1 != 0 {
            return Err(invalid("negative CHAR tab position"));
        }
        unpack_margin(signed)
    } else {
        unpack_tab(u32::try_from(raw).map_err(|_| invalid("unsigned tab packing overflow"))?)
    }
}

pub(crate) fn parts(value: ParagraphMeasure) -> (i32, &'static str) {
    match value {
        ParagraphMeasure::HwpUnit(v) => (v.as_i32(), "HWPUNIT"),
        ParagraphMeasure::CharCount(v) => (v.ticks(), "CHAR"),
        _ => unreachable!("unsupported paragraph measure"),
    }
}
fn pack(value: ParagraphMeasure) -> i64 {
    let (value, unit) = parts(value);
    i64::from(value) * 2 + i64::from(unit == "CHAR")
}
pub(crate) fn pack_margin(value: ParagraphMeasure) -> HwpxResult<i32> {
    i32::try_from(pack(value)).map_err(|_| invalid("signed margin packing overflow"))
}
pub(crate) fn pack_tab(value: ParagraphMeasure) -> HwpxResult<i64> {
    let raw = pack(value);
    match value {
        ParagraphMeasure::HwpUnit(_) => {
            i32::try_from(raw).map_err(|_| invalid("signed tab packing overflow"))?;
        }
        ParagraphMeasure::CharCount(_) => {
            u32::try_from(raw)
                .map_err(|_| invalid("negative CHAR tab position or unsigned packing overflow"))?;
        }
        _ => unreachable!("unsupported paragraph measure"),
    }
    Ok(raw)
}
pub(crate) fn case_measure(value: i32, unit: &str) -> HwpxResult<ParagraphMeasure> {
    match unit {
        "HWPUNIT" => Ok(ParagraphMeasure::from_hwp_unit(HwpUnit::new(value)?)),
        "CHAR" => Ok(ParagraphMeasure::from_char_count(CharCount::from_ticks(value))),
        _ => Err(invalid(format!("case measure missing/unknown unit {unit:?}"))),
    }
}
pub(crate) fn decode_line_spacing(
    value: u32,
    kind: &str,
    unit: &str,
    case: bool,
) -> HwpxResult<(i32, LineSpacingType)> {
    let kind = match kind {
        "PERCENT" => LineSpacingType::Percentage,
        "FIXED" => LineSpacingType::Fixed,
        "AT_LEAST" => LineSpacingType::AtLeast,
        "BETWEEN_LINES" => LineSpacingType::BetweenLines,
        _ => return Err(invalid(format!("lineSpacing unknown type {kind:?}"))),
    };
    if unit != "HWPUNIT" {
        return Err(invalid(format!("lineSpacing unknown unit {unit:?}")));
    }
    let value = i32::try_from(value).map_err(|_| invalid("lineSpacing overflow"))?;
    let value = if case && kind != LineSpacingType::Percentage {
        value.checked_mul(2).ok_or_else(|| invalid("lineSpacing case ×2 overflow"))?
    } else {
        value
    };
    Ok((value, kind))
}
pub(crate) fn encode_line_spacing(value: i32, kind: LineSpacingType) -> HwpxResult<(u32, u32)> {
    let raw = u32::try_from(value).map_err(|_| invalid("negative lineSpacing"))?;
    if kind == LineSpacingType::Percentage {
        return Ok((raw, raw));
    }
    if raw % 2 != 0 {
        return Err(invalid(format!(
            "lineSpacing odd physical raw {raw} cannot be written exactly as switch"
        )));
    }
    Ok((raw / 2, raw))
}

pub(crate) fn validate_measure_output(store: &crate::HwpxStyleStore) -> HwpxResult<()> {
    for (index, p) in store.iter_para_shapes().enumerate() {
        for (field, m) in [
            ("left", p.margin_left),
            ("right", p.margin_right),
            ("intent", p.indent),
            ("prev", p.spacing_before),
            ("next", p.spacing_after),
        ] {
            pack_margin(m).map_err(|e| {
                invalid(format!("paraPr {:?} (index {index}).{field}: {e}", p.source_id))
            })?;
        }
        encode_line_spacing(p.line_spacing, p.line_spacing_type).map_err(|e| {
            invalid(format!("paraPr {:?} (index {index}).lineSpacing: {e}", p.source_id))
        })?;
    }
    for tab in store.iter_tabs() {
        for (i, stop) in tab.stops.iter().enumerate() {
            pack_tab(stop.position)
                .map_err(|e| invalid(format!("tabPr {}.position[{i}]: {e}", tab.id)))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packing_boundaries_and_units() {
        assert_eq!(unpack_margin(-1499).unwrap().as_char_count().unwrap().ticks(), -750);
        for raw in [i32::MIN, -200_000_000, -1499, -2, -1, 0, 1, 200_000_000, i32::MAX] {
            if let Ok(m) = unpack_margin(raw) {
                assert_eq!(pack_margin(m).unwrap(), raw);
            }
        }
        for raw in [0, 1, 2001, 30000, u32::MAX] {
            assert_eq!(pack_tab(unpack_tab(raw).unwrap()).unwrap() as u32, raw);
        }
        // 이것을 실패시키는 것: 변이 12 — 범위 오류를 0/clamp 성공으로 치환.
        assert!(unpack_margin(200_000_002).is_err());
        assert!(unpack_tab(200_000_002).is_err());
        assert!(pack_margin(ParagraphMeasure::from_char_count(CharCount::from_ticks(i32::MAX)))
            .is_err());
        assert!(pack_tab(unpack_margin(-1).unwrap()).is_err());
        assert!(encode_line_spacing(2001, LineSpacingType::Fixed).is_err());
        assert!(decode_line_spacing(i32::MAX as u32, "FIXED", "HWPUNIT", true).is_err());
        assert_eq!(decode_line_spacing(1000, "FIXED", "HWPUNIT", true).unwrap().0, 2000);
    }
    #[test]
    fn fix1_tab_signed_bits_and_char_output_boundaries() {
        // 이것을 실패시키는 것: HWPUNIT unsigned 해석 또는 음수 CHAR 출력 허용.
        for (raw, expected) in [(4294707008, -130144), (4294706880, -130208), (u32::MAX - 1, -1)] {
            let value = unpack_tab(raw).unwrap();
            assert_eq!(value.as_hwp_unit().unwrap().as_i32(), expected);
            assert_eq!(pack_tab(value).unwrap(), i64::from(expected) * 2);
        }
        assert!(unpack_tab((-200_000_002i32) as u32).is_err());
        assert_eq!(unpack_tab(u32::MAX).unwrap().as_char_count().unwrap().ticks(), i32::MAX);
        assert_eq!(
            pack_tab(ParagraphMeasure::from_char_count(CharCount::from_ticks(i32::MAX))).unwrap(),
            i64::from(u32::MAX)
        );
        assert!(pack_tab(ParagraphMeasure::from_char_count(CharCount::from_ticks(-1))).is_err());
    }
    proptest::proptest! {
        #[test]
        fn signed_raw_roundtrip(raw in proptest::prelude::any::<i32>()) {
            if let Ok(value) = unpack_margin(raw) { proptest::prop_assert_eq!(pack_margin(value).unwrap(),raw); }
        }
        #[test]
        fn unsigned_raw_roundtrip(raw in proptest::prelude::any::<u32>()) {
            if let Ok(value) = unpack_tab(raw) { proptest::prop_assert_eq!(pack_tab(value).unwrap() as u32,raw); }
        }
    }
}
