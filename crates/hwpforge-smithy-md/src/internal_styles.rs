use hwpforge_core::ParagraphListRef;

const LIST_CONTINUATION_STYLE_PREFIX: &str = "__hwpforge_md_list_continuation_level_";

pub(crate) fn list_continuation_style_name(level: u8) -> String {
    format!("{LIST_CONTINUATION_STYLE_PREFIX}{level}")
}

pub(crate) fn parse_list_continuation_style_name(name: &str) -> Option<u8> {
    let level = name.strip_prefix(LIST_CONTINUATION_STYLE_PREFIX)?.parse::<u8>().ok()?;
    (level <= ParagraphListRef::MAX_LEVEL).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_style_name_roundtrips() {
        let name = list_continuation_style_name(3);
        assert_eq!(parse_list_continuation_style_name(&name), Some(3));
    }

    #[test]
    fn invalid_continuation_style_name_returns_none() {
        assert_eq!(parse_list_continuation_style_name("body"), None);
    }
}
