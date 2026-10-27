//! Unified field discovery, range validation and value-edit policy.
//!
//! Each paragraph list is an independent flow. Ranges may cross paragraphs
//! and overlap, but cannot cross into a cell, note, caption or annotation.
//! Bodies borrow one shared run array per flow; ranges store only two indices.
//! New editors must call `protects`, `protects_paragraph` or `protects_insertion`
//! before mutation, and add their consumer column to the representation tests.
//! Do not match field representations in editors: this module owns that policy.
use crate::control::{FieldBoundary, FieldParameter, GroupChild};
use crate::node_path::CorePath;
use crate::{Control, Paragraph, RunContent, Section, ValidationError};
use hwpforge_foundation::FieldType;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

/// Why a field value can or cannot be replaced by a plain text edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldEditability {
    /// One plain text value, with no overlapping protected range.
    Editable,
    /// A calculated value, or a field kind without a value-edit contract.
    Protected,
    /// A valid range whose body is not one plain text value.
    UnsupportedRange,
    /// A source range with no end; readable but protected from edits.
    Unclosed,
    /// Incomplete or inconsistent boundary pairing in this flow.
    InvalidBoundary,
}

/// A borrowed run location. Text and subtrees are never copied into the inventory.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldBodyLocation<'a> {
    path: CorePath,
    original: &'a RunContent,
}
impl FieldBodyLocation<'_> {
    /// Semantic slot path, relative to the section.
    pub fn path(&self) -> &str {
        self.path.relative()
    }
    /// Section-qualified address shared with preservation and editors.
    pub fn address(&self) -> &CorePath {
        &self.path
    }
    /// Original run content, borrowed from Core.
    pub fn content(&self) -> &RunContent {
        self.original
    }
}

/// One field backed by a shared flow and a half-open body interval.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FieldEntry<'a> {
    section: usize,
    kind: String,
    name: Option<String>,
    id: Option<String>,
    flow: String,
    start: String,
    end: Option<String>,
    runs: Arc<[FieldBodyLocation<'a>]>,
    range: Range<usize>,
    begin: usize,
    finish: usize,
    hint: Option<String>,
    editability: FieldEditability,
}
impl PartialEq for FieldEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.section == other.section
            && self.kind == other.kind
            && self.name == other.name
            && self.id == other.id
            && self.flow == other.flow
            && self.start == other.start
            && self.end == other.end
            && self.body() == other.body()
            && self.hint == other.hint
            && self.editability == other.editability
    }
}
impl<'a> FieldEntry<'a> {
    /// Section containing this field.
    pub fn section(&self) -> usize {
        self.section
    }
    /// Authored field kind.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// Authored name.
    pub fn name(&self) -> &Option<String> {
        &self.name
    }
    /// Authored instance identifier.
    pub fn id(&self) -> &Option<String> {
        &self.id
    }
    /// Independent flow path.
    pub fn flow(&self) -> &str {
        &self.flow
    }
    /// Start run path.
    pub fn start(&self) -> &String {
        &self.start
    }
    /// End run path, if present.
    pub fn end(&self) -> &Option<String> {
        &self.end
    }
    /// Placeholder hint.
    pub fn hint(&self) -> &Option<String> {
        &self.hint
    }
    /// Shared edit decision.
    pub fn editability(&self) -> FieldEditability {
        self.editability
    }
    /// Borrow the body without allocating or copying run content.
    pub fn body(&self) -> &[FieldBodyLocation<'a>] {
        &self.runs[self.range.clone()]
    }
    /// Concatenates visible content on demand.
    pub fn current(&self) -> String {
        self.body().iter().map(|b| visible_value(b.original)).collect()
    }
}

/// Shared field semantics and structural edit preflight.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct FieldInventory<'a> {
    fields: Vec<FieldEntry<'a>>,
    diagnostics: Vec<ValidationError>,
    // Retain the depth-check result independently of diagnostic display text.
    depth_exceeded: bool,
    warnings: Vec<ValidationError>,
    positions: HashMap<CorePath, (String, usize)>,
    protected: HashMap<(usize, String), Vec<bool>>,
    protection_work: std::cell::Cell<usize>,
    #[cfg(test)]
    prefix_steps: std::cell::Cell<usize>,
}
impl<'a> FieldInventory<'a> {
    /// Visit all independent paragraph flows using the common semantic paths.
    pub fn visit_flows(
        sections: &'a [Section],
        mut visitor: impl FnMut(usize, &'a [Paragraph], &str),
    ) {
        for (si, section) in sections.iter().enumerate() {
            walk_section(section, &mut |ps, path| visitor(si, ps, path));
        }
    }
    /// Fields containing a shared semantic address.
    pub fn at_path<'b>(
        &'b self,
        path: &crate::node_path::CorePath,
    ) -> impl Iterator<Item = &'b FieldEntry<'a>> {
        self.fields_at(path.section(), path.relative())
    }
    /// Fields in document order.
    pub fn fields(&self) -> &[FieldEntry<'a>] {
        &self.fields
    }
    /// Invalid boundary pairs or input exceeding the Core nesting budget.
    pub fn diagnostics(&self) -> &[ValidationError] {
        &self.diagnostics
    }
    /// Recovered source ranges.
    pub fn warnings(&self) -> &[ValidationError] {
        &self.warnings
    }
    /// Number of stored run references, independent of range overlap.
    pub fn stored_run_count(&self) -> usize {
        let mut seen = HashSet::new();
        self.fields.iter().filter(|f| seen.insert(f.runs.as_ptr())).map(|f| f.runs.len()).sum()
    }
    /// Build an inventory without copying text or subtrees.
    /// Over-budget input is reported through [`Self::diagnostics`]. Fields and
    /// read-only protection remain complete; value clearing rejects only depth
    /// violations, while insert/delete/replace reject all diagnostics.
    pub fn from_sections(sections: &'a [Section]) -> Self {
        Self::from_sections_at(sections, 0)
    }
    /// Inventory a section slice while retaining its original document indices.
    /// Section editors must use this rather than renumber source evidence to zero.
    pub fn from_sections_at(sections: &'a [Section], first_section: usize) -> Self {
        let mut inventory = Self::default();
        // 이것을 실패시키는 것: 깊이 관문 삭제 (field_inventory_depth_diagnostics).
        for (si, section) in sections.iter().enumerate() {
            if let Err(error) = section.check_nesting_depth() {
                // 이것을 실패시키는 것: 깊이 초과 기록 삭제 (clear_editable_values_is_noop_only_above_depth_limit).
                inventory.depth_exceeded = true;
                inventory.invalid(
                    si + first_section,
                    "paragraphs",
                    &error
                        .to_string()
                        .replace("section=0", &format!("section={}", si + first_section)),
                );
            }
        }
        // Read-only consumers still need complete fields/protection at excessive depth.
        // Flow traversal is iterative; mutating operations reject diagnostics separately.
        for (si, section) in sections.iter().enumerate() {
            walk_section(section, &mut |paragraphs, path| {
                inventory.flow(si + first_section, paragraphs, path)
            });
        }
        for field in &mut inventory.fields {
            if field.editability == FieldEditability::Editable
                && [".group.", ".memo.", ".annotation.", "master_pages["]
                    .iter()
                    .any(|p| field.start.contains(p))
            {
                field.editability = FieldEditability::UnsupportedRange;
            }
        }
        inventory.index_protection();
        let mut blocked = Vec::new();
        for (i, field) in inventory.fields.iter().enumerate() {
            if field.editability == FieldEditability::Editable {
                for body in field.body() {
                    if inventory.protects(field.section, body.path()) {
                        blocked.push(i);
                        break;
                    }
                }
            }
        }
        for i in blocked {
            inventory.fields[i].editability = FieldEditability::UnsupportedRange;
        }
        inventory.index_protection();
        inventory.fields.sort_by_cached_key(|f| {
            (
                f.section,
                if f.start.starts_with("paragraphs[") {
                    0
                } else if f.start.starts_with("header") {
                    1
                } else if f.start.starts_with("footer") {
                    2
                } else {
                    3
                },
                natural_path(&f.start),
            )
        });
        inventory
    }
    /// Count interval updates, sweep positions and ancestor probes used to
    /// determine editability. Excludes discovery and document-order sorting.
    pub fn protection_work(&self) -> usize {
        self.protection_work.get()
    }
    fn index_protection(&mut self) {
        let mut differences: HashMap<(usize, String), Vec<isize>> = HashMap::new();
        for field in &self.fields {
            let d = differences
                .entry((field.section, field.flow.clone()))
                .or_insert_with(|| vec![0; field.runs.len() + 1]);
            self.protection_work.set(self.protection_work.get() + 1);
            if field.editability != FieldEditability::Editable {
                let begin = if field.editability == FieldEditability::Unclosed {
                    field.begin
                } else {
                    field.range.start
                };
                d[begin] += 1;
                d[field.range.end] -= 1;
            }
        }
        self.protected = differences
            .into_iter()
            .map(|(flow, d)| {
                let mut active = 0;
                let protected = d
                    .into_iter()
                    .map(|delta| {
                        self.protection_work.set(self.protection_work.get() + 1);
                        active += delta;
                        active > 0
                    })
                    .collect();
                (flow, protected)
            })
            .collect();
    }
    /// Insert top-level paragraphs after checking the shared field policy.
    /// Provenance moves only as part of this actual edit, never by supplying
    /// a replacement address. Existing stale evidence is not repaired.
    pub fn insert_paragraphs(
        sections: &mut [Section],
        section: usize,
        at: usize,
        paragraphs: Vec<Paragraph>,
    ) -> bool {
        let inventory = FieldInventory::from_sections(sections);
        if !inventory.diagnostics.is_empty()
            || inventory.protects_insertion(section, "paragraphs", at, None)
            || sections.get(section).is_none_or(|s| at > s.paragraphs.len())
        {
            return false;
        }
        let count = paragraphs.len();
        Self::relocate_paragraph_evidence(sections, section, |old| {
            Some(if old >= at { old + count } else { old })
        });
        sections[section].paragraphs.splice(at..at, paragraphs);
        true
    }
    /// Delete top-level paragraphs with validated, edit-owned provenance moves.
    pub fn delete_paragraphs(sections: &mut [Section], section: usize, indices: &[usize]) -> bool {
        let inventory = FieldInventory::from_sections(sections);
        if !inventory.diagnostics.is_empty()
            || sections
                .get(section)
                .is_none_or(|s| indices.iter().any(|i| *i >= s.paragraphs.len()))
            || indices.iter().any(|i| inventory.protects_paragraph(section, "paragraphs", *i))
        {
            return false;
        }
        let mut indices = indices.to_vec();
        indices.sort_unstable();
        indices.dedup();
        // Detached section containers disappear with their explicit paragraph
        // anchor. Apply the same recursive policy as for paragraph descendants
        // before relocating evidence or mutating any part of the section.
        // 이것을 실패시키는 것: 앵커 컨테이너 검사 또는 header/footer 한쪽 생략.
        for (tag, list) in
            [("header", &sections[section].headers), ("footer", &sections[section].footers)]
        {
            for (index, hf) in list.iter().enumerate() {
                if hf.anchor_paragraph.is_some_and(|a| indices.binary_search(&a).is_ok()) {
                    let prefix =
                        if list.len() == 1 { tag.to_owned() } else { format!("{tag}s[{index}]") };
                    let flow = format!("{prefix}.paragraphs");
                    if (0..hf.paragraphs.len())
                        .any(|p| inventory.protects_paragraph(section, &flow, p))
                    {
                        return false;
                    }
                }
            }
        }
        let mut translations = Vec::new();
        for (tag, list) in
            [("header", &sections[section].headers), ("footer", &sections[section].footers)]
        {
            let retained: Vec<_> = list
                .iter()
                .enumerate()
                .filter(|(_, hf)| hf.anchor_paragraph.is_none_or(|a| !indices.contains(&a)))
                .map(|(i, _)| i)
                .collect();
            for (new, old) in retained.iter().enumerate() {
                let before =
                    if list.len() == 1 { tag.to_owned() } else { format!("{tag}s[{old}]") };
                let after =
                    if retained.len() == 1 { tag.to_owned() } else { format!("{tag}s[{new}]") };
                translations.push((before, after));
            }
        }
        Self::relocate_evidence(sections, section, |old| {
            translations.iter().find_map(|(before, after)| {
                old.strip_prefix(before)
                    .filter(|rest| rest.starts_with('.'))
                    .map(|rest| format!("{after}{rest}"))
            })
        });
        Self::relocate_paragraph_evidence(sections, section, |old| {
            (!indices.contains(&old)).then(|| old - indices.partition_point(|i| *i < old))
        });
        for list in [&mut sections[section].headers, &mut sections[section].footers] {
            list.retain(|hf| hf.anchor_paragraph.is_none_or(|a| !indices.contains(&a)));
            for hf in list.iter_mut() {
                if let Some(a) = hf.anchor_paragraph {
                    hf.anchor_paragraph =
                        Some(a - indices.partition_point(|i| *i < a)).filter(|a| *a > 0);
                }
            }
        }
        for index in indices.into_iter().rev() {
            sections[section].paragraphs.remove(index);
        }
        true
    }
    fn relocate_paragraph_evidence(
        sections: &mut [Section],
        section: usize,
        map: impl Fn(usize) -> Option<usize>,
    ) {
        Self::relocate_evidence(sections, section, |old| {
            let rest = old.strip_prefix("paragraphs[")?;
            let (index, suffix) = rest.split_once(']')?;
            let new = index.parse().ok().and_then(&map)?;
            Some(format!("paragraphs[{new}]{suffix}"))
        });
    }
    fn relocate_evidence(
        sections: &mut [Section],
        section: usize,
        map: impl Fn(&str) -> Option<String>,
    ) {
        walk_section_mut(&mut sections[section], &mut |paragraphs, prefix| {
            for (pi, p) in paragraphs.iter_mut().enumerate() {
                for (ri, run) in p.runs.iter_mut().enumerate() {
                    let old = format!("{prefix}[{pi}].runs[{ri}]");
                    let Some(new) = map(&old) else { continue };
                    if let RunContent::Control(c) = &mut run.content {
                        if let Control::FieldBoundary { boundary: FieldBoundary::Start(start) } =
                            c.as_mut()
                        {
                            if let Some(origin) = &mut start.recovery.0 {
                                if start.unclosed
                                    && origin.id() == start.id
                                    && origin.path() == &CorePath::new(section, &old)
                                {
                                    origin.relocate(CorePath::new(section, new));
                                }
                            }
                        }
                    }
                }
            }
        });
    }
    /// Attach verified original XML evidence to matching decoded starts.
    /// Evidence is obtained from `XmlSource`, never from a boolean flag.
    pub fn recover_source_ranges(
        sections: &mut [Section],
        origins: &[crate::source_xml::UnclosedOrigin],
        package: &[u8],
    ) {
        Self::recover_shared_source_ranges(sections, origins, Arc::from(package));
    }
    /// Batch source recovery with caller-owned, shared immutable package bytes.
    pub fn recover_shared_source_ranges(
        sections: &mut [Section],
        origins: &[crate::source_xml::UnclosedOrigin],
        package: Arc<[u8]>,
    ) {
        let origins = crate::source_xml::UnclosedOrigin::with_package_batch(origins, package);
        Self::attach_source_ranges(sections, &origins);
    }
    fn attach_source_ranges(
        sections: &mut [Section],
        origins: &[crate::source_xml::UnclosedOrigin],
    ) {
        crate::source_xml::record_provenance_work(origins.len());
        let origins: HashMap<_, _> =
            origins.iter().map(|o| ((o.path().section(), o.path().relative()), o)).collect();
        for (si, section) in sections.iter_mut().enumerate() {
            walk_section_mut(section, &mut |paragraphs, prefix| {
                for (pi, paragraph) in paragraphs.iter_mut().enumerate() {
                    for (ri, run) in paragraph.runs.iter_mut().enumerate() {
                        let path = format!("{prefix}[{pi}].runs[{ri}]");
                        if let RunContent::Control(c) = &mut run.content {
                            if let Control::FieldBoundary { boundary: FieldBoundary::Start(s) } =
                                c.as_mut()
                            {
                                if let Some(origin) =
                                    origins.get(&(si, path.as_str())).filter(|o| o.id() == s.id)
                                {
                                    s.unclosed = true;
                                    s.recovery.0 = Some((*origin).clone());
                                }
                            }
                        }
                    }
                }
            });
        }
    }
    /// Visit every claimed unmatched start, including stale claims. A format
    /// verifier must check the current address/ID and canonical source mapping.
    pub fn source_claims(
        sections: &'a [Section],
    ) -> Vec<(CorePath, &'a crate::control::FieldStart)> {
        let mut claims = Vec::new();
        Self::visit_flows(sections, |si, paragraphs, prefix| {
            for (pi, paragraph) in paragraphs.iter().enumerate() {
                for (ri, run) in paragraph.runs.iter().enumerate() {
                    if let Some(Control::FieldBoundary { boundary: FieldBoundary::Start(s) }) =
                        run.content.as_control()
                    {
                        if s.unclosed || s.recovery.0.is_some() {
                            claims.push((
                                CorePath::new(si, format!("{prefix}[{pi}].runs[{ri}]")),
                                s.as_ref(),
                            ));
                        }
                    }
                }
            }
        });
        claims
    }
    /// Restore provenance on authored input only at the same Core address and
    /// ID as a source-backed base. No source-less constructor can grant it.
    pub fn restore_source_provenance(sections: &mut [Section], base: &[Section]) {
        let mut origins = Vec::new();
        for (si, section) in base.iter().enumerate() {
            walk_section(section, &mut |paragraphs, prefix| {
                for (pi, paragraph) in paragraphs.iter().enumerate() {
                    for (ri, r) in paragraph.runs.iter().enumerate() {
                        if let Some(Control::FieldBoundary { boundary: FieldBoundary::Start(s) }) =
                            r.content.as_control()
                        {
                            if let Some(origin) = &s.recovery.0 {
                                if s.unclosed
                                    && origin.id() == s.id
                                    && origin.path()
                                        == &CorePath::new(si, format!("{prefix}[{pi}].runs[{ri}]"))
                                {
                                    origins.push(origin.clone());
                                }
                            }
                        }
                    }
                }
            });
        }
        // Only a submitted unclosed flag requests recovery; closed authored
        // starts remain strict even if they share the base's location.
        let requested: HashSet<_> = Self::requested_unclosed(sections);
        origins.retain(|o| {
            requested.contains(&(
                o.path().section(),
                o.path().relative().to_owned(),
                o.id().to_owned(),
            ))
        });
        // 이것을 실패시키는 것: 다른 패키지의 제출 claim을 남겨 두기.
        for section in sections.iter_mut() {
            walk_section_mut(section, &mut |paragraphs, _| {
                for p in paragraphs {
                    for run in &mut p.runs {
                        if let RunContent::Control(c) = &mut run.content {
                            if let Control::FieldBoundary { boundary: FieldBoundary::Start(s) } =
                                c.as_mut()
                            {
                                s.recovery.0 = None;
                            }
                        }
                    }
                }
            });
        }
        // Clone source-backed evidence in one pass, retaining the same package Arc.
        // Format acceptance separately verifies it against the caller's base SourceMap.
        Self::attach_source_ranges(sections, &origins);
    }
    fn requested_unclosed(sections: &[Section]) -> HashSet<(usize, String, String)> {
        let mut requested = HashSet::new();
        for (si, section) in sections.iter().enumerate() {
            walk_section(section, &mut |ps, prefix| {
                for (pi, p) in ps.iter().enumerate() {
                    for (ri, r) in p.runs.iter().enumerate() {
                        if let Some(Control::FieldBoundary { boundary: FieldBoundary::Start(s) }) =
                            r.content.as_control()
                        {
                            if s.unclosed {
                                requested.insert((
                                    si,
                                    format!("{prefix}[{pi}].runs[{ri}]"),
                                    s.id.clone(),
                                ));
                            }
                        }
                    }
                }
            });
        }
        requested
    }
    /// Fields whose bodies contain this run or descendant slot.
    pub fn fields_at<'b>(
        &'b self,
        section: usize,
        path: &str,
    ) -> impl Iterator<Item = &'b FieldEntry<'a>> {
        let mut prefix = path;
        let mut positions = Vec::new();
        loop {
            #[cfg(test)]
            self.prefix_steps.set(self.prefix_steps.get() + 1);
            if let Some(p) = self.positions.get(&CorePath::new(section, prefix)) {
                positions.push(p);
            }
            let Some((parent, _)) = prefix.rsplit_once('.') else { break };
            prefix = parent;
        }
        self.fields.iter().filter(move |field| {
            self.protection_work.set(self.protection_work.get() + 1);
            positions.iter().any(|(flow, rank)| {
                field.section == section
                    && &field.flow == flow
                    && (field.range.contains(rank)
                        || (field.editability == FieldEditability::Unclosed
                            && field.begin == *rank))
            })
        })
    }
    /// True if a slot belongs to any field body.
    pub fn contains_body(&self, section: usize, path: &str) -> bool {
        // 이것을 실패시키는 것: 빈 contains_body의 prefix 순회 복원 (empty_inventory_skips_prefixes의 prefix_steps 계수).
        !self.fields.is_empty() && self.fields_at(section, path).next().is_some()
    }
    /// Common value-edit preflight, including ancestor runs.
    pub fn protects(&self, section: usize, path: &str) -> bool {
        // 이것을 실패시키는 것: 빈 inventory에서도 prefix 루프 실행 (empty_inventory_skips_prefixes).
        if self.fields.is_empty() {
            return false;
        }
        // An inner flow can lie in the body of an outer field.
        let mut prefix = path;
        loop {
            self.protection_work.set(self.protection_work.get() + 1);
            if let Some((flow, rank)) = self.positions.get(&CorePath::new(section, prefix)) {
                if self.protected.get(&(section, flow.clone())).is_some_and(|p| p[*rank]) {
                    return true;
                }
            }
            let Some((parent, _)) = prefix.rsplit_once('.') else {
                return false;
            };
            prefix = parent;
        }
    }
    /// Protect a paragraph containing a boundary, body, or nested field.
    pub fn protects_paragraph(&self, section: usize, flow: &str, paragraph: usize) -> bool {
        let prefix = format!("{flow}[{paragraph}].");
        self.fields.iter().any(|f| {
            f.section == section
                && (f.start.starts_with(&prefix)
                    || f.end.as_ref().is_some_and(|e| e.starts_with(&prefix)))
        }) || self.positions.keys().any(|p| {
            p.section() == section
                && p.relative().starts_with(&prefix)
                && self.contains_body(section, p.relative())
        }) || self.fields.iter().filter(|f| f.section == section).any(|f| {
            // Field-free child flows have no position entries. Borrow the already
            // stored body to check actual paragraph presence, including empty and
            // out-of-range paragraphs; an ancestor prefix alone is insufficient.
            // 이것을 실패시키는 것: 하위 일반 문단 보호 누락 또는 빈 문단도 보호.
            f.body()
                .iter()
                .chain((f.editability == FieldEditability::Unclosed).then(|| &f.runs[f.begin]))
                .any(|body| body_has_paragraph(body, &prefix))
        })
    }
    /// Protect gaps inside ranges. `None` is before a paragraph; `Some(run)`
    /// is before a run, including a paragraph's one-past-last run position.
    pub fn protects_insertion(
        &self,
        section: usize,
        flow: &str,
        paragraph: usize,
        run: Option<usize>,
    ) -> bool {
        let gap = (paragraph, run.unwrap_or(0));
        self.fields.iter().any(|f| {
            if f.section != section
                || f.flow != flow
                || f.end.is_none() && f.editability != FieldEditability::Unclosed
            {
                return false;
            }
            let position = |path: &str| -> Option<(usize, usize)> {
                let tail = path.strip_prefix(flow)?.strip_prefix('[')?;
                let (p, tail) = tail.split_once("].runs[")?;
                Some((p.parse().ok()?, tail.strip_suffix(']')?.parse().ok()?))
            };
            position(&f.start).is_some_and(|begin| begin < gap)
                && f.end.as_deref().and_then(position).is_none_or(|end| gap <= end)
        })
    }
    /// Clear editable scalar values for structural comparison after preflight.
    /// Protected fields remain intact, including computed display values.
    /// Over-budget input is a no-op; unrelated boundary diagnostics do not
    /// prevent clearing editable fields in independent flows.
    pub fn clear_editable_values(sections: &mut [Section], first_section: usize) {
        let inventory = FieldInventory::from_sections_at(sections, first_section);
        // 이것을 실패시키는 것: 모든 진단으로 확대하거나 깊이 관문 삭제 (clear_editable_values_*).
        if inventory.depth_exceeded {
            return;
        }
        let paths: HashSet<_> = inventory
            .fields()
            .iter()
            .filter(|f| f.editability() == FieldEditability::Editable)
            .flat_map(|f| f.body().iter().map(move |b| (f.section(), b.path().to_owned())))
            .collect();
        for (si, section) in sections.iter_mut().enumerate() {
            walk_section_mut(section, &mut |paragraphs, prefix| {
                for (pi, p) in paragraphs.iter_mut().enumerate() {
                    for (ri, r) in p.runs.iter_mut().enumerate() {
                        let path = format!("{prefix}[{pi}].runs[{ri}].control.field");
                        if paths.contains(&(si + first_section, path)) {
                            if let RunContent::Control(c) = &mut r.content {
                                if let Control::Field { display_text, .. } = c.as_mut() {
                                    display_text.clear();
                                }
                            }
                        }
                    }
                }
            });
        }
    }
    /// Replace one value after the same atomic preflight as a batch.
    pub fn replace_value(sections: &mut [Section], field: &FieldEntry<'_>, value: &str) -> bool {
        Self::replace_values(sections, &[(field, value)])
    }
    /// Replace values only if every discovery record still matches.
    pub fn replace_values(sections: &mut [Section], values: &[(&FieldEntry<'_>, &str)]) -> bool {
        let current = FieldInventory::from_sections(sections);
        if !current.diagnostics.is_empty() {
            return false;
        }
        let entries: HashMap<_, _> =
            current.fields.iter().map(|f| ((f.section, f.start.as_str()), f)).collect();
        let mut edits = HashMap::new();
        for (field, value) in values {
            if entries.get(&(field.section, field.start.as_str())).copied() != Some(*field)
                || field.editability != FieldEditability::Editable
            {
                return false;
            }
            let Some(body) = field.body().first() else {
                return false;
            };
            if edits.insert((field.section, body.path()), *value).is_some() {
                return false;
            }
        }
        drop(entries);
        drop(current);
        let mut replaced = 0;
        for (si, section) in sections.iter_mut().enumerate() {
            walk_section_mut(section, &mut |paragraphs, prefix| {
                for (pi, p) in paragraphs.iter_mut().enumerate() {
                    for (ri, run) in p.runs.iter_mut().enumerate() {
                        let suffix = match &run.content {
                            RunContent::Text(_) => "text",
                            RunContent::Control(_) => "control.field",
                            _ => continue,
                        };
                        let path = format!("{prefix}[{pi}].runs[{ri}].{suffix}");
                        let Some(value) = edits.get(&(si, path.as_str())) else {
                            continue;
                        };
                        match &mut run.content {
                            RunContent::Text(text) => {
                                *text = (*value).into();
                                replaced += 1;
                            }
                            RunContent::Control(c) => {
                                if let Control::Field { display_text, .. } = c.as_mut() {
                                    *display_text = (*value).into();
                                    replaced += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            });
        }
        replaced == edits.len()
    }
    fn flow(&mut self, section: usize, paragraphs: &'a [Paragraph], flow: &str) {
        // Shallow only: the walker must still descend into field-bearing children.
        // 이것을 실패시키는 것: 필드 없는 부모에서 자손 flow 하강까지 생략.
        if !paragraphs.iter().flat_map(|p| &p.runs).any(field_related) {
            return;
        }
        let runs: Vec<_> = paragraphs
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                p.runs
                    .iter()
                    .enumerate()
                    .map(move |(ri, r)| (pi, format!("{flow}[{pi}].runs[{ri}]"), r))
            })
            .collect();
        self.run_flow(section, runs, flow);
    }
    fn run_flow(&mut self, section: usize, runs: Vec<(usize, String, &'a crate::Run)>, flow: &str) {
        let mut stack = vec![(runs, flow.to_owned())];
        while let Some((runs, flow_name)) = stack.pop() {
            let flow = flow_name.as_str();
            let shared: Arc<[FieldBodyLocation<'a>]> =
                runs.iter().map(|(_, path, r)| body(section, path, &r.content)).collect();
            for (rank, (_, path, _)) in runs.iter().enumerate() {
                self.positions.insert(CorePath::new(section, path), (flow.into(), rank));
            }
            let first_field = self.fields.len();
            let first_diagnostic = self.diagnostics.len();
            let mut active: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            let mut seen = HashSet::new();
            for (i, (_, path, run)) in runs.iter().enumerate() {
                let Some(control) = run.content.as_control() else { continue };
                let Some(kind) = field_kind(control) else { continue };
                let mut entry = FieldEntry {
                    section,
                    kind: kind.into(),
                    name: None,
                    id: None,
                    flow: flow.into(),
                    start: path.clone(),
                    end: None,
                    runs: Arc::clone(&shared),
                    range: i..i,
                    begin: i,
                    finish: i,
                    hint: None,
                    editability: FieldEditability::Protected,
                };
                match control {
                    Control::FieldBoundary { boundary: FieldBoundary::Start(start) } => {
                        entry.kind = start.kind.clone();
                        entry.name = (!start.name.is_empty()).then(|| start.name.clone());
                        entry.id = Some(start.id.clone());
                        entry.hint = start.parameters.iter().find_map(|p| match p {
                            FieldParameter::String { name, value, .. } if name == "Direction" => {
                                Some(value.clone())
                            }
                            _ => None,
                        });
                        if !seen.insert(start.id.as_str()) || start.id.is_empty() {
                            self.invalid(section, path, "duplicate or empty field start ID");
                        }
                        active.insert(&start.id, (self.fields.len(), i));
                        self.fields.push(entry);
                    }
                    Control::FieldBoundary { boundary: FieldBoundary::End { begin_id, .. } } => {
                        if let Some((field, begin)) = active.remove(begin_id.as_str()) {
                            if matches!(runs[begin].2.content.as_control(), Some(Control::FieldBoundary { boundary: FieldBoundary::Start(s) }) if s.unclosed)
                            {
                                self.invalid(
                                    section,
                                    &runs[begin].1,
                                    "unclosed field start has an end",
                                );
                            }
                            let entry = &mut self.fields[field];
                            entry.end = Some(path.clone());
                            entry.range = begin + 1..i;
                            entry.begin = begin;
                            entry.finish = i;
                            if entry.kind == "CLICK_HERE" {
                                entry.editability = if runs[begin].0 == runs[i].0
                                    && entry.range.len() == 1
                                    && matches!(runs[begin + 1].2.content, RunContent::Text(_))
                                {
                                    FieldEditability::Editable
                                } else {
                                    FieldEditability::UnsupportedRange
                                };
                            }
                        } else {
                            self.invalid(
                                section,
                                path,
                                "field end has no preceding start in this flow",
                            );
                        }
                    }
                    Control::Field { field_type, name, hint_text, .. } => {
                        entry.name = name.clone();
                        entry.hint = hint_text.clone();
                        entry.range = i..i + 1;
                        if *field_type == FieldType::ClickHere {
                            entry.editability = FieldEditability::Editable;
                        }
                        self.fields.push(entry);
                    }
                    Control::Bookmark { name, .. } => {
                        entry.name = Some(name.clone());
                        self.fields.push(entry);
                    }
                    Control::InlinePageNumber { .. } | Control::AutoNumber { .. } => {
                        self.fields.push(entry);
                    }
                    _ => {
                        entry.range = i..i + 1;
                        self.fields.push(entry);
                    }
                }
            }
            for (id, (field, begin)) in active {
                self.fields[field].range = begin + 1..runs.len();
                self.fields[field].begin = begin;
                self.fields[field].finish = runs.len();
                let reason = format!("field start {id:?} has no end in this flow");
                if matches!(runs[begin].2.content.as_control(), Some(Control::FieldBoundary { boundary: FieldBoundary::Start(s) }) if s.unclosed && s.recovery.0.as_ref().is_some_and(|o| o.path().section() == section && o.path().relative() == runs[begin].1 && o.id() == s.id))
                {
                    self.fields[field].editability = FieldEditability::Unclosed;
                    self.warnings.push(ValidationError::InvalidFieldBoundary {
                        section_index: section,
                        path: runs[begin].1.clone(),
                        reason,
                    });
                } else {
                    self.invalid(section, &runs[begin].1, &reason);
                }
            }
            if self.diagnostics.len() != first_diagnostic {
                for field in &mut self.fields[first_field..] {
                    field.editability = FieldEditability::InvalidBoundary;
                }
            }
            // Anchors are an independent run flow; no temporary Paragraph or subtree copy.
            for (_, path, run) in runs.into_iter().rev() {
                if let Some(Control::Memo { anchor_runs, .. }) = run.content.as_control() {
                    // A Memo itself is field-related, so nested anchors still descend.
                    // 이것을 실패시키는 것: 일반 anchor 위치 저장 복원 (plain_anchor_has_no_positions).
                    if !anchor_runs.iter().any(field_related) {
                        continue;
                    }
                    let prefix = format!("{path}.control.memo.anchor");
                    let anchor = anchor_runs
                        .iter()
                        .enumerate()
                        .map(|(ri, r)| (0, format!("{prefix}[0].runs[{ri}]"), r))
                        .collect();
                    stack.push((anchor, prefix));
                }
            }
        }
    }
    fn invalid(&mut self, section: usize, path: &str, reason: &str) {
        self.diagnostics.push(ValidationError::InvalidFieldBoundary {
            section_index: section,
            path: path.into(),
            reason: reason.into(),
        });
    }
}
// One shallow classification for flow filtering and run_flow discovery.
// 이것을 실패시키는 것: Memo를 일반 control로 분류하여 필드와 anchor 하강 누락.
fn field_kind(control: &Control) -> Option<&'static str> {
    Some(match control {
        Control::FieldBoundary { .. } => "", // The start supplies the authored kind.
        Control::Field { field_type: FieldType::ClickHere, .. } => "CLICK_HERE",
        Control::Field { .. } | Control::UnknownSummary { .. } => "SUMMERY",
        Control::Hyperlink { .. } => "HYPERLINK",
        Control::CrossRef { .. } => "CROSSREF",
        Control::PathField { .. } => "PATH",
        Control::DateCodeField { .. } => "DATE",
        Control::Memo { .. } => "MEMO",
        Control::Bookmark { .. } => "BOOKMARK",
        Control::InlinePageNumber { .. } => "PAGE_NUM",
        Control::AutoNumber { .. } => "AUTO_NUMBER",
        _ => return None,
    })
}
fn field_related(run: &crate::Run) -> bool {
    run.content.as_control().and_then(field_kind).is_some()
}

fn body_has_paragraph(body: &FieldBodyLocation<'_>, prefix: &str) -> bool {
    // Fast rejection only; the walk below compares the complete paragraph prefix.
    if !prefix.strip_prefix(body.path()).is_some_and(|tail| tail.starts_with('.')) {
        return false;
    }
    let mut candidate = String::from(body.path());
    // Anchors are independent run flows, not part of visit_flows.
    if let Some(Control::Memo { anchor_runs, .. }) = body.content().as_control() {
        candidate.push_str(".control.memo.anchor[0].");
        if !anchor_runs.is_empty() && candidate.starts_with(prefix) {
            return true;
        }
    }
    walk_flow(Flow::Content(body.content()), body.path(), &mut |ps, path| {
        // Preserve container-prefix queries, reusing one buffer for all paragraphs.
        candidate.clear();
        candidate.push_str(path);
        let restore = candidate.len();
        ps.iter().enumerate().any(|(pi, p)| {
            if p.runs.is_empty() {
                return false;
            }
            candidate.truncate(restore);
            use std::fmt::Write;
            write!(candidate, "[{pi}].").expect("write to String");
            candidate.starts_with(prefix)
        })
    })
}

fn body<'a>(section: usize, path: &str, content: &'a RunContent) -> FieldBodyLocation<'a> {
    let suffix = match content {
        RunContent::Text(_) | RunContent::InlineText(_) => ".text",
        RunContent::Control(c) if matches!(c.as_ref(), Control::Field { .. }) => ".control.field",
        _ => "",
    };
    FieldBodyLocation { path: CorePath::new(section, format!("{path}{suffix}")), original: content }
}

fn visible_value(content: &RunContent) -> String {
    let mut out = String::new();
    let mut stack = vec![content];
    while let Some(content) = stack.pop() {
        match content.as_control() {
            Some(
                Control::Field { display_text, .. }
                | Control::CrossRef { display_text, .. }
                | Control::UnknownSummary { display_text, .. }
                | Control::PathField { display_text, .. }
                | Control::DateCodeField { display_text, .. },
            ) => out.push_str(display_text),
            Some(Control::Hyperlink { text, .. }) => out.push_str(text),
            Some(Control::Memo { anchor_runs, .. }) => {
                stack.extend(anchor_runs.iter().rev().map(|r| &r.content))
            }
            _ => {
                if let Some(text) = content.plain_text() {
                    out.push_str(&text);
                }
            }
        }
    }
    out
}

fn walk_section<'a>(section: &'a Section, f: &mut impl FnMut(&'a [Paragraph], &str)) {
    walk_paragraphs(&section.paragraphs, "paragraphs", f);
    for (tag, list) in [("header", &section.headers), ("footer", &section.footers)] {
        let len = list.len();
        for (i, hf) in list.iter().enumerate() {
            let prefix = if len == 1 { tag.into() } else { format!("{tag}s[{i}]") };
            walk_paragraphs(&hf.paragraphs, &format!("{prefix}.paragraphs"), f);
        }
    }
    for (i, mp) in section.master_pages.iter().flatten().enumerate() {
        walk_paragraphs(&mp.paragraphs, &format!("master_pages[{i}].paragraphs"), f);
    }
}
enum Flow<'a> {
    Paragraphs(&'a [Paragraph]),
    Control(&'a Control),
    Content(&'a RunContent),
}
fn walk_paragraphs<'a>(
    paragraphs: &'a [Paragraph],
    prefix: &str,
    f: &mut impl FnMut(&'a [Paragraph], &str),
) {
    walk_flow(Flow::Paragraphs(paragraphs), prefix, &mut |ps, path| {
        f(ps, path);
        false
    });
}
#[cfg(test)]
thread_local! {
    // Bytes actually appended from frames, isolated between parallel test threads.
    static WALK_PATH_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
// A true visitor result stops traversal before queuing that flow's descendants.
fn walk_flow<'a>(
    node: Flow<'a>,
    prefix: &str,
    f: &mut impl FnMut(&'a [Paragraph], &str) -> bool,
) -> bool {
    // Frames own only a relative suffix and the parent's cursor length.
    // 이것을 실패시키는 것: cursor 복구 길이를 한 바이트 줄이기.
    let mut path = String::new();
    let mut stack = vec![(node, prefix.to_owned(), 0)];
    let mut pending = Vec::new();
    while let Some((node, suffix, restore)) = stack.pop() {
        path.truncate(restore);
        path.push_str(&suffix);
        #[cfg(test)]
        WALK_PATH_BYTES.with(|work| work.set(work.get() + suffix.len()));
        match node {
            Flow::Paragraphs(ps) => {
                if f(ps, &path) {
                    return true;
                }
                queue_paragraphs(ps, &mut pending);
            }
            Flow::Control(c) => queue_control(c, &mut pending),
            Flow::Content(c) => queue_content(c, "", &mut pending),
        }
        let restore = path.len();
        stack.extend(pending.drain(..).rev().map(|(node, suffix)| (node, suffix, restore)));
    }
    false
}
fn queue_paragraphs<'a>(paragraphs: &'a [Paragraph], pending: &mut Vec<(Flow<'a>, String)>) {
    for (pi, p) in paragraphs.iter().enumerate() {
        for (ri, r) in p.runs.iter().enumerate() {
            let path = format!("[{pi}].runs[{ri}]");
            queue_content(&r.content, &path, pending);
        }
    }
}
fn queue_content<'a>(content: &'a RunContent, path: &str, pending: &mut Vec<(Flow<'a>, String)>) {
    match content {
        RunContent::Table(t) => {
            if let Some(c) = &t.caption {
                pending.push((
                    Flow::Paragraphs(&c.paragraphs),
                    format!("{path}.table.caption.paragraphs"),
                ));
            }
            for (row, tr) in t.rows.iter().enumerate() {
                for (cell, tc) in tr.cells.iter().enumerate() {
                    pending.push((
                        Flow::Paragraphs(&tc.paragraphs),
                        format!("{path}.table.rows[{row}].cells[{cell}].paragraphs"),
                    ));
                }
            }
        }
        RunContent::Image(image) => {
            if let Some(c) = &image.caption {
                pending.push((
                    Flow::Paragraphs(&c.paragraphs),
                    format!("{path}.image.caption.paragraphs"),
                ));
            }
        }
        RunContent::Control(c) => pending.push((Flow::Control(c), format!("{path}.control"))),
        _ => {}
    }
}
fn queue_control<'a>(c: &'a Control, pending: &mut Vec<(Flow<'a>, String)>) {
    let (tag, paragraphs, caption) = match c {
        Control::TextBox { paragraphs, caption, .. } => {
            ("textbox", Some(paragraphs), caption.as_ref())
        }
        Control::Ellipse { paragraphs, caption, .. } => {
            ("ellipse", Some(paragraphs), caption.as_ref())
        }
        Control::Polygon { paragraphs, caption, .. } => {
            ("polygon", Some(paragraphs), caption.as_ref())
        }
        Control::Footnote { paragraphs, .. } => ("footnote", Some(paragraphs), None),
        Control::Endnote { paragraphs, .. } => ("endnote", Some(paragraphs), None),
        Control::Memo { content, .. } => ("memo", Some(content), None),
        Control::FieldBoundary { boundary: FieldBoundary::Start(s) } => {
            ("annotation", s.annotation.as_ref(), None)
        }
        Control::Rect { caption, .. } => ("rect", None, caption.as_ref()),
        Control::Line { caption, .. } => ("line", None, caption.as_ref()),
        Control::Arc { caption, .. } => ("arc", None, caption.as_ref()),
        Control::Curve { caption, .. } => ("curve", None, caption.as_ref()),
        Control::ConnectLine { caption, .. } => ("connect_line", None, caption.as_ref()),
        Control::Group { children, .. } => {
            for (i, child) in children.iter().enumerate() {
                match child {
                    GroupChild::Shape(s) => {
                        pending.push((Flow::Control(s), format!(".group.children[{i}]")))
                    }
                    GroupChild::Picture(p) => {
                        if let Some(c) = &p.caption {
                            pending.push((
                                Flow::Paragraphs(&c.paragraphs),
                                format!(".group.children[{i}].caption.paragraphs"),
                            ));
                        }
                    }
                }
            }
            return;
        }
        _ => return,
    };
    if let Some(paragraphs) = paragraphs {
        pending.push((Flow::Paragraphs(paragraphs), format!(".{tag}.paragraphs")));
    }
    if let Some(caption) = caption {
        pending.push((Flow::Paragraphs(&caption.paragraphs), format!(".{tag}.caption.paragraphs")));
    }
}

pub(crate) fn walk_section_mut(section: &mut Section, f: &mut impl FnMut(&mut [Paragraph], &str)) {
    walk_paragraphs_mut(&mut section.paragraphs, "paragraphs", f);
    for (tag, list) in [("header", &mut section.headers), ("footer", &mut section.footers)] {
        let len = list.len();
        for (i, hf) in list.iter_mut().enumerate() {
            let prefix = if len == 1 { tag.into() } else { format!("{tag}s[{i}]") };
            walk_paragraphs_mut(&mut hf.paragraphs, &format!("{prefix}.paragraphs"), f);
        }
    }
    for (i, mp) in section.master_pages.iter_mut().flatten().enumerate() {
        walk_paragraphs_mut(&mut mp.paragraphs, &format!("master_pages[{i}].paragraphs"), f);
    }
}
enum FlowMut<'a> {
    Paragraphs(&'a mut [Paragraph]),
    Control(&'a mut Control),
}
fn walk_paragraphs_mut(
    paragraphs: &mut [Paragraph],
    prefix: &str,
    f: &mut impl FnMut(&mut [Paragraph], &str),
) {
    let mut stack = vec![(FlowMut::Paragraphs(paragraphs), prefix.to_owned())];
    let mut pending = Vec::new();
    while let Some((node, path)) = stack.pop() {
        match node {
            FlowMut::Paragraphs(ps) => {
                f(ps, &path);
                queue_paragraphs_mut(ps, &path, &mut pending);
            }
            FlowMut::Control(c) => queue_control_mut(c, &path, &mut pending),
        }
        stack.extend(pending.drain(..).rev());
    }
}
fn queue_paragraphs_mut<'a>(
    paragraphs: &'a mut [Paragraph],
    prefix: &str,
    pending: &mut Vec<(FlowMut<'a>, String)>,
) {
    for (pi, p) in paragraphs.iter_mut().enumerate() {
        for (ri, r) in p.runs.iter_mut().enumerate() {
            let path = format!("{prefix}[{pi}].runs[{ri}]");
            match &mut r.content {
                RunContent::Table(t) => {
                    if let Some(c) = &mut t.caption {
                        pending.push((
                            FlowMut::Paragraphs(&mut c.paragraphs),
                            format!("{path}.table.caption.paragraphs"),
                        ));
                    }
                    for (row, tr) in t.rows.iter_mut().enumerate() {
                        for (cell, tc) in tr.cells.iter_mut().enumerate() {
                            pending.push((
                                FlowMut::Paragraphs(&mut tc.paragraphs),
                                format!("{path}.table.rows[{row}].cells[{cell}].paragraphs"),
                            ));
                        }
                    }
                }
                RunContent::Image(image) => {
                    if let Some(c) = &mut image.caption {
                        pending.push((
                            FlowMut::Paragraphs(&mut c.paragraphs),
                            format!("{path}.image.caption.paragraphs"),
                        ));
                    }
                }
                RunContent::Control(c) => {
                    pending.push((FlowMut::Control(c), format!("{path}.control")))
                }
                _ => {}
            }
        }
    }
}
fn queue_control_mut<'a>(c: &'a mut Control, path: &str, pending: &mut Vec<(FlowMut<'a>, String)>) {
    let (tag, paragraphs, caption) = match c {
        Control::TextBox { paragraphs, caption, .. } => {
            ("textbox", Some(paragraphs), caption.as_mut())
        }
        Control::Ellipse { paragraphs, caption, .. } => {
            ("ellipse", Some(paragraphs), caption.as_mut())
        }
        Control::Polygon { paragraphs, caption, .. } => {
            ("polygon", Some(paragraphs), caption.as_mut())
        }
        Control::Footnote { paragraphs, .. } => ("footnote", Some(paragraphs), None),
        Control::Endnote { paragraphs, .. } => ("endnote", Some(paragraphs), None),
        Control::Memo { content, .. } => ("memo", Some(content), None),
        Control::FieldBoundary { boundary: FieldBoundary::Start(s) } => {
            ("annotation", s.annotation.as_mut(), None)
        }
        Control::Rect { caption, .. } => ("rect", None, caption.as_mut()),
        Control::Line { caption, .. } => ("line", None, caption.as_mut()),
        Control::Arc { caption, .. } => ("arc", None, caption.as_mut()),
        Control::Curve { caption, .. } => ("curve", None, caption.as_mut()),
        Control::ConnectLine { caption, .. } => ("connect_line", None, caption.as_mut()),
        Control::Group { children, .. } => {
            for (i, child) in children.iter_mut().enumerate() {
                match child {
                    GroupChild::Shape(s) => {
                        pending.push((FlowMut::Control(s), format!("{path}.group.children[{i}]")))
                    }
                    GroupChild::Picture(p) => {
                        if let Some(c) = &mut p.caption {
                            pending.push((
                                FlowMut::Paragraphs(&mut c.paragraphs),
                                format!("{path}.group.children[{i}].caption.paragraphs"),
                            ));
                        }
                    }
                }
            }
            return;
        }
        _ => return,
    };
    if let Some(paragraphs) = paragraphs {
        pending.push((FlowMut::Paragraphs(paragraphs), format!("{path}.{tag}.paragraphs")));
    }
    if let Some(caption) = caption {
        pending.push((
            FlowMut::Paragraphs(&mut caption.paragraphs),
            format!("{path}.{tag}.caption.paragraphs"),
        ));
    }
}

fn natural_path(path: &str) -> String {
    let mut result = String::new();
    let path = path
        .replace(".textbox.caption", ".textbox.zcaption")
        .replace(".ellipse.caption", ".ellipse.zcaption")
        .replace(".polygon.caption", ".polygon.zcaption");
    for part in path.split_inclusive('[') {
        if let Some((number, rest)) = part.split_once(']') {
            if let Ok(number) = number.parse::<usize>() {
                result.push_str(&format!("{number:020}]{rest}"));
                continue;
            }
        }
        result.push_str(part);
    }
    result
}

#[cfg(test)]
mod walker_tests {
    use super::*;
    use crate::Run;
    use hwpforge_foundation::CharShapeIndex;

    // 이것을 실패시키는 것: 필드 없는 flow의 주소 수집 복원 또는 빈 inventory의 조기 반환 삭제.
    #[test]
    fn empty_inventory_skips_prefixes() {
        let mut p = Paragraph::with_runs(
            vec![Run::text("leaf", CharShapeIndex::new(0))],
            hwpforge_foundation::ParaShapeIndex::new(0),
        );
        for _ in 0..1_000 {
            p = Paragraph::with_runs(
                vec![Run::control(
                    Control::text_box(
                        vec![p],
                        hwpforge_foundation::HwpUnit::ZERO,
                        hwpforge_foundation::HwpUnit::ZERO,
                    ),
                    CharShapeIndex::new(0),
                )],
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
        }
        let sections = [Section::with_paragraphs(vec![p], crate::PageSettings::a4())];
        let inventory = FieldInventory::from_sections(&sections);
        assert!(inventory.positions.is_empty());
        assert_eq!(inventory.stored_run_count(), 0);
        let path =
            format!("paragraphs[0]{}.text", ".runs[0].control.textbox.paragraphs[0]".repeat(1_000));
        let work = inventory.protection_work();
        assert!(!inventory.contains_body(0, &path));
        assert_eq!(inventory.prefix_steps.get(), 0);
        // Check that the counter observes real fields_at prefix lookups as well.
        assert!(inventory.fields_at(0, "paragraphs[0].runs[0].text").next().is_none());
        assert_eq!(inventory.prefix_steps.get(), 3);
        assert!(!inventory.protects(0, &path));
        assert_eq!(inventory.protection_work(), work);
    }

    // 이것을 실패시키는 것: frame suffix에 전체 조상 경로 재삽입.
    #[test]
    fn walker_path_copy_work_is_linear() {
        let mut p = Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0));
        const DEPTH: usize = 256;
        for _ in 0..DEPTH {
            p = Paragraph::with_runs(
                vec![Run::control(
                    Control::text_box(
                        vec![p],
                        hwpforge_foundation::HwpUnit::ZERO,
                        hwpforge_foundation::HwpUnit::ZERO,
                    ),
                    CharShapeIndex::new(0),
                )],
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
        }
        WALK_PATH_BYTES.with(|work| work.set(0));
        let mut visited = 0;
        walk_paragraphs(&[p], "paragraphs", &mut |_, _| visited += 1);
        assert_eq!(visited, DEPTH + 1);
        let copied = WALK_PATH_BYTES.with(|work| work.get());
        assert_eq!(
            copied,
            "paragraphs".len() + DEPTH * "[0].runs[0].control.textbox.paragraphs".len()
        );
    }

    // 이것을 실패시키는 것: 일치 뒤에도 보호 subtree의 하위 flow를 계속 순회.
    #[test]
    fn body_paragraph_search_stops_at_match() {
        let run = Run::control(
            Control::text_box(
                vec![Paragraph::with_runs(
                    vec![Run::control(
                        Control::text_box(
                            vec![],
                            hwpforge_foundation::HwpUnit::ZERO,
                            hwpforge_foundation::HwpUnit::ZERO,
                        ),
                        CharShapeIndex::new(0),
                    )],
                    hwpforge_foundation::ParaShapeIndex::new(0),
                )],
                hwpforge_foundation::HwpUnit::ZERO,
                hwpforge_foundation::HwpUnit::ZERO,
            ),
            CharShapeIndex::new(0),
        );
        let body = body(0, "paragraphs[0].runs[0]", &run.content);
        WALK_PATH_BYTES.with(|work| work.set(0));
        assert!(body_has_paragraph(&body, "paragraphs[0].runs[0].control.textbox.paragraphs[0]."));
        assert_eq!(
            WALK_PATH_BYTES.with(|work| work.get()),
            body.path().len() + ".control.textbox.paragraphs".len()
        );
    }

    // 이것을 실패시키는 것: 필드 없는 Memo anchor의 positions 저장 복원.
    #[test]
    fn plain_anchor_has_no_positions() {
        let p = Paragraph::with_runs(
            vec![Run::control(
                Control::memo_with_anchor(
                    vec![],
                    vec![Run::text("anchor", CharShapeIndex::new(0))],
                ),
                CharShapeIndex::new(0),
            )],
            hwpforge_foundation::ParaShapeIndex::new(0),
        );
        let sections = [Section::with_paragraphs(vec![p], crate::PageSettings::a4())];
        let inventory = FieldInventory::from_sections(&sections);
        assert_eq!(inventory.positions.len(), 1);
        assert_eq!(inventory.stored_run_count(), 1);
        assert!(inventory.protects_paragraph(0, "paragraphs[0].runs[0].control.memo.anchor", 0));
        assert!(!inventory.protects_paragraph(0, "paragraphs[0].runs[0].control.memo.anchor", 1));
    }

    // 이것을 실패시키는 것: unclosed 시작 run을 하위 문단 보호 검사에서 제외.
    #[test]
    fn unclosed_start_protects_annotation_paragraphs() {
        let (_, source) = crate::source_xml::XmlSource::read(
            "<sec><p><run><fieldBegin id=\"open\"/></run></p></sec>",
        )
        .unwrap();
        let mut start = crate::control::FieldStart {
            id: "open".into(),
            kind: "FORMULA".into(),
            unclosed: true,
            ..Default::default()
        };
        start.recovery.0 = source.unclosed_origin(
            4,
            CorePath::new(0, "paragraphs[0].runs[0]"),
            "Contents/section0.xml",
        );
        assert!(start.recovery.0.is_some());
        start.annotation = Some(vec![
            Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0)),
            Paragraph::with_runs(
                vec![Run::text("annotation", CharShapeIndex::new(0))],
                hwpforge_foundation::ParaShapeIndex::new(0),
            ),
        ]);
        let p = Paragraph::with_runs(
            vec![Run::control(
                Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                CharShapeIndex::new(0),
            )],
            hwpforge_foundation::ParaShapeIndex::new(0),
        );
        let sections = [Section::with_paragraphs(vec![p], crate::PageSettings::a4())];
        let inventory = FieldInventory::from_sections(&sections);
        assert_eq!(inventory.fields()[0].editability(), FieldEditability::Unclosed);
        let flow = "paragraphs[0].runs[0].control.annotation.paragraphs";
        assert!(!inventory.protects_paragraph(0, flow, 0));
        assert!(inventory.protects_paragraph(0, flow, 1));
        assert!(!inventory.protects_paragraph(0, flow, 2));
    }

    // 이것을 실패시키는 것: visible_value의 Memo 재귀 복원 또는 anchor 역순.
    #[test]
    fn deep_visible_memo_value() {
        crate::depth::tests::small_stack_child(
            "field_inventory::walker_tests::deep_visible_memo_value",
            "HWPFORGE_SMALL_STACK_VISIBLE",
            512 * 1024,
            || {
                let mut r = Run::text("last", CharShapeIndex::new(0));
                for _ in 0..100_000 {
                    r = Run::control(
                        Control::memo_with_anchor(vec![], vec![r]),
                        CharShapeIndex::new(0),
                    );
                }
                let r = Run::control(
                    Control::memo_with_anchor(
                        vec![],
                        vec![Run::text("first", CharShapeIndex::new(0)), r],
                    ),
                    CharShapeIndex::new(0),
                );
                assert!(visible_value(&r.content) == "firstlast");
            },
        );
    }
}
