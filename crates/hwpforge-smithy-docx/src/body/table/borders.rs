use super::*;
use hwpforge_blueprint::registry::StyleRegistry;

pub(super) fn apply(
    table: &mut Table,
    grid: &TableGrid,
    overrides: &[[Option<bool>; 6]],
    base: [Option<bool>; 6],
    registry: &mut StyleRegistry,
    location: (&str, &Location),
    w: &mut Vec<DocxWarning>,
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
    for (i, c) in anchors.iter().enumerate() {
        let mut values = [false; 4];
        for (side, value) in values.iter_mut().enumerate() {
            *value = faces[root(i * 4 + side, &mut parents)];
        }
        table.rows[c.row_idx].cells[c.cell_idx].border_fill_id =
            Some(register(values, registry, part, at)?);
    }
    table.border_fill_id = Some(register(
        [
            base[0].unwrap_or(false),
            base[1].unwrap_or(false),
            base[2].unwrap_or(false),
            base[3].unwrap_or(false),
        ],
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
    registry: &mut StyleRegistry,
    part: &str,
    at: &Location,
) -> Result<u32> {
    let mut border = Border::default();
    for (side, solid) in [&mut border.left, &mut border.right, &mut border.top, &mut border.bottom]
        .into_iter()
        .zip(values)
    {
        if solid {
            side.line_type = BorderLineType::Solid;
        }
    }
    registry
        .register_border_fill(BorderFill { border, fill: Fill::default() })
        .map_err(|e| DocxError::invalid(part, at, e.to_string()))
}
