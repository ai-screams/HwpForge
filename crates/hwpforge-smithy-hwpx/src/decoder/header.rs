//! Parses `Contents/header.xml` into an [`HwpxStyleStore`].
//!
//! Converts XML schema types (`HxCharPr`, `HxParaPr`, `HxFont`) into
//! Foundation types (`Color`, `HwpUnit`, `Alignment`) for the store.

use hwpforge_foundation::{
    Color, EmbossType, EmphasisType, EngraveType, FontIndex, HeadingType, HwpUnit, LineSpacingType,
    OutlineType, ShadowType, StrikeoutShape, TabAlign, TabLeader, UnderlineType, VerticalPosition,
    WordBreakType,
};
use quick_xml::de::from_str;

use crate::error::{HwpxError, HwpxResult};
use crate::list_bridge::bullet_def_from_hwpx;
use crate::schema::header::{
    HxBorderFill, HxCharPr, HxHead, HxNumbering, HxParaPr, HxRefList, HxStyle, HxTabItem, HxTabPr,
};
use crate::style_store::{
    parse_alignment, parse_hex_color, HwpxBorderFill, HwpxBorderLine, HwpxCharShape,
    HwpxDiagonalLine, HwpxFill, HwpxFont, HwpxFontRef, HwpxGradientFill, HwpxImageFill,
    HwpxParaShape, HwpxStyle, HwpxStyleStore,
};
use hwpforge_core::section::BeginNum;

/// Result of parsing `header.xml`.
#[derive(Debug)]
pub struct HeaderParseResult {
    /// Style information parsed from `header.xml`.
    pub style_store: HwpxStyleStore,
    /// Starting auto-numbering values from `<hh:beginNum>`, if present.
    pub begin_num: Option<BeginNum>,
}

/// Parses a `header.xml` string into an [`HwpxStyleStore`] and optional [`BeginNum`].
///
/// Extracts:
/// - Font face definitions → `Vec<HwpxFont>`
/// - Character properties → `Vec<HwpxCharShape>`
/// - Paragraph properties → `Vec<HwpxParaShape>`
/// - Beginning auto-numbering values → `Option<BeginNum>`
///
/// # Security
///
/// XML entity expansion attacks (Billion Laughs) are not a concern here:
/// quick-xml's serde deserializer does not expand custom entities and will
/// return an error if any are encountered. The ZIP size limits in
/// `PackageReader` also bound the total input size.
pub fn parse_header(xml: &str) -> HwpxResult<HeaderParseResult> {
    let head: HxHead = from_str(xml)
        .map_err(|e| HwpxError::XmlParse { file: "header.xml".into(), detail: e.to_string() })?;
    let begin_num = parse_begin_num(&head);

    let mut store = HwpxStyleStore::new();
    if let Some(ref_list) = &head.ref_list {
        populate_store_from_ref_list(&mut store, ref_list);
    }

    Ok(HeaderParseResult { style_store: store, begin_num })
}

fn parse_begin_num(head: &HxHead) -> Option<BeginNum> {
    head.begin_num.map(|bn| BeginNum {
        page: bn.page,
        footnote: bn.footnote,
        endnote: bn.endnote,
        pic: bn.pic,
        tbl: bn.tbl,
        equation: bn.equation,
    })
}

fn populate_store_from_ref_list(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    load_fonts(store, ref_list);
    load_border_fills(store, ref_list);
    load_char_shapes(store, ref_list);
    load_para_shapes(store, ref_list);
    load_tab_properties(store, ref_list);
    load_numberings(store, ref_list);
    load_bullets(store, ref_list);
    load_styles(store, ref_list);
}

fn load_fonts(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    // NOTE: Fonts from all language groups are flattened into a single Vec.
    // This works because 한글 mirrors identical fonts across all groups,
    // making group-local indices equivalent to flat indices.
    // See the ASSUMPTION comment in convert_char_pr() for details.
    if let Some(fontfaces) = &ref_list.fontfaces {
        for group in &fontfaces.groups {
            for font in &group.fonts {
                store.push_font(HwpxFont {
                    id: font.id,
                    face_name: font.face.clone(),
                    lang: group.lang.clone(),
                });
            }
        }
    }
}

fn load_border_fills(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(border_fills) = &ref_list.border_fills {
        for bf in &border_fills.items {
            store.push_border_fill(convert_border_fill(bf));
        }
    }
}

fn load_char_shapes(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(char_props) = &ref_list.char_properties {
        for cp in &char_props.items {
            store.push_char_shape(convert_char_pr(cp));
        }
    }
}

fn load_para_shapes(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(para_props) = &ref_list.para_properties {
        for pp in &para_props.items {
            store.push_para_shape(convert_para_pr(pp));
        }
    }
}

fn load_tab_properties(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(tab_props) = &ref_list.tab_properties {
        for tp in &tab_props.items {
            store.push_tab(convert_tab(tp));
        }
    }
}

fn load_numberings(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(numberings) = &ref_list.numberings {
        for ndef in &numberings.items {
            store.push_numbering(convert_numbering(ndef));
        }
    }
}

fn load_bullets(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(bullets) = &ref_list.bullets {
        for bullet in &bullets.items {
            store.push_bullet(bullet_def_from_hwpx(bullet));
        }
    }
}

fn load_styles(store: &mut HwpxStyleStore, ref_list: &HxRefList) {
    if let Some(styles) = &ref_list.styles {
        for style in &styles.items {
            store.push_style(convert_style(style));
        }
    }
}

/// Converts an [`HxNumbering`] XML type into a [`hwpforge_core::NumberingDef`].
fn convert_numbering(hx: &HxNumbering) -> hwpforge_core::NumberingDef {
    let levels = hx
        .para_heads
        .iter()
        .map(|ph| hwpforge_core::ParaHead {
            start: ph.start,
            level: ph.level,
            num_format: parse_number_format(&ph.num_format),
            text: ph.text.clone(),
            checkable: ph.checkable != 0,
        })
        .collect();
    hwpforge_core::NumberingDef { id: hx.id, start: hx.start, levels }
}

/// Parses a HWPX number format string into a [`hwpforge_foundation::NumberFormatType`].
///
/// Shared by header (numbering definitions) and section (page number format).
pub(crate) fn parse_number_format(s: &str) -> hwpforge_foundation::NumberFormatType {
    use hwpforge_foundation::NumberFormatType;
    match s {
        "DIGIT" => NumberFormatType::Digit,
        "CIRCLED_DIGIT" => NumberFormatType::CircledDigit,
        "ROMAN_CAPITAL" => NumberFormatType::RomanCapital,
        "ROMAN_SMALL" => NumberFormatType::RomanSmall,
        "LATIN_CAPITAL" => NumberFormatType::LatinCapital,
        "LATIN_SMALL" => NumberFormatType::LatinSmall,
        "CIRCLED_LATIN_SMALL" => NumberFormatType::CircledLatinSmall,
        "HANGUL_SYLLABLE" => NumberFormatType::HangulSyllable,
        "HANGUL_JAMO" => NumberFormatType::HangulJamo,
        "HANJA_DIGIT" => NumberFormatType::HanjaDigit,
        "CIRCLED_HANGUL_SYLLABLE" => NumberFormatType::CircledHangulSyllable,
        _ => NumberFormatType::Digit,
    }
}

/// Converts an [`HxTabPr`] XML type into a [`hwpforge_core::TabDef`].
fn convert_tab(hx: &HxTabPr) -> hwpforge_core::TabDef {
    hwpforge_core::TabDef {
        id: hx.id,
        auto_tab_left: hx.auto_tab_left != 0,
        auto_tab_right: hx.auto_tab_right != 0,
        stops: collect_tab_items(hx),
    }
}

fn collect_tab_items(hx: &HxTabPr) -> Vec<hwpforge_core::TabStop> {
    if !hx.items.is_empty() {
        return convert_tab_items(&hx.items, false);
    }

    if let Some(items) = hx
        .switches
        .iter()
        .find_map(|switch| switch.case.as_ref().filter(|case| !case.items.is_empty()))
        .map(|case| &case.items)
    {
        return convert_tab_items(items, false);
    }

    if let Some(items) = hx
        .switches
        .iter()
        .find_map(|switch| switch.default.as_ref().filter(|default| !default.items.is_empty()))
        .map(|default| &default.items)
    {
        return convert_tab_items(items, true);
    }

    Vec::new()
}

fn convert_tab_items(
    items: &[HxTabItem],
    legacy_default_units: bool,
) -> Vec<hwpforge_core::TabStop> {
    items
        .iter()
        .map(|item| hwpforge_core::TabStop {
            position: normalize_tab_pos(item, legacy_default_units),
            align: TabAlign::from_hwpx_str(&item.tab_type),
            leader: TabLeader::from_hwpx_str(&item.leader),
        })
        .collect()
}

fn normalize_tab_pos(item: &HxTabItem, legacy_default_units: bool) -> HwpUnit {
    let raw = u64::from(item.pos);
    let normalized = if legacy_default_units && item.unit.is_empty() { raw / 2 } else { raw };
    hwpforge_core::TabDef::clamp_position_from_unsigned(normalized)
}

// ── HWPX string → enum parsing helpers ──────────────────────────
//
// HWPX XML uses uppercase string identifiers that differ from Foundation's
// Display/FromStr representations. These functions handle the HWPX-specific
// mapping at the decoder boundary.

/// Parses a HWPX underline type string to [`UnderlineType`].
fn parse_underline_type(s: &str) -> UnderlineType {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => UnderlineType::None,
        "BOTTOM" => UnderlineType::Bottom,
        "CENTER" => UnderlineType::Center,
        "TOP" => UnderlineType::Top,
        _ => UnderlineType::None,
    }
}

/// Parses a HWPX strikeout shape string to [`StrikeoutShape`].
///
/// Note: HWPX uses `"SLASH"` for [`StrikeoutShape::Continuous`].
fn parse_strikeout_shape(s: &str) -> StrikeoutShape {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => StrikeoutShape::None,
        "SLASH" => StrikeoutShape::Continuous,
        "DASH" => StrikeoutShape::Dash,
        "DOT" => StrikeoutShape::Dot,
        "DASH_DOT" => StrikeoutShape::DashDot,
        "DASH_DOT_DOT" => StrikeoutShape::DashDotDot,
        _ => StrikeoutShape::None,
    }
}

/// Parses a HWPX line spacing type string to [`LineSpacingType`].
///
/// Note: HWPX uses `"PERCENT"` for [`LineSpacingType::Percentage`].
fn parse_line_spacing_type(s: &str) -> LineSpacingType {
    match s.to_ascii_uppercase().as_str() {
        "PERCENT" => LineSpacingType::Percentage,
        "FIXED" => LineSpacingType::Fixed,
        "BETWEEN_LINES" => LineSpacingType::BetweenLines,
        _ => LineSpacingType::Percentage,
    }
}

/// Parses a HWPX outline type string to [`OutlineType`].
fn parse_outline_type(s: &str) -> OutlineType {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => OutlineType::None,
        "SOLID" => OutlineType::Solid,
        _ => OutlineType::None,
    }
}

/// Parses a HWPX shadow type string to [`ShadowType`].
fn parse_shadow_type(s: &str) -> ShadowType {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => ShadowType::None,
        "DROP" => ShadowType::Drop,
        _ => ShadowType::None,
    }
}

/// Parses a HWPX shade color string into `Option<Color>`.
///
/// Returns `None` for `"none"`, empty strings, or black (#000000).
fn parse_optional_hex_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    let color = parse_hex_color(s);
    if color == Color::BLACK {
        // Could be a real #000000 or a parse failure; treat as None for shade
        // since black shading is not meaningful.
        None
    } else {
        Some(color)
    }
}

/// Converts an `HxCharPr` XML type into an `HwpxCharShape`.
fn convert_char_pr(cp: &HxCharPr) -> HwpxCharShape {
    // ASSUMPTION: 한글 (Hangul Word Processor) always mirrors identical fonts
    // across all 7 language groups (HANGUL, LATIN, HANJA, JAPANESE, OTHER,
    // SYMBOL, USER). Therefore, fontRef group-local indices coincide with
    // flat store indices. If a future HWPX producer uses different fonts per
    // group, this mapping breaks.
    // See: Phase 4 analysis H3, OWPML KS X 6101 §fontRef.
    // TODO(v2.0): Refactor to per-group font model for full spec compliance.
    let font_ref = cp
        .font_ref
        .as_ref()
        .map(|fr| HwpxFontRef {
            hangul: FontIndex::new(fr.hangul as usize),
            latin: FontIndex::new(fr.latin as usize),
            hanja: FontIndex::new(fr.hanja as usize),
            japanese: FontIndex::new(fr.japanese as usize),
            other: FontIndex::new(fr.other as usize),
            symbol: FontIndex::new(fr.symbol as usize),
            user: FontIndex::new(fr.user as usize),
        })
        .unwrap_or_default();

    // height is in HWPUNIT already; clamp u32 → i32 safely
    let height =
        i32::try_from(cp.height).ok().and_then(|h| HwpUnit::new(h).ok()).unwrap_or(HwpUnit::ZERO);

    HwpxCharShape {
        font_ref,
        height,
        text_color: parse_hex_color(&cp.text_color),
        shade_color: parse_optional_hex_color(&cp.shade_color),
        bold: cp.bold.is_some(),
        italic: cp.italic.is_some(),
        underline_type: cp
            .underline
            .as_ref()
            .map(|u| parse_underline_type(&u.underline_type))
            .unwrap_or(UnderlineType::None),
        underline_color: cp.underline.as_ref().and_then(|u| {
            let c = parse_hex_color(&u.color);
            if c == Color::BLACK {
                None
            } else {
                Some(c)
            }
        }),
        strikeout_shape: cp
            .strikeout
            .as_ref()
            .map(|s| parse_strikeout_shape(&s.shape))
            .unwrap_or(StrikeoutShape::None),
        strikeout_color: cp.strikeout.as_ref().and_then(|s| {
            let c = parse_hex_color(&s.color);
            if c == Color::BLACK {
                None
            } else {
                Some(c)
            }
        }),
        vertical_position: VerticalPosition::Normal, // TODO(v2.0): Parse from XML
        outline_type: cp
            .outline
            .as_ref()
            .map(|o| parse_outline_type(&o.outline_type))
            .unwrap_or(OutlineType::None),
        shadow_type: cp
            .shadow
            .as_ref()
            .map(|s| parse_shadow_type(&s.shadow_type))
            .unwrap_or(ShadowType::None),
        emboss_type: EmbossType::None,   // TODO(v2.0): Parse from XML
        engrave_type: EngraveType::None, // TODO(v2.0): Parse from XML
        emphasis: parse_emphasis_type(&cp.sym_mark),
        ratio: cp.ratio.as_ref().map_or(100, |r| r.hangul),
        spacing: cp.spacing.as_ref().map_or(0, |s| s.hangul),
        rel_sz: cp.rel_sz.as_ref().map_or(100, |r| r.hangul),
        char_offset: cp.offset.as_ref().map_or(0, |o| o.hangul),
        use_kerning: cp.use_kerning != 0,
        use_font_space: cp.use_font_space != 0,
        border_fill_id: if cp.border_fill_id_ref == 2 { None } else { Some(cp.border_fill_id_ref) },
    }
}

/// Converts an HWPX `symMark` attribute string to an [`EmphasisType`].
fn parse_emphasis_type(s: &str) -> EmphasisType {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => EmphasisType::None,
        "DOT_ABOVE" => EmphasisType::DotAbove,
        "RING_ABOVE" => EmphasisType::RingAbove,
        "TILDE" => EmphasisType::Tilde,
        "CARON" => EmphasisType::Caron,
        "SIDE" => EmphasisType::Side,
        "COLON" => EmphasisType::Colon,
        "GRAVE_ACCENT" => EmphasisType::GraveAccent,
        "ACUTE_ACCENT" => EmphasisType::AcuteAccent,
        "CIRCUMFLEX" => EmphasisType::Circumflex,
        "MACRON" => EmphasisType::Macron,
        "HOOK_ABOVE" => EmphasisType::HookAbove,
        "DOT_BELOW" => EmphasisType::DotBelow,
        _ => EmphasisType::None,
    }
}

/// Converts an `HxParaPr` XML type into an `HwpxParaShape`.
fn convert_para_pr(pp: &HxParaPr) -> HwpxParaShape {
    let alignment = pp
        .align
        .as_ref()
        .map(|a| parse_alignment(&a.horizontal))
        .unwrap_or(hwpforge_foundation::Alignment::Left);

    // Margin and line spacing come from hp:switch/hp:default
    let (margin_left, margin_right, indent, spacing_before, spacing_after) = extract_margins(pp);

    let (line_spacing, line_spacing_type) = extract_line_spacing(pp);

    let (break_latin_word, break_non_latin_word) = pp
        .break_setting
        .as_ref()
        .map(|bs| {
            (
                parse_word_break_type(&bs.break_latin_word),
                parse_word_break_type(&bs.break_non_latin_word),
            )
        })
        .unwrap_or_default();

    let heading_type = pp
        .heading
        .as_ref()
        .map_or(HeadingType::None, |heading| HeadingType::from_hwpx_str(&heading.heading_type));
    let heading_id_ref = pp.heading.as_ref().map_or(0, |heading| heading.id_ref);
    let heading_level = pp.heading.as_ref().map_or(0, |heading| heading.level);
    let checked = pp.checked != 0;
    let tab_pr_id_ref = pp.tab_pr_id_ref;
    let condense = pp.condense;

    HwpxParaShape {
        alignment,
        margin_left,
        margin_right,
        indent,
        spacing_before,
        spacing_after,
        line_spacing,
        line_spacing_type,
        break_latin_word,
        break_non_latin_word,
        heading_type,
        heading_id_ref,
        heading_level,
        checked,
        tab_pr_id_ref,
        condense,
        ..Default::default()
    }
}

/// Parses a `breakLatinWord` / `breakNonLatinWord` attribute string into a [`WordBreakType`].
fn parse_word_break_type(s: &str) -> WordBreakType {
    match s {
        "BREAK_WORD" => WordBreakType::BreakWord,
        _ => WordBreakType::KeepWord,
    }
}

/// Converts an `HxStyle` XML type into an `HwpxStyle`.
fn convert_style(s: &HxStyle) -> HwpxStyle {
    HwpxStyle {
        id: s.id,
        style_type: s.style_type.clone(),
        name: s.name.clone(),
        eng_name: s.eng_name.clone(),
        para_pr_id_ref: s.para_pr_id_ref,
        char_pr_id_ref: s.char_pr_id_ref,
        next_style_id_ref: s.next_style_id_ref,
        lang_id: s.lang_id,
        lock_form: s.lock_form,
    }
}

/// Extracts margin values from the switch/default block.
///
/// Searches all `<hp:switch>` elements for one whose `<hp:default>` contains
/// a `<hh:margin>` child (some `paraPr` have multiple switches).
fn extract_margins(pp: &HxParaPr) -> (HwpUnit, HwpUnit, HwpUnit, HwpUnit, HwpUnit) {
    let z = HwpUnit::ZERO;
    let margin = pp.switches.iter().find_map(|sw| sw.default.as_ref()?.margin.as_ref());
    let Some(margin) = margin else {
        return (z, z, z, z, z);
    };

    let to_unit = |opt: &Option<crate::schema::header::HxUnitValue>| -> HwpUnit {
        opt.as_ref().and_then(|v| HwpUnit::new(v.value).ok()).unwrap_or(z)
    };

    (
        to_unit(&margin.left),
        to_unit(&margin.right),
        to_unit(&margin.indent),
        to_unit(&margin.prev),
        to_unit(&margin.next),
    )
}

/// Extracts line spacing from the switch/default block.
///
/// Searches all `<hp:switch>` elements for one whose `<hp:default>` contains
/// a `<hh:lineSpacing>` child (some `paraPr` have multiple switches).
fn extract_line_spacing(pp: &HxParaPr) -> (i32, hwpforge_foundation::LineSpacingType) {
    use hwpforge_foundation::LineSpacingType;
    let default_ls = (160, LineSpacingType::Percentage);
    let ls = pp.switches.iter().find_map(|sw| sw.default.as_ref()?.line_spacing.as_ref());
    let Some(ls) = ls else {
        return default_ls;
    };

    let spacing_type = if ls.spacing_type.is_empty() {
        LineSpacingType::Percentage
    } else {
        parse_line_spacing_type(&ls.spacing_type)
    };

    // Saturate to i32::MAX if value exceeds range (extremely rare in real HWPX files)
    let value = ls.value.min(i32::MAX as u32) as i32;
    (value, spacing_type)
}

/// Converts an `HxBorderFill` XML type into an `HwpxBorderFill`.
fn convert_border_fill(hx: &HxBorderFill) -> HwpxBorderFill {
    let fill_projection = hx.fill_brush.as_ref().map(convert_fill_brush).unwrap_or_default();
    let slash = convert_diagonal_border(&hx.slash);
    let back_slash = convert_diagonal_border(&hx.back_slash);
    let mut border_fill = HwpxBorderFill::new(
        hx.id,
        hx.three_d != 0,
        hx.shadow != 0,
        hx.center_line.clone(),
        convert_border_line(&hx.left_border),
        convert_border_line(&hx.right_border),
        convert_border_line(&hx.top_border),
        convert_border_line(&hx.bottom_border),
        hx.diagonal.as_ref().map(convert_border_line),
        slash,
        back_slash,
        None,
    );
    apply_fill_projection(&mut border_fill, fill_projection);
    border_fill
}

#[derive(Default)]
struct BorderFillBrushProjection {
    fill: Option<HwpxFill>,
    fill_hatch_style: Option<String>,
    gradient_fill: Option<HwpxGradientFill>,
    image_fill: Option<HwpxImageFill>,
}

fn apply_fill_projection(border_fill: &mut HwpxBorderFill, projection: BorderFillBrushProjection) {
    match projection {
        BorderFillBrushProjection {
            fill: Some(HwpxFill::WinBrush { face_color, hatch_color, alpha }),
            fill_hatch_style,
            ..
        } => border_fill.set_win_brush_fill(face_color, hatch_color, alpha, fill_hatch_style),
        BorderFillBrushProjection { gradient_fill: Some(fill), .. } => {
            border_fill.set_gradient_fill(fill)
        }
        BorderFillBrushProjection { image_fill: Some(fill), .. } => {
            border_fill.set_image_fill(fill)
        }
        BorderFillBrushProjection { .. } => border_fill.clear_fill_brush(),
    }
}

fn convert_fill_brush(
    fill_brush: &crate::schema::header::HxFillBrush,
) -> BorderFillBrushProjection {
    if let Some(win_brush) = &fill_brush.win_brush {
        return BorderFillBrushProjection {
            fill: Some(HwpxFill::WinBrush {
                face_color: win_brush.face_color.clone(),
                hatch_color: win_brush.hatch_color.clone(),
                alpha: win_brush.alpha.clone(),
            }),
            fill_hatch_style: win_brush.hatch_style.clone(),
            ..BorderFillBrushProjection::default()
        };
    }
    if let Some(gradation) = &fill_brush.gradation {
        return BorderFillBrushProjection {
            gradient_fill: Some(HwpxGradientFill {
                gradient_type: gradation
                    .gradation_type
                    .parse()
                    .unwrap_or(hwpforge_foundation::GradientType::Linear),
                angle: gradation.angle,
                center_x: gradation.center_x,
                center_y: gradation.center_y,
                step: gradation.step,
                step_center: gradation.step_center,
                alpha: gradation.alpha,
                colors: gradation
                    .colors
                    .iter()
                    .map(|color| parse_hex_color(&color.value))
                    .collect(),
            }),
            ..BorderFillBrushProjection::default()
        };
    }
    if let Some(img_brush) = &fill_brush.img_brush {
        let Some(img) = &img_brush.img else {
            return BorderFillBrushProjection::default();
        };
        return BorderFillBrushProjection {
            image_fill: Some(HwpxImageFill {
                mode: img_brush.mode.clone(),
                binary_item_id_ref: img.binary_item_id_ref.clone(),
                bright: img.bright,
                contrast: img.contrast,
                effect: img.effect.clone(),
                alpha: img.alpha,
            }),
            ..BorderFillBrushProjection::default()
        };
    }
    BorderFillBrushProjection::default()
}

/// Converts an `HxBorderLine` XML type into an `HwpxBorderLine`.
fn convert_border_line(hx: &crate::schema::header::HxBorderLine) -> HwpxBorderLine {
    HwpxBorderLine {
        line_type: hx.border_type.clone(),
        width: hx.width.clone(),
        color: hx.color.clone(),
    }
}

fn convert_diagonal_border(hx: &crate::schema::header::HxDiagonalBorder) -> HwpxDiagonalLine {
    HwpxDiagonalLine {
        border_type: hx.border_type.clone(),
        crooked: parse_hwpx_boolish(&hx.crooked),
        is_counter: parse_hwpx_boolish(&hx.is_counter),
    }
}

fn parse_hwpx_boolish(value: &str) -> bool {
    matches!(value.trim(), "1") || value.trim().eq_ignore_ascii_case("true")
}

// (parse_optional_hex_color is defined above with the HWPX parsing helpers)

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{Alignment, Color, LineSpacingType, StrikeoutShape, UnderlineType};

    // ── Minimal header ───────────────────────────────────────────

    #[test]
    fn parse_empty_header() {
        let xml = r#"<head version="1.4" secCnt="1"></head>"#;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.font_count(), 0);
        assert_eq!(store.char_shape_count(), 0);
        assert_eq!(store.para_shape_count(), 0);
    }

    #[test]
    fn parse_header_with_fonts() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <fontfaces itemCnt="2">
                    <fontface lang="HANGUL" fontCnt="1">
                        <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                    </fontface>
                    <fontface lang="LATIN" fontCnt="1">
                        <font id="0" face="Times New Roman" type="TTF" isEmbedded="0"/>
                    </fontface>
                </fontfaces>
            </refList>
        </head>"##;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.font_count(), 2);

        let f0 = store.font(FontIndex::new(0)).unwrap();
        assert_eq!(f0.face_name, "함초롬돋움");
        assert_eq!(f0.lang, "HANGUL");

        let f1 = store.font(FontIndex::new(1)).unwrap();
        assert_eq!(f1.face_name, "Times New Roman");
        assert_eq!(f1.lang, "LATIN");
    }

    #[test]
    fn parse_header_tab_properties_with_explicit_switch_items() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <tabProperties itemCnt="4">
                    <tabPr id="0" autoTabLeft="0" autoTabRight="0"/>
                    <tabPr id="1" autoTabLeft="1" autoTabRight="0"/>
                    <tabPr id="2" autoTabLeft="0" autoTabRight="1"/>
                    <tabPr id="3" autoTabLeft="0" autoTabRight="0">
                        <switch>
                            <case required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">
                                <tabItem pos="15000" type="LEFT" leader="DASH" unit="HWPUNIT"/>
                            </case>
                            <default>
                                <tabItem pos="30000" type="LEFT" leader="DASH"/>
                            </default>
                        </switch>
                    </tabPr>
                </tabProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let tabs: Vec<_> = store.iter_tabs().cloned().collect();
        assert_eq!(tabs.len(), 4);
        assert_eq!(tabs[3].id, 3);
        assert_eq!(tabs[3].stops.len(), 1);
        assert_eq!(tabs[3].stops[0].position, HwpUnit::new(15000).unwrap());
        assert_eq!(tabs[3].stops[0].align, TabAlign::Left);
        assert_eq!(tabs[3].stops[0].leader.as_hwpx_str(), "DASH");
    }

    #[test]
    fn parse_header_tab_properties_clamps_oversized_positions_without_wrap() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <tabProperties itemCnt="4">
                    <tabPr id="0" autoTabLeft="0" autoTabRight="0"/>
                    <tabPr id="1" autoTabLeft="1" autoTabRight="0"/>
                    <tabPr id="2" autoTabLeft="0" autoTabRight="1"/>
                    <tabPr id="3" autoTabLeft="0" autoTabRight="0">
                        <switch>
                            <case required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">
                                <tabItem pos="4000000000" type="LEFT" leader="DASH" unit="HWPUNIT"/>
                            </case>
                            <default>
                                <tabItem pos="4000000000" type="LEFT" leader="DASH"/>
                            </default>
                        </switch>
                    </tabPr>
                </tabProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let tabs: Vec<_> = store.iter_tabs().cloned().collect();
        assert_eq!(tabs[3].stops.len(), 1);
        assert_eq!(tabs[3].stops[0].position, HwpUnit::new(HwpUnit::MAX_VALUE).unwrap());
    }

    #[test]
    fn parse_header_border_fill_preserves_hatch_style() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <borderFills itemCnt="1">
                    <borderFill id="4" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">
                        <slash type="NONE" Crooked="0" isCounter="0"/>
                        <backSlash type="NONE" Crooked="0" isCounter="0"/>
                        <leftBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <rightBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <topBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <bottomBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <diagonal type="NONE" width="0.1 mm" color="#000000"/>
                        <fillBrush>
                            <winBrush faceColor="#FFD700" hatchColor="#000000" hatchStyle="HORIZONTAL" alpha="0"/>
                        </fillBrush>
                    </borderFill>
                </borderFills>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let border_fill = store.border_fill(4).unwrap();
        assert!(matches!(border_fill.fill, Some(HwpxFill::WinBrush { .. })));
        assert_eq!(border_fill.fill_hatch_style.as_deref(), Some("HORIZONTAL"));
    }

    #[test]
    fn parse_header_border_fill_preserves_gradient_fill() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <borderFills itemCnt="1">
                    <borderFill id="4" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">
                        <slash type="NONE" Crooked="0" isCounter="0"/>
                        <backSlash type="NONE" Crooked="0" isCounter="0"/>
                        <leftBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <rightBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <topBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <bottomBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <diagonal type="NONE" width="0.1 mm" color="#000000"/>
                        <fillBrush>
                            <gradation type="LINEAR" angle="90" centerX="0" centerY="0" step="255" colorNum="2" stepCenter="50" alpha="0">
                                <color value="#FF0000"/>
                                <color value="#00FF00"/>
                            </gradation>
                        </fillBrush>
                    </borderFill>
                </borderFills>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let border_fill = store.border_fill(4).unwrap();
        assert!(matches!(
            border_fill.gradient_fill,
            Some(ref fill)
                if fill.gradient_type == hwpforge_foundation::GradientType::Linear
                    && fill.angle == 90
                    && fill.step == 255
                    && fill.step_center == 50
                    && fill.alpha == 0
                    && fill.colors == vec![Color::from_rgb(255, 0, 0), Color::from_rgb(0, 255, 0)]
        ));
        assert!(border_fill.fill.is_none());
    }

    #[test]
    fn parse_header_border_fill_preserves_image_fill() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <borderFills itemCnt="1">
                    <borderFill id="4" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">
                        <slash type="NONE" Crooked="0" isCounter="0"/>
                        <backSlash type="NONE" Crooked="0" isCounter="0"/>
                        <leftBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <rightBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <topBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <bottomBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <diagonal type="NONE" width="0.1 mm" color="#000000"/>
                        <fillBrush>
                            <imgBrush mode="TOTAL">
                                <img binaryItemIDRef="BIN0001" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                            </imgBrush>
                        </fillBrush>
                    </borderFill>
                </borderFills>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let border_fill = store.border_fill(4).unwrap();
        assert!(matches!(
            border_fill.image_fill,
            Some(ref fill)
                if fill.mode == "TOTAL"
                    && fill.binary_item_id_ref == "BIN0001"
                    && fill.bright == 0
                    && fill.contrast == 0
                    && fill.effect == "REAL_PIC"
                    && fill.alpha == 0
        ));
        assert!(border_fill.fill.is_none());
    }

    #[test]
    fn parse_header_border_fill_preserves_diagonal_flags() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <borderFills itemCnt="1">
                    <borderFill id="4" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">
                        <slash type="CENTER_BELOW" Crooked="1" isCounter="0"/>
                        <backSlash type="ALL" Crooked="0" isCounter="1"/>
                        <leftBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <rightBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <topBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <bottomBorder type="NONE" width="0.1 mm" color="#000000"/>
                        <diagonal type="NONE" width="0.1 mm" color="#000000"/>
                    </borderFill>
                </borderFills>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let border_fill = store.border_fill(4).unwrap();
        assert_eq!(border_fill.slash.border_type, "CENTER_BELOW");
        assert!(border_fill.slash.crooked);
        assert!(!border_fill.slash.is_counter);
        assert_eq!(border_fill.back_slash.border_type, "ALL");
        assert!(!border_fill.back_slash.crooked);
        assert!(border_fill.back_slash.is_counter);
    }

    // ── Character properties ─────────────────────────────────────

    #[test]
    fn parse_char_pr_basic() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <charProperties itemCnt="1">
                    <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                        <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                        <underline type="NONE" shape="SOLID" color="#000000"/>
                        <strikeout shape="NONE" color="#000000"/>
                        <outline type="NONE"/>
                        <shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>
                    </charPr>
                </charProperties>
            </refList>
        </head>"##;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.char_shape_count(), 1);

        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert_eq!(cs.text_color, Color::BLACK);
        assert!(!cs.bold);
        assert!(!cs.italic);
        assert_eq!(cs.underline_type, UnderlineType::None);
        assert_eq!(cs.strikeout_shape, StrikeoutShape::None);
    }

    #[test]
    fn parse_char_pr_with_bold_italic_and_color() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <charProperties itemCnt="1">
                    <charPr id="7" height="2500" textColor="#FF0000" shadeColor="#00FF00"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                        <fontRef hangul="1" latin="2" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                        <bold/>
                        <italic/>
                        <underline type="BOTTOM" shape="SOLID" color="#000000"/>
                        <strikeout shape="SLASH" color="#000000"/>
                    </charPr>
                </charProperties>
            </refList>
        </head>"##;
        let store = parse_header(xml).unwrap().style_store;
        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 2500);
        assert_eq!(cs.text_color, Color::from_rgb(255, 0, 0));
        assert_eq!(cs.shade_color, Some(Color::from_rgb(0, 255, 0)));
        assert!(cs.bold);
        assert!(cs.italic);
        assert_eq!(cs.font_ref.hangul.get(), 1);
        assert_eq!(cs.font_ref.latin.get(), 2);
        assert_eq!(cs.underline_type, UnderlineType::Bottom);
        assert_eq!(cs.strikeout_shape, StrikeoutShape::Continuous);
    }

    // ── Paragraph properties ─────────────────────────────────────

    #[test]
    fn parse_para_pr_with_alignment() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="CENTER" vertical="BASELINE"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.para_shape_count(), 1);

        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Center);
    }

    #[test]
    fn parse_para_pr_with_switch_margin() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="JUSTIFY" vertical="BASELINE"/>
                        <switch>
                            <default>
                                <margin>
                                    <intent value="200"/>
                                    <left value="100"/>
                                    <right value="50"/>
                                    <prev value="300"/>
                                    <next value="150"/>
                                </margin>
                                <lineSpacing type="PERCENT" value="200"/>
                            </default>
                        </switch>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.indent.as_i32(), 200);
        assert_eq!(ps.margin_left.as_i32(), 100);
        assert_eq!(ps.margin_right.as_i32(), 50);
        assert_eq!(ps.spacing_before.as_i32(), 300);
        assert_eq!(ps.spacing_after.as_i32(), 150);
        assert_eq!(ps.line_spacing, 200);
        assert_eq!(ps.line_spacing_type, LineSpacingType::Percentage);
    }

    #[test]
    fn parse_para_pr_without_switch_uses_defaults() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="LEFT" vertical="BASELINE"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.margin_left, HwpUnit::ZERO);
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, LineSpacingType::Percentage);
    }

    // ── Full header ──────────────────────────────────────────────

    #[test]
    fn parse_full_header_fonts_and_shapes() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <fontfaces itemCnt="1">
                    <fontface lang="HANGUL" fontCnt="2">
                        <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                        <font id="1" face="함초롬바탕" type="TTF" isEmbedded="0"/>
                    </fontface>
                </fontfaces>
                <charProperties itemCnt="2">
                    <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                        <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                    </charPr>
                    <charPr id="1" height="1400" textColor="#0000FF" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                        <fontRef hangul="1" latin="1" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                        <bold/>
                    </charPr>
                </charProperties>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="LEFT" vertical="BASELINE"/>
                        <switch>
                            <default>
                                <margin>
                                    <left value="0"/>
                                    <right value="0"/>
                                </margin>
                                <lineSpacing type="PERCENT" value="160"/>
                            </default>
                        </switch>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.font_count(), 2);
        assert_eq!(store.char_shape_count(), 2);
        assert_eq!(store.para_shape_count(), 1);

        // Font check
        assert_eq!(store.font(FontIndex::new(0)).unwrap().face_name, "함초롬돋움");
        assert_eq!(store.font(FontIndex::new(1)).unwrap().face_name, "함초롬바탕");

        // CharShape 1 is bold with blue color
        let cs1 = store.char_shape(hwpforge_foundation::CharShapeIndex::new(1)).unwrap();
        assert!(cs1.bold);
        assert_eq!(cs1.text_color, Color::from_rgb(0, 0, 255));
        assert_eq!(cs1.font_ref.hangul.get(), 1);
    }

    // ── Error cases ──────────────────────────────────────────────

    #[test]
    fn parse_invalid_xml() {
        let err = parse_header("<not-closed").unwrap_err();
        assert!(matches!(err, HwpxError::XmlParse { .. }));
    }

    #[test]
    fn parse_header_with_no_reflist() {
        let xml = r#"<head version="1.4" secCnt="1"></head>"#;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.font_count(), 0);
    }

    #[test]
    fn char_pr_without_font_ref_gets_default() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <charProperties itemCnt="1">
                    <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                    </charPr>
                </charProperties>
            </refList>
        </head>"##;
        let store = parse_header(xml).unwrap().style_store;
        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.font_ref.hangul.get(), 0);
        assert_eq!(cs.font_ref.latin.get(), 0);
    }

    #[test]
    fn parse_header_loads_bullets() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <numberings itemCnt="1">
                    <numbering id="1" start="0">
                        <paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1"
                            widthAdjust="0" textOffsetType="PERCENT" textOffset="50"
                            numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</paraHead>
                    </numbering>
                </numberings>
                <bullets itemCnt="1">
                    <bullet id="1" char="" useImage="1">
                        <paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1"
                            widthAdjust="0" textOffsetType="PERCENT" textOffset="50"
                            numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/>
                    </bullet>
                </bullets>
                <paraProperties itemCnt="1">
                    <paraPr id="0" tabPrIDRef="0" condense="0" fontLineHeight="0" snapToGrid="1"
                        suppressLineNumbers="0" checked="0">
                        <align horizontal="JUSTIFY" vertical="BASELINE"/>
                        <heading type="BULLET" idRef="1" level="0"/>
                        <breakSetting breakLatinWord="KEEP_WORD" breakNonLatinWord="KEEP_WORD"
                            widowOrphan="0" keepWithNext="0" keepLines="0" pageBreakBefore="0"
                            lineWrap="BREAK"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.numbering_count(), 1);
        assert_eq!(store.bullet_count(), 1);

        let bullet = store.iter_bullets().next().unwrap();
        assert_eq!(bullet.id, 1);
        assert_eq!(bullet.bullet_char, "");
        assert!(bullet.use_image);
        assert_eq!(bullet.para_head.level, 1);
    }

    // ── Styles ───────────────────────────────────────────────────

    #[test]
    fn parse_styles_basic() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <styles itemCnt="2">
                    <style id="0" type="PARA" name="바탕글" engName="Normal"
                           paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" langID="1042"/>
                    <style id="1" type="CHAR" name="본문" engName="Body"
                           paraPrIDRef="1" charPrIDRef="1" nextStyleIDRef="1" langID="1042"/>
                </styles>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.style_count(), 2);

        let s0 = store.style(0).unwrap();
        assert_eq!(s0.name, "바탕글");
        assert_eq!(s0.eng_name, "Normal");
        assert_eq!(s0.style_type, "PARA");
        assert_eq!(s0.lang_id, 1042);

        let s1 = store.style(1).unwrap();
        assert_eq!(s1.name, "본문");
        assert_eq!(s1.eng_name, "Body");
        assert_eq!(s1.style_type, "CHAR");
    }

    #[test]
    fn parse_header_without_styles() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <fontfaces itemCnt="1">
                    <fontface lang="HANGUL" fontCnt="1">
                        <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                    </fontface>
                </fontfaces>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        assert_eq!(store.style_count(), 0);
    }
}
