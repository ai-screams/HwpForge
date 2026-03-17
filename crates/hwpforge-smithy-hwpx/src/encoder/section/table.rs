use super::*;

/// Builds `HxTable` from a Core `Table`.
///
/// # Errors
///
/// Returns [`HwpxError::InvalidStructure`] if nesting depth exceeds
/// [`MAX_NESTING_DEPTH`].
pub(super) fn build_table(
    table: &Table,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTable> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    // Build grid occupancy map to compute correct cellAddr for merged cells.
    // Tracks which (row, col) positions are occupied by col_span/row_span.
    let mut occupied = std::collections::HashSet::<(u32, u32)>::new();
    let mut cell_addrs: Vec<Vec<u32>> = Vec::new();
    let mut max_col: u32 = 0;

    for (row_idx, row) in table.rows.iter().enumerate() {
        let mut col_addr: u32 = 0;
        let mut addrs = Vec::new();
        for cell in &row.cells {
            while occupied.contains(&(row_idx as u32, col_addr)) {
                col_addr += 1;
            }
            addrs.push(col_addr);

            let col_span = (cell.col_span as u32).max(1);
            let row_span = (cell.row_span as u32).max(1);
            for dr in 0..row_span {
                for dc in 0..col_span {
                    occupied.insert((row_idx as u32 + dr, col_addr + dc));
                }
            }
            col_addr += col_span;
        }
        if col_addr > max_col {
            max_col = col_addr;
        }
        cell_addrs.push(addrs);
    }
    let col_cnt = max_col;

    let table_border_fill_id = table.border_fill_id.unwrap_or(TABLE_BORDER_FILL_ID);
    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            build_table_row(
                row,
                row_idx as u32,
                &cell_addrs[row_idx],
                table_border_fill_id,
                depth,
                hyperlink_entries,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    let table_width = table.width.map(|w| w.as_i32()).unwrap_or_else(|| {
        table
            .rows
            .first()
            .map_or(DEFAULT_HORZ_SIZE, |r| r.cells.iter().map(|c| c.width.as_i32()).sum())
    });

    Ok(HxTable {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "TABLE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),
        page_break: encode_table_page_break(table.page_break).to_string(),
        repeat_header: u32::from(table.repeat_header),
        row_cnt: table.rows.len() as u32,
        col_cnt,
        cell_spacing: table.cell_spacing.unwrap_or(HwpUnit::ZERO).as_i32().try_into().map_err(
            |_| HwpxError::InvalidStructure {
                detail: format!(
                    "table cell_spacing out of HWPX u32 range: {}",
                    table.cell_spacing.unwrap_or(HwpUnit::ZERO).as_i32()
                ),
            },
        )?,
        border_fill_id_ref: table_border_fill_id,
        no_adjust: 0,
        sz: Some(HxTableSz {
            width: table_width,
            width_rel_to: "ABSOLUTE".to_string(),
            height: 0,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(HxTablePos {
            treat_as_char: 0,
            affect_l_spacing: 0,
            flow_with_text: 1,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "COLUMN".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        }),
        out_margin: Some(DEFAULT_OUT_MARGIN),
        caption: table
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, table_width, depth, hyperlink_entries))
            .transpose()?,
        in_margin: Some(DEFAULT_CELL_MARGIN),
        rows,
    })
}

/// Builds `HxTableRow` from a Core `TableRow`.
///
/// `col_addrs` contains the precomputed grid column address for each cell,
/// accounting for col_span/row_span from this and previous rows.
fn build_table_row(
    row: &TableRow,
    row_idx: u32,
    col_addrs: &[u32],
    table_border_fill_id: u32,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTableRow> {
    let row_fallback_height =
        (!row.cells.iter().any(|cell| cell.height.is_some())).then_some(row.height).flatten();
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let col_addr = col_addrs.get(i).copied().unwrap_or(i as u32);
            build_table_cell(
                cell,
                TableCellBuildContext {
                    col_idx: col_addr,
                    row_idx,
                    row_is_header: row.is_header,
                    row_height: row_fallback_height,
                    table_border_fill_id,
                },
                depth,
                hyperlink_entries,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxTableRow { cells })
}

/// Builds `HxTableCell` from a Core `TableCell`.
///
/// Cell paragraphs are built recursively at `depth + 1` to track nesting.
/// `col_idx` and `row_idx` are used to populate `<hp:cellAddr>`.
#[derive(Clone, Copy)]
struct TableCellBuildContext {
    col_idx: u32,
    row_idx: u32,
    row_is_header: bool,
    row_height: Option<HwpUnit>,
    table_border_fill_id: u32,
}

fn build_table_cell(
    cell: &TableCell,
    ctx: TableCellBuildContext,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTableCell> {
    Ok(HxTableCell {
        name: String::new(),
        header: u32::from(ctx.row_is_header),
        has_margin: u32::from(cell.margin.is_some()),
        protect: 0,
        editable: 0,
        dirty: 0,
        border_fill_id_ref: cell.border_fill_id.unwrap_or(ctx.table_border_fill_id),
        sub_list: Some(build_sublist(
            &cell.paragraphs,
            depth,
            encode_table_vertical_align(cell.vertical_align.unwrap_or(TableVerticalAlign::Center)),
            hyperlink_entries,
        )?),
        cell_addr: Some(HxCellAddr { col_addr: ctx.col_idx, row_addr: ctx.row_idx }),
        cell_span: Some(HxCellSpan {
            col_span: cell.col_span as u32,
            row_span: cell.row_span as u32,
        }),
        cell_sz: Some(HxCellSz {
            width: cell.width.as_i32(),
            height: cell.height.or(ctx.row_height).unwrap_or(HwpUnit::ZERO).as_i32(),
        }),
        cell_margin: Some(cell.margin.map(encode_table_margin).unwrap_or(DEFAULT_CELL_MARGIN)),
    })
}
