//! Error types for the HWPX decoder.
//!
//! Error codes occupy the 4000-4099 range, consistent with the
//! Foundation (1000), Core (2000), and Blueprint (3000) convention.

use std::fmt;

/// Top-level error type for HWPX decoding operations.
///
/// Every fallible operation in smithy-hwpx returns `Result<T, HwpxError>`.
/// Both [`hwpforge_core::CoreError`] and
/// [`hwpforge_foundation::FoundationError`] convert via `#[from]`.
///
/// # Examples
///
/// ```
/// use hwpforge_smithy_hwpx::HwpxError;
///
/// let err = HwpxError::MissingFile {
///     path: "Contents/header.xml".into(),
/// };
/// assert!(err.to_string().contains("header.xml"));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HwpxError {
    /// ZIP archive could not be read or is corrupt.
    #[error("ZIP error: {0}")]
    Zip(String),

    /// The `mimetype` entry has an unexpected value.
    #[error("Invalid HWPX mimetype: expected 'application/hwp+zip', got '{actual}'")]
    InvalidMimetype {
        /// The value found in the archive.
        actual: String,
    },

    /// A required file is missing from the ZIP archive.
    #[error("Missing required file in HWPX archive: '{path}'")]
    MissingFile {
        /// The expected path inside the archive.
        path: String,
    },

    /// XML could not be deserialized.
    #[error("XML parse error in '{file}': {detail}")]
    XmlParse {
        /// Which file inside the archive failed.
        file: String,
        /// The underlying parse error message.
        detail: String,
    },

    /// An attribute value could not be converted.
    #[error("Invalid attribute '{attribute}' on <{element}>: '{value}'")]
    InvalidAttribute {
        /// The XML element name.
        element: String,
        /// The attribute name.
        attribute: String,
        /// The rejected value.
        value: String,
    },

    /// A style index reference exceeds the header's definition count.
    #[error("{kind} index {index} out of bounds (max: {max})")]
    IndexOutOfBounds {
        /// What kind of index (e.g. "charPrIDRef", "paraPrIDRef").
        kind: &'static str,
        /// The rejected index value.
        index: u32,
        /// The upper bound (exclusive).
        max: u32,
    },

    /// Structural issue in the HWPX content.
    #[error("Invalid HWPX structure: {detail}")]
    InvalidStructure {
        /// What went wrong.
        detail: String,
    },

    /// An I/O error occurred (e.g. reading a file from disk).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A Core-layer error propagated upward.
    #[error("Core error: {0}")]
    Core(#[from] hwpforge_core::CoreError),

    /// A Foundation-layer error propagated upward.
    #[error("Foundation error: {0}")]
    Foundation(#[from] hwpforge_foundation::FoundationError),

    /// XML serialization failure (encoder).
    #[error("XML serialization error: {detail}")]
    XmlSerialize {
        /// The serialization error message.
        detail: String,
    },
}

/// Error codes for smithy-hwpx (4000-4099 range).
///
/// These follow the same convention as Foundation (1000-1099),
/// Core (2000-2099), and Blueprint (3000-3099).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HwpxErrorCode {
    /// Generic ZIP failure.
    Zip = 4000,
    /// Invalid mimetype in archive.
    InvalidMimetype = 4001,
    /// Required file missing from archive.
    MissingFile = 4002,
    /// XML deserialization failure.
    XmlParse = 4003,
    /// Bad attribute value during conversion.
    InvalidAttribute = 4004,
    /// Style index reference out of range.
    IndexOutOfBounds = 4005,
    /// Structural issue.
    InvalidStructure = 4006,
    /// I/O failure.
    Io = 4007,
    /// Propagated Core error.
    Core = 4008,
    /// Propagated Foundation error.
    Foundation = 4009,
    /// XML serialization failure.
    XmlSerialize = 4010,
}

impl fmt::Display for HwpxErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{}", *self as u16)
    }
}

impl HwpxError {
    /// Returns the corresponding error code.
    pub fn code(&self) -> HwpxErrorCode {
        match self {
            Self::Zip(_) => HwpxErrorCode::Zip,
            Self::InvalidMimetype { .. } => HwpxErrorCode::InvalidMimetype,
            Self::MissingFile { .. } => HwpxErrorCode::MissingFile,
            Self::XmlParse { .. } => HwpxErrorCode::XmlParse,
            Self::InvalidAttribute { .. } => HwpxErrorCode::InvalidAttribute,
            Self::IndexOutOfBounds { .. } => HwpxErrorCode::IndexOutOfBounds,
            Self::InvalidStructure { .. } => HwpxErrorCode::InvalidStructure,
            Self::Io(_) => HwpxErrorCode::Io,
            Self::Core(_) => HwpxErrorCode::Core,
            Self::Foundation(_) => HwpxErrorCode::Foundation,
            Self::XmlSerialize { .. } => HwpxErrorCode::XmlSerialize,
        }
    }
}

/// Convenience alias used throughout this crate.
pub type HwpxResult<T> = Result<T, HwpxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_error_display() {
        let err = HwpxError::Zip("corrupt archive".into());
        assert_eq!(err.to_string(), "ZIP error: corrupt archive");
        assert_eq!(err.code(), HwpxErrorCode::Zip);
    }

    #[test]
    fn invalid_mimetype_display() {
        let err = HwpxError::InvalidMimetype { actual: "application/zip".into() };
        let msg = err.to_string();
        assert!(msg.contains("application/hwp+zip"));
        assert!(msg.contains("application/zip"));
        assert_eq!(err.code(), HwpxErrorCode::InvalidMimetype);
    }

    #[test]
    fn missing_file_display() {
        let err = HwpxError::MissingFile { path: "Contents/header.xml".into() };
        assert!(err.to_string().contains("header.xml"));
        assert_eq!(err.code(), HwpxErrorCode::MissingFile);
    }

    #[test]
    fn xml_parse_display() {
        let err = HwpxError::XmlParse {
            file: "Contents/section0.xml".into(),
            detail: "unexpected element 'foo'".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("section0.xml"));
        assert!(msg.contains("unexpected element"));
        assert_eq!(err.code(), HwpxErrorCode::XmlParse);
    }

    #[test]
    fn invalid_attribute_display() {
        let err = HwpxError::InvalidAttribute {
            element: "hh:charPr".into(),
            attribute: "height".into(),
            value: "abc".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("hh:charPr"));
        assert!(msg.contains("height"));
        assert!(msg.contains("abc"));
        assert_eq!(err.code(), HwpxErrorCode::InvalidAttribute);
    }

    #[test]
    fn index_out_of_bounds_display() {
        let err = HwpxError::IndexOutOfBounds { kind: "charPrIDRef", index: 99, max: 5 };
        let msg = err.to_string();
        assert!(msg.contains("charPrIDRef"));
        assert!(msg.contains("99"));
        assert!(msg.contains("5"));
        assert_eq!(err.code(), HwpxErrorCode::IndexOutOfBounds);
    }

    #[test]
    fn invalid_structure_display() {
        let err = HwpxError::InvalidStructure { detail: "section has no paragraphs".into() };
        assert!(err.to_string().contains("no paragraphs"));
        assert_eq!(err.code(), HwpxErrorCode::InvalidStructure);
    }

    #[test]
    fn io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: HwpxError = io_err.into();
        assert_eq!(err.code(), HwpxErrorCode::Io);
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn error_code_display() {
        assert_eq!(HwpxErrorCode::Zip.to_string(), "E4000");
        assert_eq!(HwpxErrorCode::InvalidMimetype.to_string(), "E4001");
        assert_eq!(HwpxErrorCode::MissingFile.to_string(), "E4002");
        assert_eq!(HwpxErrorCode::XmlParse.to_string(), "E4003");
        assert_eq!(HwpxErrorCode::InvalidAttribute.to_string(), "E4004");
        assert_eq!(HwpxErrorCode::IndexOutOfBounds.to_string(), "E4005");
        assert_eq!(HwpxErrorCode::InvalidStructure.to_string(), "E4006");
        assert_eq!(HwpxErrorCode::Io.to_string(), "E4007");
        assert_eq!(HwpxErrorCode::Core.to_string(), "E4008");
        assert_eq!(HwpxErrorCode::Foundation.to_string(), "E4009");
        assert_eq!(HwpxErrorCode::XmlSerialize.to_string(), "E4010");
    }

    #[test]
    fn xml_serialize_error_display() {
        let err = HwpxError::XmlSerialize { detail: "missing field".into() };
        assert!(err.to_string().contains("missing field"));
        assert_eq!(err.code(), HwpxErrorCode::XmlSerialize);
    }

    #[test]
    fn error_codes_are_in_4000_range() {
        let codes = [
            HwpxErrorCode::Zip,
            HwpxErrorCode::InvalidMimetype,
            HwpxErrorCode::MissingFile,
            HwpxErrorCode::XmlParse,
            HwpxErrorCode::InvalidAttribute,
            HwpxErrorCode::IndexOutOfBounds,
            HwpxErrorCode::InvalidStructure,
            HwpxErrorCode::Io,
            HwpxErrorCode::Core,
            HwpxErrorCode::Foundation,
            HwpxErrorCode::XmlSerialize,
        ];
        for code in codes {
            let val = code as u16;
            assert!((4000..4100).contains(&val), "code {val} not in 4000-4099");
        }
    }

    #[test]
    fn hwpx_result_type_alias_works() {
        fn example() -> HwpxResult<u32> {
            Ok(42)
        }
        assert_eq!(example().unwrap(), 42);
    }

    #[test]
    fn hwpx_result_err_path() {
        fn example() -> HwpxResult<u32> {
            Err(HwpxError::Zip("test".into()))
        }
        assert!(example().is_err());
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<HwpxError>();
        assert_sync::<HwpxError>();
    }

    #[test]
    fn foundation_error_conversion() {
        let fe = hwpforge_foundation::FoundationError::EmptyIdentifier { item: "FontId".into() };
        let err: HwpxError = fe.into();
        assert_eq!(err.code(), HwpxErrorCode::Foundation);
    }
}
