//! `hwpforge_validate` — HWPX document validation tool.

use serde::Serialize;

use hwpforge::ops;

use crate::output::{read_file_bytes, ToolErrorInfo};

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
///
/// `ops::validate` treats an undecodable package as an
/// [`ops::OpsError::Decode`] (a *query cannot be answered* failure), but this
/// tool's frozen contract has never propagated that as a `ToolErrorInfo`: a
/// bad file is reported as `valid: false` with the decode failure folded
/// into `issues`, same as a failed `Document::validate` check, so both
/// branches are reconstructed here rather than routed through
/// `compat::tool_error`. `ops::validate`'s only error variant is `Decode`
/// (see its `# Errors` docs), and `OpsError::Decode`'s `Display` is
/// transparent to the wrapped `HwpxError`, so `format!("HWPX decode failed:
/// {err}")` reproduces the pre-migration message byte for byte. Decoder
/// warnings (`ops::ValidateOutput::warnings`) are not surfaced: `ValidateData`
/// has no field for them (schema freeze) — see the W2 report.
pub fn run_validate(file_path: &str) -> Result<ValidateData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;

    match ops::validate(&bytes) {
        Ok(out) if out.ok => Ok(ValidateData {
            valid: true,
            sections: out.sections,
            paragraphs: out.paragraphs,
            issues: vec![],
        }),
        Ok(out) => {
            let detail = out.errors.first().map(|e| e.message.as_str()).unwrap_or_default();
            Ok(ValidateData {
                valid: false,
                sections: out.sections,
                paragraphs: out.paragraphs,
                issues: vec![format!("Validation error: {detail}")],
            })
        }
        Err(err) => Ok(ValidateData {
            valid: false,
            sections: 0,
            paragraphs: 0,
            issues: vec![format!("HWPX decode failed: {err}")],
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
    fn validate_valid_document() {
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("valid.hwpx");
        crate::tools::convert::run_convert(
            "# Test\n\nParagraph.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_validate(hwpx_path.to_str().unwrap()).unwrap();
        assert!(data.valid);
        assert!(data.sections >= 1);
        assert!(data.paragraphs >= 1);
        assert!(data.issues.is_empty());
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
