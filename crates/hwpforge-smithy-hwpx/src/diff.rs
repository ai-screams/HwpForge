//! Two-document diff (E5): semantic (Core) + package (ZIP entry) channels.
//!
//! `diff` compares two HWPX files so an agent can verify that an edit
//! changed exactly what it intended — the verification half of the editing
//! loop.
//!
//! **Comparison levels are explicit** (see [`COMPARISON_NOTE`]): the
//! semantic channel compares decoded Core structure, the package channel
//! compares ZIP entries by bytes. Wire content *inside* a changed entry
//! (e.g. the `hp:linesegarray` layout cache) is not itemized — an
//! element-level section-XML sub-diff is deliberately out of scope.
//!
//! Semantic classification (typed walker over Core, not a serde-tree walk):
//! - **field values** — `list_fields` on both sides keyed by name and
//!   occurrence (added / removed / value-changed). Paragraph pairs whose
//!   only difference is field bodies are attributed to this axis and not
//!   re-reported; the walker therefore compares field-body-stripped
//!   paragraphs (stripping descends into table cells; fields inside other
//!   containers may additionally surface as `raw` — documented limit).
//! - **table cells** — paragraph pairs whose tables differ only in cell
//!   paragraphs drill to per-anchor text changes `{table, row, col,
//!   before, after}`. The completeness guard compares the tables with all
//!   cell paragraphs blanked; any structural table change falls to `raw`.
//! - **paragraph text** — joined-text changes with `{section, para}`.
//!   Field promotion (stamping) surfaces as marker-text change plus a
//!   field `added` entry.
//! - **structure** — count changes (sections, paragraphs per section).
//! - **raw** — everything unclassified, as a serde-shaped first-diff
//!   path. Capped at [`RAW_CAP`] with an explicit dropped counter —
//!   never a silent truncation.
//!
//! Paragraph arrays are aligned by common prefix/suffix trimming; the
//! middle is paired index-wise and the remainder reported as added /
//! removed. Interleaved inserts can therefore surface as change pairs —
//! a documented limitation (LCS alignment is follow-up work).

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_core::table::grid::TableGrid;
use hwpforge_core::table::Table;
use hwpforge_foundation::FieldType;

use crate::decoder::package::PackageReader;
use crate::error::HwpxResult;
use crate::fill::FieldInfo;
use crate::read::ParaLocator;
use crate::stamp::first_diff_path;
use crate::table_inventory::{tables_in_document, TableEntry};
use crate::{HwpxDecoder, HwpxDocument, HwpxFiller};

/// Explicit statement of what each channel does and does not compare.
pub const COMPARISON_NOTE: &str = "semantic = decoded Core structure; package = ZIP entries \
compared by bytes. Wire-internal layout caches (e.g. hp:linesegarray) inside changed entries \
are not itemized.";

/// Maximum `raw` entries kept before counting drops.
pub const RAW_CAP: usize = 100;

const EXCERPT_MAX: usize = 120;

/// Kind of a field-axis change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FieldChangeKind {
    /// The field exists only in the revised document (e.g. stamped).
    Added,
    /// The field exists only in the base document.
    Removed,
    /// The field body value changed.
    ValueChanged,
}

/// One field-axis change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FieldChange {
    /// Field name.
    pub name: String,
    /// What happened.
    pub kind: FieldChangeKind,
    /// Base-side value (absent for `added`).
    pub before: Option<String>,
    /// Revised-side value (absent for `removed`).
    pub after: Option<String>,
}

/// One table-cell text change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellTextChange {
    /// Base-side table ordinal (shared traversal order).
    pub table: usize,
    /// Logical grid row of the anchor.
    pub row: u32,
    /// Logical grid column of the anchor.
    pub col: u32,
    /// Base-side cell text (excerpted).
    pub before: String,
    /// Revised-side cell text (excerpted).
    pub after: String,
}

/// Kind of a paragraph-axis change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ParagraphChangeKind {
    /// Same position, different text.
    Changed,
    /// Present only in the revised document.
    Added,
    /// Present only in the base document.
    Removed,
}

/// One paragraph-axis change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParagraphChange {
    /// Position (base-side for `changed`/`removed`, revised-side for
    /// `added`).
    pub at: ParaLocator,
    /// What happened.
    pub kind: ParagraphChangeKind,
    /// Base-side text excerpt.
    pub before: Option<String>,
    /// Revised-side text excerpt.
    pub after: Option<String>,
}

/// One count-level structure change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StructureChange {
    /// What was counted (e.g. `sections`, `section 0 paragraphs`).
    pub scope: String,
    /// Base-side count.
    pub before: usize,
    /// Revised-side count.
    pub after: usize,
}

/// One unclassified change (serde-shaped first-diff path).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct RawChange {
    /// Where the change sits (serde-shaped path).
    pub path: String,
    /// First-divergence detail.
    pub detail: String,
}

/// Semantic (Core-structure) channel.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticDiff {
    /// Field-axis changes.
    pub field_values: Vec<FieldChange>,
    /// Table-cell text changes.
    pub cells: Vec<CellTextChange>,
    /// Paragraph text/structure changes.
    pub paragraphs: Vec<ParagraphChange>,
    /// Count-level changes.
    pub structure: Vec<StructureChange>,
    /// Unclassified changes (capped at [`RAW_CAP`]).
    pub raw: Vec<RawChange>,
    /// How many raw entries were dropped by the cap (0 = none).
    pub raw_dropped: usize,
}

impl SemanticDiff {
    /// `true` when no semantic change was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.field_values.is_empty()
            && self.cells.is_empty()
            && self.paragraphs.is_empty()
            && self.structure.is_empty()
            && self.raw.is_empty()
            && self.raw_dropped == 0
    }

    fn push_raw(&mut self, path: String, detail: String) {
        if self.raw.len() < RAW_CAP {
            self.raw.push(RawChange { path, detail });
        } else {
            self.raw_dropped += 1;
        }
    }
}

/// Package (ZIP entry) channel.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PackageDiff {
    /// Entries present only in the revised package.
    pub added: Vec<String>,
    /// Entries present only in the base package.
    pub removed: Vec<String>,
    /// Entries present in both but byte-unequal.
    pub changed: Vec<String>,
}

impl PackageDiff {
    /// `true` when every entry is byte-identical.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Full diff report.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DocumentDiff {
    /// `true` when both channels found nothing.
    pub identical: bool,
    /// What the channels do and do not compare.
    pub note: String,
    /// Core-structure channel.
    pub semantic: SemanticDiff,
    /// ZIP-entry channel.
    pub package: PackageDiff,
}

/// Two-document diff facade.
pub struct HwpxDiffer;

impl HwpxDiffer {
    /// Diffs `base` against `revised`.
    ///
    /// # Errors
    ///
    /// Fails when either input cannot be decoded as HWPX.
    pub fn diff(base: &[u8], revised: &[u8]) -> HwpxResult<DocumentDiff> {
        let d_base = HwpxDecoder::decode(base)?;
        let d_rev = HwpxDecoder::decode(revised)?;

        let mut semantic = SemanticDiff::default();
        diff_fields(base, revised, &mut semantic)?;
        diff_documents(&d_base, &d_rev, &mut semantic);

        if d_base.style_store != d_rev.style_store {
            semantic.push_raw(
                "$.style_store".to_string(),
                first_diff_path(&d_base.style_store, &d_rev.style_store),
            );
        }
        if d_base.image_store != d_rev.image_store {
            semantic.push_raw("$.image_store".to_string(), "(image payloads)".to_string());
        }

        let package = diff_package(base, revised)?;
        let identical = semantic.is_empty() && package.is_empty();
        Ok(DocumentDiff { identical, note: COMPARISON_NOTE.to_string(), semantic, package })
    }
}

fn diff_fields(base: &[u8], revised: &[u8], out: &mut SemanticDiff) -> HwpxResult<()> {
    let keyed = |fields: &[FieldInfo]| {
        let mut map: std::collections::BTreeMap<(String, usize), String> =
            std::collections::BTreeMap::new();
        let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for f in fields {
            let Some(name) = &f.name else { continue };
            let occurrence = seen.entry(name.clone()).or_insert(0);
            map.insert((name.clone(), *occurrence), f.current.clone());
            *occurrence += 1;
        }
        map
    };
    let before = keyed(&HwpxFiller::list_fields(base)?);
    let after = keyed(&HwpxFiller::list_fields(revised)?);

    for (key, b_val) in &before {
        match after.get(key) {
            Some(a_val) if a_val != b_val => out.field_values.push(FieldChange {
                name: key.0.clone(),
                kind: FieldChangeKind::ValueChanged,
                before: Some(excerpt(b_val)),
                after: Some(excerpt(a_val)),
            }),
            Some(_) => {}
            None => out.field_values.push(FieldChange {
                name: key.0.clone(),
                kind: FieldChangeKind::Removed,
                before: Some(excerpt(b_val)),
                after: None,
            }),
        }
    }
    for (key, a_val) in &after {
        if !before.contains_key(key) {
            out.field_values.push(FieldChange {
                name: key.0.clone(),
                kind: FieldChangeKind::Added,
                before: None,
                after: Some(excerpt(a_val)),
            });
        }
    }
    Ok(())
}

fn diff_documents(base: &HwpxDocument, rev: &HwpxDocument, out: &mut SemanticDiff) {
    if base.document.metadata() != rev.document.metadata() {
        out.push_raw(
            "$.metadata".to_string(),
            first_diff_path(base.document.metadata(), rev.document.metadata()),
        );
    }

    let b_sections = base.document.sections();
    let r_sections = rev.document.sections();
    if b_sections.len() != r_sections.len() {
        out.structure.push(StructureChange {
            scope: "sections".to_string(),
            before: b_sections.len(),
            after: r_sections.len(),
        });
    }

    let base_tables = tables_in_document(&base.document);
    for (si, (b_sec, r_sec)) in b_sections.iter().zip(r_sections).enumerate() {
        diff_section(si, b_sec, r_sec, &base_tables, out);
    }

    // Sections beyond the common prefix: itemize their paragraphs so a
    // whole-section add/remove is not paragraph-level silent.
    for (si, sec) in r_sections.iter().enumerate().skip(b_sections.len()) {
        for (pi, para) in sec.paragraphs.iter().enumerate() {
            out.paragraphs.push(ParagraphChange {
                at: ParaLocator { section: si, para: pi },
                kind: ParagraphChangeKind::Added,
                before: None,
                after: Some(excerpt(&para.text_content())),
            });
        }
    }
    for (si, sec) in b_sections.iter().enumerate().skip(r_sections.len()) {
        for (pi, para) in sec.paragraphs.iter().enumerate() {
            out.paragraphs.push(ParagraphChange {
                at: ParaLocator { section: si, para: pi },
                kind: ParagraphChangeKind::Removed,
                before: Some(excerpt(&para.text_content())),
                after: None,
            });
        }
    }
}

fn diff_section(
    section: usize,
    base: &Section,
    rev: &Section,
    base_tables: &[TableEntry<'_>],
    out: &mut SemanticDiff,
) {
    let b = &base.paragraphs;
    let r = &rev.paragraphs;
    if b.len() != r.len() {
        out.structure.push(StructureChange {
            scope: format!("section {section} paragraphs"),
            before: b.len(),
            after: r.len(),
        });
    }

    // Common prefix/suffix trim.
    let mut lo = 0;
    while lo < b.len() && lo < r.len() && b[lo] == r[lo] {
        lo += 1;
    }
    let mut hi = 0;
    while hi < b.len() - lo && hi < r.len() - lo && b[b.len() - 1 - hi] == r[r.len() - 1 - hi] {
        hi += 1;
    }
    let b_mid = &b[lo..b.len() - hi];
    let r_mid = &r[lo..r.len() - hi];

    let pairs = b_mid.len().min(r_mid.len());
    for k in 0..pairs {
        classify_pair(section, lo + k, &b_mid[k], &r_mid[k], base_tables, out);
    }
    for (k, para) in b_mid.iter().enumerate().skip(pairs) {
        out.paragraphs.push(ParagraphChange {
            at: ParaLocator { section, para: lo + k },
            kind: ParagraphChangeKind::Removed,
            before: Some(excerpt(&para.text_content())),
            after: None,
        });
    }
    for (k, para) in r_mid.iter().enumerate().skip(pairs) {
        out.paragraphs.push(ParagraphChange {
            at: ParaLocator { section, para: lo + k },
            kind: ParagraphChangeKind::Added,
            before: None,
            after: Some(excerpt(&para.text_content())),
        });
    }
}

fn classify_pair(
    section: usize,
    para: usize,
    base: &Paragraph,
    rev: &Paragraph,
    base_tables: &[TableEntry<'_>],
    out: &mut SemanticDiff,
) {
    // Field bodies are the field axis's responsibility: compare stripped.
    let b = strip_field_bodies(base);
    let r = strip_field_bodies(rev);
    if b == r {
        return;
    }

    if let Some(changes) = try_cell_diff(&b, &r, base, base_tables) {
        out.cells.extend(changes);
        return;
    }

    let b_text = b.text_content();
    let r_text = r.text_content();
    if b_text != r_text {
        out.paragraphs.push(ParagraphChange {
            at: ParaLocator { section, para },
            kind: ParagraphChangeKind::Changed,
            before: Some(excerpt(&b_text)),
            after: Some(excerpt(&r_text)),
        });
        return;
    }

    out.push_raw(format!("$.sections[{section}].paragraphs[{para}]"), first_diff_path(&b, &r));
}

/// Blanks every field body (recursing into table cells) so field-value
/// changes are owned by the field axis alone.
fn strip_field_bodies(paragraph: &Paragraph) -> Paragraph {
    let mut p = paragraph.clone();
    strip_in_paragraph(&mut p);
    p
}

fn strip_in_paragraph(paragraph: &mut Paragraph) {
    for run in &mut paragraph.runs {
        match &mut run.content {
            RunContent::Control(control) => {
                // Mirror the field axis EXACTLY: `diff_fields` only reports
                // named ClickHere fields (that is all `list_fields` emits by
                // name), so only those bodies may be blanked here. Blanking
                // any wider set would let a field-body change vanish from
                // every semantic axis at once — unnamed or non-ClickHere
                // field changes must instead fall through to `raw`.
                if let Control::Field { field_type, name, display_text, .. } = control.as_mut() {
                    if *field_type == FieldType::ClickHere && name.is_some() {
                        display_text.clear();
                    }
                }
            }
            RunContent::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        for p in &mut cell.paragraphs {
                            strip_in_paragraph(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Attempts to explain a paragraph pair entirely as table-cell text
/// changes. Returns `None` when anything else differs (falls to other
/// classifications).
fn try_cell_diff(
    base: &Paragraph,
    rev: &Paragraph,
    original_base: &Paragraph,
    base_tables: &[TableEntry<'_>],
) -> Option<Vec<CellTextChange>> {
    if base.runs.len() != rev.runs.len() {
        return None;
    }
    let mut changes = Vec::new();
    for (idx, (b_run, r_run)) in base.runs.iter().zip(&rev.runs).enumerate() {
        match (&b_run.content, &r_run.content) {
            (RunContent::Table(b_table), RunContent::Table(r_table)) => {
                if b_table == r_table {
                    continue;
                }
                if !tables_equal_modulo_cell_paragraphs(b_table, r_table) {
                    return None;
                }
                // Ordinal lookup uses the ORIGINAL (unstripped) table
                // reference, since the inventory borrows the original doc.
                let RunContent::Table(original_table) = &original_base.runs[idx].content else {
                    return None;
                };
                let ordinal = base_tables
                    .iter()
                    .find(|e| std::ptr::eq(e.table, original_table.as_ref()))
                    .map(|e| e.ordinal)?;
                collect_cell_changes(ordinal, b_table, r_table, &mut changes)?;
            }
            (b, r) if b == r => {}
            _ => return None,
        }
    }
    if changes.is_empty() {
        None
    } else {
        Some(changes)
    }
}

fn tables_equal_modulo_cell_paragraphs(a: &Table, b: &Table) -> bool {
    let blank = |t: &Table| {
        let mut t = t.clone();
        for row in &mut t.rows {
            for cell in &mut row.cells {
                cell.paragraphs = Vec::new();
            }
        }
        t
    };
    blank(a) == blank(b)
}

fn collect_cell_changes(
    ordinal: usize,
    base: &Table,
    rev: &Table,
    changes: &mut Vec<CellTextChange>,
) -> Option<()> {
    let b_grid = TableGrid::from_table(base).ok()?;
    let r_grid = TableGrid::from_table(rev).ok()?;
    if b_grid.dimensions() != r_grid.dimensions() {
        return None;
    }
    let b_anchors: Vec<_> = b_grid.iter_anchors().collect();
    let r_anchors: Vec<_> = r_grid.iter_anchors().collect();
    if b_anchors.len() != r_anchors.len() {
        return None;
    }
    for (b_anchor, r_anchor) in b_anchors.iter().zip(&r_anchors) {
        if b_anchor.anchor != r_anchor.anchor
            || b_anchor.row_span != r_anchor.row_span
            || b_anchor.col_span != r_anchor.col_span
        {
            return None;
        }
        let b_cell = &base.rows[b_anchor.row_idx].cells[b_anchor.cell_idx];
        let r_cell = &rev.rows[r_anchor.row_idx].cells[r_anchor.cell_idx];
        if b_cell.paragraphs == r_cell.paragraphs {
            continue;
        }
        let b_text = cell_text(b_cell.paragraphs.as_slice());
        let r_text = cell_text(r_cell.paragraphs.as_slice());
        if b_text == r_text {
            // Cell differs beyond its text — not explainable here.
            return None;
        }
        changes.push(CellTextChange {
            table: ordinal,
            row: b_anchor.anchor.row,
            col: b_anchor.anchor.col,
            before: excerpt(&b_text),
            after: excerpt(&r_text),
        });
    }
    Some(())
}

fn cell_text(paragraphs: &[Paragraph]) -> String {
    paragraphs.iter().map(Paragraph::text_content).collect::<Vec<_>>().join("\n")
}

fn diff_package(base: &[u8], revised: &[u8]) -> HwpxResult<PackageDiff> {
    let mut b_reader = PackageReader::new(base)?;
    let mut r_reader = PackageReader::new(revised)?;
    let b_entries: Vec<String> = b_reader.list_entries()?.into_iter().map(|e| e.path).collect();
    let r_entries: Vec<String> = r_reader.list_entries()?.into_iter().map(|e| e.path).collect();

    let b_set: std::collections::BTreeSet<&String> = b_entries.iter().collect();
    let r_set: std::collections::BTreeSet<&String> = r_entries.iter().collect();

    let mut diff = PackageDiff::default();
    for path in &r_entries {
        if !b_set.contains(path) {
            diff.added.push(path.clone());
        }
    }
    for path in &b_entries {
        if !r_set.contains(path) {
            diff.removed.push(path.clone());
        } else if b_reader.read_binary_entry(path)? != r_reader.read_binary_entry(path)? {
            diff.changed.push(path.clone());
        }
    }
    Ok(diff)
}

fn excerpt(text: &str) -> String {
    if text.chars().count() <= EXCERPT_MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(EXCERPT_MAX).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_edit::{CellSpec, CellTarget, HwpxCellEditor};
    use crate::{HwpxEncoder, HwpxReader};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_core::Run;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use std::io::Write as _;

    fn fixture(rel: &str) -> Vec<u8> {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel);
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
    }

    fn text_para(text: &str) -> Paragraph {
        let mut p = Paragraph::new(ParaShapeIndex::new(0));
        p.runs.push(Run::text(text, CharShapeIndex::new(0)));
        p
    }

    fn reencode(decoded: &HwpxDocument) -> Vec<u8> {
        let validated = decoded.document.clone().validate().expect("validate");
        HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).expect("encode")
    }

    // -- edge / unit ------------------------------------------------------

    #[test]
    fn self_diff_is_identical_and_empty() {
        let bytes = fixture("tables/merged_grid_form.hwpx");
        let diff = HwpxDiffer::diff(&bytes, &bytes).unwrap();
        assert!(diff.identical);
        assert!(diff.semantic.is_empty());
        assert!(diff.package.is_empty());
        assert_eq!(diff.note, COMPARISON_NOTE);
    }

    #[test]
    fn alignment_reports_inserted_paragraph_as_added() {
        let base = Section::with_paragraphs(
            vec![text_para("A"), text_para("B"), text_para("C")],
            PageSettings::default(),
        );
        let rev = Section::with_paragraphs(
            vec![text_para("A"), text_para("X"), text_para("B"), text_para("C")],
            PageSettings::default(),
        );
        let mut out = SemanticDiff::default();
        diff_section(0, &base, &rev, &[], &mut out);

        assert_eq!(out.structure.len(), 1);
        assert_eq!((out.structure[0].before, out.structure[0].after), (3, 4));
        assert_eq!(out.paragraphs.len(), 1);
        assert_eq!(out.paragraphs[0].kind, ParagraphChangeKind::Added);
        assert_eq!(out.paragraphs[0].at, ParaLocator { section: 0, para: 1 });
        assert_eq!(out.paragraphs[0].after.as_deref(), Some("X"));
    }

    #[test]
    fn excerpt_caps_long_text_with_ellipsis() {
        let long = "가".repeat(EXCERPT_MAX + 10);
        let cut = excerpt(&long);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.chars().count(), EXCERPT_MAX + 1);
    }

    // -- editing-chain regression gates ----------------------------------

    #[test]
    fn fill_output_diff_reports_exactly_the_field_value() {
        let base = fixture("fields/clickhere_named.hwpx");
        let mut values = std::collections::BTreeMap::new();
        values.insert("user_email".to_string(), "diff@gate.io".to_string());
        let filled = HwpxFiller::fill(&base, &values).expect("fill");

        let diff = HwpxDiffer::diff(&base, &filled.bytes).unwrap();
        assert!(!diff.identical);
        assert_eq!(diff.semantic.field_values.len(), 1);
        let change = &diff.semantic.field_values[0];
        assert_eq!(change.name, "user_email");
        assert_eq!(change.kind, FieldChangeKind::ValueChanged);
        assert_eq!(change.after.as_deref(), Some("diff@gate.io"));
        // The field axis owns the value change; W1b 이후 fill 은 해당
        // 문단의 stale linesegarray 도 제거하므로 raw 축에 `$.layout_cache`
        // 변경 하나가 **정직하게** 함께 보고된다 (§1g v5 변경 5).
        assert!(diff.semantic.cells.is_empty(), "cells: {:?}", diff.semantic.cells);
        assert!(diff.semantic.paragraphs.is_empty(), "paras: {:?}", diff.semantic.paragraphs);
        assert!(diff.semantic.structure.is_empty());
        assert_eq!(diff.semantic.raw.len(), 1, "raw: {:?}", diff.semantic.raw);
        assert_eq!(diff.semantic.raw[0].detail, "$.layout_cache");
        assert!(diff.package.changed.iter().any(|p| p.contains("section")));
    }

    #[test]
    fn set_cell_output_diff_reports_exactly_that_cell() {
        let base = fixture("tables/merged_grid_form.hwpx");
        // Self-adapting target: first empty anchor cell of table 0.
        let view = HwpxReader::read_table(&base, 0).expect("read table");
        let target = view
            .cells
            .iter()
            .find(|c| c.text.trim().is_empty() && c.contains.is_empty())
            .expect("empty cell in fixture");
        let spec = CellSpec {
            table: 0,
            target: CellTarget::At(GridCoord::new(target.row, target.col)),
            text: "diff-gate".to_string(),
        };
        let edited = HwpxCellEditor::set_cells(&base, &[spec]).expect("set_cells");

        let diff = HwpxDiffer::diff(&base, &edited.bytes).unwrap();
        assert_eq!(diff.semantic.cells.len(), 1, "cells: {:?}", diff.semantic.cells);
        let change = &diff.semantic.cells[0];
        assert_eq!((change.table, change.row, change.col), (0, target.row, target.col));
        assert_eq!(change.after, "diff-gate");
        assert!(diff.semantic.field_values.is_empty());
        assert!(diff.semantic.paragraphs.is_empty(), "paras: {:?}", diff.semantic.paragraphs);
        assert!(diff.semantic.raw.is_empty(), "raw: {:?}", diff.semantic.raw);
    }

    #[test]
    fn text_edit_reports_paragraph_change_with_before_after() {
        let base = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base).unwrap();
        let para = decoded.document.sections_mut()[0]
            .paragraphs
            .iter_mut()
            .find(|p| {
                p.runs
                    .iter()
                    .any(|r| matches!(&r.content, RunContent::Text(t) if !t.trim().is_empty()))
                    && !p.runs.iter().any(|r| matches!(r.content, RunContent::Table(_)))
            })
            .expect("paragraph with a plain text run");
        let original_text = para.text_content();
        let run = para
            .runs
            .iter_mut()
            .find(|r| matches!(&r.content, RunContent::Text(t) if !t.trim().is_empty()))
            .expect("non-empty text run");
        run.content = RunContent::Text("diff 게이트 문구".to_string());
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        let changed: Vec<_> = diff
            .semantic
            .paragraphs
            .iter()
            .filter(|p| p.kind == ParagraphChangeKind::Changed)
            .collect();
        assert_eq!(changed.len(), 1, "paras: {:?}", diff.semantic.paragraphs);
        assert_eq!(changed[0].before.as_deref(), Some(original_text.as_str()));
        assert!(changed[0].after.as_deref().unwrap().contains("diff 게이트"));
    }

    #[test]
    fn added_paragraph_reports_structure_and_added_entry() {
        let base = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base).unwrap();
        decoded.document.sections_mut()[0].paragraphs.push(text_para("추가된 문단"));
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        assert!(diff
            .semantic
            .structure
            .iter()
            .any(|s| s.scope.contains("paragraphs") && s.after == s.before + 1));
        let added: Vec<_> = diff
            .semantic
            .paragraphs
            .iter()
            .filter(|p| p.kind == ParagraphChangeKind::Added)
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].after.as_deref(), Some("추가된 문단"));
    }

    #[test]
    fn unnamed_field_body_change_falls_to_raw_not_silence() {
        // Review H1: the field axis only reports named ClickHere fields, so
        // an unnamed field's body change must surface via `raw` — never be
        // stripped into silence.
        let base_fixture = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base_fixture).unwrap();
        for p in &mut decoded.document.sections_mut()[0].paragraphs {
            for run in &mut p.runs {
                if let RunContent::Control(c) = &mut run.content {
                    if let Control::Field { name, .. } = c.as_mut() {
                        *name = None;
                    }
                }
            }
        }
        let base = reencode(&decoded);

        for p in &mut decoded.document.sections_mut()[0].paragraphs {
            for run in &mut p.runs {
                if let RunContent::Control(c) = &mut run.content {
                    if let Control::Field { display_text, .. } = c.as_mut() {
                        *display_text = "몰래 바뀐 값".to_string();
                    }
                }
            }
        }
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        assert!(diff.semantic.field_values.is_empty(), "unnamed fields have no field axis");
        assert!(!diff.semantic.is_empty(), "the change must not vanish from the semantic channel");
        assert!(!diff.semantic.raw.is_empty(), "expected a raw fallback entry");
    }

    #[test]
    fn stamp_output_diff_reports_added_fields() {
        use crate::stamp::{HwpxStamper, StampAction, StampSpec};

        let base = fixture("stamp/placeholder_basic.hwpx");
        let candidates = HwpxStamper::plan_bytes(&base).expect("plan");
        let specs: Vec<StampSpec> = candidates
            .iter()
            .filter(|c| c.guard.is_none())
            .enumerate()
            .map(|(i, c)| StampSpec {
                section: c.section,
                path: c.path.clone(),
                span: c.span.clone(),
                marker: c.marker.clone(),
                action: StampAction::Field { name: format!("필드{i}"), hint: None },
            })
            .collect();
        assert!(!specs.is_empty(), "fixture must yield unguarded candidates");
        let stamped = HwpxStamper::stamp(&base, &specs).expect("stamp");

        let diff = HwpxDiffer::diff(&base, &stamped.bytes).unwrap();
        let added: Vec<_> = diff
            .semantic
            .field_values
            .iter()
            .filter(|f| f.kind == FieldChangeKind::Added)
            .collect();
        assert_eq!(added.len(), specs.len(), "field_values: {:?}", diff.semantic.field_values);
        // Field promotion legitimately shows as marker-text changes; nothing
        // may land in raw.
        assert!(diff.semantic.raw.is_empty(), "raw: {:?}", diff.semantic.raw);
    }

    #[test]
    fn metadata_change_reports_raw_metadata_path() {
        let base = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base).unwrap();
        decoded.document.metadata_mut().title = Some("바뀐 제목".to_string());
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        assert!(
            diff.semantic.raw.iter().any(|r| r.path == "$.metadata"),
            "raw: {:?}",
            diff.semantic.raw
        );
    }

    #[test]
    fn added_section_itemizes_its_paragraphs() {
        // Review L1: a whole added section must not be paragraph-level
        // silent.
        let base = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base).unwrap();
        decoded.document.add_section(Section::with_paragraphs(
            vec![text_para("새 섹션 문단")],
            PageSettings::default(),
        ));
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        assert!(diff
            .semantic
            .structure
            .iter()
            .any(|s| s.scope == "sections" && s.after == s.before + 1));
        assert!(
            diff.semantic.paragraphs.iter().any(|p| {
                p.kind == ParagraphChangeKind::Added
                    && p.at.section == 1
                    && p.after.as_deref() == Some("새 섹션 문단")
            }),
            "paras: {:?}",
            diff.semantic.paragraphs
        );
    }

    #[test]
    fn push_raw_caps_and_counts_drops() {
        let mut s = SemanticDiff::default();
        for i in 0..(RAW_CAP + 5) {
            s.push_raw(format!("$.x[{i}]"), "d".to_string());
        }
        assert_eq!(s.raw.len(), RAW_CAP);
        assert_eq!(s.raw_dropped, 5);
        assert!(!s.is_empty());
    }

    #[test]
    fn removed_field_reports_field_removed() {
        let base = fixture("fields/clickhere_named.hwpx");
        let mut decoded = HwpxDecoder::decode(&base).unwrap();
        for p in &mut decoded.document.sections_mut()[0].paragraphs {
            p.runs.retain(|r| {
                !matches!(&r.content, RunContent::Control(c)
                    if matches!(c.as_ref(), Control::Field { .. }))
            });
        }
        let revised = reencode(&decoded);

        let diff = HwpxDiffer::diff(&base, &revised).unwrap();
        assert!(
            diff.semantic
                .field_values
                .iter()
                .any(|f| f.kind == FieldChangeKind::Removed && f.name == "user_email"),
            "field_values: {:?}",
            diff.semantic.field_values
        );
    }

    #[test]
    fn package_channel_detects_added_entry() {
        let base = fixture("tables/table_01_basic_2x2.hwpx");
        // Rewrite the package with one extra entry.
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(base.as_slice())).unwrap();
        let mut out = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
            for i in 0..archive.len() {
                let entry = archive.by_index_raw(i).unwrap();
                writer.raw_copy_file(entry).unwrap();
            }
            writer
                .start_file("Custom/extra.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"extra").unwrap();
            writer.finish().unwrap();
        }

        let diff = HwpxDiffer::diff(&base, &out).unwrap();
        assert_eq!(diff.package.added, vec!["Custom/extra.txt".to_string()]);
        assert!(diff.package.removed.is_empty());
        assert!(diff.package.changed.is_empty());
        // Core structure did not change.
        assert!(diff.semantic.is_empty(), "semantic: {:?}", diff.semantic);
    }
}
