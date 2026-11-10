//! Bounded event parsing shared by structural reference checks.
use super::StructuralEditError;
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::collections::{BTreeMap, BTreeSet};

// Descriptor callbacks see descendant text for every ancestor. Bound normalized
// copies by the longest deletion key, rather than allocating from the whole text.
pub(crate) fn comparison_value(
    value: &str,
    max_len: usize,
) -> Result<Option<String>, &'static str> {
    comparison_bytes(value.bytes(), max_len)
}

// Share the same length proof with refusal-only decoding, without allocating an
// unbounded decoded string for every ancestor of a long descriptor text value.
pub(crate) fn comparison_bytes(
    bytes: impl Iterator<Item = u8>,
    max_len: usize,
) -> Result<Option<String>, &'static str> {
    let limit = max_len.saturating_mul(4).saturating_add(64);
    let mut short = Vec::new();
    let mut len = 0usize;
    let mut part_len = 0usize;
    let mut dots = true;
    let mut parent = false;
    let mut retained = 0usize;
    for byte in bytes.chain(std::iter::once(b'/')) {
        if byte == b'/' || byte == b'\\' {
            parent |= dots && part_len == 2;
            if part_len != 0 && !(dots && part_len == 1) {
                retained = retained.saturating_add(part_len).saturating_add(1);
            }
            part_len = 0;
            dots = true;
        } else {
            part_len = part_len.saturating_add(1);
            dots &= byte == b'.';
        }
        len = len.saturating_add(1);
        if len <= limit.saturating_add(1) {
            short.push(byte);
        }
    }
    // The synthetic final separator above is not part of the compared value.
    if len.saturating_sub(1) > limit {
        if parent {
            return Err("overlong descriptor path with parent components");
        }
        if retained <= max_len.saturating_add(1) {
            return Err("overlong descriptor path with collapsible components");
        }
        return Ok(None);
    }
    short.pop();
    // Deleted package keys are ASCII. Invalid decoded octets cannot spell one,
    // but a later parent component can erase them, so normalize before comparing.
    let value = String::from_utf8_lossy(&short);
    Ok(Some(crate::raw_package::integrity::comparison_path(&value)))
}

/// Read field values once as XML events; consumers never interpret inner XML.
pub(super) fn scan(
    xml: &str,
    mut attribute: impl FnMut(&str, &str) -> Result<(), StructuralEditError>,
    mut field_value: impl FnMut(&str, &BTreeMap<String, String>) -> Result<(), StructuralEditError>,
) -> Result<(), StructuralEditError> {
    scan_values(
        xml,
        true,
        super::verification_error,
        |_, key, value| attribute(key, value),
        |_, _| Ok(()),
        |kind, values| field_value(kind, values),
    )
}

/// Same bounded event loop for descriptor values and structural fields.
pub(crate) fn scan_values<E>(
    xml: &str,
    fields: bool,
    failure: impl Fn(crate::HwpxError) -> E,
    attribute: impl FnMut(usize, &str, &str) -> Result<(), E>,
    text_value: impl FnMut(usize, &str) -> Result<(), E>,
    mut field_value: impl FnMut(&str, &BTreeMap<String, String>) -> Result<(), E>,
) -> Result<(), E> {
    scan_positioned_values(xml, fields, failure, attribute, text_value, |_, kind, values| {
        field_value(kind, values)
    })
}

/// Raw begin offsets join normalized values to the decoder's source identities.
pub(crate) fn scan_fields(
    xml: &str,
    field: impl FnMut(usize, &str, &BTreeMap<String, String>) -> crate::HwpxResult<()>,
) -> crate::HwpxResult<()> {
    scan_positioned_values(xml, true, |e| e, |_, _, _| Ok(()), |_, _| Ok(()), field)
}

fn scan_positioned_values<E>(
    xml: &str,
    fields: bool,
    failure: impl Fn(crate::HwpxError) -> E,
    mut attribute: impl FnMut(usize, &str, &str) -> Result<(), E>,
    mut text_value: impl FnMut(usize, &str) -> Result<(), E>,
    mut field_value: impl FnMut(usize, &str, &BTreeMap<String, String>) -> Result<(), E>,
) -> Result<(), E> {
    let refused = || {
        failure(crate::HwpxError::InvalidStructure {
            detail: if fields {
                "unresolved paragraph reference during structural edit"
            } else {
                "invalid descriptor XML values"
            }
            .into(),
        })
    };
    // Both readers address the same BOM-free input; callbacks address original bytes.
    let input = crate::wire_xml::PositionedXml::new(xml);
    let mut reader = NsReader::from_str(input.as_str());
    // Structural fields keep their existing lexical namespace policy. Only
    // descriptor values need expanded attribute names and reserved-prefix checks.
    let mut field_reader = fields.then(|| quick_xml::Reader::from_str(input.as_str()));
    let mut depth = 0usize;
    let mut ordinal = 0;
    let mut texts: Vec<(usize, usize, String)> = Vec::new();
    let mut all_text = String::new();
    let mut field: Field = None;
    let mut parameter: Option<(usize, String, String)> = None;
    let mut container = None;
    loop {
        let event = match &mut field_reader {
            Some(reader) => reader.read_event(),
            None => reader.read_event(),
        }
        .map_err(|_| refused())?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                // 이것을 실패시키는 것: 깊이 제한 제거 또는 경계 한 단계 축소.
                if depth >= crate::decoder::XML_RECURSION_LIMIT {
                    return Err(failure(crate::HwpxError::InvalidStructure {
                        detail: "invalid XML depth/root count".into(),
                    }));
                }
                depth += 1;
                let mut attrs = BTreeMap::new();
                let mut expanded = BTreeSet::new();
                for a in e.attributes() {
                    let a = a.map_err(|_| refused())?;
                    if !fields {
                        let (namespace, local) = reader.resolver().resolve_attribute(a.key);
                        let namespace = match namespace {
                            ResolveResult::Bound(ns) => {
                                crate::raw_package::integrity::normalized_namespace(ns.as_ref())
                                    .map_err(|_| refused())?
                                    .into_owned()
                            }
                            ResolveResult::Unbound => String::new(),
                            ResolveResult::Unknown(_) => return Err(refused()),
                        };
                        if !expanded.insert((namespace, local.as_ref().to_string())) {
                            return Err(refused());
                        }
                    }
                    let key = a.key.as_ref();
                    attrs.insert(
                        key.to_owned(),
                        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| refused())?
                            .into_owned(),
                    );
                }
                for (key, value) in &attrs {
                    attribute(ordinal, key, value)?;
                }
                if let Some((id, _, direct)) = texts.last_mut() {
                    text_value(*id, direct)?;
                    direct.clear();
                }
                texts.push((ordinal, all_text.len(), String::new()));
                ordinal += 1;
                let local = e.local_name();
                let name = local.as_ref();
                if fields && name == "fieldBegin" {
                    // Nested fields cannot establish an unambiguous parameter owner.
                    // 이것을 실패시키는 것: 중첩 fieldBegin으로 바깥 필드 상태 덮어쓰기.
                    if field.is_some() {
                        return Err(refused());
                    }
                    field = Some((
                        depth,
                        attrs.remove("type").unwrap_or_default(),
                        BTreeMap::new(),
                        input.original_offset(field_reader.as_ref().unwrap().buffer_position())
                            - e.len()
                            - if empty { 3 } else { 2 },
                    ));
                } else if let Some((field_depth, _, _, _)) = &field {
                    // 이것을 실패시키는 것: parameters 밖 값 또는 미해석 자식 요소 무시.
                    if name == "parameters" {
                        if depth != field_depth + 1 {
                            return Err(refused());
                        }
                        container = Some(depth);
                    } else if let Some(container_depth) = container {
                        if !name.ends_with("Param") || depth != container_depth + 1 {
                            return Err(refused());
                        }
                        parameter =
                            Some((depth, attrs.remove("name").ok_or_else(refused)?, String::new()));
                    } else if name.ends_with("Param") {
                        return Err(refused());
                    }
                    // MEMO subList bodies are outside parameters; the existing
                    // admission and paragraph-reference policy still inspect them.
                }
                if empty {
                    let (id, start, direct) = texts.pop().ok_or_else(refused)?;
                    text_value(id, &all_text[start..])?;
                    text_value(id, &direct)?;
                    close(
                        depth,
                        &mut field,
                        &mut parameter,
                        &mut container,
                        &mut field_value,
                        &refused,
                    )?;
                    depth -= 1;
                }
            }
            Event::Text(e) => {
                if !fields {
                    append_text(&mut all_text, &mut texts, &e.xml10_content());
                }
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&e.xml10_content());
                }
            }
            Event::CData(e) => {
                if !fields {
                    append_text(&mut all_text, &mut texts, &e.xml10_content());
                }
                // 이것을 실패시키는 것: CDATA 텍스트를 parameter 값에서 누락.
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&e.xml10_content());
                }
            }
            Event::GeneralRef(e) => {
                // 이것을 실패시키는 것: 문자 참조를 parameter 값에서 누락.
                let text = crate::wire_xml::resolve_general_ref(&e).map_err(|_| refused())?;
                if !fields {
                    append_text(&mut all_text, &mut texts, &text);
                }
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&text);
                }
            }
            Event::End(_) => {
                let (id, start, direct) = texts.pop().ok_or_else(refused)?;
                text_value(id, &all_text[start..])?;
                text_value(id, &direct)?;
                close(
                    depth,
                    &mut field,
                    &mut parameter,
                    &mut container,
                    &mut field_value,
                    &refused,
                )?;
                depth = depth.checked_sub(1).ok_or_else(refused)?;
            }
            // 이것을 실패시키는 것: 주석/PI로 쪼갠 parameter 또는 DTD를 승인.
            Event::Comment(_) | Event::PI(_) if parameter.is_some() => return Err(refused()),
            Event::DocType(_) => return Err(refused()),
            Event::Eof => return if depth == 0 { Ok(()) } else { Err(refused()) },
            _ => {}
        }
    }
}

type Field = Option<(usize, String, BTreeMap<String, String>, usize)>;
type Parameter = Option<(usize, String, String)>;

fn close<E>(
    depth: usize,
    field: &mut Field,
    parameter: &mut Parameter,
    container: &mut Option<usize>,
    field_value: &mut impl FnMut(usize, &str, &BTreeMap<String, String>) -> Result<(), E>,
    refused: &impl Fn() -> E,
) -> Result<(), E> {
    if parameter.as_ref().is_some_and(|p| p.0 == depth) {
        let (_, name, value) = parameter.take().unwrap();
        // 이것을 실패시키는 것: 중복 parameter의 마지막 값으로 앞의 개요 참조 가리기.
        if field.as_mut().unwrap().2.insert(name, value).is_some() {
            return Err(refused());
        }
    }
    // 이것을 실패시키는 것: 컨테이너 종료 후에도 메모 본문을 parameter로 해석.
    if *container == Some(depth) {
        *container = None;
    }
    if field.as_ref().is_some_and(|f| f.0 == depth) {
        let (_, kind, values, start) = field.take().unwrap();
        field_value(start, &kind, &values)?;
    }
    Ok(())
}

// Descendant text is stored once; each open element keeps only its start offset.
fn append_text(all_text: &mut String, texts: &mut [(usize, usize, String)], value: &str) {
    all_text.push_str(value);
    if let Some((_, _, direct)) = texts.last_mut() {
        direct.push_str(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 긴 값도 정규화하거나 축약 가능한 긴 경로를 불일치로 통과.
    #[test]
    fn long_comparison_values_are_bounded_without_path_bypasses() {
        let target = "Preview/PrvText.txt";
        let bound = target.len() * 4 + 64;
        assert!(comparison_value(&"a".repeat(bound + 1), target.len()).unwrap().is_none());
        assert!(comparison_value(&"a".repeat(bound), target.len()).unwrap().is_some());
        assert!(comparison_value(&format!("https://host/{}", "a".repeat(bound)), target.len())
            .unwrap()
            .is_none());
        for prefix in ["a/../", "a\\..\\", "./", "////"] {
            let value = format!("{}{target}", prefix.repeat(bound));
            assert_eq!(
                crate::raw_package::integrity::comparison_path(&value),
                target.to_ascii_lowercase()
            );
            assert!(comparison_value(&value, target.len()).is_err(), "{prefix}");
        }
        assert_eq!(
            comparison_value(target, target.len()).unwrap().unwrap(),
            target.to_ascii_lowercase()
        );
    }

    // 이것을 실패시키는 것: 구역 필드에도 namespace 검사를 강제하거나 설명 파일의 검사를 제거.
    #[test]
    fn field_and_descriptor_namespace_policies_stay_separate() {
        for prefix in ["xml", "xmlns"] {
            let xml = format!("<p xmlns:{prefix}='urn:wrong'><fieldBegin type='HYPERLINK'><parameters><stringParam name='Path'>77</stringParam></parameters></fieldBegin></p>");
            let mut values = Vec::new();
            scan(
                &xml,
                |_, _| Ok(()),
                |kind, params| {
                    values.push((kind.to_string(), params.clone()));
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(
                values,
                vec![("HYPERLINK".into(), [("Path".into(), "77".into())].into_iter().collect())]
            );
            assert!(scan_values(
                &xml,
                false,
                |e| e,
                |_, _, _| Ok(()),
                |_, _| Ok(()),
                |_, _| Ok(())
            )
            .is_err());
        }
    }

    // 이것을 실패시키는 것: 열린 조상마다 전체 텍스트 사본을 다시 저장.
    #[test]
    fn descendant_text_uses_one_shared_buffer() {
        let mut text = String::new();
        let mut frames = (0..64).map(|i| (i, 0, String::new())).collect::<Vec<_>>();
        append_text(&mut text, &mut frames, &"x".repeat(1024));
        assert_eq!(text.len(), 1024);
        assert_eq!(frames.iter().map(|(_, _, direct)| direct.len()).sum::<usize>(), 1024);
        assert!(frames.iter().all(|(_, start, _)| text[*start..] == text));
    }
}
