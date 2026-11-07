//! Bounded event parsing shared by structural reference checks.
use super::StructuralEditError;
use quick_xml::{events::Event, Reader};
use std::collections::BTreeMap;

fn refused() -> StructuralEditError {
    super::verification_error(crate::HwpxError::InvalidStructure {
        detail: "unresolved paragraph reference during structural edit".into(),
    })
}

/// Read field values once as XML events; consumers never interpret inner XML.
pub(super) fn scan(
    xml: &str,
    mut attribute: impl FnMut(&str, &str) -> Result<(), StructuralEditError>,
    mut field_value: impl FnMut(&str, &BTreeMap<String, String>) -> Result<(), StructuralEditError>,
) -> Result<(), StructuralEditError> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut field: Option<(usize, String, BTreeMap<String, String>)> = None;
    let mut parameter: Option<(usize, String, String)> = None;
    let mut container = None;
    loop {
        let event = reader.read_event().map_err(|_| refused())?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                // 이것을 실패시키는 것: 깊이 제한 제거 또는 경계 한 단계 축소.
                if depth >= crate::decoder::XML_RECURSION_LIMIT {
                    return Err(super::verification_error(crate::HwpxError::InvalidStructure {
                        detail: "invalid XML depth/root count".into(),
                    }));
                }
                depth += 1;
                let mut attrs = BTreeMap::new();
                for a in e.attributes() {
                    let a = a.map_err(|_| refused())?;
                    let key = a.key.as_ref();
                    attrs.insert(
                        key.to_owned(),
                        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| refused())?
                            .into_owned(),
                    );
                }
                for (key, value) in &attrs {
                    attribute(key, value)?;
                }
                let local = e.local_name();
                let name = local.as_ref();
                if name == "fieldBegin" {
                    // Nested fields cannot establish an unambiguous parameter owner.
                    // 이것을 실패시키는 것: 중첩 fieldBegin으로 바깥 필드 상태 덮어쓰기.
                    if field.is_some() {
                        return Err(refused());
                    }
                    field =
                        Some((depth, attrs.remove("type").unwrap_or_default(), BTreeMap::new()));
                } else if let Some((field_depth, _, _)) = &field {
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
                    close(depth, &mut field, &mut parameter, &mut container, &mut field_value)?;
                    depth -= 1;
                }
            }
            Event::Text(e) => {
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&e.xml10_content());
                }
            }
            Event::CData(e) => {
                // 이것을 실패시키는 것: CDATA 텍스트를 parameter 값에서 누락.
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&e.xml10_content());
                }
            }
            Event::GeneralRef(e) => {
                // 이것을 실패시키는 것: 문자 참조를 parameter 값에서 누락.
                let text = crate::wire_xml::resolve_general_ref(&e).map_err(|_| refused())?;
                if let Some((_, _, value)) = &mut parameter {
                    value.push_str(&text);
                }
            }
            Event::End(_) => {
                close(depth, &mut field, &mut parameter, &mut container, &mut field_value)?;
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

type Field = Option<(usize, String, BTreeMap<String, String>)>;
type Parameter = Option<(usize, String, String)>;

fn close(
    depth: usize,
    field: &mut Field,
    parameter: &mut Parameter,
    container: &mut Option<usize>,
    field_value: &mut impl FnMut(&str, &BTreeMap<String, String>) -> Result<(), StructuralEditError>,
) -> Result<(), StructuralEditError> {
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
        let (_, kind, values) = field.take().unwrap();
        field_value(&kind, &values)?;
    }
    Ok(())
}
