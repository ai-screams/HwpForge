//! Dependencies of the bytes the encoder actually emits. Core is only used
//! by the caller to locate trusted caches, never to enumerate style references.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use quick_xml::{events::Event, name::ResolveResult, NsReader};
use sha2::{Digest, Sha256};

use crate::{decoder::PathSeg, patch::RawPackage, HwpxError, HwpxResult};

#[path = "inputs.rs"]
mod inputs;

/// Wire definition kind and identifier (font kinds include their language).
pub type DependencyKey = (String, String);
type Key = DependencyKey;
type Fingerprint = [u8; 32];

#[derive(Default, Clone)]
struct Node {
    name: String,
    namespace: String,
    attrs: BTreeMap<String, String>,
    text: Vec<String>,
    preserve_space: bool,
    unresolved_links: bool,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map_or("", String::as_str)
    }

    fn parse(xml: &str) -> HwpxResult<Self> {
        fn expanded(ns: ResolveResult<'_>, local: &str) -> HwpxResult<String> {
            match ns {
                ResolveResult::Bound(ns) => Ok(format!("{{{}}}{local}", ns.as_ref())),
                ResolveResult::Unbound => Ok(local.into()),
                ResolveResult::Unknown(prefix) => Err(HwpxError::InvalidStructure {
                    detail: format!("unbound XML prefix: {}", prefix),
                }),
            }
        }
        fn record_text(n: &mut Node, text: &str) {
            n.text.resize(n.children.len() + 1, String::new());
            n.text[n.children.len()].push_str(text);
        }
        let mut reader = NsReader::from_str(xml);
        let mut stack = vec![Self::default()];
        let error =
            |e: String| HwpxError::XmlParse { file: "layout fingerprint".into(), detail: e };
        loop {
            let event = reader.read_event().map_err(|e| error(e.to_string()))?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let (ns, local) = reader.resolver().resolve_element(e.name());
                    let mut node = Self {
                        name: local.as_ref().to_string(),
                        namespace: expanded(ns, "")?,
                        preserve_space: stack.last().unwrap().preserve_space,
                        ..Self::default()
                    };
                    for attr in e.attributes() {
                        let attr = attr.map_err(|e| error(e.to_string()))?;
                        if attr.key.as_ref() == "xmlns" || attr.key.as_ref().starts_with("xmlns:") {
                            continue;
                        }
                        let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                        let key = expanded(ns, local.as_ref())?;
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|e| error(e.to_string()))?
                            .into_owned();
                        if key == "{http://www.w3.org/XML/1998/namespace}space" {
                            node.preserve_space = value == "preserve";
                        }
                        node.attrs.insert(key, value);
                    }
                    if empty {
                        stack.last_mut().unwrap().children.push(node);
                    } else {
                        stack.push(node);
                    }
                }
                Event::End(_) => {
                    let node = stack.pop().ok_or_else(|| error("unbalanced XML".into()))?;
                    stack
                        .last_mut()
                        .ok_or_else(|| error("unbalanced XML".into()))?
                        .children
                        .push(node);
                }
                Event::Text(e) => record_text(stack.last_mut().unwrap(), &e.xml10_content()),
                Event::CData(e) => record_text(stack.last_mut().unwrap(), &e.xml10_content()),
                Event::GeneralRef(e) => {
                    let escaped = format!("&{};", e.as_ref());
                    let value =
                        quick_xml::escape::unescape(&escaped).map_err(|e| error(e.to_string()))?;
                    record_text(stack.last_mut().unwrap(), &value);
                }
                Event::DocType(_) => return Err(error("DTD is not a cache-proof input".into())),
                Event::Eof => break,
                _ => {}
            }
        }
        if stack.len() != 1 {
            return Err(error("unclosed XML".into()));
        }
        let mut root = stack.pop().unwrap();
        canonicalize_field_links(&mut root);
        Ok(root)
    }
}

// Field spans use local identities rather than header definitions. Keep the
// graph by assigning definition-order ordinals, including forward references.
// A missing/duplicate begin is unresolved, never an equal pair of broken links.
fn canonicalize_field_links(root: &mut Node) {
    fn collect(n: &Node, ids: &mut HashMap<String, Option<usize>>, next: &mut usize) {
        if n.name == "fieldBegin" {
            if let Some(id) = n.attrs.get("id") {
                ids.entry(id.clone()).and_modify(|v| *v = None).or_insert(Some(*next));
                *next += 1;
            }
        }
        for child in &n.children {
            collect(child, ids, next);
        }
    }
    fn rewrite(n: &mut Node, ids: &HashMap<String, Option<usize>>) {
        for key in ["id", "beginIDRef"] {
            if key == "id" && n.name != "fieldBegin" {
                continue;
            }
            if let Some(value) = n.attrs.get_mut(key) {
                if let Some(Some(ordinal)) = ids.get(value) {
                    *value = format!("field:{ordinal}");
                } else {
                    n.unresolved_links = true;
                }
            }
        }
        for child in &mut n.children {
            rewrite(child, ids);
        }
    }
    let mut ids = HashMap::new();
    collect(root, &mut ids, &mut 0);
    rewrite(root, &ids);
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Signals {
    pub numbering: bool,
    pub backwards: bool,
    pub floating: bool,
    pub linked: bool,
    pub unknown: bool,
    pub unresolved: bool,
    pub document_input: bool,
}

impl Signals {
    pub fn merge(&mut self, other: Self) {
        self.numbering |= other.numbering;
        self.backwards |= other.backwards;
        self.floating |= other.floating;
        self.linked |= other.linked;
        self.unknown |= other.unknown;
        self.unresolved |= other.unresolved;
        self.document_input |= other.document_input;
    }

    fn scan(&mut self, n: &Node) {
        self.document_input |=
            matches!(n.name.as_str(), "fieldBegin" | "pageNum" | "pageNumCtrl" | "pageNumPos")
                || (n.name == "autoNum" && matches!(n.attr("numType"), "PAGE" | "TOTAL_PAGE"))
                || (n.name == "stringParam" && n.text.iter().any(|s| s.contains('$')));
        // Placement policy is consumed after encoding: no object-kind list.
        self.floating |= n.attr("treatAsChar") == "0";
        // Conservative section closure includes the preceding keep chain,
        // widow/orphan reflow and nested hosts, including insertions/deletions.
        self.linked |= n.attr("keepWithNext") == "1" || n.attr("widowOrphan") == "1";
        self.numbering |= matches!(n.name.as_str(), "autoNum" | "newNum" | "footNote" | "endNote")
            || (n.name == "heading" && matches!(n.attr("type"), "OUTLINE" | "NUMBER"));
        self.backwards |=
            matches!(n.name.as_str(), "pageNum" | "pageNumCtrl" | "pageNumPos" | "fieldBegin")
                || (n.name == "autoNum" && matches!(n.attr("numType"), "PAGE" | "TOTAL_PAGE"))
                || n.attr("showLineNumber") == "1";
        self.numbering |= self.backwards;
        // Only these control policies are known to be local or accounted for
        // above. A new encoder control therefore cannot silently opt in.
        if n.name == "ctrl" {
            self.unknown |= n.children.iter().any(|c| {
                !matches!(
                    c.name.as_str(),
                    "colPr"
                        | "header"
                        | "footer"
                        | "footNote"
                        | "endNote"
                        | "autoNum"
                        | "newNum"
                        | "pageNum"
                        | "pageNumCtrl"
                        | "pageNumPos"
                        | "fieldBegin"
                        | "fieldEnd"
                        | "bookmark"
                )
            });
        }
        if n.name == "run" {
            self.unknown |= n.children.iter().any(|c| {
                !matches!(
                    c.name.as_str(),
                    "t" | "ctrl"
                        | "secPr"
                        | "tbl"
                        | "pic"
                        | "rect"
                        | "ellipse"
                        | "polygon"
                        | "line"
                        | "arc"
                        | "curve"
                        | "connectLine"
                        | "container"
                        | "equation"
                        | "textart"
                        | "switch"
                        | "compose"
                        | "dutmal"
                )
            });
        }
        let known_children: Option<&[&str]> = match n.name.as_str() {
            "p" => Some(&["run", "linesegarray"]),
            "t" => Some(&[
                "tab",
                "lineBreak",
                "hyphen",
                "nbSpace",
                "fwSpace",
                "markpenBegin",
                "markpenEnd",
                "titleMark",
            ]),
            "switch" => Some(&["case", "default"]),
            "case" | "default" => Some(&["chart", "ole", "margin", "lineSpacing"]),
            "paraPr" => Some(&[
                "align",
                "heading",
                "breakSetting",
                "autoSpacing",
                "switch",
                "margin",
                "lineSpacing",
                "border",
            ]),
            "charPr" => Some(&[
                "fontRef",
                "ratio",
                "spacing",
                "relSz",
                "offset",
                "bold",
                "italic",
                "underline",
                "strikeout",
                "outline",
                "shadow",
                "emboss",
                "engrave",
                "supscript",
                "subscript",
            ]),
            _ => None,
        };
        if let Some(known) = known_children {
            self.unknown |= n.children.iter().any(|c| !known.contains(&c.name.as_str()));
        }
        if n.name == "breakSetting" {
            self.unknown |= n.attrs.keys().any(|a| {
                !matches!(
                    a.as_str(),
                    "breakLatinWord"
                        | "breakNonLatinWord"
                        | "widowOrphan"
                        | "keepWithNext"
                        | "keepLines"
                        | "pageBreakBefore"
                        | "lineWrap"
                )
            });
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    hash: Fingerprint,
    resolved: bool,
    facts: BTreeMap<String, BTreeSet<String>>,
    dependencies: BTreeSet<Key>,
}

struct Definitions {
    nodes: HashMap<Key, Option<Node>>,
    memo: HashMap<Key, (Stamp, Signals)>,
    visiting: HashSet<Key>,
    cycle_ids: HashMap<Key, usize>,
    components: HashMap<Key, std::sync::Arc<Vec<Key>>>,
    #[cfg(test)]
    evaluated: usize,
}

impl Definitions {
    fn new(header: &Node) -> Self {
        fn index(
            n: &Node,
            language: &str,
            into: &mut HashMap<Key, Option<Node>>,
            counts: &mut HashMap<String, u32>,
        ) {
            let language = if n.name == "fontface" { n.attr("lang") } else { language };
            if n.attrs.contains_key("id") {
                let kind =
                    if n.name == "font" { format!("font:{language}") } else { n.name.clone() };
                // Hancom W3: definition labels do not select the definition.
                let id = if matches!(
                    n.name.as_str(),
                    "font"
                        | "charPr"
                        | "paraPr"
                        | "style"
                        | "tabPr"
                        | "borderFill"
                        | "numbering"
                        | "bullet"
                        | "memoPr"
                        | "trackChange"
                        | "trackChangeAuthor"
                ) {
                    let base = u32::from(matches!(
                        n.name.as_str(),
                        "borderFill"
                            | "numbering"
                            | "bullet"
                            | "memoPr"
                            | "trackChange"
                            | "trackChangeAuthor"
                    ));
                    let position = counts.entry(kind.clone()).or_insert(base);
                    let id = position.to_string();
                    *position += 1;
                    id
                } else {
                    n.attr("id").into()
                };
                into.entry((kind, id)).and_modify(|v| *v = None).or_insert_with(|| Some(n.clone()));
            }
            for child in &n.children {
                index(child, language, into, counts);
            }
        }
        let mut nodes = HashMap::new();
        index(header, "", &mut nodes, &mut HashMap::new());
        let style_count = nodes.keys().filter(|(kind, _)| kind == "style").count();
        for ((kind, position), node) in &mut nodes {
            if kind == "style" {
                if let Some(node) = node {
                    if node
                        .attr("nextStyleIDRef")
                        .parse::<usize>()
                        .is_ok_and(|id| id >= style_count)
                    {
                        // Same explicit decoder recovery for Hancom's stale nextStyleIDRef.
                        node.attrs.insert("nextStyleIDRef".into(), position.clone());
                    }
                }
            }
        }
        let (components, order) = components(&nodes);
        let mut definitions = Self {
            nodes,
            memo: HashMap::new(),
            visiting: HashSet::new(),
            cycle_ids: HashMap::new(),
            components,
            #[cfg(test)]
            evaluated: 0,
        };
        // Leaves first: even an arbitrarily long next-style chain does not
        // recurse on the Rust stack during fingerprint construction.
        for key in order.into_iter().rev() {
            definitions.resolve(key);
        }
        definitions
    }

    fn resolve(&mut self, key: Key) -> (Stamp, Signals) {
        if self.visiting.contains(&key) {
            if let Some(id) = self.cycle_ids.get(&key) {
                let hash = Sha256::digest(format!("cycle-edge:{id}")).into();
                return (
                    Stamp {
                        hash,
                        resolved: true,
                        facts: BTreeMap::new(),
                        dependencies: BTreeSet::new(),
                    },
                    Signals::default(),
                );
            }
            let id = self.cycle_ids.len();
            self.cycle_ids.insert(key.clone(), id);
            return self.resolve_node(&key);
        }
        if let Some(result) = self.memo.get(&key) {
            return result.clone();
        }
        let members = self
            .components
            .get(&key)
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(vec![key.clone()]));
        // A rooted, ordered traversal gives each cycle edge a traversal ordinal.
        // Neither the raw id nor SCC member sorting contributes to its stamp.
        // Bound recursive SCC traversal; large cycles fail closed.
        if members.len() > 256 {
            return unresolved();
        }
        #[cfg(test)]
        {
            self.evaluated += members.len();
        }
        let outer_visiting = std::mem::take(&mut self.visiting);
        let outer_ids = std::mem::take(&mut self.cycle_ids);
        self.visiting.extend(members.iter().cloned());
        for root in members.iter() {
            self.cycle_ids.clear();
            self.cycle_ids.insert(root.clone(), 0);
            let (mut stamp, flags) = self.resolve_node(root);
            // Distinguish an edge inside this SCC from an edge into another
            // component. Its size is already encoded by the rooted traversal.
            stamp.hash =
                Sha256::digest([b"component:".as_slice(), stamp.hash.as_slice()].concat()).into();
            self.memo.insert(root.clone(), (stamp, flags));
        }
        self.visiting = outer_visiting;
        self.cycle_ids = outer_ids;
        self.memo[&key].clone()
    }

    fn resolve_node(&mut self, key: &Key) -> (Stamp, Signals) {
        if let Some(Some(node)) = self.nodes.get(key).cloned() {
            self.stamp(&node, false)
        } else {
            unresolved()
        }
    }

    fn stamp(&mut self, n: &Node, shell: bool) -> (Stamp, Signals) {
        let mut hash = Sha256::new();
        let mut resolved = true;
        let mut signals = Signals::default();
        let mut facts = BTreeMap::new();
        let mut dependencies = BTreeSet::new();
        fn field(hash: &mut Sha256, s: &str) {
            hash.update(s.len().to_le_bytes());
            hash.update(s);
        }
        #[allow(clippy::too_many_arguments)]
        fn walk(
            d: &mut Definitions,
            n: &Node,
            shell: bool,
            root: bool,
            hash: &mut Sha256,
            resolved: &mut bool,
            signals: &mut Signals,
            facts: &mut BTreeMap<String, BTreeSet<String>>,
            dependencies: &mut BTreeSet<Key>,
        ) {
            if n.name == "linesegarray"
                || (shell && !root && matches!(n.name.as_str(), "p" | "header" | "footer"))
            {
                return;
            }
            field(hash, &n.name);
            field(hash, &n.namespace);
            facts.entry(format!("{}#element", n.name)).or_default().insert(n.namespace.clone());
            for (key, value) in &n.attrs {
                // Definition ids and nonreferenced paragraph/object identities do not
                // affect layout. Reference values are replaced by their target below.
                if key == "id" && neutral_identity(&n.name) {
                    continue;
                }
                if is_resolved_reference(n, key) {
                    field(hash, key);
                    continue;
                }
                field(hash, key);
                field(hash, value);
                facts.entry(format!("{}@{key}", n.name)).or_default().insert(value.clone());
            }
            let mixed = n.preserve_space
                || matches!(n.name.as_str(), "t" | "stringParam" | "script" | "meta" | "title")
                || n.text.iter().any(|t| !t.trim().is_empty());
            if mixed {
                for (i, text) in n.text.iter().enumerate().filter(|(_, t)| !t.is_empty()) {
                    field(hash, &i.to_string());
                    field(hash, text);
                    facts.entry(format!("{}#text", n.name)).or_default().insert(text.clone());
                }
            }
            *resolved &= !n.unresolved_links;
            signals.unresolved |= n.unresolved_links;
            signals.scan(n);
            for reference in references(n) {
                match reference {
                    Ok(key) => {
                        let (stamp, deps) = d.resolve(key.clone());
                        field(hash, &key.0);
                        hash.update(stamp.hash);
                        merge_facts(facts, &stamp.facts);
                        dependencies.insert(key);
                        dependencies.extend(stamp.dependencies);
                        *resolved &= stamp.resolved;
                        signals.merge(deps);
                    }
                    Err(()) => {
                        *resolved = false;
                        signals.unresolved = true;
                    }
                }
            }
            for child in &n.children {
                walk(d, child, shell, false, hash, resolved, signals, facts, dependencies);
            }
            field(hash, "/");
        }
        walk(
            self,
            n,
            shell,
            true,
            &mut hash,
            &mut resolved,
            &mut signals,
            &mut facts,
            &mut dependencies,
        );
        (Stamp { hash: hash.finalize().into(), resolved, facts, dependencies }, signals)
    }
}

fn unresolved() -> (Stamp, Signals) {
    (
        Stamp {
            hash: [0; 32],
            resolved: false,
            facts: BTreeMap::new(),
            dependencies: BTreeSet::new(),
        },
        Signals { unresolved: true, ..Signals::default() },
    )
}

fn merge_facts(
    into: &mut BTreeMap<String, BTreeSet<String>>,
    from: &BTreeMap<String, BTreeSet<String>>,
) {
    for (key, values) in from {
        into.entry(key.clone()).or_default().extend(values.iter().cloned());
    }
}

// Only known definitions and nonreferenced object identities are neutral.
// An unfamiliar id may itself be a layout input; do not erase its value.
fn neutral_identity(name: &str) -> bool {
    matches!(
        name,
        "font"
            | "charPr"
            | "paraPr"
            | "style"
            | "numbering"
            | "bullet"
            | "borderFill"
            | "tabPr"
            | "memoPr"
            | "binary"
            | "part"
            | "p"
            | "tbl"
            | "pic"
            | "rect"
            | "ellipse"
            | "polygon"
            | "line"
            | "arc"
            | "curve"
            | "connectLine"
            | "container"
            | "equation"
            | "textart"
            | "chart"
            | "ole"
    )
}

fn is_resolved_reference(n: &Node, attr: &str) -> bool {
    if n.name == "fontRef" {
        return true;
    }
    let single = Node {
        name: n.name.clone(),
        attrs: n
            .attrs
            .iter()
            .filter(|(k, _)| k.as_str() == attr || k.as_str() == "type")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        ..Node::default()
    };
    !references(&single).is_empty()
}

/// Iterative Kosaraju: definition cycles are resolved once as a group, and
/// reference lookup thereafter is constant time. No repeated font scans.
type Components = HashMap<Key, std::sync::Arc<Vec<Key>>>;

fn components(nodes: &HashMap<Key, Option<Node>>) -> (Components, Vec<Key>) {
    fn edges(n: &Node, into: &mut Vec<Key>) {
        into.extend(references(n).into_iter().filter_map(Result::ok));
        for child in &n.children {
            edges(child, into);
        }
    }
    let mut graph = HashMap::new();
    let mut reverse: HashMap<Key, Vec<Key>> = HashMap::new();
    for (key, node) in nodes {
        let mut deps = Vec::new();
        if let Some(node) = node {
            edges(node, &mut deps);
        }
        deps.retain(|key| nodes.contains_key(key));
        for dep in &deps {
            reverse.entry(dep.clone()).or_default().push(key.clone());
        }
        graph.insert(key.clone(), deps);
    }
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    for root in nodes.keys() {
        let mut stack = vec![(root.clone(), false)];
        while let Some((key, closing)) = stack.pop() {
            if closing {
                order.push(key);
                continue;
            }
            if !visited.insert(key.clone()) {
                continue;
            }
            stack.push((key.clone(), true));
            stack.extend(graph[&key].iter().map(|k| (k.clone(), false)));
        }
    }
    visited.clear();
    let mut result = HashMap::new();
    let mut representatives = Vec::new();
    for root in order.into_iter().rev() {
        if visited.contains(&root) {
            continue;
        }
        let mut stack = vec![root];
        let mut members = Vec::new();
        while let Some(key) = stack.pop() {
            if !visited.insert(key.clone()) {
                continue;
            }
            if let Some(parents) = reverse.get(&key) {
                stack.extend(parents.iter().cloned());
            }
            members.push(key);
        }
        members.sort();
        representatives.push(members[0].clone());
        let members = std::sync::Arc::new(members);
        for key in members.iter() {
            result.insert(key.clone(), members.clone());
        }
    }
    (result, representatives)
}

/// XML vocabulary, not Core field names. Unknown reference attributes fail
/// closed. Zero/sentinel meanings are scoped to their actual wire attributes.
fn references(n: &Node) -> Vec<Result<Key, ()>> {
    let mut refs = Vec::new();
    for (name, value) in &n.attrs {
        if n.name == "fontRef" {
            refs.push(Ok((format!("font:{}", name.to_ascii_uppercase()), value.clone())));
            continue;
        }
        if n.name == "heading" && name == "idRef" {
            match n.attr("type") {
                "NONE" => {}
                "OUTLINE" if value == "0" => {} // inherited secPr@outlineShapeIDRef
                "OUTLINE" | "NUMBER" => refs.push(Ok(("numbering".into(), value.clone()))),
                "BULLET" => refs.push(Ok(("bullet".into(), value.clone()))),
                _ => refs.push(Err(())),
            }
            continue;
        }
        if n.name == "masterPage" && name == "idRef" {
            continue;
        }
        if !(name.ends_with("IDRef") || name.ends_with("IdRef") || name == "idRef") {
            continue;
        }
        let kind = match name.as_str() {
            "charPrIDRef" | "prIDRef" => {
                if value == "4294967295" {
                    continue;
                }
                "charPr"
            }
            "paraPrIDRef" => "paraPr",
            "styleIDRef" => "style",
            "tabPrIDRef" => "tabPr",
            "outlineShapeIDRef" => "numbering",
            "memoShapeIDRef" => {
                if value == "0" {
                    continue;
                }
                "memoPr"
            }
            "borderFillIDRef" => {
                if value == "0" {
                    continue;
                }
                "borderFill"
            }
            // These describe non-layout links or package content. Their wire
            // values remain in the stamp; changes conservatively invalidate.
            "nextStyleIDRef" => "style",
            "beginIDRef" => continue,
            "binaryItemIDRef" => "binary",
            "chartIDRef" => "part",
            "linkListIDRef" | "linkListNextIDRef" | "subjectIDRef" if value == "0" => continue,
            _ => {
                refs.push(Err(()));
                continue;
            }
        };
        refs.push(Ok((kind.into(), value.clone())));
    }
    refs
}

/// Canonical original-package fingerprints and resolved dependency graph.
pub struct PackageAnalysis {
    section_paths: Vec<String>,
    header: Fingerprint,
    definition_graph: BTreeMap<Key, BTreeSet<Key>>,
    global: Stamp,
    document: Stamp,
    paragraphs: HashMap<Vec<PathSeg>, Option<Stamp>>,
    order: Vec<crate::ParagraphPath>,
    pub(super) sections: Vec<Signals>,
    section_stamps: Vec<Stamp>,
    pub(super) starts: Vec<Fingerprint>,
}

impl PackageAnalysis {
    /// Analyze original package XML, without decoding or re-encoding it.
    pub fn read(bytes: &[u8]) -> HwpxResult<Self> {
        let package = RawPackage::read(bytes)?;
        let section_count = package.order.sections.len();
        let mut master_owners: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let header_xml = package.read_text_entry("Contents/header.xml")?;
        let header = Node::parse(&header_xml)?;
        let content = Node::parse(&package.read_text_entry("Contents/content.hpf")?)?;
        let mut indexed = header.clone();
        index_package_targets(&package, &content, &mut indexed)?;
        let mut definitions = Definitions::new(&indexed);
        let mut global = header.clone();
        fn remove_definitions(n: &mut Node) {
            n.children.retain(|c| c.name != "refList");
            if n.name == "head" {
                n.attrs.remove("secCnt");
            }
            for child in &mut n.children {
                remove_definitions(child);
            }
        }
        remove_definitions(&mut global);
        let (package_global, metadata) = inputs::package_inputs(&package, &content)?;
        global.children.push(package_global);
        let mut global = definitions.stamp(&global, false).0;
        // CHAR measures depend on style 0 even when this paragraph uses another style.
        global.combine(&definitions.resolve(("style".into(), "0".into())).0);
        let document = definitions.stamp(&metadata, false).0;
        let mut result = Self {
            section_paths: package.order.sections.clone(),
            header: definitions.stamp(&header, false).0.hash,
            definition_graph: definitions
                .memo
                .iter()
                .map(|(k, (stamp, _))| (k.clone(), stamp.dependencies.clone()))
                .collect(),
            global,
            document,
            paragraphs: HashMap::new(),
            order: Vec::new(),
            sections: vec![
                Signals {
                    unresolved: !package.order.problems.is_empty(),
                    ..Signals::default()
                };
                section_count
            ],
            section_stamps: Vec::new(),
            starts: Vec::new(),
        };
        for s in 0..section_count {
            let tree = Node::parse(&package.read_text_entry(&result.section_paths[s])?)?;
            let (mut stamp, mut flags) = definitions.stamp(&tree, false);
            let mut refs = Vec::new();
            collect_named(&tree, &["masterPage"], &mut refs);
            for reference in refs {
                if let Some(m) = reference
                    .attr("idRef")
                    .strip_prefix("masterpage")
                    .and_then(|m| m.parse::<usize>().ok())
                {
                    master_owners.entry(m).or_default().insert(s);
                    if package.read_entry(&format!("Contents/masterpage{m}.xml")).is_err() {
                        flags.unresolved = true;
                    }
                } else {
                    flags.unresolved = true;
                }
            }
            stamp.combine(&result.global);
            let mut headers = Vec::new();
            let mut footers = Vec::new();
            collect_named(&tree, &["header"], &mut headers);
            collect_named(&tree, &["footer"], &mut footers);
            // Multiple anchored page flows can be emitted in a different
            // order from Core. Without an emitted origin map, edits close
            // the whole section rather than guessing that correspondence.
            flags.unknown |= headers.len() > 1 || footers.len() > 1;
            result.section_stamps.push(stamp);
            result.sections[s].merge(flags);
            let mut context = Vec::new();
            collect_named(&tree, &["secPr", "colPr"], &mut context);
            let context = Node { children: context, ..Node::default() };
            let mut stamp = definitions.stamp(&context, false).0;
            // Root namespace bindings are part of every paragraph's wire
            // context, even when all local element names remain unchanged.
            let root = definitions.stamp(&tree, true).0;
            stamp.combine(&root);
            stamp.combine(&result.global);
            let mut starts = Vec::new();
            collect_named(&header, &["beginNum"], &mut starts);
            collect_named(&tree, &["startNum", "pagePr"], &mut starts);
            result.starts.push(
                definitions.stamp(&Node { children: starts, ..Node::default() }, false).0.hash,
            );
            result.walk(&tree, vec![PathSeg::Section(s)], &stamp, &mut definitions, &mut [0, 0], 0);
        }
        for (m, part) in package.entry_paths().filter_map(|p| {
            p.strip_prefix("Contents/masterpage")
                .and_then(|p| p.strip_suffix(".xml"))
                .and_then(|m| m.parse::<usize>().ok())
                .map(|m| (m, p))
        }) {
            let tree = Node::parse(&package.read_text_entry(part)?)?;
            let (stamp, flags) = definitions.stamp(&tree, false);
            let Some(owners) = master_owners.get(&m) else {
                for flags in &mut result.sections {
                    flags.unresolved = true;
                }
                continue;
            };
            for &s in owners {
                // Master pages are independent lists, but their nonlocal policy
                // belongs to the owning section just like body/header policy.
                result.sections[s].merge(flags);
                result.section_stamps[s].combine(&stamp);
                let context = result.global.clone();
                result.walk(
                    &tree,
                    vec![PathSeg::Section(s), PathSeg::MasterPage(m)],
                    &context,
                    &mut definitions,
                    &mut [0, 0],
                    0,
                );
            }
        }
        Ok(result)
    }

    /// Analyze actual encoder output and remap wire ordinals to emitted Core
    /// origins. Ambiguous/missing origins fail closed; no text/id guessing.
    pub fn read_emitted(
        bytes: &[u8],
        origins: &crate::encoder::EmittedOrigins,
    ) -> HwpxResult<Self> {
        let mut result = Self::read(bytes)?;
        let mut by_part: HashMap<String, usize> = HashMap::new();
        let mut paragraphs = HashMap::new();
        let mut order = Vec::new();
        for path in &result.order {
            let part = path
                .0
                .iter()
                .find_map(|s| match s {
                    PathSeg::MasterPage(m) => Some(format!("Contents/masterpage{m}.xml")),
                    _ => None,
                })
                .unwrap_or_else(|| match path.0[0] {
                    PathSeg::Section(s) => result.section_paths[s].clone(),
                    _ => unreachable!(),
                });
            let ordinal = by_part.entry(part.clone()).or_default();
            if let Some(Some(origin)) = origins.0.get(&part).and_then(|v| v.get(*ordinal)) {
                let stamp = result.paragraphs.get(&path.0).cloned().flatten();
                paragraphs.entry(origin.0.clone()).and_modify(|s| *s = None).or_insert(stamp);
                order.push(origin.clone());
            }
            *ordinal += 1;
        }
        result.paragraphs = paragraphs;
        result.order = order;
        Ok(result)
    }

    fn walk(
        &mut self,
        n: &Node,
        path: Vec<PathSeg>,
        context: &Stamp,
        definitions: &mut Definitions,
        hf: &mut [usize; 2],
        row: usize,
    ) {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for child in &n.children {
            let ordinal = counts.entry(&child.name).or_default();
            let i = *ordinal;
            *ordinal += 1;
            let mut pp = path.clone();
            let mut next_context = context.clone();
            let mut next_row = row;
            match child.name.as_str() {
                "p" => {
                    pp.push(if matches!(path.last(), Some(PathSeg::Section(_))) {
                        PathSeg::BodyParagraph(i)
                    } else {
                        PathSeg::NestedParagraph(i)
                    });
                    let (mut stamp, _) = definitions.stamp(child, true);
                    stamp.combine(context);
                    self.order.push(crate::ParagraphPath(pp.clone()));
                    self.paragraphs
                        .entry(pp.clone())
                        .and_modify(|v| *v = None)
                        .or_insert(Some(stamp));
                }
                "header" | "footer" => {
                    let k = usize::from(child.name == "footer");
                    pp.truncate(1);
                    pp.push(if k == 0 { PathSeg::Header(hf[k]) } else { PathSeg::Footer(hf[k]) });
                    hf[k] += 1;
                }
                "tr" => next_row = i,
                "tc" => pp.push(PathSeg::TableCell { row, cell: i }),
                "caption" => pp.push(PathSeg::Caption),
                "footNote" => pp.push(PathSeg::Footnote),
                "endNote" => pp.push(PathSeg::Endnote),
                "drawText" => pp.push(PathSeg::TextBox),
                _ => {}
            }
            // Ancestor container geometry (including referenced definitions),
            // excluding paragraphs, keeps sibling cell text independent.
            if !matches!(
                child.name.as_str(),
                "p" | "run" | "ctrl" | "sec" | "masterPage" | "header" | "footer"
            ) {
                let (stamp, _) = definitions.stamp(child, true);
                next_context.combine(&stamp);
            }
            self.walk(child, pp, &next_context, definitions, hf, next_row);
        }
    }

    /// Transitive definition dependency graph, resolved once per package.
    /// Missing/duplicate targets are also represented in paragraph dependencies
    /// and force unresolved invalidation.
    pub fn definition_dependencies(&self) -> &BTreeMap<DependencyKey, BTreeSet<DependencyKey>> {
        &self.definition_graph
    }

    /// Definitions and binary/chart targets read by a paragraph and its context.
    pub fn paragraph_dependencies(
        &self,
        path: &crate::ParagraphPath,
    ) -> Option<&BTreeSet<DependencyKey>> {
        self.paragraphs.get(&path.0).and_then(Option::as_ref).map(|s| &s.dependencies)
    }

    /// Paragraphs in package traversal order, including nested flows.
    pub fn paths(&self) -> impl Iterator<Item = &crate::ParagraphPath> {
        self.order.iter()
    }

    pub(super) fn reading_order_changed(&self, other: &Self) -> bool {
        // Appending a section keeps the proven prefix. A different resolved
        // target at an existing position changes the document's reading order.
        self.section_paths.iter().zip(&other.section_paths).any(|(a, b)| a != b)
    }

    pub(super) fn document_input_changed(&self, other: &Self, s: usize) -> bool {
        (self.sections.get(s).is_some_and(|f| f.document_input)
            || other.sections.get(s).is_some_and(|f| f.document_input))
            && self.document.hash != other.document.hash
    }

    pub(super) fn section_reasons(&self, other: &Self, s: usize) -> Vec<String> {
        let mut reasons = self.global.differences(&other.global);
        if self.document_input_changed(other, s) {
            reasons.push("document input changed (content.hpf/file information)".into());
            reasons.extend(self.document.differences(&other.document));
        }
        if let (Some(a), Some(b)) = (self.section_stamps.get(s), other.section_stamps.get(s)) {
            reasons.extend(a.differences(b));
        }
        reasons
    }

    pub(super) fn paragraph_reasons(
        &self,
        other: &Self,
        original: &crate::ParagraphPath,
        emitted: &crate::ParagraphPath,
    ) -> Vec<String> {
        if original == emitted && self.same(other, &original.0) {
            return Vec::new();
        }
        let mut reasons = self.global.differences(&other.global);
        match (self.paragraphs.get(&original.0), other.paragraphs.get(&emitted.0)) {
            (Some(Some(a)), Some(Some(b))) => {
                reasons.extend(a.differences(b));
                if !a.resolved || !b.resolved {
                    reasons.push("unresolved paragraph dependency".into());
                }
                if a.hash == b.hash && a.resolved && b.resolved && reasons.is_empty() {
                    return reasons;
                }
            }
            _ => reasons.push("unmapped/ambiguous paragraph origin".into()),
        }
        if reasons.is_empty() {
            reasons.push("XML structure/reference relationship changed".into());
        }
        reasons
    }

    pub(super) fn same(&self, other: &Self, path: &[PathSeg]) -> bool {
        if self.global.hash != other.global.hash {
            return false;
        }
        if let Some(PathSeg::Section(s)) = path.first() {
            if !path.iter().any(|p| matches!(p, PathSeg::MasterPage(_)))
                && self.section_same(other, *s)
            {
                return true;
            }
        }
        match (self.paragraphs.get(path), other.paragraphs.get(path)) {
            (Some(Some(a)), Some(Some(b))) => a.resolved && b.resolved && a.hash == b.hash,
            _ => false,
        }
    }

    pub(super) fn mapped(&self, path: &[PathSeg]) -> bool {
        matches!(self.paragraphs.get(path), Some(Some(_)))
    }

    pub(super) fn section_same(&self, other: &Self, s: usize) -> bool {
        if self.global.hash != other.global.hash {
            return false;
        }
        if self.document_input_changed(other, s) {
            return false;
        }
        // Unknown policy may depend implicitly on other header definitions.
        // Only identical entire headers justify keeping such a flow.
        if (self.sections.get(s).is_some_and(|f| f.unknown)
            || other.sections.get(s).is_some_and(|f| f.unknown))
            && self.header != other.header
        {
            return false;
        }
        match (self.section_stamps.get(s), other.section_stamps.get(s)) {
            (Some(a), Some(b)) => a.resolved && b.resolved && a.hash == b.hash,
            _ => false,
        }
    }
}

fn index_package_targets(
    package: &RawPackage,
    content: &Node,
    header: &mut Node,
) -> HwpxResult<()> {
    let mut items = Vec::new();
    collect_named(content, &["item"], &mut items);
    for item in items {
        let href = item.attr("href");
        if let Ok(bytes) = package.read_entry(href) {
            let mut attrs = item.attrs.clone();
            attrs.remove("href");
            attrs.insert("payload".into(), format!("{:x?}", Sha256::digest(bytes).as_slice()));
            header.children.push(Node { name: "binary".into(), attrs, ..Node::default() });
        }
    }
    for path in
        package.entry_paths().filter(|p| p.starts_with("Chart/") || p.starts_with("BinData/"))
    {
        let bytes = package.read_entry(path)?;
        let mut node = Node {
            name: "part".into(),
            attrs: BTreeMap::from([("id".into(), path.into())]),
            ..Node::default()
        };
        if path.ends_with(".xml") {
            node.children.push(Node::parse(
                std::str::from_utf8(bytes)
                    .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?,
            )?);
        } else {
            node.attrs.insert("payload".into(), format!("{:x?}", Sha256::digest(bytes).as_slice()));
        }
        header.children.push(node);
    }
    Ok(())
}

impl Stamp {
    fn combine(&mut self, other: &Self) {
        self.hash = Sha256::digest([self.hash, other.hash].concat()).into();
        self.resolved &= other.resolved;
        merge_facts(&mut self.facts, &other.facts);
        self.dependencies.extend(other.dependencies.iter().cloned());
    }

    fn differences(&self, other: &Self) -> Vec<String> {
        let mut keys: BTreeSet<_> = self.facts.keys().chain(other.facts.keys()).collect();
        keys.retain(|k| self.facts.get(*k) != other.facts.get(*k));
        keys.into_iter().cloned().collect()
    }
}

fn collect_named(n: &Node, names: &[&str], into: &mut Vec<Node>) {
    if names.contains(&n.name.as_str()) {
        into.push(n.clone());
    } else {
        for child in &n.children {
            collect_named(child, names, into);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(xml: &str) -> Node {
        Node::parse(xml).unwrap()
    }
    fn fingerprint(header: &str, body: &str) -> (Stamp, Signals) {
        Definitions::new(&node(header)).stamp(&node(body), false)
    }

    // 이것을 실패시키는 것: 참조 속성 대신 Core 이름 목록으로만 의존성을 구성하기.
    #[test]
    fn emitted_reference_graph_covers_transitive_and_unknown_references() {
        let h = "<head><style id='7' charPrIDRef='0'/><charPr id='3' borderFillIDRef='1'><fontRef latin='0'/></charPr><fontface lang='LATIN'><font id='2' face='Font'/></fontface><borderFill id='5'/></head>";
        let body = "<p styleIDRef='0'><compose><charPr prIDRef='0'/><charPr prIDRef='4294967295'/></compose></p>";
        let (base, _) = fingerprint(h, body);
        assert!(base.resolved);
        for changed in [
            h.replace("Font", "Other"),
            h.replace("<borderFill id='5'/>", "<borderFill id='5'><fill/></borderFill>"),
        ] {
            assert_ne!(base.hash, fingerprint(&changed, body).0.hash);
        }
        assert!(!fingerprint(h, "<p futureIDRef='2'/>").0.resolved);
        assert!(!fingerprint(h, "<p><charPr prIDRef='99'/></p>").0.resolved);
    }

    // 이것을 실패시키는 것: nextStyle 순환을 누락하거나 모든 순환을 미해결로 버리기.
    #[test]
    fn cyclic_definitions_include_every_member_and_external_dependency() {
        let h = "<head><style id='0' nextStyleIDRef='1'/><style id='1' nextStyleIDRef='0' charPrIDRef='0'/><charPr id='2' height='1000'/></head>";
        let changed = h.replace("1000", "2000");
        for id in [0, 1] {
            let body = format!("<p styleIDRef='{id}'/>");
            let base = fingerprint(h, &body).0;
            assert!(base.resolved);
            assert_ne!(base.hash, fingerprint(&changed, &body).0.hash);
        }
    }

    // 이것을 실패시키는 것: 글꼴 언어별 위치를 무시하거나 id 중복으로 유효 위치를 거부하기(한컴 실측).
    #[test]
    fn emitted_ids_fonts_and_duplicates_are_resolved_by_position() {
        let h = "<head><charPr id='7'><fontRef latin='0'/></charPr><fontface lang='HANGUL'><font id='0' face='K'/></fontface><fontface lang='LATIN'><font id='0' face='L'/></fontface></head>";
        let body = "<run charPrIDRef='0'/>";
        let base = fingerprint(h, body).0;
        assert!(base.resolved);
        assert_eq!(base.hash, fingerprint(&h.replace("face='K'", "face='Changed'"), body).0.hash);
        assert_ne!(base.hash, fingerprint(&h.replace("face='L'", "face='Changed'"), body).0.hash);
        assert!(fingerprint(&h.replace("face='L'/>", "face='L'/><font id='0'/>"), body).0.resolved);
        assert_eq!(base.hash, fingerprint(&h.replace("id='7'", "id='8'"), body).0.hash);
    }

    // 이것을 실패시키는 것: run마다 정의 또는 글꼴 지문을 재계산하기.
    #[test]
    fn emitted_definition_work_is_memoized_across_references() {
        let h = node("<head><charPr id='7'><fontRef latin='0'/></charPr><fontface lang='LATIN'><font id='0' face='L'/></fontface></head>");
        let mut defs = Definitions::new(&h);
        let memo = defs.memo.clone();
        let evaluated = defs.evaluated;
        for _ in 0..10000 {
            assert!(defs.stamp(&node("<run charPrIDRef='0'/>"), false).0.resolved);
        }
        assert_eq!(memo.len(), 2);
        assert_eq!(defs.memo.len(), memo.len());
        assert_eq!(defs.evaluated, evaluated);
        assert!(defs.visiting.is_empty());
    }

    // 이것을 실패시키는 것: 개체 종류 허용 목록으로 floating 정책을 추정하기.
    #[test]
    fn nonlocal_dependency_inventory_includes_nested_and_backward_consumers() {
        for kind in ["footNote", "endNote", "newNum", "autoNum"] {
            assert!(fingerprint("<head/>", &format!("<caption><{kind}/></caption>")).1.numbering);
        }
        for body in [
            "<autoNum numType='TOTAL_PAGE'/>",
            "<fieldBegin/>",
            "<pageNum/>",
            "<p showLineNumber='1'/>",
        ] {
            assert!(fingerprint("<head/>", body).1.backwards, "{body}");
        }
        for kind in ["chart", "pic", "tbl", "equation", "futureObject"] {
            assert!(
                fingerprint("<head/>", &format!("<{kind}><pos treatAsChar='0'/></{kind}>"))
                    .1
                    .floating
            );
        }
        assert!(!fingerprint("<head/>", "<pos treatAsChar='1'/>").1.floating);
        for attr in ["keepWithNext", "widowOrphan"] {
            assert!(fingerprint("<head/>", &format!("<breakSetting {attr}='1'/>")).1.linked);
        }
        assert!(fingerprint("<head/>", "<ctrl><futureControl/></ctrl>").1.unknown);
    }

    // 이것을 실패시키는 것: style을 통한 heading 참조 또는 미해결 참조를 누락하기.
    #[test]
    fn numbering_resolves_explicit_implicit_and_unresolved_style_dependencies() {
        for kind in ["OUTLINE", "NUMBER"] {
            let h = format!("<head><style id='0' paraPrIDRef='0'/><paraPr id='7'><heading type='{kind}' idRef='1'/></paraPr><numbering id='1'/></head>");
            let result = fingerprint(&h, "<p styleIDRef='0'/>");
            assert!(result.0.resolved && result.1.numbering);
            assert!(!fingerprint(&h, "<p styleIDRef='999'/>").0.resolved);
        }
    }

    // 이것을 실패시키는 것: mixed content 텍스트를 합쳐 경계 위치를 잃거나 entity를 누락하기.
    #[test]
    fn fingerprint_keeps_mixed_content_order_and_entity_values() {
        assert_ne!(
            fingerprint("<head/>", "<t>a<lineBreak/></t>").0.hash,
            fingerprint("<head/>", "<t><lineBreak/>a</t>").0.hash,
        );
        let base = fingerprint("<head/>", "<t>a<lineBreak/>b&amp;c</t>").0;
        for body in ["<t>ab<lineBreak/>&amp;c</t>", "<t>a<lineBreak/>b&lt;c</t>"] {
            assert_ne!(base.hash, fingerprint("<head/>", body).0.hash);
        }
    }
}

#[cfg(test)]
mod canonical_tests {
    use super::*;
    fn stamp(header: &str, body: &str) -> Stamp {
        Definitions::new(&Node::parse(header).unwrap()).stamp(&Node::parse(body).unwrap(), false).0
    }

    // 이것을 실패시키는 것: 시작 태그 원문, 접두사 또는 속성 입력 순서를 지문화하기.
    #[test]
    fn canonical_xml_ignores_syntax_but_preserves_namespaces_and_mixed_spaces() {
        let a = "<a:p xmlns:a='urn:p' b='2' a='1'>\n <a:run><a:t>A &amp; B<a:lineBreak/> </a:t></a:run>\n</a:p>";
        let b = "<p xmlns='urn:p' a='1' b='2'><run><t><![CDATA[A & B]]><lineBreak></lineBreak>&#32;</t></run></p>";
        assert_eq!(stamp("<head/>", a).hash, stamp("<head/>", b).hash);
        for bad in [
            b.replace("urn:p", "urn:other"),
            b.replace("&#32;", ""),
            b.replace("A & B", "A&B"),
            b.replace("<lineBreak></lineBreak>&#32;", "&#32;<lineBreak/>"),
        ] {
            assert_ne!(stamp("<head/>", a).hash, stamp("<head/>", &bad).hash, "{bad}");
        }
        // 이것을 실패시키는 것: 본문 텍스트의 앞뒤 공백을 trim으로 지우기.
        for spaced in ["<t> A</t>", "<t>A </t>", "<t> A </t>"] {
            assert_ne!(stamp("<head/>", spaced).hash, stamp("<head/>", "<t>A</t>").hash);
        }
        assert_ne!(
            stamp("<head/>", "<x xml:space='preserve'> </x>").hash,
            stamp("<head/>", "<x xml:space='preserve'/>").hash
        );
    }

    // 이것을 실패시키는 것: 원시 ID 또는 SCC 정렬 순서를 해시에 포함하거나 순환의 진입점을 합치기.
    #[test]
    fn canonical_definition_renumbering_preserves_rooted_reference_relationships() {
        // 한컴 실측: 정의 id만 바꾸면 의미 동일, 참조는 항상 위치다.
        let a = "<head><style id='1' nextStyleIDRef='1' name='A'/><style id='2' nextStyleIDRef='0' name='B'/><charPr id='3' height='1000'/></head>";
        let b = "<head><style id='90' nextStyleIDRef='1' name='A'/><style id='4' nextStyleIDRef='0' name='B'/><charPr id='20' height='1000'/></head>";
        let body = "<p styleIDRef='0'><run charPrIDRef='0'/></p>";
        let original = stamp(a, body);
        assert!(original.resolved);
        assert_eq!(original.hash, stamp(b, body).hash);
        assert_ne!(original.hash, stamp(b, "<p styleIDRef='1'><run charPrIDRef='0'/></p>").hash);
        assert_ne!(
            original.hash,
            stamp(&b.replace("nextStyleIDRef='0'", "nextStyleIDRef='1'"), body).hash
        );
        assert!(!stamp(b, "<p styleIDRef='90'/>").resolved);
    }

    // 이것을 실패시키는 것: 문서 입력 감지를 IDRef 접미사에만 의존하기.
    #[test]
    fn string_parameters_and_page_fields_read_document_inputs() {
        for xml in [
            "<stringParam>$lastsaveby</stringParam>",
            "<fieldBegin/>",
            "<autoNum numType='PAGE'/>",
            "<autoNum numType='TOTAL_PAGE'/>",
            "<pageNum/>",
            "<pageNumCtrl/>",
            "<pageNumPos/>",
        ] {
            assert!(
                Definitions::new(&Node::default())
                    .stamp(&Node::parse(xml).unwrap(), false)
                    .1
                    .document_input,
                "{xml}"
            );
        }
        assert!(
            !Definitions::new(&Node::default())
                .stamp(&Node::parse("<t>$lastsaveby</t>").unwrap(), false)
                .1
                .document_input
        );
    }

    fn package(payload: &str, chart: &str) -> RawPackage {
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", "application/hwp+zip"),
            ("Contents/header.xml", "<head/>"),
            ("Contents/section0.xml", "<sec/>"),
            ("Contents/content.hpf", "<package/>"),
            ("BinData/a.bin", payload),
            ("Chart/chart1.xml", chart),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        RawPackage::read(&zip.finish().unwrap().into_inner()).unwrap()
    }

    // 이것을 실패시키는 것: binaryItemIDRef/chartIDRef를 건너뛰거나 대상 바이트를 누락하기.
    #[test]
    fn binary_and_chart_targets_are_dependencies_and_missing_targets_fail_closed() {
        let content =
            Node::parse("<manifest><item id='img' href='BinData/a.bin'/></manifest>").unwrap();
        let fingerprint = |payload, chart, body| {
            let mut h = Node::parse("<head/>").unwrap();
            index_package_targets(&package(payload, chart), &content, &mut h).unwrap();
            Definitions::new(&h).stamp(&Node::parse(body).unwrap(), false).0
        };
        for body in ["<pic binaryItemIDRef='img'/>", "<chart chartIDRef='Chart/chart1.xml'/>"] {
            let a = fingerprint("abc", "<chart width='1'/>", body);
            assert!(a.resolved);
            let b = if body.contains("pic") {
                fingerprint("abcd", "<chart width='1'/>", body)
            } else {
                fingerprint("abc", "<chart width='2'/>", body)
            };
            assert_ne!(a.hash, b.hash);
        }
        assert!(!fingerprint("abc", "<chart/>", "<pic binaryItemIDRef='missing'/>").resolved);
        assert!(
            !fingerprint("abc", "<chart/>", "<chart chartIDRef='Chart/missing.xml'/>").resolved
        );
        assert_eq!(
            fingerprint("abc", "<c a='1' b='2'/>", "<chart chartIDRef='Chart/chart1.xml'/>").hash,
            fingerprint("abc", "<c b='2' a='1'></c>", "<chart chartIDRef='Chart/chart1.xml'/>")
                .hash
        );
    }
}

#[cfg(test)]
mod field_link_tests {
    use super::*;
    fn stamp(xml: &str) -> Stamp {
        Definitions::new(&Node::default()).stamp(&Node::parse(xml).unwrap(), false).0
    }

    // 이것을 실패시키는 것: fieldBegin id를 지우고 beginIDRef는 원시 값으로 남기거나 연결 검증을 생략하기.
    #[test]
    fn field_link_renumbering_preserves_targets_and_rejects_broken_links() {
        let a = "<p><fieldBegin id='10' type='A'/><fieldBegin id='20' type='B'/><fieldEnd beginIDRef='10'/><fieldEnd beginIDRef='20'/></p>";
        let b = a.replace("'10'", "'90'").replace("'20'", "'4'");
        let original = stamp(a);
        assert!(original.resolved);
        assert_eq!(original.hash, stamp(&b).hash);
        let swapped = a.replace("beginIDRef='10'", "beginIDRef='20'");
        assert_ne!(original.hash, stamp(&swapped).hash);
        // Similar bytes with only the definition changed must not keep caches.
        for broken in [
            a.replace("id='10'", "id='99'"),
            a.replace("id='20'", "id='10'"),
            a.replace("beginIDRef='20'", "beginIDRef='99'"),
        ] {
            assert!(!stamp(&broken).resolved, "{broken}");
        }
    }
}

#[cfg(test)]
mod unknown_identity_tests {
    use super::*;
    // 이것을 실패시키는 것: 의미를 모르는 id까지 무조건 지문에서 제외하기.
    #[test]
    fn unknown_ids_remain_inputs_while_known_object_ids_can_be_renumbered() {
        let hash = |xml: &str| {
            Definitions::new(&Node::default()).stamp(&Node::parse(xml).unwrap(), false).0.hash
        };
        assert_eq!(hash("<p id='1'><pic id='2'/></p>"), hash("<p id='9'><pic id='8'/></p>"));
        assert_ne!(hash("<future id='1'/>"), hash("<future id='2'/>"));
    }
}
