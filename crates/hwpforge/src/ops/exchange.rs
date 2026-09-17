//! The JSON round trip: [`to_json`], [`export_section`], [`from_json`] and
//! [`patch`].
//!
//! # Two shapes of the same tree
//!
//! An export is not simply `serde_json::to_value` of the typed tree. Table
//! cells carry **grid addresses** (`addr`), which are projected onto the JSON
//! after serialisation by `grid_addr::annotate_*`, because they are derived
//! from the table's span layout rather than stored in it. [`from_json`] and
//! [`patch`] verify those addresses on the way back in: absent means no
//! check, present and wrong means the caller edited against a stale export.
//!
//! So every export here returns **both** shapes. `exported` is the typed tree
//! for a Rust caller; `document` / `section` is the annotated JSON, which is
//! what the wire payload carries and what the import side expects to be
//! handed back.
//!
//! # Which way the edits go
//!
//! [`from_json`] **generates** a package from JSON: there is no original to
//! preserve, so encode warnings are reported rather than fatal. [`patch`]
//! **preserves**: it rewrites one section's text inside the existing package
//! and leaves every other byte alone, so it has no encode stage and no
//! encode warnings.

use hwpforge_core::image::ImageStore;
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::grid_addr::{
    annotate_document_addresses, annotate_section_addresses, verify_document_addresses,
    verify_section_addresses,
};
use hwpforge_smithy_hwpx::{
    EncodeOptions, ExportedDocument, ExportedSection, HwpxDecoder, HwpxEncoder, HwpxPatcher,
    HwpxStyleStore, SectionPatchOutcome,
};
use serde::{Deserialize, Serialize};

use super::{OpsError, OpsWarning};

/// The font a generated document falls back to when the JSON carries no
/// style store, matching `hwpforge from-json`.
const FALLBACK_FONT: &str = "함초롬돋움";

// ── to_json ─────────────────────────────────────────────────────

/// Options for [`to_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToJsonOptions {
    /// Whether to include the document's style store in the export.
    ///
    /// Defaults to `true`. The CLI spells the same switch as the inverted
    /// `--no-styles`, so the two must not be confused: dropping the styles
    /// means a later [`from_json`] rebuilds them from
    /// [`FALLBACK_FONT`](self), which is lossy.
    pub styles: bool,
}

impl Default for ToJsonOptions {
    /// Includes styles.
    ///
    /// Written by hand rather than derived: a derived `Default` would give
    /// `false` and silently invert the CLI's behaviour.
    fn default() -> Self {
        Self { styles: true }
    }
}

impl ToJsonOptions {
    /// Includes (or omits) the style store.
    #[must_use]
    pub fn with_styles(mut self, styles: bool) -> Self {
        self.styles = styles;
        self
    }
}

/// What [`to_json`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct ToJsonOutput {
    /// The typed export tree, without grid addresses.
    pub exported: ExportedDocument,
    /// The same tree as JSON, **with** cell grid addresses annotated. This is
    /// the payload the wire form carries and what [`from_json`] verifies.
    pub document: serde_json::Value,
    /// Decode warnings from reading the package, then one
    /// [`OpsWarning::GridAddr`] per table that could not be given grid
    /// addresses (`TABLE_GRID_UNADDRESSABLE`).
    pub warnings: Vec<OpsWarning>,
}

/// The `to_json` wire payload.
///
/// # Keys
///
/// `document`, `warnings`. `document` is the **annotated** tree, so a
/// consumer that hands it straight back to [`from_json`] keeps the grid
/// addresses and therefore keeps the staleness check.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ToJsonMeta {
    /// The annotated export tree.
    pub document: serde_json::Value,
    /// Decode warnings and unaddressable tables, in that order.
    pub warnings: Vec<WarningInfo>,
}

impl ToJsonOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> ToJsonMeta {
        ToJsonMeta {
            document: self.document.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Exports a whole document to editable JSON.
///
/// Reproduces `hwpforge to-json` minus the file I/O and the pretty-printing:
/// decode, build the export tree, serialise it, then annotate cell grid
/// addresses on the serialised tree.
///
/// Unlike the CLI, decode warnings are reported instead of dropped. The CLI
/// prints only the grid-address warnings today; both now arrive in the same
/// list.
///
/// # Errors
///
/// - [`OpsError::Decode`] (code `DECODE_FAILED`) when the bytes are not a
///   decodable HWPX package.
/// - [`OpsError::JsonSerialize`] (code `JSON_SERIALIZE_FAILED`) when the
///   export tree cannot be serialised, which in practice means non-finite
///   numbers in chart data.
/// - [`OpsError::GridAddrProjection`] (code `GRID_ADDR_PROJECTION_FAILED`)
///   when address projection fails outright, as opposed to skipping one
///   unaddressable table, which is a warning.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::exchange::{to_json, ToJsonOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = to_json(&bytes, &ToJsonOptions::default())?;
/// std::fs::write("document.json", serde_json::to_string_pretty(&out.document)?)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn to_json(hwpx: &[u8], opts: &ToJsonOptions) -> Result<ToJsonOutput, OpsError> {
    let decoded = HwpxDecoder::decode(hwpx).map_err(OpsError::decode)?;
    let mut warnings: Vec<OpsWarning> =
        decoded.warnings.into_iter().map(OpsWarning::Decode).collect();
    let styles = opts.styles.then_some(decoded.style_store);
    let exported = ExportedDocument { document: decoded.document, styles };

    let mut document = serde_json::to_value(&exported).map_err(OpsError::json_serialize)?;
    let unaddressed = annotate_document_addresses(&mut document, &exported.document)
        .map_err(OpsError::grid_addr_projection)?;
    warnings.extend(unaddressed.into_iter().map(OpsWarning::GridAddr));

    Ok(ToJsonOutput { exported, document, warnings })
}

// ── export_section ──────────────────────────────────────────────

/// Options for [`export_section`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportSectionOptions {
    /// Which section to export, 0-based. Defaults to the first.
    pub section: usize,
    /// Whether to include the style store; defaults to `true`, as in
    /// [`ToJsonOptions`].
    pub styles: bool,
}

impl Default for ExportSectionOptions {
    /// The first section, styles included.
    fn default() -> Self {
        Self { section: 0, styles: true }
    }
}

impl ExportSectionOptions {
    /// Selects the section to export.
    #[must_use]
    pub fn with_section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    /// Includes (or omits) the style store.
    #[must_use]
    pub fn with_styles(mut self, styles: bool) -> Self {
        self.styles = styles;
        self
    }
}

/// What [`export_section`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct ExportSectionOutput {
    /// The typed section export, without grid addresses.
    pub exported: ExportedSection,
    /// The same section as JSON, **with** cell grid addresses annotated —
    /// the payload [`patch`] expects back.
    pub section: serde_json::Value,
    /// Decoder warnings first, then workflow warnings, then one
    /// [`OpsWarning::GridAddr`] per table that could not be given grid
    /// addresses.
    ///
    /// The order follows the pipeline: the decode happens first and
    /// describes the input, the workflow warning describes the export, and
    /// the grid warnings describe the annotation pass over the result. The
    /// workflow one is the warning that matters most —
    /// `PRESERVATION_METADATA_UNAVAILABLE` means the section cannot be
    /// patched back, only rebuilt.
    ///
    /// This is the same decode-warning channel [`to_json`] reports, so the
    /// two whole-document exports now agree.
    pub warnings: Vec<OpsWarning>,
}

/// The `export_section` wire payload.
///
/// # Keys
///
/// `section`, `warnings`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ExportSectionMeta {
    /// The annotated section export tree.
    pub section: serde_json::Value,
    /// Decoder warnings, then workflow warnings, then unaddressable tables.
    pub warnings: Vec<WarningInfo>,
}

impl ExportSectionOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> ExportSectionMeta {
        ExportSectionMeta {
            section: self.section.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Exports one section for later patching.
///
/// Reproduces the `--section` path of `hwpforge to-json`. The export carries
/// preservation metadata, which is what lets [`patch`] put the section back
/// without re-encoding the rest of the package; when that metadata cannot be
/// built the export still succeeds and says so with a
/// `PRESERVATION_METADATA_UNAVAILABLE` warning.
///
/// # Errors
///
/// - [`OpsError::SectionWorkflow`] for an undecodable package (code
///   `DECODE_FAILED`) or a section index outside the document (code
///   `SECTION_OUT_OF_RANGE`).
/// - [`OpsError::JsonSerialize`] when the section cannot be serialised.
/// - [`OpsError::GridAddrProjection`] when address projection fails
///   outright.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::exchange::{export_section, ExportSectionOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = export_section(&bytes, &ExportSectionOptions::default().with_section(1))?;
/// for warning in &out.warnings {
///     eprintln!("{}: {}", warning.info().code, warning.info().message);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn export_section(
    hwpx: &[u8],
    opts: &ExportSectionOptions,
) -> Result<ExportSectionOutput, OpsError> {
    let diagnosed =
        HwpxPatcher::export_section_for_edit_with_diagnostics(hwpx, opts.section, opts.styles)?;
    let outcome = diagnosed.value;
    let exported = outcome.exported;
    // Merge order (documented on `ExportSectionOutput::warnings`):
    // decoder → workflow → grid.
    let mut warnings: Vec<OpsWarning> =
        diagnosed.warnings.into_iter().map(OpsWarning::Decode).collect();
    warnings.extend(outcome.warning.into_iter().map(OpsWarning::SectionWorkflow));

    let mut section = serde_json::to_value(&exported).map_err(OpsError::json_serialize)?;
    let unaddressed =
        annotate_section_addresses(&mut section, &exported.section, exported.section_index)
            .map_err(OpsError::grid_addr_projection)?;
    warnings.extend(unaddressed.into_iter().map(OpsWarning::GridAddr));

    Ok(ExportSectionOutput { exported, section, warnings })
}

// ── from_json ───────────────────────────────────────────────────

/// Options for [`from_json`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FromJsonOptions {
    /// An existing package to inherit the image store from.
    ///
    /// JSON carries image *references*, never image bytes, so a document
    /// whose JSON mentions images produces a package with broken references
    /// unless the original package is supplied here.
    pub base: Option<Vec<u8>>,
}

impl FromJsonOptions {
    /// Inherits images from an existing package.
    #[must_use]
    pub fn with_base(mut self, base: impl Into<Vec<u8>>) -> Self {
        self.base = Some(base.into());
        self
    }
}

/// What [`from_json`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct FromJsonOutput {
    /// The generated HWPX package.
    pub bytes: Vec<u8>,
    /// Section count of the generated document.
    ///
    /// Not part of the wire payload; it is here because the document is
    /// consumed by the encode, so a caller that reports "N sections" cannot
    /// recover it afterwards without decoding the result again.
    pub sections: usize,
    /// Encode warnings. Generation has no original meaning to preserve, so
    /// these are reported, never fatal.
    pub warnings: Vec<OpsWarning>,
}

/// The wire payload of an operation whose only report is its warnings.
///
/// # Keys
///
/// `warnings`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct EncodeMeta {
    /// Encode warnings.
    pub warnings: Vec<WarningInfo>,
}

impl FromJsonOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> EncodeMeta {
        EncodeMeta { warnings: self.warnings.iter().map(OpsWarning::info).collect() }
    }
}

/// Builds an HWPX package from an exported JSON document.
///
/// Reproduces `hwpforge from-json` minus the file I/O: parse, verify any
/// supplied grid addresses, fall back to a default style store when the JSON
/// carries none, validate, inherit images from `base`, encode.
///
/// Supplied grid addresses are validated and then discarded. Their absence
/// means no check was asked for; a mismatch means the caller edited against a
/// stale export, which is refused rather than guessed at.
///
/// Unlike every edit in [`edit`](super::edit), this operation is **not**
/// fail-closed on semantic-loss warnings: there is no original document whose
/// meaning could be lost, so the warnings describe the generated output and
/// are returned with it.
///
/// # Errors
///
/// - [`OpsError::Json`] (code `JSON_PARSE_FAILED`) when the text is not JSON
///   or does not match the exported-document schema.
/// - [`OpsError::GridAddr`] (code `GRID_ADDR_INVALID`) when a supplied cell
///   address does not match the document.
/// - [`OpsError::Core`] (code `VALIDATION_FAILED`) when the document is
///   structurally invalid.
/// - [`OpsError::Decode`] when `base` is not a decodable HWPX package, and
///   [`OpsError::Encode`] when the package cannot be written.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::exchange::{from_json, FromJsonOptions};
///
/// let json = std::fs::read_to_string("document.json")?;
/// let base = std::fs::read("original.hwpx")?;
/// let out = from_json(&json, &FromJsonOptions::default().with_base(base))?;
/// std::fs::write("rebuilt.hwpx", &out.bytes)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn from_json(json: &str, opts: &FromJsonOptions) -> Result<FromJsonOutput, OpsError> {
    // Parsed once: the typed document deserialises from the tree by
    // reference, and the same tree carries the addresses to verify.
    let value: serde_json::Value = serde_json::from_str(json)?;
    let exported = ExportedDocument::deserialize(&value)?;

    verify_document_addresses(&value, &exported.document)?;

    let style_store =
        exported.styles.unwrap_or_else(|| HwpxStyleStore::with_default_fonts(FALLBACK_FONT));
    let validated = exported.document.validate()?;

    let image_store = match &opts.base {
        Some(base) => HwpxDecoder::decode(base).map_err(OpsError::decode)?.image_store,
        None => ImageStore::new(),
    };

    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        &style_store,
        &image_store,
        EncodeOptions::default(),
    )
    .map_err(OpsError::encode)?;

    Ok(FromJsonOutput {
        bytes: outcome.bytes,
        sections: validated.section_count(),
        warnings: outcome.warnings.into_iter().map(OpsWarning::Encode).collect(),
    })
}

// ── patch ───────────────────────────────────────────────────────

/// Options for [`patch`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatchOptions {
    /// Which section the patch replaces, 0-based.
    pub section: usize,
    /// The edited section JSON, as produced by [`export_section`].
    pub patch: String,
}

impl PatchOptions {
    /// Selects the section to replace.
    #[must_use]
    pub fn with_section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    /// Supplies the edited section JSON.
    #[must_use]
    pub fn with_patch(mut self, patch: impl Into<String>) -> Self {
        self.patch = patch.into();
        self
    }
}

/// What [`patch`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct PatchOutput {
    /// The patched HWPX package.
    pub bytes: Vec<u8>,
    /// Which section was replaced.
    pub section: usize,
    /// Section count of the output package, which a patch never changes.
    pub sections: usize,
    /// Decoder warnings for the base package, in decoder order.
    ///
    /// There are no *encode* warnings here and there never will be: a
    /// preserving patch splices one section's XML and leaves every other ZIP
    /// entry alone, so no encoder runs. The base is still decoded, to check
    /// the replacement against it, and that decode reports.
    pub warnings: Vec<OpsWarning>,
}

/// The `patch` wire payload.
///
/// # Keys
///
/// `section`, `warnings`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PatchMeta {
    /// Which section was replaced.
    pub section: usize,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

impl PatchOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> PatchMeta {
        PatchMeta {
            section: self.section,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Replaces one section of a package with an edited export.
///
/// Reproduces `hwpforge patch` minus the file I/O. This is a **preserving**
/// edit: the rest of the package keeps its original bytes, including the line
/// layout cache that a full re-encode would drop, so page breaks do not move.
///
/// The patch JSON must come from [`export_section`] of the same document.
/// Its grid addresses are verified before anything is written, and its
/// `section_index` must agree with
/// [`PatchOptions::section`] — a mismatch means the caller is about to write
/// one section's content over another's.
///
/// # Errors
///
/// - [`OpsError::Json`] (code `JSON_PARSE_FAILED`) when the patch text is not
///   JSON or does not match the exported-section schema.
/// - [`OpsError::GridAddr`] (code `GRID_ADDR_INVALID`) when a supplied cell
///   address does not match the section.
/// - [`OpsError::SectionWorkflow`] for an undecodable base (`DECODE_FAILED`),
///   a section outside the document (`SECTION_OUT_OF_RANGE`), a section index
///   that disagrees with the JSON (`SECTION_INDEX_MISMATCH`), or a patch the
///   preservation metadata cannot apply (`PATCH_FAILED`).
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let exported = export_section(&bytes, &ExportSectionOptions::default())?;
/// let edited = serde_json::to_string(&exported.section)?;
/// let out = patch(&bytes, &PatchOptions::default().with_patch(edited))?;
/// std::fs::write("patched.hwpx", &out.bytes)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn patch(hwpx: &[u8], opts: &PatchOptions) -> Result<PatchOutput, OpsError> {
    let value: serde_json::Value = serde_json::from_str(&opts.patch)?;
    let exported = ExportedSection::deserialize(&value)?;

    verify_section_addresses(&value, &exported.section, exported.section_index)?;

    let diagnosed =
        HwpxPatcher::patch_exported_section_with_diagnostics(hwpx, opts.section, &exported)?;
    let SectionPatchOutcome { bytes, patched_section, sections } = diagnosed.value;

    Ok(PatchOutput {
        bytes,
        section: patched_section,
        sections,
        warnings: diagnosed.warnings.into_iter().map(OpsWarning::Decode).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_default_to_included_for_both_exports() {
        // A derived `Default` would give `false` here and quietly invert the
        // CLI's `--no-styles` switch.
        assert!(ToJsonOptions::default().styles);
        assert!(ExportSectionOptions::default().styles);
        assert_eq!(ExportSectionOptions::default().section, 0);
    }

    #[test]
    fn option_setters_replace_rather_than_accumulate() {
        assert!(!ToJsonOptions::default().with_styles(true).with_styles(false).styles);
        assert_eq!(ExportSectionOptions::default().with_section(1).with_section(4).section, 4);
        assert_eq!(PatchOptions::default().with_section(2).section, 2);
        assert_eq!(PatchOptions::default().with_patch("a").with_patch("b").patch, "b");
        assert_eq!(FromJsonOptions::default().with_base(vec![1, 2]).base, Some(vec![1, 2]));
    }

    #[test]
    fn a_patch_option_starts_empty_which_is_not_valid_json() {
        // `Default` cannot invent a section payload, so the empty default is
        // rejected by the parser rather than silently patching nothing.
        let error = patch(&[], &PatchOptions::default()).expect_err("must reject");

        assert_eq!(
            error.code(),
            hwpforge_foundation::diagnostics::OpsCode::JsonParseFailed,
            "{error}"
        );
    }

    // `TABLE_GRID_UNADDRESSABLE` used to be pinned here by constructing the
    // warning by hand, which proved the wording but never that an operation
    // can reach it. It is now exercised end to end — encode a ragged table,
    // run `to_json`, read the code and the message off `meta()` — in
    // `tests/ops_to_json.rs::a_ragged_table_warns_through_the_operation`.
}
