pub(super) fn format_list_item(
    text: &str,
    list_type: &str,
    level: u8,
    checked: Option<bool>,
) -> String {
    let indent = "  ".repeat(level as usize);
    if list_type == "NUMBER" {
        format!("{indent}1. {text}")
    } else if let Some(checked) = checked {
        let marker = if checked { "x" } else { " " };
        format!("{indent}- [{marker}] {text}")
    } else {
        format!("{indent}- {text}")
    }
}
