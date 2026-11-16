//! Reject structures whose retained line breaks (`textpos`, `horzsize`) can depend
//! on vertical position: side-wrapped or unreadable objects, multiple columns,
//! vertical writing, and ambiguous section/page settings.
//!
//! A table-hosting line's saved height can change when the table splits across
//! pages. This remains allowed: Hancom lays tables out when opening the document;
//! floating table splits were observed to be recalculated despite stale heights.
use super::{verification_error, StructuralEditError};
use crate::raw_package::RawPackage;
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    NsReader, XmlVersion,
};

const HP: &str = crate::wire_xml::PARAGRAPH_NS;
const HC: &str = "http://www.hancom.co.kr/hwpml/2011/core";
const HS: &str = crate::wire_xml::SECTION_NS;
const MASTER: &str = "http://www.hancom.co.kr/hwpml/2011/master-page";
/// Saved parts declare their namespaces once on the root (fewer than 20 in observed
/// Hancom output). Element prefixes are resolved against every active declaration, so
/// an unbounded count would make the scan quadratic in the input size.
const MAX_NAMESPACE_DECLARATIONS: usize = 64;

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
/// Objects whose wrapping is read from `textWrap` and `hp:pos`. This is narrower than
/// `cache_flow::object_element` on purpose: text art, OLE, video and form controls are
/// decoded, but their placement here has not been checked against Hancom, so a document
/// holding them keeps its original admission error.
fn is_object(name: &str) -> bool {
    matches!(
        name,
        "tbl"
            | "pic"
            | "rect"
            | "line"
            | "ellipse"
            | "arc"
            | "polygon"
            | "curve"
            | "connectLine"
            | "container"
            | "equation"
            | "chart"
    )
}
/// Children allowed in a slot where wrapping objects can appear, besides the objects.
/// `None` means the parent is not such a slot.
fn slot_child(parent: &str, name: &str) -> Option<bool> {
    let allowed = match parent {
        "run" | "case" | "default" => {
            matches!(name, "secPr" | "t" | "ctrl" | "switch" | "titleMark" | "dutmal" | "compose")
        }
        "ctrl" => matches!(
            name,
            "colPr"
                | "header"
                | "footer"
                | "pageNum"
                | "footNote"
                | "endNote"
                | "bookmark"
                | "indexmark"
                | "fieldBegin"
                | "fieldEnd"
                | "autoNum"
                | "newNum"
                | "pageHiding"
        ),
        "container" => matches!(
            name,
            "offset"
                | "orgSz"
                | "curSz"
                | "renderingInfo"
                | "pos"
                | "sz"
                | "flip"
                | "rotationInfo"
                | "outMargin"
                | "shapeComment"
                | "caption"
                | "parameterset"
        ),
        _ => return None,
    };
    Some(allowed)
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

fn namespace(resolved: ResolveResult<'_>) -> Namespace {
    match resolved {
        ResolveResult::Unbound => Namespace::None,
        ResolveResult::Bound(ns) => match ns.as_ref() {
            HP => Namespace::Paragraph,
            HC => Namespace::Core,
            HS => Namespace::Section,
            MASTER => Namespace::Master,
            _ => Namespace::Other,
        },
        _ => Namespace::Other,
    }
}

fn is_root(master: bool, ns: Namespace, name: &str) -> bool {
    if master {
        name == "masterPage" && matches!(ns, Namespace::None | Namespace::Master)
    } else {
        name == "sec" && ns == Namespace::Section
    }
}

/// Wrapping objects occur in runs (including alternative branches), containers, or
/// controls. Static descendants elsewhere need no name inventory.
fn check_child(
    parent: &Frame,
    ns: Namespace,
    name: &str,
    object: bool,
) -> Result<(), StructuralEditError> {
    // 이것을 실패시키는 것: 정적 하위 요소에 다른 이름공간 승인.
    require(matches!(ns, Namespace::Paragraph | Namespace::Core))?;
    if parent.ns == Namespace::Paragraph {
        if let Some(listed) = slot_child(&parent.name, name) {
            // 이것을 실패시키는 것: ctrl/run/container/대안 분기의 자식 목록 검사 생략.
            require(ns == Namespace::Paragraph && (listed || parent.name != "ctrl" && object))?;
        }
    }
    Ok(())
}

/// The attributes the scan reads from one element.
struct Attributes {
    wrap: Option<String>,
    as_char: Option<String>,
    columns: Option<String>,
    direction: Option<String>,
}

impl Attributes {
    fn read(e: &BytesStart<'_>, declarations: &mut usize) -> Result<Self, StructuralEditError> {
        let mut attrs = Self { wrap: None, as_char: None, columns: None, direction: None };
        for a in e.attributes() {
            let a = a.map_err(|_| invalid())?;
            let value = a.normalized_value(XmlVersion::Explicit1_0).map_err(|_| invalid())?;
            let key = a.key.as_ref();
            if key == "xmlns" || key.starts_with("xmlns:") {
                *declarations += 1;
                // 이것을 실패시키는 것: 이름공간 선언 수 상한 생략 또는 상한을 실제 문서보다 낮춤.
                require(*declarations <= MAX_NAMESPACE_DECLARATIONS)?;
            }
            match key {
                "textWrap" => attrs.wrap = Some(value.into_owned()),
                "treatAsChar" => attrs.as_char = Some(value.into_owned()),
                "colCount" => attrs.columns = Some(value.into_owned()),
                "textDirection" => attrs.direction = Some(value.into_owned()),
                _ => {}
            }
        }
        Ok(attrs)
    }
}

/// Section and page settings seen in a section part.
#[derive(Default)]
struct Settings {
    sections: usize,
    pages: usize,
}

impl Settings {
    fn record(
        &mut self,
        name: &str,
        master: bool,
        attrs: &Attributes,
    ) -> Result<(), StructuralEditError> {
        match name {
            // 이것을 실패시키는 것: 다단 또는 colCount 누락 승인.
            "colPr" => require(attrs.columns.as_deref() == Some("1")),
            "secPr" if !master => {
                self.sections += 1;
                // 이것을 실패시키는 것: 세로쓰기 승인.
                require(attrs.direction.as_deref() == Some("HORIZONTAL"))
            }
            "pagePr" if !master => {
                self.pages += 1;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn check_xml(xml: &str, master: bool) -> Result<(), StructuralEditError> {
    let input = crate::wire_xml::PositionedXml::new(xml);
    let mut reader = NsReader::from_str(input.as_str());
    let mut stack = Vec::<Frame>::new();
    let (mut roots, mut declarations) = (0, 0);
    let mut settings = Settings::default();
    loop {
        let event = reader.read_event().map_err(|_| invalid())?;
        crate::work::tick();
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let ns = namespace(reader.resolver().resolve_element(e.name()).0);
                let name = e.local_name().as_ref().to_owned();
                let object = ns == Namespace::Paragraph && is_object(&name);
                if let Some(parent) = stack.last() {
                    check_child(parent, ns, &name, object)?;
                } else {
                    roots += 1;
                    // 이것을 실패시키는 것: 바탕쪽 또는 구역 루트 이름/이름공간 검사 생략.
                    require(roots == 1 && is_root(master, ns, &name))?;
                }
                let mut attrs = Attributes::read(&e, &mut declarations)?;
                if name == "pos" {
                    let parent = stack.last_mut().ok_or_else(invalid)?;
                    // 이것을 실패시키는 것: 다른 자리의 미지 개체 또는 다른 이름공간의 pos 승인.
                    require(ns == Namespace::Paragraph && parent.object)?;
                    parent.positions += 1;
                    parent.as_char = attrs.as_char.take();
                }
                if ns == Namespace::Paragraph {
                    settings.record(&name, master, &attrs)?;
                }
                let frame =
                    Frame { ns, name, object, wrap: attrs.wrap, positions: 0, as_char: None };
                if empty {
                    close(frame)?;
                } else {
                    // 이것을 실패시키는 것: 요소 깊이 상한 생략(디코더와 같은 상한).
                    require(stack.len() < crate::decoder::XML_RECURSION_LIMIT)?;
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
    require(
        roots == 1 && stack.is_empty() && (master || settings.sections == 1 && settings.pages == 1),
    )
}
