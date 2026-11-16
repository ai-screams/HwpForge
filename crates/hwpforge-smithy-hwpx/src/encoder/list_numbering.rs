//! Assign list presentations from the paragraph starts that actually reach XML.
mod counter;
#[cfg(test)]
mod tests;
use crate::list_marker_layout::{
    final_coordinates, paragraph_requirement, MarkerCoordinates, MarkerPlacement, MarkerRequirement,
};
use crate::paragraph_measure::invalid;
use crate::style_store::fidelity::HeadMetrics;
use crate::{EncodeWarning, HwpxResult, HwpxStyleStore};
use counter::CounterState;
use hwpforge_core::{BulletDef, NumberingDef, Paragraph, ParagraphMeasure};
use hwpforge_foundation::{HeadingType, HwpUnit, NumberFormatType, ParaShapeIndex};
use quick_xml::{events::Event, Reader};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DefinitionKey {
    bullet: bool,
    position: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ParagraphShapeKey {
    original: usize,
    segment: usize,
    margin: i32,
    margin_unit: &'static str,
    indent: i32,
    indent_unit: &'static str,
}
#[derive(Clone, Copy)]
pub(super) enum ListScope {
    Unsupported,
    TableCells(usize),
    TableCaption(usize),
}
struct ListUse {
    original: ParaShapeIndex,
    requirement: MarkerRequirement,
    unsupported: bool,
    cells: Vec<usize>,
    caption: Option<usize>,
}
pub(super) struct ListLedger {
    source: HwpxStyleStore,
    targets: BTreeSet<DefinitionKey>,
    uses: Vec<ListUse>,
    scopes: Vec<ListScope>,
    table_sequence: usize,
    budget: SplitBudget,
}
#[derive(Clone, Debug)]
struct SplitBudget {
    definitions: usize,
    text: usize,
    paragraphs: usize,
}
impl Default for SplitBudget {
    fn default() -> Self {
        Self { definitions: 16_384, text: 67_108_864, paragraphs: 16_384 }
    }
}
impl SplitBudget {
    fn remaining(&self, definitions: usize, text: usize, paragraphs: usize) -> Option<Self> {
        Some(Self {
            definitions: self.definitions.checked_sub(definitions)?,
            text: self.text.checked_sub(text)?,
            paragraphs: self.paragraphs.checked_sub(paragraphs)?,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ApproximationReason {
    Unsupported,
    SharedCaption,
    InvalidLevel,
    ZeroStart,
    Budget,
}
impl ApproximationReason {
    fn message(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported paragraph placement",
            Self::SharedCaption => "table caption and cells share a definition",
            Self::InvalidLevel => {
                "paragraph level is outside 0..=9 or head level is outside 1..=10"
            }
            Self::ZeroStart => "definition contains a zero start",
            Self::Budget => "split budget or start range exceeded",
        }
    }
}
struct Segment {
    starts: [u32; 10],
    heads: BTreeMap<u32, HeadMetrics>,
    counter: CounterState,
    unrequested: BTreeSet<u32>,
    widened: BTreeSet<u32>,
}
struct Assignment {
    use_id: usize,
    segment: usize,
}
struct AssignedUse {
    use_id: usize,
    shape: ParaShapeIndex,
    definition: u32,
    head: HeadMetrics,
}
struct PlannedDefinition {
    key: DefinitionKey,
    reasons: BTreeSet<ApproximationReason>,
    plan: Option<DefinitionPlan>,
}
struct DefinitionPlan {
    segments: Vec<Segment>,
    assignments: Vec<Assignment>,
    coordinates: Vec<MarkerCoordinates>,
    shape_keys: Vec<ParagraphShapeKey>,
    text_bytes: usize,
    additional_shapes: usize,
}
struct Mark {
    use_id: usize,
    reference: u32,
    value: Range<usize>,
    marker: Range<usize>,
}
struct PartEdits {
    marks: Vec<Mark>,
}
impl ListLedger {
    pub fn new(source: &HwpxStyleStore) -> HwpxResult<Option<Self>> {
        if source.list_marker_layouts.is_empty() {
            return Ok(None);
        }
        crate::list_marker_layout::validate_requests(source)?;
        let mut targets = BTreeSet::new();
        for id in source.list_marker_layouts.keys() {
            let shape = source.para_shape(ParaShapeIndex::new(*id as usize))?;
            if matches!(shape.heading_type, HeadingType::Number | HeadingType::Bullet) {
                targets.insert(DefinitionKey {
                    bullet: shape.heading_type == HeadingType::Bullet,
                    position: shape.heading_id_ref,
                });
            }
        }
        Ok(Some(Self {
            source: source.clone(),
            targets,
            uses: Vec::new(),
            scopes: Vec::new(),
            table_sequence: 0,
            budget: SplitBudget::default(),
        }))
    }
    pub fn push_scope(&mut self, scope: ListScope) {
        self.scopes.push(scope);
    }
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    pub fn next_table(&mut self) -> usize {
        let id = self.table_sequence;
        self.table_sequence += 1;
        id
    }
    pub fn record(&mut self, p: &Paragraph, path: &[crate::PathSeg]) -> HwpxResult<Option<usize>> {
        let shape = self.source.para_shape(p.para_shape_id)?;
        if !self.targets.contains(&DefinitionKey {
            bullet: shape.heading_type == HeadingType::Bullet,
            position: shape.heading_id_ref,
        }) || !matches!(shape.heading_type, HeadingType::Number | HeadingType::Bullet)
        {
            return Ok(None);
        }
        let Some(requirement) = paragraph_requirement(&self.source, p)? else {
            return Ok(None);
        };
        // 이것을 실패시키는 것: 안쪽 지원 자리로 바깥 미지원 상속을 덮음.
        let unsupported = path.iter().any(|s| {
            matches!(
                s,
                crate::PathSeg::Footer(_)
                    | crate::PathSeg::Endnote
                    | crate::PathSeg::Memo
                    | crate::PathSeg::MasterPage(_)
                    | crate::PathSeg::GroupChild(_)
            )
        }) || self.scopes.iter().any(|s| matches!(s, ListScope::Unsupported));
        let cells = self
            .scopes
            .iter()
            .filter_map(|s| if let ListScope::TableCells(id) = s { Some(*id) } else { None })
            .collect();
        let caption = self.scopes.iter().rev().find_map(|s| {
            if let ListScope::TableCaption(id) = s {
                Some(*id)
            } else {
                None
            }
        });
        let id = self.uses.len();
        self.uses.push(ListUse {
            original: p.para_shape_id,
            requirement,
            unsupported,
            cells,
            caption,
        });
        Ok(Some(id))
    }
    fn starts(&self, key: DefinitionKey) -> [u32; 10] {
        let mut starts = [1; 10];
        if !key.bullet {
            if let Some(def) = self.source.numbering_at(key.position as usize - 1) {
                for h in &def.levels {
                    if let Some(level) = level_index(h.level) {
                        starts[level] = h.start;
                    }
                }
            }
        }
        starts
    }
    fn head(&self, key: DefinitionKey, level: u32) -> Option<&hwpforge_core::ParaHead> {
        if key.bullet {
            self.source.bullet_at(key.position as usize - 1).map(|d| &d.para_head)
        } else {
            self.source
                .numbering_at(key.position as usize - 1)?
                .levels
                .iter()
                .find(|h| h.level == level)
        }
    }
    fn reasons(&self, key: DefinitionKey, ids: &[usize]) -> BTreeSet<ApproximationReason> {
        let mut reasons = BTreeSet::new();
        let mut cells = BTreeSet::new();
        let mut captions = BTreeSet::new();
        if !key.bullet
            && self
                .source
                .numbering_at(key.position as usize - 1)
                .is_some_and(|d| d.levels.iter().any(|h| h.start == 0))
        {
            reasons.insert(ApproximationReason::ZeroStart);
        }
        for &id in ids {
            let u = &self.uses[id];
            if u.unsupported {
                reasons.insert(ApproximationReason::Unsupported);
            }
            cells.extend(&u.cells);
            if let Some(t) = u.caption {
                captions.insert(t);
            }
            if !key.bullet {
                let Some(_level) = level_index(u.requirement.key.level) else {
                    reasons.insert(ApproximationReason::InvalidLevel);
                    continue;
                };
            }
        }
        if !cells.is_disjoint(&captions) {
            reasons.insert(ApproximationReason::SharedCaption);
        }
        reasons
    }
    fn plan(
        &self,
        key: DefinitionKey,
        ids: &[usize],
    ) -> HwpxResult<Result<DefinitionPlan, ApproximationReason>> {
        let bytes = if key.bullet {
            self.source
                .bullet_at(key.position as usize - 1)
                .ok_or_else(|| invalid("missing bullet definition"))?
                .para_head
                .text
                .len()
        } else {
            self.source
                .numbering_at(key.position as usize - 1)
                .ok_or_else(|| invalid("missing numbering definition"))?
                .levels
                .iter()
                .try_fold(0usize, |a, h| a.checked_add(h.text.len()))
                .ok_or_else(|| invalid("head text size overflow"))?
        };
        let starts = self.starts(key);
        let mut logical = CounterState::new(starts);
        let mut segments = vec![Segment {
            starts,
            heads: BTreeMap::new(),
            counter: CounterState::new(starts),
            unrequested: BTreeSet::new(),
            widened: BTreeSet::new(),
        }];
        let mut assignments = Vec::new();
        let mut values = BTreeMap::new();
        for &id in ids {
            let u = &self.uses[id];
            let r = &u.requirement;
            // Each emitted use must have a paragraph level in 0..=9 and select a head
            // level in 1..=10; otherwise the definition keeps the first-use projection.
            // A bullet head does not carry the paragraph level, so both are read here.
            if self.source.para_shape(u.original)?.heading_level >= 10
                || level_index(r.native_level).is_none()
            {
                return Ok(Err(ApproximationReason::InvalidLevel));
            }
            let level = if key.bullet {
                0
            } else {
                let Some(level) = level_index(r.key.level) else {
                    return Ok(Err(ApproximationReason::InvalidLevel));
                };
                level
            };
            let head =
                self.head(key, r.native_level).ok_or_else(|| invalid("missing list level"))?;
            let text = &head.text;
            if !key.bullet {
                logical.advance(level)?;
            }
            values.insert(id, logical.values[level]);
            let candidate = segments.last().expect("initial segment");
            let mut next = candidate.counter.clone();
            if !key.bullet {
                next.advance(level)?;
            }
            let widened = if matches!(r.placement, MarkerPlacement::FixedRight)
                && !key.bullet
                && head.num_format == NumberFormatType::Digit
            {
                match r.measured_size {
                    Some(size) => decimal_width(text, r.key.level, logical.values[level], size)?
                        .is_some_and(|width| {
                            i64::from(width) > i64::from(r.base) + i64::from(r.metrics.width_adjust)
                        }),
                    None => false,
                }
            } else {
                false
            };
            // 이것을 실패시키는 것: 넓힌 RIGHT head를 무요청 사용처와 공유.
            let preserves_unrequested = !(candidate.unrequested.contains(&r.key.level) && widened
                || !r.requested && candidate.widened.contains(&r.key.level));
            // 이것을 실패시키는 것: 재시작 뒤 출력 값과 논리 값의 비교를 제거.
            let compatible = preserves_unrequested
                && candidate.heads.get(&r.key.level).is_none_or(|m| m == &r.metrics)
                && (key.bullet || next.references(text) == logical.references(text));
            if !compatible {
                let Some(starts) = logical.segment_starts(level) else {
                    return Ok(Err(ApproximationReason::Budget));
                };
                // 이것을 실패시키는 것: 모든 구간을 만든 뒤에만 정의 예산을 검사.
                if self
                    .budget
                    .remaining(segments.len(), bytes.saturating_mul(segments.len()), 0)
                    .is_none()
                {
                    return Ok(Err(ApproximationReason::Budget));
                }
                segments.push(Segment {
                    starts,
                    heads: BTreeMap::new(),
                    counter: CounterState::new(starts),
                    unrequested: BTreeSet::new(),
                    widened: BTreeSet::new(),
                });
            }
            let segment = segments.last_mut().expect("initial segment");
            if !r.requested {
                segment.unrequested.insert(r.key.level);
            }
            if widened {
                segment.widened.insert(r.key.level);
            }
            segment.heads.entry(r.key.level).or_insert_with(|| r.metrics.clone());
            if !key.bullet {
                segment.counter.advance(level)?;
            }
            assignments.push(Assignment { use_id: id, segment: segments.len() - 1 });
        }
        // 이것을 실패시키는 것: RIGHT 폭 확장 후에도 이전 여백을 사용.
        for &Assignment { use_id: id, segment: index } in &assignments {
            let r = &self.uses[id].requirement;
            if !matches!(r.placement, MarkerPlacement::FixedRight) {
                continue;
            }
            let Some(size) = r.measured_size else {
                continue;
            };
            let head =
                self.head(key, r.native_level).ok_or_else(|| invalid("missing list level"))?;
            if key.bullet || head.num_format != NumberFormatType::Digit {
                continue;
            }
            if let Some(width) = decimal_width(&head.text, r.key.level, values[&id], size)? {
                let metrics = segments[index].heads.get_mut(&r.key.level).expect("assigned head");
                metrics.width_adjust = metrics.width_adjust.max(
                    width.checked_sub(r.base).ok_or_else(|| invalid("marker width overflow"))?,
                );
            }
        }
        let mut coordinates = Vec::new();
        let mut shape_keys = Vec::new();
        let mut original_keys = BTreeMap::new();
        let mut copies = BTreeSet::new();
        for &Assignment { use_id: id, segment } in &assignments {
            let u = &self.uses[id];
            let r = &u.requirement;
            let metrics = &segments[segment].heads[&r.key.level];
            let c = final_coordinates(r, metrics)?;
            let k = ParagraphShapeKey {
                original: u.original.get(),
                segment,
                margin: c.margin,
                margin_unit: c.margin_unit,
                indent: c.indent,
                indent_unit: c.indent_unit,
            };
            if original_keys.entry(u.original).or_insert(k) != &k {
                copies.insert(k);
            }
            if copies.len() > self.budget.paragraphs {
                return Ok(Err(ApproximationReason::Budget));
            }
            shape_keys.push(k);
            coordinates.push(c);
        }
        let Some(text_bytes) = bytes.checked_mul(segments.len() - 1) else {
            return Ok(Err(ApproximationReason::Budget));
        };
        if self.budget.remaining(segments.len() - 1, text_bytes, copies.len()).is_none() {
            return Ok(Err(ApproximationReason::Budget));
        }
        Ok(Ok(DefinitionPlan {
            segments,
            assignments,
            coordinates,
            shape_keys,
            text_bytes,
            additional_shapes: copies.len(),
        }))
    }
    pub fn finish(
        mut self,
        parts: &mut [super::section::SectionEncodeResult],
    ) -> HwpxResult<(HwpxStyleStore, Vec<EncodeWarning>)> {
        let mut edits = Vec::new();
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        for part in parts.iter() {
            let e = scan(&part.xml, self.uses.len())?;
            for m in &e.marks {
                if !seen.insert(m.use_id) {
                    return Err(invalid("duplicate list use marker"));
                }
                order.push(m.use_id);
            }
            edits.push(e);
            for (_, xml) in &part.master_pages {
                let e = scan(xml, self.uses.len())?;
                for m in &e.marks {
                    if !seen.insert(m.use_id) {
                        return Err(invalid("duplicate list use marker"));
                    }
                    order.push(m.use_id);
                }
                edits.push(e);
            }
        }
        let mut groups: BTreeMap<DefinitionKey, Vec<usize>> = BTreeMap::new();
        let mut keys = Vec::new();
        for &id in &order {
            let r = &self.uses[id].requirement;
            let key = DefinitionKey { bullet: r.key.bullet, position: r.key.position };
            if !groups.contains_key(&key) {
                keys.push(key);
            }
            groups.entry(key).or_default().push(id);
        }
        // Plans contain temporary segment positions; no output table changes precede approval.
        let mut plans = Vec::new();
        for key in keys {
            let ids = &groups[&key];
            let mut reasons = self.reasons(key, ids);
            // 이것을 실패시키는 것: 근사 사유만으로 분리 없는 정의에도 충돌 경고.
            let plan = match self.plan(key, ids)? {
                Ok(plan)
                    if reasons.is_empty()
                        || (plan.segments.len() == 1 && plan.additional_shapes == 0) =>
                {
                    self.budget = self
                        .budget
                        .remaining(plan.segments.len() - 1, plan.text_bytes, plan.additional_shapes)
                        .ok_or_else(|| invalid("unapproved split budget"))?;
                    Some(plan)
                }
                Ok(_) => None,
                Err(reason) => {
                    reasons.insert(reason);
                    None
                }
            };
            plans.push(PlannedDefinition { key, reasons, plan });
        }
        let mut out = std::mem::take(&mut self.source);
        let mut warnings = Vec::new();
        let mut references = BTreeMap::new();
        let mut expected = Vec::new();
        let mut used_shapes = BTreeSet::new();
        for PlannedDefinition { key, reasons, plan } in plans {
            let ids = &groups[&key];
            for &id in ids {
                used_shapes.insert(self.uses[id].original);
            }
            if let Some(plan) = plan {
                let mut definitions = vec![key.position];
                let base_fidelity = if key.bullet {
                    out.header_fidelity.bullet.get(&key.position).cloned()
                } else {
                    out.header_fidelity.numbering.get(&key.position).cloned()
                };
                for segment in plan.segments.iter().skip(1) {
                    let position = if key.bullet {
                        let d: BulletDef = out
                            .bullet_at(key.position as usize - 1)
                            .ok_or_else(|| invalid("missing bullet definition"))?
                            .clone();
                        out.push_bullet(d);
                        out.bullet_count() as u32
                    } else {
                        let mut d: NumberingDef = out
                            .numbering_at(key.position as usize - 1)
                            .ok_or_else(|| invalid("missing numbering definition"))?
                            .clone();
                        for h in &mut d.levels {
                            if let Some(level) = level_index(h.level) {
                                h.start = segment.starts[level];
                            }
                        }
                        d.start = segment.starts[0];
                        out.push_numbering(d);
                        out.numbering_count() as u32
                    };
                    if let Some(f) = &base_fidelity {
                        if key.bullet {
                            out.header_fidelity.bullet.insert(position, f.clone());
                        } else {
                            out.header_fidelity.numbering.insert(position, f.clone());
                        }
                    }
                    definitions.push(position);
                }
                for (i, segment) in plan.segments.iter().enumerate() {
                    for (&level, metrics) in &segment.heads {
                        if get_head(&out, key.bullet, definitions[i], level) != *metrics {
                            set_head(&mut out, key.bullet, definitions[i], level, metrics.clone())?;
                        }
                    }
                }
                let mut first = BTreeMap::new();
                let mut shapes = BTreeMap::new();
                let mut diagnosed = BTreeSet::new();
                for ((Assignment { use_id: id, segment }, c), k) in
                    plan.assignments.iter().zip(&plan.coordinates).zip(&plan.shape_keys)
                {
                    let u = &self.uses[*id];
                    let original = u.original;
                    let target = if let Some(id) = shapes.get(k) {
                        *id
                    } else {
                        let mut shape = out.para_shape(original)?.clone();
                        shape.heading_id_ref = definitions[*segment];
                        set_coordinates(&mut shape, c)?;
                        let id = if first.insert(original, ()).is_none() {
                            *out.para_shape_mut(original)? = shape;
                            original
                        } else {
                            let id = out.push_para_shape(shape);
                            if let Some(f) = out
                                .header_fidelity
                                .auto_spacing
                                .get(&(original.get() as u32))
                                .copied()
                            {
                                out.header_fidelity.auto_spacing.insert(id.get() as u32, f);
                            }
                            id
                        };
                        shapes.insert(*k, id);
                        id
                    };
                    references.insert(*id, target.get() as u32);
                    expected.push(AssignedUse {
                        use_id: *id,
                        shape: target,
                        definition: definitions[*segment],
                        head: plan.segments[*segment].heads[&u.requirement.key.level].clone(),
                    });
                    for warning in &u.requirement.warnings {
                        if let EncodeWarning::ContentLost { kind, .. } = warning {
                            if diagnosed.insert((u.requirement.key.level, kind.clone())) {
                                warnings.push(warning.clone());
                            }
                        }
                    }
                    if let Some(warning) = crate::list_marker_layout::continuation_warning(
                        original,
                        c.margin,
                        u.requirement.continuation,
                        c.indent,
                        u.requirement.requested,
                    ) {
                        if diagnosed
                            .insert((u.requirement.key.level, "list_marker_continuation".into()))
                        {
                            warnings.push(warning);
                        }
                    }
                }
                if definitions.len() > 1 {
                    warnings.push(EncodeWarning::ListNumberingSplit {
                        kind: if key.bullet { HeadingType::Bullet } else { HeadingType::Number },
                        definition_id: key.position,
                        additional_definitions: definitions.len() - 1,
                    });
                }
            } else {
                let reason = reasons.iter().map(|r| r.message()).collect::<Vec<_>>().join("; ");
                let (definition_warnings, has_conflicts) =
                    crate::list_marker_layout::project_first_uses(
                        &mut out,
                        ids.iter().map(|&id| (self.uses[id].original, &self.uses[id].requirement)),
                        Some(&reason),
                    )?;
                // Validate the local bundle before appending; never rescan earlier definitions.
                validate_approximations(has_conflicts, &definition_warnings)?;
                warnings.extend(definition_warnings);
                for &id in ids {
                    references.insert(id, self.uses[id].original.get() as u32);
                }
            }
        }
        for id in used_shapes {
            out.list_marker_layouts.remove(&(id.get() as u32));
        }
        crate::list_marker_layout::fold_requests(&mut out)?;
        validate(&out, &self.uses, &expected)?;
        let mut e = edits.into_iter();
        for part in parts {
            part.xml = rewrite(&part.xml, e.next().expect("section edits"), &references)?;
            for (_, xml) in &mut part.master_pages {
                *xml = rewrite(xml, e.next().expect("master edits"), &references)?;
            }
        }
        Ok((out, warnings))
    }
}
fn level_index(level: u32) -> Option<usize> {
    level.checked_sub(1).filter(|&level| level < 10).map(|level| level as usize)
}
fn set_head(
    out: &mut HwpxStyleStore,
    bullet: bool,
    position: u32,
    level: u32,
    metrics: HeadMetrics,
) -> HwpxResult<()> {
    let align = match metrics.align.as_str() {
        "LEFT" => hwpforge_foundation::Alignment::Left,
        "RIGHT" => hwpforge_foundation::Alignment::Right,
        "CENTER" => hwpforge_foundation::Alignment::Center,
        _ => return Err(invalid("unsupported marker head alignment")),
    };
    let native_level = if bullet {
        out.bullet_at(position as usize - 1)
            .ok_or_else(|| invalid("missing bullet definition"))?
            .para_head
            .level
    } else {
        level
    };
    out.set_paragraph_head_metrics(
        if bullet { HeadingType::Bullet } else { HeadingType::Number },
        position,
        native_level,
        align,
        metrics.use_inst_width != 0,
        metrics.auto_indent != 0,
        metrics.width_adjust,
        metrics.text_offset,
        metrics.text_offset_type == "HWPUNIT",
        metrics.char_pr_id_ref,
    )
}
fn get_head(out: &HwpxStyleStore, bullet: bool, position: u32, level: u32) -> HeadMetrics {
    let map = if bullet { &out.header_fidelity.bullet } else { &out.header_fidelity.numbering };
    map.get(&position)
        .and_then(|m| m.get(&level))
        .cloned()
        .unwrap_or_else(|| HeadMetrics::generated(u32::from(!bullet)))
}
fn set_coordinates(
    shape: &mut crate::style_store::HwpxParaShape,
    c: &MarkerCoordinates,
) -> HwpxResult<()> {
    if c.margin_unit == "HWPUNIT" {
        shape.margin_left = ParagraphMeasure::from_hwp_unit(HwpUnit::new(c.margin)?);
    }
    if c.indent_unit == "HWPUNIT" {
        shape.indent = ParagraphMeasure::from_hwp_unit(HwpUnit::new(c.indent)?);
    }
    Ok(())
}
#[cfg(test)]
fn conflict(key: DefinitionKey, reason: String) -> EncodeWarning {
    EncodeWarning::ContentLost {
        part: "Contents/header.xml".into(),
        path: crate::ParagraphPath(Vec::new()),
        kind: "list_marker_conflict".into(),
        reason: format!(
            "{} definition position {}: {reason}; first use head and relative margin shift applied",
            if key.bullet { "bullet" } else { "numbering" },
            key.position
        ),
    }
}
fn validate_approximations(has_conflicts: bool, warnings: &[EncodeWarning]) -> HwpxResult<()> {
    let count = warnings.iter().filter(|w| matches!(w, EncodeWarning::ContentLost { kind, .. } if kind == "list_marker_conflict")).count();
    if count != usize::from(has_conflicts) {
        return Err(invalid(
            "approximate list conflict warning count does not match head differences",
        ));
    }
    Ok(())
}
fn validate(out: &HwpxStyleStore, uses: &[ListUse], expected: &[AssignedUse]) -> HwpxResult<()> {
    for AssignedUse { use_id: id, shape: para, definition, head } in expected {
        let r = &uses[*id].requirement;
        let p = out.para_shape(*para)?;
        if p.heading_id_ref != *definition
            || get_head(out, r.key.bullet, *definition, r.key.level) != *head
        {
            return Err(invalid("list head does not match its assigned requirement"));
        }
        let coordinates = final_coordinates(r, head)?;
        let (margin, margin_unit) = crate::paragraph_measure::parts(p.margin_left);
        let (indent, indent_unit) = crate::paragraph_measure::parts(p.indent);
        if coordinates != (MarkerCoordinates { margin, margin_unit, indent, indent_unit }) {
            return Err(invalid("list coordinates do not match their assigned requirement"));
        }
    }
    Ok(())
}
fn decimal_width(text: &str, level: u32, value: u64, size: i32) -> HwpxResult<Option<i32>> {
    let own = format!("^{level}");
    let Some((a, b)) = text.split_once(&own) else {
        return Ok(None);
    };
    // 이것을 실패시키는 것: ^1을 ^10의 접두 토큰으로 수용.
    if (level == 1 && b.starts_with('0'))
        || !a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit() || c == b'.')
    {
        return Ok(None);
    }
    let mut ratio = 0i64;
    for c in a.bytes().chain(value.to_string().bytes()).chain(b.bytes()) {
        ratio = ratio
            .checked_add(if c == b'.' { 242 } else { 606 })
            .ok_or_else(|| invalid("marker width overflow"))?;
    }
    let width = i64::from(size)
        .checked_mul(ratio)
        .and_then(|v| v.checked_add(550))
        .ok_or_else(|| invalid("marker width overflow"))?
        / 1100;
    Ok(Some(i32::try_from(width).map_err(|_| invalid("marker width overflow"))?))
}
fn scan(xml: &str, count: usize) -> HwpxResult<PartEdits> {
    let mut reader = Reader::from_str(xml);
    let mut marks = Vec::new();
    loop {
        match reader.read_event().map_err(|e| invalid(format!("list XML: {e}")))? {
            Event::Start(e) | Event::Empty(e) => {
                let mut marker = None;
                let mut reference = None;
                for a in e.attributes() {
                    let a = a.map_err(|e| invalid(format!("list attribute: {e}")))?;
                    if a.key.as_ref() == "hwpforgeList"
                        || (e.local_name().as_ref() == "p" && a.key.as_ref() == "paraPrIDRef")
                    {
                        let start = a.value.as_ptr() as usize - xml.as_ptr() as usize;
                        let end = start + a.value.len();
                        if end > xml.len() {
                            return Err(invalid("list attribute outside XML"));
                        }
                        if a.key.as_ref() == "hwpforgeList" {
                            if e.local_name().as_ref() != "p" {
                                return Err(invalid("list marker outside paragraph"));
                            }
                            let id = xml[start..end]
                                .parse::<usize>()
                                .map_err(|_| invalid("invalid list marker"))?;
                            if id >= count {
                                return Err(invalid("unknown list marker"));
                            }
                            let key_start =
                                a.key.as_ref().as_ptr() as usize - xml.as_ptr() as usize;
                            let mut remove_start = key_start;
                            while remove_start > 0
                                && xml.as_bytes()[remove_start - 1].is_ascii_whitespace()
                            {
                                remove_start -= 1;
                            }
                            marker = Some((id, remove_start..end + 1));
                        } else {
                            let id = xml[start..end]
                                .parse::<u32>()
                                .map_err(|_| invalid("invalid list paragraph reference"))?;
                            reference = Some((id, start..end));
                        }
                    }
                }
                if let Some((use_id, marker)) = marker {
                    let (reference, value) =
                        reference.ok_or_else(|| invalid("list paragraph lacks a reference"))?;
                    marks.push(Mark { use_id, reference, value, marker });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(PartEdits { marks })
}
fn rewrite(xml: &str, edits: PartEdits, references: &BTreeMap<usize, u32>) -> HwpxResult<String> {
    let mut replacements = Vec::new();
    for m in edits.marks {
        let id = references.get(&m.use_id).ok_or_else(|| invalid("unassigned list marker"))?;
        if *id != m.reference {
            replacements.push((m.value, id.to_string()));
        }
        replacements.push((m.marker, String::new()));
    }
    replacements.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(xml.len());
    let mut at = 0;
    for (range, value) in replacements {
        if range.start < at || range.end > xml.len() {
            return Err(invalid("overlapping list edits"));
        }
        out.push_str(&xml[at..range.start]);
        out.push_str(&value);
        at = range.end;
    }
    out.push_str(&xml[at..]);
    scan(&out, 0)?;
    Ok(out)
}
