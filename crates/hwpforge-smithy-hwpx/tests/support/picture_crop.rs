#[path = "package.rs"]
mod package;
#[path = "xml.rs"]
mod xml;

use super::*;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}

/// Each picture's frame elements, in document order: `orgSz`, `imgClip`,
/// `imgDim` (or `-`), and the `scaMatrix` `e1`/`e5` as numbers.
pub(super) fn frames(xml: &str) -> Vec<(String, String, String, (f64, f64))> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<hp:pic ") {
        let end = at + rest[at..].find("</hp:pic>").expect("pic end");
        let pic = &rest[at..end];
        let element = |tag: &str| {
            pic.find(&format!("<{tag} "))
                .map(|p| pic[p..p + pic[p..].find("/>").unwrap() + 2].to_string())
                .unwrap_or_else(|| "-".to_string())
        };
        let sca = element("hc:scaMatrix");
        let num = |name: &str| attr(&sca, name).parse::<f64>().unwrap();
        out.push((
            element("hp:orgSz"),
            element("hp:imgClip"),
            element("hp:imgDim"),
            (num("e1"), num("e5")),
        ));
        rest = &rest[end..];
    }
    out
}

pub(super) fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    xml::attr(tag, name)
}

pub(super) fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}
