//! Which paragraph line caches a text change makes stale,
//! shared by the two preserve-first text editors: `fill` and `patch`.
//!
//! The conservative rule drops the edited paragraph, its flow tail and host
//! chain. Proven single-line body and direct table-cell edits do not start
//! invalidation. Other body edits can retain an untouched body tail whose
//! line breaking inputs stay fixed: Hancom restacks its vertical positions.
//! Re-encoding callers keep the conservative rule below.

use std::ops::Range;

use crate::error::{HwpxError, HwpxResult};

/// The caches a set of changed paragraphs makes stale: for each, from that
/// paragraph to the end of its list, then from the list's host paragraph to
/// the end of the host's list, up to the section body. Each list keeps only
/// its earliest invalidation point, so every paragraph is examined once.
///
/// `caches[p]` are paragraph `p`'s own cache spans, `para_list[p]` the list
/// it sits in, `list_host[l]` the paragraph hosting list `l` (`None` for the
/// section body). `tick` counts the work (chain steps and paragraphs
/// examined) for the linearity tests.
///
/// 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · host 사슬 생략 · 목록 처음부터
/// (fill: `cell_fill_preserves_proven_lines_and_drops_overflow_tail`,
/// `direct_cells_keep_caches_and_other_containers_drop_tail_and_hosts`; patch:
/// `patch_drops_the_changed_flow_tail_and_host_chain`).
pub(crate) fn stale_caches(
    caches: &[Vec<Range<usize>>],
    para_list: &[usize],
    list_host: &[Option<usize>],
    paragraphs: impl IntoIterator<Item = usize>,
    tick: &mut impl FnMut(),
) -> Vec<Range<usize>> {
    stale_paragraphs(para_list, list_host, paragraphs, tick)
        .into_iter()
        .zip(caches)
        .filter(|(stale, _)| *stale)
        .flat_map(|(_, spans)| spans.iter().cloned())
        .collect()
}

/// Which paragraphs a set of changed `paragraphs` makes stale — the rule of
/// [`stale_caches`], one flag per paragraph. Also the rule of the
/// re-encoding editors' cache carry (`layout_carry`), which passes no hosts:
/// there a host holding an edit is itself changed.
pub(crate) fn stale_paragraphs(
    para_list: &[usize],
    list_host: &[Option<usize>],
    paragraphs: impl IntoIterator<Item = usize>,
    tick: &mut impl FnMut(),
) -> Vec<bool> {
    let mut from = vec![usize::MAX; list_host.len()];
    for paragraph in paragraphs {
        let mut at = Some(paragraph);
        while let Some(q) = at {
            tick();
            crate::work::tick();
            let list = para_list[q];
            from[list] = from[list].min(q);
            at = list_host[list];
        }
    }
    (0..para_list.len())
        .map(|j| {
            tick();
            crate::work::tick();
            j >= from[para_list[j]]
        })
        .collect()
}

/// Every `<hp:p>` of a section XML in start order, whatever container holds
/// it: its byte span, its own cache spans, the list it sits in (the element
/// whose direct children the paragraphs are) and the paragraph hosting that
/// list. Element names are compared without their prefix, as the decoder
/// does.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParagraphFlow {
    spans: Vec<Range<usize>>,
    caches: Vec<Vec<Range<usize>>>,
    para_list: Vec<usize>,
    list_host: Vec<Option<usize>>,
    /// The paragraph each paragraph sits inside (`None` in the section
    /// body) — unlike `list_host`, also across a header or footer.
    parent: Vec<Option<usize>>,
    /// Whether a paragraph directly owns an element outside the text-tail
    /// allowlist ([`text_tail_element`]), or holds the begin, body or end of
    /// a field other than ClickHere ([`Self::project_fields`]). Such a
    /// paragraph may change width or line breaks when pages move.
    has_disallowed_element: Vec<bool>,
    has_disallowed_self_element: Vec<bool>,
    // Re-encoded cell proofs run without prove_text_tail. Retain decoder-owned
    // ranges here so both consumers can read all original runs without a new scan.
    paragraph_starts: Vec<Range<usize>>,
    run_starts: Vec<Vec<Range<usize>>>,
    sublist_starts: Vec<Option<Range<usize>>>,
    /// Source ownership is proven only for body and direct top-level table cells.
    supported_owner: Vec<bool>,
    property_starts: Vec<[Range<usize>; 3]>,
    grid_starts: Vec<Range<usize>>,
    rebound_namespace: bool,
    /// Whether the section's gates or field ranges could not be placed on
    /// its source paragraphs ([`Self::prove_text_tail`]), so no paragraph
    /// can be proven plain.
    unproven_fields: bool,
    /// Whether the section holds an object that text wraps around and that
    /// is not placed as a character. Such an object stays where it is while
    /// a preserved tail moves under it, so its lines would be wrong.
    has_wrapping_object: bool,
    /// Whether some `secPr` of the section declares horizontal text.
    has_horizontal_section: bool,
    /// Whether some `secPr` or `colPr` of the section declares a setting
    /// under which preserved line breaks are not known to stay valid
    /// (another text direction, columns of different widths).
    incompatible_section: bool,
}

impl ParagraphFlow {
    /// Proven body/direct-cell edits start no invalidation. Other edits
    /// retain the legacy nested-edit dependencies. In a body
    /// tail, keep the plain prefix before the first disallowed owner. A
    /// wrapping object anywhere in the section, or incompatible section
    /// geometry, keeps the conservative rule for the whole section.
    /// keepWithNext, keepLines and widowOrphan only move vertical positions;
    /// they are not gates for untouched line breaking inputs.
    pub(crate) fn caches_to_drop_for_text_edits(
        &self,
        paragraphs: impl IntoIterator<Item = usize>,
        proven: &[bool],
    ) -> Vec<Range<usize>> {
        let mut changed = vec![false; self.spans.len()];
        for p in paragraphs {
            crate::work::tick();
            changed[p] = true;
        }
        let edited = || {
            changed.iter().enumerate().filter_map(|(p, &yes)| {
                crate::work::tick();
                yes.then_some(p)
            })
        };
        if !self.has_horizontal_section
            || self.incompatible_section
            || self.has_wrapping_object
            || self.unproven_fields
        {
            return self.stale_caches(edited());
        }
        // 이것을 실패시키는 것: 호출자의 proven을 소유 관계/자기 조건 없이 신뢰,
        // 성공한 한 줄 편집을 장벽 또는 내부 흐름 전파 시작점에 포함.
        let seeds: Vec<bool> = changed
            .iter()
            .enumerate()
            .map(|(p, &edited)| {
                edited && !(proven.get(p).copied().unwrap_or(false) && self.self_owned(p))
            })
            .collect();
        let starts = || seeds.iter().enumerate().filter_map(|(p, &yes)| yes.then_some(p));
        let stale = stale_paragraphs(&self.para_list, &self.list_host, starts(), &mut || {});
        let nested = stale_paragraphs(
            &self.para_list,
            &self.list_host,
            starts().filter(|&p| self.parent[p].is_some()),
            &mut || {},
        );
        let mut block_from = vec![usize::MAX; self.list_host.len()];
        for p in 0..self.spans.len() {
            crate::work::tick();
            if !stale[p] {
                continue;
            }
            let list = self.para_list[p];
            if !changed[p] && self.has_disallowed_element[p] {
                block_from[list] = block_from[list].min(p);
            }
        }
        let mut ranges = Vec::new();
        for p in 0..self.spans.len() {
            crate::work::tick();
            if seeds[p] || nested[p] || (stale[p] && p >= block_from[self.para_list[p]]) {
                ranges.extend(self.caches[p].iter().cloned());
            }
        }
        ranges
    }

    fn self_owned(&self, p: usize) -> bool {
        self.supported_owner.get(p).copied().unwrap_or(false)
            && self.has_disallowed_self_element.get(p) == Some(&false)
    }

    /// Supported source ownership and one direct cache array, using decoder ranges.
    pub(crate) fn self_candidate(&self, p: usize, span: &Range<usize>) -> Option<()> {
        if !self.self_owned(p) || self.spans.get(p)? != span || self.caches.get(p)?.len() != 1 {
            return None;
        }
        Some(())
    }

    pub(crate) fn section_evidence(&self, xml: &str) -> Option<crate::line_keep::SectionEvidence> {
        if self.rebound_namespace {
            return None;
        }
        let (root, namespaces) =
            crate::line_keep::Tag::opening_with_bindings(xml, &Default::default())?;
        if root.name != "sec" {
            return None;
        }
        let mut evidence = crate::line_keep::SectionEvidence { namespaces, ..Default::default() };
        for starts in &self.property_starts {
            for (range, name) in starts.iter().zip(["p", "run", "secPr"]) {
                let tag =
                    crate::line_keep::Tag::opening(xml.get(range.clone())?, &evidence.namespaces)?;
                if tag.name != name {
                    return None;
                }
            }
            evidence.properties += 1;
        }
        for range in &self.grid_starts {
            let tag =
                crate::line_keep::Tag::opening(xml.get(range.clone())?, &evidence.namespaces)?;
            evidence.record_grid(&tag);
        }
        Some(evidence)
    }

    pub(crate) fn references(
        &self,
        p: usize,
        xml: &str,
        namespaces: &crate::line_keep::NamespaceBindings,
    ) -> Option<crate::line_keep::References> {
        if let Some(range) = self.sublist_starts.get(p)? {
            let sub = crate::line_keep::Tag::opening(xml.get(range.clone())?, namespaces)?;
            if sub.name != "subList" {
                return None;
            }
        }
        crate::line_keep::references(
            xml.get(self.paragraph_starts.get(p)?.clone())?,
            self.run_starts
                .get(p)?
                .iter()
                .map(|r| xml.get(r.clone()))
                .collect::<Option<Vec<_>>>()?,
            namespaces,
        )
    }

    pub(crate) fn paragraph_count(&self) -> usize {
        self.spans.len()
    }

    /// Build from the decoder's original-element records, without reading XML.
    /// The text-tail gates are not built here: until
    /// [`Self::prove_text_tail`] runs no section is known to be horizontal,
    /// so [`Self::caches_to_drop_for_text_edits`] keeps the conservative rule.
    pub(crate) fn from_source(source: &hwpforge_core::source_xml::XmlSource) -> Self {
        let mut flow = Self::default();
        let mut nearest = vec![None; source.elements().len()];
        let mut hosts = nearest.clone();
        let mut lists = std::collections::HashMap::new();
        for (id, element) in source.elements().iter().enumerate().skip(1) {
            let parent = element.parent();
            nearest[id] = nearest[parent];
            hosts[id] = hosts[parent];
            match element.name() {
                "header" | "footer" => hosts[id] = None,
                "p" => {
                    let list = *lists.entry(parent).or_insert_with(|| {
                        flow.list_host.push(hosts[parent]);
                        flow.list_host.len() - 1
                    });
                    let paragraph = flow.spans.len();
                    flow.spans.push(element.outer());
                    flow.paragraph_starts.push(start_range(element));
                    flow.run_starts.push(Vec::new());
                    flow.sublist_starts.push(
                        (source.elements()[parent].name() == "subList")
                            .then(|| start_range(&source.elements()[parent])),
                    );
                    flow.caches.push(Vec::new());
                    flow.para_list.push(list);
                    flow.parent.push(nearest[parent]);
                    nearest[id] = Some(paragraph);
                    hosts[id] = Some(paragraph);
                }
                "run" if source.elements()[parent].name() == "p" => {
                    if let Some(paragraph) = nearest[parent] {
                        flow.run_starts[paragraph].push(start_range(element));
                    }
                }
                "linesegarray" if source.elements()[parent].name() == "p" => {
                    if let Some(paragraph) = nearest[parent] {
                        flow.caches[paragraph].push(element.outer());
                    }
                }
                _ => {}
            }
        }
        flow
    }

    /// Build the text-tail gates for the preserving editors: scan `source`
    /// (a fresh read of `xml`, the section text the flow was built from),
    /// then place the field ranges of `inventory` ([`Self::project_fields`]).
    /// The flow is proven only when both succeed.
    pub(crate) fn prove_text_tail(
        &mut self,
        source: &hwpforge_core::source_xml::XmlSource,
        xml: &str,
        inventory: &hwpforge_core::field_inventory::FieldInventory<'_>,
        run_start: impl Fn(&str) -> Option<usize>,
    ) {
        self.unproven_fields = true;
        if self.scan_text_tail(source, xml).is_some() {
            self.project_fields(inventory, run_start);
        }
    }

    /// The per-element part of the gates: the text-tail allowlist per owner
    /// paragraph, section direction and columns, and objects text wraps
    /// around. Each start tag is read at most once. `None` (fail closed) when
    /// `source` does not match the flow's paragraphs or a start tag cannot be
    /// read.
    fn scan_text_tail(
        &mut self,
        source: &hwpforge_core::source_xml::XmlSource,
        xml: &str,
    ) -> Option<()> {
        let elements = source.elements();
        self.has_disallowed_element = vec![false; self.spans.len()];
        self.has_disallowed_self_element = vec![false; self.spans.len()];
        self.supported_owner = vec![false; self.spans.len()];
        let mut direct_cell_list = vec![false; elements.len()];
        let mut cell_sublist_count = vec![0; elements.len()];
        self.property_starts.clear();
        self.grid_starts.clear();
        self.rebound_namespace = false;
        let mut in_section = vec![false; elements.len()];
        let mut section_property = vec![false; elements.len()];
        let mut click_metadata = vec![false; elements.len()];
        self.has_wrapping_object = false;
        self.has_horizontal_section = false;
        self.incompatible_section = false;
        let mut nearest = vec![None; elements.len()];
        let mut paragraphs = 0;
        for (id, element) in elements.iter().enumerate().skip(1) {
            crate::work::tick();
            let parent = element.parent();
            nearest[id] = nearest[parent];
            let name = element.name();
            // Deliberately conservative: this cheap prefilter also closes the proof
            // when an attribute value contains "xmlns". Resolving every opening tag
            // here would make non-candidates pay for namespace parsing.
            if parent != 0 && xml.get(start_range(element))?.contains("xmlns") {
                self.rebound_namespace = true;
            }
            in_section[id] = in_section[parent] || name == "secPr";
            click_metadata[id] = click_metadata[parent];
            if name == "tc" {
                // 이것을 실패시키는 것: subList마다 같은 칸의 자식을 다시 세기.
                cell_sublist_count[id] = element
                    .children()
                    .iter()
                    .filter(|&&child| {
                        crate::work::tick();
                        elements[child].name() == "subList"
                    })
                    .count();
            }
            direct_cell_list[id] =
                direct_cell_sublist(element, elements, xml, &cell_sublist_count)?;
            if name == "p" {
                if self.spans.get(paragraphs) != Some(&element.outer()) {
                    return None;
                }
                self.supported_owner[paragraphs] = (elements[parent].name() == "sec"
                    && elements[parent].parent() == 0)
                    || direct_cell_list[parent];
                nearest[id] = Some(paragraphs);
                paragraphs += 1;
            }
            // Section-level gates come first: they hold whichever paragraph,
            // header or footer owns the element, or none.
            match name {
                "secPr" => {
                    let start = source_start(xml, element)?;
                    let horizontal =
                        attribute(&start, "textDirection").as_deref() == Some("HORIZONTAL");
                    self.has_horizontal_section |= horizontal;
                    self.incompatible_section |= !horizontal;
                    let run = &elements[parent];
                    let paragraph = &elements[run.parent()];
                    if run.name() == "run"
                        && paragraph.name() == "p"
                        && elements[paragraph.parent()].parent() == 0
                    {
                        section_property[id] = true;
                        self.property_starts.push([
                            start_range(paragraph),
                            start_range(run),
                            start_range(element),
                        ]);
                    }
                }
                "grid" if section_property[parent] => {
                    self.grid_starts.push(start_range(element));
                }
                "colPr" => {
                    let start = source_start(xml, element)?;
                    self.incompatible_section |= !(attribute(&start, "colCount").as_deref()
                        == Some("1")
                        || attribute(&start, "sameSz").as_deref() == Some("1"));
                }
                // A `pos` is judged with the object that owns it (below); one
                // directly under the document has no owner to judge it by.
                "pos" if parent == 0 => return None,
                _ => {}
            }
            // An object, or any element that owns a `pos`, is placed by its
            // `pos` children: their `treatAsChar` and its own `textWrap`
            // (missing counts as wrapping and floating). An object without
            // its own `pos` has no proof of being inline, so it counts as
            // floating unless its wrap style leaves the lines alone. A
            // group's children carry no `pos`: their container's judges them.
            let mut placed = element.children().iter().filter(|&&c| elements[c].name() == "pos");
            let first_pos = placed.next();
            if first_pos.is_some() || object_element(name) {
                let wrap = attribute(&source_start(xml, element)?, "textWrap");
                match first_pos {
                    Some(&first) => {
                        let wraps =
                            matches!(wrap.as_deref(), None | Some("SQUARE" | "TIGHT" | "THROUGH"));
                        for &pos in std::iter::once(&first).chain(placed) {
                            let pos = source_start(xml, &elements[pos])?;
                            let inline = attribute(&pos, "treatAsChar").as_deref() == Some("1");
                            self.has_wrapping_object |= wraps && !inline;
                        }
                    }
                    None if elements[parent].name() != "container" => {
                        self.has_wrapping_object |= !matches!(
                            wrap.as_deref(),
                            Some("TOP_AND_BOTTOM" | "BEHIND_TEXT" | "IN_FRONT_OF_TEXT")
                        );
                    }
                    None => {}
                }
            }
            // Only the innermost owner receives the flag. Containers already
            // flag their host; descendant aggregation would mix independent flows.
            let Some(owner) = nearest[id] else { continue };
            if name == "fieldBegin" {
                click_metadata[id] = attribute(&source_start(xml, element)?, "type").as_deref()
                    == Some("CLICK_HERE");
            }
            let disallowed =
                !text_tail_element(name) || (name == "fieldBegin" && !click_metadata[id]);
            self.has_disallowed_element[owner] |= disallowed;
            let parent_name = elements[parent].name();
            let allowed = in_section[id]
                || match name {
                    "p" => true,
                    "run" | "linesegarray" => parent_name == "p",
                    "t" | "ctrl" => parent_name == "run",
                    "lineseg" => parent_name == "linesegarray",
                    "colPr" | "fieldEnd" => parent_name == "ctrl",
                    "fieldBegin" => parent_name == "ctrl" && click_metadata[id],
                    "parameters" => parent_name == "fieldBegin" && click_metadata[id],
                    "stringParam" | "integerParam" | "booleanParam" => {
                        parent_name == "parameters" && click_metadata[id]
                    }
                    _ => false,
                };
            self.has_disallowed_self_element[owner] |= !allowed;
        }
        (paragraphs == self.spans.len()).then_some(())
    }

    /// Marks every paragraph covered by non-ClickHere fields for tail removal,
    /// and by any field without an editable value for self-cache removal.
    /// Uses begin, body and end ranges from the section's
    /// `inventory`. `run_start` gives a run path's first source byte.
    /// A diagnostic or recovered warning leaves the section unproven.
    /// An unlocated range does too, except for bookmarks and ClickHere:
    /// their unknown coverage blocks only self-cache preservation throughout
    /// the section, leaving the existing tail proof intact.
    fn project_fields(
        &mut self,
        inventory: &hwpforge_core::field_inventory::FieldInventory<'_>,
        run_start: impl Fn(&str) -> Option<usize>,
    ) {
        self.unproven_fields = true;
        if !inventory.diagnostics().is_empty() || !inventory.warnings().is_empty() {
            return;
        }
        let mut opens = vec![0usize; self.spans.len()];
        let mut closes = opens.clone();
        let mut self_opens = opens.clone();
        let mut self_closes = opens.clone();
        let mut unlocated_self_field = false;
        for field in inventory.fields() {
            crate::work::tick();
            // Only editable ClickHere values have complete patchable text slots.
            // Other ranges may hide visible text from the width calculation.
            if field.kind() == "CLICK_HERE"
                && field.editability() == hwpforge_core::field_inventory::FieldEditability::Editable
            {
                continue;
            }
            let at = |path: &str| run_start(path).and_then(|offset| self.paragraph_at(offset));
            let begin = at(field.start());
            let end = field.end().as_deref().map_or(begin, at);
            let self_only = matches!(field.kind(), "BOOKMARK" | "CLICK_HERE");
            match (begin, end) {
                (Some(b), Some(e)) if b <= e && self.para_list[b] == self.para_list[e] => {
                    self_opens[b] += 1;
                    self_closes[e] += 1;
                    if !self_only {
                        opens[b] += 1;
                        closes[e] += 1;
                    }
                }
                _ if self_only => unlocated_self_field = true,
                _ => return,
            }
        }
        // Paragraphs of one list rise in index order, so a running count per
        // list covers every paragraph from a begin to its end.
        let mut running = vec![0usize; self.list_host.len()];
        let mut self_running = running.clone();
        for p in 0..self.spans.len() {
            crate::work::tick();
            let list = self.para_list[p];
            running[list] += opens[p];
            self.has_disallowed_element[p] |= running[list] > 0;
            running[list] -= closes[p];
            self_running[list] += self_opens[p];
            self.has_disallowed_self_element[p] |= unlocated_self_field || self_running[list] > 0;
            self_running[list] -= self_closes[p];
        }
        self.unproven_fields = false;
    }

    /// The innermost paragraph whose span holds byte `offset`.
    ///
    /// The last paragraph to start at or before `offset` is either it or
    /// nested in it (every paragraph still open at `offset` started before
    /// that one and so encloses it), so the walk only climbs parents.
    pub(crate) fn paragraph_at(&self, offset: usize) -> Option<usize> {
        let last = self.spans.partition_point(|span| span.start <= offset).checked_sub(1)?;
        let mut at = Some(last);
        while let Some(p) = at {
            crate::work::tick();
            if self.spans[p].contains(&offset) {
                return Some(p);
            }
            at = self.parent[p];
        }
        None
    }

    /// Direct caches of the original paragraph. Structural edits keep their
    /// established body-only policy while sharing the complete flow ledger.
    pub(crate) fn paragraph_caches(&self, offset: usize) -> Option<&[Range<usize>]> {
        let p = self.paragraph_at(offset)?;
        (self.spans[p].start == offset).then_some(self.caches[p].as_slice())
    }

    /// The caches the changed `paragraphs` make stale ([`stale_caches`]).
    pub(crate) fn stale_caches(
        &self,
        paragraphs: impl IntoIterator<Item = usize>,
    ) -> Vec<Range<usize>> {
        stale_caches(&self.caches, &self.para_list, &self.list_host, paragraphs, &mut || {})
    }
}

// 이것을 실패시키는 것: tbl 조상만으로 승인, 중복/방향 누락 subList 승인.
fn direct_cell_sublist(
    element: &hwpforge_core::source_xml::XmlElement,
    elements: &[hwpforge_core::source_xml::XmlElement],
    xml: &str,
    cell_sublist_count: &[usize],
) -> Option<bool> {
    if element.name() != "subList" {
        return Some(false);
    }
    let parent = element.parent();
    let mut ancestor = parent;
    let direct = ["tc", "tr", "tbl", "run", "p", "sec"].iter().all(|name| {
        let element = &elements[ancestor];
        ancestor = element.parent();
        element.name() == *name
    }) && ancestor == 0;
    if direct && cell_sublist_count[parent] == 1 {
        let tag = source_start(xml, element)?;
        return Some(crate::line_keep::cell_flow_allows(
            attribute(&tag, "textDirection").as_deref(),
            attribute(&tag, "lineWrap").as_deref(),
        ));
    }
    Some(false)
}

fn start_range(element: &hwpforge_core::source_xml::XmlElement) -> Range<usize> {
    let outer = element.outer();
    outer.start..element.inner().map_or(outer.end, |inner| inner.start)
}

/// The source ledger already records the end of the opening tag, including
/// quoted `>` and either quote style. No tag scanner or XML reader is needed.
/// `None` when the recorded range is not a start tag of `xml` (for example
/// the empty range of the document's virtual root). Counts the bytes read.
pub(crate) fn source_start<'a>(
    xml: &'a str,
    element: &hwpforge_core::source_xml::XmlElement,
) -> Option<quick_xml::events::BytesStart<'a>> {
    let outer = element.outer();
    let end = element.inner().map_or(outer.end, |inner| inner.start);
    let content = xml.get(outer.start.checked_add(1)?..end.checked_sub(1)?)?;
    let content = content.trim_end_matches('/');
    let name = element.qualified_name().len();
    crate::work::tick_n(content.len());
    (name <= content.len() && content.is_char_boundary(name))
        .then(|| quick_xml::events::BytesStart::from_content(content, name))
}

/// The normalized value of attribute `key`, `None` when absent or unreadable.
fn attribute(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start.attributes().flatten().find(|a| a.key.as_ref() == key).and_then(|a| {
        a.normalized_value(quick_xml::XmlVersion::Explicit1_0).ok().map(|v| v.into_owned())
    })
}

/// Objects placed by an `hp:pos` child: shapes, tables, pictures, equations,
/// OLE, charts, text art, video and form controls.
fn object_element(name: &str) -> bool {
    matches!(
        name,
        "tbl"
            | "pic"
            | "rect"
            | "ellipse"
            | "line"
            | "arc"
            | "polygon"
            | "curve"
            | "connectLine"
            | "container"
            | "equation"
            | "ole"
            | "chart"
            | "textart"
            | "video"
            | "btn"
            | "radioBtn"
            | "checkBtn"
            | "comboBox"
            | "listBox"
            | "edit"
            | "scrollBar"
    )
}

/// Elements a preserved tail paragraph may hold: text, line caches, breaks,
/// marks and the parts of a field (`fieldBegin` is further limited to
/// ClickHere in [`ParagraphFlow::scan_text_tail`]).
fn text_tail_element(name: &str) -> bool {
    matches!(
        name,
        "p" | "run"
            | "t"
            | "linesegarray"
            | "lineseg"
            | "ctrl"
            | "fieldBegin"
            | "fieldEnd"
            | "parameters"
            | "stringParam"
            | "integerParam"
            | "booleanParam"
            | "tab"
            | "lineBreak"
            | "nbSpace"
            | "fwSpace"
            | "markpenBegin"
            | "markpenEnd"
            | "titleMark"
            | "bookmark"
    )
}

/// Writes `xml` with each `(range, replacement)` applied, in one pass.
///
/// # Errors
///
/// [`HwpxError::InvalidStructure`] when two ranges overlap or one falls
/// outside `xml` — the edits would not mean what they say.
pub(crate) fn splice(
    xml: &str,
    mut replacements: Vec<(Range<usize>, String)>,
) -> HwpxResult<String> {
    replacements.sort_unstable_by_key(|(range, _)| (range.start, range.end));
    // One unit per KiB written, so writing the part once per range shows.
    crate::work::tick_n(xml.len() / 1024);
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0;
    let mut previous = None;
    for (range, text) in replacements {
        crate::work::tick();
        // 이것을 실패시키는 것: 같은 위치의 빈 삽입 범위 중복 허용.
        if previous.as_ref() == Some(&range)
            || range.start < cursor
            || range.end < range.start
            || !xml.is_char_boundary(range.start)
            || !xml.is_char_boundary(range.end)
        {
            return Err(HwpxError::InvalidStructure {
                detail: format!("section edit spans overlap or leave the part at {range:?}"),
            });
        }
        out.push_str(&xml[cursor..range.start]);
        out.push_str(&text);
        cursor = range.end;
        previous = Some(range);
    }
    out.push_str(&xml[cursor..]);
    Ok(out)
}

#[cfg(test)]
#[path = "cache_flow/text_edits_tests.rs"]
mod text_edits_tests;

/// Compare final bytes with the exact declared replacements and untouched gaps.
pub(crate) fn verify_replacements(
    original: &str,
    output: &str,
    mut allowed: Vec<(Range<usize>, String)>,
) -> HwpxResult<()> {
    let mismatch = || HwpxError::InvalidStructure {
        detail: "bytes differ outside declared replacements".into(),
    };
    allowed.sort_unstable_by_key(|(range, _)| (range.start, range.end));
    let (mut source_at, mut output_at) = (0, 0);
    for (range, replacement) in allowed {
        let gap = original.get(source_at..range.start).ok_or_else(mismatch)?;
        for expected in [gap, replacement.as_str()] {
            // 이것을 실패시키는 것: 허용 구간 밖 같은 길이 변조 또는 추가 치환 허용.
            if output.as_bytes().get(output_at..output_at + expected.len())
                != Some(expected.as_bytes())
            {
                return Err(mismatch());
            }
            output_at += expected.len();
        }
        source_at = range.end;
    }
    // 이것을 실패시키는 것: 마지막 허용 구간 뒤의 변조/추가 바이트 무시.
    if original.get(source_at..) != output.get(output_at..) {
        return Err(mismatch());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Cache marker: [`add_caches`] puts a cache with `vertpos` = 9000 + (the
    //! order the paragraph closes in) before every `</hp:p>`; inner paragraphs
    //! close first, so each number names one paragraph.

    use crate::patch::RawPackage;
    use crate::{HwpxDecoder, HwpxPatcher};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::run::{Run, RunContent};
    use hwpforge_core::section::{HeaderFooter, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{PageSettings, Paragraph};
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn encode(section: Section) -> Vec<u8> {
        let mut doc = Document::new();
        doc.add_section(section);
        let styles = crate::style_store_for_preset("default").expect("preset");
        crate::HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode")
    }

    fn section_xml(bytes: &[u8]) -> String {
        RawPackage::read(bytes)
            .expect("read")
            .read_text_entry("Contents/section0.xml")
            .expect("xml")
    }

    fn with_section_xml(bytes: &[u8], xml: String) -> Vec<u8> {
        let mut pkg = RawPackage::read(bytes).expect("read");
        pkg.replace_text_entry("Contents/section0.xml", xml);
        pkg.write().expect("write")
    }

    fn add_caches(xml: &str) -> String {
        let closes = xml.matches("</hp:p>").count();
        let mut out = String::with_capacity(xml.len());
        for (k, piece) in xml.split("</hp:p>").enumerate() {
            if k > 0 {
                out.push_str("</hp:p>");
            }
            out.push_str(piece);
            if k < closes {
                out.push_str(&format!(
                    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" \
                     textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                     horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray>",
                    9000 + k
                ));
            }
        }
        out
    }

    fn caches(xml: &str) -> Vec<usize> {
        (0..64).filter(|k| xml.contains(&format!("vertpos=\"{}\"", 9000 + k))).collect()
    }

    /// Patches section 0 of `bytes` after `edit` changes its decoded section.
    fn patch(bytes: &[u8], edit: impl FnOnce(&mut Section)) -> Vec<u8> {
        let decoded = HwpxDecoder::decode(bytes).expect("decode");
        let base = decoded.document.sections()[0].clone();
        let preservation =
            HwpxPatcher::export_section_preservation(bytes, 0, &base).expect("preservation");
        let mut replacement = base;
        edit(&mut replacement);
        HwpxPatcher::patch_section_preserving(bytes, 0, &replacement, None, Some(&preservation))
            .expect("patch")
    }

    fn set_text(paragraph: &mut Paragraph, text: &str) {
        paragraph.runs[0].content = RunContent::Text(text.into());
    }

    /// Counterexample: a body paragraph "ABCDEFGHIJKLMNOPQRST"
    /// with a line at 10, patched to "A", kept `textpos="10"` and no warning;
    /// the re-decode then dropped the cache as out of range.
    /// 이것을 실패시키는 것: patch 의 캐시 제거 삭제 또는 기존 꼬리 제거 호출.
    #[test]
    fn a_shortened_patch_drops_the_stale_line_start() {
        let bytes = encode(Section::with_paragraphs(
            vec![para("앞"), para("ABCDEFGHIJKLMNOPQRST"), para("뒤")],
            PageSettings::a4(),
        ));
        // Close order: 앞 0, the long paragraph 1, 뒤 2. The long one gets a
        // second line at 10 (the first paragraph also holds the section
        // controls, so it is not used).
        let xml = add_caches(&section_xml(&bytes));
        let at = xml.match_indices("</hp:linesegarray>").nth(1).expect("second cache").0;
        let xml = format!(
            "{}<hp:lineseg textpos=\"10\" vertpos=\"1600\" vertsize=\"1000\" \
             textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
             horzsize=\"42520\" flags=\"393216\"/>{}",
            &xml[..at],
            &xml[at..]
        );
        let bytes = with_section_xml(&bytes, xml);
        let warnings = HwpxDecoder::decode(&bytes).expect("decode").warnings;
        assert!(warnings.is_empty(), "{warnings:?}");
        let out = patch(&bytes, |s| set_text(&mut s.paragraphs[1], "A"));
        let out_xml = section_xml(&out);
        assert!(!out_xml.contains("textpos=\"10\""), "{out_xml}");
        assert_eq!(caches(&out_xml), vec![0, 2], "plain body tail stays verbatim");
        let decoded = HwpxDecoder::decode(&out).expect("decode");
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
    }

    /// A cell edit drops its paragraph, the cell's later paragraph, the host
    /// paragraph and the body paragraph after it. A header edit drops its own
    /// flow only: a header is laid out apart from the body, so the paragraph
    /// holding its control keeps its lines.
    /// 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · host 사슬 생략 · 목록
    /// 처음부터 · 바뀐 글자의 문단을 바깥 문단으로 잡기.
    #[test]
    fn patch_drops_the_changed_flow_tail_and_host_chain() {
        let width = HwpUnit::new(4000).expect("width");
        let cell = TableCell::new(vec![para("c0"), para("mid"), para("c2")], width);
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(
            Table::new(vec![TableRow::new(vec![cell])]),
            CharShapeIndex::new(0),
        ));
        let mut section =
            Section::with_paragraphs(vec![para("앞"), host, para("뒤")], PageSettings::a4());
        section.headers.push(HeaderFooter::new(vec![para("h0"), para("h1")], ApplyPageType::Both));
        let bytes = encode(section);
        let xml = section_xml(&bytes);
        let bytes = with_section_xml(&bytes, add_caches(&xml));
        let all = caches(&section_xml(&bytes));
        assert_eq!(all.len(), 8, "{all:?}");

        // Close order: h0 0, h1 1, 앞 2, c0 3, mid 4, c2 5, host 6, 뒤 7.
        let cell_edit = |s: &mut Section| {
            let RunContent::Table(table) = &mut s.paragraphs[1].runs[0].content else {
                panic!("table host");
            };
            set_text(&mut table.rows[0].cells[0].paragraphs[1], "많이 길어진 칸");
        };
        assert_eq!(caches(&section_xml(&patch(&bytes, cell_edit))), vec![0, 1, 2, 3]);
        let header_edit = |s: &mut Section| {
            set_text(&mut s.headers[0].paragraphs[0], "H");
        };
        assert_eq!(caches(&section_xml(&patch(&bytes, header_edit))), vec![2, 3, 4, 5, 6, 7]);
        // The body text after the header control belongs to the body
        // paragraph, found through its parent, not the header's flow.
        // 이것을 실패시키는 것: `paragraph_at` 이 부모 대신 host 사슬을 타기
        // (머리말 목록의 host 가 없어 문단을 못 찾는다).
        let body_edit = |s: &mut Section| set_text(&mut s.paragraphs[0], "앞앞");
        assert_eq!(caches(&section_xml(&patch(&bytes, body_edit))), vec![0, 1, 3, 4, 5]);
    }

    /// 이것을 실패시키는 것: 겹침 검사 삭제(뒤 교체가 앞 교체 안을 다시 쓴다).
    #[test]
    fn splice_refuses_overlapping_spans() {
        let ok = super::splice("abcdef", vec![(3..4, "Y".into()), (1..2, "X".into())]);
        assert_eq!(ok.expect("disjoint"), "aXcYef");
        assert!(super::splice("abcdef", vec![(1..3, "X".into()), (2..4, "Y".into())]).is_err());
        assert!(super::splice("abc", vec![(2..9, String::new())]).is_err());
    }
}
