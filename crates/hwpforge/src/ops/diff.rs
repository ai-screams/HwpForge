//! `diff` — what actually changed between two HWPX packages.
//!
//! Two channels, both reported: the **semantic** channel compares the decoded
//! Core documents (fields, cells, paragraphs, structure), and the **package**
//! channel compares ZIP entries. A change only the package channel sees means
//! the bytes moved without the meaning moving, which is what a re-encode
//! looks like.
//!
//! This is a query, so it never emits bytes and never edits. Both inputs are
//! decoded, and both decodes report — see [`DiffOutput::warnings`] for the
//! order.

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{DocumentDiff, HwpxDiffer};
use serde::Serialize;

use super::{OpsError, OpsWarning};

/// What [`diff`] returns: the two-channel report plus any warnings.
#[derive(Debug)]
#[non_exhaustive]
pub struct DiffOutput {
    /// The semantic and package channels, and whether they found nothing.
    pub diff: DocumentDiff,
    /// Decoder warnings from **both** inputs: every warning from `base`
    /// first, then every warning from `revised`, each group in decoder
    /// order.
    ///
    /// A diff decodes two documents, so the list needs a stated provenance
    /// rule or a caller cannot tell which document a warning belongs to.
    /// Base-then-revised matches the argument order, which is the only
    /// ordering a caller can predict without reading the warnings.
    pub warnings: Vec<OpsWarning>,
}

/// The `diff` wire payload: [`DocumentDiff`]'s own keys, plus `warnings`.
///
/// # Keys
///
/// `identical`, `note`, `semantic`, `package`, `warnings`.
///
/// The report is **flattened** rather than nested under a `diff` key, because
/// the design's return table names `DocumentDiff` itself as the payload. The
/// wrapper exists only to carry `warnings`, which must not be added to
/// `DocumentDiff` — that DTO's schema is shared with the CLI's `--output`
/// file and the MCP tool response.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct DiffMeta {
    /// The diff report, flattened into this object.
    #[serde(flatten)]
    pub diff: DocumentDiff,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

impl DiffOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> DiffMeta {
        DiffMeta {
            diff: self.diff.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Diffs `base` against `revised`.
///
/// Reproduces `hwpforge diff` minus the file I/O and the optional report
/// file. Both inputs are decoded, so both must be valid HWPX packages;
/// comparing a document with itself is the cheapest way to confirm that an
/// edit pipeline is byte-stable.
///
/// The unclassified (`semantic.raw`) list is capped by the library, and
/// `semantic.raw_dropped` says how many changes the cap hid — a caller that
/// reports raw changes must report that count too, or it will claim a
/// smaller diff than the one that happened.
///
/// # Errors
///
/// [`OpsError::Decode`] (code `DECODE_FAILED`) when either input is not a
/// decodable HWPX package. The error does not say which one; the CLI does not
/// either.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::diff::diff;
///
/// let before = std::fs::read("before.hwpx")?;
/// let after = std::fs::read("after.hwpx")?;
/// let out = diff(&before, &after)?;
/// if !out.diff.identical {
///     for change in &out.diff.semantic.field_values {
///         println!("{}: {:?} -> {:?}", change.name, change.before, change.after);
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn diff(base: &[u8], revised: &[u8]) -> Result<DiffOutput, OpsError> {
    let diagnosed = HwpxDiffer::diff_with_diagnostics(base, revised).map_err(OpsError::decode)?;

    // Base first, then revised — the documented provenance rule.
    let warnings: Vec<OpsWarning> = diagnosed
        .base_warnings
        .into_iter()
        .chain(diagnosed.revised_warnings)
        .map(OpsWarning::Decode)
        .collect();

    Ok(DiffOutput { diff: diagnosed.diff, warnings })
}
