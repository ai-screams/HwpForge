//! Stored result freshness and table-height invalidations in original coordinates.
//! Deliberately private: callers must first establish field/reference and admission safety.
use crate::cache_flow::{source_start, ParagraphFlow};
use crate::line_keep::{NamespaceBindings, Tag};
use crate::{HwpxError, HwpxResult};
use hwpforge_core::source_xml::{XmlElement, XmlSource};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FlowOrdinal(usize);
#[derive(Debug, Clone, Copy)]
#[allow(dead_code, reason = "insertion boundaries are not consumed by deletion")]
pub(crate) enum FlowBoundary {
    Before(FlowOrdinal),
    EndOfBody,
}
pub(crate) enum DerivedEdit<'a> {
    Delete {
        section: usize,
        paragraphs: &'a [FlowOrdinal],
    },
    #[allow(dead_code, reason = "insert retains its existing admission contract")]
    Insert {
        section: usize,
        at: FlowBoundary,
        fragment: &'a str,
    },
    #[allow(dead_code, reason = "text edits retain their existing admission contract")]
    Text {
        section: usize,
        changed: &'a [FlowOrdinal],
        proven: &'a [bool],
        slots: &'a [(Range<usize>, String)],
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeightInvalidation {
    pub(crate) section: usize,
    pub(crate) table_start: usize,
    pub(crate) value: Range<usize>,
}

fn invalid() -> HwpxError {
    HwpxError::InvalidStructure {
        detail: "unproven stored derived value or original source address".into(),
    }
}
fn require(yes: bool) -> HwpxResult<()> {
    if yes {
        Ok(())
    } else {
        Err(invalid())
    }
}

struct Part<'a> {
    section: Option<usize>,
    xml: String,
    source: XmlSource,
    by_start: HashMap<usize, usize>,
    flow: ParagraphFlow,
    tables: table_ledger::TableLedger,
    tags: Vec<Tag>,
    note_policies: [NotePolicy; 2],
    fields: BTreeMap<usize, Field>,
    map: Option<&'a crate::decoder::source_map::SourceMap>,
    text_spans: BTreeSet<(usize, usize)>,
}
#[derive(Clone, Copy)]
enum NotePolicy {
    Unknown,
    Continuous,
    Section,
}
struct Field {
    kind: String,
    params: BTreeMap<String, String>,
    end: usize,
    display: bool,
}
/// One bounded package read shared by both decisions. Source identities come from decoding.
pub(crate) struct DerivedFacts<'a> {
    parts: Vec<Part<'a>>,
    targets: BTreeMap<TargetKey, Vec<(usize, usize)>>,
}
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum TargetKey {
    Name(String),
    Object(u64),
}

#[cfg(test)]
thread_local! {pub(crate) static COLLECTIONS: std::cell::Cell<usize> = const {std::cell::Cell::new(0)};}

#[cfg(test)]
thread_local! {pub(crate) static DECISIONS:std::cell::RefCell<Vec<&'static str>>=const{std::cell::RefCell::new(Vec::new())};}

impl<'a> DerivedFacts<'a> {
    pub(crate) fn collect(
        package: &crate::raw_package::RawPackage,
        decoded: &'a crate::HwpxDocument,
    ) -> HwpxResult<Self> {
        #[cfg(test)]
        COLLECTIONS.with(|count| count.set(count.get() + 1));
        #[cfg(test)]
        DECISIONS.with(|d| d.borrow_mut().push("collect"));
        let sections: HashMap<_, _> = (0..decoded.source_maps.len())
            .map(|s| Ok((package.section_path(s)?, s)))
            .collect::<HwpxResult<_>>()?;
        let mut parts = Vec::new();
        for path in package.entry_paths().filter(|p| p.ends_with(".xml") || p.ends_with(".hpf")) {
            let xml = crate::package_xml::text_entry(path, package.read_entry(path)?.to_vec())?;
            let section = sections.get(path).copied();
            let map = section.map(|s| &decoded.source_maps[s]);
            if map.is_none() {
                // Unverified parts never take a negative lexical shortcut.
                crate::structural::xml_values::scan_fields(&xml, |_, _, _| Ok(()))?;
            }
            let source = XmlSource::read(&xml).map_err(|_| invalid())?.1;
            if let Some(map) = map {
                require(*source.digest() == map.source_digest)?;
            }
            let flow =
                map.map_or_else(|| ParagraphFlow::from_source(&source), |m| m.cache_flow.clone());
            parts.push(Part::read(section, xml, source, flow, map)?);
        }
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            decoded.document.sections(),
        );
        require(inventory.diagnostics().is_empty())?;
        for part in &mut parts {
            if let Some(map) = part.map {
                let section = part.section.ok_or_else(invalid)?;
                let by_span: HashMap<_, _> = part
                    .fields
                    .keys()
                    .map(|&id| {
                        let r = part.source.elements()[id].outer();
                        ((r.start, r.end), id)
                    })
                    .collect();
                for entry in inventory.fields().iter().filter(|f| f.section() == section) {
                    let node = map
                        .node(&hwpforge_core::node_path::CorePath::new(section, entry.start()))
                        .ok_or_else(invalid)?;
                    let candidates: BTreeSet<_> = node
                        .ranges()
                        .iter()
                        .filter_map(|r| by_span.get(&(r.start, r.end)).copied())
                        .collect();
                    // Native bookmarks and autoNum carriers have no fieldBegin display.
                    // Their source identities and stored values are checked in the raw ledger.
                    if candidates.is_empty()
                        && matches!(entry.kind(), "BOOKMARK" | "PAGE_NUM" | "AUTO_NUMBER")
                    {
                        continue;
                    }
                    require(candidates.len() == 1)?;
                    let field =
                        part.fields.get_mut(candidates.first().unwrap()).ok_or_else(invalid)?;
                    field.display |= !entry.current().is_empty();
                }
            }
        }
        Ok(Self::from_parts(parts))
    }
    fn from_parts(parts: Vec<Part<'a>>) -> Self {
        let mut targets = BTreeMap::<_, Vec<_>>::new();
        for (part_index, part) in parts.iter().enumerate() {
            for (id, e) in part.source.elements().iter().enumerate().skip(1) {
                let tag = &part.tags[id];
                if e.name() == "bookmark"
                    || e.name() == "fieldBegin"
                        && tag.attrs.get("type").is_some_and(|t| t == "BOOKMARK")
                {
                    if let Some(name) = tag.attrs.get("name") {
                        targets
                            .entry(TargetKey::Name(name.clone()))
                            .or_default()
                            .push((part_index, id));
                    }
                }
                if !matches!(e.name(), "p" | "fieldBegin" | "fieldEnd" | "header" | "footer")
                    && tag.name == e.name()
                {
                    let mut ids = BTreeSet::new();
                    for key in ["id", "instId", "instid"] {
                        if let Some(object) = tag
                            .attrs
                            .get(key)
                            .and_then(|v| crate::decoder::section::reference_object_id(v))
                        {
                            ids.insert(object);
                        }
                    }
                    for object in ids {
                        targets
                            .entry(TargetKey::Object(object))
                            .or_default()
                            .push((part_index, id));
                    }
                }
            }
        }
        Self { parts, targets }
    }
    fn section(&self, section: usize) -> HwpxResult<&Part<'a>> {
        self.parts.iter().find(|p| p.section == Some(section)).ok_or_else(invalid)
    }
    /// Resolve a body locator by its decoder span, never by casting an index.
    pub(crate) fn body_ordinal(&self, section: usize, index: usize) -> HwpxResult<FlowOrdinal> {
        let part = self.section(section)?;
        let map = part.map.ok_or_else(invalid)?;
        let p = map.paragraphs.get(index).ok_or_else(invalid)?;
        part.ordinal_at(p.outer.start, true)
    }
    #[allow(dead_code, reason = "insert retains its existing admission contract")]
    pub(crate) fn insertion_boundary(
        &self,
        section: usize,
        index: usize,
    ) -> HwpxResult<FlowBoundary> {
        let part = self.section(section)?;
        let map = part.map.ok_or_else(invalid)?;
        if index == map.paragraphs.len() {
            return Ok(FlowBoundary::EndOfBody);
        }
        Ok(FlowBoundary::Before(self.body_ordinal(section, index)?))
    }
}

impl<'a> Part<'a> {
    fn read(
        section: Option<usize>,
        xml: String,
        source: XmlSource,
        flow: ParagraphFlow,
        map: Option<&'a crate::decoder::source_map::SourceMap>,
    ) -> HwpxResult<Self> {
        let tables = table_ledger::TableLedger::from_source(&source, &flow)?;
        let text_spans = if let Some(map) = map {
            map.text_slots()
                .iter()
                .filter_map(|slot| match slot.locator {
                    crate::exchange::TextLocator::TextElement {
                        content_start: Some(start),
                        content_end: Some(end),
                        ..
                    } => Some((start, end)),
                    _ => None,
                })
                .collect()
        } else {
            source
                .elements()
                .iter()
                .filter(|e| e.name() == "t")
                .filter_map(|e| e.inner().map(|r| (r.start, r.end)))
                .collect()
        };
        let by_start = source
            .elements()
            .iter()
            .enumerate()
            .skip(1)
            .map(|(id, e)| {
                crate::work::tick();
                (e.outer().start, id)
            })
            .collect();
        // Only decoder-validated sections with a bound root and no local-name
        // clues can avoid semantic collection. Unverified/aliased/rebound parts parse.
        if map.is_some() && safe_negative(&xml) {
            let tags = vec![Tag::default(); source.elements().len()];
            return Ok(Self {
                section,
                xml,
                source,
                by_start,
                flow,
                tables,
                tags,
                note_policies: [NotePolicy::Unknown; 2],
                fields: BTreeMap::new(),
                map,
                text_spans,
            });
        }
        let elements = source.elements();
        let mut tags = vec![Tag::default(); elements.len()];
        let empty_bindings = Rc::new(NamespaceBindings::new());
        let mut bindings = vec![empty_bindings; elements.len()];
        for (id, e) in elements.iter().enumerate().skip(1) {
            let outer = e.outer();
            let end = e.inner().map_or(outer.end, |r| r.start);
            let fragment = xml.get(outer.start..end).ok_or_else(invalid)?;
            let start = source_start(&xml, e).ok_or_else(invalid)?;
            let declares = start.attributes().try_fold(false, |declares, a| {
                let a = a.map_err(|_| invalid())?;
                Ok::<_, HwpxError>(
                    declares || a.key.as_ref() == "xmlns" || a.key.as_ref().starts_with("xmlns:"),
                )
            })?;
            let (mut tag, scope) = if declares {
                let (tag, scope) = Tag::opening_with_bindings(fragment, &bindings[e.parent()])
                    .ok_or_else(invalid)?;
                (tag, Rc::new(scope))
            } else {
                (
                    Tag::opening(fragment, &bindings[e.parent()]).ok_or_else(invalid)?,
                    bindings[e.parent()].clone(),
                )
            };
            let (prefix, _) =
                e.qualified_name().split_once(':').unwrap_or(("", e.qualified_name()));
            tag.name = if scope.get(prefix).map(AsRef::as_ref) == Some(namespace(e.name())) {
                e.name().into()
            } else {
                format!("{{foreign}}{}", e.name())
            };
            tags[id] = tag;
            bindings[id] = scope;
            crate::work::tick();
        }
        let mut values = BTreeMap::new();
        crate::structural::xml_values::scan_fields(&xml, |start, kind, params| {
            require(values.insert(start, (kind.to_owned(), params.clone())).is_none())
        })?;
        let mut fields = BTreeMap::new();
        let mut open: HashMap<(usize, String), usize> = HashMap::new();
        let mut seen_fields = BTreeSet::new();
        for (id, e) in elements.iter().enumerate().skip(1) {
            if candidate(e.name()) {
                // Namespace-less fragments and foreign lookalikes are never R evidence.
                let (prefix, _) =
                    e.qualified_name().split_once(':').unwrap_or(("", e.qualified_name()));
                let uri = bindings[id].get(prefix).map(AsRef::as_ref);
                require(uri == Some(namespace(e.name())))?;
            }
            if e.name() == "fieldBegin" {
                let (kind, params) = values.remove(&e.outer().start).ok_or_else(invalid)?;
                let key = (field_flow(elements, id), attr(&tags[id], "id")?.to_owned());
                require(seen_fields.insert(key.clone()))?;
                require(open.insert(key, id).is_none())?;
                fields.insert(id, Field { kind, params, end: 0, display: false });
            } else if e.name() == "fieldEnd" {
                let key = (field_flow(elements, id), attr(&tags[id], "beginIDRef")?.to_owned());
                let begin = open.remove(&key).ok_or_else(invalid)?;
                let f = fields.get_mut(&begin).ok_or_else(invalid)?;
                f.end = id;
                f.display = elements[begin + 1..id]
                    .iter()
                    .any(|t| t.name() == "t" && t.inner().is_some_and(|r| !xml[r].is_empty()));
            }
        }
        require(open.is_empty() && values.is_empty())?;
        let mut part = Self {
            section,
            xml,
            source,
            by_start,
            flow,
            tables,
            tags,
            fields,
            map,
            text_spans,
            note_policies: [NotePolicy::Unknown; 2],
        };
        part.note_policies = part.collect_note_policies();
        Ok(part)
    }
    // Each part owns one policy per note kind; all stored number representations reuse it.
    fn collect_note_policies(&self) -> [NotePolicy; 2] {
        let mut policies: [Vec<usize>; 2] = Default::default();
        for (id, element) in self.source.elements().iter().enumerate() {
            crate::work::tick();
            match element.name() {
                "footNotePr" => policies[0].push(id),
                "endNotePr" => policies[1].push(id),
                _ => {}
            }
        }
        policies.map(|ids| {
            let [id] = ids.as_slice() else {
                return NotePolicy::Unknown;
            };
            let Ok(numbering) = self.one_child(*id, "numbering") else {
                return NotePolicy::Unknown;
            };
            match self.tags[numbering].attrs.get("type").map(String::as_str) {
                Some("CONTINUOUS") => NotePolicy::Continuous,
                Some("ON_SECTION") => NotePolicy::Section,
                _ => NotePolicy::Unknown,
            }
        })
    }
    fn ordinal_at(&self, offset: usize, body: bool) -> HwpxResult<FlowOrdinal> {
        let p = self.flow.paragraph_at(offset).ok_or_else(invalid)?;
        require(self.flow.span(p).is_some_and(|r| r.start == offset))?;
        if body {
            let elements = self.source.elements();
            let e = &elements[self.element_at(offset)?];
            require(e.name() == "p")?;
            require(elements[e.parent()].name() == "sec")?;
        }
        Ok(FlowOrdinal(p))
    }
    fn element_at(&self, start: usize) -> HwpxResult<usize> {
        crate::work::tick();
        self.by_start.get(&start).copied().ok_or_else(invalid)
    }
    fn owner(&self, id: usize) -> Option<usize> {
        let p = self.flow.paragraph_at(self.source.elements()[id].outer().start)?;
        let table = self.tables.table_owner(p)?;
        Some(self.tables.tables[table].outer.start)
    }
    fn body_path(&self, id: usize) -> HwpxResult<Vec<&str>> {
        let elements = self.source.elements();
        let mut names = Vec::new();
        let mut at = id;
        while at != 0 {
            require(self.tags[at].name == elements[at].name())?;
            names.push(elements[at].name());
            at = elements[at].parent();
        }
        names.reverse();
        Ok(names)
    }
    fn body_bookmark_target(&self, id: usize) -> HwpxResult<()> {
        require(self.section.is_some())?;
        let mut at = id;
        while at != 0 {
            let element = &self.source.elements()[at];
            let tag = &self.tags[at];
            let independent = matches!(
                element.name(),
                "header" | "footer" | "masterPage" | "footNote" | "endNote" | "memo"
            ) || element.name() == "fieldBegin"
                && tag.attrs.get("type").is_some_and(|kind| kind == "MEMO");
            require(!independent && tag.name == element.name())?;
            at = element.parent();
        }
        Ok(())
    }
    fn one_child(&self, id: usize, name: &str) -> HwpxResult<usize> {
        let matches: Vec<_> = self.source.elements()[id]
            .children()
            .iter()
            .copied()
            .filter(|&i| self.source.elements()[i].name() == name)
            .collect();
        require(matches.len() == 1)?;
        require(self.tags[matches[0]].name == name)?;
        Ok(matches[0])
    }
}
fn safe_negative(xml: &str) -> bool {
    let Some((root, scope)) = Tag::opening_with_bindings(xml, &Default::default()) else {
        return false;
    };
    let input = crate::wire_xml::PositionedXml::new(xml);
    let mut reader = quick_xml::Reader::from_str(input.as_str());
    let mut end = 0;
    while let Ok(event) = reader.read_event() {
        match event {
            quick_xml::events::Event::Start(_) | quick_xml::events::Event::Empty(_) => {
                end = input.original_offset(reader.buffer_position());
                break;
            }
            quick_xml::events::Event::Decl(_) => {}
            _ => return false,
        }
    }
    let bound = scope.values().any(|v| v.as_ref() == "http://www.hancom.co.kr/hwpml/2011/section");
    root.name == "sec"
        && bound
        && !xml.contains("<!")
        && !xml[end..].contains("xmlns")
        && !xml.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|token| {
            candidate(token)
                || matches!(
                    token,
                    "bookmark"
                        | "footNotePr"
                        | "endNotePr"
                        | "LastResult"
                        | "Formula"
                        | "Command"
                        | "startNum"
                        | "beginNum"
                        | "toc"
                )
                || token.ends_with("Result")
        })
}
fn namespace(name: &str) -> &str {
    if name == "sec" {
        "http://www.hancom.co.kr/hwpml/2011/section"
    } else if matches!(name, "numCache" | "strCache") {
        "http://schemas.openxmlformats.org/drawingml/2006/chart"
    } else {
        "http://www.hancom.co.kr/hwpml/2011/paragraph"
    }
}
fn candidate(name: &str) -> bool {
    matches!(
        name,
        "parameters"
            | "stringParam"
            | "integerParam"
            | "booleanParam"
            | "autoNum"
            | "autoNumFormat"
            | "fieldBegin"
            | "fieldEnd"
            | "footNote"
            | "endNote"
            | "footNotePr"
            | "endNotePr"
            | "bookmark"
            | "tbl"
            | "tr"
            | "tc"
            | "sz"
            | "pos"
            | "cellSz"
            | "cellSpan"
            | "subList"
            | "numCache"
            | "strCache"
            | "newNum"
            | "pageNum"
    )
}
fn attr<'a>(tag: &'a Tag, key: &str) -> HwpxResult<&'a str> {
    tag.attrs.get(key).map(String::as_str).ok_or_else(invalid)
}
fn exact(tag: &Tag, expected: &[(&str, &str)]) -> HwpxResult<()> {
    for &(k, v) in expected {
        require(attr(tag, k)? == v)?;
    }
    Ok(())
}
fn field_flow(elements: &[XmlElement], mut id: usize) -> usize {
    while id != 0 {
        let parent = elements[id].parent();
        if elements[id].name() == "p" {
            return parent;
        }
        id = parent;
    }
    0
}

mod table_ledger;
mod table_profiles;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Default)]
struct Effect {
    deleted: Vec<Range<usize>>,
    touched: Vec<Range<usize>>,
    changed: BTreeSet<usize>,
    seeds: Vec<bool>,
    proof: Vec<bool>,
    kinds: BTreeSet<String>,
    page_restart: bool,
    flow: bool,
    body_structure: bool,
}
impl Effect {
    fn removed(&self, span: &Range<usize>) -> bool {
        let before = self.deleted.partition_point(|r| r.start <= span.start);
        before > 0 && self.deleted[before - 1].end >= span.end
    }
    fn touches(&self, span: &Range<usize>) -> bool {
        intersects(&self.touched, span)
    }
    fn survives(&self, span: &Range<usize>) -> HwpxResult<bool> {
        if self.removed(span) {
            return Ok(false);
        }
        require(!intersects(&self.deleted, span))?;
        Ok(true)
    }
}
fn intersects(ranges: &[Range<usize>], span: &Range<usize>) -> bool {
    let before = ranges.partition_point(|r| r.start < span.end);
    before > 0 && ranges[before - 1].end > span.start
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}
fn edit_section(edit: &DerivedEdit<'_>) -> usize {
    match edit {
        DerivedEdit::Delete { section, .. }
        | DerivedEdit::Insert { section, .. }
        | DerivedEdit::Text { section, .. } => *section,
    }
}
fn effects(
    facts: &DerivedFacts<'_>,
    edits: &[DerivedEdit<'_>],
) -> HwpxResult<BTreeMap<usize, Effect>> {
    let mut effects = BTreeMap::<usize, Effect>::new();
    let mut boundaries = BTreeSet::new();
    for edit in edits {
        let section = edit_section(edit);
        let part = facts.section(section)?;
        let n = part.flow.paragraph_count();
        let effect = effects.entry(section).or_insert_with(|| Effect {
            seeds: vec![false; n],
            proof: vec![true; n],
            ..Default::default()
        });
        match edit {
            DerivedEdit::Delete { paragraphs, .. } => {
                require(!paragraphs.is_empty())?;
                for &FlowOrdinal(p) in *paragraphs {
                    let span = part.flow.span(p).ok_or_else(invalid)?;
                    part.ordinal_at(span.start, true)?;
                    require(effect.changed.insert(p))?;
                    effect.seeds[p] = true;
                    effect.deleted.push(span.clone());
                    effect.touched.push(span);
                }
                effect.flow = true;
                effect.body_structure = true;
            }
            DerivedEdit::Insert { at, fragment, .. } => {
                require(!fragment.is_empty())?;
                let key = match at {
                    FlowBoundary::Before(FlowOrdinal(p)) => {
                        let span = part.flow.span(*p).ok_or_else(invalid)?;
                        part.ordinal_at(span.start, true)?;
                        effect.seeds[*p] = true;
                        Some(*p)
                    }
                    FlowBoundary::EndOfBody => None,
                };
                require(boundaries.insert((section, key)))?;
                let (root, scope) = Tag::opening_with_bindings(&part.xml, &Default::default())
                    .ok_or_else(invalid)?;
                require(root.name == "sec")?;
                let namespaces = scope
                    .iter()
                    .map(|(k, v)| {
                        let prefix =
                            if k.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{k}") };
                        format!(" {prefix}=\"{}\"", quick_xml::escape::escape(v.as_ref()))
                    })
                    .collect::<String>();
                let xml = format!("<sec{namespaces}>{fragment}</sec>");
                crate::structural::xml_values::scan_fields(&xml, |_, _, _| Ok(()))?;
                let source = XmlSource::read(&xml).map_err(|_| invalid())?.1;
                let flow = ParagraphFlow::from_source(&source);
                let fragment = Part::read(None, xml, source, flow, None)?;
                // Inherited heading policy can change surviving outline/number references.
                for (id, e) in fragment.source.elements().iter().enumerate().skip(1) {
                    record_delta(effect, e, &fragment.tags[id]);
                }
                effect.flow = true;
                effect.body_structure = true;
            }
            DerivedEdit::Text { changed, proven, slots, .. } => {
                require(proven.len() == n && !changed.is_empty() && !slots.is_empty())?;
                let requested: BTreeSet<_> = changed.iter().map(|p| p.0).collect();
                require(requested.len() == changed.len())?;
                for &p in &requested {
                    require(p < n && effect.changed.insert(p))?;
                }
                let mut found = BTreeSet::new();
                for (span, replacement) in *slots {
                    let original = part.xml.get(span.clone()).ok_or_else(invalid)?;
                    require(original != replacement && !span.is_empty())?;
                    let p = part.flow.paragraph_at(span.start).ok_or_else(invalid)?;
                    require(
                        requested.contains(&p)
                            && part.flow.span(p).is_some_and(|r| span.end <= r.end),
                    )?;
                    // Decoder nodes establish slot ownership; raw fallback needs a whole t body.
                    require(part.text_spans.contains(&(span.start, span.end)))?;
                    found.insert(p);
                    effect.touched.push(span.clone());
                }
                require(found == requested)?;
                for (p, &yes) in proven.iter().enumerate() {
                    effect.proof[p] &= yes;
                }
            }
        }
    }
    for (&section, effect) in &mut effects {
        let part = facts.section(section)?;
        let mut changed = vec![false; part.flow.paragraph_count()];
        for &p in &effect.changed {
            changed[p] = true;
        }
        let text_seeds = part.flow.effective_text_seeds(&changed, &effect.proof);
        for (seed, text) in effect.seeds.iter_mut().zip(text_seeds) {
            *seed |= text;
        }
        effect.flow |= effect.seeds.iter().any(|&seed| seed);
        effect.deleted.sort_by_key(|r| r.start);
        effect.touched.sort_by_key(|r| r.start);
        require(!effect.touched.windows(2).any(|pair| overlaps(&pair[0], &pair[1])))?;
        for (id, e) in part.source.elements().iter().enumerate().skip(1) {
            if effect.removed(&e.outer()) {
                record_delta(effect, e, &part.tags[id]);
            }
        }
    }
    Ok(effects)
}
fn record_delta(effect: &mut Effect, e: &XmlElement, tag: &Tag) {
    let kind = match e.name() {
        "tbl" => Some("TABLE"),
        "pic" => Some("PICTURE"),
        "equation" => Some("EQUATION"),
        "footNote" => Some("FOOTNOTE"),
        "endNote" => Some("ENDNOTE"),
        "autoNum" => tag.attrs.get("numType").map(String::as_str),
        "newNum" => tag.attrs.get("numType").or_else(|| tag.attrs.get("type")).map(String::as_str),
        _ => None,
    };
    if let Some(kind) = kind {
        effect.kinds.insert(kind.to_owned());
    }
    let known_kind =
        matches!(kind, Some("PAGE" | "TABLE" | "PICTURE" | "EQUATION" | "FOOTNOTE" | "ENDNOTE"))
            || e.name() == "autoNum" && kind == Some("TOTAL_PAGE");
    let broad = e.name() == "beginNum" || matches!(e.name(), "autoNum" | "newNum") && !known_kind;
    if e.name() == "startNum" {
        effect.kinds.extend(["TABLE", "PICTURE", "EQUATION"].map(str::to_owned));
    }
    if broad {
        effect.kinds.extend(
            ["PAGE", "TABLE", "PICTURE", "EQUATION", "FOOTNOTE", "ENDNOTE"].map(str::to_owned),
        );
    }
    if broad
        || matches!(e.name(), "pageNum" | "startNum")
        || kind == Some("PAGE") && e.name() == "newNum"
    {
        effect.page_restart = true;
    }
    if matches!(e.name(), "footNotePr" | "endNotePr") {
        effect.kinds.insert(if e.name() == "footNotePr" { "FOOTNOTE" } else { "ENDNOTE" }.into());
    }
}

/// Effects are bound to their source facts and shared by both decisions.
pub(crate) struct DerivedEffects<'facts, 'source> {
    facts: &'facts DerivedFacts<'source>,
    affected_tables: BTreeMap<usize, AffectedTables>,
    effects: BTreeMap<usize, Effect>,
    flow: bool,
    kinds: BTreeSet<String>,
    page_restart: bool,
}

struct AffectedTables {
    by_table: Vec<bool>,
    // Sorted starts with prefix maximum ends also cover nested table ranges.
    starts_and_max_ends: Vec<(usize, usize)>,
}
impl AffectedTables {
    fn new(by_table: Vec<bool>, tables: &[table_ledger::FlowTable]) -> Self {
        let mut starts_and_max_ends: Vec<_> = by_table
            .iter()
            .zip(tables)
            .filter_map(|(&affected, table)| {
                crate::work::tick();
                affected.then_some((table.outer.start, table.outer.end))
            })
            .collect();
        starts_and_max_ends.sort_unstable_by_key(|&(start, _)| start);
        let mut max_end = 0;
        for (_, end) in &mut starts_and_max_ends {
            crate::work::tick();
            max_end = max_end.max(*end);
            *end = max_end;
        }
        Self { by_table, starts_and_max_ends }
    }

    fn overlaps(&self, span: &Range<usize>) -> bool {
        crate::work::tick();
        let before = self.starts_and_max_ends.partition_point(|&(start, _)| start < span.end);
        before > 0 && self.starts_and_max_ends[before - 1].1 > span.start
    }
}
impl<'source> DerivedFacts<'source> {
    pub(crate) fn effects(
        &self,
        edits: &[DerivedEdit<'_>],
    ) -> HwpxResult<DerivedEffects<'_, 'source>> {
        let effects = effects(self, edits)?;
        let flow = effects.values().any(|e| e.flow);
        let kinds = effects.values().flat_map(|e| e.kinds.iter().cloned()).collect();
        let page_restart = effects.values().any(|e| e.page_restart);
        let affected_tables = effects
            .iter()
            .map(|(&section, effect)| {
                let part = self.section(section)?;
                let mut affected = part.tables.affected_tables(&part.flow, &effect.seeds);
                for (yes, record) in affected.iter_mut().zip(&part.tables.tables) {
                    *yes |= effect.body_structure && record.host.is_none() && record.boundary == 0;
                }
                Ok((section, AffectedTables::new(affected, &part.tables.tables)))
            })
            .collect::<HwpxResult<_>>()?;
        Ok(DerivedEffects { facts: self, effects, affected_tables, flow, kinds, page_restart })
    }
}
impl DerivedEffects<'_, '_> {
    pub(crate) fn check_preserved_loss(
        &self,
        section: usize,
        span: &Range<usize>,
        digest: &[u8; 32],
    ) -> HwpxResult<()> {
        let part = self.facts.section(section)?;
        require(part.source.digest() == digest)?;
        require(part.source.elements()[part.element_at(span.start)?].outer() == *span)?;
        if let Some(effect) = self.effects.get(&section) {
            require(effect.survives(span)?)?;
            require(!self.affected_tables[&section].overlaps(span))?;
        }
        Ok(())
    }
    // Every stored number representation uses the same policy decision.
    fn number_unchanged(&self, part: &Part<'_>, kind: &str) -> HwpxResult<()> {
        match kind {
            "FOOTNOTE" | "ENDNOTE" => {
                note_unchanged(part, kind, self.flow, self.kinds.contains(kind), &self.effects)
            }
            _ => require(!self.kinds.contains(kind)),
        }
    }
    fn page_numbering_unchanged(&self) -> HwpxResult<()> {
        require(!self.page_restart)
    }

    pub(crate) fn check_derived_values(&self) -> HwpxResult<()> {
        #[cfg(test)]
        DECISIONS.with(|d| d.borrow_mut().push("check"));
        let facts = self.facts;
        let effects = &self.effects;
        if effects.is_empty() {
            return Ok(());
        }
        for part in &facts.parts {
            let effect = part.section.and_then(|s| effects.get(&s));
            for (id, e) in part.source.elements().iter().enumerate().skip(1) {
                if e.name() == "fieldBegin" {
                    let f = part.fields.get(&id).ok_or_else(invalid)?;
                    if let Some(effect) = effect {
                        let relevant = (id..=f.end).filter(|&i| {
                            matches!(
                                part.source.elements()[i].name(),
                                "fieldBegin" | "fieldEnd" | "t"
                            )
                        });
                        let removed: Vec<_> = relevant
                            .map(|i| effect.survives(&part.source.elements()[i].outer()))
                            .collect::<HwpxResult<_>>()?;
                        if removed.iter().all(|&survives| !survives) {
                            continue;
                        }
                        require(removed.iter().all(|&survives| survives))?;
                    }
                } else if effect.is_some_and(|x| x.removed(&e.outer())) {
                    continue;
                }
                let tag = &part.tags[id];
                match e.name() {
                    "autoNum" => {
                        // Presence, including zero, is stored state; absence is unknown.
                        let _ = attr(tag, "num")?.parse::<u32>().map_err(|_| invalid())?;
                        let kind = attr(tag, "numType")?;
                        match kind {
                            "PAGE" | "TOTAL_PAGE" => {
                                self.page_numbering_unchanged()?;
                                if self.flow {
                                    page_r(part, id, kind)?;
                                }
                            }
                            "FOOTNOTE" | "ENDNOTE" | "TABLE" | "PICTURE" | "EQUATION" => {
                                self.number_unchanged(part, kind)?
                            }
                            _ => return Err(invalid()),
                        }
                    }
                    "footNote" | "endNote" => {
                        let _ = attr(tag, "number")?.parse::<u32>().map_err(|_| invalid())?;
                        self.number_unchanged(
                            part,
                            if e.name() == "footNote" { "FOOTNOTE" } else { "ENDNOTE" },
                        )?;
                    }
                    "fieldBegin" => {
                        let f = part.fields.get(&id).ok_or_else(invalid)?;
                        field_unchanged(self, part, id, f)?;
                    }
                    // Chart data and relationships are unedited by these paragraph operations.
                    "numCache" | "strCache" => {
                        require(!effect.is_some_and(|x| x.touches(&e.outer())))?;
                    }
                    name if name.ends_with("Result") || name == "toc" => return Err(invalid()),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn page_r(part: &Part<'_>, id: usize, kind: &str) -> HwpxResult<()> {
    require(part.section.is_some())?;
    let path = part.body_path(id)?;
    let body = path == ["sec", "p", "run", "ctrl", "autoNum"];
    let footer =
        path == ["sec", "p", "run", "ctrl", "footer", "subList", "p", "run", "ctrl", "autoNum"];
    require(body || kind == "PAGE" && footer)?;
    let format = part.one_child(id, "autoNumFormat")?;
    exact(
        &part.tags[format],
        &[
            ("type", "DIGIT"),
            ("userChar", ""),
            ("prefixChar", ""),
            ("suffixChar", ""),
            ("supscript", "0"),
        ],
    )
}
fn note_unchanged(
    part: &Part<'_>,
    kind: &str,
    flow: bool,
    changed: bool,
    effects: &BTreeMap<usize, Effect>,
) -> HwpxResult<()> {
    crate::work::tick();
    if !flow && !changed {
        return Ok(());
    }
    match part.note_policies[usize::from(kind == "ENDNOTE")] {
        NotePolicy::Continuous => require(!changed),
        NotePolicy::Section => require(
            part.section.is_some_and(|s| !effects.get(&s).is_some_and(|e| e.kinds.contains(kind))),
        ),
        NotePolicy::Unknown => Err(invalid()),
    }
}

fn field_unchanged(
    changes: &DerivedEffects<'_, '_>,
    part: &Part<'_>,
    id: usize,
    f: &Field,
) -> HwpxResult<()> {
    let effects = &changes.effects;
    let effect = part.section.and_then(|section| effects.get(&section));
    match f.kind.as_str() {
        "CLICK_HERE" | "BOOKMARK" | "MEMO" | "HYPERLINK" => Ok(()),
        "DATE" | "TIME" => Ok(()),
        "SUMMERY" | "DOC_SUMMARY" | "USER_INFO" | "PATH" => {
            require(
                f.params
                    .get("Property")
                    .is_none_or(|p| f.params.get("Command").is_none_or(|c| c == p)),
            )?;
            let command =
                f.params.get("Command").or_else(|| f.params.get("Property")).ok_or_else(invalid)?;
            require(matches!(
                command.as_str(),
                "$author"
                    | "$lastsaveby"
                    | "$createtime"
                    | "$modifiedtime"
                    | "$title"
                    | "$subject"
                    | "$keywords"
                    | "$comments"
                    | "$username"
                    | "$filepath"
                    | "$filename"
                    | "$P$F"
                    | "$P"
                    | "$F"
            ))
        }
        "CALC" | "FORMULA" | "SUM" => {
            let stored = f.display
                || f.params.contains_key("LastResult")
                || f.params.get("Command").is_some_and(|c| c.contains(";;"));
            let formula = local_sum(&f.params)?;
            require(formula)?;
            let owner = part.owner(id).ok_or_else(invalid)?;
            if stored {
                let table = &part.source.elements()[part.element_at(owner)?];
                require(table.name() == "tbl")?;
                require(!effect.is_some_and(|x| x.touches(&table.outer())))?;
            }
            Ok(())
        }
        "CROSSREF" => crossref_unchanged(changes, part, id, f),
        _ => Err(invalid()),
    }
}
fn local_sum(params: &BTreeMap<String, String>) -> HwpxResult<bool> {
    require(params.keys().all(|k| {
        matches!(k.as_str(), "Prop" | "Formula" | "Command" | "ResultFormat" | "LastResult")
    }))?;
    let formula = params.get("Formula");
    let command = params.get("Command");
    let mut command_formula = None;
    if let Some(command) = command {
        let (expression, metadata) =
            command.split_once("??").map_or((command.as_str(), None), |(e, m)| (e, Some(m)));
        if let Some(metadata) = metadata {
            let (format, result) = metadata.split_once(";;").ok_or_else(invalid)?;
            require(format == "%g," && params.get("ResultFormat").is_none_or(|f| f == format))?;
            require(!result.is_empty() && result.parse::<f64>().is_ok_and(|n| n.is_finite()))?;
        } else {
            require(!expression.contains(";;"))?;
        }
        command_formula = Some(expression);
    }
    if let (Some(formula), Some(command)) = (formula, command_formula) {
        require(formula == command)?;
    }
    let expression = formula.map(String::as_str).or(command_formula).ok_or_else(invalid)?;
    let Some(arg) = expression.strip_prefix("=SUM(").and_then(|v| v.strip_suffix(')')) else {
        return Ok(false);
    };
    if matches!(arg, "LEFT" | "RIGHT" | "ABOVE" | "BELOW") {
        return Ok(true);
    }
    let Some((a, b)) = arg.split_once(':') else {
        return Ok(false);
    };
    fn cell(s: &str) -> bool {
        let letters = if s.starts_with('?') {
            1
        } else {
            s.bytes().take_while(u8::is_ascii_uppercase).count()
        };
        let rest = &s[letters..];
        letters > 0 && !rest.is_empty() && (rest == "?" || rest.bytes().all(|b| b.is_ascii_digit()))
    }
    Ok(cell(a) && cell(b))
}
fn crossref_unchanged(
    changes: &DerivedEffects<'_, '_>,
    part: &Part<'_>,
    id: usize,
    f: &Field,
) -> HwpxResult<()> {
    let facts = changes.facts;
    let effects = &changes.effects;
    use hwpforge_core::control::RefTarget;
    use hwpforge_foundation::{RefContentType, RefType};
    require(
        crate::structural::references::Removed::default().field_is_safe("CROSSREF", &f.params),
    )?;
    let kind =
        f.params.get("RefType").ok_or_else(invalid)?.parse::<RefType>().map_err(|_| invalid())?;
    let content_type = f
        .params
        .get("RefContentType")
        .ok_or_else(invalid)?
        .parse::<RefContentType>()
        .map_err(|_| invalid())?;
    let command = f.params.get("Command").ok_or_else(invalid)?;
    let (_, codes) = command.split_once(';').ok_or_else(invalid)?;
    let mut codes = codes.split(';');
    let type_code = codes.next().and_then(|n| n.parse::<u8>().ok());
    let content_code = codes.next().and_then(|n| n.parse::<u8>().ok());
    require(
        type_code == Some(crate::encoder::section::ref_type_wire_code(&kind))
            && content_code
                == Some(crate::encoder::section::ref_content_type_wire_code(&kind, &content_type)),
    )?;
    let path = f.params.get("RefPath").ok_or_else(invalid)?;
    require(
        path.starts_with('?')
            && path.ends_with(';')
            && !path[1..path.len() - 1].contains(['?', ';', '%']),
    )?;
    let target = crate::decoder::section::decode_crossref_target(path, &kind);
    let key = match target {
        RefTarget::Name(name) => TargetKey::Name(name),
        RefTarget::Object(object) => TargetKey::Object(object.value()),
        _ => return Err(invalid()),
    };
    let targets = facts.targets.get(&key).ok_or_else(invalid)?;
    require(targets.len() == 1)?;
    let (target_part_index, target_id) = targets[0];
    let target_part = &facts.parts[target_part_index];
    if let Some(effect) = target_part.section.and_then(|s| effects.get(&s)) {
        require(effect.survives(&target_part.source.elements()[target_id].outer())?)?;
    }
    let target_name = target_part.source.elements()[target_id].name();
    let target_matches = match kind {
        RefType::Bookmark => matches!(target_name, "bookmark" | "fieldBegin"),
        RefType::Table => target_name == "tbl",
        RefType::Figure => target_name == "pic",
        RefType::Equation => target_name == "equation",
        RefType::Footnote => target_name == "footNote",
        RefType::Endnote => target_name == "endNote",
        _ => false,
    };
    require(target_matches)?;
    if !f.display {
        return Ok(());
    }
    let content = f.params.get("RefContentType").ok_or_else(invalid)?;
    match content.as_str() {
        "OBJECT_TYPE_PAGE" => {
            changes.page_numbering_unchanged()?;
            if !changes.flow {
                return Ok(());
            }
            require(part.section.is_some())?;
            target_part.body_bookmark_target(target_id)?;
            require(
                matches!(kind, RefType::Bookmark)
                    && part.body_path(id)? == ["sec", "p", "run", "ctrl", "fieldBegin"],
            )
        }
        "OBJECT_TYPE_NUMBER" => {
            let number_kind = match kind {
                RefType::Table => "TABLE",
                RefType::Figure => "PICTURE",
                RefType::Equation => "EQUATION",
                RefType::Footnote => "FOOTNOTE",
                RefType::Endnote => "ENDNOTE",
                _ => return Err(invalid()),
            };
            changes.number_unchanged(target_part, number_kind)
        }
        "OBJECT_TYPE_CONTENTS" => {
            // Only an actual object subtree provides a complete content boundary.
            require(!matches!(kind, RefType::Bookmark))?;
            let range = target_part.source.elements()[target_id].outer();
            require(
                !target_part
                    .section
                    .and_then(|s| effects.get(&s))
                    .is_some_and(|e| e.touches(&range)),
            )
        }
        _ => Err(invalid()),
    }
}

impl DerivedEffects<'_, '_> {
    pub(crate) fn collect_height_invalidations(&self) -> HwpxResult<Vec<HeightInvalidation>> {
        #[cfg(test)]
        DECISIONS.with(|d| d.borrow_mut().push("heights"));
        let facts = self.facts;
        let effects = &self.effects;
        let mut heights = Vec::new();
        for (&section, effect) in effects {
            let part = facts.section(section)?;
            let affected = &self.affected_tables[&section].by_table;
            for (table, record) in part.tables.tables.iter().enumerate() {
                let survives = effect.survives(&record.outer)?;
                if survives && affected[table] {
                    if let Some(height) = height_for_table(part, table)? {
                        heights.push(height);
                    }
                }
            }
        }
        Ok(heights)
    }
}

fn height_for_table(part: &Part<'_>, table: usize) -> HwpxResult<Option<HeightInvalidation>> {
    let record = part.tables.tables.get(table).ok_or_else(invalid)?;
    require(record.parent.is_none() && record.boundary == 0)?;
    let id = part.element_at(record.outer.start)?;
    require(part.body_path(id)? == ["sec", "p", "run", "tbl"])?;
    require(table_profiles::matches(part, id))?;
    let sz = part.one_child(id, "sz")?;
    let elements = part.source.elements();
    let start = source_start(&part.xml, &elements[sz]).ok_or_else(invalid)?;
    let mut height = None;
    for a in start.attributes() {
        let a = a.map_err(|_| invalid())?;
        if a.key.as_ref() == "height" {
            let number = a
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|_| invalid())?
                .parse::<u32>()
                .map_err(|_| invalid())?;
            let std::borrow::Cow::Borrowed(raw) = a.value else {
                return Err(invalid());
            };
            let offset = (raw.as_ptr() as usize)
                .checked_sub(part.xml.as_ptr() as usize)
                .ok_or_else(invalid)?;
            let range = offset..offset.checked_add(raw.len()).ok_or_else(invalid)?;
            require(
                range.start >= elements[sz].outer().start
                    && range.end <= elements[sz].outer().end
                    && part.xml.get(range.clone()) == Some(raw),
            )?;
            require(height.replace((number, range)).is_none())?;
        }
    }
    let Some((number, value)) = height else {
        return Ok(None);
    };
    if number == 0 {
        return Ok(None);
    }
    Ok(Some(HeightInvalidation {
        section: part.section.ok_or_else(invalid)?,
        table_start: elements[id].outer().start,
        value,
    }))
}

/// Revalidate independently supplied authorizations before adding them to either plan.
pub(crate) fn append_height_replacements(
    xml: &str,
    map: &crate::decoder::source_map::SourceMap,
    section: usize,
    heights: &[HeightInvalidation],
    edits: &mut Vec<(Range<usize>, String)>,
) -> HwpxResult<()> {
    if heights.is_empty() {
        return Ok(());
    }
    let source = XmlSource::read(xml).map_err(|_| invalid())?.1;
    require(*source.digest() == map.source_digest)?;
    let by_start: HashMap<_, _> = source
        .elements()
        .iter()
        .enumerate()
        .skip(1)
        .map(|(id, e)| {
            crate::work::tick();
            (e.outer().start, id)
        })
        .collect();
    let ledger = table_ledger::TableLedger::from_source(&source, &map.cache_flow)?;
    let tables: HashMap<_, _> = ledger
        .tables
        .iter()
        .map(|t| {
            crate::work::tick();
            (t.outer.start, &t.outer)
        })
        .collect();
    for height in heights {
        crate::work::tick();
        require(height.section == section)?;
        let outer = tables.get(&height.table_start).ok_or_else(invalid)?;
        let id = by_start.get(&height.table_start).ok_or_else(invalid)?;
        let table = &source.elements()[*id];
        require(table.name() == "tbl" && table.outer() == **outer)?;
        let sizes: Vec<_> = table
            .children()
            .iter()
            .map(|&i| &source.elements()[i])
            .filter(|e| e.name() == "sz")
            .collect();
        require(sizes.len() == 1)?;
        let start = source_start(xml, sizes[0]).ok_or_else(invalid)?;
        let mut matched = false;
        for a in start.attributes() {
            let a = a.map_err(|_| invalid())?;
            if a.key.as_ref() == "height" {
                let normalized = a
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                    .map_err(|_| invalid())?;
                let std::borrow::Cow::Borrowed(raw) = a.value else {
                    return Err(invalid());
                };
                let offset = (raw.as_ptr() as usize)
                    .checked_sub(xml.as_ptr() as usize)
                    .ok_or_else(invalid)?;
                require(
                    height.value == (offset..offset + raw.len())
                        && normalized.parse::<u32>().is_ok_and(|n| n > 0),
                )?;
                matched = true;
            }
        }
        require(matched)?;
        edits.push((height.value.clone(), "0".into()));
    }
    Ok(())
}
