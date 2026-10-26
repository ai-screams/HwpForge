use super::*;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

/// The top-level `<hp:p>` elements of a section part, in order (nested
/// paragraphs stay inside their parent's slice).
pub(super) fn top_level_paragraphs(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
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
                out.push(&xml[start..i]);
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Top-level paragraph indices holding `needle`, in order.
pub(super) fn paragraphs_with(xml: &str, needle: &str) -> Vec<usize> {
    top_level_paragraphs(xml)
        .iter()
        .enumerate()
        .filter(|(_, p)| p.contains(needle))
        .map(|(i, _)| i)
        .collect()
}
