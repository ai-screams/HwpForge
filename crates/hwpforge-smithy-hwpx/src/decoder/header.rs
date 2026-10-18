//! Parses `Contents/header.xml` into an [`HwpxStyleStore`].
//!
//! Converts XML schema types (`HxCharPr`, `HxParaPr`, `HxFont`) into
//! Foundation types (`Color`, `HwpUnit`, `Alignment`) for the store.

use hwpforge_foundation::{
    BorderFillIndex, BreakType, CharShadow, Color, EmbossType, EmphasisType, EngraveType,
    FontIndex, HeadingType, HwpUnit, LineSpacingType, OutlineType, ShadowOffset, ShadowType,
    StrikeoutShape, TabAlign, TabLeader, UnderlineShape, UnderlineType, VerticalPosition,
    WordBreakType,
};

use crate::decoder::{xml_error_detail, xml_from_str};
use crate::error::{HwpxError, HwpxResult};
use crate::list_bridge::bullet_def_from_hwpx;
use crate::paragraph_measure::{
    case_measure, decode_line_spacing, invalid, pack_margin, pack_tab, unpack_margin,
    unpack_xml_tab, MEASURE_NAMESPACE,
};
use crate::schema::header::{
    HxBorderFill, HxCharPr, HxHead, HxNumbering, HxParaPr, HxRefList, HxShadow, HxStyle, HxTabItem,
    HxTabPr,
};
use crate::style_store::{
    parse_alignment, parse_hex_color, HwpxBorderFill, HwpxBorderLine, HwpxCharShape,
    HwpxDiagonalLine, HwpxFill, HwpxFont, HwpxFontRef, HwpxFontType, HwpxFontTypeInfo,
    HwpxGradientFill, HwpxImageFill, HwpxParaShape, HwpxStyle, HwpxStyleStore, HwpxSubstFont,
};
use hwpforge_core::section::BeginNum;
use hwpforge_core::ParagraphMeasure;

/// Result of parsing `header.xml`.
#[derive(Debug)]
pub struct HeaderParseResult {
    /// Style information parsed from `header.xml`.
    pub style_store: HwpxStyleStore,
    /// Starting auto-numbering values from `<hh:beginNum>`, if present.
    pub begin_num: Option<BeginNum>,
    /// What the style store could not hold (per-language character scalars).
    pub warnings: Vec<super::DecodeWarning>,
    pub(crate) memo_ids: std::collections::BTreeSet<u32>,
    pub(crate) track_change_ids: std::collections::BTreeSet<u32>,
    pub(crate) track_change_author_ids: std::collections::BTreeSet<u32>,
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
    let head: HxHead = xml_from_str(xml).map_err(|e| HwpxError::XmlParse {
        file: "header.xml".into(),
        detail: xml_error_detail(&e),
    })?;
    let begin_num = parse_begin_num(&head);

    let mut store = HwpxStyleStore::new();
    let mut warnings = Vec::new();
    let mut memo_ids = std::collections::BTreeSet::new();
    let mut track_change_ids = std::collections::BTreeSet::new();
    let mut track_change_author_ids = std::collections::BTreeSet::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(|e| invalid(e.to_string()))? {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                if matches!(
                    e.local_name().as_ref(),
                    "memoPr" | "trackChange" | "trackChangeAuthor"
                ) =>
            {
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| invalid(e.to_string()))?;
                    if attr.key.as_ref() == "id" {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|e| invalid(e.to_string()))?;
                        let id = value.parse().map_err(|_| {
                            invalid(format!("{} invalid ID {value}", e.local_name().as_ref()))
                        })?;
                        match e.local_name().as_ref() {
                            "memoPr" => {
                                memo_ids.insert(id);
                            }
                            "trackChange" => {
                                track_change_ids.insert(id);
                            }
                            _ => {
                                track_change_author_ids.insert(id);
                            }
                        }
                    }
                }
                warnings.push(super::DecodeWarning::ContentLost {
                    part: "Contents/header.xml".into(),
                    path: super::ParagraphPath::default(),
                    kind: e.local_name().as_ref().to_string(),
                    reason: "unsupported header definition and its references are not carried"
                        .into(),
                });
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    if let Some(ref_list) = &head.ref_list {
        populate_store_from_ref_list(&mut store, ref_list, &mut warnings)?;
        store.header_fidelity.capture(ref_list);
        warnings.extend(per_language_scalar_losses(ref_list));
        warnings.extend(border_offset_losses(ref_list));
        warnings.extend(char_shadow_losses(ref_list));
        warnings.extend(font_embedding_losses(ref_list));
    }

    crate::header_references::HeaderReferences::build(&store)?;
    Ok(HeaderParseResult {
        style_store: store,
        begin_num,
        warnings,
        memo_ids,
        track_change_ids,
        track_change_author_ids,
    })
}

/// A `paraPr` border offset outside the HWPUNIT range: the style store
/// cannot hold it and keeps 0 (review r1: it was dropped without a word).
/// 이것을 실패시키는 것: 이 보고를 지우기
/// (`para_border_offset_out_of_range_is_reported_lost`).
fn border_offset_losses(ref_list: &HxRefList) -> Vec<super::DecodeWarning> {
    let Some(para_props) = &ref_list.para_properties else { return Vec::new() };
    para_props
        .items
        .iter()
        .filter_map(|pp| {
            let b = pp.border.as_ref()?;
            let bad: Vec<i32> = [b.offset_left, b.offset_right, b.offset_top, b.offset_bottom]
                .into_iter()
                .filter(|&v| HwpUnit::new(v).is_err())
                .collect();
            (!bad.is_empty()).then(|| super::DecodeWarning::ContentLost {
                part: "Contents/header.xml".to_string(),
                path: super::ParagraphPath::default(),
                kind: "para_border_offset".to_string(),
                reason: format!(
                    "paraPr {} border offset {bad:?} is out of range; kept as 0",
                    pp.id
                ),
            })
        })
        .collect()
}

/// A `charPr` shadow the style store cannot hold as written — an unknown
/// `type`, a `color` that is neither `#RRGGBB` nor `none`, or an offset
/// outside −100..=100. The store keeps a fallback (kind NONE, the default
/// color, the clamped offset), so a re-encode would write a different
/// shadow; the editors refuse the input instead (S21 r1-1).
fn char_shadow_losses(ref_list: &HxRefList) -> Vec<super::DecodeWarning> {
    let Some(char_props) = &ref_list.char_properties else { return Vec::new() };
    char_props
        .items
        .iter()
        .filter_map(|cp| {
            let problems = parse_char_shadow(cp.shadow.as_ref()?).1;
            (!problems.is_empty()).then(|| super::DecodeWarning::ContentLost {
                part: "Contents/header.xml".to_string(),
                path: super::ParagraphPath::default(),
                kind: "char_shadow".to_string(),
                reason: format!(
                    "charPr {} shadow {} cannot be kept as written",
                    cp.id,
                    problems.join(", ")
                ),
            })
        })
        .collect()
}

/// A `charPr` whose ratio, spacing, relative size or offset differs between
/// languages: the style store keeps one value per field (the hangul one),
/// so a re-encode writes it for every language and the other scripts' glyph
/// widths and positions change. Reported as lost, one warning per `charPr`.
fn per_language_scalar_losses(ref_list: &HxRefList) -> Vec<super::DecodeWarning> {
    let Some(char_props) = &ref_list.char_properties else { return Vec::new() };
    let uniform = |v: &Option<crate::schema::header::HxLangValues>| {
        v.as_ref().is_none_or(|v| {
            [v.latin, v.hanja, v.japanese, v.other, v.symbol, v.user].iter().all(|x| *x == v.hangul)
        })
    };
    char_props
        .items
        .iter()
        .filter_map(|cp| {
            let fields: Vec<&str> = [
                ("ratio", &cp.ratio),
                ("spacing", &cp.spacing),
                ("relSz", &cp.rel_sz),
                ("offset", &cp.offset),
            ]
            .into_iter()
            .filter(|(_, v)| !uniform(v))
            .map(|(name, _)| name)
            .collect();
            (!fields.is_empty()).then(|| super::DecodeWarning::ContentLost {
                part: "Contents/header.xml".to_string(),
                path: super::ParagraphPath::default(),
                kind: "char_shape_per_language".to_string(),
                reason: format!(
                    "charPr {} has per-language {} values; only the hangul value is kept",
                    cp.id,
                    fields.join(", ")
                ),
            })
        })
        .collect()
}

/// An embedded font (`<hh:font isEmbedded="1">`) or substitute font
/// (`<hh:substFont isEmbedded="1">` or a `binaryItemIDRef`): the style store
/// keeps neither the flag nor the reference, so a re-encode writes the font
/// as not embedded and its link to the package binary is cut. Reported as
/// lost (editors then refuse the input); the Hancom corpus has none (3,828
/// header parts, 59,531 substitutes, 0 embedded), so carrying the binary
/// link is left until one is seen.
/// 이것을 실패시키는 것: 이 보고를 지우기 · 대체 글꼴 참조를 보지 않기
/// (`embedded_fonts_are_reported_as_lost`).
fn font_embedding_losses(ref_list: &HxRefList) -> Vec<super::DecodeWarning> {
    let Some(fontfaces) = &ref_list.fontfaces else { return Vec::new() };
    let lost = |id: u32, lang: &str, what: &str| super::DecodeWarning::ContentLost {
        part: "Contents/header.xml".to_string(),
        path: super::ParagraphPath::default(),
        kind: "font_embedding".to_string(),
        reason: format!("{lang} font {id}: the embedded {what} is written as not embedded"),
    };
    let mut out = Vec::new();
    for group in &fontfaces.groups {
        for font in &group.fonts {
            if font.is_embedded != 0 {
                out.push(lost(font.id, &group.lang, "font"));
            }
            if font
                .subst_font
                .as_ref()
                .is_some_and(|s| s.is_embedded != 0 || !s.binary_item_id_ref.is_empty())
            {
                out.push(lost(font.id, &group.lang, "substitute font"));
            }
        }
    }
    out
}

fn parse_begin_num(head: &HxHead) -> Option<BeginNum> {
    head.begin_num.map(|bn| BeginNum {
        page: bn.page,
        footnote: bn.footnote,
        endnote: bn.endnote,
        pic: bn.pic,
        tbl: bn.tbl,
        equation: bn.equation,
        // header.xml <hh:beginNum> 에는 pageStartsOn 이 없다 — 섹션
        // <hp:startNum> 전용 속성 (기본 Both).
        ..BeginNum::default()
    })
}

fn populate_store_from_ref_list(
    store: &mut HwpxStyleStore,
    ref_list: &HxRefList,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<()> {
    load_fonts(store, ref_list);
    load_border_fills(store, ref_list);
    load_char_shapes(store, ref_list);
    load_para_shapes(store, ref_list, warnings)?;
    load_tab_properties(store, ref_list, warnings)?;
    load_numberings(store, ref_list);
    load_bullets(store, ref_list);
    load_styles(store, ref_list);
    Ok(())
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
                    // Same cleaning as the encoder, so a no-op round trip stays one.
                    face_name: crate::wire_xml::clean_font_name(&font.face),
                    lang: group.lang.clone(),
                    // C: kind, substitute and classification round-trip.
                    // 이것을 실패시키는 것: 셋 중 하나를 읽지 않기
                    // (`font_metadata_round_trips`).
                    font_type: HwpxFontType::from_hwpx_str(&font.font_type),
                    substitute: font.subst_font.as_ref().map(|sub| {
                        HwpxSubstFont::new(
                            HwpxFontType::from_hwpx_str(&sub.font_type),
                            crate::wire_xml::clean_font_name(&sub.face),
                        )
                    }),
                    type_info: font.type_info.as_ref().map(|ti| {
                        HwpxFontTypeInfo::new(
                            ti.family_type.clone(),
                            [
                                ti.weight,
                                ti.proportion,
                                ti.contrast,
                                ti.stroke_variation,
                                ti.arm_style,
                                ti.letterform,
                                ti.midline,
                                ti.x_height,
                            ],
                        )
                    }),
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

fn load_para_shapes(
    store: &mut HwpxStyleStore,
    ref_list: &HxRefList,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<()> {
    if let Some(para_props) = &ref_list.para_properties {
        for pp in &para_props.items {
            store.push_para_shape(
                convert_para_pr(pp, warnings)
                    .map_err(|e| invalid(format!("paraPr {}: {e}", pp.id)))?,
            );
        }
    }
    Ok(())
}

fn load_tab_properties(
    store: &mut HwpxStyleStore,
    ref_list: &HxRefList,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<()> {
    if let Some(tab_props) = &ref_list.tab_properties {
        for tp in &tab_props.items {
            store.push_tab(
                convert_tab(tp, warnings).map_err(|e| invalid(format!("tabPr {}: {e}", tp.id)))?,
            );
        }
    }
    Ok(())
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
    parse_number_format_opt(s).unwrap_or(hwpforge_foundation::NumberFormatType::Digit)
}

/// [`parse_number_format`] 의 판정 코어 — 미지/결측 = `None`.
///
/// 호출자가 폴백 사실을 표면화(디코드 경고)할 수 있게 분리했다 (W5-α M4).
pub(crate) fn parse_number_format_opt(s: &str) -> Option<hwpforge_foundation::NumberFormatType> {
    use hwpforge_foundation::NumberFormatType;
    match s {
        "DIGIT" => Some(NumberFormatType::Digit),
        "CIRCLED_DIGIT" => Some(NumberFormatType::CircledDigit),
        "ROMAN_CAPITAL" => Some(NumberFormatType::RomanCapital),
        "ROMAN_SMALL" => Some(NumberFormatType::RomanSmall),
        "LATIN_CAPITAL" => Some(NumberFormatType::LatinCapital),
        "LATIN_SMALL" => Some(NumberFormatType::LatinSmall),
        "CIRCLED_LATIN_SMALL" => Some(NumberFormatType::CircledLatinSmall),
        "HANGUL_SYLLABLE" => Some(NumberFormatType::HangulSyllable),
        "HANGUL_JAMO" => Some(NumberFormatType::HangulJamo),
        "HANJA_DIGIT" => Some(NumberFormatType::HanjaDigit),
        "CIRCLED_HANGUL_SYLLABLE" => Some(NumberFormatType::CircledHangulSyllable),
        _ => None,
    }
}

/// Converts an [`HxTabPr`] XML type into a [`hwpforge_core::TabDef`].
fn convert_tab(
    hx: &HxTabPr,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<hwpforge_core::TabDef> {
    Ok(hwpforge_core::TabDef {
        id: hx.id,
        auto_tab_left: hx.auto_tab_left != 0,
        auto_tab_right: hx.auto_tab_right != 0,
        stops: collect_tab_items(hx, warnings)
            .map_err(|e| invalid(format!("tabPr {}.stops: {e}", hx.id)))?,
    })
}

fn collect_tab_items(
    hx: &HxTabPr,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<Vec<hwpforge_core::TabStop>> {
    if !hx.items.is_empty() {
        if !hx.switches.is_empty() || hx.items.iter().any(|i| !i.unit.is_empty()) {
            return Err(invalid(format!(
                "tabPr {} conflicting or unverified direct tab unit",
                hx.id
            )));
        }
        // 12 original HWPX/HWP5 twins, 24 direct definitions: positions equal
        // HWP5 packed raw. Evidence: tests/fixtures/paragraph_measure/direct-twin-evidence.json.
        normalized(
            warnings,
            "tabPr",
            hx.id,
            "stops",
            "verified unitless direct packed tabs will be emitted as switch",
        );
        return convert_tab_items(&hx.items, false);
    }
    let mut selected = Vec::new();
    for sw in &hx.switches {
        if !sw.cases.iter().any(|c| !c.items.is_empty())
            && sw.default.as_ref().is_none_or(|d| d.items.is_empty())
        {
            continue;
        }
        let case = sw.cases.iter().find(|c| c.required_namespace == MEASURE_NAMESPACE);
        if let Some(c) = case {
            let stops = convert_tab_items(&c.items, true)?;
            if let Some(d) = &sw.default {
                if d.items.len() != stops.len() {
                    normalized(
                        warnings,
                        "tabPr",
                        hx.id,
                        "stops",
                        "default stop count differs from case",
                    );
                }
                for (i, (stop, raw)) in stops.iter().zip(&d.items).enumerate() {
                    if pack_tab(stop.position).ok() != Some(raw.pos)
                        || (!raw.unit.is_empty() && raw.unit != "HWPUNIT")
                        || stop.align != TabAlign::from_hwpx_str(&raw.tab_type)
                        || stop.leader != TabLeader::from_hwpx_str(&raw.leader)
                    {
                        normalized(
                            warnings,
                            "tabPr",
                            hx.id,
                            &format!("position[{i}]"),
                            "default differs from selected case; regenerate from case",
                        );
                    }
                }
            } else {
                normalized(
                    warnings,
                    "tabPr",
                    hx.id,
                    "stops",
                    "case-only tabs; generate packed default",
                );
            }
            selected.extend(stops);
        } else if let Some(d) = &sw.default {
            normalized(
                warnings,
                "tabPr",
                hx.id,
                "stops",
                "no supported case; selected packed default",
            );
            selected.extend(convert_tab_items(&d.items, false)?);
        } else {
            return Err(invalid(format!("tabPr {} unsupported namespace without default", hx.id)));
        }
    }
    Ok(selected)
}

fn convert_tab_items(items: &[HxTabItem], case: bool) -> HwpxResult<Vec<hwpforge_core::TabStop>> {
    items
        .iter()
        .map(|item| {
            let position = if case {
                case_measure(
                    i32::try_from(item.pos).map_err(|_| invalid("case tab overflow"))?,
                    &item.unit,
                )?
            } else if item.unit.is_empty() || item.unit == "HWPUNIT" {
                unpack_xml_tab(item.pos)?
            } else {
                return Err(invalid("packed tab unknown unit"));
            };
            pack_tab(position)?;
            Ok(hwpforge_core::TabStop {
                position,
                align: TabAlign::from_hwpx_str(&item.tab_type),
                leader: TabLeader::from_hwpx_str(&item.leader),
            })
        })
        .collect()
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

/// Parses a HWPX underline shape string to [`UnderlineShape`].
///
/// Unknown values fall back to [`UnderlineShape::Solid`] (warning-first principle:
/// non-recognized shapes are tolerated rather than causing a fatal error).
fn parse_underline_shape(s: &str) -> UnderlineShape {
    s.parse::<UnderlineShape>().unwrap_or(UnderlineShape::Solid)
}

/// Parses a HWPX strikeout shape string to [`StrikeoutShape`].
fn parse_strikeout_shape(s: &str) -> StrikeoutShape {
    match s.to_ascii_uppercase().as_str() {
        "NONE" => StrikeoutShape::None,
        "SOLID" | "SLASH" => StrikeoutShape::Solid,
        "DASH" => StrikeoutShape::Dash,
        "DOT" => StrikeoutShape::Dot,
        "DASH_DOT" => StrikeoutShape::DashDot,
        "DASH_DOT_DOT" => StrikeoutShape::DashDotDot,
        "LONG_DASH" => StrikeoutShape::LongDash,
        "CIRCLE" => StrikeoutShape::Circle,
        "DOUBLE_SLIM" => StrikeoutShape::DoubleSlim,
        "SLIM_THICK" => StrikeoutShape::SlimThick,
        "THICK_SLIM" => StrikeoutShape::ThickSlim,
        "THICK_SLIM_THICK" => StrikeoutShape::ThickSlimThick,
        "WAVE" => StrikeoutShape::Wave,
        _ => StrikeoutShape::None,
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

/// Parses `<hh:shadow>` into a [`CharShadow`], naming each value it could
/// not keep (an unknown type falls back to NONE, a bad color to the default
/// color, an out-of-range offset is clamped).
fn parse_char_shadow(s: &HxShadow) -> (CharShadow, Vec<String>) {
    let mut problems = Vec::new();
    let kind = match s.shadow_type.to_ascii_uppercase().as_str() {
        "NONE" => ShadowType::None,
        "DROP" => ShadowType::Drop,
        "CONTINUOUS" => ShadowType::Continuous,
        _ => {
            problems.push(format!("type {:?}", s.shadow_type));
            ShadowType::None
        }
    };
    let color = if s.color.eq_ignore_ascii_case("none") {
        None
    } else if let Ok(c) = Color::from_hex_rgb(&s.color) {
        Some(c)
    } else {
        problems.push(format!("color {:?}", s.color));
        CharShadow::DEFAULT.color()
    };
    let (x, x_clamped) = ShadowOffset::saturating_from_i32(s.offset_x);
    let (y, y_clamped) = ShadowOffset::saturating_from_i32(s.offset_y);
    if x_clamped || y_clamped {
        problems.push(format!("offset ({}, {})", s.offset_x, s.offset_y));
    }
    let shadow = CharShadow::new(kind)
        .with_color(color)
        .expect("from_hex_rgb and DEFAULT colors have a zero upper byte")
        .with_offset(x, y);
    (shadow, problems)
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

    let mut shape = HwpxCharShape::default();
    shape.source_id = Some(cp.id);
    shape.font_ref = font_ref;
    shape.height = height;
    shape.text_color = parse_hex_color(&cp.text_color);
    shape.shade_color = parse_optional_hex_color(&cp.shade_color);
    shape.bold = cp.bold.is_some();
    shape.italic = cp.italic.is_some();
    shape.underline_type = cp
        .underline
        .as_ref()
        .map(|u| parse_underline_type(&u.underline_type))
        .unwrap_or(UnderlineType::None);
    shape.underline_shape = cp
        .underline
        .as_ref()
        .map(|u| parse_underline_shape(&u.shape))
        .unwrap_or(UnderlineShape::Solid);
    shape.underline_color = cp.underline.as_ref().and_then(|u| {
        let c = parse_hex_color(&u.color);
        if c == Color::BLACK {
            None
        } else {
            Some(c)
        }
    });
    shape.strikeout_shape = cp
        .strikeout
        .as_ref()
        .map(|s| parse_strikeout_shape(&s.shape))
        .unwrap_or(StrikeoutShape::None);
    shape.strikeout_color = cp.strikeout.as_ref().and_then(|s| {
        let c = parse_hex_color(&s.color);
        if c == Color::BLACK {
            None
        } else {
            Some(c)
        }
    });
    shape.vertical_position = parse_vertical_position(cp);
    shape.outline_type = cp
        .outline
        .as_ref()
        .map(|o| parse_outline_type(&o.outline_type))
        .unwrap_or(OutlineType::None);
    // No `<hh:shadow>` (older Hancom versions omit it for kind 0): the
    // default, without a warning — the kind is NONE either way.
    shape.shadow = cp.shadow.as_ref().map_or(CharShadow::DEFAULT, |s| parse_char_shadow(s).0);
    shape.emboss_type = if cp.emboss.is_some() { EmbossType::Emboss } else { EmbossType::None };
    shape.engrave_type =
        if cp.engrave.is_some() { EngraveType::Engrave } else { EngraveType::None };
    shape.emphasis = parse_emphasis_type(&cp.sym_mark);
    shape.ratio = cp.ratio.as_ref().map_or(100, |r| r.hangul);
    shape.spacing = cp.spacing.as_ref().map_or(0, |s| s.hangul);
    shape.rel_sz = cp.rel_sz.as_ref().map_or(100, |r| r.hangul);
    shape.char_offset = cp.offset.as_ref().map_or(0, |o| o.hangul);
    shape.use_kerning = cp.use_kerning != 0;
    shape.use_font_space = cp.use_font_space != 0;
    shape.border_fill_id = Some(cp.border_fill_id_ref);
    shape
}

fn parse_vertical_position(cp: &HxCharPr) -> VerticalPosition {
    // Legacy payloads can set both flags. Shared IR intentionally normalizes to
    // one vertical-position enum and keeps superscript precedence.
    if cp.supscript.is_some() {
        VerticalPosition::Superscript
    } else if cp.subscript.is_some() {
        VerticalPosition::Subscript
    } else {
        VerticalPosition::Normal
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
fn convert_para_pr(
    pp: &HxParaPr,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<HwpxParaShape> {
    let default_para_shape = HwpxParaShape::default();
    let alignment = pp
        .align
        .as_ref()
        .map(|a| parse_alignment(&a.horizontal))
        .unwrap_or(default_para_shape.alignment);

    // Margin and line spacing come from hp:switch/hp:default
    let (margin_left, margin_right, indent, spacing_before, spacing_after) =
        extract_margins(pp, warnings)?;

    let (line_spacing, line_spacing_type) = extract_line_spacing(pp, warnings)?;

    let (break_latin_word, break_non_latin_word, line_wrap) = pp
        .break_setting
        .as_ref()
        .map(|bs| {
            (
                parse_latin_word_break_type(&bs.break_latin_word),
                parse_non_latin_word_break_type(&bs.break_non_latin_word),
                normalize_line_wrap(&bs.line_wrap),
            )
        })
        .unwrap_or((
            default_para_shape.break_latin_word,
            default_para_shape.break_non_latin_word,
            default_para_shape.line_wrap.clone(),
        ));
    let break_type = pp.break_setting.as_ref().map_or(default_para_shape.break_type, |bs| {
        if bs.page_break_before != 0 {
            BreakType::Page
        } else {
            BreakType::None
        }
    });
    let keep_with_next = pp
        .break_setting
        .as_ref()
        .map(|bs| bs.keep_with_next != 0)
        .unwrap_or(default_para_shape.keep_with_next);
    let keep_lines_together = pp
        .break_setting
        .as_ref()
        .map(|bs| bs.keep_lines != 0)
        .unwrap_or(default_para_shape.keep_lines_together);
    let widow_orphan = pp
        .break_setting
        .as_ref()
        .map(|bs| bs.widow_orphan != 0)
        .unwrap_or(default_para_shape.widow_orphan);
    let border_fill_id = pp.border.as_ref().and_then(|border| {
        (border.border_fill_id_ref != 0)
            .then(|| BorderFillIndex::new(border.border_fill_id_ref as usize))
    });

    let heading_type = pp
        .heading
        .as_ref()
        .map_or(HeadingType::None, |heading| HeadingType::from_hwpx_str(&heading.heading_type));
    let heading_id_ref = pp.heading.as_ref().map_or(0, |heading| heading.id_ref);
    let heading_level = pp.heading.as_ref().map_or(0, |heading| heading.level);
    let checked = pp.checked != 0;
    let tab_pr_id_ref = pp.tab_pr_id_ref;
    let condense = pp.condense;
    let vertical_align = pp
        .align
        .as_ref()
        .map(|a| a.vertical.trim())
        .filter(|v| !v.is_empty())
        .map_or(default_para_shape.vertical_align, str::to_string);
    let offset = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);
    let border_offset = pp.border.as_ref().map_or(default_para_shape.border_offset, |b| {
        [b.offset_left, b.offset_right, b.offset_top, b.offset_bottom].map(offset)
    });
    let border_connect = pp.border.as_ref().is_some_and(|b| b.connect != 0);
    let border_ignore_margin = pp.border.as_ref().is_some_and(|b| b.ignore_margin != 0);

    Ok(HwpxParaShape {
        source_id: Some(pp.id),
        alignment,
        margin_left,
        margin_right,
        indent,
        spacing_before,
        spacing_after,
        line_spacing,
        line_spacing_type,
        snap_to_grid: pp.snap_to_grid.unwrap_or(u32::from(default_para_shape.snap_to_grid)) != 0,
        break_type,
        keep_with_next,
        keep_lines_together,
        widow_orphan,
        break_latin_word,
        break_non_latin_word,
        line_wrap,
        border_fill_id,
        heading_type,
        heading_id_ref,
        heading_level,
        checked,
        tab_pr_id_ref,
        condense,
        vertical_align,
        border_offset,
        border_connect,
        border_ignore_margin,
    })
}

/// Parses `breakLatinWord` wire values into a [`WordBreakType`].
fn parse_latin_word_break_type(s: &str) -> WordBreakType {
    match s {
        "BREAK_WORD" => WordBreakType::BreakWord,
        "HYPHENATION" => WordBreakType::Hyphenation,
        _ => WordBreakType::KeepWord,
    }
}

/// Parses `breakNonLatinWord` wire values into a [`WordBreakType`].
///
/// Hancom HWPX stores the non-Latin labels opposite to the shared semantic
/// meaning, so the inversion stays local to this codec boundary.
fn parse_non_latin_word_break_type(s: &str) -> WordBreakType {
    match s {
        "BREAK_WORD" => WordBreakType::KeepWord,
        _ => WordBreakType::BreakWord,
    }
}

fn normalize_line_wrap(s: &str) -> String {
    let normalized = s.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        HwpxParaShape::default().line_wrap
    } else {
        normalized
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

// The selected branch is the only source of fields; never merge branches.
fn select_measure_branch(
    pp: &HxParaPr,
    line: bool,
) -> HwpxResult<Option<(&crate::schema::header::HxSwitch, bool)>> {
    let mut selected = None;
    for sw in &pp.switches {
        let has_payload =
            sw.cases
                .iter()
                .any(|c| if line { c.line_spacing.is_some() } else { c.margin.is_some() })
                || sw.default.as_ref().is_some_and(|d| {
                    if line {
                        d.line_spacing.is_some()
                    } else {
                        d.margin.is_some()
                    }
                });
        if !has_payload {
            continue;
        }
        if selected.is_some() {
            return Err(invalid(format!("paraPr {} multiple measure blocks", pp.id)));
        }
        let supported = sw.cases.iter().find(|c| c.required_namespace == MEASURE_NAMESPACE);
        if let Some(c) = supported {
            if !(if line { c.line_spacing.is_some() } else { c.margin.is_some() }) {
                return Err(invalid(format!("paraPr {} selected case missing payload", pp.id)));
            }
        } else if !sw.default.as_ref().is_some_and(|d| {
            if line {
                d.line_spacing.is_some()
            } else {
                d.margin.is_some()
            }
        }) {
            return Err(invalid(format!("paraPr {} unsupported namespace without default", pp.id)));
        }
        selected = Some((sw, supported.is_some()));
    }
    Ok(selected)
}

fn normalized(
    warnings: &mut Vec<super::DecodeWarning>,
    definition: &'static str,
    id: u32,
    field: &str,
    reason: &str,
) {
    warnings.push(super::DecodeWarning::ParagraphMeasureNormalized {
        definition,
        id,
        field: field.into(),
        reason: reason.into(),
    });
}

type Margins =
    (ParagraphMeasure, ParagraphMeasure, ParagraphMeasure, ParagraphMeasure, ParagraphMeasure);
fn margin_values(m: &crate::schema::header::HxMargin, case: bool, id: u32) -> HwpxResult<Margins> {
    let read = |field: &str, v: &Option<crate::schema::header::HxUnitValue>| {
        let decode = || {
            let v = v.as_ref().ok_or_else(|| invalid("partial margin payload"))?;
            let value = i32::try_from(v.value)
                .map_err(|_| invalid("margin value outside signed 32-bit range"))?;
            if case {
                case_measure(value, &v.unit)
            } else if v.unit == "HWPUNIT" {
                unpack_margin(value)
            } else {
                Err(invalid(format!("packed margin unknown unit {:?}", v.unit)))
            }
        };
        decode().map_err(|e| invalid(format!("paraPr {id}.{field}: {e}")))
    };
    Ok((
        read("left", &m.left)?,
        read("right", &m.right)?,
        read("intent", &m.indent)?,
        read("prev", &m.prev)?,
        read("next", &m.next)?,
    ))
}

fn extract_margins(pp: &HxParaPr, warnings: &mut Vec<super::DecodeWarning>) -> HwpxResult<Margins> {
    let selected = select_measure_branch(pp, false)?;
    if pp.margin.is_some() && selected.is_some() {
        return Err(invalid(format!("paraPr {} direct/switch margin conflict", pp.id)));
    }
    if let Some((sw, case)) = selected {
        if case {
            let m = sw
                .cases
                .iter()
                .find(|c| c.required_namespace == MEASURE_NAMESPACE)
                .unwrap()
                .margin
                .as_ref()
                .unwrap();
            let values = margin_values(m, true, pp.id)?;
            if let Some(default) = sw.default.as_ref().and_then(|d| d.margin.as_ref()) {
                for (field, value, raw) in [
                    ("left", values.0, &default.left),
                    ("right", values.1, &default.right),
                    ("intent", values.2, &default.indent),
                    ("prev", values.3, &default.prev),
                    ("next", values.4, &default.next),
                ] {
                    if raw.as_ref().is_none_or(|r| {
                        r.unit != "HWPUNIT"
                            || pack_margin(value).ok().map(i64::from) != Some(r.value)
                    }) {
                        normalized(
                            warnings,
                            "paraPr",
                            pp.id,
                            field,
                            "default differs from selected case; regenerate from case",
                        );
                    }
                }
            } else {
                normalized(
                    warnings,
                    "paraPr",
                    pp.id,
                    "margin",
                    "case-only margin; generate packed default",
                );
            }
            return Ok(values);
        }
        normalized(
            warnings,
            "paraPr",
            pp.id,
            "margin",
            "no supported case; selected packed default",
        );
        return margin_values(sw.default.as_ref().unwrap().margin.as_ref().unwrap(), false, pp.id);
    }
    if let Some(m) = &pp.margin {
        normalized(
            warnings,
            "paraPr",
            pp.id,
            "margin",
            "direct packed margin will be emitted as switch",
        );
        return margin_values(m, false, pp.id);
    }
    let z = ParagraphMeasure::from_hwp_unit(HwpUnit::ZERO);
    Ok((z, z, z, z, z))
}

fn extract_line_spacing(
    pp: &HxParaPr,
    warnings: &mut Vec<super::DecodeWarning>,
) -> HwpxResult<(i32, LineSpacingType)> {
    let selected = select_measure_branch(pp, true)?;
    if pp.line_spacing.is_some() && selected.is_some() {
        return Err(invalid(format!("paraPr {} direct/switch lineSpacing conflict", pp.id)));
    }
    let read = |ls: &crate::schema::header::HxLineSpacing, case| {
        decode_line_spacing(ls.value, &ls.spacing_type, &ls.unit, case)
            .map_err(|e| invalid(format!("paraPr {}.lineSpacing: {e}", pp.id)))
    };
    if let Some((sw, case)) = selected {
        if case {
            let ls = sw
                .cases
                .iter()
                .find(|c| c.required_namespace == MEASURE_NAMESPACE)
                .unwrap()
                .line_spacing
                .as_ref()
                .unwrap();
            let value = read(ls, true)?;
            if let Some(d) = sw.default.as_ref().and_then(|d| d.line_spacing.as_ref()) {
                if read(d, false).ok() != Some(value) {
                    normalized(
                        warnings,
                        "paraPr",
                        pp.id,
                        "lineSpacing",
                        "default differs from selected case; regenerate from case",
                    );
                }
            } else {
                normalized(
                    warnings,
                    "paraPr",
                    pp.id,
                    "lineSpacing",
                    "case-only lineSpacing; generate legacy default",
                );
            }
            return Ok(value);
        }
        normalized(
            warnings,
            "paraPr",
            pp.id,
            "lineSpacing",
            "no supported case; selected legacy default",
        );
        return read(sw.default.as_ref().unwrap().line_spacing.as_ref().unwrap(), false);
    }
    if let Some(ls) = &pp.line_spacing {
        normalized(
            warnings,
            "paraPr",
            pp.id,
            "lineSpacing",
            "direct lineSpacing will be emitted as switch",
        );
        return read(ls, false);
    }
    Ok((160, LineSpacingType::Percentage))
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
    use hwpforge_foundation::{
        Alignment, BorderFillIndex, BreakType, Color, LineSpacingType, StrikeoutShape,
        UnderlineType, WordBreakType,
    };

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

    // The encoder writes font names cleaned (`wire_xml::clean_font_name`); reading
    // them the same way keeps decode→encode→decode a no-op for edits.
    // 이것을 실패시키는 것: `load_fonts` 에서 `clean_font_name` 을 빼는 것.
    #[test]
    fn font_names_are_read_cleaned() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <fontfaces itemCnt="1">
                    <fontface lang="HANGUL" fontCnt="3">
                        <font id="0" face=" 윤명조440" type="TTF" isEmbedded="0"/>
                        <font id="1" face="함초롬바탕&#13;" type="TTF" isEmbedded="0"/>
                        <font id="2" face="Times New Roman" type="TTF" isEmbedded="0"/>
                    </fontface>
                </fontfaces>
            </refList>
        </head>"##;
        let store = parse_header(xml).unwrap().style_store;
        let names: Vec<_> =
            (0..3).map(|i| store.font(FontIndex::new(i)).unwrap().face_name.clone()).collect();
        assert_eq!(names, ["윤명조440", "함초롬바탕", "Times New Roman"]);
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
        assert_eq!(
            tabs[3].stops[0].position,
            hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(15000).unwrap())
        );
        assert_eq!(tabs[3].stops[0].align, TabAlign::Left);
        assert_eq!(tabs[3].stops[0].leader.as_hwpx_str(), "DASH");
    }

    #[test]
    fn parse_header_tab_properties_rejects_oversized_positions_without_wrap() {
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

        // 이것을 실패시키는 것: 변이 12 — 범위 오류를 clamp로 바꾸기.
        assert!(parse_header(xml).is_err());
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

    /// Review r1 HIGH-1: an embedded font or substitute font (flag or a
    /// binary reference) is reported as lost content; a plain one is not.
    /// 이것을 실패시키는 것: 보고 삭제 · 대체 글꼴 참조 무시 · 일반 글꼴에도 보고.
    #[test]
    fn embedded_fonts_are_reported_as_lost() {
        let header = |font_embedded: u32, subst: &str| {
            format!(
                r#"<head version="1.4" secCnt="1"><refList><fontfaces itemCnt="1"><fontface lang="HANGUL" fontCnt="1"><font id="0" face="A" type="TTF" isEmbedded="{font_embedded}">{subst}</font></fontface></fontfaces></refList></head>"#
            )
        };
        let lost = |xml: String| {
            parse_header(&xml)
                .unwrap()
                .warnings
                .iter()
                .filter(|w| matches!(w, crate::DecodeWarning::ContentLost { kind, .. } if kind == "font_embedding"))
                .count()
        };
        let plain = r#"<substFont face="B" type="TTF" isEmbedded="0" binaryItemIDRef=""/>"#;
        assert_eq!(lost(header(0, plain)), 0);
        assert_eq!(lost(header(0, "")), 0);
        assert_eq!(lost(header(1, plain)), 1);
        assert_eq!(
            lost(header(
                0,
                r#"<substFont face="B" type="TTF" isEmbedded="1" binaryItemIDRef=""/>"#
            )),
            1
        );
        assert_eq!(
            lost(header(
                0,
                r#"<substFont face="B" type="TTF" isEmbedded="0" binaryItemIDRef="font1"/>"#
            )),
            1
        );
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
                        <strikeout shape="SOLID" color="#000000"/>
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
        assert_eq!(cs.strikeout_shape, StrikeoutShape::Solid);
    }

    #[test]
    fn parse_char_pr_with_relief_and_vertical_position() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <charProperties itemCnt="2">
                    <charPr id="17" height="900" textColor="#0D0D0D" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="3">
                        <fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                        <underline type="NONE" shape="SOLID" color="#000000"/>
                        <strikeout shape="NONE" color="#000000"/>
                        <outline type="NONE"/>
                        <shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>
                        <emboss/>
                        <supscript/>
                    </charPr>
                    <charPr id="18" height="900" textColor="#0D0D0D" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="3">
                        <fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                        <underline type="NONE" shape="SOLID" color="#000000"/>
                        <strikeout shape="NONE" color="#000000"/>
                        <outline type="NONE"/>
                        <shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>
                        <engrave/>
                        <subscript/>
                    </charPr>
                </charProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;

        let superscript = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(superscript.vertical_position, VerticalPosition::Superscript);
        assert_eq!(superscript.emboss_type, EmbossType::Emboss);
        assert_eq!(superscript.engrave_type, EngraveType::None);

        let subscript = store.char_shape(hwpforge_foundation::CharShapeIndex::new(1)).unwrap();
        assert_eq!(subscript.vertical_position, VerticalPosition::Subscript);
        assert_eq!(subscript.emboss_type, EmbossType::None);
        assert_eq!(subscript.engrave_type, EngraveType::Engrave);
    }

    #[test]
    fn parse_char_pr_with_conflicting_vertical_position_prefers_superscript() {
        let xml = r##"<head version="1.4" secCnt="1">
            <refList>
                <charProperties itemCnt="1">
                    <charPr id="17" height="900" textColor="#0D0D0D" shadeColor="none"
                            useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="3">
                        <fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                        <underline type="NONE" shape="SOLID" color="#000000"/>
                        <strikeout shape="NONE" color="#000000"/>
                        <outline type="NONE"/>
                        <shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>
                        <supscript/>
                        <subscript/>
                    </charPr>
                </charProperties>
            </refList>
        </head>"##;

        let store = parse_header(xml).unwrap().style_store;
        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.vertical_position, VerticalPosition::Superscript);
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
                                    <intent value="200" unit="HWPUNIT"/>
                                    <left value="100" unit="HWPUNIT"/>
                                    <right value="50" unit="HWPUNIT"/>
                                    <prev value="300" unit="HWPUNIT"/>
                                    <next value="150" unit="HWPUNIT"/>
                                </margin>
                                <lineSpacing type="PERCENT" value="200" unit="HWPUNIT"/>
                            </default>
                        </switch>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;
        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.indent.as_hwp_unit().unwrap().as_i32(), 100);
        assert_eq!(ps.margin_left.as_hwp_unit().unwrap().as_i32(), 50);
        assert_eq!(ps.margin_right.as_hwp_unit().unwrap().as_i32(), 25);
        assert_eq!(ps.spacing_before.as_hwp_unit().unwrap().as_i32(), 150);
        assert_eq!(ps.spacing_after.as_hwp_unit().unwrap().as_i32(), 75);
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
        assert_eq!(ps.margin_left, hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO));
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, LineSpacingType::Percentage);
        assert!(ps.snap_to_grid);
        assert!(ps.widow_orphan);
        assert_eq!(ps.line_wrap, "BREAK");
    }

    #[test]
    fn parse_para_pr_preserves_supported_break_flags_and_border_fill() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0" tabPrIDRef="3" condense="20" snapToGrid="0" checked="1">
                        <align horizontal="LEFT" vertical="BASELINE"/>
                        <breakSetting breakLatinWord="BREAK_WORD" breakNonLatinWord="KEEP_WORD"
                            widowOrphan="1" keepWithNext="1" keepLines="1" pageBreakBefore="1"
                            lineWrap="SQUEEZE"/>
                        <border borderFillIDRef="5" offsetLeft="0" offsetRight="0" offsetTop="0"
                            offsetBottom="0" connect="0" ignoreMargin="0"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;

        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();

        assert_eq!(ps.break_latin_word, WordBreakType::BreakWord);
        assert_eq!(ps.break_non_latin_word, WordBreakType::BreakWord);
        assert_eq!(ps.break_type, BreakType::Page);
        assert!(ps.keep_with_next);
        assert!(ps.keep_lines_together);
        assert!(ps.widow_orphan);
        assert!(!ps.snap_to_grid);
        assert_eq!(ps.line_wrap, "SQUEEZE");
        assert_eq!(ps.border_fill_id, Some(BorderFillIndex::new(5)));
        assert!(ps.checked);
        assert_eq!(ps.tab_pr_id_ref, 3);
        assert_eq!(ps.condense, 20);
    }

    #[test]
    fn parse_para_pr_carries_latin_hyphenation() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="LEFT" vertical="BASELINE"/>
                        <breakSetting breakLatinWord="HYPHENATION" breakNonLatinWord="KEEP_WORD"
                            widowOrphan="0" keepWithNext="0" keepLines="0" pageBreakBefore="0"
                            lineWrap="BREAK"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;

        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();

        assert_eq!(ps.break_latin_word, WordBreakType::Hyphenation);
    }

    #[test]
    fn parse_para_pr_inverts_hancom_non_latin_break_wire() {
        let xml = r#"<head version="1.4" secCnt="1">
            <refList>
                <paraProperties itemCnt="1">
                    <paraPr id="0">
                        <align horizontal="LEFT" vertical="BASELINE"/>
                        <breakSetting breakLatinWord="KEEP_WORD" breakNonLatinWord="BREAK_WORD"
                            widowOrphan="0" keepWithNext="0" keepLines="0" pageBreakBefore="0"
                            lineWrap="BREAK"/>
                    </paraPr>
                </paraProperties>
            </refList>
        </head>"#;

        let store = parse_header(xml).unwrap().style_store;
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();

        assert_eq!(ps.break_latin_word, WordBreakType::KeepWord);
        assert_eq!(ps.break_non_latin_word, WordBreakType::KeepWord);
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
                                    <left value="0" unit="HWPUNIT"/>
                                    <right value="0" unit="HWPUNIT"/><intent value="0" unit="HWPUNIT"/><prev value="0" unit="HWPUNIT"/><next value="0" unit="HWPUNIT"/>
                                </margin>
                                <lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/>
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
