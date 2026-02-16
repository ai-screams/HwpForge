//! Error types for the HwpForge Foundation crate.
//!
//! All Foundation types return [`FoundationError`] on validation failure.
//! Each error variant carries enough context for debugging.
//!
//! # Error Code Ranges
//!
//! | Range | Crate |
//! |-------|-------|
//! | 1000-1999 | Foundation |
//! | 2000-2999 | Core |
//! | 3000-3999 | Blueprint |
//! | 4000-4999 | Smithy-HWPX |
//! | 5000-5999 | Smithy-HWP5 |
//! | 6000-6999 | Smithy-MD |

use std::fmt;

/// Numeric error codes for programmatic handling and FFI.
///
/// Foundation errors occupy the 1000-1999 range.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ErrorCode;
///
/// let code = ErrorCode::InvalidHwpUnit;
/// assert_eq!(code as u32, 1000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ErrorCode {
    /// HwpUnit value out of valid range.
    InvalidHwpUnit = 1000,
    /// Color component or raw value invalid.
    InvalidColor = 1001,
    /// Branded index exceeded collection bounds.
    IndexOutOfBounds = 1002,
    /// String identifier was empty or invalid.
    EmptyIdentifier = 1003,
    /// Generic field validation failure.
    InvalidField = 1004,
    /// String-to-enum parsing failure.
    ParseError = 1005,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", *self as u32)
    }
}

/// Trait for mapping domain errors to numeric [`ErrorCode`] values.
///
/// Each crate's error type implements this to provide a stable,
/// FFI-safe error code.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{FoundationError, ErrorCodeExt, ErrorCode};
///
/// let err = FoundationError::EmptyIdentifier {
///     item: "FontId".to_string(),
/// };
/// assert_eq!(err.code(), ErrorCode::EmptyIdentifier);
/// ```
pub trait ErrorCodeExt {
    /// Returns the numeric error code for this error.
    fn code(&self) -> ErrorCode;
}

/// The primary error type for the Foundation crate.
///
/// Returned by constructors and validators when input violates
/// constraints. Every variant carries enough context to produce
/// a meaningful diagnostic message.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::FoundationError;
///
/// let err = FoundationError::InvalidHwpUnit {
///     value: 999_999_999,
///     min: -100_000_000,
///     max: 100_000_000,
/// };
/// assert!(err.to_string().contains("999999999"));
/// ```
#[derive(Debug, Clone, thiserror::Error)]
pub enum FoundationError {
    /// An HwpUnit value was outside the valid range.
    #[error("invalid HwpUnit value {value}: must be in [{min}, {max}]")]
    InvalidHwpUnit {
        /// The rejected value (as i64 to avoid truncation in error messages).
        value: i64,
        /// Minimum allowed value.
        min: i32,
        /// Maximum allowed value.
        max: i32,
    },

    /// A Color value or component was invalid.
    #[error("invalid color {component}: value {value}")]
    InvalidColor {
        /// Which component failed (e.g. "red", "raw").
        component: String,
        /// The rejected value.
        value: String,
    },

    /// A branded index exceeded the collection bounds.
    #[error("index out of bounds: {type_name}[{index}] but max is {max}")]
    IndexOutOfBounds {
        /// The rejected index value.
        index: usize,
        /// The upper bound (exclusive).
        max: usize,
        /// The phantom type name for diagnostics.
        type_name: &'static str,
    },

    /// A string identifier was empty.
    #[error("{item} must not be empty")]
    EmptyIdentifier {
        /// What kind of identifier (e.g. "FontId", "TemplateName").
        item: String,
    },

    /// A generic field validation failure.
    #[error("invalid field '{field}': {reason}")]
    InvalidField {
        /// The field that failed validation.
        field: String,
        /// Why validation failed.
        reason: String,
    },

    /// A string could not be parsed into the target enum.
    #[error("cannot parse '{value}' as {type_name}; valid values: {valid_values}")]
    ParseError {
        /// The target type name (e.g. "Alignment").
        type_name: String,
        /// The rejected input string.
        value: String,
        /// Comma-separated list of valid values.
        valid_values: String,
    },
}

impl ErrorCodeExt for FoundationError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidHwpUnit { .. } => ErrorCode::InvalidHwpUnit,
            Self::InvalidColor { .. } => ErrorCode::InvalidColor,
            Self::IndexOutOfBounds { .. } => ErrorCode::IndexOutOfBounds,
            Self::EmptyIdentifier { .. } => ErrorCode::EmptyIdentifier,
            Self::InvalidField { .. } => ErrorCode::InvalidField,
            Self::ParseError { .. } => ErrorCode::ParseError,
        }
    }
}

/// Convenience type alias for Foundation operations.
pub type FoundationResult<T> = Result<T, FoundationError>;

#[cfg(test)]
mod tests {
    use super::*;

    // === Edge Case 1: All variants constructible ===

    #[test]
    fn error_invalid_hwpunit_displays_value_and_range() {
        let err = FoundationError::InvalidHwpUnit {
            value: 999_999_999,
            min: -100_000_000,
            max: 100_000_000,
        };
        let msg = err.to_string();
        assert!(msg.contains("999999999"), "should contain value: {msg}");
        assert!(msg.contains("-100000000"), "should contain min: {msg}");
        assert!(msg.contains("100000000"), "should contain max: {msg}");
    }

    #[test]
    fn error_invalid_color_displays_component_and_value() {
        let err = FoundationError::InvalidColor {
            component: "red".to_string(),
            value: "256".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("red"), "should contain component: {msg}");
        assert!(msg.contains("256"), "should contain value: {msg}");
    }

    #[test]
    fn error_index_out_of_bounds_displays_context() {
        let err = FoundationError::IndexOutOfBounds { index: 42, max: 10, type_name: "CharShape" };
        let msg = err.to_string();
        assert!(msg.contains("42"), "should contain index: {msg}");
        assert!(msg.contains("10"), "should contain max: {msg}");
        assert!(msg.contains("CharShape"), "should contain type name: {msg}");
    }

    #[test]
    fn error_empty_identifier_displays_item() {
        let err = FoundationError::EmptyIdentifier { item: "FontId".to_string() };
        let msg = err.to_string();
        assert!(msg.contains("FontId"), "should contain item: {msg}");
        assert!(msg.contains("empty"), "should mention empty: {msg}");
    }

    #[test]
    fn error_invalid_field_displays_field_and_reason() {
        let err = FoundationError::InvalidField {
            field: "width".to_string(),
            reason: "must be positive".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("width"), "should contain field: {msg}");
        assert!(msg.contains("must be positive"), "should contain reason: {msg}");
    }

    #[test]
    fn error_parse_error_displays_type_value_and_valid() {
        let err = FoundationError::ParseError {
            type_name: "Alignment".to_string(),
            value: "leftt".to_string(),
            valid_values: "Left, Center, Right, Justify".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Alignment"), "should contain type: {msg}");
        assert!(msg.contains("leftt"), "should contain value: {msg}");
        assert!(msg.contains("Left"), "should contain valid values: {msg}");
    }

    // === Edge Case 2: ErrorCode numeric values ===

    #[test]
    fn error_codes_in_foundation_range() {
        assert_eq!(ErrorCode::InvalidHwpUnit as u32, 1000);
        assert_eq!(ErrorCode::InvalidColor as u32, 1001);
        assert_eq!(ErrorCode::IndexOutOfBounds as u32, 1002);
        assert_eq!(ErrorCode::EmptyIdentifier as u32, 1003);
        assert_eq!(ErrorCode::InvalidField as u32, 1004);
        assert_eq!(ErrorCode::ParseError as u32, 1005);
    }

    // === Edge Case 3: ErrorCode Display as E#### ===

    #[test]
    fn error_code_display_format() {
        assert_eq!(ErrorCode::InvalidHwpUnit.to_string(), "E1000");
        assert_eq!(ErrorCode::ParseError.to_string(), "E1005");
    }

    // === Edge Case 4: ErrorCodeExt mapping ===

    #[test]
    fn error_code_ext_maps_all_variants() {
        let cases: Vec<(FoundationError, ErrorCode)> = vec![
            (
                FoundationError::InvalidHwpUnit { value: 0, min: 0, max: 0 },
                ErrorCode::InvalidHwpUnit,
            ),
            (
                FoundationError::InvalidColor { component: String::new(), value: String::new() },
                ErrorCode::InvalidColor,
            ),
            (
                FoundationError::IndexOutOfBounds { index: 0, max: 0, type_name: "" },
                ErrorCode::IndexOutOfBounds,
            ),
            (FoundationError::EmptyIdentifier { item: String::new() }, ErrorCode::EmptyIdentifier),
            (
                FoundationError::InvalidField { field: String::new(), reason: String::new() },
                ErrorCode::InvalidField,
            ),
            (
                FoundationError::ParseError {
                    type_name: String::new(),
                    value: String::new(),
                    valid_values: String::new(),
                },
                ErrorCode::ParseError,
            ),
        ];
        for (err, expected_code) in cases {
            assert_eq!(err.code(), expected_code, "mismatch for {err:?}");
        }
    }

    // === Edge Case 5: Error is Send + Sync ===

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<FoundationError>();
        assert_sync::<FoundationError>();
    }

    // === Edge Case 6: Error implements std::error::Error ===

    #[test]
    fn error_implements_std_error() {
        let err = FoundationError::InvalidHwpUnit { value: 0, min: -1, max: 1 };
        let _: &dyn std::error::Error = &err;
    }

    // === Edge Case 7: ErrorCode derives Clone, Copy, Hash ===

    #[test]
    fn error_code_is_copy_and_hashable() {
        use std::collections::HashSet;
        let code = ErrorCode::InvalidHwpUnit;
        let code2 = code; // Copy
        assert_eq!(code, code2);

        let mut set = HashSet::new();
        set.insert(ErrorCode::InvalidHwpUnit);
        set.insert(ErrorCode::InvalidColor);
        assert_eq!(set.len(), 2);
    }

    // === Edge Case 8: Empty string fields in error ===

    #[test]
    fn error_handles_empty_string_fields_gracefully() {
        let err = FoundationError::ParseError {
            type_name: String::new(),
            value: String::new(),
            valid_values: String::new(),
        };
        // Should not panic
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }

    // === Edge Case 9: Very long strings in error ===

    #[test]
    fn error_handles_long_strings() {
        let long = "x".repeat(10_000);
        let err = FoundationError::InvalidField { field: long.clone(), reason: long.clone() };
        let msg = err.to_string();
        assert!(msg.len() > 10_000);
    }

    // === Edge Case 10: FoundationResult alias works ===

    #[test]
    fn foundation_result_alias_compiles() {
        fn example() -> FoundationResult<i32> {
            Ok(42)
        }
        assert_eq!(example().unwrap(), 42);
    }
}
