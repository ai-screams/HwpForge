//! Reject structures whose retained line breaks (`textpos`, `horzsize`) can depend
//! on vertical position: side-wrapped or unreadable objects, multiple columns,
//! vertical writing, and ambiguous section/page settings.
//!
//! A table-hosting line's saved height can change when the table splits across
//! pages. This remains allowed: Hancom lays tables out when opening the document;
//! floating table splits were observed to be recalculated despite stale heights.
use super::{verification_error, StructuralEditError};
use crate::raw_package::RawPackage;
use quick_xml::{events::Event, name::ResolveResult, NsReader, XmlVersion};

const HP: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";
const HC: &str = "http://www.hancom.co.kr/hwpml/2011/core";
const HS: &str = "http://www.hancom.co.kr/hwpml/2011/section";
const MASTER: &str = "http://www.hancom.co.kr/hwpml/2011/master-page";
const OBJECTS: &str =
    "tbl pic rect line ellipse arc polygon curve connectLine container equation chart";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Namespace {
    Paragraph,
    Core,
    Section,
    Master,
    None,
    Other,
}
struct Frame {
    ns: Namespace,
    name: String,
    object: bool,
    wrap: Option<String>,
    positions: usize,
    as_char: Option<String>,
}
fn contains(names: &str, name: &str) -> bool {
    names.split_ascii_whitespace().any(|n| n == name)
}
fn invalid() -> StructuralEditError {
    verification_error(crate::HwpxError::InvalidStructure {
        detail: "unproven line layout for source-preserving deletion".into(),
    })
}
fn require(value: bool) -> Result<(), StructuralEditError> {
    if value {
        Ok(())
    } else {
        Err(invalid())
    }
}

pub(super) fn check(package: &RawPackage, section_xml: &str) -> Result<(), StructuralEditError> {
    check_xml(section_xml, false)?;
    for path in package.entry_paths() {
        let lower = path.to_ascii_lowercase();
        if lower.starts_with("contents/masterpage") && lower.ends_with(".xml") {
            let bytes = package.read_entry(path).map_err(|_| invalid())?.to_vec();
            let xml = crate::package_xml::text_entry(path, bytes).map_err(|_| invalid())?;
            check_xml(&xml, true)?;
        }
    }
    Ok(())
}

fn close(frame: Frame) -> Result<(), StructuralEditError> {
    // 이것을 실패시키는 것: 배치 속성이 있는 미지 개체, 중복 pos, 누락 wrap 승인.
    if frame.object || frame.wrap.is_some() || frame.positions != 0 {
        require(frame.object && frame.positions <= 1)?;
        require(frame.positions == 0 || matches!(frame.as_char.as_deref(), Some("0" | "1")))?;
        let wrap = frame.wrap.as_deref().ok_or_else(invalid)?;
        require(matches!(
            wrap,
            "TOP_AND_BOTTOM" | "SQUARE" | "TIGHT" | "THROUGH" | "BEHIND_TEXT" | "IN_FRONT_OF_TEXT"
        ))?;
        // 이것을 실패시키는 것: pos 없는 개체에도 글자처럼 취급 예외 적용 또는 TOP_AND_BOTTOM만 허용.
        require(
            frame.positions == 1 && frame.as_char.as_deref() == Some("1")
                || !matches!(wrap, "SQUARE" | "TIGHT" | "THROUGH"),
        )?;
    }
    Ok(())
}

fn check_xml(xml: &str, master: bool) -> Result<(), StructuralEditError> {
    let input = crate::wire_xml::PositionedXml::new(xml);
    let mut reader = NsReader::from_str(input.as_str());
    let mut stack = Vec::<Frame>::new();
    let mut roots = 0;
    let (mut sections, mut pages) = (0, 0);
    loop {
        let event = reader.read_event().map_err(|_| invalid())?;
        crate::work::tick();
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let (resolved, _) = reader.resolver().resolve_element(e.name());
                let ns = match resolved {
                    ResolveResult::Unbound => Namespace::None,
                    ResolveResult::Bound(ns) => match ns.as_ref() {
                        HP => Namespace::Paragraph,
                        HC => Namespace::Core,
                        HS => Namespace::Section,
                        MASTER => Namespace::Master,
                        _ => Namespace::Other,
                    },
                    _ => Namespace::Other,
                };
                let name = e.local_name().as_ref().to_owned();
                let object = ns == Namespace::Paragraph && contains(OBJECTS, &name);
                if let Some(parent) = stack.last() {
                    // 이것을 실패시키는 것: 정적 하위 요소에 다른 이름공간 승인.
                    require(matches!(ns, Namespace::Paragraph | Namespace::Core))?;
                    // Wrapping objects occur in runs (including alternative branches),
                    // containers, or controls. Static descendants need no name inventory.
                    if parent.ns == Namespace::Paragraph {
                        let extra = match parent.name.as_str() {
                            "run" | "case" | "default" => Some("secPr t ctrl switch titleMark dutmal compose"),
                            "ctrl" => Some("colPr header footer pageNum footNote endNote bookmark indexmark fieldBegin fieldEnd autoNum newNum pageHiding"),
                            "container" => Some("offset orgSz curSz renderingInfo pos sz flip rotationInfo outMargin shapeComment caption parameterset"),
                            _ => None,
                        };
                        if let Some(extra) = extra {
                            // 이것을 실패시키는 것: ctrl/run/container/대안 분기의 자식 목록 검사 생략.
                            require(
                                ns == Namespace::Paragraph
                                    && (contains(extra, &name) || parent.name != "ctrl" && object),
                            )?;
                        }
                    }
                } else {
                    roots += 1;
                    // 이것을 실패시키는 것: 바탕쪽 또는 구역 루트 이름/이름공간 검사 생략.
                    require(
                        roots == 1
                            && if master {
                                name == "masterPage"
                                    && matches!(ns, Namespace::None | Namespace::Master)
                            } else {
                                name == "sec" && ns == Namespace::Section
                            },
                    )?;
                }
                let mut frame = Frame { ns, name, object, wrap: None, positions: 0, as_char: None };
                let mut as_char = None;
                let mut columns = None;
                let mut direction = None;
                for a in e.attributes() {
                    let a = a.map_err(|_| invalid())?;
                    let value =
                        a.normalized_value(XmlVersion::Explicit1_0).map_err(|_| invalid())?;
                    match a.key.as_ref() {
                        "textWrap" => frame.wrap = Some(value.into_owned()),
                        "treatAsChar" => as_char = Some(value.into_owned()),
                        "colCount" => columns = Some(value.into_owned()),
                        "textDirection" => direction = Some(value.into_owned()),
                        _ => {}
                    }
                }
                if frame.name == "pos" {
                    let parent = stack.last_mut().ok_or_else(invalid)?;
                    // 이것을 실패시키는 것: 다른 자리의 미지 개체 또는 다른 이름공간의 pos 승인.
                    require(ns == Namespace::Paragraph && parent.object)?;
                    parent.positions += 1;
                    parent.as_char = as_char;
                }
                if ns == Namespace::Paragraph {
                    match frame.name.as_str() {
                        "colPr" => {
                            // 이것을 실패시키는 것: 다단 또는 colCount 누락 승인.
                            require(columns.as_deref() == Some("1"))?;
                        }
                        "secPr" if !master => {
                            sections += 1;
                            // 이것을 실패시키는 것: 세로쓰기 승인.
                            require(direction.as_deref() == Some("HORIZONTAL"))?;
                        }
                        "pagePr" if !master => pages += 1,
                        _ => {}
                    }
                }
                if empty {
                    close(frame)?;
                } else {
                    stack.push(frame);
                }
            }
            Event::End(_) => close(stack.pop().ok_or_else(invalid)?)?,
            Event::DocType(_) => return Err(invalid()),
            Event::Eof => break,
            _ => {}
        }
    }
    // 이것을 실패시키는 것: 구역/쪽 설정 개수 검사 생략.
    require(roots == 1 && stack.is_empty() && (master || sections == 1 && pages == 1))
}
