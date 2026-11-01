//! Conservative one-line filter for text-only cell replacement.
use super::*;
use crate::patch::RawPackage;
use crate::style_store::{HwpxCharShape, HwpxStyleStore};
use hwpforge_core::layout::LayoutCache;
use hwpforge_core::node_path::CorePath;
use quick_xml::{events::Event, Reader};

pub(super) struct Candidate {
    table: usize,
    row: usize,
    cell: usize,
    text: String,
    cache: LayoutCache,
}

// Only attributes actually present on the source count as evidence.
#[derive(Default)]
struct Tag {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Tag>,
}
impl Tag {
    fn read(xml: &str) -> Option<Self> {
        let mut reader = Reader::from_str(xml);
        let mut stack = vec![Self::default()];
        loop {
            let event = reader.read_event().ok()?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let mut tag =
                        Self { name: e.local_name().as_ref().to_string(), ..Self::default() };
                    for attr in e.attributes() {
                        let attr = attr.ok()?;
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .ok()?
                            .into_owned();
                        tag.attrs.insert(attr.key.as_ref().to_string(), value);
                    }
                    if empty {
                        stack.last_mut()?.children.push(tag);
                    } else {
                        if stack.len() > crate::decoder::XML_RECURSION_LIMIT {
                            return None;
                        }
                        stack.push(tag);
                    }
                }
                Event::End(_) => {
                    let tag = stack.pop()?;
                    stack.last_mut()?.children.push(tag);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if stack.len() != 1 {
            return None;
        }
        let mut root = stack.pop()?;
        if root.children.len() != 1 {
            return None;
        }
        root.children.pop()
    }
    fn child(&self, name: &str) -> Option<&Self> {
        let mut children = self.children.iter().filter(|c| c.name == name);
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        Some(child)
    }
    fn into_child(self, name: &str) -> Option<Self> {
        let mut children = self.children.into_iter().filter(|c| c.name == name);
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        Some(child)
    }
    fn is(&self, key: &str, value: &str) -> bool {
        self.attrs.get(key).map(String::as_str) == Some(value)
    }
}

fn weight(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    text.chars().try_fold(0u64, |sum, c| {
        // Conservative measured advances for the supported character set.
        let weight = match c {
            '\u{ac00}'..='\u{d7a3}' => 105,
            ' ' => 50,
            '0'..='9' | '.' | ',' | '-' | '(' | ')' | '/' | ':' => 100,
            _ => return None,
        };
        sum.checked_add(weight)
    })
}

fn width(text: &str, shape: &HwpxCharShape) -> Option<u64> {
    // Positive spacing at reduced relative size has no measured width bound.
    if shape.rel_sz < 100 && shape.spacing > 0 {
        return None;
    }
    let height = u64::try_from(shape.height.as_i32()).ok().filter(|v| *v > 0)?;
    let rel = u64::try_from(shape.rel_sz).ok().filter(|v| *v > 0)?;
    let ratio = u64::try_from(shape.ratio).ok().filter(|v| *v > 0)?;
    let spacing = shape.spacing.max(0) as u64;
    weight(text)?
        .checked_mul(height)?
        .checked_mul(rel)?
        .checked_mul(ratio.checked_add(spacing)?)?
        .checked_add(999_999)
        .map(|n| n / 1_000_000)
}

fn eligible<'a>(
    cell: &'a TableCell,
    text: &str,
    styles: &HwpxStyleStore,
) -> Option<(&'a str, &'a LayoutCache)> {
    let [para] = cell.paragraphs.as_slice() else {
        return None;
    };
    let [run] = para.runs.as_slice() else {
        return None;
    };
    let RunContent::Text(old) = &run.content else {
        return None;
    };
    if para.end_char_shape.is_some() {
        return None;
    }
    let cache = para.layout_cache.as_ref()?;
    let [line] = cache.lines.as_slice() else {
        return None;
    };
    if line.textpos != 0 || line.boundary.is_some() {
        return None;
    }
    weight(old)?;
    let shape = styles.char_shape(run.char_shape_id).ok()?;
    let available = u64::try_from(line.horzsize).ok()?;
    if width(text, shape)? > available {
        return None;
    }
    Some((old, cache))
}

fn source_tag(
    parts: &HashMap<String, String>,
    map: &crate::decoder::source_map::SourceMap,
    path: &CorePath,
) -> Option<Tag> {
    count(2);
    let node = map.node(path)?;
    let range = node.ranges().first()?;
    let xml = parts.get(node.part())?;
    Tag::read(xml.get(range.clone())?)
}

fn source_allows(
    parts: &HashMap<String, String>,
    map: &crate::decoder::source_map::SourceMap,
    path: &CorePath,
    definitions: &[Tag],
) -> Option<()> {
    let cell = source_tag(parts, map, path)?;
    let sub = cell.child("subList")?;
    if !sub.is("textDirection", "HORIZONTAL") || !sub.is("lineWrap", "BREAK") {
        return None;
    }
    let paragraph = source_tag(
        parts,
        map,
        &CorePath::new(path.section(), format!("{}.paragraphs[0]", path.relative())),
    )?;
    let index: usize = paragraph.attrs.get("paraPrIDRef")?.parse().ok()?;
    // Header references use definition order, independent of authored id attributes.
    let definition = definitions.get(index)?;
    if !definition.is("fontLineHeight", "0") || !definition.is("condense", "0") {
        return None;
    }
    let auto = definition.child("autoSpacing")?;
    if !auto.is("eAsianEng", "0") || !auto.is("eAsianNum", "0") {
        return None;
    }
    if !definition.child("breakSetting")?.is("lineWrap", "BREAK")
        || !definition.child("heading")?.is("type", "NONE")
    {
        return None;
    }
    Some(())
}

// Grid widths are outside the measured bound; missing properties are not evidence.
fn section_allows(xml: &str) -> Option<()> {
    let section = Tag::read(xml)?;
    let mut properties = section
        .children
        .iter()
        .filter(|p| p.name == "p")
        .flat_map(|p| &p.children)
        .filter(|r| r.name == "run")
        .flat_map(|r| &r.children)
        .filter(|t| t.name == "secPr");
    let property = properties.next()?;
    if properties.next().is_some() {
        return None;
    }
    let grid = property.child("grid")?;
    if !grid.is("charGrid", "0") || !grid.is("wonggojiFormat", "0") {
        return None;
    }
    Some(())
}

fn read_header(package: &RawPackage) -> Option<Tag> {
    count(0);
    package.read_text_entry("Contents/header.xml").ok().and_then(|xml| Tag::read(&xml))
}

/// Selects cells whose Core cache and original wire properties prove one line.
pub(super) fn candidates(
    decoded: &HwpxDocument,
    package: &RawPackage,
    edits: &[ResolvedEdit],
) -> Vec<Candidate> {
    let header = std::cell::OnceCell::new();
    let tables = tables_in_document(&decoded.document);
    let mut parts = HashMap::new();
    let mut section_allowed = HashMap::new();
    edits
        .iter()
        .filter_map(|edit| {
            let entry = &tables[edit.table];
            let cell = &entry.table.rows[edit.row_idx].cells[edit.cell_idx];
            let (text, cache) = eligible(cell, &edit.text, &decoded.style_store)?;
            let definitions = header
                .get_or_init(|| {
                    let properties = read_header(package)?
                        .into_child("refList")?
                        .into_child("paraProperties")?;
                    Some(
                        properties
                            .children
                            .into_iter()
                            .filter(|tag| {
                                count(4);
                                tag.name == "paraPr"
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .as_ref()?;
            let table_path = crate::table_inventory::core_path(entry);
            let path = CorePath::new(
                entry.section,
                format!(
                    "{}.rows[{}].cells[{}]",
                    table_path.relative(),
                    edit.row_idx,
                    edit.cell_idx
                ),
            );
            let map = decoded.source_map(entry.section)?;
            count(2);
            let part = map.node(&path)?.part();
            if !parts.contains_key(part) {
                count(1);
                let xml = package.read_text_entry(part).ok()?;
                let allowed = section_allows(&xml).is_some();
                parts.insert(part.to_owned(), xml);
                section_allowed.insert(part.to_owned(), allowed);
            }
            if !section_allowed.get(part).copied()? {
                return None;
            }
            source_allows(&parts, map, &path, definitions)?;
            Some(Candidate {
                table: edit.table,
                row: edit.row_idx,
                cell: edit.cell_idx,
                text: text.into(),
                cache: cache.clone(),
            })
        })
        .collect()
}

fn visit(
    document: &mut Document<Draft>,
    candidates: &[Candidate],
    mut f: impl FnMut(&mut Paragraph, &Candidate),
) {
    let mut by_table: HashMap<usize, Vec<&Candidate>> = HashMap::new();
    for candidate in candidates {
        by_table.entry(candidate.table).or_default().push(candidate);
    }
    for_each_table_mut(document, &mut |ordinal, table| {
        if let Some(candidates) = by_table.get(&ordinal) {
            for c in candidates {
                f(&mut table.rows[c.row].cells[c.cell].paragraphs[0], c);
            }
        }
    });
}
pub(super) fn restore_caches(document: &mut Document<Draft>, candidates: &[Candidate]) {
    visit(document, candidates, |p, c| p.layout_cache = Some(c.cache.clone()));
}
/// Revokes evidence lost by the source encoder; unreadable evidence cancels all candidates.
pub(super) fn cancel_dropped(
    source: &crate::layout_carry::CacheSource,
    candidates: &mut Vec<Candidate>,
) {
    if candidates.is_empty() {
        return;
    }
    // Warning paths omit run identities. Decode only a marked source, so
    // the decoder's inventory distinguishes sibling tables in the same host.
    match source.marked_document() {
        Err(_) => candidates.clear(),
        Ok(Some(decoded)) => {
            let tables = tables_in_document(&decoded.document);
            candidates.retain(|c| {
                tables
                    .get(c.table)
                    .and_then(|entry| entry.table.rows.get(c.row))
                    .and_then(|row| row.cells.get(c.cell))
                    .and_then(|cell| cell.paragraphs.first())
                    .is_some_and(|p| p.layout_cache.is_some())
            });
        }
        Ok(None) => {}
    }
}
/// Reverts only surviving candidates' text for the baseline comparison.
pub(super) fn probe(
    mut document: Document<Draft>,
    styles: &HwpxStyleStore,
    images: &hwpforge_core::ImageStore,
    candidates: &[Candidate],
) -> crate::HwpxResult<Option<Vec<u8>>> {
    if candidates.is_empty() {
        return Ok(None);
    }
    visit(&mut document, candidates, |p, c| p.runs[0].content = RunContent::Text(c.text.clone()));
    let validated = document
        .validate()
        .map_err(|e| crate::HwpxError::InvalidStructure { detail: e.to_string() })?;
    count(3);
    HwpxEncoder::encode(&validated, styles, images).map(Some)
}

#[cfg(test)]
#[path = "line_keep_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    // Header reads, section reads, node queries, probe encodes, definition checks on this thread.
    static COST: std::cell::Cell<(usize, usize, usize, usize, usize)> = const { std::cell::Cell::new((0, 0, 0, 0, 0)) };
}

#[inline]
#[cfg_attr(not(test), allow(unused_variables))]
fn count(field: usize) {
    #[cfg(test)]
    COST.with(|cost| {
        let (h, p, q, e, d) = cost.get();
        let mut fields = [h, p, q, e, d];
        fields[field] += 1;
        cost.set((fields[0], fields[1], fields[2], fields[3], fields[4]));
    });
}
