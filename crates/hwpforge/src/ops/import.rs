//! Office document import into HWPX.
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_docx::decode_docx;
use serde::{Deserialize, Serialize};

use super::{encode_registry::encode_import_registry, OpsError, OpsWarning};

/// Generated HWPX bytes and diagnostics in decode, project, encode order.
#[derive(Debug)]
#[non_exhaustive]
pub struct ImportOutput {
    /// Generated HWPX package.
    pub bytes: Vec<u8>,
    /// Number of document sections.
    pub sections: usize,
    /// Top-level body paragraphs summed across sections.
    pub paragraphs: usize,
    /// Producer diagnostics, with their original aggregation and order.
    pub warnings: Vec<OpsWarning>,
}

impl ImportOutput {
    /// Copies metadata without regrouping warnings or changing their counts.
    #[must_use]
    pub fn meta(&self) -> ImportMeta {
        ImportMeta {
            sections: self.sections,
            paragraphs: self.paragraphs,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Serializable import report; package bytes travel separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ImportMeta {
    /// Number of document sections.
    pub sections: usize,
    /// Top-level body paragraphs summed across sections.
    pub paragraphs: usize,
    /// Diagnostics with producer-owned counts and first locations.
    pub warnings: Vec<WarningInfo>,
}

/// Converts DOCX package bytes into HWPX, preserving effective source styles.
///
/// Input is identified by package contents; filenames and extensions play no role.
/// No Markdown preset substitution or filesystem access is performed.
///
/// # Errors
///
/// Returns [`OpsError::DocxDecode`] for invalid or unsupported DOCX input,
/// including tracked changes, with the original location and recovery guidance.
/// Projection, rebinding, validation and encoding use the shared ops errors.
pub fn convert_docx(bytes: &[u8]) -> Result<ImportOutput, OpsError> {
    let decoded = decode_docx(bytes).map_err(OpsError::DocxDecode)?;
    let encoded =
        encode_import_registry(decoded.document, &decoded.registry, &decoded.image_store)?;
    let mut warnings: Vec<_> = decoded.warnings.into_iter().map(OpsWarning::Docx).collect();
    warnings.extend(encoded.bridge_warnings.into_iter().map(OpsWarning::Projection));
    warnings.extend(encoded.encode_warnings.into_iter().map(OpsWarning::Encode));
    Ok(ImportOutput {
        bytes: encoded.bytes,
        sections: encoded.sections,
        paragraphs: encoded.paragraphs,
        warnings,
    })
}
