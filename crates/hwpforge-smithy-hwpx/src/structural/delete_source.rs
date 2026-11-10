//! Prove that a decoder-addressed deletion contains only plain body text.
use super::{references, verification_error, StructuralEditError};
use crate::decoder::source_map::{SourceCoverage, SourceMap, SourceNode};
use crate::exchange::TextLocator;
use hwpforge_core::{
    node_path::{CorePath, NodeKind},
    Paragraph, RunContent,
};
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::{collections::HashMap, ops::Range};

#[cfg(test)]
thread_local! {
    pub(super) static INDEX_LOOKUPS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    pub(super) static MARKUP_PATH: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

fn invalid() -> StructuralEditError {
    verification_error(crate::HwpxError::InvalidStructure {
        detail: "unproven plain paragraph deletion source".into(),
    })
}
fn require(value: bool) -> Result<(), StructuralEditError> {
    if value {
        Ok(())
    } else {
        Err(invalid())
    }
}
pub(super) fn complete<'a>(
    node: Option<&'a SourceNode>,
    part: &str,
    kind: NodeKind,
) -> Result<&'a [Range<usize>], StructuralEditError> {
    let node = node.ok_or_else(invalid)?;
    require(node.part() == part && node.kind() == kind)?;
    match node.copy_source() {
        SourceCoverage::Complete(ranges) => Ok(ranges),
        _ => Err(invalid()),
    }
}

pub(super) fn check(
    xml: &str,
    part: &str,
    map: &SourceMap,
    section: usize,
    indices: &[usize],
    paragraphs: &[Paragraph],
) -> Result<(), StructuralEditError> {
    let source = references::source(xml)?;
    require(source.digest() == &map.source_digest)?;
    let elements = source.elements();
    // Keep the first match, as the source-order searches did, even for malformed maps.
    let mut element_by_span = HashMap::with_capacity(elements.len());
    for (id, element) in elements.iter().enumerate() {
        crate::work::tick();
        element_by_span.entry((element.outer().start, element.outer().end)).or_insert(id);
    }
    let mut slot_by_path = HashMap::with_capacity(map.text_slots().len());
    for slot in map.text_slots() {
        crate::work::tick();
        slot_by_path.entry(slot.path.as_str()).or_insert(slot);
    }
    let mut targets = Vec::with_capacity(indices.len());
    for &index in indices {
        crate::work::tick();
        let span = &map.paragraphs.get(index).ok_or_else(invalid)?.outer;
        let path = format!("paragraphs[{index}]");
        require(
            complete(map.node(&CorePath::new(section, &path)), part, NodeKind::Paragraph)?
                == std::slice::from_ref(span),
        )?;
        #[cfg(test)]
        INDEX_LOOKUPS.with(|c| c.set((c.get().0 + 1, c.get().1)));
        let id = element_by_span.get(&(span.start, span.end)).ok_or_else(invalid)?;
        let p = &elements[*id];
        let wire_runs: Vec<_> = p
            .children()
            .iter()
            .filter_map(|&id| elements.get(id))
            .filter(|e| e.name() == "run")
            .collect();
        let paragraph = paragraphs.get(index).ok_or_else(invalid)?;
        require(wire_runs.len() == paragraph.runs.len())?;
        for (ri, (run, wire)) in paragraph.runs.iter().zip(wire_runs).enumerate() {
            crate::work::tick();
            let RunContent::Text(text) = &run.content else { return Err(invalid()) };
            let path = format!("{path}.runs[{ri}]");
            require(
                complete(map.node(&CorePath::new(section, &path)), part, NodeKind::Run)?
                    == [wire.outer()],
            )?;
            let path = format!("{path}.text");
            #[cfg(test)]
            INDEX_LOOKUPS.with(|c| c.set((c.get().0, c.get().1 + 1)));
            let slot = slot_by_path.get(path.as_str()).ok_or_else(invalid)?;
            require(!slot.has_inline_markup && slot.original_text == *text)?;
            let text_elements: Vec<_> =
                wire.children().iter().filter_map(|&id| elements.get(id)).collect();
            match &slot.locator {
                TextLocator::TextElement {
                    element_start,
                    element_end,
                    content_start,
                    content_end,
                } => {
                    let [t] = text_elements.as_slice() else { return Err(invalid()) };
                    require(t.name() == "t" && t.outer() == (*element_start..*element_end))?;
                    require(
                        t.inner().as_ref().map(|r| r.start) == *content_start
                            && t.inner().as_ref().map(|r| r.end) == *content_end,
                    )?;
                    require(
                        complete(map.node(&CorePath::new(section, &path)), part, NodeKind::Text)?
                            == [t.outer()],
                    )?;
                }
                TextLocator::EmptyRun { run_start, run_end } => {
                    require(
                        text.is_empty()
                            && text_elements.is_empty()
                            && wire.outer() == (*run_start..*run_end),
                    )?;
                    require(
                        complete(map.node(&CorePath::new(section, &path)), part, NodeKind::Text)?
                            == [wire.outer()],
                    )?;
                }
            }
        }
        targets.push(span.clone());
    }
    check_markup(xml, &targets)
}

// Read namespace scopes from the actual part. Offsets and text identity remain decoder-owned.
pub(super) fn check_markup(xml: &str, targets: &[Range<usize>]) -> Result<(), StructuralEditError> {
    let mut sorted = targets.to_vec();
    sorted.sort_unstable_by_key(|range| {
        crate::work::tick();
        #[cfg(test)]
        MARKUP_PATH.with(|c| c.set((c.get().0 + 1, c.get().1)));
        range.start
    });
    let mut remaining = sorted.iter().peekable();
    let input = crate::wire_xml::PositionedXml::new(xml);
    let mut reader = NsReader::from_str(input.as_str());
    let mut stack = Vec::<String>::new();
    loop {
        let at = input.original_offset(reader.buffer_position());
        let event = reader.read_event().map_err(|_| invalid())?;
        crate::work::tick();
        // Reader offsets only advance, so each expired range is visited once.
        while remaining.peek().is_some_and(|r| r.end <= at) {
            crate::work::tick();
            #[cfg(test)]
            MARKUP_PATH.with(|c| c.set((c.get().0, c.get().1 + 1)));
            remaining.next();
        }
        let selected = remaining.peek().is_some_and(|r| r.contains(&at));
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if stack.is_empty() {
                    let (ns, _) = reader.resolver().resolve_element(e.name());
                    require(
                        name == "sec"
                            && matches!(ns,ResolveResult::Bound(ns) if ns.as_ref()=="http://www.hancom.co.kr/hwpml/2011/section"),
                    )?;
                }
                if selected {
                    let (ns, _) = reader.resolver().resolve_element(e.name());
                    require(
                        matches!(ns,ResolveResult::Bound(ns) if ns.as_ref()=="http://www.hancom.co.kr/hwpml/2011/paragraph"),
                    )?;
                    let parent = stack.last().map(String::as_str).unwrap_or_default();
                    require(matches!(
                        (parent, name),
                        ("sec", "p")
                            | ("p", "run" | "linesegarray")
                            | ("run", "t")
                            | ("linesegarray", "lineseg")
                    ))?;
                    let mut false_flags = 0;
                    for a in e.attributes() {
                        let a = a.map_err(|_| invalid())?;
                        let key = a.key.as_ref();
                        if key == "xmlns" || key.starts_with("xmlns:") {
                            continue;
                        }
                        let value = a
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| invalid())?;
                        let allowed = match name {
                            "p" => match key {
                                "id" | "paraPrIDRef" | "styleIDRef" => true,
                                "pageBreak" | "columnBreak" | "merged" => value == "0",
                                _ => false,
                            },
                            "run" => key == "charPrIDRef",
                            "lineseg" => matches!(
                                key,
                                "textpos"
                                    | "vertpos"
                                    | "vertsize"
                                    | "textheight"
                                    | "baseline"
                                    | "spacing"
                                    | "horzpos"
                                    | "horzsize"
                                    | "flags"
                            ),
                            _ => false,
                        };
                        require(allowed)?;
                        if matches!(key, "pageBreak" | "columnBreak" | "merged") {
                            false_flags += 1;
                        }
                    }
                    if name == "p" {
                        require(false_flags == 3)?;
                    }
                }
                if !empty {
                    stack.push(name.to_owned())
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(e) if selected => {
                require(stack.last().is_some_and(|s| s == "t") || e.as_ref().trim().is_empty())?;
            }
            Event::CData(_) | Event::GeneralRef(_) if selected => {
                require(stack.last().is_some_and(|s| s == "t"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::PI(_) | Event::DocType(_) | Event::Decl(_) if selected => {
                return Err(invalid())
            }
            _ => {}
        }
    }
    Ok(())
}
