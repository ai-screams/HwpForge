//! Error types for the HWP5 decoder.
//!
//! Error codes occupy the 5000-5099 range, consistent with the
//! Foundation (1000), Core (2000), Blueprint (3000), and HWPX (4000) conventions.

use std::fmt;

/// Top-level error type for HWP5 decoding operations.
///
/// Every fallible operation in smithy-hwp5 returns `Result<T, Hwp5Error>`.
///
/// # Examples
///
/// ```
/// use hwpforge_smithy_hwp5::Hwp5Error;
///
/// let err = Hwp5Error::NotHwp5 { detail: "missing HWP signature".into() };
/// assert!(err.to_string().contains("HWP5"));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Hwp5Error {
    /// The file is not a valid HWP5 document.
    #[error("Not a valid HWP5 file: {detail}")]
    NotHwp5 {
        /// Description of why the file was rejected.
        detail: String,
    },

    /// The OLE2 compound file could not be opened or read.
    #[error("OLE2/CFB error: {detail}")]
    Cfb {
        /// The underlying error message.
        detail: String,
    },

    /// A required OLE2 stream is missing from the compound file.
    #[error("Missing required HWP5 stream: '{name}'")]
    MissingStream {
        /// The stream name that was expected.
        name: String,
    },

    /// A binary record could not be parsed.
    #[error("Record parse error at offset {offset}: {detail}")]
    RecordParse {
        /// Byte offset where the error occurred.
        offset: usize,
        /// Description of the parse failure.
        detail: String,
    },

    /// The HWP5 version is not supported.
    #[error("Unsupported HWP5 version: {major}.{minor}.{micro}.{build}")]
    UnsupportedVersion {
        /// Major version number.
        major: u8,
        /// Minor version number.
        minor: u8,
        /// Micro version number.
        micro: u8,
        /// Build version number.
        build: u8,
    },

    /// The document is password-protected and cannot be decoded.
    #[error("HWP5 document is password-protected")]
    PasswordProtected,

    /// A text encoding conversion failed.
    #[error("Text encoding error: {detail}")]
    Encoding {
        /// Description of the encoding failure.
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
}

/// Error codes for smithy-hwp5 (5000-5099 range).
///
/// These follow the same convention as Foundation (1000-1099),
/// Core (2000-2099), Blueprint (3000-3099), and HWPX (4000-4099).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Hwp5ErrorCode {
    /// File is not a valid HWP5 document.
    NotHwp5 = 5000,
    /// OLE2/CFB container error.
    Cfb = 5001,
    /// Required stream is missing.
    MissingStream = 5002,
    /// Binary record parse failure.
    RecordParse = 5003,
    /// Unsupported HWP5 version.
    UnsupportedVersion = 5004,
    /// Document is password-protected.
    PasswordProtected = 5005,
    /// Text encoding failure.
    Encoding = 5006,
    /// I/O failure.
    Io = 5007,
    /// Propagated Core error.
    Core = 5008,
    /// Propagated Foundation error.
    Foundation = 5009,
}

impl fmt::Display for Hwp5ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{}", *self as u16)
    }
}

impl Hwp5Error {
    /// Returns the corresponding error code.
    pub fn code(&self) -> Hwp5ErrorCode {
        match self {
            Self::NotHwp5 { .. } => Hwp5ErrorCode::NotHwp5,
            Self::Cfb { .. } => Hwp5ErrorCode::Cfb,
            Self::MissingStream { .. } => Hwp5ErrorCode::MissingStream,
            Self::RecordParse { .. } => Hwp5ErrorCode::RecordParse,
            Self::UnsupportedVersion { .. } => Hwp5ErrorCode::UnsupportedVersion,
            Self::PasswordProtected => Hwp5ErrorCode::PasswordProtected,
            Self::Encoding { .. } => Hwp5ErrorCode::Encoding,
            Self::Io(_) => Hwp5ErrorCode::Io,
            Self::Core(_) => Hwp5ErrorCode::Core,
            Self::Foundation(_) => Hwp5ErrorCode::Foundation,
        }
    }
}

/// Convenience alias used throughout this crate.
pub type Hwp5Result<T> = Result<T, Hwp5Error>;
