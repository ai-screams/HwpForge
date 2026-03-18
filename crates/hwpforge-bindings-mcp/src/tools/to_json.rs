//! `hwpforge_to_json` — HWPX → JSON export tool.

use serde::Serialize;

use hwpforge_smithy_hwpx::{ExportedDocument, ExportedSection, HwpxDecoder, HwpxPatcher};

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful JSON export.
#[derive(Debug, Serialize)]
pub struct ToJsonData {
    /// Path to the generated JSON file (if written to file).
    pub output_path: Option<String>,
    /// Size of the JSON in bytes.
    pub size_bytes: u64,
    /// Whether this is a section-only export.
    pub section_only: bool,
    /// The JSON string (returned inline when no output_path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_content: Option<String>,
}

/// Export HWPX to JSON (full document or single section).
pub fn run_to_json(
    file_path: &str,
    section_idx: Option<usize>,
    output_path: Option<&str>,
) -> Result<ToJsonData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    let styles = Some(hwpx_doc.style_store);

    let json_string = if let Some(idx) = section_idx {
        let sections = hwpx_doc.document.sections();
        if idx >= sections.len() {
            return Err(ToolErrorInfo::new(
                "SECTION_OUT_OF_RANGE",
                format!("Section {idx} does not exist (document has {} sections)", sections.len()),
                format!("Valid range: 0..={}", sections.len().saturating_sub(1)),
            ));
        }
        let preservation =
            HwpxPatcher::export_section_preservation(&bytes, idx, &sections[idx]).ok();
        let exported = ExportedSection {
            section_index: idx,
            section: sections[idx].clone(),
            styles,
            preservation,
        };
        serde_json::to_string_pretty(&exported).map_err(|e| {
            ToolErrorInfo::new(
                "SERIALIZE_ERROR",
                format!("Failed to serialize section: {e}"),
                "This may be a bug.",
            )
        })?
    } else {
        let exported = ExportedDocument { document: hwpx_doc.document, styles };
        serde_json::to_string_pretty(&exported).map_err(|e| {
            ToolErrorInfo::new(
                "SERIALIZE_ERROR",
                format!("Failed to serialize document: {e}"),
                "This may be a bug.",
            )
        })?
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
        })
    } else {
        // Warn if inline response is very large (> 1 MB)
        const MAX_INLINE_RESPONSE: u64 = 1024 * 1024;
        if size_bytes > MAX_INLINE_RESPONSE {
            return Err(ToolErrorInfo::new(
                "OUTPUT_TOO_LARGE",
                format!("JSON output is {} KB, too large for inline response", size_bytes / 1024,),
                "Use output_path to write to a file, or use section parameter to export a single section.",
            ));
        }
        Ok(ToJsonData {
            output_path: None,
            size_bytes,
            section_only: section_idx.is_some(),
            json_content: Some(json_string),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!exported.preservation.unwrap().text_slots.is_empty());
    }
}
