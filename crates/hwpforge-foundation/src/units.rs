//! HWP measurement unit system.
//!
//! HWP documents measure everything in **HwpUnit**, where 1 point = 100 HwpUnit.
//! This module provides [`HwpUnit`] and geometry composites built from it:
//! [`Size`], [`Point`], [`Rect`], and [`Insets`].
//!
//! # Conversion Constants
//!
//! | Unit | HwpUnit equivalent |
//! |------|--------------------|
//! | 1 pt | 100 |
//! | 1 inch | 7200 |
//! | 1 mm | 283.465... (f64 math) |
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::HwpUnit;
//!
//! let twelve_pt = HwpUnit::from_pt(12.0).unwrap();
//! assert_eq!(twelve_pt.as_i32(), 1200);
//! assert!((twelve_pt.to_pt() - 12.0).abs() < f64::EPSILON);
//! ```

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

use crate::error::{FoundationError, FoundationResult};

// ---------------------------------------------------------------------------
// HwpUnit
// ---------------------------------------------------------------------------

/// The universal measurement unit used throughout HWP documents.
///
/// Internally an `i32` where **1 point = 100 HwpUnit**.
/// `repr(transparent)` guarantees zero overhead over a bare `i32`.
///
/// FROZEN: Do not change the internal representation after v1.0.
///
/// # Valid Range
///
/// `[-100_000_000, 100_000_000]` -- comfortably covers A0 paper
/// (841 mm width ~ 2_384_252 HwpUnit).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::HwpUnit;
///
/// let one_inch = HwpUnit::from_inch(1.0).unwrap();
/// assert_eq!(one_inch.as_i32(), 7200);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct HwpUnit(i32);

// Compile-time size guarantee
const _: () = assert!(std::mem::size_of::<HwpUnit>() == 4);

/// Conversion: 1 point = 100 HwpUnit.
const HWPUNIT_PER_PT: f64 = 100.0;

/// Conversion: 1 inch = 72 pt = 7200 HwpUnit.
const HWPUNIT_PER_INCH: f64 = 7200.0;

/// Conversion: 1 mm = 72/25.4 pt = 283.4645... HwpUnit.
const HWPUNIT_PER_MM: f64 = 7200.0 / 25.4;

impl HwpUnit {
    /// Minimum valid value (inclusive).
    pub const MIN_VALUE: i32 = -100_000_000;
    /// Maximum valid value (inclusive).
    pub const MAX_VALUE: i32 = 100_000_000;

    /// Zero HwpUnit.
    pub const ZERO: Self = Self(0);
    /// One typographic point (100 HwpUnit).
    pub const ONE_PT: Self = Self(100);

    /// Creates an `HwpUnit` from a raw `i32`, validating the range.
    ///
    /// # Errors
    ///
    /// Returns [`FoundationError::InvalidHwpUnit`] when `value` lies
    /// outside `[MIN_VALUE, MAX_VALUE]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// assert!(HwpUnit::new(0).is_ok());
    /// assert!(HwpUnit::new(200_000_000).is_err());
    /// ```
    pub fn new(value: i32) -> FoundationResult<Self> {
        if !(Self::MIN_VALUE..=Self::MAX_VALUE).contains(&value) {
            return Err(FoundationError::InvalidHwpUnit {
                value: value as i64, // i64 for error reporting (no truncation)
                min: Self::MIN_VALUE,
                max: Self::MAX_VALUE,
            });
        }
        Ok(Self(value))
    }

    /// Creates an `HwpUnit` from a raw `i32` without validation.
    ///
    /// Intended for internal use where the value is already known-valid
    /// (e.g. constants, deserialized-then-checked data).
    pub(crate) const fn new_unchecked(value: i32) -> Self {
        Self(value)
    }

    /// Returns the raw `i32` value.
    pub const fn as_i32(self) -> i32 {
        self.0
    }

    /// Returns `true` if this unit is zero.
    ///
    /// Useful as a `skip_serializing_if` predicate for serde.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Constructs an `HwpUnit` from typographic points.
    ///
    /// # Errors
    ///
    /// Returns an error when `pt` is non-finite or the converted
    /// value exceeds the valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let u = HwpUnit::from_pt(12.0).unwrap();
    /// assert_eq!(u.as_i32(), 1200);
    /// ```
    pub fn from_pt(pt: f64) -> FoundationResult<Self> {
        Self::from_f64(pt, HWPUNIT_PER_PT, "pt")
    }

    /// Constructs an `HwpUnit` from millimeters.
    ///
    /// # Errors
    ///
    /// Returns an error when `mm` is non-finite or the converted
    /// value exceeds the valid range.
    pub fn from_mm(mm: f64) -> FoundationResult<Self> {
        Self::from_f64(mm, HWPUNIT_PER_MM, "mm")
    }

    /// Constructs an `HwpUnit` from inches.
    ///
    /// # Errors
    ///
    /// Returns an error when `inch` is non-finite or the converted
    /// value exceeds the valid range.
    pub fn from_inch(inch: f64) -> FoundationResult<Self> {
        Self::from_f64(inch, HWPUNIT_PER_INCH, "inch")
    }

    /// Converts to typographic points (f64).
    pub fn to_pt(self) -> f64 {
        self.0 as f64 / HWPUNIT_PER_PT
    }

    /// Converts to millimeters (f64).
    pub fn to_mm(self) -> f64 {
        self.0 as f64 / HWPUNIT_PER_MM
    }

    /// Converts to inches (f64).
    pub fn to_inch(self) -> f64 {
        self.0 as f64 / HWPUNIT_PER_INCH
    }

    // Internal: shared f64 -> HwpUnit conversion with validation.
    fn from_f64(value: f64, scale: f64, unit_name: &str) -> FoundationResult<Self> {
        if !value.is_finite() {
            return Err(FoundationError::InvalidField {
                field: unit_name.to_string(),
                reason: format!("{value} is not finite"),
            });
        }
        let raw = (value * scale).round() as i64;
        if raw < Self::MIN_VALUE as i64 || raw > Self::MAX_VALUE as i64 {
            return Err(FoundationError::InvalidHwpUnit {
                value: raw, // i64 그대로 (truncation 방지)
                min: Self::MIN_VALUE,
                max: Self::MAX_VALUE,
            });
        }
        Ok(Self(raw as i32))
    }
}

impl Default for HwpUnit {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Debug for HwpUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HwpUnit({})", self.0)
    }
}

impl fmt::Display for HwpUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} hwp", self.0)
    }
}

impl Add for HwpUnit {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sub for HwpUnit {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl Neg for HwpUnit {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }
}

impl Mul<i32> for HwpUnit {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        Self(self.0.saturating_mul(rhs))
    }
}

impl Div<i32> for HwpUnit {
    type Output = Self;
    fn div(self, rhs: i32) -> Self {
        Self(self.0 / rhs)
    }
}

// ---------------------------------------------------------------------------
// Geometry composites
// ---------------------------------------------------------------------------

/// A 2-dimensional size (width x height) in [`HwpUnit`].
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{HwpUnit, Size};
///
/// let a4 = Size::A4;
/// assert!(a4.width.as_i32() > 0);
/// assert!(a4.height.as_i32() > 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Size {
    /// Horizontal extent.
    pub width: HwpUnit,
    /// Vertical extent.
    pub height: HwpUnit,
}

const _: () = assert!(std::mem::size_of::<Size>() == 8);

impl Size {
    /// A4 paper: 210 mm x 297 mm.
    pub const A4: Self = Self {
        width: HwpUnit::new_unchecked(59528),  // round(210 * 7200/25.4)
        height: HwpUnit::new_unchecked(84188), // round(297 * 7200/25.4)
    };

    /// US Letter: 8.5 in x 11 in.
    pub const LETTER: Self = Self {
        width: HwpUnit::new_unchecked(61200),  // 8.5 * 7200
        height: HwpUnit::new_unchecked(79200), // 11 * 7200
    };

    /// B5 (JIS): 182 mm x 257 mm.
    pub const B5: Self = Self {
        width: HwpUnit::new_unchecked(51591),  // round(182 * 7200/25.4)
        height: HwpUnit::new_unchecked(72850), // round(257 * 7200/25.4)
    };

    /// Constructs a new [`Size`].
    pub const fn new(width: HwpUnit, height: HwpUnit) -> Self {
        Self { width, height }
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {}", self.width, self.height)
    }
}

/// A 2-dimensional point (x, y) in [`HwpUnit`].
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{HwpUnit, Point};
///
/// let origin = Point::ORIGIN;
/// assert_eq!(origin.x, HwpUnit::ZERO);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: HwpUnit,
    /// Vertical coordinate.
    pub y: HwpUnit,
}

const _: () = assert!(std::mem::size_of::<Point>() == 8);

impl Point {
    /// The origin (0, 0).
    pub const ORIGIN: Self = Self { x: HwpUnit::ZERO, y: HwpUnit::ZERO };

    /// Constructs a new [`Point`].
    pub const fn new(x: HwpUnit, y: HwpUnit) -> Self {
        Self { x, y }
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A rectangle defined by an origin [`Point`] and a [`Size`].
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{Point, Size, Rect};
///
/// let r = Rect::new(Point::ORIGIN, Size::A4);
/// assert_eq!(r.origin, Point::ORIGIN);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    /// Top-left corner.
    pub origin: Point,
    /// Width and height.
    pub size: Size,
}

const _: () = assert!(std::mem::size_of::<Rect>() == 16);

impl Rect {
    /// Constructs a new [`Rect`].
    pub const fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @ {}", self.size, self.origin)
    }
}

/// Edge insets (margins/padding) in [`HwpUnit`].
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{HwpUnit, Insets};
///
/// let uniform = Insets::uniform(HwpUnit::ONE_PT);
/// assert_eq!(uniform.top, uniform.bottom);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Insets {
    /// Top inset.
    pub top: HwpUnit,
    /// Bottom inset.
    pub bottom: HwpUnit,
    /// Left inset.
    pub left: HwpUnit,
    /// Right inset.
    pub right: HwpUnit,
}

const _: () = assert!(std::mem::size_of::<Insets>() == 16);

impl Insets {
    /// Creates insets with the same value on all four sides.
    pub const fn uniform(value: HwpUnit) -> Self {
        Self { top: value, bottom: value, left: value, right: value }
    }

    /// Creates insets with separate horizontal and vertical values.
    pub const fn symmetric(horizontal: HwpUnit, vertical: HwpUnit) -> Self {
        Self { top: vertical, bottom: vertical, left: horizontal, right: horizontal }
    }

    /// Creates insets with individual side values.
    pub const fn new(top: HwpUnit, bottom: HwpUnit, left: HwpUnit, right: HwpUnit) -> Self {
        Self { top, bottom, left, right }
    }
}

impl fmt::Display for Insets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Insets(top={}, bottom={}, left={}, right={})",
            self.top, self.bottom, self.left, self.right
        )
    }
}

// ---------------------------------------------------------------------------
// schemars impls (manual for transparent newtypes)
// ---------------------------------------------------------------------------

impl schemars::JsonSchema for HwpUnit {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "HwpUnit".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<i32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // HwpUnit edge cases (10+)
    // ===================================================================

    // Edge Case 1: Zero
    #[test]
    fn hwpunit_zero() {
        let u = HwpUnit::new(0).unwrap();
        assert_eq!(u.as_i32(), 0);
        assert_eq!(u, HwpUnit::ZERO);
    }

    // Edge Case 2: Minimum valid boundary
    #[test]
    fn hwpunit_min_valid() {
        let u = HwpUnit::new(HwpUnit::MIN_VALUE).unwrap();
        assert_eq!(u.as_i32(), -100_000_000);
    }

    // Edge Case 3: Maximum valid boundary
    #[test]
    fn hwpunit_max_valid() {
        let u = HwpUnit::new(HwpUnit::MAX_VALUE).unwrap();
        assert_eq!(u.as_i32(), 100_000_000);
    }

    // Edge Case 4: Below minimum -> error
    #[test]
    fn hwpunit_below_min_is_error() {
        assert!(HwpUnit::new(HwpUnit::MIN_VALUE - 1).is_err());
        assert!(HwpUnit::new(i32::MIN).is_err());
    }

    // Edge Case 5: Above maximum -> error
    #[test]
    fn hwpunit_above_max_is_error() {
        assert!(HwpUnit::new(HwpUnit::MAX_VALUE + 1).is_err());
        assert!(HwpUnit::new(i32::MAX).is_err());
    }

    // Edge Case 6: Infinity -> error
    #[test]
    fn hwpunit_from_pt_infinity_is_error() {
        assert!(HwpUnit::from_pt(f64::INFINITY).is_err());
        assert!(HwpUnit::from_pt(f64::NEG_INFINITY).is_err());
    }

    // Edge Case 7: NaN -> error
    #[test]
    fn hwpunit_from_pt_nan_is_error() {
        assert!(HwpUnit::from_pt(f64::NAN).is_err());
    }

    // Edge Case 8: Negative zero -> produces 0
    #[test]
    fn hwpunit_from_pt_negative_zero() {
        let u = HwpUnit::from_pt(-0.0).unwrap();
        assert_eq!(u.as_i32(), 0);
    }

    // Edge Case 9: Roundtrip pt
    #[test]
    fn hwpunit_roundtrip_pt() {
        let u = HwpUnit::from_pt(12.5).unwrap();
        assert!((u.to_pt() - 12.5).abs() < 0.01);
    }

    // Edge Case 10: Roundtrip mm
    #[test]
    fn hwpunit_roundtrip_mm() {
        let u = HwpUnit::from_mm(25.4).unwrap();
        // 25.4 mm = 1 inch = 72 pt = 7200 hwp
        assert_eq!(u.as_i32(), 7200);
        assert!((u.to_mm() - 25.4).abs() < 0.01);
    }

    // Edge Case 11: Roundtrip inch
    #[test]
    fn hwpunit_roundtrip_inch() {
        let u = HwpUnit::from_inch(1.0).unwrap();
        assert_eq!(u.as_i32(), 7200);
        assert!((u.to_inch() - 1.0).abs() < f64::EPSILON);
    }

    // Arithmetic tests

    #[test]
    fn hwpunit_add() {
        let a = HwpUnit::new(100).unwrap();
        let b = HwpUnit::new(200).unwrap();
        assert_eq!((a + b).as_i32(), 300);
    }

    #[test]
    fn hwpunit_sub() {
        let a = HwpUnit::new(300).unwrap();
        let b = HwpUnit::new(100).unwrap();
        assert_eq!((a - b).as_i32(), 200);
    }

    #[test]
    fn hwpunit_neg() {
        let a = HwpUnit::new(100).unwrap();
        assert_eq!((-a).as_i32(), -100);
    }

    #[test]
    fn hwpunit_mul_scalar() {
        let a = HwpUnit::new(100).unwrap();
        assert_eq!((a * 3).as_i32(), 300);
    }

    #[test]
    fn hwpunit_div_scalar() {
        let a = HwpUnit::new(300).unwrap();
        assert_eq!((a / 3).as_i32(), 100);
    }

    #[test]
    fn hwpunit_add_saturates_on_overflow() {
        let a = HwpUnit::new_unchecked(i32::MAX);
        let b = HwpUnit::new_unchecked(1);
        assert_eq!((a + b).as_i32(), i32::MAX);
    }

    #[test]
    fn hwpunit_display() {
        let u = HwpUnit::new(7200).unwrap();
        assert_eq!(u.to_string(), "7200 hwp");
    }

    #[test]
    fn hwpunit_debug() {
        let u = HwpUnit::new(100).unwrap();
        assert_eq!(format!("{u:?}"), "HwpUnit(100)");
    }

    #[test]
    fn hwpunit_default_is_zero() {
        assert_eq!(HwpUnit::default(), HwpUnit::ZERO);
    }

    #[test]
    fn hwpunit_ord() {
        let a = HwpUnit::new(100).unwrap();
        let b = HwpUnit::new(200).unwrap();
        assert!(a < b);
    }

    #[test]
    fn hwpunit_serde_roundtrip() {
        let u = HwpUnit::new(1200).unwrap();
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(json, "1200");
        let back: HwpUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, u);
    }

    // ===================================================================
    // Geometry types tests (10+)
    // ===================================================================

    #[test]
    fn size_a4_dimensions() {
        // A4 = 210mm x 297mm
        let a4 = Size::A4;
        assert!((HwpUnit(a4.width.as_i32()).to_mm() - 210.0).abs() < 0.1);
        assert!((HwpUnit(a4.height.as_i32()).to_mm() - 297.0).abs() < 0.1);
    }

    #[test]
    fn size_letter_dimensions() {
        let letter = Size::LETTER;
        assert_eq!(letter.width.as_i32(), 61200);
        assert_eq!(letter.height.as_i32(), 79200);
    }

    #[test]
    fn size_display() {
        let s = Size::new(HwpUnit::new_unchecked(100), HwpUnit::new_unchecked(200));
        assert_eq!(s.to_string(), "100 hwp x 200 hwp");
    }

    #[test]
    fn point_origin() {
        let o = Point::ORIGIN;
        assert_eq!(o.x, HwpUnit::ZERO);
        assert_eq!(o.y, HwpUnit::ZERO);
    }

    #[test]
    fn point_display() {
        let p = Point::new(HwpUnit::new_unchecked(10), HwpUnit::new_unchecked(20));
        assert_eq!(p.to_string(), "(10 hwp, 20 hwp)");
    }

    #[test]
    fn rect_construction() {
        let r = Rect::new(Point::ORIGIN, Size::A4);
        assert_eq!(r.origin, Point::ORIGIN);
        assert_eq!(r.size, Size::A4);
    }

    #[test]
    fn rect_display() {
        let r = Rect::new(
            Point::new(HwpUnit::new_unchecked(1), HwpUnit::new_unchecked(2)),
            Size::new(HwpUnit::new_unchecked(3), HwpUnit::new_unchecked(4)),
        );
        assert_eq!(r.to_string(), "3 hwp x 4 hwp @ (1 hwp, 2 hwp)");
    }

    #[test]
    fn insets_uniform() {
        let ins = Insets::uniform(HwpUnit::ONE_PT);
        assert_eq!(ins.top, HwpUnit::ONE_PT);
        assert_eq!(ins.bottom, HwpUnit::ONE_PT);
        assert_eq!(ins.left, HwpUnit::ONE_PT);
        assert_eq!(ins.right, HwpUnit::ONE_PT);
    }

    #[test]
    fn insets_symmetric() {
        let h = HwpUnit::new(10).unwrap();
        let v = HwpUnit::new(20).unwrap();
        let ins = Insets::symmetric(h, v);
        assert_eq!(ins.left, h);
        assert_eq!(ins.right, h);
        assert_eq!(ins.top, v);
        assert_eq!(ins.bottom, v);
    }

    #[test]
    fn geometry_serde_roundtrip() {
        let size = Size::A4;
        let json = serde_json::to_string(&size).unwrap();
        let back: Size = serde_json::from_str(&json).unwrap();
        assert_eq!(back, size);

        let rect = Rect::new(Point::ORIGIN, Size::A4);
        let json = serde_json::to_string(&rect).unwrap();
        let back: Rect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rect);
    }

    #[test]
    fn geometry_default_is_zero() {
        assert_eq!(Size::default(), Size::new(HwpUnit::ZERO, HwpUnit::ZERO));
        assert_eq!(Point::default(), Point::ORIGIN);
    }

    // ===================================================================
    // proptest
    // ===================================================================

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_hwpunit_pt_roundtrip(pt in -1_000_000.0f64..1_000_000.0f64) {
            if let Ok(u) = HwpUnit::from_pt(pt) {
                let back = u.to_pt();
                prop_assert!((back - pt).abs() < 0.01,
                    "pt={pt}, back={back}, diff={}", (back - pt).abs());
            }
        }

        #[test]
        fn prop_hwpunit_mm_roundtrip(mm in -350.0f64..350.0f64) {
            if let Ok(u) = HwpUnit::from_mm(mm) {
                let back = u.to_mm();
                prop_assert!((back - mm).abs() < 0.01,
                    "mm={mm}, back={back}, diff={}", (back - mm).abs());
            }
        }

        #[test]
        fn prop_hwpunit_inch_roundtrip(inch in -14.0f64..14.0f64) {
            if let Ok(u) = HwpUnit::from_inch(inch) {
                let back = u.to_inch();
                prop_assert!((back - inch).abs() < 0.001,
                    "inch={inch}, back={back}");
            }
        }
    }
}
