//! Conservative one-line filter for text-only cell replacement.
use super::*;
use crate::line_keep::Tag;
use crate::patch::RawPackage;
use crate::style_store::HwpxStyleStore;
use hwpforge_core::layout::LayoutCache;
use hwpforge_core::node_path::CorePath;

pub(super) struct Candidate {
    table: usize,
    row: usize,
    cell: usize,
    text: String,
    cache: LayoutCache,
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
    Some((old, crate::line_keep::eligible(para, old, text, styles)?))
}

fn source_tag(
    parts: &HashMap<String, String>,
    map: &crate::decoder::source_map::SourceMap,
    path: &CorePath,
    section: &crate::line_keep::SectionEvidence,
) -> Option<Tag> {
    count(2);
    let node = map.node(path)?;
    let range = node.ranges().first()?;
    let xml = parts.get(node.part())?;
    Tag::read_in(xml.get(range.clone())?, &section.namespaces)
}

fn source_allows(
    parts: &HashMap<String, String>,
    map: &crate::decoder::source_map::SourceMap,
    path: &CorePath,
    header: &crate::line_keep::Header,
    section: &crate::line_keep::SectionEvidence,
    paragraph: &Paragraph,
    styles: &HwpxStyleStore,
) -> Option<()> {
    let cell = source_tag(parts, map, path, section)?;
    let sub = cell.child("subList")?;
    if !crate::line_keep::cell_flow_allows(
        sub.attrs.get("textDirection").map(String::as_str),
        sub.attrs.get("lineWrap").map(String::as_str),
    ) {
        return None;
    }
    let paragraph_path =
        CorePath::new(path.section(), format!("{}.paragraphs[0]", path.relative()));
    count(2);
    let node = map.node(&paragraph_path)?;
    let [range] = node.ranges() else {
        return None;
    };
    let xml = parts.get(node.part())?;
    let p = map.cache_flow.paragraph_at(range.start)?;
    let source = map.cache_flow.references(p, xml, &section.namespaces)?;
    header.proves(section, &source, paragraph, styles, true).then_some(())
}

/// Selects cells whose Core cache and original wire properties prove one line.
pub(super) fn candidates(
    decoded: &HwpxDocument,
    package: &RawPackage,
    edits: &[ResolvedEdit],
) -> Vec<Candidate> {
    if decoded.document.format_source::<crate::decoder::SourceBasis>().is_none_or(|basis| {
        !basis.compatibility.proves_single_line_width(decoded.document.layout_profile())
    }) {
        return Vec::new();
    }
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
                    count(0);
                    crate::line_keep::Header::read(package, &decoded.style_store, || count(4))
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
                let allowed = crate::line_keep::SectionEvidence::read(&xml);
                parts.insert(part.to_owned(), xml);
                section_allowed.insert(part.to_owned(), allowed);
            }
            source_allows(
                &parts,
                map,
                &path,
                definitions,
                section_allowed.get(part)?.as_ref()?,
                &cell.paragraphs[0],
                &decoded.style_store,
            )?;
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
