//! Output-only projection of imported table border space.
//!
//! Output subfields keep their existing policy: Caption.full_size/last_width are
//! preserved; caption descendants are outside body scope. Placement's
//! affect_line_spacing/hold_anchor_and_so are preserved and do not enter the
//! formula. Placement.out_margin is not consumed by the table writer, which
//! uses Table.out_margin instead.
//!
//! `placement` is irrelevant to this projection assuming a body flow table.
use crate::{EncodeWarning, HwpxError, HwpxResult, HwpxStyleStore, ParagraphPath, PathSeg};
use hwpforge_core::{
    table::{
        grid::{GridCoord, TableGrid},
        TableMargin,
    },
    Document, LayoutProfile, Paragraph, RunContent, Table, Validated,
};
use hwpforge_foundation::HwpUnit;
use std::collections::{HashMap, HashSet};

/// A table is identified by its containing run, independently of diagnostic scopes.
pub(crate) fn table_key(parent: &[PathSeg], run: usize) -> Vec<PathSeg> {
    let mut key = parent.to_vec();
    key.push(PathSeg::Run(run));
    key
}

type Slot = (usize, usize);
struct TablePlan {
    margins: HashMap<Slot, TableMargin>,
    emitted: HashSet<Slot>,
}
pub(crate) struct Projection {
    tables: HashMap<Vec<PathSeg>, TablePlan>,
    candidates: HashSet<Vec<PathSeg>>,
    visited: HashSet<Vec<PathSeg>>,
    pub(crate) warnings: Vec<EncodeWarning>,
    pub(crate) changed: bool,
}
fn invalid(detail: impl ToString) -> HwpxError {
    HwpxError::InvalidStructure { detail: detail.to_string() }
}
impl Projection {
    pub(crate) fn begin_table(&mut self, key: &[PathSeg]) -> HwpxResult<()> {
        if self.candidates.contains(key) && !self.visited.insert(key.to_vec()) {
            return Err(invalid("Table emitted more than once"));
        }
        Ok(())
    }
    pub(crate) fn margin(
        &mut self,
        path: &[PathSeg],
        slot: Slot,
    ) -> HwpxResult<Option<TableMargin>> {
        let Some(plan) = self.tables.get_mut(path) else { return Ok(None) };
        let margin =
            *plan.margins.get(&slot).ok_or_else(|| invalid("Unprepared table cell emitted"))?;
        if !plan.emitted.insert(slot) {
            return Err(invalid("Table cell emitted more than once"));
        }
        Ok(Some(margin))
    }
    pub(crate) fn finish(&self) -> HwpxResult<()> {
        if self.candidates != self.visited
            || self.tables.values().any(|p| p.margins.len() != p.emitted.len())
        {
            return Err(invalid("Table border space preparation and emission disagree"));
        }
        Ok(())
    }
    fn warning(&mut self, path: &[PathSeg], reason: &'static str) -> HwpxResult<()> {
        for warning in &mut self.warnings {
            if let EncodeWarning::TableBorderSpaceApproximated { reason: existing, count, .. } =
                warning
            {
                if *existing == reason {
                    *count = count
                        .checked_add(1)
                        .ok_or_else(|| invalid("Table warning count overflow"))?;
                    return Ok(());
                }
            }
        }
        self.warnings.push(EncodeWarning::TableBorderSpaceApproximated {
            path: ParagraphPath(path.to_vec()),
            reason,
            count: 1,
        });
        Ok(())
    }
}
fn widths(store: &HwpxStyleStore, id: u32) -> HwpxResult<[Option<i32>; 4]> {
    let fill = store.border_fill(id)?;
    Ok(store.imported_border_widths.get(&id).copied().unwrap_or_else(|| {
        [&fill.left, &fill.right, &fill.top, &fill.bottom]
            .map(|s| (s.line_type == "NONE").then_some(0))
    }))
}
// All current numerical decisions live here. Source-only admission belongs to the decoder.
fn plan(
    table: &Table,
    store: &HwpxStyleStore,
    profile: LayoutProfile,
    path: &[PathSeg],
) -> HwpxResult<(Option<TablePlan>, Option<&'static str>, bool)> {
    let fallback = table.border_fill_id.unwrap_or(crate::encoder::section::TABLE_BORDER_FILL_ID);
    let mut faces = HashMap::new();
    for (r, row) in table.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            faces.insert((r, c), widths(store, cell.border_fill_id.unwrap_or(fallback))?);
        }
    }
    if !faces.values().flatten().any(|w| w.is_some_and(|w| w > 0)) {
        return Ok((None, None, false));
    }
    let grid = TableGrid::from_table(table).map_err(invalid)?;
    let reason = if profile != LayoutProfile::WordCompatible
        || !crate::word_line_spacing::body_scope(path)
    {
        Some("outside_scope")
    } else if table.frame.size_protect {
        Some("size_protected")
    } else if table.cell_spacing.is_some_and(|v| v.as_i32() != 0) {
        Some("nonzero_cell_spacing")
    } else if !table.cell_zones.is_empty() {
        Some("cell_zones")
    } else if faces.values().flatten().any(Option::is_none) {
        Some("incomplete_widths")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Ok((None, Some(reason), false));
    }
    let (rows, cols) = grid.dimensions();
    let mut owner = vec![(0, 0); rows as usize * cols as usize];
    for cell in grid.iter_anchors() {
        for r in cell.anchor.row..cell.anchor.row + u32::from(cell.row_span) {
            for c in cell.anchor.col..cell.anchor.col + u32::from(cell.col_span) {
                owner[(r * cols + c) as usize] = (cell.row_idx, cell.cell_idx);
            }
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            let a = owner[(r * cols + c) as usize];
            for (rr, cc, side, other) in [(r, c + 1, 1, 0), (r + 1, c, 3, 2)] {
                if rr < rows && cc < cols {
                    let b = owner[(rr * cols + cc) as usize];
                    if a != b && faces[&a][side] != faces[&b][other] {
                        return Ok((None, Some("inconsistent_edges"), false));
                    }
                }
            }
        }
    }
    if table.rows.iter().flat_map(|r| &r.cells).any(|c| c.margin.or(table.in_margin).is_none()) {
        return Ok((None, Some("missing_margins"), false));
    }
    let positive: HashSet<i32> =
        faces.values().flatten().flatten().copied().filter(|w| *w > 0).collect();
    if positive.len() > 1 && positive.iter().any(|w| !matches!(w, 50 | 150)) {
        return Ok((None, Some("unverified_mixed_widths"), false));
    }
    let first =
        grid.resolve(GridCoord::new(0, 0)).ok_or_else(|| invalid("Missing first table cell"))?;
    let shift = faces[&(first.row_idx, first.cell_idx)][0].unwrap_or(0);
    if (0..rows).any(|r| faces[&owner[(r * cols) as usize]][0].unwrap_or(0) > shift) {
        return Ok((None, Some("later_left_wider"), false));
    }
    let mut margins = HashMap::new();
    let mut changed = false;
    for anchor in grid.iter_anchors() {
        let slot = (anchor.row_idx, anchor.cell_idx);
        let cell = &table.rows[slot.0].cells[slot.1];
        let m =
            cell.margin.or(table.in_margin).ok_or_else(|| invalid("Missing prepared margins"))?;
        let [l, r, t, b] = faces[&slot].map(|v| i64::from(v.unwrap_or(0)));
        let right = (i64::from(m.right.as_i32()) * 2).max(r) - i64::from(shift);
        if right < 0 {
            return Ok((None, Some("negative_right_margin"), false));
        }
        let out = TableMargin {
            left: rounded(i64::from(shift) + (i64::from(m.left.as_i32()) * 2).max(l))?,
            right: rounded(right)?,
            top: rounded(i64::from(m.top.as_i32()) * 2 + t * 2)?,
            bottom: rounded(
                i64::from(m.bottom.as_i32()) * 2
                    + if anchor.anchor.row + u32::from(anchor.row_span) == rows {
                        b * 2
                    } else {
                        0
                    },
            )?,
        };
        changed |= out != m;
        margins.insert(slot, out);
    }
    let reason = (positive.len() == 1 && positive.iter().any(|w| !matches!(w, 50 | 150)))
        .then_some("unverified_uniform_width");
    Ok((Some(TablePlan { margins, emitted: HashSet::new() }), reason, changed))
}
fn rounded(doubled: i64) -> HwpxResult<HwpUnit> {
    let half = doubled.div_euclid(2);
    let value = half + i64::from(doubled.rem_euclid(2) == 1 && half.rem_euclid(2) == 1);
    let value = i32::try_from(value).map_err(invalid)?;
    HwpUnit::new(value).map_err(invalid)
}

pub(crate) fn prepare(
    document: &Document<Validated>,
    store: &HwpxStyleStore,
) -> HwpxResult<Projection> {
    let mut result = Projection {
        tables: HashMap::new(),
        candidates: HashSet::new(),
        visited: HashSet::new(),
        warnings: Vec::new(),
        changed: false,
    };
    if store.imported_border_widths.is_empty() {
        return Ok(result);
    }
    let mut visitor = Visitor { result: &mut result, store, profile: document.layout_profile() };
    let mut master = 0;
    for (si, s) in document.sections().iter().enumerate() {
        visitor.paragraphs(&s.paragraphs, &mut vec![PathSeg::Section(si)], true)?;
        for (i, h) in s.headers.iter().enumerate() {
            visitor.paragraphs(
                &h.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::Header(i)],
                false,
            )?;
        }
        for (i, h) in s.footers.iter().enumerate() {
            visitor.paragraphs(
                &h.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::Footer(i)],
                false,
            )?;
        }
        for m in s.master_pages.iter().flatten() {
            visitor.paragraphs(
                &m.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::MasterPage(master)],
                false,
            )?;
            master += 1;
        }
    }
    Ok(result)
}
struct Visitor<'a> {
    result: &'a mut Projection,
    store: &'a HwpxStyleStore,
    profile: LayoutProfile,
}
impl Visitor<'_> {
    fn paragraphs(
        &mut self,
        paragraphs: &[Paragraph],
        path: &mut Vec<PathSeg>,
        body: bool,
    ) -> HwpxResult<()> {
        for (i, p) in paragraphs.iter().enumerate() {
            path.push(if body { PathSeg::BodyParagraph(i) } else { PathSeg::NestedParagraph(i) });
            for (i, run) in p.runs.iter().enumerate() {
                let parent = std::mem::replace(path, table_key(path, i));
                match &run.content {
                    RunContent::Table(t) => {
                        let (plan, reason, changed) = plan(t, self.store, self.profile, path)?;
                        self.result.changed |= changed;
                        if plan.is_some() || reason.is_some() {
                            self.result.candidates.insert(path.clone());
                        }
                        if let Some(plan) = plan {
                            self.result.tables.insert(path.clone(), plan);
                        }
                        if let Some(reason) = reason {
                            self.result.warning(path, reason)?;
                        }
                        if let Some(c) = &t.caption {
                            self.caption(c, path)?;
                        }
                        for (r, row) in t.rows.iter().enumerate() {
                            for (c, cell) in row.cells.iter().enumerate() {
                                path.push(PathSeg::TableCell { row: r, cell: c });
                                self.paragraphs(&cell.paragraphs, path, false)?;
                                path.pop();
                            }
                        }
                    }
                    RunContent::Image(i) => {
                        if let Some(c) = &i.caption {
                            self.caption(c, path)?;
                        }
                    }
                    RunContent::Control(c) => self.control(c, path)?,
                    _ => {}
                }
                *path = parent;
            }
            path.pop();
        }
        Ok(())
    }
    fn caption(
        &mut self,
        c: &hwpforge_core::caption::Caption,
        path: &mut Vec<PathSeg>,
    ) -> HwpxResult<()> {
        path.push(PathSeg::Caption);
        self.paragraphs(&c.paragraphs, path, false)?;
        path.pop();
        Ok(())
    }
    fn control(
        &mut self,
        c: &hwpforge_core::control::Control,
        path: &mut Vec<PathSeg>,
    ) -> HwpxResult<()> {
        use hwpforge_core::control::{Control, GroupChild};
        match c {
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                path.push(if matches!(c, Control::Footnote { .. }) {
                    PathSeg::Footnote
                } else {
                    PathSeg::Endnote
                });
                self.paragraphs(paragraphs, path, false)?;
                path.pop();
            }
            Control::TextBox { paragraphs, caption, .. }
            | Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                path.push(PathSeg::TextBox);
                self.paragraphs(paragraphs, path, false)?;
                path.pop();
                if let Some(c) = caption {
                    self.caption(c, path)?;
                }
            }
            Control::Rect { caption, .. }
            | Control::Line { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                if let Some(c) = caption {
                    self.caption(c, path)?;
                }
            }
            Control::FieldBoundary {
                boundary: boundary @ hwpforge_core::control::FieldBoundary::Start(start),
            } if crate::encoder::section::field_boundary_is_encodable(boundary) => {
                if let Some(content) = &start.annotation {
                    path.push(PathSeg::Memo);
                    self.paragraphs(content, path, false)?;
                    path.pop();
                }
            }
            Control::Memo { content, .. } => {
                path.push(PathSeg::Memo);
                self.paragraphs(content, path, false)?;
                path.pop();
            }
            Control::Group { children, .. } => {
                let mut i = 0;
                for c in children {
                    if let GroupChild::Shape(shape) = c {
                        if !crate::encoder::shapes::group_child_is_encodable(shape) {
                            continue;
                        }
                    }
                    path.push(PathSeg::GroupChild(i));
                    match c {
                        GroupChild::Shape(c) => self.control(c, path)?,
                        GroupChild::Picture(p) => {
                            if let Some(c) = &p.caption {
                                self.caption(c, path)?;
                            }
                        }
                        _ => {}
                    }
                    path.pop();
                    i += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn projection() -> Projection {
        let key = table_key(&[PathSeg::Section(0), PathSeg::BodyParagraph(0)], 0);
        let m = TableMargin::default();
        Projection {
            tables: HashMap::from([(
                key.clone(),
                TablePlan {
                    margins: HashMap::from([((0, 0), m), ((0, 1), m)]),
                    emitted: HashSet::new(),
                },
            )]),
            candidates: HashSet::from([key]),
            visited: HashSet::new(),
            warnings: vec![],
            changed: false,
        }
    }
    // 이것을 실패시키는 것: 같은 여백 값의 슬롯을 합치거나 방출 완전성 검사를 키 존재 검사로 대체.
    #[test]
    fn emission_requires_each_prepared_slot_exactly_once() {
        let key = table_key(&[PathSeg::Section(0), PathSeg::BodyParagraph(0)], 0);
        let mut p = projection();
        p.begin_table(&key).unwrap();
        p.margin(&key, (0, 0)).unwrap();
        assert!(p.finish().is_err());
        assert!(p.margin(&key, (0, 0)).is_err());
        assert!(p.margin(&key, (1, 0)).is_err());
        p.margin(&key, (0, 1)).unwrap();
        p.finish().unwrap();
        assert!(p.begin_table(&key).is_err());
        let mut p = projection();
        p.margin(&key, (0, 0)).unwrap();
        p.margin(&key, (0, 1)).unwrap();
        assert!(p.finish().is_err());
    }
    // 이것을 실패시키는 것: count overflow를 포화/잘림으로 바꾸거나 ties-to-even 대신 절반 올림.
    #[test]
    fn warning_counts_and_output_arithmetic_fail_before_overflow() {
        let mut p = projection();
        let key = table_key(&[], 0);
        p.warning(&key, "missing_margins").unwrap();
        if let EncodeWarning::TableBorderSpaceApproximated { count, .. } = &mut p.warnings[0] {
            *count = u64::MAX;
        }
        assert!(p.warning(&key, "missing_margins").is_err());
        assert_eq!(rounded(1117).unwrap().as_i32(), 558);
        assert_eq!(rounded(1119).unwrap().as_i32(), 560);
        assert!(rounded(i64::from(HwpUnit::MAX_VALUE) * 2 + 2).is_err());
        assert!(rounded(i64::MAX).is_err());
    }
    // 이것을 실패시키는 것: 새 경고를 의미 손실로 분류하거나 ContentLost를 비의미로 분류.
    #[test]
    fn approximation_returns_bytes_but_content_loss_keeps_fail_closed() {
        let advisory = EncodeWarning::TableBorderSpaceApproximated {
            path: ParagraphPath(vec![]),
            reason: "negative_right_margin",
            count: 1,
        };
        let outcome = crate::EncodeOutcome { bytes: vec![1, 2], warnings: vec![advisory.clone()] };
        assert_eq!(
            crate::encoder::split_successful_encode(outcome).unwrap(),
            (vec![1, 2], vec![advisory.clone()])
        );
        let loss = EncodeWarning::ContentLost {
            part: "Contents/section0.xml".into(),
            path: ParagraphPath(vec![]),
            kind: "unknown".into(),
            reason: "omitted".into(),
        };
        let err = crate::encoder::split_successful_encode(crate::EncodeOutcome {
            bytes: vec![1, 2],
            warnings: vec![advisory.clone(), loss.clone()],
        })
        .unwrap_err();
        assert_eq!(err, (vec![loss], vec![advisory]));
    }
    // 이것을 실패시키는 것: import map을 정의 비교/JSON에 포함하거나 PartialEq에서 제외.
    #[test]
    fn import_authority_is_private_and_distinct_from_definition_identity() {
        let plain = HwpxStyleStore::with_default_fonts("Batang");
        let mut imported = plain.clone();
        imported.imported_border_widths.insert(1, [Some(50); 4]);
        assert_ne!(plain, imported);
        assert!(plain.same_definitions(&imported));
        assert_eq!(serde_json::to_value(&plain).unwrap(), serde_json::to_value(&imported).unwrap());
        assert_eq!(imported.clone(), imported);
    }
    // 이것을 실패시키는 것: map 없는 Solid를 0으로 대체하거나 명시 NONE를 미확정으로 처리.
    #[test]
    fn unmapped_faces_distinguish_explicit_none_from_unknown_solid() {
        let mut store = HwpxStyleStore::with_default_fonts("Batang");
        let mut fill = crate::style_store::HwpxBorderFill::default_table_border();
        fill.left.line_type = "NONE".into();
        let id = store.push_border_fill(fill);
        assert_eq!(widths(&store, id).unwrap(), [Some(0), None, None, None]);
        store.imported_border_widths.insert(id, [Some(0), Some(50), Some(150), None]);
        assert_eq!(widths(&store, id).unwrap(), [Some(0), Some(50), Some(150), None]);
    }
}
