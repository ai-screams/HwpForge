//! Typed CLI errors and bounded input I/O. Every exit uses (command, code).
use crate::compat::Command;
use hwpforge::ops::OpsError;
use hwpforge_foundation::diagnostics::OpsCode;
use serde::Serialize;
use std::{fmt, io::Read, process};

pub use hwpforge::ops::fs::MAX_FILE_SIZE;
pub const MAX_STDIN_SIZE: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliCode {
    Ops(OpsCode),
    Cli(CliOnly),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliOnly {
    InvalidSet,
    DuplicateSet,
    Hwp5CensusFailed,
    Hwp5SemanticFailed,
    HwpxAnalysisFailed,
    HwpxCensusFailed,
}

impl CliCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ops(code) => code.as_str(),
            Self::Cli(CliOnly::InvalidSet) => "INVALID_SET",
            Self::Cli(CliOnly::DuplicateSet) => "DUPLICATE_SET",
            Self::Cli(CliOnly::Hwp5CensusFailed) => "HWP5_CENSUS_FAILED",
            Self::Cli(CliOnly::Hwp5SemanticFailed) => "HWP5_SEMANTIC_FAILED",
            Self::Cli(CliOnly::HwpxAnalysisFailed) => "HWPX_ANALYSIS_FAILED",
            Self::Cli(CliOnly::HwpxCensusFailed) => "HWPX_CENSUS_FAILED",
        }
    }
}

impl Serialize for CliCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for CliCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured CLI error for both human and machine consumption.
#[derive(Debug, Serialize)]
pub struct CliError {
    /// Always `"error"`.
    pub status: &'static str,
    /// Machine-readable error code (e.g. `"FILE_READ_FAILED"`).
    pub code: CliCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional hint for resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Optional machine-readable cause (top-level `code` stays the stable
    /// coarse contract; `cause` refines it without breaking consumers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<ErrorCause>,
}

/// Machine-readable refinement of a coarse [`CliError::code`].
#[derive(Debug, Serialize)]
pub struct ErrorCause {
    /// Pipeline stage that produced the failure (e.g. `"render"`).
    pub stage: &'static str,
    /// SCREAMING_SNAKE variant code within the stage.
    pub code: &'static str,
    /// Sub-kind when the variant carries one (e.g. `UnsupportedContent.kind`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Document-coordinate location when the variant carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

impl CliError {
    /// Creates a new error with the given code and message.
    pub fn new(code: CliCode, message: impl Into<String>) -> Self {
        Self { status: "error", code, message: message.into(), hint: None, cause: None }
    }

    /// Adds a hint to this error.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Adds a machine-readable cause to this error.
    pub fn with_cause(mut self, cause: ErrorCause) -> Self {
        self.cause = Some(cause);
        self
    }

    /// Print error and exit with given code.
    pub fn exit(self, json_mode: bool, command: Command) -> ! {
        if json_mode {
            let _ = serde_json::to_writer(std::io::stderr(), &self);
            eprintln!();
        } else {
            eprintln!("Error [{}]: {}", self.code, self.message);
            if let Some(hint) = &self.hint {
                eprintln!("Hint: {hint}");
            }
        }
        process::exit(crate::compat::exit_code(command, self.code));
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Cheap regular-file check; the bounded reader remains the authoritative cap.
pub fn check_file_size(path: &std::path::Path, json_mode: bool, command: Command) {
    if let Ok(m) = std::fs::metadata(path) {
        if m.len() > MAX_FILE_SIZE {
            CliError::new(
                CliCode::Ops(OpsCode::InputTooLarge),
                format!(
                    "File '{}' is {} MB, exceeds {} MB limit",
                    path.display(),
                    m.len() / 1024 / 1024,
                    MAX_FILE_SIZE / 1024 / 1024
                ),
            )
            .exit(json_mode, command);
        }
    }
}

fn read_capped(mut reader: impl Read, max: u64) -> Result<Vec<u8>, OpsError> {
    let mut buf = Vec::new();
    reader.by_ref().take(max.saturating_add(1)).read_to_end(&mut buf).map_err(OpsError::Io)?;
    if buf.len() as u64 > max {
        return Err(OpsError::Rejected {
            code: OpsCode::InputTooLarge,
            reason: format!("input exceeds {} MB limit", max / 1024 / 1024),
        });
    }
    Ok(buf)
}

pub fn read_bounded(path: impl AsRef<std::path::Path>) -> Result<Vec<u8>, OpsError> {
    hwpforge::ops::fs::read_bounded(path, MAX_FILE_SIZE)
}

pub fn read_input(path: impl AsRef<std::path::Path>, json_mode: bool, command: Command) -> Vec<u8> {
    read_bounded(path.as_ref()).unwrap_or_else(|err| {
        CliError::new(
            CliCode::Ops(err.code()),
            format!("Cannot read '{}': {err}", path.as_ref().display()),
        )
        .exit(json_mode, command)
    })
}

pub fn read_capped_string(reader: impl Read, max: u64) -> Result<String, OpsError> {
    utf8(read_capped(reader, max)?)
}

fn utf8(bytes: Vec<u8>) -> Result<String, OpsError> {
    String::from_utf8(bytes).map_err(|_| {
        OpsError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ))
    })
}

pub fn read_bounded_string(path: impl AsRef<std::path::Path>) -> Result<String, OpsError> {
    utf8(read_bounded(path)?)
}

pub fn read_input_string(
    path: impl AsRef<std::path::Path>,
    json_mode: bool,
    command: Command,
) -> String {
    read_bounded_string(path.as_ref()).unwrap_or_else(|err| {
        CliError::new(
            CliCode::Ops(err.code()),
            format!("Cannot read '{}': {err}", path.as_ref().display()),
        )
        .exit(json_mode, command)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_capped_within_limit_returns_all_bytes() {
        let data = vec![7u8; 5];
        let out = read_capped(Cursor::new(data.clone()), 10).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn read_capped_at_exact_limit_returns_all_bytes() {
        let data = vec![7u8; 10];
        let out = read_capped(Cursor::new(data.clone()), 10).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    // 이것을 실패시키는 것: bounded reader의 초과 guard 비활성화.
    fn read_capped_over_limit_errors_file_too_large() {
        // 11 bytes through a 10-byte cap: `take(max + 1)` lets the 11th
        // byte through so the length check can distinguish "exactly at the
        // limit" from "over it" — this exercises that off-by-one boundary.
        let data = vec![7u8; 11];
        let err = read_capped(Cursor::new(data), 10).unwrap_err();
        assert_eq!(err.code(), OpsCode::InputTooLarge);
    }

    #[test]
    // 이것을 실패시키는 것: NotFound→FileReadFailed; CLI bounded helper 시험으로 검출.
    fn read_bounded_missing_file_is_not_found() {
        let err =
            read_bounded(std::path::Path::new("/nonexistent/hwpforge-missing-input-probe.hwpx"))
                .unwrap_err();
        assert_eq!(err.code(), OpsCode::FileNotFound);
    }

    #[test]
    fn read_bounded_regular_file_within_limit() {
        let dir =
            std::env::temp_dir().join(format!("hwpforge_error_rs_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, b"hello").unwrap();

        assert_eq!(read_bounded(&path).unwrap(), b"hello");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_capped_string_within_limit_returns_the_string() {
        let out = read_capped_string(Cursor::new(b"hello".to_vec()), 10).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    // 이것을 실패시키는 것: bounded reader의 초과 guard 비활성화.
    fn read_capped_string_over_limit_errors_file_too_large() {
        // Same off-by-one boundary as `read_capped_over_limit_errors_file_too_large`,
        // through the UTF-8-validating twin.
        let data = vec![b'a'; 11];
        let err = read_capped_string(Cursor::new(data), 10).unwrap_err();
        assert_eq!(err.code(), OpsCode::InputTooLarge);
    }

    #[test]
    // 이것을 실패시키는 것: UTF-8 InvalidData를 NotFound로 변경.
    fn read_capped_string_non_utf8_reports_the_std_message() {
        let err = read_capped_string(Cursor::new(vec![0xFF, 0xFE]), 10).unwrap_err();
        assert_eq!(err.code(), OpsCode::FileReadFailed);
        assert_eq!(err.to_string(), "stream did not contain valid UTF-8");
    }

    #[test]
    // 이것을 실패시키는 것: NotFound→FileReadFailed; CLI bounded helper 시험으로 검출.
    fn read_bounded_string_missing_file_is_not_found() {
        let err = read_bounded_string(std::path::Path::new(
            "/nonexistent/hwpforge-missing-input-probe2.txt",
        ))
        .unwrap_err();
        assert_eq!(err.code(), OpsCode::FileNotFound);
    }

    #[test]
    fn read_bounded_string_regular_file_within_limit() {
        let dir = std::env::temp_dir()
            .join(format!("hwpforge_error_rs_string_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello").unwrap();

        assert_eq!(read_bounded_string(&path).unwrap(), "hello");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod input_error_tests {
    use super::*;

    #[test]
    // 이것을 실패시키는 것: bounded reader의 초과 guard 비활성화, bounded reader의 >를 >=로 변경, reader I/O를 InputTooLarge로 오분류.
    fn stdin_cap_and_io_classification() {
        // 이것을 실패시키는 것: cap의 >를 >=로 변경, cap 제거, reader I/O를 InputTooLarge로 변경.
        assert_eq!(read_capped_string(&b"abcd"[..], 4).unwrap(), "abcd");
        assert_eq!(
            read_capped_string(&b"abcde"[..], 4).unwrap_err().code(),
            OpsCode::InputTooLarge
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            }
        }
        assert_eq!(read_capped_string(Broken, 4).unwrap_err().code(), OpsCode::FileReadFailed);
    }
}
