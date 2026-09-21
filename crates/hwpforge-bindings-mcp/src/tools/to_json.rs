//! `hwpforge_to_json` — HWPX → JSON export tool.

use serde::Serialize;

use hwpforge::ops::{self, ExportSectionOptions, OpsWarning, ToJsonOptions};
use hwpforge_smithy_hwpx::SectionWorkflowWarning;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo, ToolWarningInfo};

/// Output data from a successful JSON export.
#[derive(Debug, Serialize)]
pub struct ToJsonData {
    /// Path to the generated JSON file (if written to file).
    pub output_path: Option<String>,
    /// Size of the exported document/section JSON in bytes — `json_content`'s
    /// length, not the whole MCP response (`warnings` adds to that but not
    /// to this field). The `OUTPUT_TOO_LARGE` inline-mode gate below
    /// measures the complete serialized response, including `warnings`, so
    /// a response can be rejected as too large even when `size_bytes` alone
    /// looks small.
    pub size_bytes: u64,
    /// Whether this is a section-only export.
    pub section_only: bool,
    /// The JSON string (returned inline when no output_path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_content: Option<String>,
    /// Non-fatal warnings encountered during export.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Inline response ceiling shared with `hwpforge_outline`/`hwpforge_diff` (1 MB).
const MAX_INLINE_RESPONSE: usize = 1024 * 1024;

/// Export HWPX to JSON (full document or single section).
///
/// Every warning `ops::to_json`/`ops::export_section` reports — decode
/// (`OpsWarning::Decode`, e.g. `LAYOUT_CACHE_DROPPED`/`UNKNOWN_ENUM_VALUE`),
/// section-workflow and grid-address — is surfaced through `ToJsonData.warnings`.
/// A JSON export the caller reuses for `from_json`/`patch` would otherwise
/// persist a decode-time fallback (an unknown enum value, a dropped layout
/// cache) without disclosing it. `SectionWorkflow`'s
/// `PRESERVATION_METADATA_UNAVAILABLE` keeps its tool-specific hint via
/// [`map_section_workflow_warning_for_to_json`]; every other warning goes
/// through `compat::warning` unchanged from what this file computed by hand
/// before.
///
/// MEDIUM (audit): the inline-mode `OUTPUT_TOO_LARGE` gate used to measure
/// only the document/section JSON's own length, so a document just under
/// the ceiling with enough decode warnings attached could still serialize
/// to an oversized MCP response. [`build_to_json_data`] gates on the
/// complete serialized [`ToJsonData`] — `warnings` included — exactly as
/// `hwpforge_outline`'s `build_outline_data` and `hwpforge_diff`'s
/// `build_diff_data` already do.
pub fn run_to_json(
    file_path: &str,
    section_idx: Option<usize>,
    output_path: Option<&str>,
) -> Result<ToJsonData, ToolErrorInfo> {
    // Guard the output extension (matches the .hwpx guard on convert/patch).
    // Inline mode (no output_path) writes nothing, so it needs no guard.
    if let Some(out_path) = output_path {
        if !out_path.ends_with(".json") {
            return Err(ToolErrorInfo::new(
                "INVALID_EXTENSION",
                format!("Output path must end with .json: {out_path}"),
                "Use a .json output_path, or omit output_path to receive the JSON inline.",
            ));
        }
    }
    let bytes = read_file_bytes(file_path)?;
    let mut warnings: Vec<ToolWarningInfo> = Vec::new();

    let json_string = if let Some(idx) = section_idx {
        let out = ops::export_section(&bytes, &ExportSectionOptions::default().with_section(idx))
            .map_err(|e| compat::tool_error(Tool::ToJson, e))?;
        for w in &out.warnings {
            match w {
                OpsWarning::SectionWorkflow(sw) => {
                    warnings.push(map_section_workflow_warning_for_to_json(sw.clone()));
                }
                other => warnings.push(compat::warning(other)),
            }
        }
        render_pretty_value(&out.section)?
    } else {
        let out = ops::to_json(&bytes, &ToJsonOptions::default())
            .map_err(|e| compat::tool_error(Tool::ToJson, e))?;
        for w in &out.warnings {
            warnings.push(compat::warning(w));
        }
        render_pretty_value(&out.document)?
    };

    let size_bytes = json_string.len() as u64;

    // Write to file if output_path is given
    if let Some(out_path) = output_path {
        write_output_file(out_path, json_string.as_bytes())?;
        Ok(ToJsonData {
            output_path: Some(out_path.to_string()),
            size_bytes,
            section_only: section_idx.is_some(),
            json_content: None,
            warnings,
        })
    } else {
        build_to_json_data(json_string, section_idx.is_some(), warnings)
    }
}

/// Builds the final inline-mode [`ToJsonData`], gating on the complete
/// serialized response — including `warnings` — rather than on the
/// document/section JSON alone: a document with many decode warnings but a
/// small export could otherwise slip past a narrower check while still
/// exceeding the real inline ceiling (the file-output branch in
/// [`run_to_json`] has no such gate — it always writes to disk).
///
/// LOW (audit): measuring the complete response used to serialize the whole
/// [`ToJsonData`] into a second full `String` (`serde_json::to_string`) just
/// to read its length, on top of the `json_string` already held in
/// `json_content` — and the MCP server serializes the returned value a
/// third time to actually send it. `json_content` alone exceeding the
/// ceiling is rejected immediately, before `ToJsonData` is even built;
/// otherwise the total is measured with [`CountingWriter`], a
/// [`std::io::Write`] sink that only counts bytes, so `serde_json::to_writer`
/// never allocates the serialized bytes at all.
///
/// Split out from [`run_to_json`] so a test can exercise the gate with a
/// synthetic oversized `warnings` list, without needing a fixture large
/// enough to trigger it for real.
fn build_to_json_data(
    json_string: String,
    section_only: bool,
    warnings: Vec<ToolWarningInfo>,
) -> Result<ToJsonData, ToolErrorInfo> {
    // `json_content` alone already over the ceiling makes the whole
    // response oversized regardless of `warnings` — reject before copying
    // it into `ToJsonData` and measuring the rest.
    if json_string.len() > MAX_INLINE_RESPONSE {
        return Err(output_too_large_error(json_string.len()));
    }

    let size_bytes = json_string.len() as u64;
    let data = ToJsonData {
        output_path: None,
        size_bytes,
        section_only,
        json_content: Some(json_string),
        warnings,
    };

    let mut counter = CountingWriter::default();
    let inline_size = match serde_json::to_writer(&mut counter, &data) {
        Ok(()) => counter.0,
        Err(_) => usize::MAX,
    };
    if inline_size > MAX_INLINE_RESPONSE {
        return Err(output_too_large_error(inline_size));
    }
    Ok(data)
}

fn output_too_large_error(inline_size: usize) -> ToolErrorInfo {
    ToolErrorInfo::new(
        "OUTPUT_TOO_LARGE",
        format!("JSON response is {inline_size} bytes (limit {MAX_INLINE_RESPONSE})"),
        "Use output_path to write to a file, or use section parameter to export a single section.",
    )
}

/// A [`std::io::Write`] sink that only counts the bytes written to it —
/// lets [`build_to_json_data`] learn a `serde_json::to_writer` output's
/// exact size without allocating the serialized bytes themselves.
#[derive(Default)]
struct CountingWriter(usize);

impl std::io::Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn render_pretty_value(value: &serde_json::Value) -> Result<String, ToolErrorInfo> {
    serde_json::to_string_pretty(value).map_err(|e| {
        ToolErrorInfo::new(
            "SERIALIZE_ERROR",
            format!("Failed to serialize export: {e}"),
            "This may be a bug.",
        )
    })
}

/// Kept as a standalone function (rather than inlined at the one call site)
/// so `to_json_maps_preservation_warning_for_machine_consumers` keeps
/// exercising this exact mapping. `compat::warning` already reproduces the
/// legacy `(code, message)` pair byte for byte (`SectionWorkflowWarning::code`/
/// `::message` are what it reads); only the tool-specific hint — absent from
/// the generic compat envelope — is re-applied here.
fn map_section_workflow_warning_for_to_json(warning: SectionWorkflowWarning) -> ToolWarningInfo {
    let mapped = compat::warning(&OpsWarning::SectionWorkflow(warning));
    if mapped.code == "PRESERVATION_METADATA_UNAVAILABLE" {
        mapped.with_hint(
            "This JSON export may inspect correctly, but later hwpforge_patch can fail until preservation metadata is available. Re-export with the current tool after simplifying unsupported mixed-content edits.",
        )
    } else {
        mapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_smithy_hwpx::ExportedSection;
    use hwpforge_smithy_hwpx::SECTION_PRESERVATION_VERSION;

    #[test]
    fn to_json_section_embeds_preservation_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("source.hwpx");
        crate::tools::convert::run_convert(
            "# 제목\n\n본문 문단입니다.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_to_json(hwpx_path.to_str().unwrap(), Some(0), None).unwrap();
        assert!(data.section_only);
        let json = data.json_content.expect("inline section json expected");
        let exported: ExportedSection = serde_json::from_str(&json).unwrap();
        assert_eq!(exported.section_index, 0);
        assert!(exported.preservation.is_some(), "section export must embed preservation metadata");
        let preservation = exported.preservation.unwrap();
        assert_eq!(preservation.preservation_version, SECTION_PRESERVATION_VERSION);
        assert!(!preservation.text_slots.is_empty());
        assert!(data.warnings.is_empty());
    }

    #[test]
    fn to_json_maps_preservation_warning_for_machine_consumers() {
        let warning = SectionWorkflowWarning::PreservationMetadataUnavailable {
            detail: "raw/semantic mismatch".to_string(),
        };

        let mapped = map_section_workflow_warning_for_to_json(warning);
        assert_eq!(mapped.code, "PRESERVATION_METADATA_UNAVAILABLE");
        assert!(mapped.message.contains("raw/semantic mismatch"));
        assert!(mapped.hint.as_deref().unwrap().contains("hwpforge_patch"));
    }

    /// Repo-level fixture whose layout cache the decoder drops
    /// (`LAYOUT_CACHE_DROPPED`), shared with other crates' tests.
    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn to_json_surfaces_decode_warnings_for_both_export_shapes() {
        let path = fixture("layout/stale-line-cache.hwpx");

        let full = run_to_json(&path, None, None).unwrap();
        assert!(
            full.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "full-document export must surface the decode warning: {:?}",
            full.warnings
        );

        let section = run_to_json(&path, Some(0), None).unwrap();
        assert!(
            section.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "section export must surface the decode warning: {:?}",
            section.warnings
        );
    }

    #[test]
    fn to_json_rejects_non_json_output_path() {
        // The extension guard runs before any file access, so the input path
        // doesn't need to exist for this check.
        let err = run_to_json("ignored.hwpx", None, Some("out.txt"))
            .expect_err("non-.json output_path must be rejected");
        assert_eq!(err.code, "INVALID_EXTENSION");
        assert!(err.message.contains(".json"), "message should mention the .json requirement");
    }

    #[test]
    fn to_json_inline_mode_skips_extension_guard() {
        // No output_path → inline mode → guard must not fire on a missing
        // file; it should fail later with a read error instead.
        let err = run_to_json("/nonexistent/file.hwpx", None, None)
            .expect_err("missing input must still error");
        assert_ne!(err.code, "INVALID_EXTENSION", "inline mode must not hit the extension guard");
    }

    /// MEDIUM (audit): the document JSON itself is tiny here — the oversized
    /// total comes entirely from `warnings` — so this exercises the branch
    /// the whole-payload gate exists for: a small export and a large
    /// `warnings` list must still report `OUTPUT_TOO_LARGE`, not slip past
    /// because only `json_string.len()` was checked. Mirrors
    /// `outline_oversized_warnings_alone_reports_output_too_large`.
    #[test]
    fn to_json_oversized_warnings_alone_reports_output_too_large() {
        let huge =
            vec![ToolWarningInfo::new("STUB_OVERSIZED", "x".repeat(MAX_INLINE_RESPONSE + 1))];

        let err = build_to_json_data("{}".to_string(), false, huge).unwrap_err();
        assert_eq!(err.code, "OUTPUT_TOO_LARGE");
    }

    /// LOW (audit): `json_content` alone over the ceiling must be rejected
    /// before `ToJsonData` is even built, not measured via a second
    /// serialization of the whole struct. The reported size proves which
    /// branch ran: the early return reports exactly `json_string.len()`,
    /// while going through the later `CountingWriter` measurement would add
    /// the struct's JSON-wrapper overhead (field names, the surrounding
    /// quotes) and report a strictly larger number.
    #[test]
    fn to_json_content_alone_over_limit_is_rejected_before_building_the_response() {
        let huge = "x".repeat(MAX_INLINE_RESPONSE + 1);
        let len = huge.len();

        let err = build_to_json_data(huge, false, Vec::new()).unwrap_err();
        assert_eq!(err.code, "OUTPUT_TOO_LARGE");
        assert!(
            err.message.contains(&format!("{len} bytes")),
            "expected the early-return branch to report exactly {len} bytes: {}",
            err.message
        );
    }
}
