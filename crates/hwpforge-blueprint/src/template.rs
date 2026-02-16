//! Template definition and YAML parsing.
//!
//! A **Template** is a YAML-serializable container for:
//! - Metadata (name, version, parent template reference)
//! - Page settings (dimensions, margins)
//! - Style definitions (character and paragraph styles)
//! - Markdown-to-style mappings
//!
//! Templates support inheritance via the `extends` field, allowing
//! templates to build upon each other like CSS cascading.

use std::collections::BTreeMap;

use hwpforge_core::PageSettings;
use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{BlueprintError, BlueprintResult};
use crate::serde_helpers::{de_dim_opt, ser_dim_opt};
use crate::style::PartialStyle;

// ---------------------------------------------------------------------------
// TemplateMeta
// ---------------------------------------------------------------------------

/// Template metadata (name, version, parent reference).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TemplateMeta {
    /// Template name (required).
    pub name: String,

    /// Template version (defaults to "1.0.0").
    #[serde(default = "default_version")]
    pub version: String,

    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Parent template name for inheritance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

// ---------------------------------------------------------------------------
// PageStyle
// ---------------------------------------------------------------------------

/// Page dimensions and margins with optional fields (for YAML).
///
/// After parsing, use [`PageStyle::to_page_settings()`] to convert to
/// a fully-resolved [`PageSettings`] with defaults.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PageStyle {
    /// Page width.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<HwpUnit>,

    /// Page height.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub height: Option<HwpUnit>,

    /// Top margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub margin_top: Option<HwpUnit>,

    /// Bottom margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub margin_bottom: Option<HwpUnit>,

    /// Left margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub margin_left: Option<HwpUnit>,

    /// Right margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub margin_right: Option<HwpUnit>,

    /// Header margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub header_margin: Option<HwpUnit>,

    /// Footer margin.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub footer_margin: Option<HwpUnit>,
}

impl PageStyle {
    /// A4 page settings for templates.
    pub fn a4() -> Self {
        let a4 = PageSettings::a4();
        Self {
            width: Some(a4.width),
            height: Some(a4.height),
            margin_top: Some(a4.margin_top),
            margin_bottom: Some(a4.margin_bottom),
            margin_left: Some(a4.margin_left),
            margin_right: Some(a4.margin_right),
            header_margin: Some(a4.header_margin),
            footer_margin: Some(a4.footer_margin),
        }
    }

    /// Converts to [`PageSettings`], using A4 defaults for `None` fields.
    pub fn to_page_settings(&self) -> PageSettings {
        let a4 = PageSettings::a4();
        PageSettings {
            width: self.width.unwrap_or(a4.width),
            height: self.height.unwrap_or(a4.height),
            margin_top: self.margin_top.unwrap_or(a4.margin_top),
            margin_bottom: self.margin_bottom.unwrap_or(a4.margin_bottom),
            margin_left: self.margin_left.unwrap_or(a4.margin_left),
            margin_right: self.margin_right.unwrap_or(a4.margin_right),
            header_margin: self.header_margin.unwrap_or(a4.header_margin),
            footer_margin: self.footer_margin.unwrap_or(a4.footer_margin),
        }
    }
}

// ---------------------------------------------------------------------------
// MarkdownMapping
// ---------------------------------------------------------------------------

/// Maps markdown elements to style names.
///
/// Example YAML:
/// ```yaml
/// markdown_mapping:
///   body: body_style
///   heading1: h1_style
///   code: code_style
/// ```
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct MarkdownMapping {
    /// Body text style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Heading 1 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading1: Option<String>,

    /// Heading 2 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading2: Option<String>,

    /// Heading 3 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading3: Option<String>,

    /// Heading 4 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading4: Option<String>,

    /// Heading 5 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading5: Option<String>,

    /// Heading 6 style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading6: Option<String>,

    /// Code block style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Blockquote style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockquote: Option<String>,

    /// List item style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_item: Option<String>,
}

// ---------------------------------------------------------------------------
// Template
// ---------------------------------------------------------------------------

/// A complete YAML template.
///
/// Contains metadata, page settings, style definitions, and markdown mappings.
///
/// # Example YAML
///
/// ```yaml
/// meta:
///   name: government_proposal
///   version: 1.0.0
///   description: Korean government proposal template
///
/// page:
///   width: 210mm
///   height: 297mm
///   margin_top: 20mm
///   margin_bottom: 20mm
///   margin_left: 20mm
///   margin_right: 20mm
///
/// styles:
///   body:
///     char_shape:
///       font: 한컴바탕
///       size: 10pt
///     para_shape:
///       alignment: Left
///
/// markdown_mapping:
///   body: body
///   heading1: h1
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Template {
    /// Template metadata.
    pub meta: TemplateMeta,

    /// Page settings (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageStyle>,

    /// Style definitions (name → PartialStyle).
    #[serde(default)]
    pub styles: BTreeMap<String, PartialStyle>,

    /// Markdown element mappings (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown_mapping: Option<MarkdownMapping>,
}

impl Template {
    /// Parses a template from YAML string.
    ///
    /// # Errors
    ///
    /// Returns [`BlueprintError::YamlParse`] if the YAML is invalid.
    pub fn from_yaml(yaml: &str) -> BlueprintResult<Self> {
        serde_yaml::from_str(yaml).map_err(|e| BlueprintError::YamlParse { message: e.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{Alignment, Color};
    use pretty_assertions::assert_eq;

    use crate::style::{PartialCharShape, PartialParaShape};

    // -----------------------------------------------------------------------
    // TemplateMeta
    // -----------------------------------------------------------------------

    #[test]
    fn template_meta_from_yaml_minimal() {
        let yaml = "name: test_template";
        let meta: TemplateMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.name, "test_template");
        assert_eq!(meta.version, "1.0.0");
        assert!(meta.description.is_none());
        assert!(meta.extends.is_none());
    }

    #[test]
    fn template_meta_from_yaml_full() {
        let yaml = r#"
name: child_template
version: 2.0.0
description: Child template for testing
extends: parent_template
"#;
        let meta: TemplateMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.name, "child_template");
        assert_eq!(meta.version, "2.0.0");
        assert_eq!(meta.description, Some("Child template for testing".to_string()));
        assert_eq!(meta.extends, Some("parent_template".to_string()));
    }

    #[test]
    fn template_meta_with_extends() {
        let yaml = r#"
name: derived
extends: base
"#;
        let meta: TemplateMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.extends, Some("base".to_string()));
    }

    #[test]
    fn template_meta_without_extends() {
        let yaml = "name: standalone";
        let meta: TemplateMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.extends.is_none());
    }

    // -----------------------------------------------------------------------
    // PageStyle
    // -----------------------------------------------------------------------

    #[test]
    fn page_style_default_is_all_none() {
        let ps = PageStyle::default();
        assert!(ps.width.is_none());
        assert!(ps.height.is_none());
        assert!(ps.margin_top.is_none());
        assert!(ps.margin_bottom.is_none());
        assert!(ps.margin_left.is_none());
        assert!(ps.margin_right.is_none());
        assert!(ps.header_margin.is_none());
        assert!(ps.footer_margin.is_none());
    }

    #[test]
    fn page_style_a4_constructor() {
        let ps = PageStyle::a4();
        assert!(ps.width.is_some());
        assert!(ps.height.is_some());
        let settings = ps.to_page_settings();
        assert!((settings.width.to_mm() - 210.0).abs() < 0.1);
        assert!((settings.height.to_mm() - 297.0).abs() < 0.1);
    }

    #[test]
    fn page_style_to_page_settings_uses_a4_defaults() {
        let ps = PageStyle { width: Some(HwpUnit::from_mm(100.0).unwrap()), ..Default::default() };
        let settings = ps.to_page_settings();
        assert_eq!(settings.width, HwpUnit::from_mm(100.0).unwrap());
        // Other fields should be A4 defaults
        assert!((settings.height.to_mm() - 297.0).abs() < 0.1);
        assert!((settings.margin_top.to_mm() - 20.0).abs() < 0.1);
    }

    #[test]
    fn page_style_from_yaml() {
        let yaml = r#"
width: 210mm
height: 297mm
margin_top: 25mm
margin_bottom: 25mm
margin_left: 30mm
margin_right: 30mm
header_margin: 15mm
footer_margin: 15mm
"#;
        let ps: PageStyle = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ps.width, Some(HwpUnit::from_mm(210.0).unwrap()));
        assert_eq!(ps.height, Some(HwpUnit::from_mm(297.0).unwrap()));
        assert_eq!(ps.margin_top, Some(HwpUnit::from_mm(25.0).unwrap()));
        assert_eq!(ps.header_margin, Some(HwpUnit::from_mm(15.0).unwrap()));
    }

    #[test]
    fn page_style_partial_yaml() {
        let yaml = "width: 100mm\nheight: 200mm";
        let ps: PageStyle = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ps.width, Some(HwpUnit::from_mm(100.0).unwrap()));
        assert_eq!(ps.height, Some(HwpUnit::from_mm(200.0).unwrap()));
        assert!(ps.margin_top.is_none());
    }

    // -----------------------------------------------------------------------
    // MarkdownMapping
    // -----------------------------------------------------------------------

    #[test]
    fn markdown_mapping_default_is_all_none() {
        let mm = MarkdownMapping::default();
        assert!(mm.body.is_none());
        assert!(mm.heading1.is_none());
        assert!(mm.heading2.is_none());
        assert!(mm.heading3.is_none());
        assert!(mm.heading4.is_none());
        assert!(mm.heading5.is_none());
        assert!(mm.heading6.is_none());
        assert!(mm.code.is_none());
        assert!(mm.blockquote.is_none());
        assert!(mm.list_item.is_none());
    }

    #[test]
    fn markdown_mapping_from_yaml() {
        let yaml = r#"
body: body_style
heading1: h1_style
heading2: h2_style
code: code_style
blockquote: quote_style
list_item: list_style
"#;
        let mm: MarkdownMapping = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(mm.body, Some("body_style".to_string()));
        assert_eq!(mm.heading1, Some("h1_style".to_string()));
        assert_eq!(mm.heading2, Some("h2_style".to_string()));
        assert_eq!(mm.code, Some("code_style".to_string()));
        assert_eq!(mm.blockquote, Some("quote_style".to_string()));
        assert_eq!(mm.list_item, Some("list_style".to_string()));
        assert!(mm.heading3.is_none());
    }

    #[test]
    fn markdown_mapping_partial_yaml() {
        let yaml = "body: body\nheading1: h1";
        let mm: MarkdownMapping = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(mm.body, Some("body".to_string()));
        assert_eq!(mm.heading1, Some("h1".to_string()));
        assert!(mm.code.is_none());
    }

    // -----------------------------------------------------------------------
    // Template
    // -----------------------------------------------------------------------

    #[test]
    fn template_from_yaml_minimal() {
        let yaml = r#"
meta:
  name: minimal_template
"#;
        let tmpl = Template::from_yaml(yaml).unwrap();
        assert_eq!(tmpl.meta.name, "minimal_template");
        assert_eq!(tmpl.meta.version, "1.0.0");
        assert!(tmpl.page.is_none());
        assert!(tmpl.styles.is_empty());
        assert!(tmpl.markdown_mapping.is_none());
    }

    #[test]
    fn template_from_yaml_full() {
        let yaml = r#"
meta:
  name: full_template
  version: 2.0.0
  description: Full template example

page:
  width: 210mm
  height: 297mm
  margin_top: 20mm
  margin_bottom: 20mm
  margin_left: 20mm
  margin_right: 20mm

styles:
  body:
    char_shape:
      font: 한컴바탕
      size: 10pt
      color: '#000000'
    para_shape:
      alignment: Left
  heading1:
    char_shape:
      font: 한컴바탕
      size: 16pt
      bold: true

markdown_mapping:
  body: body
  heading1: heading1
"#;
        let tmpl = Template::from_yaml(yaml).unwrap();
        assert_eq!(tmpl.meta.name, "full_template");
        assert_eq!(tmpl.meta.version, "2.0.0");
        assert!(tmpl.page.is_some());
        assert_eq!(tmpl.styles.len(), 2);

        let body_style = tmpl.styles.get("body").unwrap();
        assert_eq!(body_style.char_shape.as_ref().unwrap().font, Some("한컴바탕".to_string()));
        assert_eq!(
            body_style.char_shape.as_ref().unwrap().size,
            Some(HwpUnit::from_pt(10.0).unwrap())
        );
        assert_eq!(body_style.para_shape.as_ref().unwrap().alignment, Some(Alignment::Left));

        let h1_style = tmpl.styles.get("heading1").unwrap();
        assert_eq!(h1_style.char_shape.as_ref().unwrap().bold, Some(true));

        let mapping = tmpl.markdown_mapping.as_ref().unwrap();
        assert_eq!(mapping.body, Some("body".to_string()));
        assert_eq!(mapping.heading1, Some("heading1".to_string()));
    }

    #[test]
    fn template_from_yaml_with_extends() {
        let yaml = r#"
meta:
  name: child
  extends: parent

styles:
  custom:
    char_shape:
      font: Arial
      size: 12pt
"#;
        let tmpl = Template::from_yaml(yaml).unwrap();
        assert_eq!(tmpl.meta.name, "child");
        assert_eq!(tmpl.meta.extends, Some("parent".to_string()));
        assert_eq!(tmpl.styles.len(), 1);
    }

    #[test]
    fn template_from_yaml_invalid_yaml_error() {
        let yaml = "meta:\n  name: [invalid";
        let err = Template::from_yaml(yaml).unwrap_err();
        assert!(matches!(err, BlueprintError::YamlParse { .. }));
        assert!(err.to_string().contains("YAML parse error"));
    }

    #[test]
    fn template_sorts_style_keys_alphabetically() {
        let yaml = r#"
meta:
  name: ordered

styles:
  z_style:
    char_shape:
      font: A
      size: 10pt
  a_style:
    char_shape:
      font: B
      size: 12pt
  m_style:
    char_shape:
      font: C
      size: 14pt
"#;
        let tmpl = Template::from_yaml(yaml).unwrap();
        let keys: Vec<&String> = tmpl.styles.keys().collect();
        // BTreeMap sorts keys alphabetically
        assert_eq!(keys, vec!["a_style", "m_style", "z_style"]);
    }

    #[test]
    fn template_serde_roundtrip() {
        let mut styles = BTreeMap::new();
        styles.insert(
            "body".to_string(),
            PartialStyle {
                char_shape: Some(PartialCharShape {
                    font: Some("한컴바탕".to_string()),
                    size: Some(HwpUnit::from_pt(10.0).unwrap()),
                    color: Some(Color::BLACK),
                    ..Default::default()
                }),
                para_shape: Some(PartialParaShape {
                    alignment: Some(Alignment::Justify),
                    ..Default::default()
                }),
            },
        );

        let original = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None, // Skip page to avoid floating-point rounding
            styles,
            markdown_mapping: Some(MarkdownMapping {
                body: Some("body".to_string()),
                ..Default::default()
            }),
        };

        let yaml = serde_yaml::to_string(&original).unwrap();
        let roundtripped = Template::from_yaml(&yaml).unwrap();
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn template_empty_styles_is_valid() {
        let yaml = r#"
meta:
  name: empty_styles
styles: {}
"#;
        let tmpl = Template::from_yaml(yaml).unwrap();
        assert!(tmpl.styles.is_empty());
    }

    #[test]
    fn page_style_serde_skips_none_fields() {
        let ps = PageStyle {
            width: Some(HwpUnit::from_mm(210.0).unwrap()),
            height: Some(HwpUnit::from_mm(297.0).unwrap()),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&ps).unwrap();
        assert!(yaml.contains("width"));
        assert!(yaml.contains("height"));
        assert!(!yaml.contains("margin_top"));
        assert!(!yaml.contains("header_margin"));
    }
}
