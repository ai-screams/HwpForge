//! Structural paragraph editing (E4): insert / delete top-level paragraphs.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{
    HwpxStructuralEditor, InsertPosition, Insertion, ParagraphLocator, StructuralEditError,
};

use crate::error::{check_file_size, CliError};

/// Run `delete-para`.
pub fn run_delete(
    file: &PathBuf,
    section: usize,
    indices: &[usize],
    output: &PathBuf,
    json_mode: bool,
) {
    if indices.is_empty() {
        CliError::new("DELETE_NO_TARGET", "Pass at least one --index").exit(json_mode, 1);
    }
    let bytes = read_input(file, json_mode);
    let targets: Vec<ParagraphLocator> =
        indices.iter().map(|&index| ParagraphLocator { section, index }).collect();
    match HwpxStructuralEditor::delete_paragraphs(&bytes, &targets) {
        Ok(out) => write_output(&out, output, json_mode, |v| {
            *v = serde_json::json!({
                "status": "ok",
                "deleted": indices.len(),
                "section": section,
                "indices": indices,
                "output": output.display().to_string(),
            });
        }),
        Err(e) => exit_structural_error(e, json_mode),
    }
}

/// Run `insert-para`.
#[allow(clippy::too_many_arguments)]
pub fn run_insert(
    file: &PathBuf,
    section: usize,
    anchor: usize,
    before: bool,
    text: &str,
    output: &PathBuf,
    json_mode: bool,
) {
    let bytes = read_input(file, json_mode);
    let position = if before { InsertPosition::Before } else { InsertPosition::After };
    let insertion = Insertion {
        anchor: ParagraphLocator { section, index: anchor },
        position,
        text: text.to_string(),
    };
    match HwpxStructuralEditor::insert_paragraph(&bytes, &insertion) {
        Ok(out) => write_output(&out, output, json_mode, |v| {
            *v = serde_json::json!({
                "status": "ok",
                "section": section,
                "anchor": anchor,
                "position": if before { "before" } else { "after" },
                "output": output.display().to_string(),
            });
        }),
        Err(e) => exit_structural_error(e, json_mode),
    }
}

fn read_input(file: &PathBuf, json_mode: bool) -> Vec<u8> {
    check_file_size(file, json_mode);
    match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    }
}

fn write_output(
    bytes: &[u8],
    output: &PathBuf,
    json_mode: bool,
    fill: impl FnOnce(&mut serde_json::Value),
) {
    if let Err(e) = std::fs::write(output, bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 2);
    }
    if json_mode {
        let mut v = serde_json::Value::Null;
        fill(&mut v);
        println!("{}", serde_json::to_string(&v).unwrap());
    } else {
        println!("Wrote {}", output.display());
    }
}

fn exit_structural_error(err: StructuralEditError, json_mode: bool) -> ! {
    let code = match &err {
        StructuralEditError::Codec(_) => "STRUCTURAL_CODEC",
        StructuralEditError::NotRoundTripSafe { .. } => "INPUT_NOT_ROUNDTRIP_SAFE",
        StructuralEditError::UncarriedZipEntries { .. } => "UNCARRIED_ZIP_ENTRIES",
        StructuralEditError::SectionOutOfRange { .. } => "SECTION_OUT_OF_RANGE",
        StructuralEditError::ParagraphOutOfRange { .. } => "PARAGRAPH_OUT_OF_RANGE",
        StructuralEditError::DuplicateTarget { .. } => "DUPLICATE_TARGET",
        StructuralEditError::ReferenceStranded { .. } => "REFERENCE_STRANDED",
        StructuralEditError::HardBreakLoss { .. } => "HARD_BREAK_LOSS",
        StructuralEditError::EmptySection { .. } => "EMPTY_SECTION",
        StructuralEditError::SectionPropertiesParagraph { .. } => "SECTION_PROPERTIES_PARAGRAPH",
        StructuralEditError::SpanCountMismatch { .. } => "SPAN_COUNT_MISMATCH",
        StructuralEditError::DeltaMismatch { .. } => "SELF_VERIFY_FAILED",
        StructuralEditError::MultiParagraphText => "MULTI_PARAGRAPH_TEXT",
        StructuralEditError::InsertBeforeSectionProperties { .. } => {
            "INSERT_BEFORE_SECTION_PROPERTIES"
        }
        _ => "STRUCTURAL_EDIT_FAILED",
    };
    let exit = if matches!(err, StructuralEditError::Codec(_)) { 2 } else { 1 };
    CliError::new(code, err.to_string()).exit(json_mode, exit);
}
