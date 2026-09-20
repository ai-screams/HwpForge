//! Shared output types for MCP tool responses.

use std::path::Path;

use serde::Serialize;

/// Maximum file size: 100 MB.
///
/// W6b audit follow-up: re-exported from the shared operation layer
/// (`hwpforge::ops::fs::MAX_FILE_SIZE`) rather than declared here, so this
/// crate, the CLI and the Python bindings all read the one constant instead
/// of three copies of the same literal.
pub use hwpforge::ops::fs::MAX_FILE_SIZE;
/// Maximum inline content size: 50 MB.
pub const MAX_INLINE_SIZE: usize = 50 * 1024 * 1024;

/// Read a file as bytes with size check and structured errors.
///
/// Uses `metadata()` for size guard (prevents OOM in the common case), then
/// reads through [`hwpforge::ops::fs::read_bounded`] — a hard cap of
/// [`MAX_FILE_SIZE`] bytes enforced on the read itself, not just on
/// `metadata()` (W6b audit follow-up: this is the one bounded reader every
/// frontend shares) — with `ErrorKind`-based error mapping. No separate
/// `exists()` call (TOCTOU-safe).
pub fn read_file_bytes(file_path: &str) -> Result<Vec<u8>, ToolErrorInfo> {
    let path = Path::new(file_path);
    check_file_size(path)?;
    // Safety net: if the file disappears between metadata() and read()
    // (TOCTOU), this match catches NotFound again. In normal flow,
    // check_file_size handles it. Also the guarantee for a source
    // (FIFO/process substitution) whose `metadata().len()` under-reports:
    // `read_bounded` still caps it here.
    hwpforge::ops::fs::read_bounded(path, MAX_FILE_SIZE).map_err(|e| match e {
        hwpforge::ops::OpsError::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => ToolErrorInfo::new(
                "FILE_NOT_FOUND",
                format!("File not found: {file_path}"),
                "Check the file path and try again.",
            ),
            _ => ToolErrorInfo::new(
                "READ_ERROR",
                format!("Failed to read file: {io_err}"),
                "Check file permissions.",
            ),
        },
        hwpforge::ops::OpsError::Rejected {
            code: hwpforge_foundation::diagnostics::OpsCode::InputTooLarge,
            ..
        } => ToolErrorInfo::new(
            "INPUT_TOO_LARGE",
            format!("File '{file_path}' exceeds {} MB limit", MAX_FILE_SIZE / 1024 / 1024),
            "Use a smaller file or split the document into sections.",
        ),
        other => ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read file: {other}"),
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

/// Resolve the user's home directory from the environment (`HOME`, then
/// `USERPROFILE` on Windows). Returns `None` when neither is set.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Whether `target` (an absolute path) points into a sensitive user location:
/// known credential/config directories under `$HOME`, or a dotfile directly in
/// `$HOME` (e.g. `~/.zshrc`). Component-wise `starts_with` means `~/.sshfoo`
/// does NOT match `~/.ssh`. Pure (no filesystem access) so it is unit-testable
/// with a synthetic `home`.
fn is_sensitive_output_path(target: &Path, home: &Path) -> bool {
    const SENSITIVE_SUBDIRS: &[&str] =
        &[".ssh", ".gnupg", ".aws", ".config", ".claude", ".codex", ".gemini"];
    if SENSITIVE_SUBDIRS.iter().any(|sub| target.starts_with(home.join(sub))) {
        return true;
    }
    // A dotfile written directly into $HOME (e.g. ~/.zshrc, ~/.bashrc).
    if target.parent() == Some(home) {
        if let Some(name) = target.file_name().and_then(|n| n.to_str()) {
            return name.starts_with('.');
        }
    }
    false
}

/// Write data to a file, creating parent directories as needed.
///
/// # Path safety
///
/// `output_path` originates from MCP tool arguments (i.e. the model), so this
/// applies defense-in-depth before writing:
/// 1. Reject any `..` (`ParentDir`) component — the relative path-traversal
///    vector (CWE-22).
/// 2. For absolute paths, reject writes into sensitive user locations
///    (`~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config`, `~/.claude`, …, and
///    dotfiles directly under `$HOME`) via [`is_sensitive_output_path`].
/// 3. Refuse to write when the final path component is a symlink (no-follow),
///    so a planted symlink cannot redirect the write elsewhere.
///
/// Non-sensitive absolute paths stay allowed: callers legitimately derive
/// output from absolute input paths (`to_md`) and agents write into e.g. the
/// user's Documents. Full allowed-root confinement (config-driven) is a
/// possible future tightening (see `.docs/planning` E1 audit).
pub fn write_output_file(output_path: &str, data: &[u8]) -> Result<(), ToolErrorInfo> {
    let out = Path::new(output_path);
    if out.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Unsafe output path contains '..': {output_path}"),
            "Provide an output path without any '..' parent-directory segments.",
        ));
    }
    if out.is_absolute() {
        if let Some(home) = home_dir() {
            if is_sensitive_output_path(out, &home) {
                return Err(ToolErrorInfo::new(
                    "WRITE_ERROR",
                    format!("Refusing to write to a sensitive location: {output_path}"),
                    "Choose an output path outside SSH/credential/config directories.",
                ));
            }
        }
    }
    if out.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Refusing to write through a symlink: {output_path}"),
            "Provide a real (non-symlink) output path.",
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
///
/// W6b audit follow-up: this is a structural (field-by-field) twin of
/// `hwpforge_foundation::diagnostics::WarningInfo` — same `{code, message,
/// hint}` shape, `hint` omitted the same way when absent. Kept as its own
/// type rather than replaced outright because every tool file's `warnings`
/// field is typed on it already (`fields.rs`, `fill.rs`, `stamp.rs`, …), well
/// outside this lane's scope; the `From<WarningInfo>` impl below is the one
/// bridge [`crate::compat::warning`] needs.
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

impl From<hwpforge_foundation::diagnostics::WarningInfo> for ToolWarningInfo {
    /// A field-by-field copy through this type's own constructor — the two
    /// types serialise identically, so this is the whole conversion.
    fn from(info: hwpforge_foundation::diagnostics::WarningInfo) -> Self {
        match info.hint {
            Some(hint) => Self::new(info.code, info.message).with_hint(hint),
            None => Self::new(info.code, info.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Cursor-based cap-boundary tests that used to live here (a small
    // `max` against `read_capped`, this file's own private reader) moved
    // with the reader itself to `hwpforge::ops::fs::tests` — W6b audit
    // follow-up: one bounded reader, one set of boundary tests, shared by
    // every frontend. `read_file_bytes_over_the_size_cap_reports_input_too_large`
    // below is this file's own regression: the *mapping* from the shared
    // reader's `OpsError` onto this crate's `ToolErrorInfo` shape.

    #[test]
    fn read_file_bytes_missing_file() {
        let err = read_file_bytes("/nonexistent/path.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[cfg(unix)]
    #[test]
    fn read_file_bytes_over_the_size_cap_reports_input_too_large() {
        // Mirrors the CLI's own FIFO regression
        // (`fifo_input_over_the_size_cap_reports_input_too_large`): a FIFO's
        // `metadata().len()` reports `0`, so `check_file_size` lets it
        // through, and only the shared `ops::fs::read_bounded`'s cap on the
        // read itself catches it.
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let fifo_path = dir.path().join("oversized.hwpx");
        let status = std::process::Command::new("mkfifo").arg(&fifo_path).status().unwrap();
        assert!(status.success(), "mkfifo failed for {}", fifo_path.display());

        let writer_path = fifo_path.clone();
        let writer = std::thread::spawn(move || {
            let Ok(mut f) = std::fs::File::create(&writer_path) else { return };
            let chunk = vec![0u8; 1024 * 1024];
            // Comfortably over the 100 MB cap; the reader's
            // `take(MAX_FILE_SIZE + 1)` stops consuming once it has the extra
            // byte it needs to detect the overflow, so `write_all` reports
            // `BrokenPipe` on the first write past that point.
            for _ in 0..150 {
                if f.write_all(&chunk).is_err() {
                    break;
                }
            }
        });

        let err = read_file_bytes(fifo_path.to_str().unwrap()).unwrap_err();
        writer.join().unwrap();

        assert_eq!(err.code, "INPUT_TOO_LARGE");
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

    #[test]
    fn is_sensitive_output_path_flags_credential_and_dotfile_targets() {
        let home = Path::new("/home/u");
        // Credential/config dirs under $HOME → sensitive.
        assert!(is_sensitive_output_path(Path::new("/home/u/.ssh/authorized_keys"), home));
        assert!(is_sensitive_output_path(Path::new("/home/u/.claude/settings.json"), home));
        assert!(is_sensitive_output_path(Path::new("/home/u/.config/x/y.toml"), home));
        // Dotfile directly in $HOME → sensitive.
        assert!(is_sensitive_output_path(Path::new("/home/u/.zshrc"), home));
        // Legitimate non-sensitive absolute outputs → allowed.
        assert!(!is_sensitive_output_path(Path::new("/home/u/Documents/out.hwpx"), home));
        assert!(!is_sensitive_output_path(Path::new("/home/u/proposals/p.hwpx"), home));
        assert!(!is_sensitive_output_path(Path::new("/tmp/out.hwpx"), home));
        // Component-wise: '.sshfoo' must NOT match '.ssh'.
        assert!(!is_sensitive_output_path(Path::new("/home/u/.sshfoo/x"), home));
    }

    #[test]
    fn write_output_file_refuses_sensitive_home_target() {
        // End-to-end: a write into a sensitive $HOME subdir is rejected before
        // any directory is created or file written. Uses the real home + a
        // unique name; the guard must return Err so nothing is created.
        let Some(home) = home_dir() else { return };
        let target = home.join(".ssh").join("hwpforge-e1-guard-probe.tmp");
        let err = write_output_file(target.to_str().unwrap(), b"x").unwrap_err();
        assert_eq!(err.code, "WRITE_ERROR");
        assert!(!target.exists(), "sensitive target must not be written");
    }

    #[cfg(unix)]
    #[test]
    fn write_output_file_refuses_symlink_final_component() {
        // A planted symlink as the final component must not be followed.
        let dir = tempfile::tempdir().unwrap();
        let real_target = dir.path().join("real.txt");
        let link = dir.path().join("link.hwpx");
        std::os::unix::fs::symlink(&real_target, &link).unwrap();

        let err = write_output_file(link.to_str().unwrap(), b"data").unwrap_err();
        assert_eq!(err.code, "WRITE_ERROR");
        assert!(!real_target.exists(), "symlink target must not be written through");
    }
}
