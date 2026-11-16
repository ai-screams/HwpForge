//! Translate cell addresses into the existing preserving text patch.
use super::*;
use crate::decoder::source_map::{SourceCoverage, SourceMap};
use crate::diagnostics::WithCodecWarnings;
use crate::exchange::{PreservedTextSlot, SectionPreservation, TextLocator};
use crate::patch::{HwpxPatcher, PatchError, RawPackage};
use crate::raw_package::{preview::PreviewRemoval, OutputChanges};
use hwpforge_core::node_path::{CorePath, NodeKind};
use hwpforge_core::Section;
use std::collections::BTreeMap;
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static BEFORE_FINAL_PACKAGE_CHECK: std::cell::RefCell<Option<fn(&mut Vec<u8>)>> = const { std::cell::RefCell::new(None) };
    pub(super) static CELL_REENCODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) struct PreservingPlan {
    sections: Vec<SectionTextEdit>,
    outcome: SetCellOutcome,
    cleanup_warnings: Vec<EncodeWarning>,
}
struct SectionTextEdit {
    section: usize,
    replacement: Section,
    preservation: SectionPreservation,
    targets: Vec<CellTextTarget>,
}
struct CellTextTarget {
    table: usize,
    row_idx: usize,
    cell_idx: usize,
    path: CorePath,
    text: String,
}

fn invariant(detail: impl Into<String>) -> CellEditError {
    CellEditError::Invariant { detail: detail.into() }
}
fn codec(stage: CodecStage) -> impl FnOnce(crate::HwpxError) -> CellEditError {
    move |error| CellEditError::Codec(CodecFailure::hwpx(stage, &error))
}
fn patch_error(error: PatchError) -> CellEditError {
    match error {
        PatchError::Codec(failure) => CellEditError::Codec(failure),
        PatchError::Invariant { detail } => invariant(detail),
        other => invariant(other.to_string()),
    }
}
fn cell_path(entry: &crate::table_inventory::TableEntry<'_>, row: usize, cell: usize) -> String {
    format!("{}.rows[{row}].cells[{cell}]", crate::table_inventory::core_path(entry).relative())
}

// Text splicing needs one recorded location, not an independently copyable node.
// A shared ancestor wire run marks descendants Incomplete even when their slot
// is exact. A node with an unrecorded contributing element carries the same
// mark; the slot text comparison and the final cell value check catch that case.
// Patch rebuilds and checks the slots before splicing; containment and
// locator agreement below still reject contradictory recorded provenance.
fn source_range(
    map: &SourceMap,
    path: &CorePath,
    kind: NodeKind,
    part: &str,
    xml: &str,
) -> Result<Option<Range<usize>>, CellEditError> {
    let Some(node) = map.node(path) else { return Ok(None) };
    if node.kind() != kind || node.part() != part {
        return Err(invariant(format!("source kind/part differs at {}", path.relative())));
    }
    let (SourceCoverage::Complete(ranges) | SourceCoverage::Incomplete(ranges)) =
        node.copy_source()
    else {
        return Ok(None);
    };
    let [range] = ranges.as_slice() else { return Ok(None) };
    if range.is_empty() || xml.get(range.clone()).is_none() {
        return Err(invariant(format!("invalid source range at {}", path.relative())));
    }
    Ok(Some(range.clone()))
}
fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
fn check_source(
    map: &SourceMap,
    path: &CorePath,
    part: &str,
    xml: &str,
    old: &str,
    slots: &HashMap<&str, &PreservedTextSlot>,
) -> Result<bool, CellEditError> {
    let cell = path.relative().strip_suffix(".paragraphs[0].runs[0].text").unwrap();
    let paths = [
        (cell.to_owned(), NodeKind::Cell),
        (format!("{cell}.paragraphs[0]"), NodeKind::Paragraph),
        (format!("{cell}.paragraphs[0].runs[0]"), NodeKind::Run),
        (path.relative().to_owned(), NodeKind::Text),
    ];
    let mut ranges = Vec::new();
    for (relative, kind) in paths {
        let Some(range) =
            source_range(map, &CorePath::new(path.section(), relative), kind, part, xml)?
        else {
            return Ok(false);
        };
        if ranges.last().is_some_and(|outer| !contains(outer, &range)) {
            return Err(invariant(format!("source containment differs at {}", path.relative())));
        }
        ranges.push(range);
    }
    let Some(slot) = slots.get(path.relative()) else { return Ok(false) };
    if slot.original_text != old {
        return Err(invariant(format!("source text differs at {}", path.relative())));
    }
    let locator = match &slot.locator {
        TextLocator::TextElement { element_start, element_end, content_start, content_end } => {
            let outer = *element_start..*element_end;
            match (content_start, content_end) {
                (Some(start), Some(end)) if start <= end && contains(&outer, &(*start..*end)) => {}
                (None, None) => {}
                _ => return Err(invariant("inconsistent text content range")),
            }
            outer
        }
        TextLocator::EmptyRun { run_start, run_end } => *run_start..*run_end,
    };
    if locator != ranges[3] {
        return Err(invariant(format!("source locator differs at {}", path.relative())));
    }
    Ok(true)
}

pub(super) fn plan(
    decoded: &HwpxDocument,
    package: &RawPackage,
    specs: &[CellSpec],
) -> Result<Option<PreservingPlan>, CellEditError> {
    if specs.is_empty() {
        return Ok(None);
    }
    let (cleaned, removed): (Vec<_>, Vec<_>) = specs
        .iter()
        .map(|spec| {
            let (text, count) = crate::wire_xml::clean_edit_text(&spec.text);
            let text = crate::wire_xml::body_text_line_breaks(&text, true).into_owned();
            (CellSpec { text, ..spec.clone() }, count)
        })
        .unzip();
    let edits = resolve_edits(&decoded.document, &cleaned)?;
    // A header reference construction failure belongs to the existing admission contract.
    if !matches!(
        crate::patch::needs_header_reencoding(
            decoded.requires_header_normalization(),
            &decoded.style_store
        ),
        Ok(false)
    ) {
        return Ok(None);
    }
    let entries = tables_in_document(&decoded.document);
    let mut grouped: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    let mut cleanup_warnings = Vec::new();
    for (edit, count) in edits.iter().zip(removed) {
        let entry = &entries[edit.table];
        let [PathSeg::Field("paragraphs"), PathSeg::Index(pi), PathSeg::Field("runs"), PathSeg::Index(ri), PathSeg::Field("content"), PathSeg::Field("Table")] =
            entry.path.as_slice()
        else {
            return Ok(None);
        };
        let cell = &entry.table.rows[edit.row_idx].cells[edit.cell_idx];
        let [paragraph] = cell.paragraphs.as_slice() else { return Ok(None) };
        let [run] = paragraph.runs.as_slice() else { return Ok(None) };
        let RunContent::Text(old) = &run.content else { return Ok(None) };
        let path = CorePath::new(
            entry.section,
            format!("{}.paragraphs[0].runs[0].text", cell_path(entry, edit.row_idx, edit.cell_idx)),
        );
        grouped.entry(entry.section).or_default().push((edit, *pi, *ri, path, old));
        if count > 0 {
            cleanup_warnings.push(EncodeWarning::XmlForbiddenCharsRemoved {
                part: package.section_path(entry.section).map_err(|e| invariant(e.to_string()))?,
                location: format!(
                    "table {} cell ({}, {})",
                    edit.table, edit.anchor.row, edit.anchor.col
                ),
                count,
            });
        }
    }
    let mut sections = Vec::new();
    for (section, edits) in grouped {
        let Some(map) = decoded.source_map(section) else { return Ok(None) };
        let part = package.section_path(section).map_err(|e| invariant(e.to_string()))?;
        let xml = package.read_text_entry(&part).map_err(codec(CodecStage::Decode))?;
        let mut slots = HashMap::new();
        for slot in map.text_slots() {
            if slots.insert(slot.path.as_str(), slot).is_some() {
                return Err(invariant("duplicate source text path"));
            }
        }
        for (_, _, _, path, old) in &edits {
            if !check_source(map, path, &part, &xml, old, &slots)? {
                return Ok(None);
            }
        }
        let original = &decoded.document.sections()[section];
        let preservation =
            match crate::patch::preservation_from_map(&xml, &part, original, map, section, None) {
                Ok(value) => value,
                Err(PatchError::Rejected(_)) => return Ok(None),
                Err(error) => return Err(patch_error(error)),
            };
        let mut editable_paths: HashMap<&str, usize> = HashMap::new();
        for slot in &preservation.text_slots {
            *editable_paths.entry(&slot.path).or_default() += 1;
        }
        let mut replacement = original.clone();
        let mut targets = Vec::new();
        let mut changed = false;
        for (edit, pi, ri, path, old) in edits {
            if editable_paths.get(path.relative()) != Some(&1) {
                return Err(invariant("target does not have exactly one editable slot"));
            }
            changed |= old != &edit.text;
            let RunContent::Table(table) = &mut replacement.paragraphs[pi].runs[ri].content else {
                return Err(invariant("planned table path differs"));
            };
            table.rows[edit.row_idx].cells[edit.cell_idx].paragraphs[0].runs[0].content =
                RunContent::Text(edit.text.clone());
            targets.push(CellTextTarget {
                table: edit.table,
                row_idx: edit.row_idx,
                cell_idx: edit.cell_idx,
                path,
                text: edit.text.clone(),
            });
        }
        if changed {
            sections.push(SectionTextEdit { section, replacement, preservation, targets });
        }
    }
    Ok(Some(PreservingPlan { sections, outcome: outcome_for(&edits), cleanup_warnings }))
}

pub(super) fn apply(
    base: &[u8],
    decoded: &HwpxDocument,
    package: &RawPackage,
    plan: PreservingPlan,
) -> Result<Option<WithCodecWarnings<CellEditResult>>, CellEditError> {
    let PreservingPlan { sections, outcome, mut cleanup_warnings } = plan;
    let mut bytes = base.to_vec();
    if !sections.is_empty() {
        let mut changes = OutputChanges::default();
        for edit in &sections {
            let result = match HwpxPatcher::patch_section_preserving_with_changes(
                &bytes,
                edit.section,
                &edit.replacement,
                None,
                Some(&edit.preservation),
                Some(&mut changes),
            ) {
                Ok(result) => result,
                Err(PatchError::Rejected(_) | PatchError::ProtectedFieldRange { .. }) => {
                    return Ok(None)
                }
                Err(error) => return Err(patch_error(error)),
            };
            let (loss, others) = crate::encoder::partition_semantic_loss(result.encode_warnings);
            if !loss.is_empty() {
                return Err(CellEditError::SemanticLoss { warnings: loss, others });
            }
            cleanup_warnings.extend(
                others.into_iter().filter(|w| !matches!(w, EncodeWarning::PreviewRemoved { .. })),
            );
            bytes = result.value;
        }
        changes.rewritten.sort();
        changes.rewritten.dedup();
        changes.deleted.sort();
        changes.deleted.dedup();
        #[cfg(test)]
        BEFORE_FINAL_PACKAGE_CHECK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook(&mut bytes);
            }
        });
        package.verify_output_with(base, &bytes, &changes).map_err(CellEditError::Codec)?;
        cleanup_warnings.extend(
            PreviewRemoval::plan(package)
                .map_err(codec(CodecStage::Verification))?
                .verify_final(&bytes)
                .map_err(codec(CodecStage::Verification))?,
        );
        verify_cells(&bytes, &sections)?;
    }
    Ok(Some(WithCodecWarnings::new(
        CellEditResult { bytes, outcome },
        decoded.warnings.clone(),
        cleanup_warnings,
    )))
}

fn verify_cells(bytes: &[u8], sections: &[SectionTextEdit]) -> Result<(), CellEditError> {
    let decoded = HwpxDecoder::decode(bytes).map_err(codec(CodecStage::Verification))?;
    let entries = tables_in_document(&decoded.document);
    for section in sections {
        let map = decoded
            .source_map(section.section)
            .ok_or_else(|| verification("missing output source map"))?;
        let slots: HashMap<_, _> =
            map.text_slots().iter().map(|slot| (slot.path.as_str(), slot)).collect();
        for target in &section.targets {
            let entry =
                entries.get(target.table).ok_or_else(|| verification("missing output table"))?;
            if entry.section != section.section {
                return Err(verification("output table section differs"));
            }
            let path = format!(
                "{}.paragraphs[0].runs[0].text",
                cell_path(entry, target.row_idx, target.cell_idx)
            );
            if path != target.path.relative() {
                return Err(invariant("planned target path differs"));
            }
            let cell = entry
                .table
                .rows
                .get(target.row_idx)
                .and_then(|row| row.cells.get(target.cell_idx))
                .ok_or_else(|| verification("missing output cell"))?;
            if cell.paragraphs.len() != 1
                || cell.paragraphs[0].runs.len() != 1
                || slots.get(path.as_str()).is_none_or(|slot| slot.original_text != target.text)
            {
                return Err(verification("output cell text or shape differs"));
            }
        }
    }
    Ok(())
}
fn verification(detail: &str) -> CellEditError {
    CellEditError::Codec(CodecFailure::hwpx(
        CodecStage::Verification,
        &crate::HwpxError::InvalidStructure { detail: detail.into() },
    ))
}

#[cfg(test)]
#[path = "preserving_tests.rs"]
mod tests;
