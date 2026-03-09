//! Shared output types for MCP tool responses.

use std::path::Path;

use serde::Serialize;

/// Maximum file size: 100 MB.
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
/// Maximum inline content size: 50 MB.
pub const MAX_INLINE_SIZE: usize = 50 * 1024 * 1024;

/// Check file size against the maximum limit before reading.
pub fn check_file_size(path: &Path) -> Result<(), ToolErrorInfo> {
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
        _ => Ok(()),
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
