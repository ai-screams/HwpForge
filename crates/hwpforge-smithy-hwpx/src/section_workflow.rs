//! Shared section export/patch orchestration for bindings.
//!
//! This module centralizes section-level workflow decisions shared by the CLI
//! and MCP bindings while leaving file I/O and presentation-layer rendering in
//! the bindings themselves.

use crate::error::HwpxError;
use crate::{ExportedSection, HwpxDecoder, HwpxPatcher, PackageReader};

/// Result of a section-only export intended for later patching.
#[derive(Debug)]
pub struct SectionExportOutcome {
    /// Section export payload.
    pub exported: ExportedSection,
    /// Optional workflow warning that callers may surface to users.
    pub warning: Option<SectionWorkflowWarning>,
}

/// Result of patching a single exported section back into a base HWPX package.
#[derive(Debug)]
pub struct SectionPatchOutcome {
    /// Patched HWPX bytes.
    pub bytes: Vec<u8>,
    /// Which section was patched.
    pub patched_section: usize,
    /// Total section count in the output package.
    pub sections: usize,
}

/// Non-fatal warning emitted while preparing a section export.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionWorkflowWarning {
    /// Preservation metadata could not be generated, so later preserving patch
    /// is unavailable until the section is re-exported successfully.
    PreservationMetadataUnavailable {
        /// Underlying detail from the preserving metadata builder.
        detail: String,
    },
}

impl SectionWorkflowWarning {
    /// Stable machine-readable warning code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::PreservationMetadataUnavailable { .. } => "PRESERVATION_METADATA_UNAVAILABLE",
        }
    }

    /// Human-readable warning message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::PreservationMetadataUnavailable { detail } => {
                format!("Preserving patch metadata unavailable: {detail}")
            }
        }
    }
}

/// Domain error for shared section export/patch workflows.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SectionWorkflowError {
    /// Base HWPX could not be decoded well enough to proceed.
    #[error("HWPX decode failed: {detail}")]
    Decode {
        /// Underlying decode detail.
        detail: String,
    },
    /// Requested section index is outside the document range.
    #[error("Section {requested} does not exist (document has {sections} sections)")]
    SectionOutOfRange {
        /// Requested section index.
        requested: usize,
        /// Total section count in the document.
        sections: usize,
    },
    /// Requested CLI/MCP section does not match the JSON payload section.
    #[error("Requested section {requested} but JSON contains section {actual} data")]
    SectionIndexMismatch {
        /// Caller-requested section index.
        requested: usize,
        /// Section index found in the JSON payload.
        actual: usize,
    },
    /// Preserve-first patch failed after orchestration-level validation.
    #[error("Preserving patch failed: {0}")]
    PreservingPatch(#[from] HwpxError),
}

impl SectionWorkflowError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Decode { .. } => "DECODE_FAILED",
            Self::SectionOutOfRange { .. } => "SECTION_OUT_OF_RANGE",
            Self::SectionIndexMismatch { .. } => "SECTION_INDEX_MISMATCH",
            Self::PreservingPatch(_) => "PATCH_FAILED",
        }
    }
}

impl HwpxPatcher {
    /// Export a single section plus optional preservation metadata for editing.
    pub fn export_section_for_edit(
        base_bytes: &[u8],
        section_idx: usize,
        include_styles: bool,
    ) -> Result<SectionExportOutcome, SectionWorkflowError> {
        let hwpx_doc = HwpxDecoder::decode(base_bytes)
            .map_err(|error| SectionWorkflowError::Decode { detail: error.to_string() })?;

        let section = hwpx_doc.document.sections().get(section_idx).cloned().ok_or(
            SectionWorkflowError::SectionOutOfRange {
                requested: section_idx,
                sections: hwpx_doc.document.sections().len(),
            },
        )?;

        let preservation =
            match HwpxPatcher::export_section_preservation(base_bytes, section_idx, &section) {
                Ok(metadata) => (Some(metadata), None),
                Err(error) => (
                    None,
                    Some(SectionWorkflowWarning::PreservationMetadataUnavailable {
                        detail: error.to_string(),
                    }),
                ),
            };

        let exported = ExportedSection {
            section_index: section_idx,
            section,
            styles: include_styles.then_some(hwpx_doc.style_store),
            preservation: preservation.0,
        };

        Ok(SectionExportOutcome { exported, warning: preservation.1 })
    }

    /// Apply a section export back onto a base HWPX package.
    pub fn patch_exported_section(
        base_bytes: &[u8],
        section_idx: usize,
        exported: &ExportedSection,
    ) -> Result<SectionPatchOutcome, SectionWorkflowError> {
        let section_count = PackageReader::new(base_bytes)
            .map_err(|error| SectionWorkflowError::Decode { detail: error.to_string() })?
            .section_count();

        if exported.section_index != section_idx {
            return Err(SectionWorkflowError::SectionIndexMismatch {
                requested: section_idx,
                actual: exported.section_index,
            });
        }

        if section_idx >= section_count {
            return Err(SectionWorkflowError::SectionOutOfRange {
                requested: section_idx,
                sections: section_count,
            });
        }

        let bytes = HwpxPatcher::patch_section_preserving(
            base_bytes,
            section_idx,
            &exported.section,
            exported.styles.as_ref(),
            exported.preservation.as_ref(),
        )?;

        Ok(SectionPatchOutcome { bytes, patched_section: section_idx, sections: section_count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::document::Document;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::Draft;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    use crate::{HwpxCharShape, HwpxEncoder, HwpxParaShape, HwpxResult, HwpxStyleStore};

    fn minimal_hwpx_bytes() -> HwpxResult<Vec<u8>> {
        let mut doc: Document<Draft> = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("hello", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::default(),
        ));
        let mut styles: HwpxStyleStore = HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(HwpxCharShape::default());
        styles.push_para_shape(HwpxParaShape::default());
        let validated = doc.validate()?;
        HwpxEncoder::encode(&validated, &styles, &ImageStore::new())
    }

    #[test]
    fn export_section_for_edit_embeds_preservation_when_available() {
        let bytes = minimal_hwpx_bytes().unwrap();
        let outcome = HwpxPatcher::export_section_for_edit(&bytes, 0, true).unwrap();
        assert!(outcome.warning.is_none());
        assert_eq!(outcome.exported.section_index, 0);
        assert!(outcome.exported.styles.is_some());
        assert!(outcome.exported.preservation.is_some());
    }

    #[test]
    fn patch_exported_section_rejects_index_mismatch() {
        let bytes = minimal_hwpx_bytes().unwrap();
        let mut exported = HwpxPatcher::export_section_for_edit(&bytes, 0, true).unwrap().exported;
        exported.section_index = 1;

        let error = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap_err();
        assert!(matches!(
            error,
            SectionWorkflowError::SectionIndexMismatch { requested: 0, actual: 1 }
        ));
    }

    #[test]
    fn patch_exported_section_prefers_index_mismatch_over_out_of_range() {
        let bytes = minimal_hwpx_bytes().unwrap();
        let exported = HwpxPatcher::export_section_for_edit(&bytes, 0, true).unwrap().exported;

        let error = HwpxPatcher::patch_exported_section(&bytes, 99, &exported).unwrap_err();
        assert!(matches!(
            error,
            SectionWorkflowError::SectionIndexMismatch { requested: 99, actual: 0 }
        ));
    }
}
