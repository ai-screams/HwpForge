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
        path_inventory.extend(scan_section_xml(
            section_index,
            &package_reader.section_paths()[section_index],
            &xml,
        )?);
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

#[derive(Debug, Serialize)]
pub(crate) struct MasterPartInventory {
    pub section_index: usize,
    pub index: usize,
    pub part: String,
    pub occurrences: Vec<HwpxPathOccurrence>,
}

pub(crate) fn collect_master_path_inventory(
    pkg: &mut PackageReader<'_>,
) -> HwpxResult<Vec<MasterPartInventory>> {
    let paths = hwpforge_smithy_hwpx::master_page_part_paths(pkg)?;
    let mut out = Vec::new();
    for (section_index, parts) in paths.into_iter().enumerate() {
        for (index, part) in parts.into_iter().enumerate() {
            let xml = pkg.read_text_entry(&part)?;
            let occurrences = scan_section_xml(section_index, &part, &xml)?;
            out.push(MasterPartInventory { section_index, index, part, occurrences });
        }
    }
    Ok(out)
}

/// Character Core reads for an inline element inside `<hp:t>`, written either
/// as an empty element or with start and end tags.
fn inline_text_char(name: &str) -> Option<char> {
    match name {
        "tab" => Some('\t'),
        "lineBreak" => Some('\n'),
        "nbSpace" => Some('\u{a0}'),
        "fwSpace" => Some('\u{1f}'),
        _ => None,
    }
}

pub(crate) fn scan_section_xml(
    section_index: usize,
    part: &str,
    xml: &str,
) -> HwpxResult<Vec<HwpxPathOccurrence>> {
    let mut reader: Reader<&[u8]> = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf: Vec<u8> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut occurrences: Vec<HwpxPathOccurrence> = Vec::new();
    let mut text_value = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(element)) => {
                let name: String = local_name(element.name().as_ref()).to_string();
                let path: String = build_path(&stack, &name);
                record_element_occurrence(section_index, &path, &name, &element, &mut occurrences);
                if stack.last().is_some_and(|parent| parent == "t") {
                    text_value.extend(inline_text_char(&name));
                }
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                let qname = element.name();
                let name: &str = local_name(qname.as_ref());
                let path: String = build_path(&stack, name);
                record_element_occurrence(section_index, &path, name, &element, &mut occurrences);
                if stack.last().is_some_and(|parent| parent == "t") {
                    text_value.extend(inline_text_char(name));
                }
            }
            Ok(Event::Text(text)) => {
                if stack.last().is_some_and(|name| name == "t") {
                    text_value.push_str(&text.xml_content(XmlVersion::Explicit1_0));
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                let wire = format!("&{};", &*reference);
                let resolved = quick_xml::escape::unescape(&wire).map_err(|err| {
                    HwpxError::XmlParse { file: part.into(), detail: err.to_string() }
                })?;
                if stack.last().is_some_and(|name| name == "t") {
                    text_value.push_str(&resolved);
                }
            }
            Ok(Event::End(_)) => {
                if stack.last().is_some_and(|name| name == "t") {
                    let trimmed = quick_xml::utils::trim_xml_spaces(&text_value);
                    if !trimmed.is_empty() {
                        occurrences.push(HwpxPathOccurrence {
                            section_index,
                            kind: "text".to_string(),
                            path: build_path(&stack[..stack.len().saturating_sub(1)], "t"),
                            ref_id: None,
                            text: Some(trimmed.to_string()),
                        });
                    }
                    text_value.clear();
                }
                stack.pop();
            }
            Ok(Event::Eof) => {
                if !stack.is_empty() {
                    return Err(HwpxError::XmlParse {
                        file: part.into(),
                        detail: format!("unexpected end of input, unclosed <{}>", stack.join("/")),
                    });
                }
                break;
            }
            Ok(_) => {}
            Err(err) => {
                return Err(HwpxError::XmlParse { file: part.into(), detail: err.to_string() });
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
        let err = scan_section_xml(3, "Contents/section7.xml", xml).unwrap_err();
        match err {
            HwpxError::XmlParse { file, detail } => {
                assert_eq!(file, "Contents/section7.xml");
                assert!(detail.contains("hp:T"), "{detail}");
            }
            other => panic!("expected XmlParse, got {other:?}"),
        }
    }

    #[test]
    fn scan_reports_unclosed_element_at_eof() {
        // 이것을 실패시키는 것: `Eof` 팔의 `!stack.is_empty()` 검사 삭제
        // (raw reader 는 닫히지 않은 요소가 있어도 Eof 를 정상으로 돌려줌)
        let err = scan_section_xml(0, "Contents/section7.xml", "<hs:sec><hp:tbl/>").unwrap_err();
        assert!(
            matches!(err, HwpxError::XmlParse { ref file, .. } if file == "Contents/section7.xml"),
            "{err:?}"
        );
    }

    // 이것을 실패시키는 것: inventory가 파서에 실제 spine 파트 대신 숫자 파일명을 전달하기.
    // mismatch/EOF 두 오류 팔 모두 같은 실제 경로를 사용해야 한다.
    #[test]
    fn reversed_spine_inventory_error_names_the_actual_part() {
        use std::io::{Cursor, Write};
        for broken in ["<sec><t>x</T></sec>", "<sec><t/>"] {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (path, xml) in [
                ("mimetype", "application/hwp+zip"),
                ("Contents/section0.xml", "<sec/>"),
                ("Contents/section1.xml", broken),
                (
                    "Contents/content.hpf",
                    r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="a" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="b" href="Contents/section1.xml" media-type="application/xml"/></opf:manifest><opf:spine><opf:itemref idref="b"/><opf:itemref idref="a"/></opf:spine></opf:package>"#,
                ),
            ] {
                zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(xml.as_bytes()).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let mut reader = PackageReader::new(&bytes).unwrap();
            assert_eq!(reader.section_paths()[0], "Contents/section1.xml");
            let error = collect_section_path_inventory(&mut reader).unwrap_err();
            assert!(
                matches!(error, HwpxError::XmlParse { ref file, .. } if file == "Contents/section1.xml"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn scan_accepts_well_formed_xml() {
        let xml = "<hs:sec><hp:tbl/><hp:t>x</hp:t></hs:sec>";
        let found = scan_section_xml(0, "Contents/section0.xml", xml).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, "tbl");
        assert_eq!(found[1].text.as_deref(), Some("x"));
    }
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    // 이것을 실패시키는 것: 참조 무시, 조각별 trim, 인접 t 병합, 빈 값 기록,
    // Text·GeneralRef·End의 부모 t 가드 제거.
    #[test]
    fn text_reference_boundaries() {
        let rows = scan_section_xml(0, "section.xml", "<sec>outside&amp;<p><run><t> A &amp; B </t><t>&amp;</t><t>&amp;&lt;&#xAC00;</t><t>&amp;amp;</t><t> </t><t>C<tab>ignored&amp;</tab>D</t></run></p></sec>").unwrap();
        let texts: Vec<_> = rows.iter().filter_map(|r| r.text.as_deref()).collect();
        assert_eq!(texts, ["A & B", "&", "&<가", "&amp;", "C\tD"]);
    }
    // 이것을 실패시키는 것: 참조 오류 무시.
    #[test]
    fn invalid_text_reference() {
        for reference in ["&unknown;", "&#0;", "&#xD800;"] {
            assert!(scan_section_xml(0, "section.xml", &format!("<sec><t>{reference}</t></sec>"))
                .is_err());
        }
    }
}

#[cfg(test)]
mod inline_empty_tests {
    use super::*;

    // 이것을 실패시키는 것: 빈 인라인 요소별 문자 누락, 부모 t 가드 제거.
    #[test]
    fn empty_inline_text_characters() {
        for (tag, expected) in [
            ("tab", "\t"),
            ("lineBreak", "\n"),
            ("nbSpace", "\u{a0}"),
            ("fwSpace", "\u{1f}"),
            ("unknown", ""),
        ] {
            let xml = format!("<sec><hp:{tag}/><hp:p><hp:run><hp:t>이름<hp:{tag}/>홍길동</hp:t><hp:t>다음</hp:t></hp:run></hp:p></sec>");
            let rows = scan_section_xml(0, "section.xml", &xml).unwrap();
            let texts: Vec<_> = rows.iter().filter_map(|r| r.text.as_deref()).collect();
            assert_eq!(texts, [format!("이름{expected}홍길동"), "다음".into()]);
        }
    }

    // 이것을 실패시키는 것: 기록 앞뒤 정리를 XML 공백 4종이 아닌 유니코드 공백 기준(`str::trim`)으로
    // 되돌리기(전각 공백이 잘림), 또는 시작·끝 태그로 쓴 tab 을 무시하기.
    #[test]
    fn record_edges_follow_xml_whitespace() {
        let rows = scan_section_xml(
            0,
            "section.xml",
            "<sec><t> \u{3000}이름\u{3000} </t><t><tab/>홍길동</t><t>가<tab>무시</tab>나</t></sec>",
        )
        .unwrap();
        let texts: Vec<_> = rows.iter().filter_map(|r| r.text.as_deref()).collect();
        assert_eq!(texts, ["\u{3000}이름\u{3000}", "홍길동", "가\t나"]);
    }
}
