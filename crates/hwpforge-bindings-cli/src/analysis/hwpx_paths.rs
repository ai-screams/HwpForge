use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;
use serde::Serialize;

use hwpforge_smithy_hwpx::{HwpxError, HwpxResult, PackageReader};

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
        path_inventory.extend(scan_section_xml(section_index, &xml)?);
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

pub(crate) fn scan_section_xml(
    section_index: usize,
    xml: &str,
) -> HwpxResult<Vec<HwpxPathOccurrence>> {
    let mut reader: Reader<&[u8]> = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf: Vec<u8> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut occurrences: Vec<HwpxPathOccurrence> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(element)) => {
                let name: String = local_name(element.name().as_ref()).to_string();
                let path: String = build_path(&stack, &name);
                record_element_occurrence(section_index, &path, &name, &element, &mut occurrences);
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                let qname = element.name();
                let name: &str = local_name(qname.as_ref());
                let path: String = build_path(&stack, name);
                record_element_occurrence(section_index, &path, name, &element, &mut occurrences);
            }
            Ok(Event::Text(text)) => {
                if stack.last().is_some_and(|name| name == "t") {
                    let decoded_text = text.xml_content(XmlVersion::Explicit1_0);
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
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => {
                if !stack.is_empty() {
                    return Err(HwpxError::XmlParse {
                        file: format!("Contents/section{section_index}.xml"),
                        detail: format!("unexpected end of input, unclosed <{}>", stack.join("/")),
                    });
                }
                break;
            }
            Ok(_) => {}
            Err(err) => {
                return Err(HwpxError::XmlParse {
                    file: format!("Contents/section{section_index}.xml"),
                    detail: err.to_string(),
                });
            }
        }

        buf.clear();
    }

    Ok(occurrences)
}

fn record_element_occurrence(
    section_index: usize,
    path: &str,
    name: &str,
    element: &BytesStart<'_>,
    occurrences: &mut Vec<HwpxPathOccurrence>,
) {
    if !is_interesting_element(name) {
        return;
    }

    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    for attribute in element.attributes().with_checks(false).flatten() {
        let key: &str = local_name(attribute.key.as_ref());
        if matches!(key, "binaryItemIDRef" | "chartIDRef") {
            if let Ok(value) = attribute.normalized_value(XmlVersion::Explicit1_0) {
                refs.insert(key.to_string(), value.into_owned());
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

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_reports_xml_error_instead_of_truncating() {
        // 이것을 실패시키는 것: `Err(err)` 팔을 `Err(_) => break` 로 되돌리는 변경
        // (앞부분 목록만 Ok 로 반환됨)
        let xml = "<hs:sec><hp:tbl/><hp:t>x</hp:T></hs:sec>";
        let err = scan_section_xml(3, xml).unwrap_err();
        match err {
            HwpxError::XmlParse { file, detail } => {
                assert_eq!(file, "Contents/section3.xml");
                assert!(detail.contains("hp:T"), "{detail}");
            }
            other => panic!("expected XmlParse, got {other:?}"),
        }
    }

    #[test]
    fn scan_reports_unclosed_element_at_eof() {
        // 이것을 실패시키는 것: `Eof` 팔의 `!stack.is_empty()` 검사 삭제
        // (raw reader 는 닫히지 않은 요소가 있어도 Eof 를 정상으로 돌려줌)
        let err = scan_section_xml(0, "<hs:sec><hp:tbl/>").unwrap_err();
        assert!(matches!(err, HwpxError::XmlParse { .. }), "{err:?}");
    }

    #[test]
    fn scan_accepts_well_formed_xml() {
        let xml = "<hs:sec><hp:tbl/><hp:t>x</hp:t></hs:sec>";
        let found = scan_section_xml(0, xml).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, "tbl");
        assert_eq!(found[1].text.as_deref(), Some("x"));
    }
}
