//! Page settings for document sections.
//!
//! [`PageSettings`] defines the physical dimensions of a page: width,
//! height, and margins. Each section in a document can have its own
//! page settings (e.g. landscape pages mixed with portrait).
//!
//! All measurements use [`HwpUnit`] from Foundation.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::PageSettings;
//!
//! let a4 = PageSettings::a4();
//! assert!(a4.width.to_mm() > 209.0);
//! assert!(a4.width.to_mm() < 211.0);
//!
//! let letter = PageSettings::letter();
//! assert!(letter.width.to_inch() > 8.4);
//! ```

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Physical page dimensions and margins for a section.
///
/// Contains 8 [`HwpUnit`] fields covering all geometry a page needs.
/// `Copy` because it is 32 bytes -- small enough to pass by value.
///
/// # Presets
///
/// - [`PageSettings::a4()`] -- A4 (210 mm x 297 mm) with 20 mm margins
/// - [`PageSettings::letter()`] -- US Letter (8.5" x 11") with 1" margins
///
/// # Examples
///
/// ```
/// use hwpforge_core::PageSettings;
/// use hwpforge_foundation::HwpUnit;
///
/// let custom = PageSettings {
///     width: HwpUnit::from_mm(148.0).unwrap(),
///     height: HwpUnit::from_mm(210.0).unwrap(),
///     ..PageSettings::a4()
/// };
/// assert!(custom.width.to_mm() < 149.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageSettings {
    /// Page width.
    pub width: HwpUnit,
    /// Page height.
    pub height: HwpUnit,
    /// Left margin.
    pub margin_left: HwpUnit,
    /// Right margin.
    pub margin_right: HwpUnit,
    /// Top margin.
    pub margin_top: HwpUnit,
    /// Bottom margin.
    pub margin_bottom: HwpUnit,
    /// Header margin (distance from page top to header baseline).
    pub header_margin: HwpUnit,
    /// Footer margin (distance from page bottom to footer baseline).
    pub footer_margin: HwpUnit,
}

// 8 x HwpUnit(i32 = 4 bytes) = 32 bytes
const _: () = assert!(std::mem::size_of::<PageSettings>() == 32);

impl PageSettings {
    /// A4 paper (210 mm x 297 mm) with 20 mm margins, 10 mm header/footer.
    ///
    /// These are the de-facto default settings for Korean government
    /// documents and the HWP editor's default.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::PageSettings;
    ///
    /// let a4 = PageSettings::a4();
    /// assert!((a4.width.to_mm() - 210.0).abs() < 0.1);
    /// assert!((a4.height.to_mm() - 297.0).abs() < 0.1);
    /// ```
    pub fn a4() -> Self {
        // round(210 * 7200/25.4) = 59528
        // round(297 * 7200/25.4) = 84188
        // round(20 * 7200/25.4)  = 5669
        // round(10 * 7200/25.4)  = 2835
        Self {
            width: HwpUnit::from_mm(210.0).unwrap(),
            height: HwpUnit::from_mm(297.0).unwrap(),
            margin_left: HwpUnit::from_mm(20.0).unwrap(),
            margin_right: HwpUnit::from_mm(20.0).unwrap(),
            margin_top: HwpUnit::from_mm(20.0).unwrap(),
            margin_bottom: HwpUnit::from_mm(20.0).unwrap(),
            header_margin: HwpUnit::from_mm(10.0).unwrap(),
            footer_margin: HwpUnit::from_mm(10.0).unwrap(),
        }
    }

    /// US Letter (8.5" x 11") with 1" margins, 0.5" header/footer.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::PageSettings;
    ///
    /// let letter = PageSettings::letter();
    /// assert_eq!(letter.width.as_i32(), 61200); // 8.5 * 7200
    /// assert_eq!(letter.height.as_i32(), 79200); // 11 * 7200
    /// ```
    pub fn letter() -> Self {
        Self {
            width: HwpUnit::from_inch(8.5).unwrap(),
            height: HwpUnit::from_inch(11.0).unwrap(),
            margin_left: HwpUnit::from_inch(1.0).unwrap(),
            margin_right: HwpUnit::from_inch(1.0).unwrap(),
            margin_top: HwpUnit::from_inch(1.0).unwrap(),
            margin_bottom: HwpUnit::from_inch(1.0).unwrap(),
            header_margin: HwpUnit::from_inch(0.5).unwrap(),
            footer_margin: HwpUnit::from_inch(0.5).unwrap(),
        }
    }

    /// Returns the printable width (page width minus left and right margins).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::PageSettings;
    ///
    /// let a4 = PageSettings::a4();
    /// let printable = a4.printable_width();
    /// // 210mm - 20mm - 20mm = 170mm
    /// assert!((printable.to_mm() - 170.0).abs() < 0.5);
    /// ```
    pub fn printable_width(&self) -> HwpUnit {
        self.width - self.margin_left - self.margin_right
    }

    /// Returns the printable height (page height minus top and bottom margins).
    pub fn printable_height(&self) -> HwpUnit {
        self.height - self.margin_top - self.margin_bottom
    }
}

impl Default for PageSettings {
    /// Default page settings are A4 with 20 mm margins.
    fn default() -> Self {
        Self::a4()
    }
}

impl std::fmt::Display for PageSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PageSettings({:.1}mm x {:.1}mm)",
            self.width.to_mm(),
            self.height.to_mm()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_dimensions() {
        let a4 = PageSettings::a4();
        assert!((a4.width.to_mm() - 210.0).abs() < 0.1, "width: {}", a4.width.to_mm());
        assert!((a4.height.to_mm() - 297.0).abs() < 0.1, "height: {}", a4.height.to_mm());
    }

    #[test]
    fn a4_margins() {
        let a4 = PageSettings::a4();
        assert!((a4.margin_left.to_mm() - 20.0).abs() < 0.1);
        assert!((a4.margin_right.to_mm() - 20.0).abs() < 0.1);
        assert!((a4.margin_top.to_mm() - 20.0).abs() < 0.1);
        assert!((a4.margin_bottom.to_mm() - 20.0).abs() < 0.1);
    }

    #[test]
    fn a4_header_footer_margins() {
        let a4 = PageSettings::a4();
        assert!((a4.header_margin.to_mm() - 10.0).abs() < 0.1);
        assert!((a4.footer_margin.to_mm() - 10.0).abs() < 0.1);
    }

    #[test]
    fn letter_dimensions() {
        let letter = PageSettings::letter();
        assert_eq!(letter.width.as_i32(), 61200);
        assert_eq!(letter.height.as_i32(), 79200);
    }

    #[test]
    fn letter_margins() {
        let letter = PageSettings::letter();
        assert_eq!(letter.margin_left.as_i32(), 7200);
        assert_eq!(letter.margin_right.as_i32(), 7200);
        assert_eq!(letter.margin_top.as_i32(), 7200);
        assert_eq!(letter.margin_bottom.as_i32(), 7200);
    }

    #[test]
    fn default_is_a4() {
        assert_eq!(PageSettings::default(), PageSettings::a4());
    }

    #[test]
    fn printable_width() {
        let a4 = PageSettings::a4();
        let pw = a4.printable_width();
        // 210 - 20 - 20 = 170mm
        assert!((pw.to_mm() - 170.0).abs() < 0.5, "printable width: {}mm", pw.to_mm());
    }

    #[test]
    fn printable_height() {
        let a4 = PageSettings::a4();
        let ph = a4.printable_height();
        // 297 - 20 - 20 = 257mm
        assert!((ph.to_mm() - 257.0).abs() < 0.5, "printable height: {}mm", ph.to_mm());
    }

    #[test]
    fn custom_page_with_struct_update() {
        let custom = PageSettings {
            width: HwpUnit::from_mm(148.0).unwrap(),
            height: HwpUnit::from_mm(210.0).unwrap(),
            ..PageSettings::a4()
        };
        assert!((custom.width.to_mm() - 148.0).abs() < 0.1);
        assert!((custom.height.to_mm() - 210.0).abs() < 0.1);
        // margins inherited from A4
        assert!((custom.margin_left.to_mm() - 20.0).abs() < 0.1);
    }

    #[test]
    fn size_assertion() {
        assert_eq!(std::mem::size_of::<PageSettings>(), 32);
    }

    #[test]
    fn display_format() {
        let a4 = PageSettings::a4();
        let s = a4.to_string();
        assert!(s.contains("210.0"), "display: {s}");
        assert!(s.contains("297.0"), "display: {s}");
    }

    #[test]
    fn copy_semantics() {
        let a = PageSettings::a4();
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn serde_roundtrip() {
        let ps = PageSettings::a4();
        let json = serde_json::to_string(&ps).unwrap();
        let back: PageSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(ps, back);
    }

    #[test]
    fn letter_serde_roundtrip() {
        let ps = PageSettings::letter();
        let json = serde_json::to_string(&ps).unwrap();
        let back: PageSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(ps, back);
    }
}
