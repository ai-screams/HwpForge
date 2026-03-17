use std::collections::{BTreeMap, BTreeSet};

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_smithy_hwp5::{
    Hwp5SemanticConfidence, Hwp5SemanticContainerKind, Hwp5SemanticControlId,
    Hwp5SemanticControlKind, Hwp5SemanticDocument, Hwp5SemanticParagraph, Hwp5SemanticSection,
};
use hwpforge_smithy_hwpx::{HwpxDocument, HwpxResult, PackageReader};

use crate::analysis::hwpx_paths::{collect_section_path_inventory, HwpxPathOccurrence};

mod table_properties;

use table_properties::{
    append_table_property_notes, summarize_hwp5_table_properties, summarize_hwpx_table_properties,
    DeepTablePropertiesSummary,
};

pub(crate) use table_properties::{DeepTableCellEvidence, DeepTableEvidence};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeepDocumentSummary {
    pub sections: Vec<DeepSectionSummary>,
    pub notes: Vec<String>,
    pub table_properties: DeepTablePropertiesSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeepSectionSummary {
    pub index: usize,
    pub paragraphs: usize,
    pub non_empty_paragraphs: usize,
    pub deep_paragraphs: usize,
    pub deep_non_empty_paragraphs: usize,
    pub tables: usize,
    pub images: usize,
    pub charts: usize,
    pub ole_objects: usize,
    pub text_boxes: usize,
    pub lines: usize,
    pub rectangles: usize,
    pub polygons: usize,
    pub has_header: bool,
    pub has_footer: bool,
    pub has_page_number: bool,
    pub landscape: bool,
    pub first_non_empty_text: Option<String>,
}

impl DeepDocumentSummary {
    pub(crate) fn total_sections(&self) -> usize {
        self.sections.len()
    }

    pub(crate) fn total_paragraphs(&self) -> usize {
        self.sections.iter().map(|section| section.paragraphs).sum()
    }

    pub(crate) fn total_non_empty_paragraphs(&self) -> usize {
        self.sections.iter().map(|section| section.non_empty_paragraphs).sum()
    }

    pub(crate) fn total_tables(&self) -> usize {
        self.sections.iter().map(|section| section.tables).sum()
    }

    pub(crate) fn total_images(&self) -> usize {
        self.sections.iter().map(|section| section.images).sum()
    }

    pub(crate) fn total_text_boxes(&self) -> usize {
        self.sections.iter().map(|section| section.text_boxes).sum()
    }

    pub(crate) fn total_ole_objects(&self) -> usize {
        self.sections.iter().map(|section| section.ole_objects).sum()
    }

    pub(crate) fn total_lines(&self) -> usize {
        self.sections.iter().map(|section| section.lines).sum()
    }

    pub(crate) fn total_rectangles(&self) -> usize {
        self.sections.iter().map(|section| section.rectangles).sum()
    }

    pub(crate) fn total_polygons(&self) -> usize {
        self.sections.iter().map(|section| section.polygons).sum()
    }

    pub(crate) fn total_headers(&self) -> usize {
        self.sections.iter().filter(|section| section.has_header).count()
    }

    pub(crate) fn total_footers(&self) -> usize {
        self.sections.iter().filter(|section| section.has_footer).count()
    }

    pub(crate) fn total_page_numbers(&self) -> usize {
        self.sections.iter().filter(|section| section.has_page_number).count()
    }

    pub(crate) fn total_landscape_sections(&self) -> usize {
        self.sections.iter().filter(|section| section.landscape).count()
    }
}

pub(crate) fn summarize_hwpx_document(
    bytes: &[u8],
    document: &HwpxDocument,
) -> HwpxResult<DeepDocumentSummary> {
    let mut package_reader: PackageReader<'_> = PackageReader::new(bytes)?;
    let path_inventory: Vec<HwpxPathOccurrence> =
        collect_section_path_inventory(&mut package_reader)?;

    let mut occurrences_by_section: BTreeMap<usize, Vec<&HwpxPathOccurrence>> = BTreeMap::new();
    for occurrence in &path_inventory {
        occurrences_by_section.entry(occurrence.section_index).or_default().push(occurrence);
    }

    let sections: Vec<DeepSectionSummary> = document
        .document
        .sections()
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let section_occurrences: &[&HwpxPathOccurrence] =
                occurrences_by_section.get(&index).map(Vec::as_slice).unwrap_or(&[]);
            summarize_hwpx_section(index, section, section_occurrences)
        })
        .collect();

    let mut notes: Vec<String> = Vec::new();
    let total_ole_objects: usize = sections.iter().map(|section| section.ole_objects).sum();
    if total_ole_objects > 0 {
        notes.push(format!("hwpx-ole-fallback-present: {total_ole_objects}"));
    }

    let table_properties = summarize_hwpx_table_properties(document.document.sections());

    append_table_property_notes(&mut notes, &table_properties);

    Ok(DeepDocumentSummary { sections, notes, table_properties })
}

pub(crate) fn summarize_hwp5_semantic(document: &Hwp5SemanticDocument) -> DeepDocumentSummary {
    let sections: Vec<DeepSectionSummary> =
        document.sections.iter().map(summarize_hwp5_semantic_section).collect();

    let mut notes: Vec<String> = Vec::new();
    let total_ole_objects: usize = sections.iter().map(|section| section.ole_objects).sum();
    if total_ole_objects > 0 {
        notes.push(format!("ole-backed-gso-evidence: {total_ole_objects}"));
    }

    let high_confidence_ole: usize = document
        .sections
        .iter()
        .flat_map(|section| section.controls.iter())
        .filter(|control| {
            matches!(control.kind, Hwp5SemanticControlKind::OleObject)
                && matches!(control.confidence, Hwp5SemanticConfidence::High)
        })
        .count();
    if high_confidence_ole > 0 {
        notes.push(format!("ole-high-confidence: {high_confidence_ole}"));
    }

    let unresolved_count: usize = document.unresolved.len();
    if unresolved_count > 0 {
        notes.push(format!("semantic-unresolved-items: {unresolved_count}"));
    }

    let table_properties: DeepTablePropertiesSummary = summarize_hwp5_table_properties(document);
    append_table_property_notes(&mut notes, &table_properties);

    DeepDocumentSummary { sections, notes, table_properties }
}

fn summarize_hwpx_section(
    index: usize,
    section: &Section,
    occurrences: &[&HwpxPathOccurrence],
) -> DeepSectionSummary {
    let paragraphs: usize = section.paragraphs.len();
    let non_empty_paragraphs: usize = section
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph_has_visible_text_deep(paragraph))
        .count();
    let deep_paragraphs: usize = count_paragraphs_recursive(&section.paragraphs)
        + section
            .header
            .as_ref()
            .map_or(0, |header| count_paragraphs_recursive(&header.paragraphs))
        + section
            .footer
            .as_ref()
            .map_or(0, |footer| count_paragraphs_recursive(&footer.paragraphs));
    let deep_non_empty_paragraphs: usize =
        count_non_empty_paragraphs_recursive(&section.paragraphs)
            + section
                .header
                .as_ref()
                .map_or(0, |header| count_non_empty_paragraphs_recursive(&header.paragraphs))
            + section
                .footer
                .as_ref()
                .map_or(0, |footer| count_non_empty_paragraphs_recursive(&footer.paragraphs));

    let raw_rectangles: usize = count_occurrences(occurrences, "rect");
    let text_boxes: usize = count_occurrences(occurrences, "drawText");

    DeepSectionSummary {
        index,
        paragraphs,
        non_empty_paragraphs,
        deep_paragraphs,
        deep_non_empty_paragraphs,
        tables: count_occurrences(occurrences, "tbl"),
        images: count_occurrences(occurrences, "pic"),
        charts: count_occurrences(occurrences, "chart"),
        ole_objects: count_occurrences(occurrences, "ole"),
        text_boxes,
        lines: count_occurrences(occurrences, "line"),
        rectangles: raw_rectangles.saturating_sub(text_boxes),
        polygons: count_occurrences(occurrences, "polygon"),
        has_header: section.header.is_some(),
        has_footer: section.footer.is_some(),
        has_page_number: section.page_number.is_some(),
        landscape: section.page_settings.landscape,
        first_non_empty_text: first_visible_text_in_paragraphs_deep(&section.paragraphs),
    }
}

fn summarize_hwp5_semantic_section(section: &Hwp5SemanticSection) -> DeepSectionSummary {
    let owned_paragraphs_by_control_id: BTreeMap<
        Hwp5SemanticControlId,
        Vec<&Hwp5SemanticParagraph>,
    > = build_semantic_owned_paragraph_map(section);
    let body_paragraphs: Vec<_> = section
        .paragraphs
        .iter()
        .filter(|paragraph| {
            matches!(paragraph.container.terminal_kind(), Hwp5SemanticContainerKind::Body)
        })
        .collect();

    let paragraphs: usize = body_paragraphs.len();
    let non_empty_paragraphs: usize = body_paragraphs
        .iter()
        .filter(|paragraph| {
            semantic_paragraph_has_visible_text_deep(paragraph, &owned_paragraphs_by_control_id)
        })
        .count();
    let deep_paragraphs: usize = section.paragraphs.len();
    let deep_non_empty_paragraphs: usize = section
        .paragraphs
        .iter()
        .filter(|paragraph| {
            semantic_paragraph_has_visible_text_deep(paragraph, &owned_paragraphs_by_control_id)
        })
        .count();

    let tables: usize = count_semantic_controls(section, Hwp5SemanticControlKind::Table);
    let images: usize = count_semantic_controls(section, Hwp5SemanticControlKind::Image);
    let charts: usize = count_semantic_controls(section, Hwp5SemanticControlKind::Chart);
    let ole_objects: usize = count_semantic_controls(section, Hwp5SemanticControlKind::OleObject);
    let text_boxes: usize = count_semantic_controls(section, Hwp5SemanticControlKind::TextBox);
    let mut line_count: usize = count_semantic_controls(section, Hwp5SemanticControlKind::Line);
    let mut rectangle_count: usize =
        count_semantic_controls(section, Hwp5SemanticControlKind::Rect);
    let mut polygon_count: usize =
        count_semantic_controls(section, Hwp5SemanticControlKind::Polygon);
    let has_header: bool = section
        .controls
        .iter()
        .any(|control| matches!(control.kind, Hwp5SemanticControlKind::Header));
    let has_footer: bool = section
        .controls
        .iter()
        .any(|control| matches!(control.kind, Hwp5SemanticControlKind::Footer));
    let has_page_number: bool = section
        .controls
        .iter()
        .any(|control| matches!(control.kind, Hwp5SemanticControlKind::PageNumber));
    let first_non_empty_text: Option<String> = body_paragraphs.iter().find_map(|paragraph| {
        first_visible_text_in_semantic_paragraph(paragraph, &owned_paragraphs_by_control_id)
    });

    for control in &section.controls {
        if let Hwp5SemanticControlKind::Unknown(tag) = &control.kind {
            if tag.contains("line") {
                line_count += 1;
            } else if tag.contains("rect") {
                rectangle_count += 1;
            } else if tag.contains("polygon") {
                polygon_count += 1;
            }
        }
    }

    DeepSectionSummary {
        index: section.index,
        paragraphs,
        non_empty_paragraphs,
        deep_paragraphs,
        deep_non_empty_paragraphs,
        tables,
        images,
        charts,
        ole_objects,
        text_boxes,
        lines: line_count,
        rectangles: rectangle_count,
        polygons: polygon_count,
        has_header,
        has_footer,
        has_page_number,
        landscape: section.page_def.as_ref().is_some_and(|page_def| page_def.landscape),
        first_non_empty_text,
    }
}

fn semantic_paragraph_has_visible_text_deep(
    paragraph: &Hwp5SemanticParagraph,
    owned_paragraphs_by_control_id: &BTreeMap<Hwp5SemanticControlId, Vec<&Hwp5SemanticParagraph>>,
) -> bool {
    first_visible_text_in_semantic_paragraph(paragraph, owned_paragraphs_by_control_id).is_some()
}

fn first_visible_text_in_semantic_paragraph(
    paragraph: &Hwp5SemanticParagraph,
    owned_paragraphs_by_control_id: &BTreeMap<Hwp5SemanticControlId, Vec<&Hwp5SemanticParagraph>>,
) -> Option<String> {
    let mut seen_controls: BTreeSet<Hwp5SemanticControlId> = BTreeSet::new();
    first_visible_text_in_semantic_paragraph_with_seen(
        paragraph,
        owned_paragraphs_by_control_id,
        &mut seen_controls,
    )
}

fn first_visible_text_in_semantic_paragraph_with_seen(
    paragraph: &Hwp5SemanticParagraph,
    owned_paragraphs_by_control_id: &BTreeMap<Hwp5SemanticControlId, Vec<&Hwp5SemanticParagraph>>,
    seen_controls: &mut BTreeSet<Hwp5SemanticControlId>,
) -> Option<String> {
    let normalized: String = normalized_semantic_text(&paragraph.inline_text_summary());
    if !normalized.is_empty() {
        return Some(normalized);
    }

    paragraph.inline_control_ids().into_iter().find_map(|control_id| {
        first_visible_text_in_semantic_control(
            control_id,
            owned_paragraphs_by_control_id,
            seen_controls,
        )
    })
}

fn first_visible_text_in_semantic_control(
    control_id: Hwp5SemanticControlId,
    owned_paragraphs_by_control_id: &BTreeMap<Hwp5SemanticControlId, Vec<&Hwp5SemanticParagraph>>,
    seen_controls: &mut BTreeSet<Hwp5SemanticControlId>,
) -> Option<String> {
    if !seen_controls.insert(control_id) {
        return None;
    }

    let result: Option<String> =
        owned_paragraphs_by_control_id.get(&control_id).and_then(|paragraphs| {
            paragraphs.iter().find_map(|paragraph| {
                first_visible_text_in_semantic_paragraph_with_seen(
                    paragraph,
                    owned_paragraphs_by_control_id,
                    seen_controls,
                )
            })
        });

    seen_controls.remove(&control_id);
    result
}

fn normalized_semantic_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\u{fffc}' | '\u{200b}' | '\u{feff}'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn build_semantic_owned_paragraph_map(
    section: &Hwp5SemanticSection,
) -> BTreeMap<Hwp5SemanticControlId, Vec<&Hwp5SemanticParagraph>> {
    let mut owned_paragraphs_by_control_id: BTreeMap<
        Hwp5SemanticControlId,
        Vec<&Hwp5SemanticParagraph>,
    > = BTreeMap::new();
    for paragraph in &section.paragraphs {
        if let Some(owner_control_id) = paragraph.owner_control_id {
            owned_paragraphs_by_control_id.entry(owner_control_id).or_default().push(paragraph);
        }
    }
    owned_paragraphs_by_control_id
}

fn count_occurrences(occurrences: &[&HwpxPathOccurrence], kind: &str) -> usize {
    occurrences.iter().filter(|occurrence| occurrence.kind == kind).count()
}

fn count_semantic_controls(section: &Hwp5SemanticSection, kind: Hwp5SemanticControlKind) -> usize {
    section.controls.iter().filter(|control| control.kind == kind).count()
}

fn count_paragraphs_recursive(paragraphs: &[Paragraph]) -> usize {
    paragraphs
        .iter()
        .map(|paragraph| 1 + paragraph.runs.iter().map(count_runs_paragraphs).sum::<usize>())
        .sum()
}

fn count_non_empty_paragraphs_recursive(paragraphs: &[Paragraph]) -> usize {
    paragraphs
        .iter()
        .map(|paragraph| {
            let current: usize = usize::from(paragraph_has_visible_text_deep(paragraph));
            current + paragraph.runs.iter().map(count_runs_non_empty_paragraphs).sum::<usize>()
        })
        .sum()
}

fn count_runs_paragraphs(run: &hwpforge_core::run::Run) -> usize {
    match &run.content {
        RunContent::Text(_) | RunContent::Image(_) => 0,
        RunContent::Table(table) => table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| count_paragraphs_recursive(&cell.paragraphs))
            .sum(),
        RunContent::Control(control) => count_control_paragraphs(control),
        _ => 0,
    }
}

fn count_runs_non_empty_paragraphs(run: &hwpforge_core::run::Run) -> usize {
    match &run.content {
        RunContent::Text(_) | RunContent::Image(_) => 0,
        RunContent::Table(table) => table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| count_non_empty_paragraphs_recursive(&cell.paragraphs))
            .sum(),
        RunContent::Control(control) => count_control_non_empty_paragraphs(control),
        _ => 0,
    }
}

fn count_control_paragraphs(control: &Control) -> usize {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => count_paragraphs_recursive(paragraphs),
        Control::Memo { content, .. } => count_paragraphs_recursive(content),
        _ => 0,
    }
}

fn count_control_non_empty_paragraphs(control: &Control) -> usize {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => count_non_empty_paragraphs_recursive(paragraphs),
        Control::Memo { content, .. } => count_non_empty_paragraphs_recursive(content),
        _ => 0,
    }
}

fn paragraph_has_visible_text_deep(paragraph: &Paragraph) -> bool {
    first_visible_text_in_paragraph_deep(paragraph).is_some()
}

fn first_visible_text_in_paragraphs_deep(paragraphs: &[Paragraph]) -> Option<String> {
    paragraphs.iter().find_map(first_visible_text_in_paragraph_deep)
}

fn first_visible_text_in_paragraph_deep(paragraph: &Paragraph) -> Option<String> {
    paragraph.runs.iter().find_map(first_visible_text_in_run_deep)
}

fn first_visible_text_in_run_deep(run: &hwpforge_core::run::Run) -> Option<String> {
    match &run.content {
        RunContent::Text(text) => {
            let trimmed: &str = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        RunContent::Image(_) => None,
        RunContent::Table(table) => table.rows.iter().find_map(|row| {
            row.cells
                .iter()
                .find_map(|cell| first_visible_text_in_paragraphs_deep(&cell.paragraphs))
        }),
        RunContent::Control(control) => first_visible_text_in_control_deep(control),
        _ => None,
    }
}

fn first_visible_text_in_control_deep(control: &Control) -> Option<String> {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => first_visible_text_in_paragraphs_deep(paragraphs),
        Control::Memo { content, .. } => first_visible_text_in_paragraphs_deep(content),
        Control::Hyperlink { text, .. } => {
            let trimmed: &str = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}
