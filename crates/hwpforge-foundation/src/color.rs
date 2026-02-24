//! BGR color type matching the HWP specification.
//!
//! HWP documents store colors in **BGR** byte order (blue in the high byte,
//! red in the low byte). This module provides [`Color`], a zero-cost `u32`
//! wrapper that makes BGR handling explicit and safe.
//!
//! Bits 24-31 are reserved (typically zero). The HWP5 format uses bit 24
//! as a transparency flag; this crate treats those bits as opaque data
//! preserved through round-trips.
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::Color;
//!
//! let red = Color::from_rgb(255, 0, 0);
//! assert_eq!(red.red(), 255);
//! assert_eq!(red.green(), 0);
//! assert_eq!(red.blue(), 0);
//! assert_eq!(red.to_raw(), 0x000000FF); // BGR: red in low byte
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// A color stored in BGR format, matching the HWP binary specification.
///
/// Internally a `u32` with `repr(transparent)` for zero overhead.
///
/// FROZEN: Do not change the internal representation after v1.0.
///
/// # Layout
///
/// ```text
/// Bits: [31..24 reserved] [23..16 blue] [15..8 green] [7..0 red]
/// ```
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::Color;
///
/// let c = Color::from_rgb(0x11, 0x22, 0x33);
/// assert_eq!(c.to_raw(), 0x00332211);
/// assert_eq!(c.to_string(), "#112233");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Color(u32);

// Compile-time size guarantee
const _: () = assert!(std::mem::size_of::<Color>() == 4);

impl Color {
    // Named constants (all with reserved bits = 0)

    /// Black: RGB(0, 0, 0).
    pub const BLACK: Self = Self(0x00000000);
    /// White: RGB(255, 255, 255).
    pub const WHITE: Self = Self(0x00FFFFFF);
    /// Pure red: RGB(255, 0, 0).
    pub const RED: Self = Self(0x000000FF);
    /// Pure green: RGB(0, 255, 0).
    pub const GREEN: Self = Self(0x0000FF00);
    /// Pure blue: RGB(0, 0, 255).
    pub const BLUE: Self = Self(0x00FF0000);

    /// Constructs a [`Color`] from RGB components.
    ///
    /// The components are stored in BGR order internally.
    /// Reserved bits (24-31) are set to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::Color;
    ///
    /// let magenta = Color::from_rgb(255, 0, 255);
    /// assert_eq!(magenta.red(), 255);
    /// assert_eq!(magenta.blue(), 255);
    /// ```
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self((b as u32) << 16 | (g as u32) << 8 | (r as u32))
    }

    /// Extracts the RGB components as a tuple `(r, g, b)`.
    pub const fn to_rgb(self) -> (u8, u8, u8) {
        (self.red(), self.green(), self.blue())
    }

    /// Constructs a [`Color`] from a raw BGR `u32`.
    ///
    /// Use this when reading values directly from HWP binary data.
    /// All 32 bits are preserved (including reserved bits 24-31).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::Color;
    ///
    /// let c = Color::from_raw(0x00FF0000);
    /// assert_eq!(c.blue(), 255);
    /// assert_eq!(c.red(), 0);
    /// ```
    pub const fn from_raw(bgr: u32) -> Self {
        Self(bgr)
    }

    /// Returns the raw BGR `u32` value.
    pub const fn to_raw(self) -> u32 {
        self.0
    }

    /// Returns the red component (bits 0-7).
    pub const fn red(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Returns the green component (bits 8-15).
    pub const fn green(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Returns the blue component (bits 16-23).
    pub const fn blue(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

impl fmt::Debug for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Color(#{:02X}{:02X}{:02X})", self.red(), self.green(), self.blue())
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.red(), self.green(), self.blue())
    }
}

impl schemars::JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // Color edge cases (10+)
    // ===================================================================

    // Edge Case 1: Pure red -- BGR byte order check
    #[test]
    fn color_pure_red_bgr_order() {
        let c = Color::from_rgb(255, 0, 0);
        assert_eq!(c.to_raw(), 0x000000FF);
        assert_eq!(c.red(), 255);
        assert_eq!(c.green(), 0);
        assert_eq!(c.blue(), 0);
    }

    // Edge Case 2: Pure blue -- high byte in BGR
    #[test]
    fn color_pure_blue_bgr_order() {
        let c = Color::from_rgb(0, 0, 255);
        assert_eq!(c.to_raw(), 0x00FF0000);
        assert_eq!(c.blue(), 255);
        assert_eq!(c.red(), 0);
    }

    // Edge Case 3: Black
    #[test]
    fn color_black() {
        let c = Color::from_rgb(0, 0, 0);
        assert_eq!(c, Color::BLACK);
        assert_eq!(c.to_raw(), 0x00000000);
    }

    // Edge Case 4: White
    #[test]
    fn color_white() {
        let c = Color::from_rgb(255, 255, 255);
        assert_eq!(c, Color::WHITE);
        assert_eq!(c.to_raw(), 0x00FFFFFF);
    }

    // Edge Case 5: RGB -> BGR -> RGB roundtrip
    #[test]
    fn color_rgb_roundtrip() {
        let (r, g, b) = (0x11, 0x22, 0x33);
        let c = Color::from_rgb(r, g, b);
        assert_eq!(c.to_rgb(), (r, g, b));
    }

    // Edge Case 6: Raw u32::MAX -- reserved bits preserved
    #[test]
    fn color_from_raw_u32_max() {
        let c = Color::from_raw(u32::MAX);
        assert_eq!(c.red(), 255);
        assert_eq!(c.green(), 255);
        assert_eq!(c.blue(), 255);
        assert_eq!(c.to_raw(), u32::MAX);
    }

    // Edge Case 7: Raw 0 == BLACK
    #[test]
    fn color_from_raw_zero() {
        let c = Color::from_raw(0);
        assert_eq!(c, Color::BLACK);
    }

    // Edge Case 8: Named constants match from_rgb
    #[test]
    fn color_named_constants() {
        assert_eq!(Color::RED, Color::from_rgb(255, 0, 0));
        assert_eq!(Color::GREEN, Color::from_rgb(0, 255, 0));
        assert_eq!(Color::BLUE, Color::from_rgb(0, 0, 255));
        assert_eq!(Color::BLACK, Color::from_rgb(0, 0, 0));
        assert_eq!(Color::WHITE, Color::from_rgb(255, 255, 255));
    }

    // Edge Case 9: Display as #RRGGBB
    #[test]
    fn color_display_hex() {
        let c = Color::from_rgb(0xAB, 0xCD, 0xEF);
        assert_eq!(c.to_string(), "#ABCDEF");
    }

    // Edge Case 10: Default is BLACK
    #[test]
    fn color_default_is_black() {
        assert_eq!(Color::default(), Color::BLACK);
    }

    // Additional tests

    #[test]
    fn color_debug_format() {
        let c = Color::from_rgb(0x11, 0x22, 0x33);
        assert_eq!(format!("{c:?}"), "Color(#112233)");
    }

    #[test]
    fn color_copy_and_hash() {
        use std::collections::HashSet;
        let c = Color::RED;
        let c2 = c; // Copy
        assert_eq!(c, c2);

        let mut set = HashSet::new();
        set.insert(Color::RED);
        set.insert(Color::GREEN);
        set.insert(Color::RED); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn color_serde_roundtrip() {
        let c = Color::from_rgb(0xAA, 0xBB, 0xCC);
        let json = serde_json::to_string(&c).unwrap();
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn color_individual_components_isolated() {
        // Ensure bit manipulation doesn't bleed between components
        let c = Color::from_rgb(0x01, 0x00, 0x00);
        assert_eq!(c.red(), 1);
        assert_eq!(c.green(), 0);
        assert_eq!(c.blue(), 0);

        let c = Color::from_rgb(0x00, 0x01, 0x00);
        assert_eq!(c.red(), 0);
        assert_eq!(c.green(), 1);
        assert_eq!(c.blue(), 0);

        let c = Color::from_rgb(0x00, 0x00, 0x01);
        assert_eq!(c.red(), 0);
        assert_eq!(c.green(), 0);
        assert_eq!(c.blue(), 1);
    }

    // ===================================================================
    // proptest
    // ===================================================================

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_color_rgb_roundtrip(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let c = Color::from_rgb(r, g, b);
            prop_assert_eq!(c.to_rgb(), (r, g, b));
        }

        #[test]
        fn prop_color_raw_preserves_bits(raw in 0u32..=0x00FFFFFF) {
            let c = Color::from_raw(raw);
            prop_assert_eq!(c.to_raw(), raw);
            // Components should reconstruct the raw value
            let r = c.red() as u32;
            let g = (c.green() as u32) << 8;
            let b = (c.blue() as u32) << 16;
            prop_assert_eq!(r | g | b, raw);
        }
    }
}
