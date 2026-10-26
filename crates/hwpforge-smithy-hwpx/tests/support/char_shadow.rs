use super::*;

pub(super) fn header(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/header.xml").expect("header").read_to_string(&mut out).unwrap();
    out
}

/// Every `<hh:shadow …/>` element, in document order.
pub(super) fn shadows(header: &str) -> Vec<String> {
    header
        .match_indices("<hh:shadow ")
        .map(|(at, _)| header[at..at + header[at..].find("/>").expect("end") + 2].to_string())
        .collect()
}
