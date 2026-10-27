#[path = "package.rs"]
mod package;

/// -2429, the most common wrapped `vertOffset` in the Hancom set.
pub(super) const V: &str = "4294964867";
/// -106, the most common wrapped `horzOffset`.
pub(super) const H: &str = "4294967190";

pub(super) fn section_xml(bytes: &[u8]) -> String {
    package::section_xml(bytes)
}

/// No offset attribute in `xml` is written as a negative literal.
pub(super) fn assert_no_signed_offset(xml: &str) {
    for bad in ["vertOffset=\"-", "horzOffset=\"-"] {
        assert!(!xml.contains(bad), "{bad} in {xml}");
    }
    for rest in xml.split("<hp:offset ").skip(1) {
        let attrs = &rest[..rest.find("/>").expect("self-closing")];
        assert!(!attrs.contains("=\"-"), "<hp:offset {attrs}/> in {xml}");
    }
}
