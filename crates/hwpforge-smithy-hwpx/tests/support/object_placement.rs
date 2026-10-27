#[path = "package.rs"]
mod package;
#[path = "xml.rs"]
mod xml;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}

/// The first `<hp:pos …/>` after the first `<{tag} ` element start, and that
/// element's start tag.
pub(super) fn element_and_pos(xml: &str, tag: &str) -> (String, String) {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    let start = xml[at..at + xml[at..].find('>').unwrap() + 1].to_string();
    let p = at + xml[at..].find("<hp:pos ").expect("pos");
    let pos = xml[p..p + xml[p..].find("/>").unwrap() + 2].to_string();
    (start, pos)
}

pub(super) fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    xml::attr(tag, name)
}
