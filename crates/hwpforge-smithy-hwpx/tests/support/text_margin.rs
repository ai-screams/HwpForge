#[path = "package.rs"]
mod package;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}

pub(super) fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    package::with_section(bytes, xml)
}

pub(super) const DEFAULT: &str =
    r#"<hp:textMargin left="283" right="283" top="283" bottom="283"/>"#;
pub(super) const ODD: &str = r#"<hp:textMargin left="850" right="700" top="566" bottom="400"/>"#;
