//! Parses `Contents/header.xml` into an [`HwpxStyleStore`].
//!
//! Converts XML schema types (`HxCharPr`, `HxParaPr`, `HxFont`) into
//! Foundation types (`Color`, `HwpUnit`, `Alignment`) for the store.

use hwpforge_foundation::{FontIndex, HwpUnit};
use quick_xml::de::from_str;

use crate::error::{HwpxError, HwpxResult};
use crate::schema::header::{HxCharPr, HxHead, HxParaPr};
use crate::style_store::{
    parse_alignment, parse_hex_color, HwpxCharShape, HwpxFont, HwpxFontRef,
    HwpxParaShape, HwpxStyleStore,
};

/// Parses a `header.xml` string into an [`HwpxStyleStore`].
///
/// Extracts:
/// - Font face definitions → `Vec<HwpxFont>`
/// - Character properties → `Vec<HwpxCharShape>`
/// - Paragraph properties → `Vec<HwpxParaShape>`
///
/// # Security
///
/// XML entity expansion attacks (Billion Laughs) are not a concern here:
/// quick-xml's serde deserializer does not expand custom entities and will
/// return an error if any are encountered. The ZIP size limits in
/// `PackageReader` also bound the total input size.
pub fn parse_header(xml: &str) -> HwpxResult<HwpxStyleStore> {
    let head: HxHead = from_str(xml).map_err(|e| HwpxError::XmlParse {
        file: "header.xml".into(),
        detail: e.to_string(),
    })?;

    let mut store = HwpxStyleStore::new();

    if let Some(ref_list) = &head.ref_list {
        // ── Fonts ────────────────────────────────────────────
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

        // ── Character Shapes ─────────────────────────────────
        if let Some(char_props) = &ref_list.char_properties {
            for cp in &char_props.items {
                store.push_char_shape(convert_char_pr(cp));
            }
        }

        // ── Paragraph Shapes ─────────────────────────────────
        if let Some(para_props) = &ref_list.para_properties {
            for pp in &para_props.items {
                store.push_para_shape(convert_para_pr(pp));
            }
        }
    }

    Ok(store)
}

/// Converts an `HxCharPr` XML type into an `HwpxCharShape`.
fn convert_char_pr(cp: &HxCharPr) -> HwpxCharShape {
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
    let height = i32::try_from(cp.height)
        .ok()
        .and_then(|h| HwpUnit::new(h).ok())
        .unwrap_or(HwpUnit::ZERO);

    HwpxCharShape {
        font_ref,
        height,
        text_color: parse_hex_color(&cp.text_color),
        shade_color: parse_hex_color(&cp.shade_color),
        bold: cp.bold.is_some(),
        italic: cp.italic.is_some(),
        underline_type: cp
            .underline
            .as_ref()
            .map(|u| u.underline_type.clone())
            .unwrap_or_else(|| "NONE".into()),
        strikeout_shape: cp
            .strikeout
            .as_ref()
            .map(|s| s.shape.clone())
            .unwrap_or_else(|| "NONE".into()),
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
    let (margin_left, margin_right, indent, spacing_before, spacing_after) =
        extract_margins(pp);

    let (line_spacing, line_spacing_type) = extract_line_spacing(pp);

    HwpxParaShape {
        alignment,
        margin_left,
        margin_right,
        indent,
        spacing_before,
        spacing_after,
        line_spacing,
        line_spacing_type,
    }
}

/// Extracts margin values from the switch/default block.
fn extract_margins(pp: &HxParaPr) -> (HwpUnit, HwpUnit, HwpUnit, HwpUnit, HwpUnit) {
    let z = HwpUnit::ZERO;
    let Some(switch) = &pp.switch else {
        return (z, z, z, z, z);
    };
    let Some(default) = &switch.default else {
        return (z, z, z, z, z);
    };
    let Some(margin) = &default.margin else {
        return (z, z, z, z, z);
    };

    let to_unit = |opt: &Option<crate::schema::header::HxUnitValue>| -> HwpUnit {
        opt.as_ref()
            .and_then(|v| HwpUnit::new(v.value).ok())
            .unwrap_or(z)
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
fn extract_line_spacing(pp: &HxParaPr) -> (i32, String) {
    let default_ls = (160, "PERCENT".into());
    let Some(switch) = &pp.switch else {
        return default_ls;
    };
    let Some(default) = &switch.default else {
        return default_ls;
    };
    let Some(ls) = &default.line_spacing else {
        return default_ls;
    };

    let spacing_type = if ls.spacing_type.is_empty() {
        "PERCENT".to_string()
    } else {
        ls.spacing_type.clone()
    };

    // Saturate to i32::MAX if value exceeds range (extremely rare in real HWPX files)
    let value = ls.value.min(i32::MAX as u32) as i32;
    (value, spacing_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{Alignment, Color};

    // ── Minimal header ───────────────────────────────────────────

    #[test]
    fn parse_empty_header() {
        let xml = r#"<head version="1.4" secCnt="1"></head>"#;
        let store = parse_header(xml).unwrap();
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
        let store = parse_header(xml).unwrap();
        assert_eq!(store.font_count(), 2);

        let f0 = store.font(FontIndex::new(0)).unwrap();
        assert_eq!(f0.face_name, "함초롬돋움");
        assert_eq!(f0.lang, "HANGUL");

        let f1 = store.font(FontIndex::new(1)).unwrap();
        assert_eq!(f1.face_name, "Times New Roman");
        assert_eq!(f1.lang, "LATIN");
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
        let store = parse_header(xml).unwrap();
        assert_eq!(store.char_shape_count(), 1);

        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert_eq!(cs.text_color, Color::BLACK);
        assert!(!cs.bold);
        assert!(!cs.italic);
        assert_eq!(cs.underline_type, "NONE");
        assert_eq!(cs.strikeout_shape, "NONE");
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
        let store = parse_header(xml).unwrap();
        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 2500);
        assert_eq!(cs.text_color, Color::from_rgb(255, 0, 0));
        assert_eq!(cs.shade_color, Color::from_rgb(0, 255, 0));
        assert!(cs.bold);
        assert!(cs.italic);
        assert_eq!(cs.font_ref.hangul.get(), 1);
        assert_eq!(cs.font_ref.latin.get(), 2);
        assert_eq!(cs.underline_type, "BOTTOM");
        assert_eq!(cs.strikeout_shape, "SLASH");
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
        let store = parse_header(xml).unwrap();
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
        let store = parse_header(xml).unwrap();
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.indent.as_i32(), 200);
        assert_eq!(ps.margin_left.as_i32(), 100);
        assert_eq!(ps.margin_right.as_i32(), 50);
        assert_eq!(ps.spacing_before.as_i32(), 300);
        assert_eq!(ps.spacing_after.as_i32(), 150);
        assert_eq!(ps.line_spacing, 200);
        assert_eq!(ps.line_spacing_type, "PERCENT");
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
        let store = parse_header(xml).unwrap();
        let ps = store.para_shape(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.margin_left, HwpUnit::ZERO);
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, "PERCENT");
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

        let store = parse_header(xml).unwrap();
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
        let store = parse_header(xml).unwrap();
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
        let store = parse_header(xml).unwrap();
        let cs = store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.font_ref.hangul.get(), 0);
        assert_eq!(cs.font_ref.latin.get(), 0);
    }
}
