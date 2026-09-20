//! `hwpforge_insert_para` / `hwpforge_delete_para` — structural paragraph edits (E4).

use serde::Serialize;

use hwpforge::ops::{self, OpsWarning};

use crate::compat::{self, Tool};
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

/// Renders one warning into this file's `Vec<String>` shape: a
/// [`OpsWarning::Structural`] advisory keeps its own `Display` (unchanged,
/// pre-migration behaviour); every other kind — chiefly `Decode`, for
/// example `LAYOUT_CACHE_DROPPED` — renders as `"{code}: {message}"` via
/// [`OpsWarning::info`]. Matches the CLI's `structural.rs::render_warning`
/// (`1ac435b`) so the two frontends never disagree about the string.
fn render_warning(warning: &OpsWarning) -> String {
    match warning {
        OpsWarning::Structural(sw) => sw.to_string(),
        other => {
            let info = other.info();
            format!("{}: {}", info.code, info.message)
        }
    }
}

/// Keeps the advisory scan's own warnings (`INDEX_MARK_REMOVED`, …) and,
/// additively (W5 follow-up), the admission decode's warnings that
/// `ops::insert_para`/`ops::delete_para` also report (for example
/// `LAYOUT_CACHE_DROPPED`) — before this follow-up the filter kept only
/// `Structural`, dropping decode warnings entirely (`insert_para`'s
/// `warnings` was always empty because it never produces a `Structural`
/// warning). `ops::edit`'s documented merge order is decode-then-advisory
/// (see that module's doc comment), and this filter does not reorder, so the
/// array comes out decode-first — the same order the CLI's `structural.rs`
/// renders (`out.warnings.iter().map(render_warning)`, no reordering there
/// either). Every existing `Structural` string stays byte-identical:
/// [`render_warning`] still calls `sw.to_string()` for that variant.
fn structural_advisories(warnings: &[OpsWarning]) -> Vec<String> {
    warnings
        .iter()
        .filter(|w| matches!(w, OpsWarning::Structural(_) | OpsWarning::Decode(_)))
        .map(render_warning)
        .collect()
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
    let opts =
        ops::DeleteParaOptions::default().with_section(section).with_indexes(indices.to_vec());
    let out =
        ops::delete_para(&bytes, &opts).map_err(|e| compat::tool_error(Tool::DeletePara, e))?;
    let warnings = structural_advisories(&out.warnings);
    write_bytes(&out.bytes, output_path)?;
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
    let opts = ops::InsertParaOptions::default()
        .with_section(section)
        .with_anchor(anchor)
        .with_text(block.clone())
        .with_before(before);
    let out =
        ops::insert_para(&bytes, &opts).map_err(|e| compat::tool_error(Tool::InsertPara, e))?;
    let warnings = structural_advisories(&out.warnings);
    write_bytes(&out.bytes, output_path)?;
    let where_ = if before { "before" } else { "after" };
    Ok(StructuralData {
        output_path: output_path.to_string(),
        change: format!(
            "inserted {} paragraph(s) {where_} section {section} paragraph {anchor}",
            block.len()
        ),
        warnings,
    })
}

/// Writes bytes to `path` without creating parent directories (unlike
/// `output::write_output_file`) — a missing parent must fail with
/// `FILE_WRITE_FAILED`, which this tool has reported since before the ops
/// migration.
fn write_bytes(bytes: &[u8], path: &str) -> Result<(), ToolErrorInfo> {
    std::fs::write(path, bytes).map_err(|e| {
        ToolErrorInfo::new(
            "FILE_WRITE_FAILED",
            format!("Cannot write '{path}': {e}"),
            "Check the output path and permissions.",
        )
    })
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

    /// A package whose decode raises `LAYOUT_CACHE_DROPPED` and which is
    /// still admissible — the same fixture `ops::edit`'s own tests use
    /// (`crates/hwpforge/tests/ops_insert_para.rs`/`ops_delete_para.rs`), so
    /// the two surfaces cannot disagree about what "a document that warns"
    /// means. Copied into the tempdir because these tools take a file path,
    /// not bytes.
    fn warning_fixture(dir: &tempfile::TempDir) -> String {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/layout/stale-line-cache.hwpx"
        ))
        .expect("stale-line-cache.hwpx");
        let path = dir.path().join("stale-line-cache.hwpx");
        std::fs::write(&path, bytes).unwrap();
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
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
    }

    /// W5 follow-up: `insert_para` has no advisory scan of its own, so
    /// before this fix its `warnings` was always empty — the admission
    /// decode's `LAYOUT_CACHE_DROPPED` never reached this tool's output.
    #[test]
    fn insert_para_surfaces_the_decode_warning_from_a_stale_layout_cache() {
        let dir = tempfile::tempdir().unwrap();
        let base = warning_fixture(&dir);
        let out = dir.path().join("out.hwpx");
        let data =
            run_insert_para(&base, 0, 2, false, Some("끼움"), None, out.to_str().unwrap()).unwrap();

        assert_eq!(data.warnings.len(), 1, "{:?}", data.warnings);
        assert!(data.warnings[0].contains("LAYOUT_CACHE_DROPPED"), "{:?}", data.warnings);
    }

    /// The documented merge order — what the decode reported, then what the
    /// edit itself advises — must survive this tool's filter unreordered.
    /// Paragraph 2 of the fixture carries an index mark (see
    /// `ops_delete_para.rs::the_decode_warning_precedes_the_delete_advisory`),
    /// so deleting it produces one of each.
    #[test]
    fn delete_para_surfaces_the_decode_warning_before_its_own_advisory() {
        let dir = tempfile::tempdir().unwrap();
        let base = warning_fixture(&dir);
        let out = dir.path().join("out.hwpx");
        let data = run_delete_para(&base, 0, &[2], out.to_str().unwrap()).unwrap();

        assert_eq!(data.warnings.len(), 2, "{:?}", data.warnings);
        assert!(data.warnings[0].contains("LAYOUT_CACHE_DROPPED"), "{:?}", data.warnings);
        assert!(data.warnings[1].contains("index-mark"), "{:?}", data.warnings);
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
