use super::*;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
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
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle).unwrap_or_else(|| panic!("{name} in {tag}")) + needle.len();
    &tag[at..at + tag[at..].find('"').unwrap()]
}
