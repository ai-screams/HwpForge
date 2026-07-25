//! `hwpforge_insert_para` / `hwpforge_delete_para` — structural paragraph edits (E4).

use serde::Serialize;

use hwpforge_smithy_hwpx::{
    scan_delete_warnings, HwpxStructuralEditor, InsertPosition, ParagraphLocator,
    StructuralEditError,
};

use crate::output::{read_file_bytes, ToolErrorInfo};

/// Output of a structural edit.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct StructuralData {
    /// Path the edited HWPX was written to.
    pub output_path: String,
    /// Human-readable description of what changed.
    pub change: String,
    /// Non-blocking advisories (e.g. index-mark removal). Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Delete top-level paragraphs, all-or-nothing.
pub fn run_delete_para(
    file_path: &str,
    section: usize,
    indices: &[usize],
    output_path: &str,
) -> Result<StructuralData, ToolErrorInfo> {
    if indices.is_empty() {
        return Err(ToolErrorInfo::new(
            "DELETE_NO_TARGET",
            "Pass at least one paragraph index",
            "indices must be a non-empty list of top-level paragraph indices.",
        ));
    }
    let bytes = read_file_bytes(file_path)?;
    let targets: Vec<ParagraphLocator> =
        indices.iter().map(|&index| ParagraphLocator { section, index }).collect();
    // Advisory scan (shared library messages — never a refusal): surfaced
    // only alongside a successful edit.
    let warnings: Vec<String> =
        scan_delete_warnings(&bytes, &targets).iter().map(ToString::to_string).collect();
    let out = HwpxStructuralEditor::delete_paragraphs(&bytes, &targets).map_err(map_error)?;
    write_bytes(&out, output_path)?;
    Ok(StructuralData {
        output_path: output_path.to_string(),
        change: format!("deleted {} paragraph(s) from section {section}", indices.len()),
        warnings,
    })
}

/// Insert a contiguous block of paragraphs relative to an anchor.
///
/// Exactly one of `text` (single paragraph) or `texts` (block) must be
/// provided.
pub fn run_insert_para(
    file_path: &str,
    section: usize,
    anchor: usize,
    before: bool,
    text: Option<&str>,
    texts: Option<&[String]>,
    output_path: &str,
) -> Result<StructuralData, ToolErrorInfo> {
    let block: Vec<String> = match (text, texts) {
        (Some(t), None) => vec![t.to_string()],
        (None, Some(ts)) if !ts.is_empty() => ts.to_vec(),
        _ => {
            return Err(ToolErrorInfo::new(
                "INSERT_TEXT_REQUIRED",
                "Provide exactly one of `text` or `texts` (non-empty)",
                "Use `text` for a single paragraph or `texts` for a contiguous block.",
            ));
        }
    };
    let bytes = read_file_bytes(file_path)?;
    let position = if before { InsertPosition::Before } else { InsertPosition::After };
    let anchor_loc = ParagraphLocator { section, index: anchor };
    let out = HwpxStructuralEditor::insert_paragraphs(&bytes, anchor_loc, position, &block)
        .map_err(map_error)?;
    write_bytes(&out, output_path)?;
    let where_ = if before { "before" } else { "after" };
    Ok(StructuralData {
        output_path: output_path.to_string(),
        change: format!(
            "inserted {} paragraph(s) {where_} section {section} paragraph {anchor}",
            block.len()
        ),
        warnings: Vec::new(),
    })
}

fn write_bytes(bytes: &[u8], path: &str) -> Result<(), ToolErrorInfo> {
    std::fs::write(path, bytes).map_err(|e| {
        ToolErrorInfo::new(
            "FILE_WRITE_FAILED",
            format!("Cannot write '{path}': {e}"),
            "Check the output path and permissions.",
        )
    })
}

fn map_error(err: StructuralEditError) -> ToolErrorInfo {
    let (code, hint): (&str, &str) = match &err {
        StructuralEditError::NotRoundTripSafe { .. } => (
            "INPUT_NOT_ROUNDTRIP_SAFE",
            "Structural edits require a round-trip-safe input; this document has a codec fidelity gap.",
        ),
        StructuralEditError::ReferenceStranded { .. } => (
            "REFERENCE_STRANDED",
            "This paragraph carries a bookmark/cross-ref/footnote; deleting it could strand a reference.",
        ),
        StructuralEditError::HardBreakLoss { .. } => {
            ("HARD_BREAK_LOSS", "This paragraph carries a hard page/column break.")
        }
        StructuralEditError::SectionPropertiesParagraph { .. }
        | StructuralEditError::InsertBeforeSectionProperties { .. } => (
            "SECTION_PROPERTIES_PARAGRAPH",
            "The section's first paragraph holds page setup; it cannot be deleted or displaced.",
        ),
        StructuralEditError::EmptySection { .. } => {
            ("EMPTY_SECTION", "A section must keep at least one paragraph.")
        }
        StructuralEditError::MultiParagraphText => {
            ("MULTI_PARAGRAPH_TEXT", "Insert one paragraph per call; text may not contain line breaks.")
        }
        StructuralEditError::SectionOutOfRange { .. }
        | StructuralEditError::ParagraphOutOfRange { .. } => {
            ("INDEX_OUT_OF_RANGE", "Use hwpforge_outline to see section and paragraph counts.")
        }
        StructuralEditError::DuplicateTarget { .. } => {
            ("DUPLICATE_TARGET", "Each paragraph index may appear once per batch.")
        }
        StructuralEditError::DeltaMismatch { .. } => {
            ("SELF_VERIFY_FAILED", "The edit did not verify; no output was written.")
        }
        StructuralEditError::Codec(_) => {
            ("STRUCTURAL_CODEC", "Check that the file is valid HWPX.")
        }
        _ => ("STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),
    };
    ToolErrorInfo::new(code, err.to_string(), hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_doc(dir: &tempfile::TempDir) -> String {
        let path = dir.path().join("base.hwpx");
        crate::tools::convert::run_convert(
            "첫째 문단.\n\n둘째 문단.\n\n셋째 문단.",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn insert_para_via_mcp_surface() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");
        let data =
            run_insert_para(&base, 0, 1, false, Some("삽입"), None, out.to_str().unwrap()).unwrap();
        assert!(data.change.contains("inserted 1 paragraph"));
        assert!(out.exists());
    }

    #[test]
    fn insert_para_batch_via_texts() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");
        let block = vec!["하나.".to_string(), "둘.".to_string()];
        let data =
            run_insert_para(&base, 0, 1, false, None, Some(&block), out.to_str().unwrap()).unwrap();
        assert!(data.change.contains("inserted 2 paragraph"));
        assert!(out.exists());
    }

    #[test]
    fn insert_para_requires_exactly_one_text_form() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");
        let block = vec!["x".to_string()];

        // Neither form.
        let err =
            run_insert_para(&base, 0, 1, false, None, None, out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "INSERT_TEXT_REQUIRED");
        // Both forms (ambiguous).
        let err =
            run_insert_para(&base, 0, 1, false, Some("x"), Some(&block), out.to_str().unwrap())
                .unwrap_err();
        assert_eq!(err.code, "INSERT_TEXT_REQUIRED");
        // Empty block.
        let err = run_insert_para(&base, 0, 1, false, None, Some(&[]), out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "INSERT_TEXT_REQUIRED");
    }

    #[test]
    fn delete_para_via_mcp_surface_and_secpr_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");
        let data = run_delete_para(&base, 0, &[1], out.to_str().unwrap()).unwrap();
        assert!(data.change.contains("deleted"));
        assert!(data.warnings.is_empty(), "plain paragraph must not warn: {:?}", data.warnings);

        let err = run_delete_para(&base, 0, &[0], out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "SECTION_PROPERTIES_PARAGRAPH");
    }

    #[test]
    fn delete_para_rejects_empty_targets() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");
        let err = run_delete_para(&base, 0, &[], out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "DELETE_NO_TARGET");
    }

    #[test]
    fn error_code_mapping_covers_common_rejections() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let out = dir.path().join("out.hwpx");

        // out-of-range anchor / index.
        let err = run_insert_para(&base, 0, 99, false, Some("x"), None, out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "INDEX_OUT_OF_RANGE");
        let err = run_delete_para(&base, 9, &[0], out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "INDEX_OUT_OF_RANGE");

        // multiline text.
        let err = run_insert_para(&base, 0, 1, false, Some("a\nb"), None, out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "MULTI_PARAGRAPH_TEXT");

        // duplicate target in a batch.
        let err = run_delete_para(&base, 0, &[1, 1], out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "DUPLICATE_TARGET");

        // decode failure on non-HWPX input.
        let garbage = dir.path().join("g.hwpx");
        std::fs::write(&garbage, b"not a zip").unwrap();
        let err =
            run_delete_para(garbage.to_str().unwrap(), 0, &[0], out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "STRUCTURAL_CODEC");
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/structural")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn write_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_doc(&dir);
        let err =
            run_insert_para(&base, 0, 1, false, Some("x"), None, "/nonexistent-dir-e4/o.hwpx")
                .unwrap_err();
        assert_eq!(err.code, "FILE_WRITE_FAILED");
        let err = run_delete_para(&base, 0, &[1], "/nonexistent-dir-e4/o.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_WRITE_FAILED");
    }

    #[test]
    fn error_code_mapping_reference_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.hwpx");

        let err = run_delete_para(&fixture("crossref_para.hwpx"), 0, &[0], out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "REFERENCE_STRANDED");

        let err = run_delete_para(&fixture("page_break.hwpx"), 0, &[1], out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "HARD_BREAK_LOSS");

        let err = run_delete_para(&fixture("plain_inserted.hwpx"), 0, &[1], out.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.code, "INPUT_NOT_ROUNDTRIP_SAFE");
    }
}
