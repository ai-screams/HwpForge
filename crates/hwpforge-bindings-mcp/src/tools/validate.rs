//! `hwpforge_validate` — HWPX document validation tool.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwpx::HwpxDecoder;

use crate::output::{check_file_size, ToolErrorInfo};

/// Output data from a validation check.
#[derive(Debug, Serialize)]
pub struct ValidateData {
    /// Whether the document is valid.
    pub valid: bool,
    /// Number of sections.
    pub sections: usize,
    /// Total paragraphs across all sections.
    pub paragraphs: usize,
    /// List of issues found (empty if valid).
    pub issues: Vec<String>,
}

/// Validate an HWPX file structure and integrity.
pub fn run_validate(file_path: &str) -> Result<ValidateData, ToolErrorInfo> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("HWPX file not found: {file_path}"),
            "Check the file path and try again.",
        ));
    }

    check_file_size(path)?;
    let bytes = std::fs::read(path).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read file: {e}"),
            "Check file permissions.",
        )
    })?;

    // Phase 1: Decode
    let hwpx_doc = match HwpxDecoder::decode(&bytes) {
        Ok(doc) => doc,
        Err(e) => {
            return Ok(ValidateData {
                valid: false,
                sections: 0,
                paragraphs: 0,
                issues: vec![format!("HWPX decode failed: {e}")],
            });
        }
    };

    let sections = hwpx_doc.document.sections().len();
    let paragraphs: usize = hwpx_doc.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    // Phase 2: Validate
    match hwpx_doc.document.validate() {
        Ok(_) => Ok(ValidateData { valid: true, sections, paragraphs, issues: vec![] }),
        Err(e) => Ok(ValidateData {
            valid: false,
            sections,
            paragraphs,
            issues: vec![format!("Validation error: {e}")],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_missing_file() {
        let err = run_validate("/nonexistent/file.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn validate_invalid_hwpx_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.hwpx");
        std::fs::write(&path, b"not a zip file").unwrap();

        let data = run_validate(path.to_str().unwrap()).unwrap();
        assert!(!data.valid);
        assert_eq!(data.sections, 0);
        assert!(!data.issues.is_empty());
    }
}
