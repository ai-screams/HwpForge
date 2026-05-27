use crate::decoder::Hwp5Warning;
use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
    Hwp5RawTabDef,
};
use crate::style_store::Hwp5StyleStore;
use crate::warning_utils::push_projection_fallback;
use hwpforge_core::{TabDef, TabStop};
use hwpforge_foundation::{
    BorderFillIndex, Color, EmbossType, EngraveType, FontIndex, HeadingType, HwpUnit,
    StrikeoutShape, TabAlign, TabLeader, UnderlineShape, UnderlineType, VerticalPosition,
};
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
    shape.underline_type = raw.underline_type();
    shape.underline_shape = match raw.underline_type() {
        UnderlineType::None => UnderlineShape::Solid,
        _ => match raw.underline_shape_raw() {
            0 => UnderlineShape::Solid,
            1 => UnderlineShape::Dash,
            2 => UnderlineShape::Dot,
            3 => UnderlineShape::DashDot,
            4 => UnderlineShape::DashDotDot,
            5 => UnderlineShape::LongDash,
            6 => UnderlineShape::Circle,
            7 => UnderlineShape::DoubleSlim,
            8 => UnderlineShape::SlimThick,
            9 => UnderlineShape::ThickSlim,
            10 => UnderlineShape::ThickSlimThick,
            11 => UnderlineShape::Wave,
            _ => UnderlineShape::Solid,
        },
    };
    shape.underline_color = match raw.underline_type() {
        UnderlineType::None => None,
        _ => optional_non_black_color(raw.underline_color),
    };
    shape.strikeout_shape = raw.strikeout_shape();
    shape.strikeout_color = match raw.strikeout_shape() {
        StrikeoutShape::None => None,
        _ => raw.strike_color.and_then(optional_non_black_color),
    };
    shape.vertical_position = if raw.superscript() {
        VerticalPosition::Superscript
    } else if raw.subscript() {
        VerticalPosition::Subscript
    } else {
        VerticalPosition::Normal
    };
    shape.outline_type = raw.outline_type();
    shape.shadow_type = raw.shadow_type();
    shape.emboss_type = if raw.emboss() { EmbossType::Emboss } else { EmbossType::None };
    shape.engrave_type = if raw.engrave() { EngraveType::Engrave } else { EngraveType::None };
    shape.emphasis = raw.emphasis();
    shape.ratio = raw.primary_ratio();
    shape.spacing = raw.primary_spacing();
    shape.rel_sz = raw.primary_rel_size();
    shape.char_offset = raw.primary_offset();
    shape.use_kerning = raw.use_kerning();
    shape.use_font_space = raw.use_font_space();
    shape.border_fill_id = raw.border_fill_id.filter(|id| *id != 0 && *id != 2).map(u32::from);
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

pub(crate) fn hwp5_char_shape_to_hwpx_with_counts_and_warnings(
    raw: &Hwp5RawCharShape,
    font_group_counts: [usize; 7],
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) -> HwpxCharShape {
    append_char_shape_projection_warnings(raw, raw_id, warnings);
    hwp5_char_shape_to_hwpx_with_counts(raw, font_group_counts)
}

#[cfg(test)]
pub(crate) fn hwp5_para_shape_to_hwpx(raw: &Hwp5RawParaShape) -> HwpxParaShape {
    hwp5_para_shape_to_hwpx_with_tab_id(raw, raw.tab_def_id as u32)
}

pub(crate) fn hwp5_para_shape_to_hwpx_with_tab_id(
    raw: &Hwp5RawParaShape,
    tab_pr_id_ref: u32,
) -> HwpxParaShape {
    let to_unit = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);
    let (heading_type, heading_id_ref, heading_level) = hwp5_heading_wire_parts(raw);

    let mut shape = HwpxParaShape::default();
    shape.alignment = raw.alignment();
    shape.margin_left = to_unit(raw.left_margin);
    shape.margin_right = to_unit(raw.right_margin);
    shape.indent = to_unit(raw.indent);
    shape.spacing_before = to_unit(raw.space_before);
    shape.spacing_after = to_unit(raw.space_after);
    shape.line_spacing = raw.line_spacing_value();
    shape.line_spacing_type = raw.line_spacing_type();
    shape.snap_to_grid = raw.snap_to_grid();
    shape.break_type = raw.break_type();
    shape.keep_with_next = raw.keep_with_next();
    shape.keep_lines_together = raw.keep_lines_together();
    shape.widow_orphan = raw.widow_orphan();
    shape.break_latin_word = raw.break_latin_word();
    shape.break_non_latin_word = raw.break_non_latin_word();
    shape.border_fill_id = match raw.border_fill_id {
        0 => None,
        id => Some(BorderFillIndex::new(id as usize)),
    };
    shape.heading_type = heading_type;
    shape.heading_level = heading_level;
    shape.heading_id_ref = heading_id_ref;
    shape.checked = raw.checked();
    shape.tab_pr_id_ref = tab_pr_id_ref;
    shape.condense = raw.condense();
    shape
}

pub(crate) fn hwp5_para_shape_to_hwpx_with_tab_id_and_warnings(
    raw: &Hwp5RawParaShape,
    tab_pr_id_ref: u32,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) -> HwpxParaShape {
    append_para_shape_projection_warnings(raw, raw_id, warnings);
    hwp5_para_shape_to_hwpx_with_tab_id(raw, tab_pr_id_ref)
}

fn optional_non_black_color(bgr: u32) -> Option<Color> {
    let color = bgr_colorref_to_color(bgr);
    if color == Color::BLACK {
        None
    } else {
        Some(color)
    }
}

fn append_char_shape_projection_warnings(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    warn_on_char_vertical_position_conflict(raw, raw_id, warnings);
    warn_on_char_outline_kind(raw, raw_id, warnings);
    warn_on_char_shadow_kind(raw, raw_id, warnings);
    warn_on_char_shadow_color(raw, raw_id, warnings);
    warn_on_char_shadow_offset(raw, raw_id, warnings);
    warn_on_char_script_scalar_collapse(raw, raw_id, warnings);
}

fn append_para_shape_projection_warnings(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    warn_on_para_line_spacing_kind(raw, raw_id, warnings);
    warn_on_para_break_latin_word(raw, raw_id, warnings);
    warn_on_para_vertical_align(raw, raw_id, warnings);
    warn_on_para_font_line_height(raw, raw_id, warnings);
    warn_on_para_auto_spacing(raw, raw_id, warnings);
    warn_on_para_border_offsets(raw, raw_id, warnings);
    warn_on_para_border_flags(raw, raw_id, warnings);
}

fn warn_on_char_vertical_position_conflict(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if !raw.superscript() || !raw.subscript() {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.vertical_position",
        format!(
            "char shape {raw_id} enabled both superscript and subscript; normalized to Superscript because the shared IR stores a single vertical_position"
        ),
    );
}

fn warn_on_char_outline_kind(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.outline_kind_raw() == 0 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.outline_kind",
        format!(
            "char shape {raw_id} outline kind {} collapsed to Solid because the shared IR only carries None vs Solid",
            raw.outline_kind_raw()
        ),
    );
}

fn warn_on_char_shadow_kind(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.shadow_kind_raw() == 0 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.shadow_kind",
        format!(
            "char shape {raw_id} shadow kind {} collapsed to Drop because the shared IR only carries None vs Drop",
            raw.shadow_kind_raw()
        ),
    );
}

fn warn_on_char_shadow_color(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.shadow_kind_raw() == 0 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.shadow_color",
        format!(
            "char shape {raw_id} shadow color 0x{:08X} is not carried because HwpxCharShape does not store shadow_color",
            raw.shadow_color
        ),
    );
}

fn warn_on_char_shadow_offset(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.shadow_kind_raw() == 0 {
        return;
    }
    if raw.shadow_gap_x == 0 && raw.shadow_gap_y == 0 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.shadow_offset",
        format!(
            "char shape {raw_id} shadow offset (dx={}, dy={}) is not carried because HwpxCharShape does not store shadow gaps",
            raw.shadow_gap_x, raw.shadow_gap_y
        ),
    );
}

fn warn_on_char_script_scalar_collapse(
    raw: &Hwp5RawCharShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let mut divergent_fields: Vec<&'static str> = Vec::new();
    if !raw.ratio_is_uniform() {
        divergent_fields.push("ratio");
    }
    if !raw.spacing_is_uniform() {
        divergent_fields.push("spacing");
    }
    if !raw.rel_size_is_uniform() {
        divergent_fields.push("rel_sz");
    }
    if !raw.offset_is_uniform() {
        divergent_fields.push("char_offset");
    }
    if divergent_fields.is_empty() {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.char_shape.script_scalars",
        format!(
            "char shape {raw_id} has script-specific {} values; collapsing to the hangul slot because the shared IR stores a single scalar per field",
            divergent_fields.join(", ")
        ),
    );
}

fn warn_on_para_line_spacing_kind(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.line_spacing_kind_raw() <= 3 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.para_shape.line_spacing",
        format!(
            "para shape {raw_id} line spacing kind {} collapsed to Percentage because the shared IR only supports Percentage, Fixed, and BetweenLines",
            raw.line_spacing_kind_raw()
        ),
    );
}

fn warn_on_para_break_latin_word(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    // Raw 0/1/2 are carried (KEEP_WORD/HYPHENATION/BREAK_WORD); raw 3 is
    // unspecified and collapses to KEEP_WORD by the projection's fallback arm.
    if raw.break_latin_word_raw() == 3 {
        push_projection_fallback(
            warnings,
            "style.para_shape.break_latin_word",
            format!("para shape {raw_id} latin break mode 3 is unknown and collapsed to KEEP_WORD"),
        );
    }
}

fn warn_on_para_vertical_align(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.vertical_align_raw() == 0 {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.para_shape.vertical_align",
        format!(
            "para shape {raw_id} vertical align mode {} was dropped because the current style surface only carries horizontal alignment",
            raw.vertical_align_raw()
        ),
    );
}

fn warn_on_para_font_line_height(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if !raw.font_line_height() {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.para_shape.font_line_height",
        format!(
            "para shape {raw_id} font-line-height flag was dropped because the current style surface does not carry that switch"
        ),
    );
}

fn warn_on_para_auto_spacing(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if !raw.auto_spacing_kr_eng() && !raw.auto_spacing_kr_num() {
        return;
    }
    let mut enabled: Vec<&str> = Vec::new();
    if raw.auto_spacing_kr_eng() {
        enabled.push("kr_eng");
    }
    if raw.auto_spacing_kr_num() {
        enabled.push("kr_num");
    }
    push_projection_fallback(
        warnings,
        "style.para_shape.auto_spacing",
        format!(
            "para shape {raw_id} auto-spacing [{}] was dropped because the current style surface does not carry those flags",
            enabled.join(", ")
        ),
    );
}

fn warn_on_para_border_offsets(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let border_offsets = [
        raw.border_offset_left,
        raw.border_offset_right,
        raw.border_offset_top,
        raw.border_offset_bottom,
    ];
    if !border_offsets.into_iter().any(|offset| offset != 0) {
        return;
    }
    push_projection_fallback(
        warnings,
        "style.para_shape.border_offsets",
        format!(
            "para shape {raw_id} border offsets [{}, {}, {}, {}] were dropped because the current style surface only carries borderFill references",
            raw.border_offset_left,
            raw.border_offset_right,
            raw.border_offset_top,
            raw.border_offset_bottom
        ),
    );
}

fn warn_on_para_border_flags(
    raw: &Hwp5RawParaShape,
    raw_id: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if raw.border_connect() {
        push_projection_fallback(
            warnings,
            "style.para_shape.border_connect",
            format!(
                "para shape {raw_id} border-connect flag was dropped because the current style surface does not carry paragraph border adjacency semantics"
            ),
        );
    }
    if raw.border_ignore_margin() {
        push_projection_fallback(
            warnings,
            "style.para_shape.border_ignore_margin",
            format!(
                "para shape {raw_id} border-ignore-margin flag was dropped because the current style surface does not carry that rendering mode"
            ),
        );
    }
}

fn hwp5_heading_wire_parts(raw: &Hwp5RawParaShape) -> (HeadingType, u32, u32) {
    let heading_type = raw.heading_kind();
    let heading_level = match heading_type {
        // HWP5 heading bits and HWPX `paraPr/heading(level)` are both
        // normalized to zero-based values for shared list semantics.
        HeadingType::Outline => u32::from(raw.heading_level()),
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

/// Maps a HWP5 TabDef `fill_type` to the HWPX `<hh:tabItem leader>` string.
///
/// **This is NOT the table 25 (border line types) mapping**, despite the
/// HWP 5.0 spec §4.2.7 referencing table 25 verbatim. Empirical evidence
/// from 한컴-authored truth fixtures (see
/// `examples/probe_tabdef.rs` against `sample-tab.hwpx`) and openhwp's
/// reference encoder (`crates/hwp/src/convert/to_ir.rs:521-527` joined
/// with `crates/hwpx/src/convert/from_ir.rs:2286-2290`) confirm a
/// dedicated tab-leader enum:
///
/// | HWP5 fill_type | HWPX leader  | openhwp IR  |
/// |---------------:|--------------|-------------|
/// | 0              | `NONE`       | None        |
/// | 1              | `DOT`        | Dot         |
/// | 2              | `DASH_DOT_DOT` | LongDash  |
/// | 3              | `DASH`       | Dash        |
/// | 4              | `SOLID`      | Underscore  |
/// | 5+             | `NONE`       | None (fallback) |
///
/// See `.docs/research/2026-05-26_tab_fidelity_bugs.md` (Bug B2) for
/// the investigation log.
fn hwp5_fill_type_to_tab_leader(fill_type: u8) -> TabLeader {
    let leader = match fill_type {
        0 => "NONE",
        1 => "DOT",
        2 => "DASH_DOT_DOT",
        3 => "DASH",
        4 => "SOLID",
        _ => "NONE",
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

/// Converts a HWP5 raw tab stop position (HwpUnit) into the value
/// expected by `crates/hwpforge-smithy-hwpx/src/encoder/header_tabs.rs`.
///
/// The HWPX encoder treats `TabStop.position` as **HwpUnitChar**: it emits
/// the value as-is into the `HwpUnitChar` switch branch and doubles it
/// into the legacy default branch. HWP5 stores the position in HwpUnit
/// (per HWP 5.0 spec §4.2.7), so we halve here to land in the encoder's
/// HwpUnitChar convention. Confirmed against the
/// `sample-tab.hwp{,x}` truth pair: HWP5 raw `30000` → HWPX
/// `pos="15000"` (HwpUnitChar) / `pos="30000"` (default).
///
/// See `.docs/research/2026-05-26_tab_fidelity_bugs.md` (Bug B1).
fn hwp5_tab_position_to_hwp_unit(position: u32) -> HwpUnit {
    TabDef::clamp_position_from_unsigned(u64::from(position) / 2)
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
