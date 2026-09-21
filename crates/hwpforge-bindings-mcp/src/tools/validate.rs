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

    /// `run_validate`'s `Ok(out)` non-`ok` arm (`validate.rs:56-65`) was once
    /// believed unreachable through the public API with a real file, on the
    /// theory that encoding always requires `Document::validate` to pass
    /// first. That reasoning only covers packages *this crate's own
    /// encoder* produces — it says nothing about `HwpxDecoder`, which builds
    /// a `Document<Draft>` straight from wire XML with none of
    /// `Document::validate`'s structural rules applied. A hand-tampered
    /// package — one real section plus a second, empty one, built by
    /// splicing a bare `<hs:sec>` into `stale-line-cache.hwpx`'s own ZIP —
    /// decodes cleanly and only fails afterward, in this function's own
    /// `Document::validate()` call, exercising the arm for real. Built from
    /// that fixture specifically so the same decode also carries its
    /// `LAYOUT_CACHE_DROPPED` warning, proving `valid` and `warnings` really
    /// are independent, as the module doc above promises.
    #[test]
    fn validate_reports_decode_warnings_and_a_real_validation_failure_together() {
        use std::io::{Cursor, Write};

        let bytes = std::fs::read(fixture("layout/stale-line-cache.hwpx")).expect("read fixture");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i).expect("entry");
            writer.raw_copy_file(entry).expect("copy");
        }
        // `HwpxDecoder` has no opinion on an empty `<hs:sec>` — it just
        // decodes zero `<hp:p>` elements — but `Document::validate` rejects
        // an empty section (`ValidationError::EmptySection`), the cheapest
        // rule to trip without touching the fixture's own cached section 0.
        writer
            .start_file("Contents/section1.xml", zip::write::SimpleFileOptions::default())
            .expect("start section1");
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"></hs:sec>"#,
            )
            .expect("write section1");
        let tampered = writer.finish().expect("finish").into_inner();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two-section-one-empty.hwpx");
        std::fs::write(&path, &tampered).expect("write tampered fixture");

        let data = run_validate(path.to_str().unwrap()).unwrap();

        assert!(!data.valid, "the second, empty section must fail Document::validate");
        assert_eq!(data.sections, 2, "the decode itself sees both sections");
        assert!(
            data.issues.iter().any(|issue| issue.contains("Section 1 has no paragraphs")),
            "the validation error must name the rule it tripped: {:?}",
            data.issues
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
    }
}
