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
    warnings: Vec<ValidationError>,
    positions: HashMap<CorePath, (String, usize)>,
    protected: HashMap<(usize, String), Vec<bool>>,
    protection_work: std::cell::Cell<usize>,
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
    /// read-only protection remain complete; editing still rejects diagnostics.
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
        self.fields_at(section, path).next().is_some()
    }
    /// Common value-edit preflight, including ancestor runs.
    pub fn protects(&self, section: usize, path: &str) -> bool {
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
    pub fn clear_editable_values(sections: &mut [Section], first_section: usize) {
        let inventory = FieldInventory::from_sections_at(sections, first_section);
        // Clearing over-budget drafts is now a no-op; previously it cleared editable values.
        if !inventory.diagnostics().is_empty() {
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
                let Some(control) = run.content.as_control() else {
                    continue;
                };
                let mut entry = FieldEntry {
                    section,
                    kind: String::new(),
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
                        entry.kind = if *field_type == FieldType::ClickHere {
                            "CLICK_HERE"
                        } else {
                            "SUMMERY"
                        }
                        .into();
                        entry.name = name.clone();
                        entry.hint = hint_text.clone();
                        entry.range = i..i + 1;
                        if *field_type == FieldType::ClickHere {
                            entry.editability = FieldEditability::Editable;
                        }
                        self.fields.push(entry);
                    }
                    Control::Hyperlink { .. }
                    | Control::CrossRef { .. }
                    | Control::UnknownSummary { .. }
                    | Control::PathField { .. }
                    | Control::DateCodeField { .. }
                    | Control::Memo { .. } => {
                        entry.kind = match control {
                            Control::Hyperlink { .. } => "HYPERLINK",
                            Control::CrossRef { .. } => "CROSSREF",
                            Control::UnknownSummary { .. } => "SUMMERY",
                            Control::DateCodeField { .. } => "DATE",
                            Control::Memo { .. } => "MEMO",
                            _ => "PATH",
                        }
                        .into();
                        entry.range = i..i + 1;
                        self.fields.push(entry);
                    }
                    Control::Bookmark { name, .. } => {
                        entry.kind = "BOOKMARK".into();
                        entry.name = Some(name.clone());
                        self.fields.push(entry);
                    }
                    Control::InlinePageNumber { .. } | Control::AutoNumber { .. } => {
                        entry.kind = if matches!(control, Control::InlinePageNumber { .. }) {
                            "PAGE_NUM"
                        } else {
                            "AUTO_NUMBER"
                        }
                        .into();
                        self.fields.push(entry);
                    }
                    _ => {}
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
}
fn walk_paragraphs<'a>(
    paragraphs: &'a [Paragraph],
    prefix: &str,
    f: &mut impl FnMut(&'a [Paragraph], &str),
) {
    let mut stack = vec![(Flow::Paragraphs(paragraphs), prefix.to_owned())];
    let mut pending = Vec::new();
    while let Some((node, path)) = stack.pop() {
        match node {
            Flow::Paragraphs(ps) => {
                f(ps, &path);
                queue_paragraphs(ps, &path, &mut pending);
            }
            Flow::Control(c) => queue_control(c, &path, &mut pending),
        }
        stack.extend(pending.drain(..).rev());
    }
}
fn queue_paragraphs<'a>(
    paragraphs: &'a [Paragraph],
    prefix: &str,
    pending: &mut Vec<(Flow<'a>, String)>,
) {
    for (pi, p) in paragraphs.iter().enumerate() {
        for (ri, r) in p.runs.iter().enumerate() {
            let path = format!("{prefix}[{pi}].runs[{ri}]");
            match &r.content {
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
                RunContent::Control(c) => {
                    pending.push((Flow::Control(c), format!("{path}.control")))
                }
                _ => {}
            }
        }
    }
}
fn queue_control<'a>(c: &'a Control, path: &str, pending: &mut Vec<(Flow<'a>, String)>) {
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
                        pending.push((Flow::Control(s), format!("{path}.group.children[{i}]")))
                    }
                    GroupChild::Picture(p) => {
                        if let Some(c) = &p.caption {
                            pending.push((
                                Flow::Paragraphs(&c.paragraphs),
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
        pending.push((Flow::Paragraphs(paragraphs), format!("{path}.{tag}.paragraphs")));
    }
    if let Some(caption) = caption {
        pending.push((
            Flow::Paragraphs(&caption.paragraphs),
            format!("{path}.{tag}.caption.paragraphs"),
        ));
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

    // 이것을 실패시키는 것: visible_value의 Memo 재귀 복원 또는 anchor 역순.
    #[test]
    fn deep_visible_memo_value() {
        crate::depth::tests::small_stack_child(
            "field_inventory::walker_tests::deep_visible_memo_value",
            "HWPFORGE_W3_VISIBLE",
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
