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
mod tests {
    use super::*;
    use crate::cell_edit::test_package;
    use hwpforge_core::{ImageStore, PageSettings, TableRow};
    use hwpforge_foundation::HwpUnit;

    fn para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }
    fn fixture() -> Vec<u8> {
        let mut document = Document::new();
        for _ in 0..2 {
            let table = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![para("원본")],
                HwpUnit::new(8000).unwrap(),
            )])]);
            document.add_section(Section::with_paragraphs(
                vec![
                    Paragraph::with_runs(
                        vec![Run::table(table, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    ),
                    para("뒤"),
                ],
                PageSettings::a4(),
            ));
        }
        let mut styles = crate::HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(crate::HwpxCharShape::default());
        styles.push_para_shape(crate::HwpxParaShape::default());
        HwpxEncoder::encode(&document.validate().unwrap(), &styles, &ImageStore::new()).unwrap()
    }
    fn specs() -> Vec<CellSpec> {
        (0..2)
            .map(|table| CellSpec {
                table,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "새값".into(),
            })
            .collect()
    }
    fn reset() {
        CELL_REENCODE_CALLS.set(0);
        crate::patch::PRESERVING_SPLICES.set(0);
        crate::patch::FAIL_AFTER_SPLICE.set(None);
        BEFORE_FINAL_PACKAGE_CHECK.with(|hook| *hook.borrow_mut() = None);
    }
    fn planned(bytes: &[u8]) -> (HwpxDocument, RawPackage, PreservingPlan) {
        let decoded = HwpxDecoder::decode(bytes).unwrap();
        let package = RawPackage::read(bytes).unwrap();
        let plan = plan(&decoded, &package, &specs()).unwrap().expect("preserving plan");
        (decoded, package, plan)
    }

    fn shared_run_fixture() -> Vec<u8> {
        let base = test_package::splice(&fixture(), "<hp:tbl ", "<hp:t>앞</hp:t><hp:tbl ");
        test_package::splice(&base, "</hp:tbl>", "</hp:tbl><hp:t/>")
    }

    // 이것을 실패시키는 것: 칸/문단/run/텍스트 중 하나에 Complete를 다시 요구하기.
    #[test]
    fn shared_run_sources_use_the_text_slot() {
        for empty in [false, true] {
            let base = shared_run_fixture();
            let base =
                if empty { test_package::splice(&base, "<hp:t>원본</hp:t>", "") } else { base };
            let (decoded, _, _) = planned(&base);
            let cell = "paragraphs[0].runs[1].table.rows[0].cells[0]";
            for suffix in
                ["", ".paragraphs[0]", ".paragraphs[0].runs[0]", ".paragraphs[0].runs[0].text"]
            {
                let path = CorePath::new(0, format!("{cell}{suffix}"));
                assert!(matches!(
                    decoded.source_maps[0].node(&path).unwrap().copy_source(),
                    SourceCoverage::Incomplete(ranges) if ranges.len() == 1
                ));
            }
            reset();
            HwpxCellEditor::set_cells(&base, &specs()).unwrap();
            assert_eq!(CELL_REENCODE_CALLS.get(), 0);
            assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 2);
        }
    }

    // 이것을 실패시키는 것: 공유 출처의 슬롯 메타데이터 검증 또는 inline 거부를 생략하기.
    #[test]
    fn shared_run_sources_keep_patch_guards() {
        let base = shared_run_fixture();
        let (decoded, package, mut plan) = planned(&base);
        let TextLocator::TextElement { element_start, .. } =
            &mut plan.sections[0].preservation.text_slots[1].locator
        else {
            panic!("text element")
        };
        *element_start += 1;
        reset();
        assert!(apply(&base, &decoded, &package, plan).unwrap().is_none());
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        assert_eq!(CELL_REENCODE_CALLS.get(), 0);

        let base = test_package::splice(&base, "원본</hp:t>", "원본<hp:imaginary/></hp:t>");
        let (decoded, package, plan) = planned(&base);
        reset();
        assert!(apply(&base, &decoded, &package, plan).unwrap().is_none());
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
    }

    // 이것을 실패시키는 것: 공유 출처의 splice 뒤 패키지 또는 대상 검증을 생략하기.
    #[test]
    fn shared_run_sources_keep_final_verification() {
        for corrupt_header in [false, true] {
            let base = shared_run_fixture();
            reset();
            BEFORE_FINAL_PACKAGE_CHECK.with(|hook| {
                *hook.borrow_mut() = Some(if corrupt_header {
                    |bytes| {
                        *bytes = test_package::splice_part(
                            bytes,
                            "Contents/header.xml",
                            "함초롬돋움",
                            "다른 글꼴",
                        );
                    }
                } else {
                    |bytes| {
                        *bytes = test_package::splice(bytes, "새값</hp:t>", "다른값</hp:t>");
                    }
                });
            });
            let error = HwpxCellEditor::set_cells(&base, &specs()).unwrap_err();
            assert!(matches!(
                error,
                CellEditError::Codec(CodecFailure { stage: CodecStage::Verification, .. })
            ));
            assert_eq!(CELL_REENCODE_CALLS.get(), 0);
            assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 2);
        }
    }

    // 이것을 실패시키는 것: 주소 오류를 범위 검사나 admission 뒤로 미루기.
    #[test]
    fn lookup_errors_precede_admission() {
        let base = test_package::two_text_runs(&fixture(), "<hp:t>원본</hp:t>");
        let base = test_package::splice(&base, "</hp:t>", "</hp:t><hp:imaginary/>");
        for target in
            [CellTarget::RightOf("없는 라벨".into()), CellTarget::At(GridCoord::new(50, 0))]
        {
            reset();
            let error =
                HwpxCellEditor::set_cells(&base, &[CellSpec { target, ..specs()[0].clone() }])
                    .unwrap_err();
            assert!(matches!(error, CellEditError::CellNotFound { .. }));
            assert_eq!(CELL_REENCODE_CALLS.get(), 0);
            assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        }
        reset();
        let error =
            HwpxCellEditor::set_cells(&base, &[CellSpec { table: 20, ..specs()[0].clone() }])
                .unwrap_err();
        assert!(matches!(error, CellEditError::TableNotFound { .. }));
        assert_eq!(CELL_REENCODE_CALLS.get(), 0);
    }

    // 이것을 실패시키는 것: 미기록/여러 범위 출처 또는 합성 빈 텍스트를 승인하기.
    #[test]
    fn unrecorded_ambiguous_and_synthetic_sources_are_not_preserving_candidates() {
        let base = fixture();
        for suffix in
            ["", ".paragraphs[0]", ".paragraphs[0].runs[0]", ".paragraphs[0].runs[0].text"]
        {
            for coverage in [
                SourceCoverage::Unrecorded,
                SourceCoverage::Complete(vec![0..1, 2..3]),
                SourceCoverage::Incomplete(vec![0..1, 2..3]),
            ] {
                let (mut decoded, package, _) = planned(&base);
                let path = CorePath::new(
                    0,
                    format!("paragraphs[0].runs[0].table.rows[0].cells[0]{suffix}"),
                );
                decoded.source_maps[0].set_node_coverage(&path, coverage);
                assert!(plan(&decoded, &package, &specs()).unwrap().is_none());
            }
        }
        let base = test_package::splice(
            &base,
            r#"<hp:run charPrIDRef="0"><hp:t>원본</hp:t></hp:run>"#,
            "",
        );
        let decoded = HwpxDecoder::decode(&base).unwrap();
        resolve_edits(&decoded.document, &specs()).unwrap();
        assert!(plan(&decoded, &RawPackage::read(&base).unwrap(), &specs()).unwrap().is_none());
    }

    // 이것을 실패시키는 것: 모순된 슬롯/범위를 None으로 숨겨 재인코드하기.
    #[test]
    fn source_contradictions_are_invariants() {
        for fault in 0..7 {
            let base = fixture();
            let (mut decoded, mut package, _) = planned(&base);
            let path = CorePath::new(
                0,
                "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text",
            );
            match fault {
                0 => {
                    decoded.source_maps[0]
                        .text_slots_mut()
                        .iter_mut()
                        .find(|slot| slot.path == path.relative())
                        .unwrap()
                        .original_text = "다른값".into()
                }
                1 => decoded.source_maps[0].set_node_coverage(
                    &CorePath::new(0, "paragraphs[0].runs[0].table.rows[0].cells[0]"),
                    SourceCoverage::Complete(std::iter::once(0..usize::MAX).collect()),
                ),
                2 => {
                    let slots = decoded.source_maps[0].text_slots_mut();
                    slots[1].path = slots[0].path.clone();
                }
                3 => {
                    let content = package
                        .read_text_entry("Contents/content.hpf")
                        .unwrap()
                        .replace("idref=\"section0\"", "idref=\"TEMP\"")
                        .replace("idref=\"section1\"", "idref=\"section0\"")
                        .replace("idref=\"TEMP\"", "idref=\"section1\"");
                    package.replace_text_entry("Contents/content.hpf", content);
                    package = RawPackage::read(&package.write().unwrap()).unwrap();
                }
                4 => decoded.source_maps[0].set_node_coverage(
                    &CorePath::new(0, "paragraphs[0].runs[0].table.rows[0].cells[0]"),
                    SourceCoverage::Complete(std::iter::once(0..1).collect()),
                ),
                _ => {
                    let slot = decoded.source_maps[0]
                        .text_slots_mut()
                        .iter_mut()
                        .find(|slot| slot.path == path.relative())
                        .unwrap();
                    let TextLocator::TextElement { element_start, content_start, .. } =
                        &mut slot.locator
                    else {
                        panic!("text element")
                    };
                    if fault == 5 {
                        *element_start += 1;
                    } else {
                        *content_start = Some(0);
                    }
                }
            }
            reset();
            assert!(
                matches!(plan(&decoded, &package, &specs()), Err(CellEditError::Invariant { .. })),
                "fault {fault}"
            );
            assert_eq!(CELL_REENCODE_CALLS.get(), 0);
            assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        }
    }

    // 이것을 실패시키는 것: 실제 inline 거부를 오류로 끝내거나 중간 출력으로 재시도하기.
    #[test]
    fn inline_refusal_discards_earlier_section_changes() {
        let base = test_package::splice_part(
            &fixture(),
            "Contents/section1.xml",
            "원본</hp:t>",
            "원본<hp:imaginary/></hp:t>",
        );
        let (decoded, package, plan) = planned(&base);
        let edit = &plan.sections[1];
        reset();
        let error = HwpxPatcher::patch_section_preserving(
            &base,
            1,
            &edit.replacement,
            None,
            Some(&edit.preservation),
        )
        .unwrap_err();
        assert!(matches!(error, PatchError::Rejected(_)));
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        assert!(apply(&base, &decoded, &package, plan).unwrap().is_none());
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 1);
        reset();
        let actual = HwpxCellEditor::set_cells(&base, &specs()).unwrap_err();
        assert_eq!(CELL_REENCODE_CALLS.get(), 1);
        let expected = super::super::set_cells_reencoded(
            &base,
            &specs(),
            HwpxDecoder::decode(&base).unwrap(),
            RawPackage::read(&base).unwrap(),
        )
        .unwrap_err();
        assert_eq!(actual.to_string(), expected.to_string());
    }

    // 이것을 실패시키는 것: splice 뒤 Codec 오류에도 재인코드 fallback하기.
    #[test]
    fn post_splice_codec_failure_does_not_retry() {
        let base = fixture();
        reset();
        crate::patch::FAIL_AFTER_SPLICE.set(Some(1));
        let error = HwpxCellEditor::set_cells(&base, &specs()).unwrap_err();
        assert!(matches!(
            error,
            CellEditError::Codec(CodecFailure { stage: CodecStage::Verification, .. })
        ));
        assert_eq!(CELL_REENCODE_CALLS.get(), 0);
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 2);
        assert_eq!(crate::patch::FAIL_AFTER_SPLICE.get(), None);
    }

    // 이것을 실패시키는 것: Preview가 없을 때 최종 패키지 검증을 생략하기.
    #[test]
    fn final_package_verification_is_unconditional() {
        let base = fixture();
        reset();
        BEFORE_FINAL_PACKAGE_CHECK.with(|hook| {
            *hook.borrow_mut() = Some(|bytes| {
                *bytes = test_package::splice_part(
                    bytes,
                    "Contents/header.xml",
                    "함초롬돋움",
                    "다른 글꼴",
                );
            })
        });
        let error = HwpxCellEditor::set_cells(&base, &specs()).unwrap_err();
        assert!(matches!(
            error,
            CellEditError::Codec(CodecFailure { stage: CodecStage::Verification, .. })
        ));
        assert_eq!(CELL_REENCODE_CALLS.get(), 0);
    }

    // 이것을 실패시키는 것: 승인된 구역 payload의 최종 대상 텍스트 검증 생략하기.
    #[test]
    fn final_cell_text_is_verified() {
        let base = fixture();
        reset();
        BEFORE_FINAL_PACKAGE_CHECK.with(|hook| {
            *hook.borrow_mut() = Some(|bytes| {
                *bytes = test_package::splice(bytes, "새값</hp:t>", "잘못된값</hp:t>");
            })
        });
        let error = HwpxCellEditor::set_cells(&base, &specs()).unwrap_err();
        assert!(matches!(
            error,
            CellEditError::Codec(CodecFailure { stage: CodecStage::Verification, .. })
        ));
        assert_eq!(CELL_REENCODE_CALLS.get(), 0);
    }
    // 이것을 실패시키는 것: 본문 직속 표 조건을 없애거나 여러 run/문단을 보존 후보로 승인하기.
    #[test]
    fn plan_returns_none_for_out_of_scope_cells() {
        for container in 0..3 {
            let base = fixture();
            let mut decoded = HwpxDecoder::decode(&base).unwrap();
            let host = decoded.document.sections()[0].paragraphs[0].clone();
            let unit = HwpUnit::new(8000).unwrap();
            let run = match container {
                0 => Run::table(
                    Table::new(vec![TableRow::new(vec![TableCell::new(vec![host], unit)])]),
                    CharShapeIndex::new(0),
                ),
                1 => Run::control(
                    hwpforge_core::Control::text_box(vec![host], unit, unit),
                    CharShapeIndex::new(0),
                ),
                _ => Run::control(
                    hwpforge_core::Control::footnote(vec![host]),
                    CharShapeIndex::new(0),
                ),
            };
            decoded.document.sections_mut()[0].paragraphs[0].runs = vec![run];
            let bytes = HwpxEncoder::encode(
                &decoded.document.validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
            )
            .unwrap();
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let edits = [CellSpec { table: usize::from(container == 0), ..specs()[0].clone() }];
            resolve_edits(&decoded.document, &edits).unwrap();
            assert!(
                plan(&decoded, &RawPackage::read(&bytes).unwrap(), &edits).unwrap().is_none(),
                "container {container}"
            );
        }
        for (from, to) in [
            ("원본</hp:t>", "원본</hp:t><hp:t>둘</hp:t>"),
            ("원본</hp:t>", "원본<hp:tab width=\"4000\" leader=\"0\" type=\"1\"/></hp:t>"),
            ("</hp:subList>", "<hp:p id=\"999\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" columnBreak=\"0\" merged=\"0\"><hp:run charPrIDRef=\"0\"><hp:t>둘</hp:t></hp:run></hp:p></hp:subList>"),
        ] {
            let bytes = test_package::splice(&fixture(), from, to);
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            resolve_edits(&decoded.document, &specs()).unwrap();
            assert!(plan(&decoded, &RawPackage::read(&bytes).unwrap(), &specs()).unwrap().is_none());
        }
        let bytes = test_package::two_text_runs(&fixture(), "<hp:t>원본</hp:t>");
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(plan(&decoded, &RawPackage::read(&bytes).unwrap(), &specs()).unwrap().is_none());
    }

    // 이것을 실패시키는 것: patch 지원 거부가 splice 뒤에 일어나도록 이동하기.
    #[test]
    fn patch_refusals_precede_splicing() {
        let base = fixture();
        let (_, _, plan) = planned(&base);
        let edit = &plan.sections[0];
        for fault in 0..5 {
            let mut metadata = edit.preservation.clone();
            match fault {
                0 => metadata.preservation_version = 0,
                1 => metadata.section_path = "Contents/elsewhere.xml".into(),
                2 => metadata.section_sha256 = "wrong".into(),
                3 => metadata.text_slots.clear(),
                _ => {}
            }
            reset();
            let error = HwpxPatcher::patch_section_preserving(
                &base,
                0,
                &edit.replacement,
                None,
                if fault == 4 { None } else { Some(&metadata) },
            )
            .unwrap_err();
            assert!(matches!(error, PatchError::Rejected(_)));
            assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        }
        reset();
        HwpxPatcher::patch_section_preserving(
            &base,
            0,
            &edit.replacement,
            None,
            Some(&edit.preservation),
        )
        .unwrap();
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 1);
    }

    // 이것을 실패시키는 것: header 정규화 필요 또는 참조 구성 실패를 보존으로 승인하기.
    #[test]
    fn header_reencoding_requirements_keep_existing_route() {
        let base = fixture();
        let header = test_package::part(&base, "Contents/header.xml");
        let raw = header.replace(
            "<hc:left value=\"0\" unit=\"HWPUNIT\"/>",
            "<hc:left value=\"2\" unit=\"HWPUNIT\"/>",
        );
        assert_ne!(raw, header);
        let bytes = test_package::splice_part(&base, "Contents/header.xml", &header, &raw);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(decoded.requires_header_normalization());
        assert!(plan(&decoded, &RawPackage::read(&bytes).unwrap(), &specs()).unwrap().is_none());
        let (mut decoded, package, _) = planned(&base);
        decoded.style_store = crate::HwpxStyleStore::new();
        assert!(plan(&decoded, &package, &specs()).unwrap().is_none());
    }

    // 이것을 실패시키는 것: 최종 표/구역/칸/경로/값 일치 조건 중 하나를 생략하기.
    #[test]
    fn final_cell_addresses_and_shape_must_match_the_plan() {
        let base = fixture();
        for fault in 0..6 {
            let (_, _, mut plan) = planned(&base);
            // Match the original bytes before independently corrupting one planned fact.
            for section in &mut plan.sections {
                for target in &mut section.targets {
                    target.text = "원본".into();
                }
            }
            match fault {
                0 => plan.sections[0].targets[0].table = usize::MAX,
                1 => plan.sections[0].targets[0].table = 1,
                2 => plan.sections[0].targets[0].row_idx = usize::MAX,
                3 => plan.sections[0].targets[0].path = CorePath::new(0, "wrong"),
                4 => plan.sections[0].targets[0].text = "다른값".into(),
                _ => plan.sections[0].section = 50,
            }
            assert!(verify_cells(&base, &plan.sections).is_err(), "fault {fault}");
        }
        for multiple_runs in [false, true] {
            let bytes = if multiple_runs {
                test_package::two_text_runs(&base, "<hp:t>원본</hp:t>")
            } else {
                test_package::splice(&base, "</hp:subList>", "<hp:p id=\"999\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" columnBreak=\"0\" merged=\"0\"><hp:run charPrIDRef=\"0\"><hp:t>추가</hp:t></hp:run></hp:p></hp:subList>")
            };
            let (_, _, mut plan) = planned(&base);
            for section in &mut plan.sections {
                for target in &mut section.targets {
                    target.text = if multiple_runs && section.section == 0 {
                        "A".into()
                    } else {
                        "원본".into()
                    };
                }
            }
            assert!(verify_cells(&bytes, &plan.sections).is_err());
        }
    }
    // 이것을 실패시키는 것: 실제 ProtectedFieldRange를 fallback 대신 오류로 반환하기.
    #[test]
    fn protected_field_refusal_is_a_typed_fallback() {
        let base = test_package::splice(
            &fixture(),
            "<hp:t>뒤</hp:t>",
            r#"<hp:ctrl><hp:fieldBegin id="7" type="BOOKMARK" name="p" editable="0" dirty="1" zorder="-1" fieldid="7"/></hp:ctrl><hp:t>뒤</hp:t><hp:ctrl><hp:fieldEnd beginIDRef="7" fieldid="7"/></hp:ctrl>"#,
        );
        let (decoded, package, mut plan) = planned(&base);
        for run in &mut plan.sections[0].replacement.paragraphs[1].runs {
            if let RunContent::Text(text) = &mut run.content {
                *text = "다른본문".into();
            }
        }
        reset();
        let edit = &plan.sections[0];
        let error = HwpxPatcher::patch_section_preserving(
            &base,
            0,
            &edit.replacement,
            None,
            Some(&edit.preservation),
        )
        .unwrap_err();
        assert!(matches!(error, PatchError::ProtectedFieldRange { .. }));
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
        assert!(apply(&base, &decoded, &package, plan).unwrap().is_none());
        assert_eq!(crate::patch::PRESERVING_SPLICES.get(), 0);
    }
}
