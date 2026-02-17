//! Multi-column layout settings for document sections.
//!
//! A [`ColumnSettings`] describes how a section is divided into multiple
//! columns (다단). In HWPX this maps to `<hp:ctrl><hp:colPr>` elements
//! appearing after `</hp:secPr>` in the first run of the first paragraph.
//!
//! Single-column layout is represented as `None` on [`Section`](crate::section::Section),
//! not as a `ColumnSettings` with one column. This keeps the common case
//! (single column) zero-cost and matches HWPX conventions.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::column::{ColumnSettings, ColumnType, ColumnLayoutMode, ColumnDef};
//! use hwpforge_foundation::HwpUnit;
//!
//! // Equal-width 2-column layout with 4mm gap
//! let cols = ColumnSettings::equal_columns(2, HwpUnit::from_mm(4.0).unwrap());
//! assert_eq!(cols.columns.len(), 2);
//! assert_eq!(cols.column_type, ColumnType::Newspaper);
//! ```

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ColumnType
// ---------------------------------------------------------------------------

/// Column flow type: how text flows between columns.
///
/// In HWPX this maps to the `type` attribute on `<hp:colPr>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ColumnType {
    /// Text flows from column 1 -> 2 -> 3 (newspaper style). Most common.
    #[default]
    Newspaper,
    /// Each column is independent (side-by-side comparisons). Rare.
    Parallel,
}

// ---------------------------------------------------------------------------
// ColumnLayoutMode
// ---------------------------------------------------------------------------

/// Column balance strategy.
///
/// In HWPX this maps to the `layout` attribute on `<hp:colPr>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ColumnLayoutMode {
    /// Balance towards left column. Most common.
    #[default]
    Left,
    /// Balance towards right column.
    Right,
    /// Symmetric balance (mirrors on odd/even pages).
    Mirror,
}

// ---------------------------------------------------------------------------
// ColumnDef
// ---------------------------------------------------------------------------

/// Individual column dimensions.
///
/// Each column has a width and a gap (space after the column).
/// The last column's gap should be [`HwpUnit::ZERO`].
///
/// # Examples
///
/// ```
/// use hwpforge_core::column::ColumnDef;
/// use hwpforge_foundation::HwpUnit;
///
/// let col = ColumnDef {
///     width: HwpUnit::from_mm(80.0).unwrap(),
///     gap: HwpUnit::from_mm(4.0).unwrap(),
/// };
/// assert!(col.width.as_i32() > 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnDef {
    /// Column width (HWPUNIT).
    pub width: HwpUnit,
    /// Gap after this column (HWPUNIT). Last column gap is always 0.
    pub gap: HwpUnit,
}

// ---------------------------------------------------------------------------
// ColumnSettings
// ---------------------------------------------------------------------------

/// Multi-column layout settings for a section.
///
/// Maps to HWPX `<hp:ctrl><hp:colPr>`. Single-column layout is
/// represented as `None` on [`Section`](crate::section::Section)
/// rather than a `ColumnSettings` with one column.
///
/// # Examples
///
/// ```
/// use hwpforge_core::column::{ColumnSettings, ColumnType, ColumnLayoutMode};
/// use hwpforge_foundation::HwpUnit;
///
/// let cs = ColumnSettings::equal_columns(3, HwpUnit::from_mm(4.0).unwrap());
/// assert_eq!(cs.columns.len(), 3);
/// assert_eq!(cs.column_type, ColumnType::Newspaper);
/// assert_eq!(cs.layout_mode, ColumnLayoutMode::Left);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnSettings {
    /// Column flow type.
    pub column_type: ColumnType,
    /// Column balance strategy.
    pub layout_mode: ColumnLayoutMode,
    /// Individual column definitions. Length = number of columns (>= 2).
    pub columns: Vec<ColumnDef>,
}

impl ColumnSettings {
    /// Creates an equal-width N-column layout with the given gap.
    ///
    /// All columns get the same gap value (last column gap is set to zero
    /// by the encoder). Uses [`ColumnType::Newspaper`] and
    /// [`ColumnLayoutMode::Left`] as defaults.
    ///
    /// # Panics
    ///
    /// Panics if `count < 2` (single-column should be `None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::column::ColumnSettings;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let cs = ColumnSettings::equal_columns(2, HwpUnit::from_mm(4.0).unwrap());
    /// assert_eq!(cs.columns.len(), 2);
    /// ```
    pub fn equal_columns(count: u32, gap: HwpUnit) -> Self {
        assert!(count >= 2, "column count must be >= 2 (use None for single column)");
        let columns: Vec<ColumnDef> = (0..count)
            .map(|i| ColumnDef {
                width: HwpUnit::ZERO, // widths calculated by 한글 when sameSz=1
                gap: if i < count - 1 { gap } else { HwpUnit::ZERO },
            })
            .collect();
        Self { column_type: ColumnType::Newspaper, layout_mode: ColumnLayoutMode::Left, columns }
    }

    /// Creates a variable-width column layout from explicit definitions.
    ///
    /// Uses [`ColumnType::Newspaper`] and [`ColumnLayoutMode::Left`] as defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if `columns.len() < 2` (single-column should be `None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::column::{ColumnSettings, ColumnDef};
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let cs = ColumnSettings::custom(vec![
    ///     ColumnDef { width: HwpUnit::new(14000).unwrap(), gap: HwpUnit::new(1134).unwrap() },
    ///     ColumnDef { width: HwpUnit::new(27000).unwrap(), gap: HwpUnit::ZERO },
    /// ]).unwrap();
    /// assert_eq!(cs.columns.len(), 2);
    /// ```
    pub fn custom(columns: Vec<ColumnDef>) -> Result<Self, &'static str> {
        if columns.len() < 2 {
            return Err("column count must be >= 2 (use None for single column)");
        }
        Ok(Self {
            column_type: ColumnType::Newspaper,
            layout_mode: ColumnLayoutMode::Left,
            columns,
        })
    }

    /// Returns the number of columns.
    pub fn count(&self) -> usize {
        self.columns.len()
    }

    /// Returns `true` if all columns have the same width (or width is zero,
    /// meaning 한글 calculates equal widths).
    pub fn is_equal_width(&self) -> bool {
        if self.columns.is_empty() {
            return true;
        }
        let first = self.columns[0].width;
        self.columns.iter().all(|c| c.width == first)
    }
}

impl std::fmt::Display for ColumnSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ColumnSettings({} columns, {:?})", self.columns.len(), self.column_type)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_columns_2() {
        let gap = HwpUnit::new(1134).unwrap();
        let cs = ColumnSettings::equal_columns(2, gap);
        assert_eq!(cs.count(), 2);
        assert_eq!(cs.column_type, ColumnType::Newspaper);
        assert_eq!(cs.layout_mode, ColumnLayoutMode::Left);
        assert_eq!(cs.columns[0].gap, gap);
        assert_eq!(cs.columns[1].gap, HwpUnit::ZERO);
        assert!(cs.is_equal_width());
    }

    #[test]
    fn equal_columns_3() {
        let gap = HwpUnit::new(1134).unwrap();
        let cs = ColumnSettings::equal_columns(3, gap);
        assert_eq!(cs.count(), 3);
        assert_eq!(cs.columns[0].gap, gap);
        assert_eq!(cs.columns[1].gap, gap);
        assert_eq!(cs.columns[2].gap, HwpUnit::ZERO);
    }

    #[test]
    #[should_panic(expected = "column count must be >= 2")]
    fn equal_columns_panics_on_1() {
        ColumnSettings::equal_columns(1, HwpUnit::ZERO);
    }

    #[test]
    fn custom_columns() {
        let cs = ColumnSettings::custom(vec![
            ColumnDef { width: HwpUnit::new(14000).unwrap(), gap: HwpUnit::new(1134).unwrap() },
            ColumnDef { width: HwpUnit::new(27000).unwrap(), gap: HwpUnit::ZERO },
        ])
        .unwrap();
        assert_eq!(cs.count(), 2);
        assert!(!cs.is_equal_width());
        assert_eq!(cs.columns[0].width.as_i32(), 14000);
        assert_eq!(cs.columns[1].width.as_i32(), 27000);
    }

    #[test]
    fn custom_returns_error_on_1() {
        let result =
            ColumnSettings::custom(vec![ColumnDef { width: HwpUnit::ZERO, gap: HwpUnit::ZERO }]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "column count must be >= 2 (use None for single column)");
    }

    #[test]
    fn serde_roundtrip() {
        let cs = ColumnSettings::equal_columns(2, HwpUnit::new(1134).unwrap());
        let json = serde_json::to_string(&cs).unwrap();
        let back: ColumnSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(cs, back);
    }

    #[test]
    fn serde_roundtrip_custom() {
        let cs = ColumnSettings::custom(vec![
            ColumnDef { width: HwpUnit::new(14000).unwrap(), gap: HwpUnit::new(1134).unwrap() },
            ColumnDef { width: HwpUnit::new(27000).unwrap(), gap: HwpUnit::ZERO },
        ])
        .unwrap();
        let json = serde_json::to_string(&cs).unwrap();
        let back: ColumnSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(cs, back);
    }

    #[test]
    fn display() {
        let cs = ColumnSettings::equal_columns(2, HwpUnit::new(1134).unwrap());
        let s = cs.to_string();
        assert!(s.contains("2 columns"), "display: {s}");
        assert!(s.contains("Newspaper"), "display: {s}");
    }

    #[test]
    fn default_types() {
        assert_eq!(ColumnType::default(), ColumnType::Newspaper);
        assert_eq!(ColumnLayoutMode::default(), ColumnLayoutMode::Left);
    }

    #[test]
    fn parallel_type() {
        let mut cs = ColumnSettings::equal_columns(2, HwpUnit::ZERO);
        cs.column_type = ColumnType::Parallel;
        assert_eq!(cs.column_type, ColumnType::Parallel);
    }

    #[test]
    fn mirror_layout() {
        let mut cs = ColumnSettings::equal_columns(2, HwpUnit::ZERO);
        cs.layout_mode = ColumnLayoutMode::Mirror;
        assert_eq!(cs.layout_mode, ColumnLayoutMode::Mirror);
    }

    #[test]
    fn is_equal_width_with_zero_widths() {
        let cs = ColumnSettings::equal_columns(3, HwpUnit::new(1134).unwrap());
        // All widths are ZERO (sameSz mode), which counts as equal
        assert!(cs.is_equal_width());
    }

    #[test]
    fn clone_independence() {
        let cs = ColumnSettings::equal_columns(2, HwpUnit::new(1134).unwrap());
        let mut cloned = cs.clone();
        cloned.column_type = ColumnType::Parallel;
        assert_eq!(cs.column_type, ColumnType::Newspaper);
        assert_eq!(cloned.column_type, ColumnType::Parallel);
    }

    #[test]
    fn column_settings_serde_roundtrip() {
        let cs = ColumnSettings::equal_columns(2, HwpUnit::new(1134).unwrap());
        let json = serde_json::to_string(&cs).unwrap();
        let back: ColumnSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(cs, back);
    }
}
