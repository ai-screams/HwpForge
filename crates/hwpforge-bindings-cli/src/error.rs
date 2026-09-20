//! CLI error types with JSON-friendly output.

use serde::Serialize;
use std::fmt;
use std::io::Read;
use std::process;

/// Maximum file size: 100 MB.
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
/// Maximum stdin size: 50 MB.
pub const MAX_STDIN_SIZE: usize = 50 * 1024 * 1024;

/// Structured CLI error for both human and machine consumption.
#[derive(Debug, Serialize)]
pub struct CliError {
    /// Always `"error"`.
    pub status: &'static str,
    /// Machine-readable error code (e.g. `"FILE_READ_FAILED"`).
    pub code: String,
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
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: "error",
            code: code.into(),
            message: message.into(),
            hint: None,
            cause: None,
        }
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
    pub fn exit(self, json_mode: bool, exit_code: i32) -> ! {
        if json_mode {
            let _ = serde_json::to_writer(std::io::stderr(), &self);
            eprintln!();
        } else {
            eprintln!("Error [{}]: {}", self.code, self.message);
            if let Some(hint) = &self.hint {
                eprintln!("Hint: {hint}");
            }
        }
        process::exit(exit_code);
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Check file size against the maximum limit before reading.
pub fn check_file_size(path: &std::path::Path, json_mode: bool) {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_SIZE => {
            CliError::new(
                "INPUT_TOO_LARGE",
                format!(
                    "File '{}' is {} MB, exceeds {} MB limit",
                    path.display(),
                    m.len() / 1024 / 1024,
                    MAX_FILE_SIZE / 1024 / 1024
                ),
            )
            .exit(json_mode, 1);
        }
        _ => {} // File doesn't exist or is within limit — let the subsequent read handle missing files
    }
}

/// Reads `reader` fully, but never more than `max + 1` bytes.
///
/// Backs [`read_bounded`]/[`read_input`]: [`check_file_size`]'s
/// `metadata().len()` guard is only accurate for regular files — a FIFO or
/// process substitution reports `0` and would otherwise be read to
/// completion by a plain [`std::fs::read`] (measured: an unbounded read of
/// a 200 MB FIFO drove RSS to 2.8 GB in 20 s). Capping the read itself, not
/// just the pre-check, is the actual guarantee; `metadata()` is just the
/// cheap fast path for the common case where it happens to be accurate.
/// Generic over `impl Read` so unit tests can exercise the cap with a small
/// `max` against a `Cursor` instead of allocating real megabytes.
fn read_capped(mut reader: impl Read, max: u64) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    reader.by_ref().take(max + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            format!("input exceeds {} MB limit", max / 1024 / 1024),
        ));
    }
    Ok(buf)
}

/// Reads `path` into memory, bounded by [`MAX_FILE_SIZE`] regardless of what
/// `metadata()` reports (see [`read_capped`]).
///
/// A drop-in replacement for `std::fs::read(path)` at call sites that
/// already build their own [`CliError`] around the read result — their
/// existing code/message/exit shape stays untouched; only the previously
/// unbounded read underneath it is fixed. Exceeding the cap reports
/// [`std::io::ErrorKind::FileTooLarge`] so those call sites can still tell
/// it apart from a plain I/O failure if they choose to. Generic over
/// `impl AsRef<Path>`, matching `std::fs::read`'s own signature, so callers
/// that hold a `&PathBuf` keep passing it directly (no `clippy::ptr_arg`
/// pressure to widen their own parameter to `&Path` just to call this).
pub fn read_bounded(path: impl AsRef<std::path::Path>) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path.as_ref())?;
    read_capped(file, MAX_FILE_SIZE)
}

/// Reads `path` for the common CLI read-command contract: exits with
/// `FILE_READ_FAILED`/exit 1 on I/O failure (the same code/message shape
/// every sibling read command already used for a plain `std::fs::read`
/// failure), and `INPUT_TOO_LARGE`/exit 1 — the same code
/// [`check_file_size`] uses for the metadata-caught case — when
/// [`read_bounded`]'s cap catches a source whose `metadata()` lied (a FIFO
/// or process substitution).
pub fn read_input(path: impl AsRef<std::path::Path>, json_mode: bool) -> Vec<u8> {
    let path = path.as_ref();
    read_bounded(path).unwrap_or_else(|e| {
        if e.kind() == std::io::ErrorKind::FileTooLarge {
            CliError::new(
                "INPUT_TOO_LARGE",
                format!(
                    "File '{}' exceeds {} MB limit",
                    path.display(),
                    MAX_FILE_SIZE / 1024 / 1024
                ),
            )
            .exit(json_mode, 1)
        } else {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", path.display()))
                .exit(json_mode, 1)
        }
    })
}

/// Reads `reader` fully as UTF-8, but never more than `max + 1` bytes.
///
/// The `read_to_string` twin of [`read_capped`]: bounds the read the same
/// way, then validates UTF-8 with the exact message
/// `std::io::Read::read_to_string` itself uses ("stream did not contain
/// valid UTF-8") — so a non-UTF-8 file reports identically to before this
/// guard existed, whether the size cap ever comes into play or not.
fn read_capped_string(reader: impl Read, max: u64) -> std::io::Result<String> {
    let bytes = read_capped(reader, max)?;
    String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "stream did not contain valid UTF-8")
    })
}

/// Reads `path` as a UTF-8 string, bounded by [`MAX_FILE_SIZE`] regardless
/// of what `metadata()` reports (see [`read_capped_string`]).
///
/// The `std::fs::read_to_string(path)` twin of [`read_bounded`] — a drop-in
/// replacement at call sites that already build their own [`CliError`]
/// around the read result, for the same reason [`read_bounded`] exists: a
/// `read_to_string` this size guard doesn't touch is exactly as exploitable
/// via a FIFO/process substitution as `std::fs::read` was.
pub fn read_bounded_string(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    let file = std::fs::File::open(path.as_ref())?;
    read_capped_string(file, MAX_FILE_SIZE)
}

/// Reads `path` as a UTF-8 string for the common CLI read-command contract
/// — the `read_to_string` twin of [`read_input`]: `FILE_READ_FAILED`/exit 1
/// on I/O or non-UTF-8 failure, `INPUT_TOO_LARGE`/exit 1 when
/// [`read_bounded_string`]'s cap catches a source whose `metadata()` lied.
pub fn read_input_string(path: impl AsRef<std::path::Path>, json_mode: bool) -> String {
    let path = path.as_ref();
    read_bounded_string(path).unwrap_or_else(|e| {
        if e.kind() == std::io::ErrorKind::FileTooLarge {
            CliError::new(
                "INPUT_TOO_LARGE",
                format!(
                    "File '{}' exceeds {} MB limit",
                    path.display(),
                    MAX_FILE_SIZE / 1024 / 1024
                ),
            )
            .exit(json_mode, 1)
        } else {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", path.display()))
                .exit(json_mode, 1)
        }
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
    fn read_capped_over_limit_errors_file_too_large() {
        // 11 bytes through a 10-byte cap: `take(max + 1)` lets the 11th
        // byte through so the length check can distinguish "exactly at the
        // limit" from "over it" — this exercises that off-by-one boundary.
        let data = vec![7u8; 11];
        let err = read_capped(Cursor::new(data), 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn read_bounded_missing_file_is_not_found() {
        let err =
            read_bounded(std::path::Path::new("/nonexistent/hwpforge-w6a-probe.hwpx")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
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
    fn read_capped_string_over_limit_errors_file_too_large() {
        // Same off-by-one boundary as `read_capped_over_limit_errors_file_too_large`,
        // through the UTF-8-validating twin.
        let data = vec![b'a'; 11];
        let err = read_capped_string(Cursor::new(data), 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn read_capped_string_non_utf8_reports_the_std_message() {
        let err = read_capped_string(Cursor::new(vec![0xFF, 0xFE]), 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "stream did not contain valid UTF-8");
    }

    #[test]
    fn read_bounded_string_missing_file_is_not_found() {
        let err = read_bounded_string(std::path::Path::new("/nonexistent/hwpforge-w6a-probe2.txt"))
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
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
