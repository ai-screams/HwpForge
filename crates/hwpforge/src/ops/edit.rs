//! `fill` · `set_cell` · `insert_para` · `delete_para` — the four edits that
//! change a document without rewriting the package.
//!
//! # Preservation and fail-closed behaviour
//!
//! `fill`, `insert_para` and `delete_para` are **preserving** edits: they
//! splice the affected section XML and leave every other ZIP entry byte for
//! byte, so there is no re-encode that could lose meaning. `set_cell` is a
//! **regenerating** edit — it rebuilds the package — and therefore runs
//! behind an admission gate and refuses to emit bytes when the encode
//! reports semantic loss (code `ENCODE_SEMANTIC_LOSS`). That refusal is
//! implemented in `smithy-hwpx`
//! ([`CellEditError::SemanticLoss`](hwpforge_smithy_hwpx::CellEditError::SemanticLoss)),
//! not here.
//!
//! # Warnings
//!
//! Only [`delete_para`] reports any today, through the library's advisory
//! scan: deleting a paragraph takes its index-mark entries out of the
//! document index, which is intended but worth saying out loud.
//!
//! The other three have no warning channel to pass through.
//! [`FillOutcome`](hwpforge_smithy_hwpx::FillOutcome) and
//! [`SetCellOutcome`](hwpforge_smithy_hwpx::SetCellOutcome) document that
//! adding one is a public-field (semver) change still awaiting approval, and
//! `insert_paragraphs` returns bare bytes. Their `warnings` field is part of
//! the operation contract and is populated the moment the library grows the
//! channel; until then those edits either fail closed or succeed silently.

use std::collections::BTreeMap;

use hwpforge_core::table::grid::GridCoord;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwpx::{
    scan_delete_warnings, CellSpec, CellTarget, FilledField, HwpxCellEditor, HwpxFiller,
    HwpxStructuralEditor, InsertPosition, ParagraphLocator, SetCellResult,
};
use serde::{Deserialize, Serialize};

use super::{OpsError, OpsWarning};

/// Builds the rejection for an argument the library never gets to see.
///
/// The frontends validate a few arguments themselves (an empty value map,
/// two mutually exclusive cell targets). Each of those keeps its own stable
/// code and the frontend's sentence.
fn rejected(code: OpsCode, reason: impl Into<String>) -> OpsError {
    OpsError::Rejected { code, reason: reason.into() }
}

// ── fill ────────────────────────────────────────────────────────

/// Options for [`fill`].
///
/// The operation takes none today. The struct exists so that the call site
/// has a place for the first one, and destructuring it inside [`fill`] makes
/// adding a field a compile error there rather than a silently ignored
/// argument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FillOptions {}

/// What [`fill`] returns: the filled package plus what it filled.
#[derive(Debug)]
#[non_exhaustive]
pub struct FillOutput {
    /// The filled HWPX package. Untouched entries are byte-identical.
    pub bytes: Vec<u8>,
    /// The fields that were filled, in document order.
    pub filled: Vec<FilledField>,
    /// Non-fatal diagnostics. Always empty — see the module docs.
    pub warnings: Vec<OpsWarning>,
}

impl FillOutput {
    /// The serialisable metadata of this fill (everything but the bytes).
    #[must_use]
    pub fn meta(&self) -> FillMeta {
        FillMeta {
            filled: self.filled.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Wire shape of a [`FillOutput`] minus its bytes.
///
/// Deriving `Deserialize` is not possible: the payload
/// [`FilledField`] is `Serialize`-only upstream.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FillMeta {
    /// The fields that were filled, in document order.
    pub filled: Vec<FilledField>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Fills named click-here fields with values, all-or-nothing.
///
/// `values` is a `name → value` list; it is turned into the map the library
/// takes, so a name may appear only once. Nothing is written to disk.
///
/// # Errors
///
/// - [`OpsError::InvalidInput`] when `values` is empty (the frontends call
///   this `NO_VALUES`) or names a field twice.
/// - [`OpsError::Fill`] for every library rejection: an unknown name
///   (`FIELD_NOT_FOUND`), an empty value (`EMPTY_FIELD_VALUE`), a duplicated
///   field name in the *document* (`FIELD_NAME_AMBIGUOUS`), or a field with
///   no patchable body (`FIELD_NOT_FILLABLE`).
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::edit::{fill, FillOptions};
///
/// let bytes = std::fs::read("template.hwpx")?;
/// let values = [("과제명".to_string(), "AI 문서 자동화".to_string())];
/// let out = fill(&bytes, &values, &FillOptions::default())?;
/// assert_eq!(out.filled.len(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn fill(
    hwpx: &[u8],
    values: &[(String, String)],
    opts: &FillOptions,
) -> Result<FillOutput, OpsError> {
    let FillOptions {} = opts;
    if values.is_empty() {
        return Err(rejected(OpsCode::NoValues, "no field values given"));
    }

    let mut map = BTreeMap::new();
    for (name, value) in values {
        if map.insert(name.clone(), value.clone()).is_some() {
            // The CLI's own `DUPLICATE_SET` covers repeated `--set` flags,
            // which is an argument-parsing concern of that frontend; a
            // repeated name in an ops call has no dedicated code.
            return Err(OpsError::InvalidInput {
                reason: format!("field '{name}' given more than once"),
            });
        }
    }

    let outcome = HwpxFiller::fill(hwpx, &map)?;
    Ok(FillOutput { bytes: outcome.bytes, filled: outcome.filled, warnings: Vec::new() })
}

// ── set_cell ────────────────────────────────────────────────────

/// Options for [`set_cell`]: one target, or a batch of them.
///
/// Either describe a single cell with [`table`](Self::table),
/// [`text`](Self::text) and exactly one of [`at`](Self::at),
/// [`right_of`](Self::right_of) or [`below`](Self::below), **or** pass a
/// [`specs`](Self::specs) batch. Mixing the two is rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SetCellOptions {
    /// Table ordinal in the shared traversal order (the `to_json` order).
    pub table: Option<usize>,
    /// Grid coordinate as `"row,col"`; covered positions resolve to their
    /// merge anchor.
    pub at: Option<String>,
    /// Label of the cell immediately left of the target.
    pub right_of: Option<String>,
    /// Label of the cell immediately above the target.
    pub below: Option<String>,
    /// Replacement text; the empty string clears the cell.
    pub text: Option<String>,
    /// A batch of targets, mutually exclusive with the single-target fields.
    pub specs: Option<Vec<CellSpec>>,
}

impl SetCellOptions {
    /// Sets the table ordinal of the single target.
    #[must_use]
    pub fn with_table(mut self, table: usize) -> Self {
        self.table = Some(table);
        self
    }

    /// Addresses the single target by `"row,col"`.
    #[must_use]
    pub fn with_at(mut self, at: impl Into<String>) -> Self {
        self.at = Some(at.into());
        self
    }

    /// Addresses the single target as the neighbour right of `label`.
    #[must_use]
    pub fn with_right_of(mut self, label: impl Into<String>) -> Self {
        self.right_of = Some(label.into());
        self
    }

    /// Addresses the single target as the neighbour below `label`.
    #[must_use]
    pub fn with_below(mut self, label: impl Into<String>) -> Self {
        self.below = Some(label.into());
        self
    }

    /// Sets the replacement text of the single target.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Replaces the single target with a batch of specs.
    #[must_use]
    pub fn with_specs(mut self, specs: Vec<CellSpec>) -> Self {
        self.specs = Some(specs);
        self
    }
}

/// What [`set_cell`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct SetCellOutput {
    /// The edited HWPX package.
    pub bytes: Vec<u8>,
    /// One record per applied edit, in spec order.
    pub results: Vec<SetCellResult>,
    /// Non-fatal diagnostics. Always empty — see the module docs.
    pub warnings: Vec<OpsWarning>,
}

impl SetCellOutput {
    /// The serialisable metadata of this edit (everything but the bytes).
    #[must_use]
    pub fn meta(&self) -> SetCellMeta {
        SetCellMeta {
            results: self.results.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Wire shape of a [`SetCellOutput`] minus its bytes.
///
/// Neither `Deserialize` nor `JsonSchema` is derived: the payload
/// [`SetCellResult`] is `Serialize`-only upstream and has no `schemars`
/// derive.
#[derive(Debug, Clone, Serialize)]
pub struct SetCellMeta {
    /// One record per applied edit, in spec order.
    pub results: Vec<SetCellResult>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Replaces the text of grid-addressed table cells, all-or-nothing.
///
/// This is a regenerating edit: the whole package is rebuilt behind the
/// admission gate, and an encode that loses meaning produces no bytes.
///
/// # Errors
///
/// - [`OpsError::InvalidInput`] for an unusable option combination (the
///   frontends call these `INVALID_SET_CELL_ARGS` and, for an empty batch,
///   `INVALID_SET_CELL_MAP`).
/// - [`OpsError::CellEdit`] for every library rejection, including
///   `TABLE_NOT_FOUND`, `CELL_LABEL_AMBIGUOUS`,
///   `CELL_HAS_NON_TEXT_CONTENT`, the admission refusals
///   (`INPUT_NOT_ROUNDTRIP_SAFE`, `INPUT_ENTRIES_NOT_CARRIED`) and the
///   fail-closed `ENCODE_SEMANTIC_LOSS`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::edit::{set_cell, SetCellOptions};
///
/// let bytes = std::fs::read("form.hwpx")?;
/// let opts = SetCellOptions::default().with_table(0).with_at("1,2").with_text("2026-09-17");
/// let out = set_cell(&bytes, &opts)?;
/// assert_eq!(out.results.len(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn set_cell(hwpx: &[u8], opts: &SetCellOptions) -> Result<SetCellOutput, OpsError> {
    let specs = build_specs(opts)?;
    let result = HwpxCellEditor::set_cells(hwpx, &specs)?;
    Ok(SetCellOutput { bytes: result.bytes, results: result.outcome.cells, warnings: Vec::new() })
}

/// Turns the options into the spec list the library takes.
fn build_specs(opts: &SetCellOptions) -> Result<Vec<CellSpec>, OpsError> {
    let has_single = opts.table.is_some()
        || opts.at.is_some()
        || opts.right_of.is_some()
        || opts.below.is_some()
        || opts.text.is_some();

    if let Some(specs) = &opts.specs {
        if has_single {
            return Err(rejected(
                OpsCode::InvalidSetCellArgs,
                "specs cannot be combined with table/at/right_of/below/text",
            ));
        }
        if specs.is_empty() {
            return Err(rejected(OpsCode::InvalidSetCellMap, "spec list is empty"));
        }
        return Ok(specs.clone());
    }

    let Some(table) = opts.table else {
        return Err(rejected(OpsCode::InvalidSetCellArgs, "table is required (or pass specs)"));
    };
    let Some(text) = opts.text.as_deref() else {
        return Err(rejected(
            OpsCode::InvalidSetCellArgs,
            "text is required (the empty string clears the cell)",
        ));
    };
    let target = match (opts.at.as_deref(), opts.right_of.as_deref(), opts.below.as_deref()) {
        (Some(at), None, None) => CellTarget::At(parse_coord(at)?),
        (None, Some(label), None) => CellTarget::RightOf(label.to_owned()),
        (None, None, Some(label)) => CellTarget::Below(label.to_owned()),
        _ => {
            return Err(rejected(
                OpsCode::InvalidSetCellArgs,
                "exactly one of at / right_of / below is required",
            ))
        }
    };
    Ok(vec![CellSpec { table, target, text: text.to_owned() }])
}

/// Parses the `"row,col"` spelling the frontends accept.
///
/// The parse lives here rather than in each frontend so that the Python
/// bindings, the CLI and the MCP server cannot drift on what a coordinate
/// string means.
fn parse_coord(at: &str) -> Result<GridCoord, OpsError> {
    let parts: Vec<&str> = at.split(',').map(str::trim).collect();
    if let [row, col] = parts[..] {
        if let (Ok(row), Ok(col)) = (row.parse(), col.parse()) {
            return Ok(GridCoord::new(row, col));
        }
    }
    Err(rejected(OpsCode::InvalidSetCellArgs, format!("at expects \"row,col\", got '{at}'")))
}

// ── insert_para / delete_para ───────────────────────────────────

/// Options for [`insert_para`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InsertParaOptions {
    /// Zero-based section index.
    pub section: usize,
    /// Zero-based index of the anchor paragraph inside the section.
    pub anchor: usize,
    /// One plain-text paragraph per entry, inserted in order.
    pub text: Vec<String>,
    /// Insert before the anchor instead of after it.
    pub before: bool,
}

impl InsertParaOptions {
    /// Sets the section the anchor lives in.
    #[must_use]
    pub fn with_section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    /// Sets the anchor paragraph index.
    #[must_use]
    pub fn with_anchor(mut self, anchor: usize) -> Self {
        self.anchor = anchor;
        self
    }

    /// Sets the paragraph texts to insert.
    #[must_use]
    pub fn with_text(mut self, text: Vec<String>) -> Self {
        self.text = text;
        self
    }

    /// Inserts before the anchor instead of after it.
    #[must_use]
    pub fn with_before(mut self, before: bool) -> Self {
        self.before = before;
        self
    }
}

/// Options for [`delete_para`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeleteParaOptions {
    /// Zero-based section index.
    pub section: usize,
    /// Zero-based paragraph indexes to delete, resolved against one
    /// pristine snapshot (they do not shift as the batch applies).
    pub indexes: Vec<usize>,
}

impl DeleteParaOptions {
    /// Sets the section to delete from.
    #[must_use]
    pub fn with_section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    /// Sets the paragraph indexes to delete.
    #[must_use]
    pub fn with_indexes(mut self, indexes: Vec<usize>) -> Self {
        self.indexes = indexes;
        self
    }
}

/// What [`insert_para`] and [`delete_para`] return.
///
/// `inserted` and `deleted` echo the request: both library entry points
/// return bare bytes, and an edit is all-or-nothing, so the request count
/// *is* the applied count. The operation that did not run reports zero.
#[derive(Debug)]
#[non_exhaustive]
pub struct StructuralOutput {
    /// The edited HWPX package. Untouched entries are byte-identical.
    pub bytes: Vec<u8>,
    /// Paragraphs inserted; zero for [`delete_para`].
    pub inserted: usize,
    /// Paragraphs deleted; zero for [`insert_para`].
    pub deleted: usize,
    /// Advisory diagnostics about what the edit took with it. Only
    /// [`delete_para`] produces any; [`insert_para`] always reports none.
    pub warnings: Vec<OpsWarning>,
}

impl StructuralOutput {
    /// The serialisable metadata of this edit (everything but the bytes).
    #[must_use]
    pub fn meta(&self) -> StructuralMeta {
        StructuralMeta {
            inserted: self.inserted,
            deleted: self.deleted,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Wire shape of a [`StructuralOutput`] minus its bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StructuralMeta {
    /// Paragraphs inserted.
    pub inserted: usize,
    /// Paragraphs deleted.
    pub deleted: usize,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Inserts a block of plain-text paragraphs at one anchor.
///
/// Every inserted paragraph inherits the anchor's paragraph and character
/// shape, so no style is invented. Untouched paragraphs keep their bytes,
/// including Hancom's line-layout cache. Inserting warns about nothing: the
/// library has no advisory scan for it.
///
/// # Errors
///
/// - [`OpsError::InvalidInput`] when `text` is empty (the MCP server calls
///   this `INSERT_TEXT_REQUIRED`). The library treats an empty batch as a
///   byte-identical no-op, so this operation has to refuse it here.
/// - [`OpsError::StructuralEdit`] for every library rejection, including
///   `SECTION_OUT_OF_RANGE`, `PARAGRAPH_OUT_OF_RANGE`,
///   `MULTI_PARAGRAPH_TEXT` (a text containing a newline) and
///   `INSERT_BEFORE_SECTION_PROPERTIES`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::edit::{insert_para, InsertParaOptions};
///
/// let bytes = std::fs::read("report.hwpx")?;
/// let opts = InsertParaOptions::default().with_text(vec!["새 문단".to_string()]);
/// let out = insert_para(&bytes, &opts)?;
/// assert_eq!((out.inserted, out.deleted), (1, 0));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn insert_para(hwpx: &[u8], opts: &InsertParaOptions) -> Result<StructuralOutput, OpsError> {
    if opts.text.is_empty() {
        return Err(rejected(
            OpsCode::InsertTextRequired,
            "insert_para needs at least one paragraph text",
        ));
    }
    let position = if opts.before { InsertPosition::Before } else { InsertPosition::After };
    let anchor = ParagraphLocator { section: opts.section, index: opts.anchor };
    let bytes = HwpxStructuralEditor::insert_paragraphs(hwpx, anchor, position, &opts.text)?;
    Ok(StructuralOutput { bytes, inserted: opts.text.len(), deleted: 0, warnings: Vec::new() })
}

/// Deletes top-level paragraphs from one section, all-or-nothing.
///
/// Removing a paragraph also removes any index-mark entries it carried from
/// the document index. That is intended, not a refusal, so it comes back as
/// an `INDEX_MARK_REMOVED` warning beside the edited bytes.
///
/// # Errors
///
/// - [`OpsError::InvalidInput`] when `indexes` is empty (the frontends call
///   this `DELETE_NO_TARGET`). The library treats an empty target list as a
///   byte-identical no-op, so this operation has to refuse it here.
/// - [`OpsError::StructuralEdit`] for every library rejection, including
///   `SECTION_OUT_OF_RANGE`, `PARAGRAPH_OUT_OF_RANGE`, `DUPLICATE_TARGET`,
///   `REFERENCE_STRANDED`, `HARD_BREAK_LOSS` and `EMPTY_SECTION`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::edit::{delete_para, DeleteParaOptions};
///
/// let bytes = std::fs::read("report.hwpx")?;
/// let out = delete_para(&bytes, &DeleteParaOptions::default().with_indexes(vec![3]))?;
/// assert_eq!((out.inserted, out.deleted), (0, 1));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn delete_para(hwpx: &[u8], opts: &DeleteParaOptions) -> Result<StructuralOutput, OpsError> {
    if opts.indexes.is_empty() {
        return Err(rejected(
            OpsCode::DeleteNoTarget,
            "delete_para needs at least one paragraph index",
        ));
    }
    let targets: Vec<ParagraphLocator> = opts
        .indexes
        .iter()
        .map(|&index| ParagraphLocator { section: opts.section, index })
        .collect();
    // Best-effort advisory scan, never a refusal: it has to read the
    // paragraphs before they are gone, and it is reported only when the edit
    // succeeds. An undecodable input or an out-of-range target yields
    // nothing here and is refused by the editor on the next line.
    let advisories = scan_delete_warnings(hwpx, &targets);
    let bytes = HwpxStructuralEditor::delete_paragraphs(hwpx, &targets)?;
    Ok(StructuralOutput {
        bytes,
        inserted: 0,
        deleted: opts.indexes.len(),
        warnings: advisories.into_iter().map(OpsWarning::Structural).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::diagnostics::OpsCode;

    #[test]
    fn a_coordinate_accepts_surrounding_whitespace() {
        assert_eq!(parse_coord("1,2").expect("plain"), GridCoord::new(1, 2));
        assert_eq!(parse_coord(" 3 , 4 ").expect("padded"), GridCoord::new(3, 4));
        assert_eq!(parse_coord("0,0").expect("origin"), GridCoord::new(0, 0));
    }

    #[test]
    fn a_coordinate_rejects_everything_else() {
        for bad in ["1", "1,2,3", "a,b", "", "1,", "-1,0", "1.5,2"] {
            let err = parse_coord(bad).expect_err("must reject");
            assert_eq!(err.code(), OpsCode::InvalidSetCellArgs, "{bad}");
            assert!(err.to_string().contains("row,col"), "{err}");
        }
    }

    #[test]
    fn a_single_target_needs_exactly_one_direction() {
        let base = SetCellOptions::default().with_table(0).with_text("값");

        let none = build_specs(&base.clone()).expect_err("no direction");
        assert_eq!(none.code(), OpsCode::InvalidSetCellArgs, "{none}");
        assert!(none.to_string().contains("exactly one"), "{none}");

        let both = build_specs(&base.clone().with_at("0,0").with_below("항목"))
            .expect_err("two directions");
        assert_eq!(both.code(), OpsCode::InvalidSetCellArgs, "{both}");
        assert!(both.to_string().contains("exactly one"), "{both}");

        let one = build_specs(&base.with_right_of("성명")).expect("one direction");
        assert_eq!(
            one,
            vec![CellSpec {
                table: 0,
                target: CellTarget::RightOf("성명".into()),
                text: "값".into(),
            }]
        );
    }

    #[test]
    fn a_single_target_needs_a_table_and_a_text() {
        let no_table = build_specs(&SetCellOptions::default().with_at("0,0").with_text("v"))
            .expect_err("table missing");
        assert_eq!(no_table.code(), OpsCode::InvalidSetCellArgs, "{no_table}");
        assert!(no_table.to_string().contains("table is required"), "{no_table}");

        let no_text = build_specs(&SetCellOptions::default().with_table(0).with_at("0,0"))
            .expect_err("text missing");
        assert_eq!(no_text.code(), OpsCode::InvalidSetCellArgs, "{no_text}");
        assert!(no_text.to_string().contains("text is required"), "{no_text}");
    }

    #[test]
    fn an_empty_text_clears_rather_than_rejecting() {
        let specs =
            build_specs(&SetCellOptions::default().with_table(1).with_at("2,0").with_text(""))
                .expect("the empty string is a legitimate clear");

        assert_eq!(specs[0].text, "");
        assert_eq!(specs[0].target, CellTarget::At(GridCoord::new(2, 0)));
    }

    #[test]
    fn specs_and_single_target_flags_cannot_be_mixed() {
        let spec =
            CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: "값".into() };

        let mixed =
            build_specs(&SetCellOptions::default().with_specs(vec![spec.clone()]).with_table(0))
                .expect_err("mixing is ambiguous");
        assert_eq!(mixed.code(), OpsCode::InvalidSetCellArgs, "{mixed}");
        assert!(mixed.to_string().contains("cannot be combined"), "{mixed}");

        let empty = build_specs(&SetCellOptions::default().with_specs(Vec::new()))
            .expect_err("an empty batch edits nothing");
        assert_eq!(
            empty.code(),
            OpsCode::InvalidSetCellMap,
            "a malformed batch is not an arg error"
        );
        assert!(empty.to_string().contains("spec list is empty"), "{empty}");

        assert_eq!(
            build_specs(&SetCellOptions::default().with_specs(vec![spec.clone()])).expect("batch"),
            vec![spec]
        );
    }
}
