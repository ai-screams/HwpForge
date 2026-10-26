use super::*;

pub(super) fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

pub(super) fn field_display(bytes: &[u8], section: usize, name: &str) -> String {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    decoded.document.sections()[section]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::Field { name: n, display_text, .. } if n.as_deref() == Some(name) => {
                    Some(display_text.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("field '{name}' not found in section {section}"))
}
