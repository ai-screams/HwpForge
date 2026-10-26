use super::*;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section").read_to_string(&mut out).unwrap();
    out
}

/// Top-level paragraph indices whose XML holds `needle`.
pub(super) fn paragraphs_with(xml: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let (mut depth, mut index, mut start, mut i) = (0usize, 0usize, 0usize, 0usize);
    while i < xml.len() {
        let rest = &xml[i..];
        if rest.starts_with("<hp:p ") || rest.starts_with("<hp:p>") {
            if depth == 0 {
                start = i;
            }
            depth += 1;
            i += 5;
        } else if rest.starts_with("</hp:p>") {
            depth -= 1;
            i += 7;
            if depth == 0 {
                if xml[start..i].contains(needle) {
                    found.push(index);
                }
                index += 1;
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    found
}
