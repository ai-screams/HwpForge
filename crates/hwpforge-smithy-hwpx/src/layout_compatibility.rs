//! Header compatibility belongs to this codec; Core carries only its meaning.
use hwpforge_core::{Document, LayoutProfile};
use quick_xml::{events::Event, name::ResolveResult, NsReader};

use crate::{DecodeWarning, HwpxError, HwpxResult};

#[derive(Debug, Clone)]
pub(crate) struct Compatibility {
    pub(crate) profile: LayoutProfile,
    xml: String,
    standard_hancom: bool,
}

impl Compatibility {
    // The header has already passed the decoder's XML size/depth checks.
    // Keep lexical content, adding only ancestor bindings absent from the output root.
    pub(crate) fn read(xml: &str, warnings: &mut Vec<DecodeWarning>) -> HwpxResult<Self> {
        let error =
            |detail: String| HwpxError::XmlParse { file: "Contents/header.xml".into(), detail };
        let mut reader = NsReader::from_str(xml);
        let mut depth = 0usize;
        let mut inherited = Vec::new();
        let mut local_bindings = Vec::new();
        let mut needed = std::collections::BTreeSet::new();
        let mut start = None;
        let mut fragment = None;
        let mut profile = LayoutProfile::Hancom;
        let mut standard_hancom = true;
        let mut layouts = 0;
        loop {
            let at = reader.buffer_position() as usize;
            let event = reader.read_event().map_err(|e| error(e.to_string()))?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    // Unrelated unknown subtrees retain the decoder's skip policy.
                    // 이것을 실패시키는 것: 깊은 미지 header 하위 트리를 namespace 스택에 적재.
                    if depth == 1 && e.local_name().as_ref() != "compatibleDocument" {
                        if !empty {
                            reader.read_to_end(e.name()).map_err(|e| error(e.to_string()))?;
                        }
                        continue;
                    }
                    if depth > crate::decoder::XML_RECURSION_LIMIT {
                        return Err(error("compatibility XML nesting limit".into()));
                    }

                    if depth == 0 {
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| error(e.to_string()))?;
                            let key = attr.key.as_ref();
                            if key == "xmlns" || key.starts_with("xmlns:") {
                                let value = attr
                                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                    .map_err(|e| error(e.to_string()))?;
                                let declaration = format!(
                                    " {key}=\"{}\"",
                                    quick_xml::escape::escape(value.as_ref())
                                );
                                if !crate::encoder::package::XMLNS_DECLS.contains(&declaration) {
                                    inherited.push((key.to_owned(), declaration));
                                }
                            }
                        }
                    }
                    if depth == 1 && e.local_name().as_ref() == "compatibleDocument" {
                        if start.is_some() || fragment.is_some() {
                            return Err(error("duplicate compatibleDocument".into()));
                        }
                        let mut target = None;
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| error(e.to_string()))?;
                            if attr.key.as_ref() == "targetProgram" {
                                target = Some(
                                    attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                        .map_err(|e| error(e.to_string()))?
                                        .into_owned(),
                                );
                            }
                        }
                        profile = match target.as_deref() {
                            Some("MS_WORD") => LayoutProfile::WordCompatible,
                            Some("HWP201X") | None => LayoutProfile::Hancom,
                            Some(raw) => {
                                warnings.push(DecodeWarning::UnknownEnumValue {
                                    attribute: "hh:compatibleDocument@targetProgram",
                                    raw: raw.into(),
                                    fallback: "HWP201X",
                                });
                                LayoutProfile::Hancom
                            }
                        };
                        standard_hancom = target.as_deref() == Some("HWP201X");
                        start = Some((at, reader.buffer_position() as usize));
                    }
                    if start.is_some() {
                        let element_namespace = reader.resolver().resolve_element(e.name()).0;
                        if matches!(element_namespace, ResolveResult::Unknown(_)) {
                            return Err(error("unbound compatibility element prefix".into()));
                        }
                        standard_hancom &= matches!(element_namespace, ResolveResult::Bound(ns) if ns.as_ref() == "http://www.hancom.co.kr/hwpml/2011/head");
                        if depth == 2 {
                            layouts += 1;
                        }
                        standard_hancom &= depth == 1
                            || (depth == 2 && e.local_name().as_ref() == "layoutCompatibility");
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| error(e.to_string()))?;
                            attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                .map_err(|e| error(e.to_string()))?;
                            if matches!(
                                reader.resolver().resolve_attribute(attr.key).0,
                                ResolveResult::Unknown(_)
                            ) {
                                return Err(error("unbound compatibility attribute prefix".into()));
                            }
                            let key = attr.key.as_ref();
                            if key == "xmlns" || key.starts_with("xmlns:") {
                                local_bindings.push((depth, key.to_owned()));
                            } else {
                                standard_hancom &= depth == 1 && key == "targetProgram";
                            }
                        }
                        // Only references without a binding inside the fragment need
                        // their original root declaration. Attribute values stay lexical.
                        let mut use_name = |name: &str, element: bool| {
                            let key = if let Some((prefix, _)) = name.split_once(':') {
                                format!("xmlns:{prefix}")
                            } else if element {
                                "xmlns".to_owned()
                            } else {
                                return;
                            };
                            if !local_bindings.iter().any(|(_, local)| *local == key) {
                                needed.insert(key);
                            }
                        };
                        use_name(e.name().as_ref(), true);
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| error(e.to_string()))?;
                            let key = attr.key.as_ref();
                            if key != "xmlns" && !key.starts_with("xmlns:") {
                                use_name(key, false);
                            }
                        }
                        if empty {
                            local_bindings.retain(|(at, _)| *at < depth);
                        }
                    }
                    if !empty {
                        depth += 1;
                    }
                    if empty && depth == 1 && start.is_some() {
                        fragment = Some(finish(
                            xml,
                            start.take().unwrap(),
                            reader.buffer_position() as usize,
                            &declarations(&inherited, &needed),
                        ));
                    }
                }
                Event::End(_) => {
                    depth -= 1;
                    local_bindings.retain(|(at, _)| *at < depth);
                    if depth == 1 {
                        if let Some(start) = start.take() {
                            fragment = Some(finish(
                                xml,
                                start,
                                reader.buffer_position() as usize,
                                &declarations(&inherited, &needed),
                            ));
                        }
                    }
                }
                Event::GeneralRef(e) if start.is_some() => {
                    let text = crate::wire_xml::resolve_general_ref(&e)
                        .map_err(|e| error(e.to_string()))?;
                    standard_hancom &= text.as_bytes().iter().all(u8::is_ascii_whitespace);
                }
                Event::Text(e) if start.is_some() => {
                    standard_hancom &= e.as_bytes().iter().all(u8::is_ascii_whitespace);
                }
                Event::CData(e) if start.is_some() => {
                    standard_hancom &= e.as_bytes().iter().all(u8::is_ascii_whitespace);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        standard_hancom &= fragment.is_none() || layouts == 1;
        let xml = fragment.unwrap_or_default();
        Ok(Self { profile, xml, standard_hancom })
    }

    pub(crate) fn xml(&self) -> &str {
        &self.xml
    }

    pub(crate) fn proves_single_line_width(&self, profile: LayoutProfile) -> bool {
        profile == LayoutProfile::Hancom && self.standard_hancom
    }
}

fn declarations(
    inherited: &[(String, String)],
    needed: &std::collections::BTreeSet<String>,
) -> String {
    inherited
        .iter()
        .filter(|(key, _)| needed.contains(key))
        .map(|(_, declaration)| declaration.as_str())
        .collect()
}

fn finish(
    xml: &str,
    (start, opening_end): (usize, usize),
    end: usize,
    declarations: &str,
) -> String {
    let mut fragment = xml[start..end].to_owned();
    let offset = opening_end - start - if xml[..opening_end].ends_with("/>") { 2 } else { 1 };
    fragment.insert_str(offset, declarations);
    fragment
}

pub(crate) fn for_document<S>(document: &Document<S>) -> String {
    if let Some(basis) = document.format_source::<crate::decoder::SourceBasis>() {
        if basis.compatibility.profile == document.layout_profile() {
            return basis.compatibility.xml().to_owned();
        }
    }
    crate::encoder::header::generated_compatibility(document.layout_profile()).into()
}

pub(crate) fn changed<S>(document: &Document<S>) -> bool {
    document
        .format_source::<crate::decoder::SourceBasis>()
        .is_some_and(|basis| basis.compatibility.profile != document.layout_profile())
}

pub(crate) fn require_same<A, B>(submitted: &Document<A>, base: &Document<B>) -> HwpxResult<()> {
    if submitted.layout_profile() != base.layout_profile() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "/layout_profile: submitted {:?}, base {:?}",
                submitted.layout_profile(),
                base.layout_profile()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 이것을 실패시키는 것: 프로필 또는 표준 설정 조건 중 한쪽 생략.
    #[test]
    fn single_line_width_requires_both_profile_and_standard_settings() {
        for (profile, standard, expected) in [
            (LayoutProfile::WordCompatible, false, false),
            (LayoutProfile::WordCompatible, true, false),
            (LayoutProfile::Hancom, false, false),
            (LayoutProfile::Hancom, true, true),
        ] {
            let compatibility =
                Compatibility { profile, xml: String::new(), standard_hancom: standard };
            assert_eq!(compatibility.proves_single_line_width(profile), expected);
        }
    }

    // 이것을 실패시키는 것: 미지 자식·속성·내용·namespace·중복 또는 없는 layoutCompatibility 승인,
    // targetProgram 누락 승인, 공백·빈 요소 표기 차이 거부.
    #[test]
    fn standard_hancom_is_a_structure_with_one_empty_layout() {
        for (body, expected) in [
            ("", true),
            ("<hh:compatibleDocument targetProgram='MS_WORD'><hh:layoutCompatibility/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument><hh:layoutCompatibility/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'/>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility/><hh:layoutCompatibility/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:future/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility><hh:future/></hh:layoutCompatibility></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X' extra='1'><hh:layoutCompatibility/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility extra='1'/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility xmlns:hh='urn:foreign'/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'>x<hh:layoutCompatibility/></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility><![CDATA[x]]></hh:layoutCompatibility></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility>&amp;</hh:layoutCompatibility></hh:compatibleDocument>", false),
            ("<hh:compatibleDocument targetProgram = 'HWP201X'>\n<hh:layoutCompatibility > </hh:layoutCompatibility>\n</hh:compatibleDocument>", true),
            ("<hh:compatibleDocument targetProgram='HWP201X'><hh:layoutCompatibility /></hh:compatibleDocument>", true),
        ] {
            let xml = format!("<hh:head xmlns:hh='http://www.hancom.co.kr/hwpml/2011/head'>{body}</hh:head>");
            let c = Compatibility::read(&xml, &mut Vec::new()).unwrap();
            assert_eq!(c.standard_hancom, expected, "{body}");
        }
    }
}
