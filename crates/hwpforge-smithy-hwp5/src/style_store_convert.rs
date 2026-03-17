use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
};
use crate::style_store::Hwp5StyleStore;
use hwpforge_foundation::{Alignment, Color, FontIndex, HwpUnit};
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

pub(crate) fn hwp5_para_shape_to_hwpx(raw: &Hwp5RawParaShape) -> HwpxParaShape {
    let alignment = match (raw.property1 >> 2) & 0b111 {
        0 => Alignment::Justify,
        1 => Alignment::Left,
        2 => Alignment::Right,
        3 => Alignment::Center,
        _ => Alignment::Left,
    };

    let to_unit = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);

    let mut shape = HwpxParaShape::default();
    shape.alignment = alignment;
    shape.margin_left = to_unit(raw.left_margin);
    shape.margin_right = to_unit(raw.right_margin);
    shape.indent = to_unit(raw.indent);
    shape.spacing_before = to_unit(raw.space_before);
    shape.spacing_after = to_unit(raw.space_after);
    shape.line_spacing = raw.line_spacing;
    shape
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
