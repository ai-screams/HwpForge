//! CLI error types with JSON-friendly output.

use serde::Serialize;
use std::fmt;
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
}

impl CliError {
    /// Creates a new error with the given code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { status: "error", code: code.into(), message: message.into(), hint: None }
    }

    /// Adds a hint to this error.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
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
