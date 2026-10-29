//! Paragraph lengths retain their physical or character-count unit.
use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// An exact character count on the 1/500 ch grid, independent of font size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct CharCount(#[schemars(range(min = -2147483648i64, max = 2147483647))] i32);

impl CharCount {
    /// Creates a count from signed 1/500 ch ticks.
    pub const fn from_ticks(ticks: i32) -> Self {
        Self(ticks)
    }

    /// Creates an integral character count, checking the tick range.
    pub fn from_chars(chars: i32) -> CoreResult<Self> {
        Self::from_ratio(i64::from(chars), 1)
    }

    /// Creates an exact rational count; rejects zero denominators and off-grid values.
    pub fn from_ratio(numerator: i64, denominator: i64) -> CoreResult<Self> {
        if denominator == 0 {
            return Err(CoreError::MeasureOffGrid);
        }
        let scaled = i128::from(numerator) * 500;
        let denominator = i128::from(denominator);
        if scaled % denominator != 0 {
            return Err(CoreError::MeasureOffGrid);
        }
        i32::try_from(scaled / denominator).map(Self).map_err(|_| CoreError::MeasureOverflow)
    }

    /// Returns the signed 1/500 ch tick count.
    pub const fn ticks(self) -> i32 {
        self.0
    }

    /// Adds character counts without overflow.
    pub fn checked_add(self, rhs: Self) -> CoreResult<Self> {
        self.0.checked_add(rhs.0).map(Self).ok_or(CoreError::MeasureOverflow)
    }
    /// Subtracts character counts without overflow.
    pub fn checked_sub(self, rhs: Self) -> CoreResult<Self> {
        self.0.checked_sub(rhs.0).map(Self).ok_or(CoreError::MeasureOverflow)
    }
    /// Multiplies by an integral scalar without overflow.
    pub fn checked_mul(self, rhs: i32) -> CoreResult<Self> {
        self.0.checked_mul(rhs).map(Self).ok_or(CoreError::MeasureOverflow)
    }
}

/// A paragraph margin or tab position. Equality includes the unit, even at zero.
///
/// The exchange representation is `{ "unit": "hwpunit" | "char",
/// "value": integer }`. The exchange envelope supplies `format_version: 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "unit", content = "value", deny_unknown_fields)]
#[non_exhaustive]
pub enum ParagraphMeasure {
    /// Absolute length in 1/100 pt units.
    #[serde(rename = "hwpunit")]
    HwpUnit(HwpUnit),
    /// Relative character count in 1/500 ch ticks.
    #[serde(rename = "char")]
    CharCount(CharCount),
}

impl ParagraphMeasure {
    /// Creates an absolute measure.
    pub const fn from_hwp_unit(value: HwpUnit) -> Self {
        Self::HwpUnit(value)
    }
    /// Creates a character measure.
    pub const fn from_char_count(value: CharCount) -> Self {
        Self::CharCount(value)
    }
    /// Returns an absolute value only; never converts character counts.
    pub const fn as_hwp_unit(self) -> Option<HwpUnit> {
        match self {
            Self::HwpUnit(v) => Some(v),
            Self::CharCount(_) => None,
        }
    }
    /// Returns a character value only.
    pub const fn as_char_count(self) -> Option<CharCount> {
        match self {
            Self::CharCount(v) => Some(v),
            Self::HwpUnit(_) => None,
        }
    }
    /// Adds values of the same unit, checking their semantic range.
    pub fn checked_add(self, rhs: Self) -> CoreResult<Self> {
        match (self, rhs) {
            (Self::HwpUnit(a), Self::HwpUnit(b)) => {
                Self::physical(a.as_i32().checked_add(b.as_i32()))
            }
            (Self::CharCount(a), Self::CharCount(b)) => a.checked_add(b).map(Self::CharCount),
            _ => Err(CoreError::MeasureUnitMismatch),
        }
    }
    /// Subtracts values of the same unit, checking their semantic range.
    pub fn checked_sub(self, rhs: Self) -> CoreResult<Self> {
        match (self, rhs) {
            (Self::HwpUnit(a), Self::HwpUnit(b)) => {
                Self::physical(a.as_i32().checked_sub(b.as_i32()))
            }
            (Self::CharCount(a), Self::CharCount(b)) => a.checked_sub(b).map(Self::CharCount),
            _ => Err(CoreError::MeasureUnitMismatch),
        }
    }
    /// Multiplies by an integral scalar, retaining the unit.
    pub fn checked_mul(self, rhs: i32) -> CoreResult<Self> {
        match self {
            Self::HwpUnit(v) => Self::physical(v.as_i32().checked_mul(rhs)),
            Self::CharCount(v) => v.checked_mul(rhs).map(Self::CharCount),
        }
    }
    fn physical(value: Option<i32>) -> CoreResult<Self> {
        HwpUnit::new(value.ok_or(CoreError::MeasureOverflow)?)
            .map(Self::HwpUnit)
            .map_err(|_| CoreError::MeasureOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_grid_and_checked_arithmetic() {
        assert_eq!(CharCount::from_ratio(-3, 2).unwrap().ticks(), -750);
        assert!(matches!(CharCount::from_ratio(1, 3), Err(CoreError::MeasureOffGrid)));
        assert!(CharCount::from_ratio(i64::MAX, 1).is_err());
        assert!(CharCount::from_ticks(i32::MAX).checked_add(CharCount::from_ticks(1)).is_err());
        let zero = ParagraphMeasure::from_hwp_unit(HwpUnit::ZERO);
        let ch = ParagraphMeasure::from_char_count(CharCount::from_ticks(0));
        // 이것을 실패시키는 것: 변이 14 — 0 CHAR와 절대 0을 같게 비교.
        assert_ne!(zero, ch);
        assert!(matches!(zero.checked_add(ch), Err(CoreError::MeasureUnitMismatch)));
        assert_eq!(ch.as_hwp_unit(), None);
        assert_ne!(serde_json::to_value(zero).unwrap(), serde_json::to_value(ch).unwrap());
        assert!(ParagraphMeasure::from_hwp_unit(HwpUnit::new(100_000_000).unwrap())
            .checked_mul(2)
            .is_err());
    }
}
