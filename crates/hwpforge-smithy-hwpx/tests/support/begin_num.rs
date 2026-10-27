#[path = "package.rs"]
mod package;

pub(super) fn part(bytes: &[u8], name: &str) -> String {
    package::part(bytes, name)
}

/// The attribute values of the first `<{tag} …>` in `xml`, in `names` order.
pub(super) fn attrs(xml: &str, tag: &str, names: &[&str]) -> Vec<String> {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    let element = &xml[at..at + xml[at..].find('>').expect("tag end")];
    names
        .iter()
        .map(|name| {
            let needle = format!(" {name}=\"");
            let start = element.find(&needle).unwrap_or_else(|| panic!("{name} in {element}"));
            let rest = &element[start + needle.len()..];
            rest[..rest.find('"').expect("quote")].to_string()
        })
        .collect()
}

pub(super) const HEADER: [&str; 6] = ["page", "footnote", "endnote", "pic", "tbl", "equation"];
pub(super) const START: [&str; 4] = ["page", "pic", "tbl", "equation"];

pub(super) fn header_numbers(bytes: &[u8]) -> Vec<String> {
    attrs(&part(bytes, "Contents/header.xml"), "hh:beginNum", &HEADER)
}

pub(super) fn start_numbers(bytes: &[u8]) -> Vec<String> {
    attrs(&part(bytes, "Contents/section0.xml"), "hp:startNum", &START)
}

pub(super) fn ones() -> Vec<String> {
    vec!["1".to_string(); 6]
}
