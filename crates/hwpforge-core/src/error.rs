//! Error types for the HwpForge Core crate.
//!
//! All validation and structural errors produced by Core live here.
//! Error codes occupy the **2000-2999** range, extending the Foundation
//! convention (1000-1999).
//!
//! # Error Hierarchy
//!
//! [`CoreError`] is the top-level error. It wraps:
//! - [`ValidationError`] -- document structure validation failures
//! - [`FoundationError`] -- propagated Foundation errors
//! - `InvalidStructure` -- catch-all for structural issues
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::error::{CoreError, ValidationError};
//!
//! let err = CoreError::from(ValidationError::EmptyDocument);
//! assert!(err.to_string().contains("section"));
//! ```

use hwpforge_foundation::FoundationError;

/// Top-level error type for the Core crate.
///
/// Every fallible operation in Core returns `Result<T, CoreError>`.
/// Use the `?` operator freely -- both [`ValidationError`] and
/// [`FoundationError`] convert via `#[from]`.
///
/// # Examples
///
/// ```
/// use hwpforge_core::error::{CoreError, ValidationError};
///
/// fn example() -> Result<(), CoreError> {
///     Err(ValidationError::EmptyDocument)?
/// }
/// assert!(example().is_err());
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// Document validation failed.
    #[error("Document validation failed: {0}")]
    Validation(#[from] ValidationError),

    /// A Foundation-layer error propagated upward.
    #[error("Foundation error: {0}")]
    Foundation(#[from] FoundationError),

    /// Structural issue that is not a validation failure.
    #[error("Invalid document structure in {context}: {reason}")]
    InvalidStructure {
        /// Where in the document the issue was found.
        context: String,
        /// What went wrong.
        reason: String,
    },
}

/// Specific validation failures with precise location context.
///
/// Every variant carries enough information to pinpoint the
/// exact location of the problem (section index, paragraph index, etc.).
///
/// Marked `#[non_exhaustive]` so future phases can add variants
/// without a breaking change.
///
/// # Examples
///
/// ```
/// use hwpforge_core::error::ValidationError;
///
/// let err = ValidationError::EmptySection { section_index: 2 };
/// assert!(err.to_string().contains("Section 2"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidationError {
    /// The document contains zero sections.
    #[error("Empty document: at least 1 section required")]
    EmptyDocument,

    /// A section contains zero paragraphs.
    #[error("Section {section_index} has no paragraphs")]
    EmptySection {
        /// Zero-based index of the offending section.
        section_index: usize,
    },

    /// A paragraph contains zero runs.
    #[error("Paragraph has no runs (section {section_index}, paragraph {paragraph_index})")]
    EmptyParagraph {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index within the section.
        paragraph_index: usize,
    },

    /// A table contains zero rows.
    #[error(
        "Table has no rows (section {section_index}, paragraph {paragraph_index}, run {run_index})"
    )]
    EmptyTable {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// A table row contains zero cells.
    #[error("Table row has no cells (section {section_index}, paragraph {paragraph_index}, run {run_index}, row {row_index})")]
    EmptyTableRow {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Zero-based row index within the table.
        row_index: usize,
    },

    /// A span value (col_span or row_span) is zero.
    #[error("Invalid span: {field} = {value} (section {section_index}, paragraph {paragraph_index}, run {run_index}, row {row_index}, cell {cell_index})")]
    InvalidSpan {
        /// Which span field failed ("col_span" or "row_span").
        field: &'static str,
        /// The invalid value.
        value: u16,
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Zero-based row index.
        row_index: usize,
        /// Zero-based cell index.
        cell_index: usize,
    },

    /// A TextBox control contains zero paragraphs.
    #[error("TextBox has no paragraphs (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyTextBox {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// A Footnote control contains zero paragraphs.
    #[error("Footnote has no paragraphs (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyFootnote {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// An Endnote control contains zero paragraphs.
    #[error("Endnote has no paragraphs (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyEndnote {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// A Polygon control has fewer than 3 vertices.
    #[error("Polygon has invalid vertex count: {vertex_count} (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    InvalidPolygon {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Number of vertices found.
        vertex_count: usize,
    },

    /// A shape (Ellipse or Polygon) has zero width or height.
    #[error("Shape {shape_type} has zero dimension (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    InvalidShapeDimension {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Type of shape ("Ellipse" or "Polygon").
        shape_type: &'static str,
    },

    /// A Chart control has no data series.
    #[error("Chart has empty data (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyChartData {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// An Equation control has an empty script.
    #[error("Equation has empty script (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyEquation {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// A Category chart has an empty categories list.
    #[error("Chart has empty category labels (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    EmptyCategoryLabels {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
    },

    /// An XY series has mismatched x/y value lengths.
    #[error("XY series '{series_name}' has mismatched lengths: x={x_len}, y={y_len} (section {section_index}, paragraph {paragraph_index}, run {run_index})")]
    MismatchedSeriesLengths {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Name of the offending series.
        series_name: String,
        /// Length of x_values.
        x_len: usize,
        /// Length of y_values.
        y_len: usize,
    },

    /// A table cell contains zero paragraphs.
    #[error("Table cell has no paragraphs (section {section_index}, paragraph {paragraph_index}, run {run_index}, row {row_index}, cell {cell_index})")]
    EmptyTableCell {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Zero-based row index.
        row_index: usize,
        /// Zero-based cell index.
        cell_index: usize,
    },

    /// A header row appears after a non-header row.
    #[error("Table header row is not part of the leading header block (section {section_index}, paragraph {paragraph_index}, run {run_index}, row {row_index})")]
    NonLeadingTableHeaderRow {
        /// Zero-based section index.
        section_index: usize,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index.
        run_index: usize,
        /// Zero-based row index.
        row_index: usize,
    },
}

// ---------------------------------------------------------------------------
// ErrorCode integration
// ---------------------------------------------------------------------------

/// Core validation error codes (2000-2099).
///
/// Extends Foundation's [`ErrorCode`](hwpforge_foundation::ErrorCode) convention into the Core range.
///
/// # Examples
///
/// ```
/// use hwpforge_core::error::CoreErrorCode;
///
/// assert_eq!(CoreErrorCode::EmptyDocument as u32, 2000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum CoreErrorCode {
    /// Empty document (no sections).
    EmptyDocument = 2000,
    /// Empty section (no paragraphs).
    EmptySection = 2001,
    /// Empty paragraph (no runs).
    EmptyParagraph = 2002,
    /// Empty table (no rows).
    EmptyTable = 2003,
    /// Empty table row (no cells).
    EmptyTableRow = 2004,
    /// Invalid span value (zero).
    InvalidSpan = 2005,
    /// Empty TextBox (no paragraphs).
    EmptyTextBox = 2006,
    /// Empty Footnote (no paragraphs).
    EmptyFootnote = 2007,
    /// Empty table cell (no paragraphs).
    EmptyTableCell = 2008,
    /// Empty Endnote (no paragraphs).
    EmptyEndnote = 2009,
    /// Invalid Polygon (fewer than 3 vertices).
    InvalidPolygon = 2010,
    /// Invalid shape dimension (zero width or height).
    InvalidShapeDimension = 2011,
    /// Empty Equation (empty script).
    EmptyEquation = 2012,
    /// Empty Chart data (no series).
    EmptyChartData = 2013,
    /// Empty category labels in a Category chart.
    EmptyCategoryLabels = 2014,
    /// Mismatched x/y value lengths in an XY series.
    MismatchedSeriesLengths = 2015,
    /// Non-leading table header row.
    NonLeadingTableHeaderRow = 2016,
    /// Invalid document structure.
    InvalidStructure = 2100,
}

impl std::fmt::Display for CoreErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "E{:04}", *self as u32)
    }
}

impl ValidationError {
    /// Returns the numeric error code for this validation error.
    pub fn code(&self) -> CoreErrorCode {
        match self {
            Self::EmptyDocument => CoreErrorCode::EmptyDocument,
            Self::EmptySection { .. } => CoreErrorCode::EmptySection,
            Self::EmptyParagraph { .. } => CoreErrorCode::EmptyParagraph,
            Self::EmptyTable { .. } => CoreErrorCode::EmptyTable,
            Self::EmptyTableRow { .. } => CoreErrorCode::EmptyTableRow,
            Self::InvalidSpan { .. } => CoreErrorCode::InvalidSpan,
            Self::NonLeadingTableHeaderRow { .. } => CoreErrorCode::NonLeadingTableHeaderRow,
            Self::EmptyTextBox { .. } => CoreErrorCode::EmptyTextBox,
            Self::EmptyFootnote { .. } => CoreErrorCode::EmptyFootnote,
            Self::EmptyTableCell { .. } => CoreErrorCode::EmptyTableCell,
            Self::EmptyEndnote { .. } => CoreErrorCode::EmptyEndnote,
            Self::InvalidPolygon { .. } => CoreErrorCode::InvalidPolygon,
            Self::InvalidShapeDimension { .. } => CoreErrorCode::InvalidShapeDimension,
            Self::EmptyChartData { .. } => CoreErrorCode::EmptyChartData,
            Self::EmptyCategoryLabels { .. } => CoreErrorCode::EmptyCategoryLabels,
            Self::MismatchedSeriesLengths { .. } => CoreErrorCode::MismatchedSeriesLengths,
            Self::EmptyEquation { .. } => CoreErrorCode::EmptyEquation,
        }
    }
}

/// Convenience type alias for Core operations.
///
/// # Examples
///
/// ```
/// use hwpforge_core::error::CoreResult;
///
/// fn always_ok() -> CoreResult<i32> {
///     Ok(42)
/// }
/// assert_eq!(always_ok().unwrap(), 42);
/// ```
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    // === Variant construction ===

    #[test]
    fn empty_document_displays_message() {
        let err = ValidationError::EmptyDocument;
        let msg = err.to_string();
        assert!(msg.contains("section"), "msg: {msg}");
        assert!(msg.contains("at least 1"), "msg: {msg}");
    }

    #[test]
    fn empty_section_displays_index() {
        let err = ValidationError::EmptySection { section_index: 3 };
        let msg = err.to_string();
        assert!(msg.contains("3"), "msg: {msg}");
        assert!(msg.contains("no paragraphs"), "msg: {msg}");
    }

    #[test]
    fn empty_paragraph_displays_location() {
        let err = ValidationError::EmptyParagraph { section_index: 1, paragraph_index: 5 };
        let msg = err.to_string();
        assert!(msg.contains("section 1"), "msg: {msg}");
        assert!(msg.contains("paragraph 5"), "msg: {msg}");
    }

    #[test]
    fn empty_table_displays_location() {
        let err =
            ValidationError::EmptyTable { section_index: 0, paragraph_index: 2, run_index: 0 };
        let msg = err.to_string();
        assert!(msg.contains("no rows"), "msg: {msg}");
    }

    #[test]
    fn empty_table_row_displays_location() {
        let err = ValidationError::EmptyTableRow {
            section_index: 0,
            paragraph_index: 0,
            run_index: 0,
            row_index: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("row 1"), "msg: {msg}");
        assert!(msg.contains("no cells"), "msg: {msg}");
    }

    #[test]
    fn invalid_span_displays_all_context() {
        let err = ValidationError::InvalidSpan {
            field: "col_span",
            value: 0,
            section_index: 0,
            paragraph_index: 1,
            run_index: 0,
            row_index: 0,
            cell_index: 2,
        };
        let msg = err.to_string();
        assert!(msg.contains("col_span"), "msg: {msg}");
        assert!(msg.contains("= 0"), "msg: {msg}");
        assert!(msg.contains("cell 2"), "msg: {msg}");
    }

    #[test]
    fn empty_text_box_displays_location() {
        let err =
            ValidationError::EmptyTextBox { section_index: 0, paragraph_index: 0, run_index: 1 };
        let msg = err.to_string();
        assert!(msg.contains("TextBox"), "msg: {msg}");
    }

    #[test]
    fn empty_footnote_displays_location() {
        let err =
            ValidationError::EmptyFootnote { section_index: 0, paragraph_index: 0, run_index: 0 };
        let msg = err.to_string();
        assert!(msg.contains("Footnote"), "msg: {msg}");
    }

    #[test]
    fn empty_table_cell_displays_location() {
        let err = ValidationError::EmptyTableCell {
            section_index: 0,
            paragraph_index: 0,
            run_index: 0,
            row_index: 0,
            cell_index: 0,
        };
        let msg = err.to_string();
        assert!(msg.contains("cell"), "msg: {msg}");
    }

    // === CoreError wrapping ===

    #[test]
    fn core_error_from_validation() {
        let ve = ValidationError::EmptyDocument;
        let ce: CoreError = ve.into();
        match ce {
            CoreError::Validation(v) => assert_eq!(v, ValidationError::EmptyDocument),
            other => panic!("expected Validation, got: {other}"),
        }
    }

    #[test]
    fn core_error_from_foundation() {
        let fe =
            FoundationError::InvalidField { field: "test".to_string(), reason: "bad".to_string() };
        let ce: CoreError = fe.into();
        assert!(matches!(ce, CoreError::Foundation(_)));
    }

    #[test]
    fn core_error_invalid_structure() {
        let ce = CoreError::InvalidStructure {
            context: "document".to_string(),
            reason: "circular reference".to_string(),
        };
        let msg = ce.to_string();
        assert!(msg.contains("document"), "msg: {msg}");
        assert!(msg.contains("circular"), "msg: {msg}");
    }

    // === Error codes ===

    #[test]
    fn error_codes_in_core_range() {
        assert_eq!(CoreErrorCode::EmptyDocument as u32, 2000);
        assert_eq!(CoreErrorCode::EmptySection as u32, 2001);
        assert_eq!(CoreErrorCode::EmptyParagraph as u32, 2002);
        assert_eq!(CoreErrorCode::EmptyTable as u32, 2003);
        assert_eq!(CoreErrorCode::EmptyTableRow as u32, 2004);
        assert_eq!(CoreErrorCode::InvalidSpan as u32, 2005);
        assert_eq!(CoreErrorCode::EmptyTextBox as u32, 2006);
        assert_eq!(CoreErrorCode::EmptyFootnote as u32, 2007);
        assert_eq!(CoreErrorCode::EmptyTableCell as u32, 2008);
        assert_eq!(CoreErrorCode::InvalidStructure as u32, 2100);
    }

    #[test]
    fn error_code_display_format() {
        assert_eq!(CoreErrorCode::EmptyDocument.to_string(), "E2000");
        assert_eq!(CoreErrorCode::InvalidStructure.to_string(), "E2100");
    }

    #[test]
    fn validation_error_code_mapping() {
        assert_eq!(ValidationError::EmptyDocument.code(), CoreErrorCode::EmptyDocument);
        assert_eq!(
            ValidationError::EmptySection { section_index: 0 }.code(),
            CoreErrorCode::EmptySection
        );
        assert_eq!(
            ValidationError::EmptyParagraph { section_index: 0, paragraph_index: 0 }.code(),
            CoreErrorCode::EmptyParagraph
        );
    }

    // === CoreResult alias ===

    #[test]
    fn core_result_alias_works() {
        fn ok_example() -> CoreResult<i32> {
            Ok(42)
        }
        fn err_example() -> CoreResult<i32> {
            Err(ValidationError::EmptyDocument)?
        }
        assert_eq!(ok_example().unwrap(), 42);
        assert!(err_example().is_err());
    }

    // === Send + Sync ===

    #[test]
    fn errors_are_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<CoreError>();
        assert_sync::<CoreError>();
        assert_send::<ValidationError>();
        assert_sync::<ValidationError>();
    }

    // === std::error::Error ===

    #[test]
    fn core_error_implements_std_error() {
        let err = CoreError::from(ValidationError::EmptyDocument);
        let _: &dyn std::error::Error = &err;
    }

    // === ValidationError PartialEq ===

    #[test]
    fn validation_error_eq() {
        let a = ValidationError::EmptyDocument;
        let b = ValidationError::EmptyDocument;
        let c = ValidationError::EmptySection { section_index: 0 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
