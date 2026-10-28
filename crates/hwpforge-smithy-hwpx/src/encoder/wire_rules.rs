//! Shared encoder output spellings and fixed control tags.
use hwpforge_foundation::DropCapStyle;

pub(crate) const DEFAULT_DROP_CAP: DropCapStyle = DropCapStyle::None;
// Big-endian ASCII control IDs: %clk, %smr, %pat, %xrf.
pub(crate) const CLICK_HERE_FIELD_ID: u32 = u32::from_be_bytes(*b"%clk");
pub(crate) const SUMMARY_FIELD_ID: u32 = u32::from_be_bytes(*b"%smr");
pub(crate) const PATH_FIELD_ID: u32 = u32::from_be_bytes(*b"%pat");
pub(crate) const CROSSREF_FIELD_ID: u32 = u32::from_be_bytes(*b"%xrf");

pub(crate) fn placeholder_run_key(char_pr_id_ref: u32, marker: &str) -> String {
    format!(r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#)
}

pub(crate) fn series_name_xml(name: &str) -> String {
    format!("<c:tx><c:v>{}</c:v></c:tx>", super::escape_xml(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 이것을 실패시키는 것: DEFAULT_DROP_CAP을 DoubleLine으로 변경.
    #[test]
    fn dropcap_spelling() {
        assert_eq!(DEFAULT_DROP_CAP.to_string(), "None");
    }

    // 이것을 실패시키는 것: 각 컨트롤 ID의 마지막 ASCII 바이트 변경.
    #[test]
    fn field_control_ids() {
        assert_eq!(CLICK_HERE_FIELD_ID, 627272811);
        assert_eq!(SUMMARY_FIELD_ID, 628321650);
        assert_eq!(PATH_FIELD_ID, 628121972);
        assert_eq!(CROSSREF_FIELD_ID, 628650598);
    }

    // 이것을 실패시키는 것: run 키의 hp:t를 hp:x로 변경.
    #[test]
    fn placeholder_key_format() {
        assert_eq!(
            placeholder_run_key(7, "MARKER"),
            r#"<hp:run charPrIDRef="7"><hp:t>MARKER</hp:t></hp:run>"#
        );
    }

    // 이것을 실패시키는 것: c:v를 c:f로 변경하거나 XML 이스케이프 제거.
    #[test]
    fn series_name_format() {
        assert_eq!(series_name_xml("A&B"), "<c:tx><c:v>A&amp;B</c:v></c:tx>");
    }
}
