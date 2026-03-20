use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
    Hwp5RawTabDef,
};
use crate::style_store::Hwp5StyleStore;
use hwpforge_core::{TabDef, TabStop};
use hwpforge_foundation::{Alignment, Color, FontIndex, HeadingType, HwpUnit, TabAlign, TabLeader};
use hwpforge_smithy_hwpx::{
    HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape, HwpxStyle, HwpxStyleStore,
};

pub(crate) fn bgr_colorref_to_color(bgr: u32) -> Color {
    Color::from_raw(bgr & 0x00FF_FFFF)
}

pub(crate) fn push_fonts(store: &mut HwpxStyleStore, src: &Hwp5StyleStore) {
    const FONT_LANGS: [&str; 7] =
        ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];

    if src.fonts.is_empty() {
        for &lang in &FONT_LANGS {
            store.push_font(HwpxFont::new(0, "함초롬바탕", lang));
        }
        return;
    }

    if let Some(groups) = font_groups_from_id_mappings(src.id_mappings.as_ref(), &src.fonts) {
        for (lang, fonts) in FONT_LANGS.into_iter().zip(groups.into_iter()) {
            for (idx, face) in fonts.iter().enumerate() {
                store.push_font(HwpxFont::new(idx as u32, &face.face_name, lang));
            }
        }
        return;
    }

    for &lang in &FONT_LANGS {
        for (idx, face) in src.fonts.iter().enumerate() {
            store.push_font(HwpxFont::new(idx as u32, &face.face_name, lang));
        }
    }
}

pub(crate) fn resolved_font_group_counts(src: &Hwp5StyleStore) -> [usize; 7] {
    if src.fonts.is_empty() {
        return [1; 7];
    }

    if let Some(groups) = font_groups_from_id_mappings(src.id_mappings.as_ref(), &src.fonts) {
        return groups.map(|group| group.len());
    }

    [src.fonts.len(); 7]
}

pub(crate) fn hwp5_char_shape_to_hwpx(raw: &Hwp5RawCharShape) -> HwpxCharShape {
    let fi = |idx: usize| FontIndex::new(raw.font_ids[idx] as usize);
    let mut font_ref = HwpxFontRef::default();
    font_ref.hangul = fi(0);
    font_ref.latin = fi(1);
    font_ref.hanja = fi(2);
    font_ref.japanese = fi(3);
    font_ref.other = fi(4);
    font_ref.symbol = fi(5);
    font_ref.user = fi(6);

    let height = HwpUnit::new(raw.height).unwrap_or_else(|_| HwpUnit::new(1000).unwrap());
    let text_color = bgr_colorref_to_color(raw.text_color);
    let shade_color = if raw.shade_color == 0xFFFF_FFFF {
        None
    } else {
        Some(bgr_colorref_to_color(raw.shade_color))
    };

    let mut shape = HwpxCharShape::default();
    shape.font_ref = font_ref;
    shape.height = height;
    shape.text_color = text_color;
    shape.shade_color = shade_color;
    shape.bold = raw.is_bold();
    shape.italic = raw.is_italic();
    shape
}

pub(crate) fn hwp5_char_shape_to_hwpx_with_counts(
    raw: &Hwp5RawCharShape,
    font_group_counts: [usize; 7],
) -> HwpxCharShape {
    let mut shape = hwp5_char_shape_to_hwpx(raw);
    let clamp = |value: u16, count: usize| -> FontIndex {
        if count == 0 {
            return FontIndex::new(0);
        }
        FontIndex::new((value as usize).min(count.saturating_sub(1)))
    };

    shape.font_ref.hangul = clamp(raw.font_ids[0], font_group_counts[0]);
    shape.font_ref.latin = clamp(raw.font_ids[1], font_group_counts[1]);
    shape.font_ref.hanja = clamp(raw.font_ids[2], font_group_counts[2]);
    shape.font_ref.japanese = clamp(raw.font_ids[3], font_group_counts[3]);
    shape.font_ref.other = clamp(raw.font_ids[4], font_group_counts[4]);
    shape.font_ref.symbol = clamp(raw.font_ids[5], font_group_counts[5]);
    shape.font_ref.user = clamp(raw.font_ids[6], font_group_counts[6]);
    shape
}

#[cfg(test)]
pub(crate) fn hwp5_para_shape_to_hwpx(raw: &Hwp5RawParaShape) -> HwpxParaShape {
    hwp5_para_shape_to_hwpx_with_tab_id(raw, raw.tab_def_id as u32)
}

pub(crate) fn hwp5_para_shape_to_hwpx_with_tab_id(
    raw: &Hwp5RawParaShape,
    tab_pr_id_ref: u32,
) -> HwpxParaShape {
    let alignment = match (raw.property1 >> 2) & 0b111 {
        0 => Alignment::Justify,
        1 => Alignment::Left,
        2 => Alignment::Right,
        3 => Alignment::Center,
        _ => Alignment::Left,
    };

    let to_unit = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);
    let (heading_type, heading_id_ref, heading_level) = hwp5_heading_wire_parts(raw);

    let mut shape = HwpxParaShape::default();
    shape.alignment = alignment;
    shape.margin_left = to_unit(raw.left_margin);
    shape.margin_right = to_unit(raw.right_margin);
    shape.indent = to_unit(raw.indent);
    shape.spacing_before = to_unit(raw.space_before);
    shape.spacing_after = to_unit(raw.space_after);
    shape.line_spacing = raw.line_spacing;
    shape.heading_type = heading_type;
    shape.heading_level = heading_level;
    shape.heading_id_ref = heading_id_ref;
    shape.tab_pr_id_ref = tab_pr_id_ref;
    shape
}

fn hwp5_heading_wire_parts(raw: &Hwp5RawParaShape) -> (HeadingType, u32, u32) {
    let heading_type = raw.heading_kind();
    let heading_level = match heading_type {
        // HWPX stores outline heading levels as one-based values even though
        // HWP5 heading bits and the shared IR are normalized to zero-based.
        HeadingType::Outline => u32::from(raw.heading_level()) + 1,
        HeadingType::Number | HeadingType::Bullet | HeadingType::None => {
            u32::from(raw.heading_level())
        }
        _ => 0,
    };
    let heading_id_ref = match heading_type {
        HeadingType::Number | HeadingType::Bullet => raw.list_ref_id(),
        HeadingType::Outline | HeadingType::None => 0,
        _ => 0,
    };

    (heading_type, heading_id_ref, heading_level)
}

fn hwp5_tab_align_to_foundation(tab_type: u8) -> TabAlign {
    match tab_type {
        1 => TabAlign::Right,
        2 => TabAlign::Center,
        3 => TabAlign::Decimal,
        _ => TabAlign::Left,
    }
}

fn hwp5_fill_type_to_tab_leader(fill_type: u8) -> TabLeader {
    let leader = match fill_type {
        0 => "SOLID",
        1 => "DASH",
        2 => "DOT",
        3 => "DASH_DOT",
        4 => "DASH_DOT_DOT",
        5 => "LONG_DASH",
        6 => "CIRCLE",
        7 => "DOUBLE_SLIM",
        8 => "SLIM_THICK",
        9 => "THICK_SLIM",
        10 => "SLIM_THICK_SLIM",
        11 => "WAVE",
        12 => "DOUBLEWAVE",
        13 => "THICK_3D",
        14 => "THICK_3D_REVERSE_LIGHTING",
        15 => "SOLID_3D",
        16 => "SOLID_3D_REVERSE_LIGHTING",
        _ => "SOLID",
    };
    TabLeader::from_hwpx_str(leader)
}

pub(crate) fn hwp5_tab_def_to_hwpx(id: u32, raw: &Hwp5RawTabDef) -> TabDef {
    TabDef {
        id,
        auto_tab_left: raw.auto_tab_left(),
        auto_tab_right: raw.auto_tab_right(),
        stops: raw
            .tab_stops
            .iter()
            .map(|stop| TabStop {
                position: hwp5_tab_position_to_hwp_unit(stop.position),
                align: hwp5_tab_align_to_foundation(stop.tab_type),
                leader: hwp5_fill_type_to_tab_leader(stop.fill_type),
            })
            .collect(),
    }
}

fn hwp5_tab_position_to_hwp_unit(position: u32) -> HwpUnit {
    TabDef::clamp_position_from_unsigned(u64::from(position))
}

pub(crate) fn hwp5_style_to_hwpx(id: u32, raw: &Hwp5RawStyle, style_count: usize) -> HwpxStyle {
    HwpxStyle::new(
        id,
        if raw.kind == 1 { "CHAR" } else { "PARA" },
        raw.name.clone(),
        raw.english_name.clone(),
        raw.para_shape_id as u32,
        raw.char_shape_id as u32,
        if (raw.next_style_id as usize) < style_count { raw.next_style_id as u32 } else { 0 },
        if raw.lang_id < 0 { 1042 } else { raw.lang_id as u32 },
        raw.lock_form as u32,
    )
}

fn font_groups_from_id_mappings<'a>(
    id_mappings: Option<&Hwp5RawIdMappings>,
    fonts: &'a [Hwp5RawFaceName],
) -> Option<[&'a [Hwp5RawFaceName]; 7]> {
    let mappings = id_mappings?;
    let counts = [
        mappings.hangul_font_count,
        mappings.english_font_count,
        mappings.hanja_font_count,
        mappings.japanese_font_count,
        mappings.other_font_count,
        mappings.symbol_font_count,
        mappings.user_font_count,
    ]
    .map(|count| count.max(0) as usize);

    let total: usize = counts.iter().sum();
    if total != fonts.len() {
        return None;
    }

    let mut offset = 0usize;
    Some(counts.map(|count| {
        let slice = &fonts[offset..offset + count];
        offset += count;
        slice
    }))
}
