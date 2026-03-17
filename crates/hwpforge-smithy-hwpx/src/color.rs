use hwpforge_foundation::Color;

pub(crate) fn parse_hex_color_raw(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return None;
    };
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    Some(Color::from_rgb(r, g, b))
}

pub(crate) fn parse_hex_color_or_black(s: &str) -> Color {
    parse_hex_color_raw(s).unwrap_or(Color::BLACK)
}
