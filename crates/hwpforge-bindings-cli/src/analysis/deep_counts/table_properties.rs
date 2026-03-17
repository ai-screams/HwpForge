use std::collections::{BTreeMap, BTreeSet};

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableMargin, TablePageBreak, TableVerticalAlign};
use hwpforge_smithy_hwp5::{
    Hwp5SemanticControlPayload, Hwp5SemanticDocument, Hwp5SemanticTableCellVerticalAlign,
    Hwp5SemanticTablePageBreak,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct DeepTableMarginHwp {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeepTableVerticalAlign {
    Top,
    Center,
    Bottom,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct DeepTableCellEvidence {
    pub section_index: usize,
    pub table_ordinal: usize,
    pub column: u16,
    pub row: u16,
    pub col_span: u16,
    pub row_span: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_header: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_hwp: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_hwp: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_hwp: Option<DeepTableMarginHwp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<DeepTableVerticalAlign>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct DeepTableEvidence {
    pub section_index: usize,
    pub table_ordinal: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_width_hwp: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_max_cell_heights_hwp: Vec<i32>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DeepTablePropertiesSummary {
    pub page_break_none: usize,
    pub page_break_table: usize,
    pub page_break_cell: usize,
    pub page_break_unknown: BTreeMap<u8, usize>,
    pub repeat_header_tables: usize,
    pub header_rows: usize,
    pub nonzero_cell_spacing_tables: usize,
    pub table_border_fill_ids: BTreeSet<u32>,
    pub cell_border_fill_ids: BTreeSet<u32>,
    pub cell_heights_hwp: BTreeSet<i32>,
    pub cell_widths_hwp: BTreeSet<i32>,
    pub table_widths_hwp: BTreeSet<i32>,
    pub row_max_cell_heights_hwp: BTreeSet<i32>,
    pub table_evidence: BTreeSet<DeepTableEvidence>,
    pub cell_evidence: BTreeSet<DeepTableCellEvidence>,
}

impl DeepTablePropertiesSummary {
    pub(crate) fn increment_page_break(&mut self, page_break: TablePageBreak) {
        match page_break {
            TablePageBreak::None => self.page_break_none += 1,
            TablePageBreak::Table => self.page_break_table += 1,
            TablePageBreak::Cell => self.page_break_cell += 1,
        }
    }

    fn observe_table(
        &mut self,
        section_index: usize,
        table_ordinal: usize,
        table: &Table,
        next_table_ordinal: &mut usize,
    ) {
        self.observe_table_summary_counts(table);
        let structural_width_hwp: Option<i32> = structural_table_width_hwp(table);
        let row_max_cell_heights_hwp: Vec<i32> = structural_row_max_cell_heights_hwp(table);
        self.observe_table_structural_evidence(
            section_index,
            table_ordinal,
            structural_width_hwp,
            row_max_cell_heights_hwp,
        );

        let cell_addrs = compute_table_cell_addresses(table);
        for (row_idx, row) in table.rows.iter().enumerate() {
            for (cell_idx, cell) in row.cells.iter().enumerate() {
                self.observe_table_cell(
                    section_index,
                    table_ordinal,
                    row_idx as u16,
                    cell_addrs[row_idx][cell_idx],
                    row.is_header,
                    cell,
                );
                accumulate_table_properties_from_paragraphs(
                    &cell.paragraphs,
                    section_index,
                    self,
                    next_table_ordinal,
                );
            }
        }
    }

    fn observe_table_summary_counts(&mut self, table: &Table) {
        self.increment_page_break(table.page_break);
        if table.repeat_header {
            self.repeat_header_tables += 1;
        }
        self.header_rows += table.rows.iter().filter(|row| row.is_header).count();
        if table.cell_spacing.is_some_and(|cell_spacing| cell_spacing.as_i32() != 0) {
            self.nonzero_cell_spacing_tables += 1;
        }
        if let Some(border_fill_id) = table.border_fill_id {
            self.table_border_fill_ids.insert(border_fill_id);
        }
    }

    fn observe_table_structural_evidence(
        &mut self,
        section_index: usize,
        table_ordinal: usize,
        structural_width_hwp: Option<i32>,
        row_max_cell_heights_hwp: Vec<i32>,
    ) {
        if let Some(width_hwp) = structural_width_hwp {
            self.table_widths_hwp.insert(width_hwp);
        }
        self.row_max_cell_heights_hwp.extend(row_max_cell_heights_hwp.iter().copied());
        self.table_evidence.insert(DeepTableEvidence {
            section_index,
            table_ordinal,
            structural_width_hwp,
            row_max_cell_heights_hwp,
        });
    }

    fn observe_table_cell(
        &mut self,
        section_index: usize,
        table_ordinal: usize,
        row_index: u16,
        column: u16,
        row_is_header: bool,
        cell: &TableCell,
    ) {
        if let Some(border_fill_id) = cell.border_fill_id {
            self.cell_border_fill_ids.insert(border_fill_id);
        }
        let raw_height: Option<i32> =
            cell.height.map(|height| height.as_i32()).filter(|height| *height > 0);
        if let Some(raw_height) = raw_height {
            self.cell_heights_hwp.insert(raw_height);
        }
        let raw_width: i32 = cell.width.as_i32();
        if raw_width > 0 {
            self.cell_widths_hwp.insert(raw_width);
        }
        self.cell_evidence.insert(DeepTableCellEvidence {
            section_index,
            table_ordinal,
            column,
            row: row_index,
            col_span: cell.col_span,
            row_span: cell.row_span,
            is_header: row_is_header,
            border_fill_id: cell.border_fill_id,
            height_hwp: raw_height,
            width_hwp: (raw_width > 0).then_some(raw_width),
            margin_hwp: cell.margin.map(deep_table_margin_from_core),
            vertical_align: cell.vertical_align.map(deep_table_vertical_align_from_core),
        });
    }
}

pub(crate) fn summarize_hwpx_table_properties(sections: &[Section]) -> DeepTablePropertiesSummary {
    let mut summary: DeepTablePropertiesSummary = DeepTablePropertiesSummary::default();
    let mut next_table_ordinal: usize = 0;

    for (section_index, section) in sections.iter().enumerate() {
        accumulate_table_properties_from_paragraphs(
            &section.paragraphs,
            section_index,
            &mut summary,
            &mut next_table_ordinal,
        );
        if let Some(header) = &section.header {
            accumulate_table_properties_from_paragraphs(
                &header.paragraphs,
                section_index,
                &mut summary,
                &mut next_table_ordinal,
            );
        }
        if let Some(footer) = &section.footer {
            accumulate_table_properties_from_paragraphs(
                &footer.paragraphs,
                section_index,
                &mut summary,
                &mut next_table_ordinal,
            );
        }
    }

    summary
}

pub(crate) fn summarize_hwp5_table_properties(
    document: &Hwp5SemanticDocument,
) -> DeepTablePropertiesSummary {
    let mut summary: DeepTablePropertiesSummary = DeepTablePropertiesSummary::default();
    let mut next_table_ordinal: usize = 0;

    for (section_index, section) in document.sections.iter().enumerate() {
        for table_payload in section.controls.iter().filter_map(|control| match &control.payload {
            Hwp5SemanticControlPayload::Table(payload) => Some(payload),
            _ => None,
        }) {
            match table_payload.page_break {
                Hwp5SemanticTablePageBreak::None => {
                    summary.increment_page_break(TablePageBreak::None)
                }
                Hwp5SemanticTablePageBreak::Table => {
                    summary.increment_page_break(TablePageBreak::Table)
                }
                Hwp5SemanticTablePageBreak::Cell => {
                    summary.increment_page_break(TablePageBreak::Cell)
                }
                Hwp5SemanticTablePageBreak::Unknown(raw) => {
                    *summary.page_break_unknown.entry(raw).or_default() += 1;
                }
            }
            if table_payload.repeat_header {
                summary.repeat_header_tables += 1;
            }
            summary.header_rows += usize::from(table_payload.header_row_count);
            if table_payload.cell_spacing_hwp != 0 {
                summary.nonzero_cell_spacing_tables += 1;
            }
            if let Some(border_fill_id) = table_payload.border_fill_id {
                summary.table_border_fill_ids.insert(u32::from(border_fill_id));
            }
            summary
                .cell_border_fill_ids
                .extend(table_payload.distinct_cell_border_fill_ids.iter().copied().map(u32::from));
            summary.cell_heights_hwp.extend(
                table_payload
                    .distinct_cell_heights_hwp
                    .iter()
                    .copied()
                    .filter(|height| *height > 0),
            );
            summary.cell_widths_hwp.extend(
                table_payload.distinct_cell_widths_hwp.iter().copied().filter(|width| *width > 0),
            );
            if let Some(width_hwp) = table_payload.structural_width_hwp.filter(|width| *width > 0) {
                summary.table_widths_hwp.insert(width_hwp);
            }
            summary.row_max_cell_heights_hwp.extend(
                table_payload.row_max_cell_heights_hwp.iter().copied().filter(|height| *height > 0),
            );
            summary.table_evidence.insert(DeepTableEvidence {
                section_index,
                table_ordinal: next_table_ordinal,
                structural_width_hwp: table_payload.structural_width_hwp.filter(|width| *width > 0),
                row_max_cell_heights_hwp: table_payload
                    .row_max_cell_heights_hwp
                    .iter()
                    .copied()
                    .filter(|height| *height > 0)
                    .collect(),
            });
            for cell in &table_payload.cells {
                summary.cell_evidence.insert(DeepTableCellEvidence {
                    section_index,
                    table_ordinal: next_table_ordinal,
                    column: cell.column,
                    row: cell.row,
                    col_span: cell.col_span,
                    row_span: cell.row_span,
                    is_header: cell.is_header,
                    border_fill_id: cell.border_fill_id.map(u32::from),
                    height_hwp: cell.height_hwp.filter(|height| *height > 0),
                    width_hwp: cell.width_hwp.filter(|width| *width > 0),
                    margin_hwp: Some(DeepTableMarginHwp {
                        left: i32::from(cell.margin_hwp.left_hwp),
                        right: i32::from(cell.margin_hwp.right_hwp),
                        top: i32::from(cell.margin_hwp.top_hwp),
                        bottom: i32::from(cell.margin_hwp.bottom_hwp),
                    }),
                    vertical_align: Some(match cell.vertical_align {
                        Hwp5SemanticTableCellVerticalAlign::Top => DeepTableVerticalAlign::Top,
                        Hwp5SemanticTableCellVerticalAlign::Center => {
                            DeepTableVerticalAlign::Center
                        }
                        Hwp5SemanticTableCellVerticalAlign::Bottom => {
                            DeepTableVerticalAlign::Bottom
                        }
                        Hwp5SemanticTableCellVerticalAlign::Unknown(raw) => {
                            DeepTableVerticalAlign::Unknown(raw)
                        }
                    }),
                });
            }
            next_table_ordinal += 1;
        }
    }

    summary
}

pub(crate) fn append_table_property_notes(
    notes: &mut Vec<String>,
    summary: &DeepTablePropertiesSummary,
) {
    for (page_break, count) in table_page_break_notes(summary) {
        if count > 0 {
            notes.push(format!("table-page-break-{page_break}: {count}"));
        }
    }
    for (raw, count) in &summary.page_break_unknown {
        notes.push(format!("table-page-break-unknown-{raw}: {count}"));
    }
    if summary.repeat_header_tables > 0 {
        notes.push(format!("table-repeat-header-on: {}", summary.repeat_header_tables));
    }
    if summary.header_rows > 0 {
        notes.push(format!("table-header-rows: {}", summary.header_rows));
    }
    if summary.nonzero_cell_spacing_tables > 0 {
        notes.push(format!("table-nonzero-cell-spacing: {}", summary.nonzero_cell_spacing_tables));
    }
    if !summary.table_border_fill_ids.is_empty() {
        let ids =
            summary.table_border_fill_ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("table-border-fill-ids: {ids}"));
    }
    if !summary.cell_border_fill_ids.is_empty() {
        let ids =
            summary.cell_border_fill_ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("table-cell-border-fill-ids: {ids}"));
    }
    if !summary.cell_heights_hwp.is_empty() {
        let heights =
            summary.cell_heights_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("table-cell-heights-hwp: {heights}"));
    }
    if !summary.cell_widths_hwp.is_empty() {
        let widths =
            summary.cell_widths_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("table-cell-widths-hwp: {widths}"));
    }
    if !summary.table_widths_hwp.is_empty() {
        let widths =
            summary.table_widths_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("table-structural-widths-hwp: {widths}"));
    }
    if !summary.row_max_cell_heights_hwp.is_empty() {
        let heights = summary
            .row_max_cell_heights_hwp
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        notes.push(format!("table-row-max-cell-heights-hwp: {heights}"));
    }
    if !summary.table_evidence.is_empty() {
        notes.push(format!("table-structural-evidence: {}", summary.table_evidence.len()));
    }
    if !summary.cell_evidence.is_empty() {
        notes.push(format!("table-cell-evidence: {}", summary.cell_evidence.len()));
    }
}

fn accumulate_table_properties_from_paragraphs(
    paragraphs: &[Paragraph],
    section_index: usize,
    summary: &mut DeepTablePropertiesSummary,
    next_table_ordinal: &mut usize,
) {
    for paragraph in paragraphs {
        for run in &paragraph.runs {
            accumulate_table_properties_from_run(run, section_index, summary, next_table_ordinal);
        }
    }
}

fn accumulate_table_properties_from_run(
    run: &hwpforge_core::run::Run,
    section_index: usize,
    summary: &mut DeepTablePropertiesSummary,
    next_table_ordinal: &mut usize,
) {
    match &run.content {
        RunContent::Table(table) => {
            let table_ordinal = *next_table_ordinal;
            *next_table_ordinal += 1;
            summary.observe_table(section_index, table_ordinal, table, next_table_ordinal);
        }
        RunContent::Control(control) => accumulate_table_properties_from_control(
            control,
            section_index,
            summary,
            next_table_ordinal,
        ),
        _ => {}
    }
}

fn accumulate_table_properties_from_control(
    control: &Control,
    section_index: usize,
    summary: &mut DeepTablePropertiesSummary,
    next_table_ordinal: &mut usize,
) {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => {
            accumulate_table_properties_from_paragraphs(
                paragraphs,
                section_index,
                summary,
                next_table_ordinal,
            );
        }
        Control::Memo { content, .. } => {
            accumulate_table_properties_from_paragraphs(
                content,
                section_index,
                summary,
                next_table_ordinal,
            );
        }
        _ => {}
    }
}

fn table_page_break_notes(summary: &DeepTablePropertiesSummary) -> [(&'static str, usize); 3] {
    [
        ("none", summary.page_break_none),
        ("table", summary.page_break_table),
        ("cell", summary.page_break_cell),
    ]
}

fn deep_table_margin_from_core(margin: TableMargin) -> DeepTableMarginHwp {
    DeepTableMarginHwp {
        left: margin.left.as_i32(),
        right: margin.right.as_i32(),
        top: margin.top.as_i32(),
        bottom: margin.bottom.as_i32(),
    }
}

fn deep_table_vertical_align_from_core(value: TableVerticalAlign) -> DeepTableVerticalAlign {
    match value {
        TableVerticalAlign::Top => DeepTableVerticalAlign::Top,
        TableVerticalAlign::Center => DeepTableVerticalAlign::Center,
        TableVerticalAlign::Bottom => DeepTableVerticalAlign::Bottom,
    }
}

fn compute_table_cell_addresses(table: &Table) -> Vec<Vec<u16>> {
    let mut occupied: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut row_addrs: Vec<Vec<u16>> = Vec::with_capacity(table.rows.len());

    for (row_idx, row) in table.rows.iter().enumerate() {
        let row_u16 = row_idx as u16;
        let mut col_addr: u16 = 0;
        let mut cell_addrs: Vec<u16> = Vec::with_capacity(row.cells.len());

        for cell in &row.cells {
            while occupied.contains(&(row_u16, col_addr)) {
                col_addr = col_addr.saturating_add(1);
            }
            cell_addrs.push(col_addr);
            let col_span = cell.col_span.max(1);
            let row_span = cell.row_span.max(1);
            for dr in 0..row_span {
                for dc in 0..col_span {
                    occupied.insert((row_u16.saturating_add(dr), col_addr.saturating_add(dc)));
                }
            }
            col_addr = col_addr.saturating_add(col_span);
        }

        row_addrs.push(cell_addrs);
    }

    row_addrs
}

fn structural_table_width_hwp(table: &Table) -> Option<i32> {
    let first_row = table.rows.first()?;
    let width_hwp: i32 = first_row.cells.iter().map(|cell| cell.width.as_i32()).sum();
    (width_hwp > 0).then_some(width_hwp)
}

fn structural_row_max_cell_heights_hwp(table: &Table) -> Vec<i32> {
    table
        .rows
        .iter()
        .filter_map(|row| {
            row.cells
                .iter()
                .filter_map(|cell| cell.height.map(|height| height.as_i32()))
                .filter(|height| *height > 0)
                .max()
        })
        .collect()
}
