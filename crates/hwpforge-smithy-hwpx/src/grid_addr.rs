//! Logical grid addresses on JSON exports (E3 Wave 2).
//!
//! Export side ([`annotate_document_addresses`] / [`annotate_section_addresses`]):
//! walks the exported `serde_json::Value` in lockstep with the typed document
//! (via the shared table inventory) and adds `"addr": {"row", "col"}` — the
//! cell's pre-merge logical grid anchor — to every table cell whose table
//! tiles a well-formed grid. Tables that fail strict grid derivation are left
//! unannotated and reported as warnings (warning-first).
//!
//! Import side ([`verify_document_addresses`] / [`verify_section_addresses`]):
//! a supplied `addr` is **validated, then discarded** (deserialization already
//! ignores unknown fields). Absent addresses mean "no check" — callers that
//! edit structure should simply drop the `addr` fields. A present address
//! that does not match the re-derived grid is rejected with the first
//! mismatching path, so callers can never act on an address the document does
//! not actually have (no fake support).

use serde_json::Value;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::section::Section;
use hwpforge_core::table::grid::{GridCoord, TableGrid};

use crate::table_inventory::{
    render_path, tables_in_document, tables_in_section, PathSeg, TableEntry,
};

/// A table that could not be annotated with grid addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridAddrWarning {
    /// Section index of the table.
    pub section: usize,
    /// Table ordinal in the shared traversal order.
    pub table_ordinal: usize,
    /// Why grid derivation failed (first tiling violation).
    pub reason: String,
}

/// Why an exported/imported JSON tree could not be processed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridAddrError {
    /// The JSON tree does not match the typed document's serde shape.
    ShapeMismatch {
        /// Diagnostic path of the first navigation failure.
        path: String,
    },
    /// A supplied `addr` is not a `{"row", "col"}` object of integers.
    AddrMalformed {
        /// Diagnostic path of the malformed address.
        path: String,
    },
    /// A supplied `addr` disagrees with the re-derived grid anchor.
    AddrMismatch {
        /// Diagnostic path of the mismatching cell.
        path: String,
        /// Address supplied by the caller.
        supplied: GridCoord,
        /// Anchor the grid actually derives for this cell.
        expected: GridCoord,
    },
    /// An `addr` was supplied on a table whose grid cannot be derived.
    AddrOnUnaddressableTable {
        /// Diagnostic path of the table.
        path: String,
        /// Why grid derivation failed.
        reason: String,
    },
}

impl core::fmt::Display for GridAddrError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ShapeMismatch { path } => {
                write!(f, "exported JSON does not match the document shape at {path}")
            }
            Self::AddrMalformed { path } => {
                write!(f, "cell addr at {path} is not a {{row, col}} integer object")
            }
            Self::AddrMismatch { path, supplied, expected } => write!(
                f,
                "cell addr at {path} is ({}, {}) but the derived grid anchor is ({}, {})",
                supplied.row, supplied.col, expected.row, expected.col
            ),
            Self::AddrOnUnaddressableTable { path, reason } => write!(
                f,
                "cell addr supplied at {path} but the table grid cannot be derived: {reason}"
            ),
        }
    }
}

impl std::error::Error for GridAddrError {}

/// Annotates a full-document export (`{"document": ...}` root) with cell
/// grid addresses. Returns warnings for tables left unannotated.
///
/// # Errors
///
/// [`GridAddrError::ShapeMismatch`] when the JSON tree does not correspond to
/// `document` — the export and the value must come from the same source.
pub fn annotate_document_addresses(
    root: &mut Value,
    document: &Document<Draft>,
) -> Result<Vec<GridAddrWarning>, GridAddrError> {
    let mut warnings = Vec::new();
    for entry in tables_in_document(document) {
        let prefix = vec![
            PathSeg::Field("document"),
            PathSeg::Field("sections"),
            PathSeg::Index(entry.section),
        ];
        annotate_table(root, &prefix, &entry, &mut warnings)?;
    }
    Ok(warnings)
}

/// Annotates a section export (`{"section": ...}` root) with cell grid
/// addresses (section-local table ordinals).
///
/// # Errors
///
/// [`GridAddrError::ShapeMismatch`] when the JSON tree does not correspond to
/// `section`.
pub fn annotate_section_addresses(
    root: &mut Value,
    section: &Section,
    section_idx: usize,
) -> Result<Vec<GridAddrWarning>, GridAddrError> {
    let mut warnings = Vec::new();
    for entry in tables_in_section(section, section_idx) {
        let prefix = vec![PathSeg::Field("section")];
        annotate_table(root, &prefix, &entry, &mut warnings)?;
    }
    Ok(warnings)
}

/// Verifies any supplied cell addresses in a full-document import.
///
/// # Errors
///
/// See [`GridAddrError`] — shape mismatches, malformed addresses, addresses
/// on unaddressable tables, and anchor mismatches are all rejected.
pub fn verify_document_addresses(
    root: &Value,
    document: &Document<Draft>,
) -> Result<(), GridAddrError> {
    for entry in tables_in_document(document) {
        let prefix = vec![
            PathSeg::Field("document"),
            PathSeg::Field("sections"),
            PathSeg::Index(entry.section),
        ];
        verify_table(root, &prefix, &entry)?;
    }
    Ok(())
}

/// Verifies any supplied cell addresses in a section import.
///
/// # Errors
///
/// See [`GridAddrError`].
pub fn verify_section_addresses(
    root: &Value,
    section: &Section,
    section_idx: usize,
) -> Result<(), GridAddrError> {
    for entry in tables_in_section(section, section_idx) {
        let prefix = vec![PathSeg::Field("section")];
        verify_table(root, &prefix, &entry)?;
    }
    Ok(())
}

fn annotate_table(
    root: &mut Value,
    prefix: &[PathSeg],
    entry: &TableEntry<'_>,
    warnings: &mut Vec<GridAddrWarning>,
) -> Result<(), GridAddrError> {
    let grid = match TableGrid::from_table(entry.table) {
        Ok(grid) => grid,
        Err(err) => {
            warnings.push(GridAddrWarning {
                section: entry.section,
                table_ordinal: entry.ordinal,
                reason: err.to_string(),
            });
            return Ok(());
        }
    };

    for anchor in grid.iter_anchors() {
        let cell_path = cell_path(prefix, entry, anchor.row_idx, anchor.cell_idx);
        let cell = nav_mut(root, &cell_path)?;
        let obj = cell
            .as_object_mut()
            .ok_or_else(|| GridAddrError::ShapeMismatch { path: render_segs(&cell_path) })?;
        obj.insert(
            "addr".to_string(),
            serde_json::json!({ "row": anchor.anchor.row, "col": anchor.anchor.col }),
        );
    }
    Ok(())
}

fn verify_table(
    root: &Value,
    prefix: &[PathSeg],
    entry: &TableEntry<'_>,
) -> Result<(), GridAddrError> {
    // Collect supplied addresses first so grid derivation only runs (and can
    // only reject) when the caller actually asserted addresses.
    let mut supplied: Vec<(usize, usize, GridCoord, Vec<PathSeg>)> = Vec::new();
    for (row_idx, row) in entry.table.rows.iter().enumerate() {
        for cell_idx in 0..row.cells.len() {
            let path = cell_path(prefix, entry, row_idx, cell_idx);
            let cell = nav(root, &path)?;
            let Some(addr) = cell.get("addr") else { continue };
            let coord = parse_addr(addr)
                .ok_or_else(|| GridAddrError::AddrMalformed { path: render_segs(&path) })?;
            supplied.push((row_idx, cell_idx, coord, path));
        }
    }
    if supplied.is_empty() {
        return Ok(());
    }

    let grid = TableGrid::from_table(entry.table).map_err(|err| {
        GridAddrError::AddrOnUnaddressableTable {
            path: render_segs(&full_table_path(prefix, entry)),
            reason: err.to_string(),
        }
    })?;
    let mut anchors = std::collections::HashMap::new();
    for anchor in grid.iter_anchors() {
        anchors.insert((anchor.row_idx, anchor.cell_idx), anchor.anchor);
    }
    for (row_idx, cell_idx, coord, path) in supplied {
        let expected = anchors
            .get(&(row_idx, cell_idx))
            .copied()
            .ok_or_else(|| GridAddrError::ShapeMismatch { path: render_segs(&path) })?;
        if expected != coord {
            return Err(GridAddrError::AddrMismatch {
                path: render_segs(&path),
                supplied: coord,
                expected,
            });
        }
    }
    Ok(())
}

fn parse_addr(addr: &Value) -> Option<GridCoord> {
    let obj = addr.as_object()?;
    if obj.len() != 2 {
        return None;
    }
    let row = obj.get("row")?.as_u64()?;
    let col = obj.get("col")?.as_u64()?;
    Some(GridCoord::new(u32::try_from(row).ok()?, u32::try_from(col).ok()?))
}

fn full_table_path(prefix: &[PathSeg], entry: &TableEntry<'_>) -> Vec<PathSeg> {
    let mut path = prefix.to_vec();
    path.extend_from_slice(&entry.path);
    path
}

fn cell_path(
    prefix: &[PathSeg],
    entry: &TableEntry<'_>,
    row_idx: usize,
    cell_idx: usize,
) -> Vec<PathSeg> {
    let mut path = full_table_path(prefix, entry);
    path.push(PathSeg::Field("rows"));
    path.push(PathSeg::Index(row_idx));
    path.push(PathSeg::Field("cells"));
    path.push(PathSeg::Index(cell_idx));
    path
}

fn render_segs(path: &[PathSeg]) -> String {
    render_path("", path)
}

fn nav<'v>(mut node: &'v Value, path: &[PathSeg]) -> Result<&'v Value, GridAddrError> {
    for (i, seg) in path.iter().enumerate() {
        let next = match seg {
            PathSeg::Field(name) => node.get(*name),
            PathSeg::Index(idx) => node.get(*idx),
        };
        node =
            next.ok_or_else(|| GridAddrError::ShapeMismatch { path: render_segs(&path[..=i]) })?;
    }
    Ok(node)
}

fn nav_mut<'v>(mut node: &'v mut Value, path: &[PathSeg]) -> Result<&'v mut Value, GridAddrError> {
    for (i, seg) in path.iter().enumerate() {
        let next = match seg {
            PathSeg::Field(name) => node.get_mut(*name),
            PathSeg::Index(idx) => node.get_mut(*idx),
        };
        node =
            next.ok_or_else(|| GridAddrError::ShapeMismatch { path: render_segs(&path[..=i]) })?;
    }
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::ExportedDocument;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn cell(row_span: u16, col_span: u16) -> TableCell {
        TableCell::with_span(vec![text_para("칸")], HwpUnit::new(8000).unwrap(), col_span, row_span)
    }

    /// blank-HPC #11 layout: 4×3, 8 anchors.
    fn merged_table() -> Table {
        Table::new(vec![
            TableRow::new(vec![cell(2, 1), cell(1, 2)]),
            TableRow::new(vec![cell(1, 1), cell(1, 1)]),
            TableRow::new(vec![cell(1, 1), cell(1, 1), cell(1, 1)]),
            TableRow::new(vec![cell(1, 3)]),
        ])
    }

    fn doc_with(table: Table) -> Document<Draft> {
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(table, CharShapeIndex::new(0)));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));
        doc
    }

    fn export_value(doc: &Document<Draft>) -> Value {
        serde_json::to_value(ExportedDocument {
            document: serde_json::from_value(serde_json::to_value(doc).unwrap()).unwrap(),
            styles: None,
        })
        .unwrap()
    }

    fn cell_value(root: &Value, row: usize, cell: usize) -> &Value {
        &root["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Table"]["rows"]
            [row]["cells"][cell]
    }

    #[test]
    fn annotate_adds_anchor_addresses_to_every_cell() {
        let doc = doc_with(merged_table());
        let mut root = export_value(&doc);
        let warnings = annotate_document_addresses(&mut root, &doc).expect("annotate");
        assert!(warnings.is_empty());

        assert_eq!(cell_value(&root, 0, 0)["addr"], serde_json::json!({"row": 0, "col": 0}));
        assert_eq!(cell_value(&root, 0, 1)["addr"], serde_json::json!({"row": 0, "col": 1}));
        // Row 1 cells land at columns 1 and 2 (column 0 covered from above).
        assert_eq!(cell_value(&root, 1, 0)["addr"], serde_json::json!({"row": 1, "col": 1}));
        assert_eq!(cell_value(&root, 1, 1)["addr"], serde_json::json!({"row": 1, "col": 2}));
        assert_eq!(cell_value(&root, 3, 0)["addr"], serde_json::json!({"row": 3, "col": 0}));
    }

    #[test]
    fn annotate_skips_untiled_table_with_warning() {
        // Ragged table: grid underivable → no addr, one warning.
        let ragged = Table::new(vec![
            TableRow::new(vec![cell(1, 1), cell(1, 1)]),
            TableRow::new(vec![cell(1, 1)]),
        ]);
        let doc = doc_with(ragged);
        let mut root = export_value(&doc);
        let warnings = annotate_document_addresses(&mut root, &doc).expect("annotate");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("no cell covers"));
        assert!(cell_value(&root, 0, 0).get("addr").is_none());
    }

    #[test]
    #[ignore = "perf probe (E3 L5): run manually — numbers recorded in the planning doc"]
    fn perf_probe_annotate_and_verify_on_large_table() {
        // 100×100 = 10,000 cells — well above the largest corpus table
        // (414 logical positions). Probes the per-cell root-`nav` cost that
        // the E3 review flagged; refactor only if these numbers matter.
        let big = Table::new(
            (0..100).map(|_| TableRow::new((0..100).map(|_| cell(1, 1)).collect())).collect(),
        );
        let doc = doc_with(big);
        let mut root = export_value(&doc);

        let t0 = std::time::Instant::now();
        annotate_document_addresses(&mut root, &doc).expect("annotate");
        let annotate = t0.elapsed();

        let t1 = std::time::Instant::now();
        verify_document_addresses(&root, &doc).expect("verify");
        let verify = t1.elapsed();

        eprintln!("perf-probe 10k cells: annotate={annotate:?} verify={verify:?}");
    }

    #[test]
    fn verify_accepts_annotated_export_and_absent_addrs() {
        let doc = doc_with(merged_table());
        let mut root = export_value(&doc);
        annotate_document_addresses(&mut root, &doc).expect("annotate");
        verify_document_addresses(&root, &doc).expect("verify annotated");

        // Absence = no check.
        let bare = export_value(&doc);
        verify_document_addresses(&bare, &doc).expect("verify bare");
    }

    #[test]
    fn verify_rejects_tampered_addr_with_first_mismatch_path() {
        let doc = doc_with(merged_table());
        let mut root = export_value(&doc);
        annotate_document_addresses(&mut root, &doc).expect("annotate");
        root["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Table"]["rows"]
            [1]["cells"][0]["addr"] = serde_json::json!({"row": 1, "col": 0});

        let err = verify_document_addresses(&root, &doc).expect_err("tampered addr");
        match err {
            GridAddrError::AddrMismatch { path, supplied, expected } => {
                assert!(path.ends_with("rows[1].cells[0]"), "{path}");
                assert_eq!(supplied, GridCoord::new(1, 0));
                assert_eq!(expected, GridCoord::new(1, 1));
            }
            other => panic!("expected AddrMismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_malformed_and_unaddressable_addrs() {
        let doc = doc_with(merged_table());
        let mut root = export_value(&doc);
        annotate_document_addresses(&mut root, &doc).expect("annotate");
        root["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Table"]["rows"]
            [0]["cells"][0]["addr"] = serde_json::json!({"row": 0});
        assert!(matches!(
            verify_document_addresses(&root, &doc),
            Err(GridAddrError::AddrMalformed { .. })
        ));

        // addr asserted on a table whose grid cannot be derived.
        let ragged = Table::new(vec![
            TableRow::new(vec![cell(1, 1), cell(1, 1)]),
            TableRow::new(vec![cell(1, 1)]),
        ]);
        let ragged_doc = doc_with(ragged);
        let mut ragged_root = export_value(&ragged_doc);
        ragged_root["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Table"]
            ["rows"][0]["cells"][0]["addr"] = serde_json::json!({"row": 0, "col": 0});
        assert!(matches!(
            verify_document_addresses(&ragged_root, &ragged_doc),
            Err(GridAddrError::AddrOnUnaddressableTable { .. })
        ));
    }

    #[test]
    fn shape_mismatch_is_fail_closed() {
        let doc = doc_with(merged_table());
        let mut root = serde_json::json!({ "document": { "sections": [] } });
        assert!(matches!(
            annotate_document_addresses(&mut root, &doc),
            Err(GridAddrError::ShapeMismatch { .. })
        ));
    }

    #[test]
    fn textbox_nested_table_is_annotated_end_to_end() {
        // 워커 경로 서술자가 serde 실형태와 어긋나면 to-json 이
        // GRID_ADDR_PROJECTION_FAILED 로 죽는다 — 컨트롤 컨테이너 내 표의
        // 실제 JSON 항법을 end-to-end 로 잠근다.
        let textbox = hwpforge_core::Control::TextBox {
            paragraphs: vec![{
                let mut p = Paragraph::new(ParaShapeIndex::new(0));
                p.add_run(Run::table(merged_table(), CharShapeIndex::new(0)));
                p
            }],
            caption: None,
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            style: None,
            text_vertical_align: hwpforge_foundation::VerticalAlign::default(),
        };
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::control(textbox, CharShapeIndex::new(0)));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));

        let mut root = export_value(&doc);
        let warnings = annotate_document_addresses(&mut root, &doc).expect("annotate");
        assert!(warnings.is_empty());
        assert_eq!(
            root["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Control"]
                ["TextBox"]["paragraphs"][0]["runs"][0]["content"]["Table"]["rows"][1]["cells"][0]
                ["addr"],
            serde_json::json!({"row": 1, "col": 1})
        );
        verify_document_addresses(&root, &doc).expect("verify");
    }

    #[test]
    fn section_export_round_trip_annotates_and_verifies() {
        let doc = doc_with(merged_table());
        let section = &doc.sections()[0];
        let mut root = serde_json::json!({ "section": serde_json::to_value(section).unwrap() });
        let warnings = annotate_section_addresses(&mut root, section, 0).expect("annotate");
        assert!(warnings.is_empty());
        assert_eq!(
            root["section"]["paragraphs"][0]["runs"][0]["content"]["Table"]["rows"][3]["cells"][0]
                ["addr"],
            serde_json::json!({"row": 3, "col": 0})
        );
        verify_section_addresses(&root, section, 0).expect("verify");
    }
}
