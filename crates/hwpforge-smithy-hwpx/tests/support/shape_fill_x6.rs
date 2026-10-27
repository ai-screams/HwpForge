#[path = "package.rs"]
mod package;

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}
