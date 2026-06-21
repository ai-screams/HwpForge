//! Shared output types for MCP tool responses.

use std::path::Path;

use serde::Serialize;

/// Maximum file size: 100 MB.
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
/// Maximum inline content size: 50 MB.
pub const MAX_INLINE_SIZE: usize = 50 * 1024 * 1024;

/// Read a file as bytes with size check and structured errors.
///
/// Uses `metadata()` for size guard (prevents OOM), then `read()` with
/// `ErrorKind`-based error mapping — no separate `exists()` call (TOCTOU-safe).
pub fn read_file_bytes(file_path: &str) -> Result<Vec<u8>, ToolErrorInfo> {
    let path = Path::new(file_path);
    check_file_size(path)?;
    // Safety net: if the file disappears between metadata() and read() (TOCTOU),
    // this match catches NotFound again. In normal flow, check_file_size handles it.
    std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("File not found: {file_path}"),
            "Check the file path and try again.",
        ),
        _ => ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read file: {e}"),
            "Check file permissions.",
        ),
    })
}

/// Read a file as a UTF-8 string with size check and structured errors.
///
/// Delegates to [`read_file_bytes`] for I/O, then validates UTF-8.
pub fn read_file_string(file_path: &str) -> Result<String, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    String::from_utf8(bytes).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("File is not valid UTF-8: {e}"),
            "Ensure the file is UTF-8 encoded.",
        )
    })
}

/// Write data to a file, creating parent directories as needed.
///
/// # Path safety
///
/// `output_path` originates from MCP tool arguments (i.e. the model). This
/// rejects any `..` (`ParentDir`) component — the relative path-traversal
/// vector (CWE-22) where a `..` segment escapes its intended directory.
///
/// Absolute paths are NOT rejected: some callers derive output locations from
/// an absolute input path (`to_md`) and MCP agents routinely pass absolute
/// paths, so confining writes to a root would break legitimate workflows.
///
/// RESIDUAL RISK (accepted, not yet mitigated): tools that pass a free-form,
/// model-supplied `output_path` (convert/patch/to_json/from_json/restyle) can
/// still write to any absolute path the process can write — blocking `..` does
/// not prevent that. Confining writes to an allowed root (or a sensitive-path
/// denylist) is tracked as a follow-up; see `.docs/planning` E1 audit.
pub fn write_output_file(output_path: &str, data: &[u8]) -> Result<(), ToolErrorInfo> {
    let out = Path::new(output_path);
    if out.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Unsafe output path contains '..': {output_path}"),
            "Provide an output path without any '..' parent-directory segments.",
        ));
    }
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolErrorInfo::new(
                    "WRITE_ERROR",
                    format!("Cannot create output directory: {e}"),
                    "Check write permissions.",
                )
            })?;
        }
    }
    std::fs::write(out, data).map_err(|e| {
        ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Failed to write file: {e}"),
            "Check disk space and permissions.",
        )
    })
}

/// Check file size against the maximum limit before reading.
///
/// Returns `FILE_NOT_FOUND` for missing files so callers don't need a
/// separate `exists()` check (eliminates TOCTOU window).
fn check_file_size(path: &Path) -> Result<(), ToolErrorInfo> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_SIZE => Err(ToolErrorInfo::new(
            "INPUT_TOO_LARGE",
            format!(
                "File '{}' is {} MB, exceeds {} MB limit",
                path.display(),
                m.len() / 1024 / 1024,
                MAX_FILE_SIZE / 1024 / 1024,
            ),
            "Use a smaller file or split the document into sections.",
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("File not found: '{}'", path.display()),
            "Check the file path and try again.",
        )),
        Err(e) => Err(ToolErrorInfo::new(
            "METADATA_ERROR",
            format!("Cannot read file metadata for '{}': {e}", path.display()),
            "Check file permissions.",
        )),
    }
}

/// 3-layer response structure for all MCP tools.
///
/// - `data`: machine-readable payload (paths, sizes, counts)
/// - `summary`: natural language summary for LLM to quote to user
/// - `next`: suggested next actions (LLM guidance)
#[derive(Debug, Serialize)]
pub struct ToolOutput<T: Serialize> {
    /// Machine-readable payload.
    pub data: T,
    /// Natural language summary for LLM.
    pub summary: String,
    /// Suggested next actions.
    pub next: Vec<String>,
}

impl<T: Serialize> ToolOutput<T> {
    /// Create a new tool output with data, summary, and suggested next actions.
    pub fn new(data: T, summary: impl Into<String>, next: Vec<&str>) -> Self {
        Self { data, summary: summary.into(), next: next.into_iter().map(String::from).collect() }
    }

    /// Serialize to JSON string for MCP `CallToolResult` content.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }
}

/// Structured error with actionable hint for LLM recovery.
#[derive(Debug, Serialize)]
pub struct ToolErrorInfo {
    /// Machine-readable error code (e.g., `FILE_NOT_FOUND`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Actionable hint for recovery.
    pub hint: String,
}

impl ToolErrorInfo {
    /// Create a new structured error.
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self { code: code.into(), message: message.into(), hint: hint.into() }
    }

    /// Serialize to JSON string for MCP error responses.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("Error: {}", self.message))
    }
}

/// Structured non-fatal warning for MCP tool responses.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ToolWarningInfo {
    /// Machine-readable warning code.
    pub code: String,
    /// Human-readable warning message.
    pub message: String,
    /// Optional actionable hint for the caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl ToolWarningInfo {
    /// Create a new structured warning.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), hint: None }
    }

    /// Add an optional hint to the warning.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_bytes_missing_file() {
        let err = read_file_bytes("/nonexistent/path.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn read_file_string_missing_file() {
        let err = read_file_string("/nonexistent/path.md").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn read_file_string_non_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.dat");
        std::fs::write(&path, [0xFF, 0xFE, 0x00, 0x80]).unwrap();

        let err = read_file_string(path.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "READ_ERROR");
        assert!(err.message.contains("UTF-8"));
    }

    #[test]
    fn read_file_bytes_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, b"hello").unwrap();

        let bytes = read_file_bytes(path.to_str().unwrap()).unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn read_file_string_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "한글 텍스트").unwrap();

        let content = read_file_string(path.to_str().unwrap()).unwrap();
        assert_eq!(content, "한글 텍스트");
    }

    #[test]
    fn write_output_file_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c/output.hwpx");

        write_output_file(path.to_str().unwrap(), b"data").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
    }

    #[test]
    fn write_output_file_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.hwpx");
        std::fs::write(&path, b"old").unwrap();

        write_output_file(path.to_str().unwrap(), b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn write_output_file_rejects_parent_dir_traversal() {
        // A relative path containing '..' must be rejected (CWE-22) and no file
        // may be written.
        let dir = tempfile::tempdir().unwrap();
        // Build a guaranteed-nonexistent escape target so we can assert it is
        // not created. Use a path that *contains* a '..' component.
        let escape = dir.path().join("sub").join("..").join("escape.txt");
        let escape_str = escape.to_str().unwrap();

        let err = write_output_file(escape_str, b"x").unwrap_err();
        assert_eq!(err.code, "WRITE_ERROR");
        assert!(err.message.contains(".."), "error must mention the '..' rejection");

        // The normalized escape target must not exist.
        let normalized = dir.path().join("escape.txt");
        assert!(!normalized.exists(), "traversal target must not be written");
    }

    #[test]
    fn write_output_file_rejects_relative_parent_dir() {
        // A purely relative '../escape' path must be rejected without writing.
        let err = write_output_file("../escape-e1-test.txt", b"x").unwrap_err();
        assert_eq!(err.code, "WRITE_ERROR");
        assert!(!Path::new("../escape-e1-test.txt").exists(), "must not write outside cwd");
    }

    #[test]
    fn write_output_file_allows_absolute_path() {
        // Absolute paths remain legitimate (e.g. `to_md` writes next to an
        // absolute input file). This is a regression guard for the #4 decision
        // to reject only '..' traversal, not absolute paths.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abs-out.hwpx");
        assert!(path.is_absolute());

        write_output_file(path.to_str().unwrap(), b"data").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
    }
}
