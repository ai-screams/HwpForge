//! Encodes an [`HwpxStyleStore`] into `header.xml` content.
//!
//! This is the reverse of [`crate::decoder::header::parse_header`]:
//! it converts Foundation types (`Color`, `HwpUnit`, `Alignment`) back
//! into the `Hx*` schema types and serializes them to XML via quick-xml.

use hwpforge_foundation::{Alignment, Color, HwpUnit};

use crate::error::{HwpxError, HwpxResult};
use crate::schema::header::{
    HxAlign, HxAutoSpacing, HxBorder, HxBreakSetting, HxCharPr, HxCharProperties, HxFont,
    HxFontFaceGroup, HxFontFaces, HxFontRef, HxHead, HxHeading, HxLangValues, HxLineSpacing,
    HxMargin, HxOutline, HxParaPr, HxParaProperties, HxPresence, HxRefList, HxShadow, HxStrikeout,
    HxStyle, HxStyles, HxSwitch, HxSwitchCase, HxSwitchDefault, HxUnderline, HxUnitValue,
};
use crate::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyle, HwpxStyleStore};

// ── Public entry point ──────────────────────────────────────────

/// Encodes an [`HwpxStyleStore`] into a complete `header.xml` string.
///
/// The output includes the XML declaration, the `<hh:head>` root element
/// with all HWPX namespace declarations, and the `<hh:refList>` content
/// built from the store's fonts, character shapes, and paragraph shapes.
///
/// # Errors
///
/// Returns [`HwpxError::XmlSerialize`] if quick-xml serialization fails.
pub(crate) fn encode_header(store: &HwpxStyleStore, sec_cnt: u32) -> HwpxResult<String> {
    let head = build_head(store, sec_cnt);
    let head_xml = quick_xml::se::to_string(&head)
        .map_err(|e| HwpxError::XmlSerialize { detail: e.to_string() })?;

    // quick_xml serializes HxHead as `<head version="..." secCnt="...">...</head>`.
    // We need to extract the inner content and wrap it in our xmlns-decorated
    // root element instead.
    let inner = extract_inner_content(&head_xml);
    Ok(wrap_header_xml(inner, sec_cnt))
}

// ── XML wrapper ─────────────────────────────────────────────────

/// Wraps inner XML content in the `<hh:head>` root element with xmlns
/// declarations.
///
/// quick-xml's serde serializer cannot emit xmlns attributes, so we
/// hand-craft the root element and splice in the serialized content.
///
/// Also injects `<hh:beginNum>` (required by 한글) before the refList,
/// and enriches the refList with `<hh:borderFills>` and `<hh:tabProperties>`
/// that charPr/paraPr reference via `borderFillIDRef` and `tabPrIDRef`.
/// Elements required after `</hh:refList>` for 한글 compatibility.
///
/// - `compatibleDocument` — declares target program compatibility.
/// - `docOption` — document link/inheritance settings.
/// - `trackchageConfig` — track-changes flags (note: "trackchage" is an
///   intentional typo preserved from the official format).
const POST_REFLIST_XML: &str = concat!(
    r#"<hh:compatibleDocument targetProgram="HWP201X">"#,
    r#"<hh:layoutCompatibility/>"#,
    r#"</hh:compatibleDocument>"#,
    r#"<hh:docOption>"#,
    r#"<hh:linkinfo path="" pageInherit="0" footnoteInherit="0"/>"#,
    r#"</hh:docOption>"#,
    r#"<hh:trackchageConfig flags="56"/>"#,
);

fn wrap_header_xml(inner_xml: &str, sec_cnt: u32) -> String {
    let enriched = enrich_ref_list(inner_xml);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hh:head{xmlns} version="1.4" secCnt="{sec_cnt}">{begin_num}{enriched}{post_reflist}</hh:head>"#,
        xmlns = crate::encoder::package::XMLNS_DECLS,
        begin_num = BEGIN_NUM_XML,
        post_reflist = POST_REFLIST_XML,
    )
}

// ── 한글 compatibility defaults ─────────────────────────────────

/// `<hh:beginNum>` — required by 한글 for page/footnote numbering.
const BEGIN_NUM_XML: &str =
    r#"<hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/>"#;

/// Default `<hh:borderFills>` with two border definitions.
///
/// `borderFillIDRef="1"` is referenced by `<hp:pageBorderFill>` in secPr.
/// `borderFillIDRef="2"` is referenced by every `<hh:charPr>`.
///
/// id=1: Empty border (no fill).
/// id=2: Character background with `fillBrush`/`winBrush` (required by 한글).
const BORDER_FILLS_XML: &str = concat!(
    r##"<hh:borderFills itemCnt="2">"##,
    r##"<hh:borderFill id="1" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">"##,
    r##"<hh:slash type="NONE" Crooked="0" isCounter="0"/>"##,
    r##"<hh:backSlash type="NONE" Crooked="0" isCounter="0"/>"##,
    r##"<hh:leftBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:rightBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:topBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:bottomBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:diagonal type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"</hh:borderFill>"##,
    r##"<hh:borderFill id="2" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">"##,
    r##"<hh:slash type="NONE" Crooked="0" isCounter="0"/>"##,
    r##"<hh:backSlash type="NONE" Crooked="0" isCounter="0"/>"##,
    r##"<hh:leftBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:rightBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:topBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:bottomBorder type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:diagonal type="NONE" width="0.1 mm" color="#000000"/>"##,
    r##"<hh:fillBrush>"##,
    r##"<hc:winBrush faceColor="none" hatchColor="#FF000000" alpha="0"/>"##,
    r##"</hh:fillBrush>"##,
    r##"</hh:borderFill>"##,
    r##"</hh:borderFills>"##,
);

/// Default `<hh:tabProperties>` with two tab definitions.
///
/// `tabPrIDRef="0"` in every `<hh:paraPr>` references id=0.
/// id=1 has `autoTabLeft="1"` for outline numbering auto-indent.
const TAB_PROPERTIES_XML: &str = concat!(
    r#"<hh:tabProperties itemCnt="2">"#,
    r#"<hh:tabPr id="0" autoTabLeft="0" autoTabRight="0"/>"#,
    r#"<hh:tabPr id="1" autoTabLeft="1" autoTabRight="0"/>"#,
    r#"</hh:tabProperties>"#,
);

/// Default `<hh:numberings>` with one numbering (7 outline levels).
///
/// Referenced by heading styles for automatic outline numbering.
/// `charPrIDRef="4294967295"` (u32::MAX) means "no override / use default".
const NUMBERINGS_XML: &str = concat!(
    r#"<hh:numberings itemCnt="1">"#,
    r#"<hh:numbering id="1" start="0">"#,
    r#"<hh:paraHead level="1" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295">^1.</hh:paraHead>"#,
    r#"<hh:paraHead level="2" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="HANGUL_SYLLABLE" charPrIDRef="4294967295">^2.</hh:paraHead>"#,
    r#"<hh:paraHead level="3" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295">^3)</hh:paraHead>"#,
    r#"<hh:paraHead level="4" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="HANGUL_SYLLABLE" charPrIDRef="4294967295">^4)</hh:paraHead>"#,
    r#"<hh:paraHead level="5" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295">(^5)</hh:paraHead>"#,
    r#"<hh:paraHead level="6" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="HANGUL_SYLLABLE" charPrIDRef="4294967295">(^6)</hh:paraHead>"#,
    r#"<hh:paraHead level="7" align="LEFT" useInstWidth="1" autoIndent="1" textOffsetType="PERCENT" textOffset="50" numFormat="CIRCLED_DIGIT" charPrIDRef="4294967295">^7</hh:paraHead>"#,
    r#"</hh:numbering>"#,
    r#"</hh:numberings>"#,
);

/// Injects `<hh:borderFills>`, `<hh:tabProperties>`, and `<hh:numberings>`
/// into the serialized refList XML at the correct positions.
///
/// Element order inside `<hh:refList>`:
/// fontfaces → **borderFills** → charProperties → **tabProperties** → **numberings** → paraProperties → styles
fn enrich_ref_list(inner_xml: &str) -> String {
    // If no refList exists, nothing to enrich
    if !inner_xml.contains("<hh:refList>") {
        return format!(
            "<hh:refList>{BORDER_FILLS_XML}{TAB_PROPERTIES_XML}{NUMBERINGS_XML}</hh:refList>{inner_xml}"
        );
    }

    let extra_len = BORDER_FILLS_XML.len() + TAB_PROPERTIES_XML.len() + NUMBERINGS_XML.len();
    let mut result = String::with_capacity(inner_xml.len() + extra_len);
    let ref_open = "<hh:refList>";
    let ref_open_pos = inner_xml.find(ref_open).unwrap();
    let after_ref_open = ref_open_pos + ref_open.len();

    // Copy up to and including <hh:refList>
    result.push_str(&inner_xml[..after_ref_open]);

    let rest = &inner_xml[after_ref_open..];

    // Insert borderFills before <hh:charProperties>
    if let Some(cp_pos) = rest.find("<hh:charProperties") {
        result.push_str(&rest[..cp_pos]);
        result.push_str(BORDER_FILLS_XML);

        let rest2 = &rest[cp_pos..];
        // Insert tabProperties + numberings before <hh:paraProperties>
        if let Some(pp_pos) = rest2.find("<hh:paraProperties") {
            result.push_str(&rest2[..pp_pos]);
            result.push_str(TAB_PROPERTIES_XML);
            result.push_str(NUMBERINGS_XML);
            result.push_str(&rest2[pp_pos..]);
        } else {
            result.push_str(rest2);
            result.push_str(TAB_PROPERTIES_XML);
            result.push_str(NUMBERINGS_XML);
        }
    } else {
        // No charProperties — insert all defaults after fontfaces
        result.push_str(BORDER_FILLS_XML);
        result.push_str(TAB_PROPERTIES_XML);
        result.push_str(NUMBERINGS_XML);
        result.push_str(rest);
    }

    result
}

/// Builds a complete `HxHead` from the store data.
fn build_head(store: &HwpxStyleStore, sec_cnt: u32) -> HxHead {
    let ref_list = build_ref_list(store);
    let has_content = ref_list.fontfaces.is_some()
        || ref_list.char_properties.is_some()
        || ref_list.para_properties.is_some()
        || ref_list.styles.is_some();

    HxHead {
        version: "1.4".into(),
        sec_cnt,
        ref_list: if has_content { Some(ref_list) } else { None },
    }
}

/// Extracts the inner content of the serialized `<head ...>...</head>`.
///
/// Finds the end of the opening `<head ...>` tag and the start of the
/// closing `</head>` tag, returning everything in between.
fn extract_inner_content(xml: &str) -> &str {
    // Find the end of the opening tag: first `>` after `<head`
    let open_end = xml.find('>').map(|i| i + 1).unwrap_or(0);
    // Find the closing tag
    let close_start = xml.rfind("</head>").unwrap_or(xml.len());
    &xml[open_end..close_start]
}

// ── RefList builder ─────────────────────────────────────────────

/// Builds the `HxRefList` from all store data.
fn build_ref_list(store: &HwpxStyleStore) -> HxRefList {
    let fontfaces = build_fontfaces(store);
    let char_properties = build_char_properties(store);
    let para_properties = build_para_properties(store);
    let styles = build_styles(store);

    HxRefList {
        fontfaces: if fontfaces.groups.is_empty() { None } else { Some(fontfaces) },
        char_properties: if char_properties.items.is_empty() {
            None
        } else {
            Some(char_properties)
        },
        para_properties: if para_properties.items.is_empty() {
            None
        } else {
            Some(para_properties)
        },
        styles: if styles.items.is_empty() { None } else { Some(styles) },
    }
}

// ── Font builders ───────────────────────────────────────────────

/// Groups fonts by language and builds `HxFontFaces`.
///
/// The decoder flattens all fonts into a single list ordered by store
/// index. The encoder re-groups them by language, preserving insertion
/// order via a simple scan (no external dependency needed).
fn build_fontfaces(store: &HwpxStyleStore) -> HxFontFaces {
    let groups = group_fonts_by_lang(store);
    let item_cnt = groups.len() as u32;
    HxFontFaces { item_cnt, groups }
}

// NOTE: Re-groups the flat font list by language tag for HWPX output.
// This reverses the decoder's flattening. The round-trip is correct
// because 한글 mirrors identical fonts across all language groups.
// See decoder/header.rs convert_char_pr() for the full ASSUMPTION note.
// TODO(v2.0): With per-group font model, this re-grouping becomes unnecessary.
/// Re-groups the store's flat font list by language tag.
///
/// Uses a `Vec`-based ordered map to keep deterministic output without
/// adding `indexmap` as a dependency. Languages appear in the order
/// their first font was encountered.
fn group_fonts_by_lang(store: &HwpxStyleStore) -> Vec<HxFontFaceGroup> {
    // Collect (lang, fonts) pairs preserving first-seen order.
    let mut langs: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<&HwpxFont>> = Vec::new();

    for font in store.iter_fonts() {
        if let Some(pos) = langs.iter().position(|l| l == &font.lang) {
            groups[pos].push(font);
        } else {
            langs.push(font.lang.clone());
            groups.push(vec![font]);
        }
    }

    langs
        .into_iter()
        .zip(groups)
        .map(|(lang, fonts)| {
            let font_cnt = fonts.len() as u32;
            let hx_fonts: Vec<HxFont> = fonts
                .into_iter()
                .map(|f| HxFont {
                    id: f.id,
                    face: f.face_name.clone(),
                    font_type: "TTF".into(),
                    is_embedded: 0,
                })
                .collect();
            HxFontFaceGroup { lang, font_cnt, fonts: hx_fonts }
        })
        .collect()
}

// ── CharPr builder ──────────────────────────────────────────────

/// Builds the `HxCharProperties` list from all char shapes in the store.
fn build_char_properties(store: &HwpxStyleStore) -> HxCharProperties {
    let items: Vec<HxCharPr> = store
        .iter_char_shapes()
        .enumerate()
        .map(|(idx, cs)| build_char_pr(idx as u32, cs))
        .collect();
    let item_cnt = items.len() as u32;
    HxCharProperties { item_cnt, items }
}

/// Converts a single `HwpxCharShape` back to `HxCharPr`.
///
/// This is the reverse of `decoder::header::convert_char_pr`.
///
/// Emits all required child elements including `ratio` (100), `spacing` (0),
/// `relSz` (100), and `offset` (0) for all 7 language groups, which 한글
/// expects to be present in every `<hh:charPr>`.
fn build_char_pr(id: u32, cs: &HwpxCharShape) -> HxCharPr {
    let fr = &cs.font_ref;
    HxCharPr {
        id,
        height: cs.height.as_i32() as u32,
        text_color: color_to_hex(&cs.text_color),
        shade_color: shade_color_to_str(&cs.shade_color),
        use_font_space: 0,
        use_kerning: 0,
        sym_mark: "NONE".into(),
        border_fill_id_ref: 2,

        font_ref: Some(HxFontRef {
            hangul: fr.hangul.get() as u32,
            latin: fr.latin.get() as u32,
            hanja: fr.hanja.get() as u32,
            japanese: fr.japanese.get() as u32,
            other: fr.other.get() as u32,
            symbol: fr.symbol.get() as u32,
            user: fr.user.get() as u32,
        }),
        ratio: Some(lang_values_all(100)),
        spacing: Some(lang_values_all(0)),
        rel_sz: Some(lang_values_all(100)),
        offset: Some(lang_values_all(0)),
        bold: if cs.bold { Some(HxPresence) } else { None },
        italic: if cs.italic { Some(HxPresence) } else { None },
        underline: Some(HxUnderline {
            underline_type: cs.underline_type.clone(),
            shape: "SOLID".into(),
            color: "#000000".into(),
        }),
        strikeout: Some(HxStrikeout { shape: cs.strikeout_shape.clone(), color: "#000000".into() }),
        outline: Some(HxOutline { outline_type: "NONE".into() }),
        shadow: Some(HxShadow {
            shadow_type: "NONE".into(),
            color: "#B2B2B2".into(),
            offset_x: 10,
            offset_y: 10,
        }),
    }
}

/// Creates an `HxLangValues` with the same value for all 7 language fields.
fn lang_values_all(v: i32) -> HxLangValues {
    HxLangValues { hangul: v, latin: v, hanja: v, japanese: v, other: v, symbol: v, user: v }
}

// ── ParaPr builder ──────────────────────────────────────────────

/// Builds the `HxParaProperties` list from all para shapes in the store.
fn build_para_properties(store: &HwpxStyleStore) -> HxParaProperties {
    let items: Vec<HxParaPr> = store
        .iter_para_shapes()
        .enumerate()
        .map(|(idx, ps)| build_para_pr(idx as u32, ps))
        .collect();
    let item_cnt = items.len() as u32;
    HxParaProperties { item_cnt, items }
}

/// Converts a single `HwpxParaShape` back to `HxParaPr`.
///
/// This is the reverse of `decoder::header::convert_para_pr`.
///
/// Emits all child elements expected by 한글: heading (NONE default),
/// breakSetting, autoSpacing, margin/lineSpacing (inside hp:switch),
/// and border (referencing borderFill id=2).
fn build_para_pr(id: u32, ps: &HwpxParaShape) -> HxParaPr {
    HxParaPr {
        id,
        tab_pr_id_ref: 0,
        condense: 0,
        align: Some(HxAlign {
            horizontal: alignment_to_str(ps.alignment).into(),
            vertical: "BASELINE".into(),
        }),
        heading: Some(HxHeading { heading_type: "NONE".into(), id_ref: 0, level: 0 }),
        break_setting: Some(HxBreakSetting {
            break_latin_word: "KEEP_WORD".into(),
            break_non_latin_word: "BREAK_WORD".into(),
            widow_orphan: 0,
            keep_with_next: 0,
            keep_lines: 0,
            page_break_before: 0,
        }),
        auto_spacing: Some(HxAutoSpacing { e_asian_eng: 0, e_asian_num: 0 }),
        switch: Some(build_margin_switch(ps)),
        border: Some(HxBorder {
            border_fill_id_ref: 2,
            offset_left: 0,
            offset_right: 0,
            offset_top: 0,
            offset_bottom: 0,
        }),
    }
}

/// Builds the `<hp:switch>` block with both `<hp:case>` and `<hp:default>`.
///
/// Both branches carry identical margin and line-spacing values, which is
/// the standard pattern emitted by the 한글 word processor.
fn build_margin_switch(ps: &HwpxParaShape) -> HxSwitch {
    HxSwitch {
        case: Some(HxSwitchCase {
            required_namespace: "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar".into(),
            margin: Some(build_margin(ps)),
            line_spacing: Some(build_line_spacing(ps)),
        }),
        default: Some(HxSwitchDefault {
            margin: Some(build_margin(ps)),
            line_spacing: Some(build_line_spacing(ps)),
        }),
    }
}

/// Builds an `HxMargin` from a para shape's margin fields.
fn build_margin(ps: &HwpxParaShape) -> HxMargin {
    HxMargin {
        indent: Some(hwpunit_value(ps.indent)),
        left: Some(hwpunit_value(ps.margin_left)),
        right: Some(hwpunit_value(ps.margin_right)),
        prev: Some(hwpunit_value(ps.spacing_before)),
        next: Some(hwpunit_value(ps.spacing_after)),
    }
}

/// Builds an `HxLineSpacing` from a para shape's line-spacing fields.
fn build_line_spacing(ps: &HwpxParaShape) -> HxLineSpacing {
    HxLineSpacing {
        spacing_type: ps.line_spacing_type.clone(),
        value: ps.line_spacing as u32,
        unit: "HWPUNIT".into(),
    }
}

/// Creates an `HxUnitValue` from an `HwpUnit`.
fn hwpunit_value(u: HwpUnit) -> HxUnitValue {
    HxUnitValue { value: u.as_i32(), unit: "HWPUNIT".into() }
}

// ── Color / alignment helpers ───────────────────────────────────

/// Formats a [`Color`] as `"#RRGGBB"`.
fn color_to_hex(c: &Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue())
}

/// Converts a shade color to its HWPX string representation.
///
/// Black shading is represented as `"none"` in HWPX (meaning no shading),
/// while any other color uses the standard `"#RRGGBB"` format.
fn shade_color_to_str(c: &Color) -> String {
    if *c == Color::BLACK {
        "none".to_string()
    } else {
        color_to_hex(c)
    }
}

/// Converts an [`Alignment`] to the HWPX uppercase string.
fn alignment_to_str(a: Alignment) -> &'static str {
    match a {
        Alignment::Left => "LEFT",
        Alignment::Center => "CENTER",
        Alignment::Right => "RIGHT",
        Alignment::Justify => "JUSTIFY",
        // non_exhaustive: default to LEFT for future variants
        _ => "LEFT",
    }
}

// ── Style builders ──────────────────────────────────────────────

/// Builds the `HxStyles` list from all styles in the store.
fn build_styles(store: &HwpxStyleStore) -> HxStyles {
    let items: Vec<HxStyle> = store.iter_styles().map(build_style).collect();
    let item_cnt = items.len() as u32;
    HxStyles { item_cnt, items }
}

/// Converts a single `HwpxStyle` back to `HxStyle`.
fn build_style(s: &HwpxStyle) -> HxStyle {
    HxStyle {
        id: s.id,
        style_type: s.style_type.clone(),
        name: s.name.clone(),
        eng_name: s.eng_name.clone(),
        para_pr_id_ref: s.para_pr_id_ref,
        char_pr_id_ref: s.char_pr_id_ref,
        next_style_id_ref: s.next_style_id_ref,
        lang_id: s.lang_id,
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{CharShapeIndex, FontIndex, ParaShapeIndex};

    // ── Helper: build a minimal store ───────────────────────────

    /// Creates a store with 1 HANGUL font, 1 char shape, 1 para shape.
    fn minimal_store() -> HwpxStyleStore {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_char_shape(HwpxCharShape {
            font_ref: crate::style_store::HwpxFontRef::default(),
            height: HwpUnit::new(1000).unwrap(),
            text_color: Color::BLACK,
            shade_color: Color::BLACK,
            bold: false,
            italic: false,
            underline_type: "NONE".into(),
            strikeout_shape: "NONE".into(),
        });
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Left,
            margin_left: HwpUnit::ZERO,
            margin_right: HwpUnit::ZERO,
            indent: HwpUnit::ZERO,
            spacing_before: HwpUnit::ZERO,
            spacing_after: HwpUnit::ZERO,
            line_spacing: 160,
            line_spacing_type: "PERCENT".into(),
        });
        store
    }

    // ── 1. Minimal store encode ─────────────────────────────────

    #[test]
    fn test_encode_minimal_store() {
        let store = minimal_store();
        let xml = encode_header(&store, 1).unwrap();

        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#));
        assert!(xml.contains("<hh:head"));
        assert!(xml.contains("</hh:head>"));
        assert!(xml.contains(r#"version="1.4""#));
        assert!(xml.contains(r#"secCnt="1""#));
        // Font
        assert!(xml.contains("함초롬돋움"));
        assert!(xml.contains(r#"lang="HANGUL""#));
        // Char shape
        assert!(xml.contains(r#"height="1000""#));
        assert!(xml.contains(r##"textColor="#000000""##));
        // Para shape
        assert!(xml.contains(r#"horizontal="LEFT""#));
        assert!(xml.contains(r#"vertical="BASELINE""#));
    }

    // ── 2. Encode-decode roundtrip ──────────────────────────────

    #[test]
    fn test_encode_header_roundtrip() {
        let store = minimal_store();
        let xml = encode_header(&store, 1).unwrap();

        // The decoder strips namespace prefixes, so we need to feed it
        // XML without them. However, the decoder's parse_header uses
        // quick-xml::de which strips namespace prefixes automatically.
        let decoded = crate::decoder::header::parse_header(&xml).unwrap();

        // Font roundtrip
        assert_eq!(decoded.font_count(), store.font_count());
        let f = decoded.font(FontIndex::new(0)).unwrap();
        assert_eq!(f.face_name, "함초롬돋움");
        assert_eq!(f.lang, "HANGUL");

        // Char shape roundtrip
        assert_eq!(decoded.char_shape_count(), store.char_shape_count());
        let cs = decoded.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert_eq!(cs.text_color, Color::BLACK);
        assert!(!cs.bold);
        assert!(!cs.italic);

        // Para shape roundtrip
        assert_eq!(decoded.para_shape_count(), store.para_shape_count());
        let ps = decoded.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, "PERCENT");
    }

    // ── 3. Bold/italic presence ─────────────────────────────────

    #[test]
    fn test_bold_italic_presence() {
        let mut store = HwpxStyleStore::new();
        store.push_char_shape(HwpxCharShape { bold: true, italic: false, ..Default::default() });
        let xml = encode_header(&store, 1).unwrap();

        assert!(xml.contains("<hh:bold"), "bold element must be present");
        assert!(!xml.contains("<hh:italic"), "italic element must be absent");
    }

    // ── 4. color_to_hex ─────────────────────────────────────────

    #[test]
    fn test_color_to_hex() {
        assert_eq!(color_to_hex(&Color::from_rgb(255, 0, 0)), "#FF0000");
        assert_eq!(color_to_hex(&Color::BLACK), "#000000");
        assert_eq!(color_to_hex(&Color::WHITE), "#FFFFFF");
        assert_eq!(color_to_hex(&Color::from_rgb(0xAB, 0xCD, 0xEF)), "#ABCDEF");
    }

    // ── 5. shade_color_to_str ───────────────────────────────────

    #[test]
    fn test_shade_color_none() {
        assert_eq!(shade_color_to_str(&Color::BLACK), "none");
        assert_eq!(shade_color_to_str(&Color::from_rgb(0, 255, 0)), "#00FF00");
        assert_eq!(shade_color_to_str(&Color::WHITE), "#FFFFFF");
    }

    // ── 6. alignment_to_str ─────────────────────────────────────

    #[test]
    fn test_alignment_to_str() {
        assert_eq!(alignment_to_str(Alignment::Left), "LEFT");
        assert_eq!(alignment_to_str(Alignment::Center), "CENTER");
        assert_eq!(alignment_to_str(Alignment::Right), "RIGHT");
        assert_eq!(alignment_to_str(Alignment::Justify), "JUSTIFY");
    }

    // ── 7. Font grouping ────────────────────────────────────────

    #[test]
    fn test_font_grouping() {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont {
            id: 1, face_name: "함초롬바탕".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont { id: 0, face_name: "Arial".into(), lang: "LATIN".into() });

        let groups = group_fonts_by_lang(&store);
        assert_eq!(groups.len(), 2);

        // First group: HANGUL with 2 fonts
        assert_eq!(groups[0].lang, "HANGUL");
        assert_eq!(groups[0].font_cnt, 2);
        assert_eq!(groups[0].fonts.len(), 2);
        assert_eq!(groups[0].fonts[0].face, "함초롬돋움");
        assert_eq!(groups[0].fonts[1].face, "함초롬바탕");

        // Second group: LATIN with 1 font
        assert_eq!(groups[1].lang, "LATIN");
        assert_eq!(groups[1].font_cnt, 1);
        assert_eq!(groups[1].fonts[0].face, "Arial");
    }

    // ── 8. Margin switch structure ──────────────────────────────

    #[test]
    fn test_margin_switch_structure() {
        let ps = HwpxParaShape {
            alignment: Alignment::Justify,
            margin_left: HwpUnit::new(100).unwrap(),
            margin_right: HwpUnit::new(50).unwrap(),
            indent: HwpUnit::new(200).unwrap(),
            spacing_before: HwpUnit::new(300).unwrap(),
            spacing_after: HwpUnit::new(150).unwrap(),
            line_spacing: 200,
            line_spacing_type: "PERCENT".into(),
        };

        let switch = build_margin_switch(&ps);

        // Case must be present
        let case = switch.case.as_ref().expect("case must be present");
        assert_eq!(case.required_namespace, "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar");
        let case_margin = case.margin.as_ref().unwrap();
        assert_eq!(case_margin.left.as_ref().unwrap().value, 100);
        assert_eq!(case_margin.right.as_ref().unwrap().value, 50);
        assert_eq!(case_margin.indent.as_ref().unwrap().value, 200);
        assert_eq!(case_margin.prev.as_ref().unwrap().value, 300);
        assert_eq!(case_margin.next.as_ref().unwrap().value, 150);
        let case_ls = case.line_spacing.as_ref().unwrap();
        assert_eq!(case_ls.value, 200);
        assert_eq!(case_ls.spacing_type, "PERCENT");

        // Default must be present with identical values
        let default = switch.default.as_ref().expect("default must be present");
        let def_margin = default.margin.as_ref().unwrap();
        assert_eq!(def_margin.left.as_ref().unwrap().value, 100);
        assert_eq!(def_margin.indent.as_ref().unwrap().value, 200);
        let def_ls = default.line_spacing.as_ref().unwrap();
        assert_eq!(def_ls.value, 200);
    }

    // ── 9. Empty store ──────────────────────────────────────────

    #[test]
    fn test_empty_store() {
        let store = HwpxStyleStore::new();
        let xml = encode_header(&store, 0).unwrap();

        assert!(xml.contains("<hh:head"));
        assert!(xml.contains(r#"secCnt="0""#));
        // refList should be empty (all fields None → skip_serializing_if)
        // The serialized HxRefList with all None fields should produce
        // an empty element or just the wrapper.
        assert!(xml.contains("</hh:head>"));
    }

    // ── 10. Roundtrip with rich data ────────────────────────────

    #[test]
    fn test_roundtrip_rich_data() {
        let mut store = HwpxStyleStore::new();

        // 2 HANGUL fonts + 1 LATIN font
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont {
            id: 1, face_name: "함초롬바탕".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont {
            id: 0,
            face_name: "Times New Roman".into(),
            lang: "LATIN".into(),
        });

        // Bold + colored char shape
        store.push_char_shape(HwpxCharShape {
            font_ref: crate::style_store::HwpxFontRef {
                hangul: FontIndex::new(1),
                latin: FontIndex::new(2),
                ..Default::default()
            },
            height: HwpUnit::new(2500).unwrap(),
            text_color: Color::from_rgb(255, 0, 0),
            shade_color: Color::from_rgb(0, 255, 0),
            bold: true,
            italic: true,
            underline_type: "BOTTOM".into(),
            strikeout_shape: "SLASH".into(),
        });

        // Justified para with margins
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Justify,
            margin_left: HwpUnit::new(100).unwrap(),
            margin_right: HwpUnit::new(50).unwrap(),
            indent: HwpUnit::new(200).unwrap(),
            spacing_before: HwpUnit::new(300).unwrap(),
            spacing_after: HwpUnit::new(150).unwrap(),
            line_spacing: 200,
            line_spacing_type: "PERCENT".into(),
        });

        let xml = encode_header(&store, 1).unwrap();
        let decoded = crate::decoder::header::parse_header(&xml).unwrap();

        // Fonts
        assert_eq!(decoded.font_count(), 3);
        assert_eq!(decoded.font(FontIndex::new(0)).unwrap().face_name, "함초롬돋움");
        assert_eq!(decoded.font(FontIndex::new(1)).unwrap().face_name, "함초롬바탕");
        assert_eq!(decoded.font(FontIndex::new(2)).unwrap().face_name, "Times New Roman");

        // Char shape
        let cs = decoded.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 2500);
        assert_eq!(cs.text_color, Color::from_rgb(255, 0, 0));
        assert_eq!(cs.shade_color, Color::from_rgb(0, 255, 0));
        assert!(cs.bold);
        assert!(cs.italic);
        assert_eq!(cs.font_ref.hangul.get(), 1);
        assert_eq!(cs.font_ref.latin.get(), 2);
        assert_eq!(cs.underline_type, "BOTTOM");
        assert_eq!(cs.strikeout_shape, "SLASH");

        // Para shape
        let ps = decoded.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.margin_left.as_i32(), 100);
        assert_eq!(ps.margin_right.as_i32(), 50);
        assert_eq!(ps.indent.as_i32(), 200);
        assert_eq!(ps.spacing_before.as_i32(), 300);
        assert_eq!(ps.spacing_after.as_i32(), 150);
        assert_eq!(ps.line_spacing, 200);
    }

    // ── 11. sec_cnt propagation ─────────────────────────────────

    #[test]
    fn test_sec_cnt_in_output() {
        let store = HwpxStyleStore::new();
        let xml = encode_header(&store, 42).unwrap();
        assert!(xml.contains(r#"secCnt="42""#));
    }

    // ── 12. Multiple char shapes get sequential IDs ─────────────

    #[test]
    fn test_multiple_char_shapes_ids() {
        let mut store = HwpxStyleStore::new();
        store.push_char_shape(HwpxCharShape { bold: true, ..Default::default() });
        store.push_char_shape(HwpxCharShape { italic: true, ..Default::default() });
        store.push_char_shape(HwpxCharShape::default());

        let xml = encode_header(&store, 1).unwrap();
        let decoded = crate::decoder::header::parse_header(&xml).unwrap();

        assert_eq!(decoded.char_shape_count(), 3);
        assert!(decoded.char_shape(CharShapeIndex::new(0)).unwrap().bold);
        assert!(decoded.char_shape(CharShapeIndex::new(1)).unwrap().italic);
        assert!(!decoded.char_shape(CharShapeIndex::new(2)).unwrap().bold);
        assert!(!decoded.char_shape(CharShapeIndex::new(2)).unwrap().italic);
    }

    // ── 13. Styles roundtrip ────────────────────────────────────

    #[test]
    fn test_styles_roundtrip() {
        let mut store = HwpxStyleStore::new();
        store.push_style(crate::style_store::HwpxStyle {
            id: 0,
            style_type: "PARA".into(),
            name: "바탕글".into(),
            eng_name: "Normal".into(),
            para_pr_id_ref: 0,
            char_pr_id_ref: 0,
            next_style_id_ref: 0,
            lang_id: 1042,
        });
        store.push_style(crate::style_store::HwpxStyle {
            id: 1,
            style_type: "CHAR".into(),
            name: "본문".into(),
            eng_name: "Body".into(),
            para_pr_id_ref: 1,
            char_pr_id_ref: 1,
            next_style_id_ref: 1,
            lang_id: 1042,
        });

        let xml = encode_header(&store, 1).unwrap();
        assert!(xml.contains("바탕글"));
        assert!(xml.contains("Normal"));
        assert!(xml.contains("본문"));
        assert!(xml.contains("Body"));

        let decoded = crate::decoder::header::parse_header(&xml).unwrap();
        assert_eq!(decoded.style_count(), 2);

        let s0 = decoded.style(0).unwrap();
        assert_eq!(s0.name, "바탕글");
        assert_eq!(s0.eng_name, "Normal");
        assert_eq!(s0.style_type, "PARA");
        assert_eq!(s0.lang_id, 1042);

        let s1 = decoded.style(1).unwrap();
        assert_eq!(s1.name, "본문");
        assert_eq!(s1.eng_name, "Body");
        assert_eq!(s1.style_type, "CHAR");
    }

    #[test]
    fn test_empty_styles_not_serialized() {
        let store = HwpxStyleStore::new();
        let xml = encode_header(&store, 1).unwrap();
        // Styles should not appear in XML when empty
        assert!(!xml.contains("<hh:styles"));
    }

    // ── 14. Verify all 6 encoder improvements ──────────────────

    #[test]
    fn test_encoder_improvements_all_present() {
        let store = minimal_store();
        let xml = encode_header(&store, 1).unwrap();

        // Gap 1: charPr ratio/spacing/relSz/offset
        assert!(xml.contains("<hh:ratio hangul=\"100\""), "charPr must have ratio");
        assert!(xml.contains("<hh:spacing hangul=\"0\""), "charPr must have spacing");
        assert!(xml.contains("<hh:relSz hangul=\"100\""), "charPr must have relSz");
        assert!(xml.contains("<hh:offset hangul=\"0\""), "charPr must have offset");

        // Gap 2: paraPr heading/breakSetting/autoSpacing/border
        assert!(xml.contains("<hh:heading type=\"NONE\""), "paraPr must have heading");
        assert!(
            xml.contains("<hh:breakSetting breakLatinWord=\"KEEP_WORD\""),
            "paraPr must have breakSetting"
        );
        assert!(xml.contains("<hh:autoSpacing eAsianEng=\"0\""), "paraPr must have autoSpacing");
        assert!(xml.contains("<hh:border borderFillIDRef=\"2\""), "paraPr must have border");

        // Gap 3: borderFill id=2 fillBrush
        assert!(xml.contains("<hh:fillBrush>"), "borderFill id=2 must have fillBrush");
        assert!(xml.contains("<hc:winBrush faceColor=\"none\""), "fillBrush must have winBrush");
        assert!(xml.contains("Crooked=\"0\""), "slash/backSlash must have Crooked attr");
        assert!(xml.contains("isCounter=\"0\""), "slash/backSlash must have isCounter attr");

        // Gap 4: tabProperties 2nd entry
        assert!(
            xml.contains("<hh:tabProperties itemCnt=\"2\""),
            "tabProperties must have 2 entries"
        );
        assert!(xml.contains("<hh:tabPr id=\"1\" autoTabLeft=\"1\""), "tabPr id=1 must exist");

        // Gap 5: numberings
        assert!(xml.contains("<hh:numberings itemCnt=\"1\""), "numberings must exist");
        assert!(xml.contains("<hh:paraHead level=\"1\""), "numbering must have paraHead levels");
        assert!(xml.contains("numFormat=\"DIGIT\""), "paraHead must have numFormat");
        assert!(
            xml.contains("charPrIDRef=\"4294967295\""),
            "paraHead must use u32::MAX for no override"
        );
    }
}
