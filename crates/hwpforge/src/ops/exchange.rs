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

use hwpforge_core::caption::Caption;
use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::Table;
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::grid_addr::{
    annotate_document_addresses, annotate_section_addresses, verify_document_addresses,
    verify_section_addresses,
};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::{
    EncodeOptions, EncodeWarning, ExportedDocument, ExportedSection, HwpxDecoder, HwpxEncoder,
    HwpxPatcher, ParagraphPath, PathSeg, SectionPatchOutcome,
};
use serde::{Deserialize, Serialize};

use super::walk::{self, ControlDescent};
use super::{OpsError, OpsWarning};

// ── to_json ─────────────────────────────────────────────────────

/// Options for [`to_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToJsonOptions {
    /// Whether to include the document's style store in the export.
    ///
    /// Defaults to `true`. The CLI spells the same switch as the inverted
    /// `--no-styles`, so the two must not be confused: dropping the styles
    /// means a later [`from_json`] rebuilds them from the built-in
    /// `"default"` preset registry, which still loses whatever fonts,
    /// sizes or colours the original store carried.
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
    /// Total paragraph count across all sections of the generated document,
    /// counted before the encode consumes it (same reason as
    /// [`sections`](Self::sections)).
    pub paragraphs: usize,
    /// Encode warnings, then one `LAYOUT_CACHE_DROPPED` per section whose
    /// input JSON carried a non-empty layout cache that this encode did not
    /// re-emit. Generation has no original meaning to preserve, so these are
    /// reported, never fatal.
    pub warnings: Vec<OpsWarning>,
}

/// The wire payload of [`from_json`].
///
/// # Keys
///
/// `paragraphs`, `warnings`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct EncodeMeta {
    /// Total paragraph count across all sections of the generated document.
    pub paragraphs: usize,
    /// Encode warnings.
    pub warnings: Vec<WarningInfo>,
}

impl FromJsonOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> EncodeMeta {
        EncodeMeta {
            paragraphs: self.paragraphs,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Builds an HWPX package from an exported JSON document.
///
/// Reproduces `hwpforge from-json` minus the file I/O: parse, verify any
/// supplied grid addresses, fall back to the built-in `"default"` style
/// preset when the JSON carries none, validate, inherit images from `base`,
/// encode.
///
/// The fallback is the full `"default"` preset registry
/// ([`style_store_for_preset`]) — char shapes, paragraph shapes, styles and
/// border fills, not only a font — because a document whose paragraphs
/// reference shape indices needs those shapes to exist, not only a font to
/// draw them with. A fonts-only store leaves such references dangling.
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
/// # Warnings
///
/// Besides the encoder's own diagnostics, one `LAYOUT_CACHE_DROPPED` per
/// section whose JSON carried a non-empty layout cache — [`to_json`] exports
/// each paragraph's promoted `linesegarray` cache, but this function always
/// encodes with [`EncodeOptions::default`], whose `emit_layout_cache` stays
/// off (a stale cache after edits is worse than none). Without this warning
/// the round trip is silently render-degraded: 한글 recomputes layout on
/// open regardless, but `hwpforge`'s own PDF path needs the cache and would
/// otherwise fail later with no link back to this step.
///
/// The warning's `path` points at the *first* cached paragraph found,
/// walking every paragraph the encoder would serialize — including
/// paragraphs nested inside an image's caption, which
/// [`Section::for_each_paragraph`] deliberately does not visit (see
/// [`hwpforge_core::document::Document::for_each_paragraph_mut`]) but the
/// encoder drops that cache too, so a bare `section[i]` marker would miss
/// it silently. The one exception is a cache inside a master page: the
/// encoder itself keeps no path there (master pages never reach the
/// per-paragraph encode path that tracks one), so the warning still stops
/// at the bare section marker in that case.
///
/// # Errors
///
/// - [`OpsError::Json`] (code `JSON_PARSE_FAILED`) when the text is not JSON
///   or does not match the exported-document schema.
/// - [`OpsError::GridAddr`] (code `GRID_ADDR_INVALID`) when a supplied cell
///   address does not match the document.
/// - [`OpsError::PresetNotFound`] (code `PRESET_NOT_FOUND`) when the
///   built-in `"default"` preset cannot be built — not reachable today, kept
///   for the day the registry changes shape.
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

    // Counted before `validate()` consumes `exported.document` — the
    // validated tree carries no direct paragraph iterator of its own that
    // makes this cheaper to redo afterwards.
    let paragraphs: usize =
        exported.document.sections().iter().map(|section| section.paragraphs.len()).sum();

    // Same reason this must run before `validate()` consumes the tree: the
    // encode below always runs with `EncodeOptions::default` (emit off), so
    // a cache the input carried is dropped without a trace unless flagged
    // here. `first_layout_cache_path` walks every paragraph the encoder
    // would serialize (including image captions — see its doc), so one
    // warning per section that carries at least one non-empty cache, with
    // `path` naming the first such paragraph rather than just the section.
    let layout_cache_warnings: Vec<OpsWarning> = exported
        .document
        .sections()
        .iter()
        .enumerate()
        .filter_map(|(index, section)| {
            first_layout_cache_path(section, index).map(|path| {
                OpsWarning::Encode(EncodeWarning::LayoutCacheDropped {
                    path,
                    reason: "layout cache present in the JSON was not re-emitted — from_json \
                             encodes with emit_layout_cache off by design"
                        .to_string(),
                })
            })
        })
        .collect();

    let style_store = match exported.styles {
        Some(styles) => styles,
        None => style_store_for_preset("default")
            .ok_or_else(|| OpsError::PresetNotFound { name: "default".to_string() })?,
    };
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

    let mut warnings: Vec<OpsWarning> =
        outcome.warnings.into_iter().map(OpsWarning::Encode).collect();
    warnings.extend(layout_cache_warnings);

    Ok(FromJsonOutput {
        bytes: outcome.bytes,
        sections: validated.section_count(),
        paragraphs,
        warnings,
    })
}

// ── layout-cache path discovery (from_json warning) ──────────────

/// Finds the path of the first paragraph in `section` whose promoted
/// [`Paragraph::layout_cache`] is non-empty.
///
/// This walks every paragraph the HWPX encoder would serialize —
/// including paragraphs nested inside an image's [`Caption`], which
/// [`Section::for_each_paragraph`] deliberately does not visit (see
/// [`hwpforge_core::document::Document::for_each_paragraph_mut`] for that
/// documented gap). The segments used here (`BodyParagraph`, `Header`,
/// `TableCell`, `Caption`, `TextBox`, `NestedParagraph`, …) are the same
/// [`PathSeg`] vocabulary the encoder's own `EncodeSink` builds in
/// `hwpforge-smithy-hwpx::encoder`, so the path this returns reads the
/// same way a real encoder-side `LayoutCacheDropped` warning would.
///
/// One spot the encoder itself never path-tracks falls back to a bare
/// `section[i]` path instead of inventing a segment it never emits: master
/// pages (`build_masterpage_entries` never receives a sink). A memo's
/// `anchor_runs` get a similar fallback but one step deeper — the memo's own
/// path rather than the section's — because the encoder flattens
/// `anchor_runs` to plain text and never recurses into a non-text run there
/// (see [`first_in_control`]), yet a cache nested inside one is still real
/// and still dropped, so it must still be found, just reported at the
/// nearest real container instead of a path the encoder never builds.
fn first_layout_cache_path(section: &Section, section_index: usize) -> Option<ParagraphPath> {
    let base = vec![PathSeg::Section(section_index)];

    first_in_paragraphs(&section.paragraphs, &base, PathSeg::BodyParagraph)
        .or_else(|| {
            section.headers.iter().enumerate().find_map(|(i, hf)| {
                let mut path = base.clone();
                path.push(PathSeg::Header(i));
                first_in_paragraphs(&hf.paragraphs, &path, PathSeg::NestedParagraph)
            })
        })
        .or_else(|| {
            section.footers.iter().enumerate().find_map(|(i, hf)| {
                let mut path = base.clone();
                path.push(PathSeg::Footer(i));
                first_in_paragraphs(&hf.paragraphs, &path, PathSeg::NestedParagraph)
            })
        })
        .or_else(|| {
            // The encoder never path-tracks master pages (see this
            // function's doc), so there is no real segment to append here
            // — but the *detection* must still recurse into runs (table
            // cells, captions, …), not just check each top-level paragraph
            // directly, or a cache nested one level deeper than before
            // would stop being caught at all (a real narrowing, not just
            // an imprecise path).
            let carries_cache = section.master_pages.iter().flatten().any(|mp| {
                mp.paragraphs.iter().any(|p| first_in_paragraph(p, base.clone()).is_some())
            });
            carries_cache.then(|| ParagraphPath(base.clone()))
        })
}

/// Whether `p` itself carries a non-empty promoted layout cache.
fn paragraph_has_cache(p: &Paragraph) -> bool {
    p.layout_cache.as_ref().is_some_and(|c| !c.is_empty())
}

/// Finds the path of the first cached paragraph in `paragraphs`, appending
/// `seg(index)` to `prefix` for each one tried — callers pass
/// [`PathSeg::BodyParagraph`] for a section's top-level paragraphs and
/// [`PathSeg::NestedParagraph`] for every other container, matching
/// `hwpforge-smithy-hwpx::encoder::section::build_sublist`.
fn first_in_paragraphs(
    paragraphs: &[Paragraph],
    prefix: &[PathSeg],
    seg: impl Fn(usize) -> PathSeg,
) -> Option<ParagraphPath> {
    paragraphs.iter().enumerate().find_map(|(idx, p)| {
        let mut path = prefix.to_vec();
        path.push(seg(idx));
        first_in_paragraph(p, path)
    })
}

/// Checks `p` itself, then recurses into its runs — pre-order, matching
/// [`Paragraph::for_each_paragraph`].
fn first_in_paragraph(p: &Paragraph, path: Vec<PathSeg>) -> Option<ParagraphPath> {
    if paragraph_has_cache(p) {
        return Some(ParagraphPath(path));
    }
    p.runs.iter().find_map(|run| first_in_run(run, &path))
}

/// Recurses into a run's content: tables and controls the encoder itself
/// recurses into, plus [`RunContent::Image`], which it does not (the gap
/// this whole module exists to close for the warning's purposes).
fn first_in_run(run: &Run, path: &[PathSeg]) -> Option<ParagraphPath> {
    match &run.content {
        RunContent::Table(table) => first_in_table(table, path),
        RunContent::Control(control) => first_in_control(control, path),
        RunContent::Image(image) => first_in_caption(image.caption.as_ref(), path),
        RunContent::Text(_) | RunContent::InlineText(_) => None,
        // `RunContent` is `#[non_exhaustive]` too, so this crate's match
        // needs a wildcard regardless of Core's own coverage — a future
        // variant that carries paragraphs would need a hand-added arm here.
        _ => None,
    }
}

/// Row → cell → cell paragraphs (nested tables recurse via
/// [`first_in_paragraphs`] → [`first_in_paragraph`] → [`first_in_run`]),
/// then the table's own caption.
fn first_in_table(table: &Table, path: &[PathSeg]) -> Option<ParagraphPath> {
    table
        .rows
        .iter()
        .enumerate()
        .find_map(|(row_idx, row)| {
            row.cells.iter().enumerate().find_map(|(cell_idx, cell)| {
                let mut cell_path = path.to_vec();
                cell_path.push(PathSeg::TableCell { row: row_idx, cell: cell_idx });
                first_in_paragraphs(&cell.paragraphs, &cell_path, PathSeg::NestedParagraph)
            })
        })
        .or_else(|| first_in_caption(table.caption.as_ref(), path))
}

/// A caption's own paragraphs, if it has one — shared by tables, shapes and
/// (via [`first_in_run`]) images.
fn first_in_caption(caption: Option<&Caption>, path: &[PathSeg]) -> Option<ParagraphPath> {
    let caption = caption?;
    let mut cap_path = path.to_vec();
    cap_path.push(PathSeg::Caption);
    first_in_paragraphs(&caption.paragraphs, &cap_path, PathSeg::NestedParagraph)
}

/// Recurses into the paragraph-bearing [`Control`] variants — shape body
/// text and captions, footnotes/endnotes, group children, and a memo's
/// visible body content and anchor.
///
/// The *recursion structure* — which nested paragraph lists each variant
/// exposes — comes from [`walk::control_descent`], the same policy
/// [`walk::object_counts`] builds on, so the two cannot drift into two
/// independently hand-maintained copies of the same "which containers does
/// this walk reach" answer. This function still supplies its own `PathSeg`
/// tag per arm (`control_descent` erases which concrete variant it came
/// from, since counting does not need to know) and its own group-child
/// `emitted_idx` skip logic (deliberately *not* shared — see
/// [`ControlDescent::Group`]'s doc for why the two callers' policies there
/// genuinely differ).
fn first_in_control(control: &Control, path: &[PathSeg]) -> Option<ParagraphPath> {
    match walk::control_descent(control) {
        ControlDescent::Body { paragraphs, caption } => {
            let mut tb_path = path.to_vec();
            tb_path.push(PathSeg::TextBox);
            first_in_paragraphs(paragraphs, &tb_path, PathSeg::NestedParagraph)
                .or_else(|| first_in_caption(caption, path))
        }
        ControlDescent::Footnote(paragraphs) => {
            let mut note_path = path.to_vec();
            note_path.push(PathSeg::Footnote);
            first_in_paragraphs(paragraphs, &note_path, PathSeg::NestedParagraph)
        }
        ControlDescent::Endnote(paragraphs) => {
            let mut note_path = path.to_vec();
            note_path.push(PathSeg::Endnote);
            first_in_paragraphs(paragraphs, &note_path, PathSeg::NestedParagraph)
        }
        ControlDescent::CaptionOnly(caption) => first_in_caption(caption, path),
        ControlDescent::Group(children) => {
            // `emitted_idx` mirrors `encode_group_child_xml`'s own counter
            // exactly (`hwpforge-smithy-hwpx::encoder::shapes`): it advances
            // only for a child that counter actually serializes, so a
            // dropped child (`Equation`/`EmbeddedChart`/… — anything outside
            // [`group_child_is_emitted`]) never consumes a `GroupChild`
            // index and the next emitted child reuses it, exactly as the
            // real encoder's `sink.enter(PathSeg::GroupChild(emitted_idx))`
            // does before deciding whether to bump it.
            let mut emitted_idx = 0usize;
            children.iter().find_map(|child| {
                let mut child_path = path.to_vec();
                child_path.push(PathSeg::GroupChild(emitted_idx));
                let found = first_in_control(child, &child_path);
                if group_child_is_emitted(child) {
                    emitted_idx += 1;
                }
                found
            })
        }
        ControlDescent::Memo { content, anchor_runs } => {
            let mut memo_path = path.to_vec();
            memo_path.push(PathSeg::Memo);
            first_in_paragraphs(content, &memo_path, PathSeg::NestedParagraph).or_else(|| {
                // The encoder flattens `anchor_runs` into a single inline
                // `<hp:t>` anchor, keeping only `RunContent::plain_text`
                // (`build_memo_anchor_xml`) — a `Table`/`Control`/`Image`
                // run there is dropped whole and never reaches the
                // encoder's own path-tracked recursion, so there is no real
                // per-path segment to report for whatever is nested inside
                // it either. `first_in_run` is reused only to *detect* a
                // cache there (same descent as everywhere else); the path
                // it would have built is discarded in favour of the memo's
                // own path, the nearest real container — the same fallback
                // shape master pages use above, one level deeper.
                anchor_runs
                    .iter()
                    .any(|run| first_in_run(run, &memo_path).is_some())
                    .then(|| ParagraphPath(memo_path.clone()))
            })
        }
        ControlDescent::None => None,
    }
}

/// Whether `encode_group_child_xml` (`hwpforge-smithy-hwpx::encoder::shapes`)
/// emits `child` as a container child at all — the exact set its match
/// covers, including the nested-`Group` branch it checks ahead of that
/// match. Anything outside this set (`Equation`, `EmbeddedChart`, `Footnote`,
/// `Memo`, …) is dropped rather than fabricated (Wave A group support), so
/// it never advances the encoder's own `emitted_idx` counter — see
/// [`first_in_control`]'s `Control::Group` arm above.
fn group_child_is_emitted(child: &Control) -> bool {
    matches!(
        child,
        Control::TextBox { .. }
            | Control::Rect { .. }
            | Control::Line { .. }
            | Control::Ellipse { .. }
            | Control::Arc { .. }
            | Control::Polygon { .. }
            | Control::Curve { .. }
            | Control::ConnectLine { .. }
            | Control::TextArt { .. }
            | Control::Group { .. }
    )
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
