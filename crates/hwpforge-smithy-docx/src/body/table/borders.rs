use super::*;
use hwpforge_blueprint::registry::StyleRegistry;

/// One source-boundary resolution feeds both row minima and admitted face widths.
pub(super) struct SourceSpace {
    pub(super) heights: Vec<i64>,
    faces: Vec<[i32; 4]>,
    cols: usize,
}
fn resolve(a: (i32, bool), b: (i32, bool)) -> i32 {
    match (a, b) {
        ((0, _), (b, _)) => b,
        ((a, _), (0, _)) => a,
        ((a, true), (_, false)) => a,
        ((_, false), (b, true)) => b,
        ((a, _), (b, _)) => a.max(b),
    }
}
pub(super) fn source_space(rows: &[Row], width: usize, base: &TableProperties) -> SourceSpace {
    let mut faces = vec![[0; 4]; rows.len() * width];
    let mut heights = Vec::with_capacity(rows.len());
    let mut previous = vec![(0, false); width];
    for (ri, row) in rows.iter().enumerate() {
        let mut sides = vec![[(0, false); 4]; width];
        for cell in &row.cells {
            let end = cell.col + usize::from(cell.span);
            for (side, base_side) in
                [(2, if ri == 0 { 2 } else { 4 }), (3, if ri + 1 == rows.len() { 3 } else { 4 })]
            {
                let value = cell.properties.border_widths[side];
                for position in &mut sides[cell.col..end] {
                    position[side] = (
                        value.unwrap_or(base.border_widths[base_side].unwrap_or(0)),
                        value.is_some(),
                    );
                }
            }
            for (side, col, base_side) in [
                (0, cell.col, if cell.col == 0 { 0 } else { 5 }),
                (1, end - 1, if end == width { 1 } else { 5 }),
            ] {
                let value = cell.properties.border_widths[side];
                sides[col][side] =
                    (value.unwrap_or(base.border_widths[base_side].unwrap_or(0)), value.is_some());
            }
            if cell.continuation {
                for col in cell.col..end {
                    sides[col][2] = (0, false);
                    previous[col] = (0, false);
                }
            }
        }
        for col in 0..width {
            let top = resolve(sides[col][2], previous[col]);
            faces[ri * width + col][2] = top;
            faces[ri * width + col][3] = sides[col][3].0;
            if ri > 0 {
                faces[(ri - 1) * width + col][3] = top;
            }
            faces[ri * width + col][0] = sides[col][0].0;
            faces[ri * width + col][1] = sides[col][1].0;
            if col > 0 {
                let value = resolve(sides[col - 1][1], sides[col][0]);
                faces[ri * width + col - 1][1] = value;
                faces[ri * width + col][0] = value;
            }
        }
        let top = faces[ri * width..(ri + 1) * width].iter().map(|s| s[2]).max().unwrap_or(0);
        let bottom =
            if ri + 1 == rows.len() { sides.iter().map(|s| s[3].0).max().unwrap_or(0) } else { 0 };
        heights.push(i64::from(top) + i64::from(bottom));
        previous = sides.iter().map(|s| s[3]).collect();
    }
    SourceSpace { heights, faces, cols: width }
}
impl SourceSpace {
    fn anchor_widths(&self, c: &hwpforge_core::table::grid::GridCell) -> Option<[i32; 4]> {
        let r = c.anchor.row as usize;
        let col = c.anchor.col as usize;
        let re = r + usize::from(c.row_span);
        let ce = col + usize::from(c.col_span);
        let mut values = [0; 4];
        for (side, slots) in [
            (0, (r..re).map(|r| r * self.cols + col).collect::<Vec<_>>()),
            (1, (r..re).map(|r| r * self.cols + ce - 1).collect()),
            (2, (col..ce).map(|col| r * self.cols + col).collect()),
            (3, (col..ce).map(|col| (re - 1) * self.cols + col).collect()),
        ] {
            values[side] = self.faces[*slots.first()?][side];
            if slots.iter().any(|i| self.faces[*i][side] != values[side]) {
                return None;
            }
        }
        Some(values)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    table: &mut Table,
    grid: &TableGrid,
    overrides: &[[Option<bool>; 6]],
    base: [Option<bool>; 6],
    registry: &mut StyleRegistry,
    location: (&str, &Location),
    w: &mut Vec<DocxWarning>,
    space: &SourceSpace,
    base_widths: &[Option<i32>; 6],
    admission: Option<&'static str>,
) -> Result<()> {
    let (part, at) = location;
    let (rows, cols) = grid.dimensions();
    let cols = cols as usize;
    let rows = rows as usize;
    let anchors: Vec<_> = grid.iter_anchors().copied().collect();
    let mut owner = vec![0; rows * cols];
    let mut faces = Vec::with_capacity(anchors.len() * 4);
    for (i, c) in anchors.iter().enumerate() {
        let r = c.anchor.row as usize;
        let col = c.anchor.col as usize;
        let re = r + usize::from(c.row_span);
        let ce = col + usize::from(c.col_span);
        for row in r..re {
            owner[row * cols + col..row * cols + ce].fill(i);
        }
        let inherited = [
            base[if col == 0 { 0 } else { 5 }],
            base[if ce == cols { 1 } else { 5 }],
            base[if r == 0 { 2 } else { 4 }],
            base[if re == rows { 3 } else { 4 }],
        ];
        for (side, value) in inherited.into_iter().enumerate() {
            faces.push(overrides[i][side].or(value).unwrap_or(false));
        }
    }
    // Connected touching faces share one decision. This also propagates an absent
    // line across the whole side of a merged cell without repeated grid rescans.
    let mut parents: Vec<_> = (0..faces.len()).collect();
    let mut sizes = vec![1; faces.len()];
    let mut conflict = false;
    for r in 0..rows {
        for c in 0..cols {
            let a = owner[r * cols + c];
            if c + 1 < cols {
                let b = owner[r * cols + c + 1];
                if a != b {
                    join(a * 4 + 1, b * 4, &mut parents, &mut sizes, &mut faces, &mut conflict);
                }
            }
            if r + 1 < rows {
                let b = owner[(r + 1) * cols + c];
                if a != b {
                    join(a * 4 + 3, b * 4 + 2, &mut parents, &mut sizes, &mut faces, &mut conflict);
                }
            }
        }
    }
    if conflict {
        record(w, Kind::TableBorderApproximated, "border_conflict", "use_none", || {
            (Some(part), Some(at), "Conflicting touching faces use no border on both sides")
        })?;
    }
    let visual: Vec<[bool; 4]> = (0..anchors.len())
        .map(|i| std::array::from_fn(|side| faces[root(i * 4 + side, &mut parents)]))
        .collect();
    let widths: Option<Vec<[i32; 4]>> = anchors.iter().map(|c| space.anchor_widths(c)).collect();
    let representable = widths.as_ref().is_some_and(|widths| {
        visual.iter().zip(widths).all(|(v, w)| v.iter().zip(w).all(|(v, w)| *v == (*w > 0)))
    });
    let reason = admission.or((!representable).then_some("unrepresentable_source_face"));
    if let Some(reason) = reason {
        record(w, Kind::TableBorderApproximated, "table_border_space", "retain_margins", || {
            (Some(part), Some(at), format!("Table border space source admission failed ({reason}); existing cell margins are retained"))
        })?;
    } else if let Some(widths) = &widths {
        let mut actual = vec![0_i64; rows];
        for (c, w) in anchors.iter().zip(widths) {
            let row = c.anchor.row as usize;
            actual[row] = actual[row].max(i64::from(w[2]));
        }
        let bottom = anchors
            .iter()
            .zip(widths)
            .filter(|(c, _)| c.anchor.row as usize + usize::from(c.row_span) == rows)
            .map(|(_, w)| i64::from(w[3]))
            .max()
            .unwrap_or(0);
        actual[rows - 1] += bottom;
        if actual != space.heights {
            return Err(DocxError::invalid(
                part,
                at,
                "Admitted border faces disagree with source row space",
            ));
        }
    }
    for (i, c) in anchors.iter().enumerate() {
        table.rows[c.row_idx].cells[c.cell_idx].border_fill_id = Some(register(
            visual[i],
            if reason.is_none() { widths.as_ref().map(|w| w[i]) } else { None },
            registry,
            part,
            at,
        )?);
    }
    table.border_fill_id = Some(register(
        [
            base[0].unwrap_or(false),
            base[1].unwrap_or(false),
            base[2].unwrap_or(false),
            base[3].unwrap_or(false),
        ],
        reason.is_none().then(|| std::array::from_fn(|i| base_widths[i].unwrap_or(0))),
        registry,
        part,
        at,
    )?);
    Ok(())
}
fn root(mut i: usize, parents: &mut [usize]) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}
fn join(
    a: usize,
    b: usize,
    parents: &mut [usize],
    sizes: &mut [usize],
    faces: &mut [bool],
    conflict: &mut bool,
) {
    let mut a = root(a, parents);
    let mut b = root(b, parents);
    if a == b {
        return;
    }
    *conflict |= faces[a] != faces[b];
    if sizes[a] < sizes[b] {
        std::mem::swap(&mut a, &mut b);
    }
    parents[b] = a;
    sizes[a] += sizes[b];
    faces[a] &= faces[b];
}
fn register(
    values: [bool; 4],
    widths: Option<[i32; 4]>,
    registry: &mut StyleRegistry,
    part: &str,
    at: &Location,
) -> Result<u32> {
    let mut border = Border::default();
    for (index, (side, solid)) in
        [&mut border.left, &mut border.right, &mut border.top, &mut border.bottom]
            .into_iter()
            .zip(values)
            .enumerate()
    {
        if solid {
            side.line_type = BorderLineType::Solid;
            side.width = widths
                .map(|w| HwpUnit::new(w[index]))
                .transpose()
                .map_err(|e| DocxError::invalid(part, at, e.to_string()))?;
        }
    }
    registry
        .register_border_fill(BorderFill { border, fill: Fill::default() })
        .map_err(|e| DocxError::invalid(part, at, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 행마다 위/아래 양쪽 비용 부과, 내부/마지막 경계 누락, 열별 비용 합산.
    #[test]
    fn source_row_budgets_share_the_resolved_boundary_costs() {
        let rows = (0..2)
            .map(|_| Row {
                cells: vec![
                    Cell { span: 1, ..Cell::default() },
                    Cell { col: 1, span: 1, ..Cell::default() },
                ],
                ..Row::default()
            })
            .collect::<Vec<_>>();
        for (top, inside, bottom, expected) in
            [(50, 50, 50, [50, 100]), (150, 50, 150, [150, 200]), (50, 0, 50, [50, 50])]
        {
            let base = TableProperties {
                border_widths: [None, None, Some(top), Some(bottom), Some(inside), None],
                ..TableProperties::default()
            };
            assert_eq!(source_space(&rows, 2, &base).heights, expected);
        }
    }
    // 이것을 실패시키는 것: 수용된 면과 원본 행 예산의 불일치를 성공으로 낮춤.
    #[test]
    fn admitted_faces_must_reproduce_the_source_row_budget() {
        let mut table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![],
            HwpUnit::new(100).unwrap(),
        )])]);
        let grid = TableGrid::from_table(&table).unwrap();
        let space = SourceSpace { heights: vec![999], faces: vec![[50; 4]], cols: 1 };
        let mut registry = StyleRegistry::with_fonts(vec![]);
        let at = Location { name: Rc::from("tbl"), ordinal: 1 };
        assert!(apply(
            &mut table,
            &grid,
            &[[None; 6]],
            [Some(true); 6],
            &mut registry,
            ("body", &at),
            &mut vec![],
            &space,
            &[Some(50); 6],
            None
        )
        .is_err());
        assert!(registry.border_fill(1).is_none());
    }
}
