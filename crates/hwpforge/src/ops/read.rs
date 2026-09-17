//! Read-only projections: [`outline`], [`fields`] and [`read`].
//!
//! These three operations never change a document. They decode the input,
//! project a view of it, and hand back a wire DTO the library already owns.
//!
//! # Where the warnings went
//!
//! [`HwpxReader`] and [`HwpxFiller::list_fields`] decode internally and drop
//! the decoder's warning list, so every `warnings` field in this module is
//! empty today. The fields exist anyway, because the alternative — decoding a
//! second time just to collect warnings — would double the cost of every
//! query, and because adding the field later would change the wire shape the
//! Python stub declares. When the reader facade starts reporting its decode
//! warnings, these operations carry them without any caller-visible change.
//! [`fields`] is the one exception: its wire wrapper has no `warnings` key at
//! all (see [`FieldsMeta`]).
//!
//! # Argument validation
//!
//! [`read`] is the only operation here that can fail before the library is
//! reached: it takes four optional targets and the CLI's rules decide which
//! combinations are addressable. The rules and their messages are reproduced
//! verbatim from `hwpforge-bindings-cli/src/commands/read.rs`.

use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwpx::{
    DocumentOutline, FieldInfo, HwpxFiller, HwpxReader, ParagraphsView, TableView,
};
use serde::Serialize;

use super::{OpsError, OpsWarning};

// ── outline ─────────────────────────────────────────────────────

/// What [`outline`] returns: the navigation map plus any warnings.
#[derive(Debug)]
#[non_exhaustive]
pub struct OutlineOutput {
    /// Headings, tables, fields and bookmarks, with their locations.
    pub outline: DocumentOutline,
    /// Non-fatal diagnostics; always empty today (see the module docs).
    pub warnings: Vec<OpsWarning>,
}

/// The `outline` wire payload.
///
/// # Keys
///
/// `outline`, `warnings`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutlineMeta {
    /// The navigation map, exactly as the library serialises it.
    pub outline: DocumentOutline,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

impl OutlineOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> OutlineMeta {
        OutlineMeta {
            outline: self.outline.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Builds the document navigation map.
///
/// Reproduces `hwpforge outline` minus the file I/O: headings in body-flow
/// order, tables in the shared traversal order (so a table nested in a cell
/// gets its own ordinal), named click-here fields, and the first occurrence
/// of each bookmark.
///
/// `SectionOutline::tables` is recounted from the recursive inventory after
/// the per-section walk, so it always equals the number of `tables` entries
/// for that section — including tables nested in a cell. The sibling counts
/// (`paragraphs`, `images`, `charts`) stay body-flow counts;
/// [`inspect`](super::inspect::inspect) is the surface that reports those
/// deeply.
///
/// # Errors
///
/// [`OpsError::Decode`] (code `DECODE_FAILED`) when the bytes are not a
/// decodable HWPX package.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::read::outline;
///
/// let bytes = std::fs::read("document.hwpx")?;
/// for table in &outline(&bytes)?.outline.tables {
///     println!("table {} in section {}", table.ordinal, table.at.section);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn outline(hwpx: &[u8]) -> Result<OutlineOutput, OpsError> {
    let outline = HwpxReader::outline(hwpx).map_err(OpsError::decode)?;
    Ok(OutlineOutput { outline, warnings: Vec::new() })
}

// ── fields ──────────────────────────────────────────────────────

/// What [`fields`] returns: every click-here field, named or not.
#[derive(Debug)]
#[non_exhaustive]
pub struct FieldsOutput {
    /// Fields in document order, duplicates and unnamed ones kept.
    pub fields: Vec<FieldInfo>,
    /// Non-fatal diagnostics; always empty today (see the module docs).
    ///
    /// Deliberately **not** part of [`FieldsMeta`].
    pub warnings: Vec<OpsWarning>,
}

/// The `fields` wire payload.
///
/// # Keys
///
/// `fields` — and nothing else. This is the one operation in this module
/// whose wire shape has no `warnings` key, because the design's return table
/// lists `fields` among the operations that cannot warn. The Rust output
/// still carries a `warnings` list so that the type matches its siblings and
/// so a future decode-warning channel has somewhere to go without breaking
/// this key set.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FieldsMeta {
    /// Fields in document order.
    pub fields: Vec<FieldInfo>,
}

impl FieldsOutput {
    /// The wire shape of this result.
    ///
    /// Drops `warnings`; see [`FieldsMeta`] for why.
    #[must_use]
    pub fn meta(&self) -> FieldsMeta {
        FieldsMeta { fields: self.fields.clone() }
    }
}

/// Lists the click-here fields of a document.
///
/// Reproduces `hwpforge fields`. Unnamed fields are included — they cannot be
/// addressed by [`fill`](super::edit), and `FieldInfo::fillable` says so —
/// because leaving them out would make the list disagree with what a user
/// sees in 한글.
///
/// # Errors
///
/// [`OpsError::Decode`] (code `DECODE_FAILED`) when the bytes are not a
/// decodable HWPX package.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::read::fields;
///
/// let bytes = std::fs::read("form.hwpx")?;
/// let fillable = fields(&bytes)?.fields.into_iter().filter(|f| f.fillable).count();
/// println!("{fillable} fillable field(s)");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn fields(hwpx: &[u8]) -> Result<FieldsOutput, OpsError> {
    let fields = HwpxFiller::list_fields(hwpx).map_err(OpsError::decode)?;
    Ok(FieldsOutput { fields, warnings: Vec::new() })
}

// ── read ────────────────────────────────────────────────────────

/// Which part of the document [`read`] should project.
///
/// Exactly one of `section`, `table` and `field` must be set; `paras` narrows
/// a `section` read and is not a target of its own. Build with
/// [`ReadOptions::default`] and the `with_*` setters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReadOptions {
    /// Section index to read paragraphs from.
    pub section: Option<usize>,
    /// Paragraph range within `section`: `"A..B"` (inclusive) or a single
    /// `"N"`. Requires `section`.
    pub paras: Option<String>,
    /// Table ordinal to read, in the shared traversal order.
    pub table: Option<usize>,
    /// Field name to read; every field with that name is returned.
    pub field: Option<String>,
}

impl ReadOptions {
    /// Targets a section's paragraphs.
    #[must_use]
    pub fn with_section(mut self, section: usize) -> Self {
        self.section = Some(section);
        self
    }

    /// Narrows a section read to a paragraph range.
    #[must_use]
    pub fn with_paras(mut self, paras: impl Into<String>) -> Self {
        self.paras = Some(paras.into());
        self
    }

    /// Targets a table by ordinal.
    #[must_use]
    pub fn with_table(mut self, table: usize) -> Self {
        self.table = Some(table);
        self
    }

    /// Targets every field carrying this name.
    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// How many mutually exclusive targets are set.
    ///
    /// `paras` is not counted: it narrows a section read rather than
    /// selecting one.
    fn target_count(&self) -> usize {
        usize::from(self.section.is_some())
            + usize::from(self.table.is_some())
            + usize::from(self.field.is_some())
    }
}

/// What [`read`] returns: whichever projection the options asked for.
///
/// Exactly one of the three payloads is `Some`, decided by the options.
#[derive(Debug)]
#[non_exhaustive]
pub struct ReadOutput {
    /// Paragraph range view, when `section` was the target.
    pub paragraphs: Option<ParagraphsView>,
    /// Table grid view, when `table` was the target.
    pub table: Option<TableView>,
    /// Every field of the requested name, when `field` was the target.
    pub fields: Option<Vec<FieldInfo>>,
    /// Non-fatal diagnostics; always empty today (see the module docs).
    pub warnings: Vec<OpsWarning>,
}

/// The `read` wire payload.
///
/// # Keys
///
/// `paragraphs`, `table`, `fields`, `warnings` — **all four always present**.
/// The three payload keys are serialised as `null` when they are not the
/// requested target: `skip_serializing_if` is deliberately not used here, so
/// that the consumer's type stub can declare a fixed key set instead of four
/// optional ones.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ReadMeta {
    /// Paragraph range view, or `null`.
    pub paragraphs: Option<ParagraphsView>,
    /// Table grid view, or `null`.
    pub table: Option<TableView>,
    /// Fields of the requested name, or `null`.
    pub fields: Option<Vec<FieldInfo>>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

impl ReadOutput {
    /// The wire shape of this result.
    #[must_use]
    pub fn meta(&self) -> ReadMeta {
        ReadMeta {
            paragraphs: self.paragraphs.clone(),
            table: self.table.clone(),
            fields: self.fields.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Reads one addressable part of a document.
///
/// Reproduces `hwpforge read` minus the file I/O. The target rules are
/// checked before the document is decoded, in the CLI's order: the target
/// count first, then `paras` without `section`. A caller who passes two
/// targets *and* an unparsable range therefore sees the target error, not the
/// range error.
///
/// `field` returns **every** field carrying that name, not the first one, so
/// that a duplicate name is visible rather than silently resolved.
///
/// # Errors
///
/// - Code `READ_TARGET_REQUIRED` when the number of targets is not exactly
///   one, `READ_PARAS_WITHOUT_SECTION` when `paras` is set without `section`,
///   and `READ_PARAS_INVALID` when the range spec will not parse.
/// - [`OpsError::Read`] for what the library rejects: an out-of-range
///   section, ordinal or paragraph range, a table whose grid is not
///   addressable, or an unknown field name.
/// - [`OpsError::Decode`] (code `DECODE_FAILED`) when the bytes are not a
///   decodable HWPX package, surfaced through [`OpsError::Read`]'s codec
///   arm.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::read::{read, ReadOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = read(&bytes, &ReadOptions::default().with_section(0).with_paras("0..4"))?;
/// for paragraph in &out.paragraphs.expect("a section read returns paragraphs").paragraphs {
///     println!("[p{}] {}", paragraph.at.para, paragraph.text);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn read(hwpx: &[u8], opts: &ReadOptions) -> Result<ReadOutput, OpsError> {
    if opts.target_count() != 1 {
        return Err(rejected(
            OpsCode::ReadTargetRequired,
            "Pass exactly one of --section, --table, --field",
        ));
    }
    if opts.paras.is_some() && opts.section.is_none() {
        return Err(rejected(OpsCode::ReadParasWithoutSection, "--paras requires --section"));
    }

    if let Some(section) = opts.section {
        let range = opts.paras.as_deref().map(parse_paras).transpose()?;
        return Ok(ReadOutput {
            paragraphs: Some(HwpxReader::read_paragraphs(hwpx, section, range)?),
            table: None,
            fields: None,
            warnings: Vec::new(),
        });
    }

    if let Some(ordinal) = opts.table {
        return Ok(ReadOutput {
            paragraphs: None,
            table: Some(HwpxReader::read_table(hwpx, ordinal)?),
            fields: None,
            warnings: Vec::new(),
        });
    }

    let name = opts.field.as_deref().expect("target validation leaves field as the only target");
    Ok(ReadOutput {
        paragraphs: None,
        table: None,
        fields: Some(HwpxReader::read_field(hwpx, name)?),
        warnings: Vec::new(),
    })
}

/// Parses `"A..B"` (inclusive) or a single `"N"` into an inclusive pair.
///
/// `split_once("..")` is what the CLI uses, so `"1..2..3"` and `"..5"` are
/// rejected while surrounding whitespace is tolerated.
fn parse_paras(spec: &str) -> Result<(usize, usize), OpsError> {
    let parsed = match spec.split_once("..") {
        Some((from, to)) => from
            .trim()
            .parse::<usize>()
            .and_then(|from| to.trim().parse::<usize>().map(|to| (from, to)))
            .ok(),
        None => spec.trim().parse::<usize>().map(|n| (n, n)).ok(),
    };
    parsed.ok_or_else(|| {
        rejected(
            OpsCode::ReadParasInvalid,
            format!("Cannot parse --paras {spec:?}: use \"A..B\" (inclusive) or a single \"N\""),
        )
    })
}

/// Rejects caller arguments that the library never sees.
///
/// Every rejection here keeps the CLI's wording as well as its code, so a
/// frontend built on this operation prints what it prints today.
fn rejected(code: OpsCode, reason: impl Into<String>) -> OpsError {
    OpsError::Rejected { code, reason: reason.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_number_is_an_inclusive_point_range() {
        assert_eq!(parse_paras("3").expect("valid"), (3, 3));
        assert_eq!(parse_paras(" 3 ").expect("whitespace is trimmed"), (3, 3));
    }

    #[test]
    fn a_two_sided_range_keeps_both_ends() {
        assert_eq!(parse_paras("0..4").expect("valid"), (0, 4));
        assert_eq!(parse_paras(" 0 .. 4 ").expect("whitespace is trimmed"), (0, 4));
    }

    #[test]
    fn a_reversed_range_parses_and_is_the_library_s_problem() {
        // The CLI does not order-check either; `read_paragraphs` rejects it
        // with `READ_PARA_RANGE_INVALID`, which keeps one owner for the rule.
        assert_eq!(parse_paras("4..0").expect("parses"), (4, 0));
    }

    #[test]
    fn malformed_ranges_are_rejected_with_the_cli_message() {
        for spec in ["", "..5", "1..", "1..2..3", "a..b", "-1", "1.5", "0...4"] {
            let error = parse_paras(spec).expect_err("must reject");
            assert_eq!(error.code(), OpsCode::ReadParasInvalid, "{spec:?}");
            assert!(
                error.to_string().contains("use \"A..B\" (inclusive) or a single \"N\""),
                "{spec:?}: {error}"
            );
        }
    }

    #[test]
    fn paras_is_not_a_target() {
        let opts = ReadOptions::default().with_paras("0..4");

        assert_eq!(opts.target_count(), 0, "a range alone selects nothing");
        assert_eq!(ReadOptions::default().with_section(0).with_paras("0..4").target_count(), 1);
    }

    #[test]
    fn each_target_counts_once_and_they_add_up() {
        assert_eq!(ReadOptions::default().target_count(), 0);
        assert_eq!(ReadOptions::default().with_section(0).target_count(), 1);
        assert_eq!(ReadOptions::default().with_table(0).target_count(), 1);
        assert_eq!(ReadOptions::default().with_field("성명").target_count(), 1);
        assert_eq!(ReadOptions::default().with_section(0).with_table(0).target_count(), 2);
        assert_eq!(
            ReadOptions::default().with_section(0).with_table(0).with_field("성명").target_count(),
            3
        );
    }

    #[test]
    fn setters_replace_rather_than_accumulate() {
        let opts = ReadOptions::default().with_section(0).with_section(2);

        assert_eq!(opts.section, Some(2));
        assert_eq!(opts.target_count(), 1);
    }
}
