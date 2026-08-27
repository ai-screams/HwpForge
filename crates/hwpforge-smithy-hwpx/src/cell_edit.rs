//! Grid-addressed table cell editing (E3 Wave 3).
//!
//! The delta surface symmetric to `fill` (name-addressed) for tables: a cell
//! is addressed by its table ordinal (shared inventory order) plus either a
//! logical grid coordinate or a label-relative direction, then its text-only
//! content is replaced (or cleared) in one all-or-nothing batch.
//!
//! Design contract (Codex-settled, see
//! `.docs/planning/2026-07-21-e3-table-grid-addressing.md`):
//!
//! - **Covered coordinates resolve silently to their anchor**, and every
//!   result always reports `requested` / `anchor` / `resolution` so callers
//!   cannot miss the redirection (28.8% of grid positions in the government
//!   corpus are covered).
//! - **Label matching is normalized exact match only** — Unicode NFC +
//!   whitespace trim/collapse, plus trailing `:`/`：` equivalence only when
//!   it selects a unique cell. No substring or fuzzy matching for mutation
//!   targets; failures return candidate labels for retry.
//! - **Text-only cells**: a cell containing tables, images, or controls is
//!   rejected (`NonTextContent`) until an explicitly destructive operation
//!   exists.
//! - **Empty string is a legitimate clear** — `fill`'s empty-value rejection
//!   guards the ClickHere hint-fallback sentinel; cells carry no such
//!   overload.
//! - Replacement style is inherited from the first text-bearing
//!   paragraph/run pair of the target cell (single source; the text-only
//!   admission plus Core validation guarantee such a pair exists).
//! - Batch preflight is file-level all-or-nothing, and rejects duplicate
//!   anchors as well as ancestor/descendant target conflicts (an outer cell
//!   replacement would destroy a nested table another spec targets).

use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::table::grid::{GridCell, GridCoord, TableGrid};
use hwpforge_core::table::{Table, TableCell};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

use crate::decoder::{HwpxDecoder, HwpxDocument};
use crate::encoder::HwpxEncoder;
use crate::stamp::{admission_compare, check_zip_carry, encode_hwpx, StamperError};
use crate::table_inventory::{for_each_table_mut, tables_in_document, PathSeg};

/// How a [`CellSpec`] addresses a cell inside its table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CellTarget {
    /// A logical grid coordinate (covered positions resolve to their anchor).
    At(GridCoord),
    /// The cell immediately right of the uniquely labeled cell.
    RightOf(String),
    /// The cell immediately below the uniquely labeled cell.
    Below(String),
}

/// One requested cell edit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellSpec {
    /// Table ordinal in the shared traversal order (see `to-json` export).
    pub table: usize,
    /// Which cell to edit.
    #[serde(flatten)]
    pub target: CellTarget,
    /// Replacement text; the empty string clears the cell.
    pub text: String,
}

/// Whether the requested coordinate was the anchor itself or was covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CellResolution {
    /// The requested position is the anchor.
    Exact,
    /// The requested position is covered by a merge; the anchor was edited.
    CoveredToAnchor,
}

/// One applied cell edit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SetCellResult {
    /// Table ordinal the edit landed in.
    pub table: usize,
    /// Position the caller asked for (label targets report the computed
    /// neighbor coordinate).
    pub requested: GridCoord,
    /// Anchor cell that was actually edited.
    pub anchor: GridCoord,
    /// How `requested` mapped onto `anchor`.
    pub resolution: CellResolution,
    /// Whether the edit cleared the cell (empty replacement text).
    pub cleared: bool,
}

/// Result of an all-or-nothing [`apply_set_cells`] batch.
///
/// 현재 인코드 [`EncodeWarning`](crate::EncodeWarning) 은 이 결과에 실리지
/// 않는다 — 경고 채널 신설은 public 필드 추가(semver) 라 별도 승인 대기
/// (각주 에픽 계획 문서 §7h).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SetCellOutcome {
    /// Applied edits, in spec order.
    pub cells: Vec<SetCellResult>,
}

/// Why a cell-edit batch was rejected (no mutation happened).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CellEditError {
    /// The table ordinal does not exist.
    TableNotFound {
        /// Requested ordinal.
        table: usize,
        /// How many tables the document has.
        tables: usize,
    },
    /// The table's cells do not tile a well-formed grid.
    TableGridInvalid {
        /// Table ordinal.
        table: usize,
        /// First tiling violation.
        reason: String,
    },
    /// No cell matches the requested coordinate or label.
    CellNotFound {
        /// Table ordinal.
        table: usize,
        /// What was looked up and why it failed.
        detail: String,
        /// Normalized labels available in the table (for label lookups).
        candidates: Vec<String>,
    },
    /// The label matches more than one cell.
    LabelAmbiguous {
        /// Table ordinal.
        table: usize,
        /// The normalized label.
        label: String,
        /// How many cells matched.
        count: usize,
    },
    /// The target cell contains non-text content (table/image/control).
    NonTextContent {
        /// Table ordinal.
        table: usize,
        /// Anchor of the rejected cell.
        anchor: GridCoord,
    },
    /// Two specs resolve to the same anchor cell.
    TargetDuplicate {
        /// Table ordinal.
        table: usize,
        /// The doubly-targeted anchor.
        anchor: GridCoord,
    },
    /// One spec's cell contains another spec's target table.
    TargetConflict {
        /// Ordinal of the table whose cell would be replaced.
        outer_table: usize,
        /// Ordinal of the nested table another spec targets.
        inner_table: usize,
    },
    /// Decode/encode/validate failure.
    Codec(String),
    /// The input is not no-op round-trip safe (admission gate).
    NotRoundTripSafe {
        /// Which decoded component differs.
        component: String,
        /// First differing path.
        diff_path: String,
    },
    /// The input has ZIP entries the encoder would not carry.
    UncarriedZipEntries {
        /// Names of the uncarried entries.
        entries: Vec<String>,
    },
}

impl core::fmt::Display for CellEditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TableNotFound { table, tables } => {
                write!(f, "table #{table} does not exist (document has {tables} tables)")
            }
            Self::TableGridInvalid { table, reason } => {
                write!(f, "table #{table} does not tile a well-formed grid: {reason}")
            }
            Self::CellNotFound { table, detail, candidates } => {
                write!(f, "table #{table}: {detail}")?;
                if !candidates.is_empty() {
                    write!(f, " (available labels: {candidates:?})")?;
                }
                Ok(())
            }
            Self::LabelAmbiguous { table, label, count } => {
                write!(f, "table #{table}: label '{label}' matches {count} cells")
            }
            Self::NonTextContent { table, anchor } => write!(
                f,
                "table #{table}: cell ({}, {}) contains non-text content (table/image/control)",
                anchor.row, anchor.col
            ),
            Self::TargetDuplicate { table, anchor } => write!(
                f,
                "table #{table}: two edits resolve to the same cell ({}, {})",
                anchor.row, anchor.col
            ),
            Self::TargetConflict { outer_table, inner_table } => write!(
                f,
                "editing a cell of table #{outer_table} would destroy nested table #{inner_table} targeted by another edit"
            ),
            Self::Codec(detail) => write!(f, "codec failure: {detail}"),
            Self::NotRoundTripSafe { component, diff_path } => write!(
                f,
                "input is not no-op round-trip safe ({component} differs at {diff_path})"
            ),
            Self::UncarriedZipEntries { entries } => {
                write!(f, "input has ZIP entries the encoder would not carry: {entries:?}")
            }
        }
    }
}

impl std::error::Error for CellEditError {}

/// Resolved target of one spec, computed during preflight.
struct ResolvedEdit {
    table: usize,
    row_idx: usize,
    cell_idx: usize,
    requested: GridCoord,
    anchor: GridCoord,
    resolution: CellResolution,
    text: String,
}

/// Applies a batch of cell edits to a document, all-or-nothing.
///
/// # Errors
///
/// See [`CellEditError`] — any preflight rejection leaves the document
/// untouched.
pub fn apply_set_cells(
    document: &mut Document<Draft>,
    specs: &[CellSpec],
) -> Result<SetCellOutcome, CellEditError> {
    // ── preflight: resolve every spec before touching anything ──────
    let entries = tables_in_document(document);
    let mut grids: HashMap<usize, TableGrid> = HashMap::new();
    let mut resolved: Vec<ResolvedEdit> = Vec::with_capacity(specs.len());

    for spec in specs {
        let entry = entries
            .get(spec.table)
            .ok_or(CellEditError::TableNotFound { table: spec.table, tables: entries.len() })?;
        if let std::collections::hash_map::Entry::Vacant(slot) = grids.entry(spec.table) {
            let grid = TableGrid::from_table(entry.table).map_err(|e| {
                CellEditError::TableGridInvalid { table: spec.table, reason: e.to_string() }
            })?;
            slot.insert(grid);
        }
        let grid = &grids[&spec.table];

        let (requested, cell) = match &spec.target {
            CellTarget::At(coord) => {
                let cell = grid.resolve(*coord).ok_or_else(|| {
                    let (rows, cols) = grid.dimensions();
                    CellEditError::CellNotFound {
                        table: spec.table,
                        detail: format!(
                            "({}, {}) is outside the {rows}x{cols} grid",
                            coord.row, coord.col
                        ),
                        candidates: Vec::new(),
                    }
                })?;
                (*coord, cell)
            }
            CellTarget::RightOf(label) => {
                let anchor = find_label(entry.table, grid, label, spec.table)?;
                let requested = GridCoord::new(
                    anchor.anchor.row,
                    anchor.anchor.col + u32::from(anchor.col_span),
                );
                let cell = grid.resolve(requested).ok_or_else(|| CellEditError::CellNotFound {
                    table: spec.table,
                    detail: format!("no cell right of label '{label}'"),
                    candidates: Vec::new(),
                })?;
                (requested, cell)
            }
            CellTarget::Below(label) => {
                let anchor = find_label(entry.table, grid, label, spec.table)?;
                let requested = GridCoord::new(
                    anchor.anchor.row + u32::from(anchor.row_span),
                    anchor.anchor.col,
                );
                let cell = grid.resolve(requested).ok_or_else(|| CellEditError::CellNotFound {
                    table: spec.table,
                    detail: format!("no cell below label '{label}'"),
                    candidates: Vec::new(),
                })?;
                (requested, cell)
            }
        };

        let target_cell = &entry.table.rows[cell.row_idx].cells[cell.cell_idx];
        if !cell_is_text_only(target_cell) {
            return Err(CellEditError::NonTextContent { table: spec.table, anchor: cell.anchor });
        }

        resolved.push(ResolvedEdit {
            table: spec.table,
            row_idx: cell.row_idx,
            cell_idx: cell.cell_idx,
            requested,
            anchor: cell.anchor,
            resolution: if requested == cell.anchor {
                CellResolution::Exact
            } else {
                CellResolution::CoveredToAnchor
            },
            text: spec.text.clone(),
        });
    }

    // Duplicate anchors.
    let mut seen: HashSet<(usize, usize, usize)> = HashSet::new();
    for edit in &resolved {
        if !seen.insert((edit.table, edit.row_idx, edit.cell_idx)) {
            return Err(CellEditError::TargetDuplicate { table: edit.table, anchor: edit.anchor });
        }
    }

    // Ancestor/descendant conflicts: another spec's table nested inside an
    // edited cell would be destroyed by the replacement.
    //
    // Belt-and-suspenders: with today's rules this scan cannot fire — a cell
    // hosting a nested table is already rejected by the text-only check
    // above. It stays as an independent invariant so a future relaxation of
    // the content gate (e.g. an explicitly destructive replace) cannot
    // silently reintroduce the destroy-a-target hazard.
    for edit in &resolved {
        let mut cell_prefix: Vec<PathSeg> = entries[edit.table].path.clone();
        cell_prefix.push(PathSeg::Field("rows"));
        cell_prefix.push(PathSeg::Index(edit.row_idx));
        cell_prefix.push(PathSeg::Field("cells"));
        cell_prefix.push(PathSeg::Index(edit.cell_idx));
        for other in &resolved {
            if other.table != edit.table
                && entries[other.table].section == entries[edit.table].section
                && path_starts_with(&entries[other.table].path, &cell_prefix)
            {
                return Err(CellEditError::TargetConflict {
                    outer_table: edit.table,
                    inner_table: other.table,
                });
            }
        }
    }

    // ── mutate (infallible after preflight) ─────────────────────────
    let mut by_table: HashMap<usize, Vec<&ResolvedEdit>> = HashMap::new();
    for edit in &resolved {
        by_table.entry(edit.table).or_default().push(edit);
    }
    for_each_table_mut(document, &mut |ordinal, table| {
        if let Some(edits) = by_table.get(&ordinal) {
            for edit in edits {
                let cell = &mut table.rows[edit.row_idx].cells[edit.cell_idx];
                let (para_shape, char_shape) = inherit_style(cell);
                cell.paragraphs =
                    vec![Paragraph::with_runs(vec![Run::text(&edit.text, char_shape)], para_shape)];
            }
        }
    });

    Ok(SetCellOutcome {
        cells: resolved
            .into_iter()
            .map(|edit| SetCellResult {
                table: edit.table,
                requested: edit.requested,
                anchor: edit.anchor,
                resolution: edit.resolution,
                cleared: edit.text.is_empty(),
            })
            .collect(),
    })
}

/// NFC + Unicode-whitespace trim/collapse.
pub(crate) fn normalize_label(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strips one trailing `:` or `：` (used only when it selects a unique cell).
fn strip_trailing_colon(s: &str) -> &str {
    s.strip_suffix(':').or_else(|| s.strip_suffix('：')).unwrap_or(s)
}

/// The label text of a cell: all paragraph text joined, normalized.
fn cell_label_text(table: &Table, cell: &GridCell) -> String {
    let paragraphs = &table.rows[cell.row_idx].cells[cell.cell_idx].paragraphs;
    normalize_label(&paragraphs.iter().map(|p| p.text_content()).collect::<Vec<_>>().join("\n"))
}

/// Finds the unique anchor cell matching a label (normalized exact match;
/// trailing-colon equivalence only as a unique fallback).
fn find_label<'g>(
    table: &Table,
    grid: &'g TableGrid,
    label: &str,
    table_ordinal: usize,
) -> Result<&'g GridCell, CellEditError> {
    let want = normalize_label(label);
    let mut exact: Vec<&GridCell> = Vec::new();
    let mut colon: Vec<&GridCell> = Vec::new();
    let mut candidates: Vec<String> = Vec::new();

    for anchor in grid.iter_anchors() {
        let text = cell_label_text(table, anchor);
        if text.is_empty() {
            continue;
        }
        if text == want {
            exact.push(anchor);
        } else if strip_trailing_colon(&text) == strip_trailing_colon(&want) {
            colon.push(anchor);
        }
        if candidates.len() < 20 && !candidates.contains(&text) {
            candidates.push(text);
        }
    }

    let unique = |matches: Vec<&'g GridCell>| -> Result<&'g GridCell, CellEditError> {
        match matches.len() {
            1 => Ok(matches[0]),
            0 => Err(CellEditError::CellNotFound {
                table: table_ordinal,
                detail: format!("label '{label}' not found"),
                candidates: candidates.clone(),
            }),
            n => Err(CellEditError::LabelAmbiguous {
                table: table_ordinal,
                label: want.clone(),
                count: n,
            }),
        }
    };
    if exact.is_empty() {
        unique(colon)
    } else {
        unique(exact)
    }
}

/// Whether every run in the cell is text-bearing (`Text` / `InlineText`).
fn cell_is_text_only(cell: &TableCell) -> bool {
    cell.paragraphs.iter().all(|p| {
        p.runs.iter().all(|r| matches!(r.content, RunContent::Text(_) | RunContent::InlineText(_)))
    })
}

/// Style for the replacement paragraph: the first text-bearing paragraph/run
/// pair of the cell (one source for both shapes). The fallback arm is
/// unreachable for admitted (text-only, Core-valid) cells but stays total.
fn inherit_style(cell: &TableCell) -> (ParaShapeIndex, CharShapeIndex) {
    for paragraph in &cell.paragraphs {
        for run in &paragraph.runs {
            if matches!(run.content, RunContent::Text(_) | RunContent::InlineText(_)) {
                return (paragraph.para_shape_id, run.char_shape_id);
            }
        }
    }
    let first = cell.paragraphs.first();
    (
        first.map(|p| p.para_shape_id).unwrap_or_else(|| ParaShapeIndex::new(0)),
        first
            .and_then(|p| p.runs.first())
            .map(|r| r.char_shape_id)
            .unwrap_or_else(|| CharShapeIndex::new(0)),
    )
}

fn path_starts_with(path: &[PathSeg], prefix: &[PathSeg]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

/// Result of a bytes-level [`HwpxCellEditor::set_cells`] run.
#[derive(Debug)]
pub struct CellEditResult {
    /// The edited HWPX package.
    pub bytes: Vec<u8>,
    /// What was edited.
    pub outcome: SetCellOutcome,
}

/// Bytes-level cell-edit facade (admission-gated, like `HwpxStamper`).
#[derive(Debug)]
pub struct HwpxCellEditor;

impl HwpxCellEditor {
    /// Edits table cells in an HWPX package, all-or-nothing.
    ///
    /// Pipeline: admission gate (no-op round-trip + ZIP closed-world) →
    /// [`apply_set_cells`] → validate → encode. Every failure is
    /// fail-closed: no bytes are produced.
    ///
    /// # Errors
    ///
    /// See [`CellEditError`].
    pub fn set_cells(base: &[u8], specs: &[CellSpec]) -> Result<CellEditResult, CellEditError> {
        let d0 = HwpxDecoder::decode(base).map_err(|e| CellEditError::Codec(e.to_string()))?;
        let e0 = encode_hwpx(&d0).map_err(map_admission_error)?;
        let d1 = HwpxDecoder::decode(&e0).map_err(|e| CellEditError::Codec(e.to_string()))?;
        admission_compare(&d0, &d1).map_err(map_admission_error)?;
        check_zip_carry(base, &e0).map_err(map_admission_error)?;

        let HwpxDocument { mut document, style_store, image_store, .. } = d0;
        let outcome = apply_set_cells(&mut document, specs)?;
        let validated =
            document.validate().map_err(|e| CellEditError::Codec(format!("validate: {e}")))?;
        let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)
            .map_err(|e| CellEditError::Codec(e.to_string()))?;
        // Untouched paragraphs keep Hancom's line-layout cache so the
        // renderer does not reflow (and repaginate) the whole document.
        let bytes = crate::layout_carry::carry_line_segs(base, &e0, &bytes)
            .map_err(|e| CellEditError::Codec(e.to_string()))?;
        Ok(CellEditResult { bytes, outcome })
    }
}

fn map_admission_error(error: StamperError) -> CellEditError {
    match error {
        StamperError::NotRoundTripSafe { component, diff_path } => {
            CellEditError::NotRoundTripSafe { component, diff_path }
        }
        StamperError::UncarriedZipEntries { entries } => {
            CellEditError::UncarriedZipEntries { entries }
        }
        other => CellEditError::Codec(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::table::{TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(3))], ParaShapeIndex::new(2))
    }

    fn cell(text: &str, row_span: u16, col_span: u16) -> TableCell {
        TableCell::with_span(vec![text_para(text)], HwpUnit::new(8000).unwrap(), col_span, row_span)
    }

    /// 2×2 label form: 「성명 | (빈칸)」 / 「주소 | (빈칸)」.
    fn label_form() -> Table {
        Table::new(vec![
            TableRow::new(vec![cell("성명", 1, 1), cell("", 1, 1)]),
            TableRow::new(vec![cell("주소:", 1, 1), cell("", 1, 1)]),
        ])
    }

    fn doc_with(tables: Vec<Table>) -> Document<Draft> {
        let paragraphs = tables
            .into_iter()
            .map(|t| {
                let mut p = Paragraph::new(ParaShapeIndex::new(0));
                p.add_run(Run::table(t, CharShapeIndex::new(0)));
                p
            })
            .collect();
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::default()));
        doc
    }

    fn spec(table: usize, target: CellTarget, text: &str) -> CellSpec {
        CellSpec { table, target, text: text.to_string() }
    }

    fn cell_text(doc: &Document<Draft>, row: usize, col: usize) -> String {
        let table =
            doc.sections()[0].paragraphs[0].runs.iter().find_map(|r| r.content.as_table()).unwrap();
        table.rows[row].cells[col].paragraphs[0].text_content()
    }

    // ── addressing ──────────────────────────────────────────────

    #[test]
    fn at_coordinate_edits_cell_and_inherits_style() {
        let mut doc = doc_with(vec![label_form()]);
        let outcome =
            apply_set_cells(&mut doc, &[spec(0, CellTarget::At(GridCoord::new(0, 1)), "홍길동")])
                .unwrap();
        assert_eq!(cell_text(&doc, 0, 1), "홍길동");
        let r = &outcome.cells[0];
        assert_eq!((r.requested, r.anchor), (GridCoord::new(0, 1), GridCoord::new(0, 1)));
        assert_eq!(r.resolution, CellResolution::Exact);
        assert!(!r.cleared);

        // Style inherited from the replaced cell's first text run.
        let table =
            doc.sections()[0].paragraphs[0].runs.iter().find_map(|r| r.content.as_table()).unwrap();
        let para = &table.rows[0].cells[1].paragraphs[0];
        assert_eq!(para.para_shape_id, ParaShapeIndex::new(2));
        assert_eq!(para.runs[0].char_shape_id, CharShapeIndex::new(3));
    }

    #[test]
    fn covered_coordinate_resolves_to_anchor_and_reports_it() {
        // 2×1: anchor rs-2 at (0,0); (1,0) is covered.
        let merged =
            Table::new(vec![TableRow::new(vec![cell("병합", 2, 1)]), TableRow::new(vec![])]);
        let mut doc = doc_with(vec![merged]);
        let outcome =
            apply_set_cells(&mut doc, &[spec(0, CellTarget::At(GridCoord::new(1, 0)), "값")])
                .unwrap();
        let r = &outcome.cells[0];
        assert_eq!(r.requested, GridCoord::new(1, 0));
        assert_eq!(r.anchor, GridCoord::new(0, 0));
        assert_eq!(r.resolution, CellResolution::CoveredToAnchor);
        assert_eq!(cell_text(&doc, 0, 0), "값");
    }

    #[test]
    fn right_of_label_with_nfc_and_whitespace_normalization() {
        let mut doc = doc_with(vec![label_form()]);
        // NFD 입력 + 둘레 공백 — 정규화 exact match 로 잡혀야 한다.
        let nfd_label: String =
            unicode_normalization::UnicodeNormalization::nfd("  성명  ").collect();
        apply_set_cells(&mut doc, &[spec(0, CellTarget::RightOf(nfd_label), "홍길동")]).unwrap();
        assert_eq!(cell_text(&doc, 0, 1), "홍길동");
    }

    #[test]
    fn trailing_colon_equivalence_applies_only_as_unique_fallback() {
        let mut doc = doc_with(vec![label_form()]);
        // 셀 텍스트는 "주소:" — 콜론 없는 라벨로도 유일하면 매칭되고,
        // 엉뚱한 라벨은 콜론 동치로도 잡히지 않는다.
        apply_set_cells(&mut doc, &[spec(0, CellTarget::RightOf("연락처".into()), "x")])
            .expect_err("bogus label must not match");
        apply_set_cells(&mut doc, &[spec(0, CellTarget::RightOf("주소".into()), "서울")]).unwrap();
        assert_eq!(cell_text(&doc, 1, 1), "서울");
    }

    #[test]
    fn label_not_found_returns_candidates() {
        let mut doc = doc_with(vec![label_form()]);
        let err = apply_set_cells(&mut doc, &[spec(0, CellTarget::RightOf("연락처".into()), "x")])
            .unwrap_err();
        match err {
            CellEditError::CellNotFound { candidates, .. } => {
                assert!(candidates.contains(&"성명".to_string()), "{candidates:?}");
                assert!(candidates.contains(&"주소:".to_string()), "{candidates:?}");
            }
            other => panic!("expected CellNotFound, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_label_rejected() {
        let dup = Table::new(vec![
            TableRow::new(vec![cell("성명", 1, 1), cell("", 1, 1)]),
            TableRow::new(vec![cell("성명", 1, 1), cell("", 1, 1)]),
        ]);
        let mut doc = doc_with(vec![dup]);
        let err = apply_set_cells(&mut doc, &[spec(0, CellTarget::RightOf("성명".into()), "x")])
            .unwrap_err();
        assert!(matches!(err, CellEditError::LabelAmbiguous { count: 2, .. }), "{err:?}");
    }

    #[test]
    fn below_label_targets_next_row() {
        let column_form = Table::new(vec![
            TableRow::new(vec![cell("항목", 1, 1)]),
            TableRow::new(vec![cell("", 1, 1)]),
        ]);
        let mut doc = doc_with(vec![column_form]);
        apply_set_cells(&mut doc, &[spec(0, CellTarget::Below("항목".into()), "값")]).unwrap();
        assert_eq!(cell_text(&doc, 1, 0), "값");
    }

    // ── rejections ──────────────────────────────────────────────

    #[test]
    fn out_of_grid_and_missing_table_rejected() {
        let mut doc = doc_with(vec![label_form()]);
        let err = apply_set_cells(&mut doc, &[spec(0, CellTarget::At(GridCoord::new(9, 0)), "x")])
            .unwrap_err();
        assert!(matches!(err, CellEditError::CellNotFound { .. }), "{err:?}");
        let err = apply_set_cells(&mut doc, &[spec(7, CellTarget::At(GridCoord::new(0, 0)), "x")])
            .unwrap_err();
        assert!(matches!(err, CellEditError::TableNotFound { table: 7, tables: 1 }), "{err:?}");
    }

    #[test]
    fn non_text_cell_rejected() {
        let mut host_cell = cell("", 1, 1);
        host_cell.paragraphs = vec![{
            let mut p = Paragraph::new(ParaShapeIndex::new(0));
            p.add_run(Run::table(label_form(), CharShapeIndex::new(0)));
            p
        }];
        let outer = Table::new(vec![TableRow::new(vec![host_cell])]);
        let mut doc = doc_with(vec![outer]);
        let err = apply_set_cells(&mut doc, &[spec(0, CellTarget::At(GridCoord::new(0, 0)), "x")])
            .unwrap_err();
        assert!(matches!(err, CellEditError::NonTextContent { .. }), "{err:?}");
    }

    #[test]
    fn duplicate_targets_rejected_even_via_different_addresses() {
        let merged =
            Table::new(vec![TableRow::new(vec![cell("병합", 2, 1)]), TableRow::new(vec![])]);
        let mut doc = doc_with(vec![merged]);
        let err = apply_set_cells(
            &mut doc,
            &[
                spec(0, CellTarget::At(GridCoord::new(0, 0)), "a"),
                spec(0, CellTarget::At(GridCoord::new(1, 0)), "b"),
            ],
        )
        .unwrap_err();
        assert!(matches!(err, CellEditError::TargetDuplicate { .. }), "{err:?}");
        // All-or-nothing: 첫 스펙도 적용되지 않았어야 한다.
        assert_eq!(cell_text(&doc, 0, 0), "병합");
    }

    #[test]
    fn nested_table_target_conflict_rejected() {
        // Table #0's cell hosts table #1; editing that cell while another
        // spec targets table #1 must be rejected.
        let mut host_cell = cell("", 1, 1);
        host_cell.paragraphs = vec![{
            let mut p = Paragraph::new(ParaShapeIndex::new(0));
            p.add_run(Run::table(label_form(), CharShapeIndex::new(0)));
            p
        }];
        let outer = Table::new(vec![TableRow::new(vec![host_cell, cell("옆", 1, 1)])]);
        let mut doc = doc_with(vec![outer]);
        let err = apply_set_cells(
            &mut doc,
            &[
                spec(1, CellTarget::RightOf("성명".into()), "홍길동"),
                spec(0, CellTarget::At(GridCoord::new(0, 0)), "지움"),
            ],
        )
        .unwrap_err();
        // NonTextContent 가 먼저 걸린다 (host cell 은 표를 포함) — conflict 검사가
        // 무의미해지지 않도록, non-text 거부가 conflict 를 포섭함을 확인.
        assert!(
            matches!(
                err,
                CellEditError::NonTextContent { .. } | CellEditError::TargetConflict { .. }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn empty_string_clears_cell() {
        let mut doc = doc_with(vec![label_form()]);
        let outcome =
            apply_set_cells(&mut doc, &[spec(0, CellTarget::At(GridCoord::new(0, 0)), "")])
                .unwrap();
        assert!(outcome.cells[0].cleared);
        assert_eq!(cell_text(&doc, 0, 0), "");
    }

    // ── spec serde shape (CLI --map / MCP contract) ─────────────

    #[test]
    fn cell_spec_serde_is_flat_and_snake_case() {
        let at: CellSpec = serde_json::from_value(
            serde_json::json!({"table": 0, "at": {"row": 1, "col": 2}, "text": "v"}),
        )
        .unwrap();
        assert_eq!(at.target, CellTarget::At(GridCoord::new(1, 2)));
        let right: CellSpec =
            serde_json::from_value(serde_json::json!({"table": 3, "right_of": "성명", "text": ""}))
                .unwrap();
        assert_eq!(right.target, CellTarget::RightOf("성명".into()));
        let round = serde_json::to_value(&right).unwrap();
        assert_eq!(round, serde_json::json!({"table": 3, "right_of": "성명", "text": ""}));
    }
}
