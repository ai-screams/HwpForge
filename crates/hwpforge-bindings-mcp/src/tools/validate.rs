//! `hwpforge_validate` — HWPX document validation tool.

use serde::Serialize;

use hwpforge::ops;

use crate::compat;
use crate::output::{read_file_bytes, ToolErrorInfo, ToolWarningInfo};

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
    /// Decode warnings raised on the way in (`ops::ValidateOutput::warnings`)
    /// — present whether the document validated or not; empty (and never
    /// populated) when the input could not even be decoded, since then
    /// `ops::validate` never produces a warning list to surface. Omitted
    /// when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
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
/// warnings from a *successful* decode (`ops::ValidateOutput::warnings`) are
/// surfaced through `ValidateData::warnings` regardless of `valid` — an
/// undecodable input has no decode to warn from, so that branch stays empty.
pub fn run_validate(file_path: &str) -> Result<ValidateData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;

    match ops::validate(&bytes) {
        Ok(out) if out.ok => Ok(ValidateData {
            valid: true,
            sections: out.sections,
            paragraphs: out.paragraphs,
            issues: vec![],
            warnings: out.warnings.iter().map(compat::warning).collect(),
        }),
        Ok(out) => {
            let detail = out.errors.first().map(|e| e.message.as_str()).unwrap_or_default();
            Ok(ValidateData {
                valid: false,
                sections: out.sections,
                paragraphs: out.paragraphs,
                issues: vec![format!("Validation error: {detail}")],
                warnings: out.warnings.iter().map(compat::warning).collect(),
            })
        }
        Err(err) => Ok(ValidateData {
            valid: false,
            sections: 0,
            paragraphs: 0,
            issues: vec![format!("HWPX decode failed: {err}")],
            warnings: vec![],
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
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
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
        assert!(data.warnings.is_empty(), "an undecodable input has no decode to warn from");
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 는 여전히 유효한 문서이므로 `valid: true`
    /// 지만, 그 성공한 디코드의 경고(`LAYOUT_CACHE_DROPPED`)는 여전히
    /// `warnings` 에 실려야 한다 — valid 여부와 무관하게.
    #[test]
    fn validate_surfaces_decode_warnings_on_a_valid_document() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_validate(&path).unwrap();
        assert!(data.valid, "{:?}", data.issues);
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "validate must surface the decode warning of a successful decode: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
    }

    /// `run_validate`'s `Ok(out)` non-`ok` arm (`validate.rs:56-65`) is not
    /// reachable through the public API with a real file: encoding always
    /// requires `Document::validate` to pass first (`Document<Validated>`),
    /// so no HWPX package this crate can produce ever decodes into
    /// something `Document::validate` then rejects, and `ops::ValidateOutput`
    /// is `#[non_exhaustive]` with no public constructor, so it cannot be
    /// built directly either — see `run_validate`'s doc for the exact split.
    /// `ValidateData` itself has no such restriction (all fields `pub`, not
    /// `#[non_exhaustive]`), so this covers what that arm actually risks:
    /// that `valid: false` and a populated `warnings` list serialize
    /// correctly together, which is the shape the two branches share.
    #[test]
    fn validate_data_serializes_invalid_with_warnings_present() {
        let data = ValidateData {
            valid: false,
            sections: 1,
            paragraphs: 3,
            issues: vec!["Validation error: Section 0 has no paragraphs".to_string()],
            warnings: vec![ToolWarningInfo::new(
                "LAYOUT_CACHE_DROPPED",
                "layout cache dropped at section[0]: ledger construction failed",
            )],
        };

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["valid"], false);
        assert!(!value["issues"].as_array().unwrap().is_empty());
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
    }
}
