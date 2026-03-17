use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use serde::Serialize;

use hwpforge_smithy_hwpx::{HwpxResult, PackageReader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HwpxPathOccurrence {
    pub section_index: usize,
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub(crate) fn collect_section_path_inventory(
    package_reader: &mut PackageReader<'_>,
) -> HwpxResult<Vec<HwpxPathOccurrence>> {
    let section_count: usize = package_reader.section_count();
    let mut path_inventory: Vec<HwpxPathOccurrence> = Vec::new();

    for section_index in 0..section_count {
        let xml: String = package_reader.read_section_xml(section_index)?;
        path_inventory.extend(scan_section_xml(section_index, &xml));
    }

    path_inventory.sort_by(|left, right| {
        left.section_index
            .cmp(&right.section_index)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.ref_id.cmp(&right.ref_id))
            .then_with(|| left.text.cmp(&right.text))
    });

    Ok(path_inventory)
}

pub(crate) fn scan_section_xml(section_index: usize, xml: &str) -> Vec<HwpxPathOccurrence> {
    let mut reader: Reader<&[u8]> = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf: Vec<u8> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut occurrences: Vec<HwpxPathOccurrence> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(element)) => {
                let name: String = local_name(element.name().as_ref());
                let path: String = build_path(&stack, &name);
                record_element_occurrence(
                    section_index,
                    &path,
                    &name,
                    &element,
                    reader.decoder(),
                    &mut occurrences,
                );
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                let name: String = local_name(element.name().as_ref());
                let path: String = build_path(&stack, &name);
                record_element_occurrence(
                    section_index,
                    &path,
                    &name,
                    &element,
                    reader.decoder(),
                    &mut occurrences,
                );
            }
            Ok(Event::Text(text)) => {
                if stack.last().is_some_and(|name| name == "t") {
                    if let Ok(decoded_text) = text.xml_content() {
                        let trimmed: &str = decoded_text.trim();
                        if !trimmed.is_empty() {
                            occurrences.push(HwpxPathOccurrence {
                                section_index,
                                kind: "text".to_string(),
                                path: build_path(&stack[..stack.len().saturating_sub(1)], "t"),
                                ref_id: None,
                                text: Some(trimmed.to_string()),
                            });
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }

        buf.clear();
    }

    occurrences
}

fn record_element_occurrence(
    section_index: usize,
    path: &str,
    name: &str,
    element: &BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
    occurrences: &mut Vec<HwpxPathOccurrence>,
) {
    if !is_interesting_element(name) {
        return;
    }

    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    for attribute in element.attributes().with_checks(false).flatten() {
        let key: String = local_name(attribute.key.as_ref());
        if matches!(key.as_str(), "binaryItemIDRef" | "chartIDRef") {
            if let Ok(value) = attribute.decode_and_unescape_value(decoder) {
                refs.insert(key, value.into_owned());
            }
        }
    }

    occurrences.push(HwpxPathOccurrence {
        section_index,
        kind: name.to_string(),
        path: path.to_string(),
        ref_id: refs.get("chartIDRef").cloned().or_else(|| refs.get("binaryItemIDRef").cloned()),
        text: None,
    });
}

fn is_interesting_element(name: &str) -> bool {
    matches!(
        name,
        "header"
            | "footer"
            | "subList"
            | "tbl"
            | "tc"
            | "rect"
            | "drawText"
            | "pic"
            | "img"
            | "chart"
            | "ole"
            | "switch"
            | "case"
            | "default"
            | "line"
            | "polygon"
            | "ellipse"
            | "curve"
            | "connectLine"
    )
}

fn build_path(stack: &[String], name: &str) -> String {
    let mut path: String = String::from("/");
    if !stack.is_empty() {
        path.push_str(&stack.join("/"));
        path.push('/');
    }
    path.push_str(name);
    path
}

fn local_name(bytes: &[u8]) -> String {
    let raw: String = String::from_utf8_lossy(bytes).into_owned();
    raw.rsplit(':').next().unwrap_or(raw.as_str()).to_string()
}
