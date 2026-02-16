//! XML schema types for `header.xml` (hh: namespace).
//!
//! Maps the `<hh:head>` element tree into Rust structs via serde.
//! All types use `#[serde(default)]` on optional sub-elements for
//! forward compatibility with different 한글 versions.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::Deserialize;

// ── Root ──────────────────────────────────────────────────────────

/// `<hh:head version="1.4" secCnt="1">`.
#[derive(Debug, Deserialize)]
#[serde(rename = "head")]
pub struct HxHead {
    #[serde(rename = "@version", default)]
    pub version: String,
    #[serde(rename = "@secCnt", default)]
    pub sec_cnt: u32,
    #[serde(rename = "refList", default)]
    pub ref_list: Option<HxRefList>,
}

// ── RefList ───────────────────────────────────────────────────────

/// `<hh:refList>` — container for all shared definitions.
#[derive(Debug, Deserialize, Default)]
pub struct HxRefList {
    #[serde(rename = "fontfaces", default)]
    pub fontfaces: Option<HxFontFaces>,
    #[serde(rename = "charProperties", default)]
    pub char_properties: Option<HxCharProperties>,
    #[serde(rename = "paraProperties", default)]
    pub para_properties: Option<HxParaProperties>,
    #[serde(rename = "styles", default)]
    pub styles: Option<HxStyles>,
    // borderFills, tabProperties, numberings — skipped (Phase 3)
}

// ── Fonts ─────────────────────────────────────────────────────────

/// `<hh:fontfaces itemCnt="7">`.
#[derive(Debug, Deserialize, Default)]
pub struct HxFontFaces {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(rename = "fontface", default)]
    pub groups: Vec<HxFontFaceGroup>,
}

/// `<hh:fontface lang="HANGUL" fontCnt="2">`.
#[derive(Debug, Deserialize)]
pub struct HxFontFaceGroup {
    #[serde(rename = "@lang", default)]
    pub lang: String,
    #[serde(rename = "@fontCnt", default)]
    pub font_cnt: u32,
    #[serde(rename = "font", default)]
    pub fonts: Vec<HxFont>,
}

/// `<hh:font id="0" face="함초롬돋움" type="TTF" isEmbedded="0">`.
#[derive(Debug, Deserialize)]
pub struct HxFont {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@face", default)]
    pub face: String,
    #[serde(rename = "@type", default)]
    pub font_type: String,
    #[serde(rename = "@isEmbedded", default)]
    pub is_embedded: u32,
}

// ── Character Properties ──────────────────────────────────────────

/// `<hh:charProperties itemCnt="8">`.
#[derive(Debug, Deserialize, Default)]
pub struct HxCharProperties {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(rename = "charPr", default)]
    pub items: Vec<HxCharPr>,
}

/// `<hh:charPr id="0" height="1000" textColor="#000000" ...>`.
///
/// Attributes: id, height, textColor, shadeColor, useFontSpace,
///     useKerning, symMark, borderFillIDRef.
/// Children: fontRef, ratio, spacing, relSz, offset, underline,
///     strikeout, outline, shadow, (optional) bold, italic.
#[derive(Debug, Deserialize)]
pub struct HxCharPr {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@height", default)]
    pub height: u32,
    #[serde(rename = "@textColor", default)]
    pub text_color: String,
    #[serde(rename = "@shadeColor", default)]
    pub shade_color: String,
    #[serde(rename = "@useFontSpace", default)]
    pub use_font_space: u32,
    #[serde(rename = "@useKerning", default)]
    pub use_kerning: u32,
    #[serde(rename = "@symMark", default)]
    pub sym_mark: String,
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id_ref: u32,

    // ── child elements ──
    #[serde(rename = "fontRef", default)]
    pub font_ref: Option<HxFontRef>,
    #[serde(rename = "bold", default)]
    pub bold: Option<HxPresence>,
    #[serde(rename = "italic", default)]
    pub italic: Option<HxPresence>,
    #[serde(rename = "underline", default)]
    pub underline: Option<HxUnderline>,
    #[serde(rename = "strikeout", default)]
    pub strikeout: Option<HxStrikeout>,
    #[serde(rename = "outline", default)]
    pub outline: Option<HxOutline>,
    #[serde(rename = "shadow", default)]
    pub shadow: Option<HxShadow>,
    // ratio, spacing, relSz, offset — ignored (Phase 3)
}

/// Per-language font index references.
/// `<hh:fontRef hangul="1" latin="1" hanja="1" .../>`.
#[derive(Debug, Deserialize, Default)]
pub struct HxFontRef {
    #[serde(rename = "@hangul", default)]
    pub hangul: u32,
    #[serde(rename = "@latin", default)]
    pub latin: u32,
    #[serde(rename = "@hanja", default)]
    pub hanja: u32,
    #[serde(rename = "@japanese", default)]
    pub japanese: u32,
    #[serde(rename = "@other", default)]
    pub other: u32,
    #[serde(rename = "@symbol", default)]
    pub symbol: u32,
    #[serde(rename = "@user", default)]
    pub user: u32,
}

/// Marker for presence-based boolean elements (`<hh:bold/>`, `<hh:italic/>`).
///
/// The element's mere presence means `true`; absence means `false`.
#[derive(Debug, Deserialize)]
pub struct HxPresence;

/// `<hh:underline type="NONE" shape="SOLID" color="#000000"/>`.
#[derive(Debug, Deserialize)]
pub struct HxUnderline {
    #[serde(rename = "@type", default)]
    pub underline_type: String,
    #[serde(rename = "@shape", default)]
    pub shape: String,
    #[serde(rename = "@color", default)]
    pub color: String,
}

/// `<hh:strikeout shape="NONE" color="#000000"/>`.
#[derive(Debug, Deserialize)]
pub struct HxStrikeout {
    #[serde(rename = "@shape", default)]
    pub shape: String,
    #[serde(rename = "@color", default)]
    pub color: String,
}

/// `<hh:outline type="NONE"/>`.
#[derive(Debug, Deserialize)]
pub struct HxOutline {
    #[serde(rename = "@type", default)]
    pub outline_type: String,
}

/// `<hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>`.
#[derive(Debug, Deserialize)]
pub struct HxShadow {
    #[serde(rename = "@type", default)]
    pub shadow_type: String,
    #[serde(rename = "@color", default)]
    pub color: String,
    #[serde(rename = "@offsetX", default)]
    pub offset_x: i32,
    #[serde(rename = "@offsetY", default)]
    pub offset_y: i32,
}

// ── Paragraph Properties ──────────────────────────────────────────

/// `<hh:paraProperties itemCnt="16">`.
#[derive(Debug, Deserialize, Default)]
pub struct HxParaProperties {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(rename = "paraPr", default)]
    pub items: Vec<HxParaPr>,
}

/// `<hh:paraPr id="0" tabPrIDRef="0" condense="0" ...>`.
#[derive(Debug, Deserialize)]
pub struct HxParaPr {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@tabPrIDRef", default)]
    pub tab_pr_id_ref: u32,
    #[serde(rename = "@condense", default)]
    pub condense: u32,

    // ── child elements ──
    #[serde(rename = "align", default)]
    pub align: Option<HxAlign>,
    #[serde(rename = "heading", default)]
    pub heading: Option<HxHeading>,
    /// The `<hp:switch>` element wrapping `<hp:default>` with margin
    /// and lineSpacing values.
    #[serde(rename = "switch", default)]
    pub switch: Option<HxSwitch>,
    // breakSetting, autoSpacing, border — ignored (Phase 3 uses defaults)
}

/// `<hh:align horizontal="LEFT" vertical="BASELINE"/>`.
#[derive(Debug, Deserialize)]
pub struct HxAlign {
    #[serde(rename = "@horizontal", default)]
    pub horizontal: String,
    #[serde(rename = "@vertical", default)]
    pub vertical: String,
}

/// `<hh:heading type="NONE" idRef="0" level="0"/>`.
#[derive(Debug, Deserialize)]
pub struct HxHeading {
    #[serde(rename = "@type", default)]
    pub heading_type: String,
    #[serde(rename = "@idRef", default)]
    pub id_ref: u32,
    #[serde(rename = "@level", default)]
    pub level: u32,
}

// ── hp:switch / hp:case / hp:default ──────────────────────────────

/// `<hp:switch>` — version-specific rendering container.
///
/// Phase 3 reads only the `<hp:default>` block for maximum
/// compatibility across 한글 versions.
#[derive(Debug, Deserialize)]
pub struct HxSwitch {
    #[serde(rename = "case", default)]
    pub case: Option<HxSwitchCase>,
    #[serde(rename = "default", default)]
    pub default: Option<HxSwitchDefault>,
}

/// `<hp:case hp:required-namespace="...">`.
#[derive(Debug, Deserialize)]
pub struct HxSwitchCase {
    #[serde(rename = "@required-namespace", default)]
    pub required_namespace: String,
    #[serde(rename = "margin", default)]
    pub margin: Option<HxMargin>,
    #[serde(rename = "lineSpacing", default)]
    pub line_spacing: Option<HxLineSpacing>,
}

/// `<hp:default>` — fallback values for older viewers.
#[derive(Debug, Deserialize)]
pub struct HxSwitchDefault {
    #[serde(rename = "margin", default)]
    pub margin: Option<HxMargin>,
    #[serde(rename = "lineSpacing", default)]
    pub line_spacing: Option<HxLineSpacing>,
}

/// `<hh:margin>` containing `<hc:intent>`, `<hc:left>`, etc.
#[derive(Debug, Deserialize, Default)]
pub struct HxMargin {
    /// NB: HWPX uses `<hc:intent>` (typo in the spec; should be "indent").
    #[serde(rename = "intent", default)]
    pub indent: Option<HxUnitValue>,
    #[serde(rename = "left", default)]
    pub left: Option<HxUnitValue>,
    #[serde(rename = "right", default)]
    pub right: Option<HxUnitValue>,
    #[serde(rename = "prev", default)]
    pub prev: Option<HxUnitValue>,
    #[serde(rename = "next", default)]
    pub next: Option<HxUnitValue>,
}

/// `<hc:left value="0" unit="HWPUNIT"/>` — generic value+unit pair.
#[derive(Debug, Deserialize)]
pub struct HxUnitValue {
    #[serde(rename = "@value", default)]
    pub value: i32,
    #[serde(rename = "@unit", default)]
    pub unit: String,
}

/// `<hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>`.
#[derive(Debug, Deserialize)]
pub struct HxLineSpacing {
    #[serde(rename = "@type", default)]
    pub spacing_type: String,
    #[serde(rename = "@value", default)]
    pub value: u32,
    #[serde(rename = "@unit", default)]
    pub unit: String,
}

// ── Styles ────────────────────────────────────────────────────────

/// `<hh:styles itemCnt="18">`.
#[derive(Debug, Deserialize, Default)]
pub struct HxStyles {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(rename = "style", default)]
    pub items: Vec<HxStyle>,
}

/// `<hh:style id="0" type="PARA" name="바탕글" engName="Normal" ...>`.
#[derive(Debug, Deserialize)]
pub struct HxStyle {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@type", default)]
    pub style_type: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@engName", default)]
    pub eng_name: String,
    #[serde(rename = "@paraPrIDRef", default)]
    pub para_pr_id_ref: u32,
    #[serde(rename = "@charPrIDRef", default)]
    pub char_pr_id_ref: u32,
    #[serde(rename = "@nextStyleIDRef", default)]
    pub next_style_id_ref: u32,
    #[serde(rename = "@langID", default)]
    pub lang_id: u32,
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_head(xml: &str) -> HxHead {
        quick_xml::de::from_str(xml).expect("failed to parse HxHead")
    }

    #[test]
    fn parse_minimal_head() {
        let xml = r#"<hh:head version="1.4" secCnt="1"></hh:head>"#;
        let head = parse_head(xml);
        assert_eq!(head.version, "1.4");
        assert_eq!(head.sec_cnt, 1);
        assert!(head.ref_list.is_none());
    }

    #[test]
    fn parse_fontfaces() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:fontfaces itemCnt="1">
              <hh:fontface lang="HANGUL" fontCnt="1">
                <hh:font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
              </hh:fontface>
            </hh:fontfaces>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let ff = head.ref_list.unwrap().fontfaces.unwrap();
        assert_eq!(ff.groups.len(), 1);
        assert_eq!(ff.groups[0].lang, "HANGUL");
        assert_eq!(ff.groups[0].fonts[0].face, "함초롬돋움");
    }

    #[test]
    fn parse_char_pr_basic() {
        let xml = r##"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:charProperties itemCnt="1">
              <hh:charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                         useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="2">
                <hh:fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                <hh:underline type="NONE" shape="SOLID" color="#000000"/>
                <hh:strikeout shape="NONE" color="#000000"/>
                <hh:outline type="NONE"/>
                <hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>
              </hh:charPr>
            </hh:charProperties>
          </hh:refList>
        </hh:head>"##;
        let head = parse_head(xml);
        let cp = &head.ref_list.unwrap().char_properties.unwrap().items[0];
        assert_eq!(cp.id, 0);
        assert_eq!(cp.height, 1000);
        assert_eq!(cp.text_color, "#000000");
        assert_eq!(cp.shade_color, "none");
        let fr = cp.font_ref.as_ref().unwrap();
        assert_eq!(fr.hangul, 1);
        assert_eq!(fr.latin, 1);
        assert!(cp.bold.is_none());
        assert!(cp.italic.is_none());
    }

    #[test]
    fn parse_char_pr_with_italic() {
        let xml = r##"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:charProperties itemCnt="1">
              <hh:charPr id="7" height="1000" textColor="#FF0000" shadeColor="none"
                         useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="2">
                <hh:fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                <hh:italic/>
                <hh:underline type="NONE" shape="SOLID" color="#000000"/>
                <hh:strikeout shape="NONE" color="#000000"/>
                <hh:outline type="NONE"/>
                <hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>
              </hh:charPr>
            </hh:charProperties>
          </hh:refList>
        </hh:head>"##;
        let head = parse_head(xml);
        let cp = &head.ref_list.unwrap().char_properties.unwrap().items[0];
        assert_eq!(cp.id, 7);
        assert_eq!(cp.text_color, "#FF0000");
        assert!(cp.italic.is_some(), "italic should be present");
        assert!(cp.bold.is_none());
    }

    #[test]
    fn parse_para_pr_with_switch() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:paraProperties itemCnt="1">
              <hh:paraPr id="0" tabPrIDRef="0" condense="0">
                <hh:align horizontal="LEFT" vertical="BASELINE"/>
                <hp:switch>
                  <hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="0" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>
                  </hp:case>
                  <hp:default>
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="0" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>
                  </hp:default>
                </hp:switch>
              </hh:paraPr>
            </hh:paraProperties>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let pp = &head.ref_list.unwrap().para_properties.unwrap().items[0];
        assert_eq!(pp.id, 0);
        let align = pp.align.as_ref().unwrap();
        assert_eq!(align.horizontal, "LEFT");
        let sw = pp.switch.as_ref().unwrap();
        let def = sw.default.as_ref().unwrap();
        let margin = def.margin.as_ref().unwrap();
        assert_eq!(margin.left.as_ref().unwrap().value, 0);
        let ls = def.line_spacing.as_ref().unwrap();
        assert_eq!(ls.spacing_type, "PERCENT");
        assert_eq!(ls.value, 130);
    }

    #[test]
    fn parse_para_pr_with_non_zero_margin() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:paraProperties itemCnt="1">
              <hh:paraPr id="4" tabPrIDRef="1" condense="20">
                <hh:align horizontal="JUSTIFY" vertical="BASELINE"/>
                <hp:switch>
                  <hp:default>
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="14000" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/>
                  </hp:default>
                </hp:switch>
              </hh:paraPr>
            </hh:paraProperties>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let pp = &head.ref_list.unwrap().para_properties.unwrap().items[0];
        let def = pp.switch.as_ref().unwrap().default.as_ref().unwrap();
        let margin = def.margin.as_ref().unwrap();
        assert_eq!(margin.left.as_ref().unwrap().value, 14000);
        assert_eq!(def.line_spacing.as_ref().unwrap().value, 160);
    }

    #[test]
    fn parse_styles() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:styles itemCnt="2">
              <hh:style id="0" type="PARA" name="바탕글" engName="Normal"
                        paraPrIDRef="3" charPrIDRef="0" nextStyleIDRef="0" langID="1042"/>
              <hh:style id="1" type="PARA" name="본문" engName="Body"
                        paraPrIDRef="11" charPrIDRef="0" nextStyleIDRef="1" langID="1042"/>
            </hh:styles>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let styles = head.ref_list.unwrap().styles.unwrap();
        assert_eq!(styles.items.len(), 2);
        assert_eq!(styles.items[0].name, "바탕글");
        assert_eq!(styles.items[0].eng_name, "Normal");
        assert_eq!(styles.items[0].para_pr_id_ref, 3);
        assert_eq!(styles.items[0].char_pr_id_ref, 0);
        assert_eq!(styles.items[1].name, "본문");
    }

    #[test]
    fn empty_ref_list_no_panic() {
        let xml = r#"<hh:head version="1.4" secCnt="1"><hh:refList/></hh:head>"#;
        let head = parse_head(xml);
        let rl = head.ref_list.unwrap();
        assert!(rl.fontfaces.is_none());
        assert!(rl.char_properties.is_none());
        assert!(rl.para_properties.is_none());
        assert!(rl.styles.is_none());
    }

    #[test]
    fn unknown_elements_are_silently_skipped() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:borderFills itemCnt="0"/>
            <hh:tabProperties itemCnt="0"/>
            <hh:numberings itemCnt="0"/>
            <hh:charProperties itemCnt="0"/>
            <hh:paraProperties itemCnt="0"/>
          </hh:refList>
          <hh:compatibleDocument targetProgram="HWP201X"/>
          <hh:docOption/>
          <hh:trackchageConfig flags="56"/>
        </hh:head>"#;
        let head = parse_head(xml);
        assert!(head.ref_list.is_some());
    }

    #[test]
    fn font_ref_defaults_to_zero() {
        let fr = HxFontRef::default();
        assert_eq!(fr.hangul, 0);
        assert_eq!(fr.latin, 0);
        assert_eq!(fr.user, 0);
    }
}
