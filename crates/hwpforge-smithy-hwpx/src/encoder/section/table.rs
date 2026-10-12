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
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxTable> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    // Guard the lenient placement scan before it allocates per-position
    // state: pathological spans (e.g. 65535×65535) cover billions of grid
    // positions. Same O(cells) pre-check the strict grid runs internally.
    let area = hwpforge_core::table::grid::covered_area(table);
    if area > hwpforge_core::table::grid::MAX_GRID_POSITIONS {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "table covered area {} exceeds the {}-position placement cap",
                area,
                hwpforge_core::table::grid::MAX_GRID_POSITIONS,
            ),
        });
    }

    // Grid placement computes correct cellAddr for merged cells. The lenient
    // scan is shared with `hwpforge_core::table::grid` and keeps historical
    // output even for tables that do not tile a well-formed grid.
    let placements = hwpforge_core::table::grid::grid_placements(table);
    let mut cell_addrs: Vec<Vec<u32>> =
        table.rows.iter().map(|row| Vec::with_capacity(row.cells.len())).collect();
    for placed in &placements.cells {
        cell_addrs[placed.row_idx].push(placed.at.col);
    }
    let col_cnt = placements.cols;

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
                table.in_margin,
                depth,
                hyperlink_entries,
                options,
                sink,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    let table_width = table.width.map(|w| w.as_i32()).unwrap_or_else(|| {
        table
            .rows
            .first()
            .map_or(DEFAULT_HORZ_SIZE, |r| r.cells.iter().map(|c| c.width.as_i32()).sum())
    });

    // §19.6 A: `None` = the placement this encoder has always written.
    // 이것을 실패시키는 것: 고정값 복귀(`table_placement_round_trips`).
    let placement = table.placement.clone().unwrap_or_else(table_default_placement);
    Ok(HxTable {
        // Wave 12p Step 4: HWPX `<hp:tbl id="...">` cross-ref target.
        // Table.inst_id 가 있으면 사용 (한컴 native 의 instance ID),
        // 없으면 sequential fallback.
        id: table.inst_id.map(|n| n.to_string()).unwrap_or_else(generate_instid),
        z_order: 0,
        numbering_type: "TABLE".to_string(),
        text_wrap: placement.text_wrap.as_hwpx_str().into_owned(),
        text_flow: placement.text_flow.as_hwpx_str().into_owned(),
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
        // E: fixed cell sizes. 이것을 실패시키는 것: 상수 0 으로 되돌리기
        // (`table_no_adjust_and_protect_round_trip`).
        no_adjust: u32::from(table.no_adjust),
        sz: Some(HxTableSz {
            width: table_width,
            width_rel_to: "ABSOLUTE".to_string(),
            height: 0,
            height_rel_to: "ABSOLUTE".to_string(),
            // S20: the table's size lock (`Table::frame`).
            // 이것을 실패시키는 것: 상수 0 으로 되돌리기(`picture_and_table_protect_round_trip`).
            protect: u32::from(table.frame.size_protect),
        }),
        pos: Some(HxTablePos {
            treat_as_char: u32::from(placement.treat_as_char),
            affect_l_spacing: u32::from(placement.affect_line_spacing),
            flow_with_text: u32::from(placement.flow_with_text),
            allow_overlap: u32::from(placement.allow_overlap),
            hold_anchor_and_so: u32::from(placement.hold_anchor_and_so),
            vert_rel_to: placement.vert_rel_to.as_hwpx_str().into_owned(),
            horz_rel_to: placement.horz_rel_to.as_hwpx_str().into_owned(),
            vert_align: placement.vert_align.as_hwpx_str().to_string(),
            horz_align: placement.horz_align.as_hwpx_str().to_string(),
            vert_offset: placement.vert_offset.as_i32(),
            horz_offset: placement.horz_offset.as_i32(),
        }),
        // Core 승격값 우선 (W3a-2 의도된 delta) — None(우리 API 저작)은 기존
        // 기본값 유지로 바이트 불변.
        out_margin: Some(table.out_margin.map(encode_table_margin).unwrap_or(DEFAULT_OUT_MARGIN)),
        caption: table
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, table_width, depth, hyperlink_entries, options, sink))
            .transpose()?,
        in_margin: Some(table.in_margin.map(encode_table_margin).unwrap_or(DEFAULT_CELL_MARGIN)),
        // F: the cell zones, only when there are any.
        // 이것을 실패시키는 것: 미방출 · 행·열 뒤바꿈(`cell_zones_round_trip`).
        cell_zone_list: (!table.cell_zones.is_empty()).then(|| HxCellZoneList {
            zones: table
                .cell_zones
                .iter()
                .map(|z| HxCellZone {
                    start_row_addr: u32::from(z.start_row),
                    start_col_addr: u32::from(z.start_col),
                    end_row_addr: u32::from(z.end_row),
                    end_col_addr: u32::from(z.end_col),
                    border_fill_id_ref: z.border_fill_id,
                })
                .collect(),
        }),
        rows,
    })
}

/// The placement written for a table whose [`Table::placement`] is `None`:
/// not treated as a character, flowing with the text, no overlap, anchored
/// to the paragraph vertically and the column horizontally, zero offsets,
/// top/left, top-and-bottom wrap, both-sides flow. The decoder folds a wire
/// placement equal to this back to `None`.
pub(crate) fn table_default_placement() -> ObjectPlacement {
    use hwpforge_core::placement::{ObjectRelativeTo, ObjectTextWrap};
    ObjectPlacement::new(
        ObjectTextWrap::TopAndBottom,
        ObjectRelativeTo::Para,
        ObjectRelativeTo::Column,
    )
    .with_flow_with_text(true)
}

/// Builds `HxTableRow` from a Core `TableRow`.
///
/// `col_addrs` contains the precomputed grid column address for each cell,
/// accounting for col_span/row_span from this and previous rows.
#[allow(clippy::too_many_arguments)]
fn build_table_row(
    row: &TableRow,
    row_idx: u32,
    col_addrs: &[u32],
    table_border_fill_id: u32,
    table_in_margin: Option<TableMargin>,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxTableRow> {
    let row_fallback_height =
        (!row.cells.iter().any(|cell| cell.height.is_some())).then_some(row.height).flatten();
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let col_addr = col_addrs.get(i).copied().unwrap_or(i as u32);
            sink.enter(crate::decoder::PathSeg::TableCell { row: row_idx as usize, cell: i });
            let result = build_table_cell(
                cell,
                TableCellBuildContext {
                    col_idx: col_addr,
                    row_idx,
                    row_is_header: row.is_header,
                    row_height: row_fallback_height,
                    table_border_fill_id,
                    table_in_margin,
                },
                depth,
                hyperlink_entries,
                options,
                sink,
            );
            sink.leave();
            result
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
    table_in_margin: Option<TableMargin>,
}

fn build_table_cell(
    cell: &TableCell,
    ctx: TableCellBuildContext,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxTableCell> {
    Ok(HxTableCell {
        name: String::new(),
        header: u32::from(ctx.row_is_header),
        has_margin: u32::from(cell.margin.is_some()),
        protect: 0,
        editable: 0,
        dirty: 0,
        border_fill_id_ref: cell.border_fill_id.unwrap_or(ctx.table_border_fill_id),
        sub_list: Some({
            let mut sub_list = build_sublist(
                &cell.paragraphs,
                depth,
                encode_table_vertical_align(
                    cell.vertical_align.unwrap_or(TableVerticalAlign::Center),
                ),
                hyperlink_entries,
                options,
                sink,
            )?;
            // §19.22 D3: the cell's line wrap, not a fixed `BREAK`.
            // 이것을 실패시키는 것: 고정 BREAK 복귀(`cell_line_wrap_round_trips`).
            sub_list.line_wrap = match cell.line_wrap {
                hwpforge_core::table::CellLineWrap::Squeeze => "SQUEEZE",
                hwpforge_core::table::CellLineWrap::Keep => "KEEP",
                _ => "BREAK",
            }
            .to_string();
            sub_list
        }),
        cell_addr: Some(HxCellAddr { col_addr: ctx.col_idx, row_addr: ctx.row_idx }),
        cell_span: Some(HxCellSpan {
            col_span: cell.col_span as u32,
            row_span: cell.row_span as u32,
        }),
        cell_sz: Some(HxCellSz {
            width: cell.width.as_i32(),
            height: cell.height.or(ctx.row_height).unwrap_or(HwpUnit::ZERO).as_i32(),
        }),
        // 셀 오버라이드 → (hasMargin=0 이면) 칸이 지닌 휴면 값(S19 — 한컴은
        // HWP5 에서 온 칸의 자기 값을 그대로 둔다) → 표 inMargin → 기본값 순.
        // 이것을 실패시키는 것: 휴면 값을 쓰지 않기
        // (`dormant_cell_margins_round_trip`).
        cell_margin: Some(
            cell.margin
                .or(cell.dormant_margin)
                .or(ctx.table_in_margin)
                .map(encode_table_margin)
                .unwrap_or(DEFAULT_CELL_MARGIN),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::table::TableLayoutCache;
    use hwpforge_foundation::ParaShapeIndex;

    fn margin(l: i32, r: i32, t: i32, b: i32) -> TableMargin {
        TableMargin {
            left: HwpUnit::new(l).unwrap(),
            right: HwpUnit::new(r).unwrap(),
            top: HwpUnit::new(t).unwrap(),
            bottom: HwpUnit::new(b).unwrap(),
        }
    }

    fn one_cell_table() -> Table {
        Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            HwpUnit::new(1000).unwrap(),
        )])])
    }

    #[test]
    fn core_margins_round_trip_to_wire() {
        // W3a-2 의도된 delta: 승격된 out/inMargin 은 고정 기본값 대신 원본값으로.
        let table = one_cell_table()
            .with_out_margin(margin(283, 284, 240, 241))
            .with_in_margin(margin(510, 511, 141, 142));
        let hx = build_table(
            &table,
            0,
            &mut Vec::new(),
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        let out = hx.out_margin.expect("outMargin");
        assert_eq!((out.left, out.right, out.top, out.bottom), (283, 284, 240, 241));
        let inm = hx.in_margin.expect("inMargin");
        assert_eq!((inm.left, inm.right, inm.top, inm.bottom), (510, 511, 141, 142));
        // 셀 오버라이드 없음(H5): hasMargin=0 + element 는 표 inMargin 실효값.
        let cell = &hx.rows[0].cells[0];
        assert_eq!(cell.has_margin, 0);
        let cm = cell.cell_margin.as_ref().expect("cellMargin element");
        assert_eq!((cm.left, cm.right, cm.top, cm.bottom), (510, 511, 141, 142));
    }

    #[test]
    fn default_emission_is_unchanged_without_core_margins() {
        // None(우리 API 저작) = 기존 고정값 그대로 — 바이트 불변 계약.
        let hx = build_table(
            &one_cell_table(),
            0,
            &mut Vec::new(),
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(hx.out_margin.expect("outMargin"), DEFAULT_OUT_MARGIN);
        assert_eq!(hx.in_margin.expect("inMargin"), DEFAULT_CELL_MARGIN);
        assert_eq!(
            hx.rows[0].cells[0].cell_margin.as_ref().expect("cellMargin"),
            &DEFAULT_CELL_MARGIN
        );
    }

    #[test]
    fn decode_only_layout_cache_is_never_emitted() {
        // sz height 는 decode-only 캐시 — 인코더는 항상 자체 정책(0)을 쓴다.
        let table = one_cell_table()
            .with_layout_cache(TableLayoutCache::new(Some(HwpUnit::new(2831).unwrap()), true));
        let hx = build_table(
            &table,
            0,
            &mut Vec::new(),
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(hx.sz.expect("sz").height, 0, "decode-only cache must not reach the wire");
    }

    #[test]
    fn explicit_cell_margin_still_wins_over_in_margin() {
        let mut table = one_cell_table().with_in_margin(margin(510, 510, 141, 141));
        table.rows[0].cells[0] = table.rows[0].cells[0].clone().with_margin(margin(10, 20, 30, 40));
        let hx = build_table(
            &table,
            0,
            &mut Vec::new(),
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        let cell = &hx.rows[0].cells[0];
        assert_eq!(cell.has_margin, 1);
        let cm = cell.cell_margin.as_ref().expect("cellMargin");
        assert_eq!((cm.left, cm.right, cm.top, cm.bottom), (10, 20, 30, 40));
    }
}
