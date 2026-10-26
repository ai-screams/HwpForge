use super::*;

pub(super) fn zip_entry(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut s = String::new();
    z.by_name(name).expect("entry").read_to_string(&mut s).expect("read");
    s
}

/// 섹션 XML 의 첫 MEMO `<hp:subList>` 조각.
pub(super) fn memo_sublist(section_xml: &str) -> &str {
    let begin = section_xml.find(r#"type="MEMO""#).expect("MEMO fieldBegin");
    let rest = &section_xml[begin..];
    let start = rest.find("<hp:subList").expect("memo subList");
    let end = rest.find("</hp:subList>").expect("memo subList end");
    &rest[start..end]
}
