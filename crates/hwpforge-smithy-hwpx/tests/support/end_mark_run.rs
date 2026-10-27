#[path = "package.rs"]
mod package;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}

pub(super) fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    package::with_section(bytes, xml)
}
