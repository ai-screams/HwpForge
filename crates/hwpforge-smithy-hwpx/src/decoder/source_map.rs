//! Decoder-owned wire ↔ Core provenance.
//!
//! The XML reader numbers every element and retains its original byte range.
//! Conversion records those identities when it emits Core runs, including
//! normalization and Memo folding. No consumer may reconstruct slot identity
//! by interpreting raw XML. `SectionPreservation.text_slots` is a projection;
//! its public format and version stay unchanged.
//!
//! New editors query `SourceMap::node` for original bytes and FieldInventory
//! for run/paragraph/insertion preflight, then add their consumer column to
//! the field representation matrix. Nodes also prepare row/paragraph editing
//! and byte-preserving re-encoding of unchanged subtrees.
use crate::exchange::{PreservedTextSlot, TextLocator};
use crate::{HwpxError, HwpxResult};
use hwpforge_core::field_inventory::FieldInventory;
use hwpforge_core::node_path::{CorePath, NodeKind};
use hwpforge_core::source_xml::{UnclosedOrigin, XmlSource};
use hwpforge_core::{Control, Paragraph, RunContent, Section};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;

/// An original node location in a package part.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SourceNode {
    path: CorePath,
    kind: NodeKind,
    part: String,
    ranges: Vec<Range<usize>>,
    copy: SourceCoverage,
    envelopes: Vec<usize>,
}
impl SourceNode {
    /// Shared Core address.
    pub fn path(&self) -> &CorePath {
        &self.path
    }
    /// Semantic node category.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }
    /// Package part path.
    pub fn part(&self) -> &str {
        &self.part
    }
    /// Contributing elements in source order. May overlap or contain each
    /// other; do not concatenate these. Use `copy_source` for copying.
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }
    /// Complete normalized copy fragments, or explicit absence of recording.
    pub fn copy_source(&self) -> &SourceCoverage {
        &self.copy
    }
}
/// Whether the decoder recorded all source contributions for a Core node.
/// Copy ranges are ordered, disjoint unions of whole source elements. Nested
/// and duplicate contributions are emitted once. They are XML fragments, not
/// necessarily a standalone document; ancestor namespace bindings still apply.
/// A missing record never means an empty node or permission to copy nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceCoverage {
    /// All contributions are known, including folded descendants.
    Complete(Vec<Range<usize>>),
    /// Some contributions are missing, or a wire run is shared with another
    /// Core node. These ranges cannot independently replace this Core node.
    Incomplete(Vec<Range<usize>>),
    /// This parser path did not record the node (for example external masters).
    Unrecorded,
}
#[derive(Debug, Clone)]
pub(crate) struct ParagraphSource {
    pub(crate) outer: Range<usize>,
    pub(crate) id_value: Option<Range<usize>>,
    pub(crate) prefix: String,
    pub(crate) line_segments: Vec<Range<usize>>,
    pub(crate) has_properties: bool,
}
/// Immutable provenance emitted by the decoder.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SourceMap {
    nodes: BTreeMap<CorePath, SourceNode>,
    slots: Vec<PreservedTextSlot>,
    // Core validation placeholders without a wire run, not editable wire slots.
    pub(crate) synthetic_empty_texts: BTreeSet<String>,
    pub(crate) text_elements: BTreeMap<usize, hwpforge_core::source_xml::XmlElement>,
    pub(crate) paragraphs: Vec<ParagraphSource>,
    pub(crate) unclosed: Vec<UnclosedOrigin>,
    pub(crate) cache_flow: crate::cache_flow::ParagraphFlow,
}
impl SourceMap {
    /// Locate an original node without reparsing its source.
    pub fn node(&self, path: &CorePath) -> Option<&SourceNode> {
        self.nodes.get(path)
    }
    /// All recorded Core nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &SourceNode> {
        self.nodes.values()
    }
    /// All decoder-recorded text addresses and original payloads, including protected fields.
    /// Presence records provenance; it does not grant editing permission.
    pub fn text_slots(&self) -> &[PreservedTextSlot] {
        &self.slots
    }
}
#[derive(Debug, Clone, Default)]
pub(crate) struct RunOrigin {
    pub(crate) node: usize,
    pub(crate) synthetic_empty: bool,
    pub(crate) texts: Vec<usize>,
    pub(crate) anchors: Vec<RunOrigin>,
    pub(crate) boundaries: Vec<usize>,
}
#[derive(Debug, Default)]
struct ParagraphOrigin {
    node: usize,
    runs: Vec<RunOrigin>,
}
#[derive(Debug, Default)]
pub(crate) struct SourceRecorder {
    source: XmlSource,
    paragraphs: HashMap<usize, ParagraphOrigin>,
}
impl SourceRecorder {
    /// Tokenize once at the decoder boundary. The private attribute carries
    /// identity through serde; offsets always address the untouched XML.
    pub(crate) fn read(xml: &str) -> HwpxResult<(String, Self)> {
        let (annotated, source) = XmlSource::read(xml)
            .map_err(|detail| HwpxError::XmlParse { file: "section.xml".into(), detail })?;
        Ok((annotated, Self { source, paragraphs: HashMap::new() }))
    }
    pub(crate) fn child(&self, node: usize, tag: &str, index: usize) -> usize {
        self.source.child(node, tag, index)
    }
    /// Restore cross-type parameter order from the same decoder element ledger.
    pub(crate) fn order_parameters(
        &self,
        begin: usize,
        parameters: &mut Vec<hwpforge_core::control::FieldParameter>,
    ) {
        use hwpforge_core::control::FieldParameter as P;
        let container = self.child(begin, "parameters", 0);
        let Some(container) = self.source.element(container) else { return };
        let mut integers = std::collections::VecDeque::new();
        let mut strings = std::collections::VecDeque::new();
        let mut booleans = std::collections::VecDeque::new();
        for p in std::mem::take(parameters) {
            match p {
                P::Integer { .. } => integers.push_back(p),
                P::String { .. } => strings.push_back(p),
                P::Boolean { .. } => booleans.push_back(p),
                _ => parameters.push(p),
            }
        }
        for id in container.children() {
            let p = match self.source.element(*id).map(|e| e.name()) {
                Some("integerParam") => integers.pop_front(),
                Some("stringParam") => strings.pop_front(),
                Some("booleanParam") => booleans.pop_front(),
                _ => None,
            };
            if let Some(p) = p {
                parameters.push(p);
            }
        }
    }
    pub(crate) fn record_paragraph(
        &mut self,
        source: usize,
        paragraph: &Paragraph,
        runs: Vec<RunOrigin>,
    ) {
        // Vec moves preserve allocation identity. This key is consumed before
        // decoded Core leaves the decoder; no pointer escapes in public metadata.
        self.paragraphs
            .insert(paragraph.runs.as_ptr() as usize, ParagraphOrigin { node: source, runs });
    }
    pub(crate) fn finish(&self, section: &Section, section_index: usize, part: &str) -> SourceMap {
        let mut result = SourceMap {
            cache_flow: crate::cache_flow::ParagraphFlow::from_source(&self.source),
            ..SourceMap::default()
        };
        FieldInventory::visit_flows(std::slice::from_ref(section), |_, paragraphs, prefix| {
            for (pi, paragraph) in paragraphs.iter().enumerate() {
                let path = format!("{prefix}[{pi}]");
                let Some(origin) = self.paragraphs.get(&(paragraph.runs.as_ptr() as usize)) else {
                    self.node(&mut result, section_index, &path, NodeKind::Paragraph, "", &[]);
                    for (ri, run) in paragraph.runs.iter().enumerate() {
                        self.record_run(
                            &mut result,
                            section_index,
                            &format!("{path}.runs[{ri}]"),
                            &run.content,
                            &RunOrigin::default(),
                            "",
                        );
                    }
                    continue;
                };
                if prefix == "paragraphs" {
                    if let Some(element) = self.source.element(origin.node) {
                        let children: Vec<_> = element
                            .children()
                            .iter()
                            .filter_map(|id| self.source.element(*id))
                            .collect();
                        let has_properties = children.iter().any(|e| {
                            e.name() == "run"
                                && e.children().iter().any(|id| {
                                    self.source.element(*id).is_some_and(|e| e.name() == "secPr")
                                })
                        });
                        result.paragraphs.push(ParagraphSource {
                            outer: element.outer(),
                            id_value: element.id_value(),
                            prefix: element
                                .qualified_name()
                                .split_once(':')
                                .map_or("", |(prefix, _)| prefix)
                                .into(),
                            line_segments: children
                                .iter()
                                .filter(|e| e.name() == "linesegarray")
                                .map(|e| e.outer())
                                .collect(),
                            has_properties,
                        });
                    }
                }
                self.node(
                    &mut result,
                    section_index,
                    &path,
                    NodeKind::Paragraph,
                    part,
                    &[origin.node],
                );
                if let Some(container) = prefix.strip_suffix(".paragraphs") {
                    let kind = if container.ends_with(".caption") {
                        Some(NodeKind::Caption)
                    } else if container.ends_with(".textbox") {
                        Some(NodeKind::TextBox)
                    } else if container.ends_with(".footnote") || container.ends_with(".endnote") {
                        Some(NodeKind::Note)
                    } else if container.starts_with("header") || container.starts_with("footer") {
                        Some(NodeKind::HeaderFooter)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        let mut id = self.source.element(origin.node).map_or(0, |e| e.parent());
                        if self.source.element(id).is_some_and(|e| e.name() == "subList") {
                            id = self.source.element(id).unwrap().parent();
                        }
                        if kind == NodeKind::TextBox
                            && self.source.element(id).is_some_and(|e| e.name() == "drawText")
                        {
                            id = self.source.element(id).unwrap().parent();
                        }
                        self.node(&mut result, section_index, container, kind, part, &[id]);
                    }
                }
                for (ri, (run, source)) in paragraph.runs.iter().zip(&origin.runs).enumerate() {
                    let path = format!("{path}.runs[{ri}]");
                    if source.synthetic_empty && self.source.element(origin.node).is_some() {
                        result.synthetic_empty_texts.insert(format!("{path}.text"));
                    }
                    self.record_run(&mut result, section_index, &path, &run.content, source, part);
                }
            }
        });
        // An ancestor may own a whole folded field; its child cannot copy
        // that run independently when it also contains the ancestor's markers.
        let mut owners: HashMap<usize, Vec<&str>> = HashMap::new();
        let mut incomplete = std::collections::HashSet::new();
        for node in
            result.nodes.values().filter(|n| matches!(n.kind, NodeKind::Run | NodeKind::MemoAnchor))
        {
            for id in &node.envelopes {
                let previous = owners.entry(*id).or_default();
                for owner in previous.iter() {
                    let path = node.path.relative();
                    if path.strip_prefix(owner).is_some_and(|r| r.starts_with('.')) {
                        incomplete.insert(path.to_owned());
                    } else if owner.strip_prefix(path).is_some_and(|r| r.starts_with('.')) {
                        incomplete.insert((*owner).to_owned());
                    } else {
                        incomplete.insert(path.to_owned());
                        incomplete.insert((*owner).to_owned());
                    }
                }
                previous.push(node.path.relative());
            }
        }
        for node in result.nodes.values_mut() {
            let mut path = node.path.relative();
            let shared = loop {
                if incomplete.contains(path) {
                    break true;
                }
                let Some((parent, _)) = path.rsplit_once('.') else {
                    break false;
                };
                path = parent;
            };
            if shared {
                if let SourceCoverage::Complete(ranges) = &node.copy {
                    node.copy = SourceCoverage::Incomplete(ranges.clone());
                }
            }
        }
        result
    }
    fn node(
        &self,
        map: &mut SourceMap,
        section: usize,
        path: &str,
        kind: NodeKind,
        part: &str,
        ids: &[usize],
    ) {
        let ranges: Vec<_> = ids
            .iter()
            .filter(|id| **id != 0)
            .filter_map(|id| self.source.element(*id))
            .map(|e| e.outer())
            .collect();
        let mut envelopes = Vec::new();
        if matches!(kind, NodeKind::Run | NodeKind::Control | NodeKind::MemoAnchor) {
            for id in ids {
                let mut id = *id;
                while let Some(element) = self.source.element(id) {
                    if element.name() == "run" {
                        envelopes.push(id);
                        break;
                    }
                    id = element.parent();
                }
            }
            envelopes.sort_unstable();
            envelopes.dedup();
        }
        let mut normalized = if envelopes.is_empty() {
            ranges.clone()
        } else {
            envelopes.iter().filter_map(|id| self.source.element(*id)).map(|e| e.outer()).collect()
        };
        normalized.sort_by_key(|r| (r.start, r.end));
        let mut copy: Vec<Range<usize>> = Vec::new();
        for range in normalized {
            if let Some(last) = copy.last_mut().filter(|last| range.start <= last.end) {
                last.end = last.end.max(range.end);
            } else {
                copy.push(range);
            }
        }
        let copy = if copy.is_empty() {
            SourceCoverage::Unrecorded
        } else if ids.contains(&0) {
            SourceCoverage::Incomplete(copy)
        } else {
            SourceCoverage::Complete(copy)
        };
        let path = CorePath::new(section, path);
        map.nodes.insert(
            path.clone(),
            SourceNode { path, kind, part: part.into(), ranges, copy, envelopes },
        );
    }
    fn record_run(
        &self,
        map: &mut SourceMap,
        section: usize,
        path: &str,
        content: &RunContent,
        source: &RunOrigin,
        part: &str,
    ) {
        fn contributions(source: &RunOrigin, ids: &mut Vec<usize>) {
            ids.push(source.node);
            ids.extend(&source.boundaries);
            ids.extend(&source.texts);
            for anchor in &source.anchors {
                contributions(anchor, ids);
            }
        }
        let mut contributing = Vec::new();
        contributions(source, &mut contributing);
        contributing.sort_unstable();
        contributing.dedup();
        self.node(map, section, path, NodeKind::Run, part, &contributing);
        let (suffix, text) = match content {
            RunContent::Text(t) => (".text", Some(t.clone())),
            RunContent::InlineText(t) => (".text", Some(t.plain_text())),
            RunContent::Control(c) => {
                self.node(
                    map,
                    section,
                    &format!("{path}.control"),
                    NodeKind::Control,
                    part,
                    &contributing,
                );
                match c.as_ref() {
                    Control::Field { display_text, .. } => {
                        (".control.field", Some(display_text.clone()))
                    }
                    Control::FieldBoundary { .. } => {
                        if let Some(origin) = self.source.unclosed_origin(
                            source.node,
                            CorePath::new(section, path),
                            part,
                        ) {
                            map.unclosed.push(origin);
                        }
                        self.node(
                            map,
                            section,
                            &format!("{path}.control.boundary"),
                            NodeKind::FieldBoundary,
                            part,
                            &[source.node],
                        );
                        ("", None)
                    }
                    Control::Memo { anchor_runs, .. } => {
                        self.node(
                            map,
                            section,
                            &format!("{path}.control.memo"),
                            NodeKind::Control,
                            part,
                            &contributing,
                        );
                        for (i, (anchor, origin)) in
                            anchor_runs.iter().zip(&source.anchors).enumerate()
                        {
                            let anchor_path = format!("{path}.control.memo.anchor[0].runs[{i}]");
                            self.record_run(
                                map,
                                section,
                                &anchor_path,
                                &anchor.content,
                                origin,
                                part,
                            );
                            self.node(
                                map,
                                section,
                                &anchor_path,
                                NodeKind::MemoAnchor,
                                part,
                                &origin.texts,
                            );
                        }
                        ("", None)
                    }
                    Control::Hyperlink { text, .. } => (".control.display", Some(text.clone())),
                    other => (
                        ".control.display",
                        hwpforge_core::field_view::scalar_text(other).map(str::to_owned),
                    ),
                }
            }
            RunContent::Table(t) => {
                let table_path = format!("{path}.table");
                self.node(map, section, &table_path, NodeKind::Table, part, &[source.node]);
                for (r, row) in t.rows.iter().enumerate() {
                    let row_id = self.child(source.node, "tr", r);
                    let row_path = format!("{table_path}.rows[{r}]");
                    self.node(map, section, &row_path, NodeKind::Row, part, &[row_id]);
                    for c in 0..row.cells.len() {
                        self.node(
                            map,
                            section,
                            &format!("{row_path}.cells[{c}]"),
                            NodeKind::Cell,
                            part,
                            &[self.child(row_id, "tc", c)],
                        );
                    }
                }
                ("", None)
            }
            _ => ("", None),
        };
        let Some(text) = text else {
            return;
        };
        let path = format!("{path}{suffix}");
        let text_sources =
            if source.texts.is_empty() { vec![source.node] } else { source.texts.clone() };
        self.node(map, section, &path, NodeKind::Text, part, &text_sources);
        if suffix == ".control.display" {
            return;
        }
        let locator = if source.texts.len() == 1 {
            let Some(e) = self.source.element(source.texts[0]).filter(|_| source.texts[0] != 0)
            else {
                return;
            };
            TextLocator::TextElement {
                element_start: e.outer().start,
                element_end: e.outer().end,
                content_start: e.inner().as_ref().map(|r| r.start),
                content_end: e.inner().as_ref().map(|r| r.end),
            }
        } else if text.is_empty() && source.texts.is_empty() {
            let Some(e) = self.source.element(source.node).filter(|e| e.name() == "run") else {
                return;
            };
            TextLocator::EmptyRun { run_start: e.outer().start, run_end: e.outer().end }
        } else {
            return;
        };
        let has_inline_markup = source
            .texts
            .iter()
            .any(|id| self.source.element(*id).is_some_and(|e| !e.children().is_empty()));
        let id = if source.texts.len() == 1 { source.texts[0] } else { source.node };
        if let Some(element) = self.source.element(id) {
            map.text_elements.insert(element.outer().start, element.clone());
        }
        map.slots.push(PreservedTextSlot { path, original_text: text, has_inline_markup, locator });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 이것을 실패시키는 것: 일부 기여분의 원본 ID가 없는데 Complete로 보고하기.
    #[test]
    fn missing_contribution_is_incomplete() {
        let (_, recorder) = SourceRecorder::read("<run><t>x</t></run>").unwrap();
        let mut map = SourceMap::default();
        recorder.node(&mut map, 0, "paragraphs[0].runs[0]", NodeKind::Run, "section.xml", &[1, 0]);
        assert!(matches!(
            map.nodes.values().next().unwrap().copy_source(),
            SourceCoverage::Incomplete(_)
        ));
    }
}
