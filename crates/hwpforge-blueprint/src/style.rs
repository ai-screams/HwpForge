//! Style types: character shapes, paragraph shapes, and their partial variants.
//!
//! The **two-type pattern** is central to Blueprint's design:
//!
//! - [`PartialCharShape`] / [`PartialParaShape`] — all fields `Option`,
//!   used for YAML deserialization and inheritance merging.
//! - [`CharShape`] / [`ParaShape`] — all fields required, produced after
//!   inheritance resolution when every field has a concrete value.
//!
//! This mirrors CSS inheritance: a child template can override only the
//! fields it cares about, inheriting the rest from the parent.

use hwpforge_foundation::{Alignment, Color, HwpUnit, LineSpacingType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{BlueprintError, BlueprintResult};
use crate::serde_helpers::{
    de_color, de_color_opt, de_dim, de_dim_opt, de_pct_opt, ser_color, ser_color_opt, ser_dim,
    ser_dim_opt, ser_pct_opt,
};

// ---------------------------------------------------------------------------
// Helper types
// ---------------------------------------------------------------------------

/// Vertical spacing (before and after a paragraph).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Spacing {
    /// Space before the paragraph.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub before: Option<HwpUnit>,
    /// Space after the paragraph.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub after: Option<HwpUnit>,
}

/// Paragraph indentation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Indent {
    /// Left indentation.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub left: Option<HwpUnit>,
    /// Right indentation.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub right: Option<HwpUnit>,
    /// First-line indentation (can be negative for hanging indent).
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub first_line: Option<HwpUnit>,
}

/// Line spacing configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineSpacing {
    /// Spacing type (percentage, fixed, between-lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_type: Option<LineSpacingType>,
    /// The value: percentage (e.g. 160.0 for 160%) or fixed HwpUnit.
    #[serde(
        default,
        serialize_with = "ser_pct_opt",
        deserialize_with = "de_pct_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<f64>,
}

// ---------------------------------------------------------------------------
// Partial types (for YAML and inheritance merging)
// ---------------------------------------------------------------------------

/// Character shape with all optional fields (for YAML parsing and inheritance).
///
/// After inheritance resolution, this is converted to [`CharShape`] where
/// all fields are guaranteed to be present.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PartialCharShape {
    /// Font name (e.g. "한컴바탕", "Arial").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Font size.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub size: Option<HwpUnit>,
    /// Bold weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Italic style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Underline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Strikethrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// Text color in `#RRGGBB`.
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub color: Option<Color>,
    /// Superscript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superscript: Option<bool>,
    /// Subscript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscript: Option<bool>,
}

impl PartialCharShape {
    /// Merges `other` into `self` (child overrides parent).
    /// Fields in `other` with `Some` value override `self`.
    pub fn merge(&mut self, other: &PartialCharShape) {
        if other.font.is_some() {
            self.font.clone_from(&other.font);
        }
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strikethrough.is_some() {
            self.strikethrough = other.strikethrough;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.superscript.is_some() {
            self.superscript = other.superscript;
        }
        if other.subscript.is_some() {
            self.subscript = other.subscript;
        }
    }

    /// Attempts to resolve this partial into a fully-specified [`CharShape`].
    ///
    /// Returns an error naming the first missing required field.
    pub fn resolve(&self, style_name: &str) -> BlueprintResult<CharShape> {
        Ok(CharShape {
            font: self.font.clone().ok_or_else(|| BlueprintError::StyleResolution {
                style_name: style_name.to_string(),
                field: "font".to_string(),
            })?,
            size: self.size.ok_or_else(|| BlueprintError::StyleResolution {
                style_name: style_name.to_string(),
                field: "size".to_string(),
            })?,
            bold: self.bold.unwrap_or(false),
            italic: self.italic.unwrap_or(false),
            underline: self.underline.unwrap_or(false),
            strikethrough: self.strikethrough.unwrap_or(false),
            color: self.color.unwrap_or(Color::BLACK),
            superscript: self.superscript.unwrap_or(false),
            subscript: self.subscript.unwrap_or(false),
        })
    }
}

/// Paragraph shape with all optional fields (for YAML parsing and inheritance).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PartialParaShape {
    /// Text alignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
    /// Line spacing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<LineSpacing>,
    /// Vertical spacing (before/after paragraph).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Spacing>,
    /// Indentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<Indent>,
}

impl Spacing {
    /// Merges `other` into `self` (child fields override parent fields).
    pub fn merge(&mut self, other: &Spacing) {
        if other.before.is_some() {
            self.before = other.before;
        }
        if other.after.is_some() {
            self.after = other.after;
        }
    }
}

impl Indent {
    /// Merges `other` into `self` (child fields override parent fields).
    pub fn merge(&mut self, other: &Indent) {
        if other.left.is_some() {
            self.left = other.left;
        }
        if other.right.is_some() {
            self.right = other.right;
        }
        if other.first_line.is_some() {
            self.first_line = other.first_line;
        }
    }
}

impl LineSpacing {
    /// Merges `other` into `self` (child fields override parent fields).
    pub fn merge(&mut self, other: &LineSpacing) {
        if other.spacing_type.is_some() {
            self.spacing_type = other.spacing_type;
        }
        if other.value.is_some() {
            self.value = other.value;
        }
    }
}

impl PartialParaShape {
    /// Merges `other` into `self` (child overrides parent, field-level deep merge).
    ///
    /// Nested structs (line_spacing, spacing, indent) are merged at the field
    /// level, not replaced wholesale. This means a child can override
    /// `spacing.after` while inheriting `spacing.before` from the parent.
    pub fn merge(&mut self, other: &PartialParaShape) {
        if other.alignment.is_some() {
            self.alignment = other.alignment;
        }
        // Deep merge: merge nested struct fields individually
        match (&mut self.line_spacing, &other.line_spacing) {
            (Some(base), Some(child)) => base.merge(child),
            (None, Some(child)) => self.line_spacing = Some(*child),
            _ => {}
        }
        match (&mut self.spacing, &other.spacing) {
            (Some(base), Some(child)) => base.merge(child),
            (None, Some(child)) => self.spacing = Some(*child),
            _ => {}
        }
        match (&mut self.indent, &other.indent) {
            (Some(base), Some(child)) => base.merge(child),
            (None, Some(child)) => self.indent = Some(*child),
            _ => {}
        }
    }

    /// Resolves into a fully-specified [`ParaShape`] with defaults.
    pub fn resolve(&self) -> ParaShape {
        ParaShape {
            alignment: self.alignment.unwrap_or(Alignment::Left),
            line_spacing_type: self
                .line_spacing
                .and_then(|ls| ls.spacing_type)
                .unwrap_or(LineSpacingType::Percentage),
            line_spacing_value: self.line_spacing.and_then(|ls| ls.value).unwrap_or(160.0),
            space_before: self.spacing.and_then(|s| s.before).unwrap_or(HwpUnit::ZERO),
            space_after: self.spacing.and_then(|s| s.after).unwrap_or(HwpUnit::ZERO),
            indent_left: self.indent.and_then(|i| i.left).unwrap_or(HwpUnit::ZERO),
            indent_right: self.indent.and_then(|i| i.right).unwrap_or(HwpUnit::ZERO),
            indent_first_line: self.indent.and_then(|i| i.first_line).unwrap_or(HwpUnit::ZERO),
        }
    }
}

/// A composite style entry (char + para shape) with optional fields for YAML.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PartialStyle {
    /// Character formatting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_shape: Option<PartialCharShape>,
    /// Paragraph formatting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para_shape: Option<PartialParaShape>,
}

impl PartialStyle {
    /// Merges `other` into `self`.
    pub fn merge(&mut self, other: &PartialStyle) {
        match (&mut self.char_shape, &other.char_shape) {
            (Some(base), Some(child)) => base.merge(child),
            (None, Some(child)) => self.char_shape = Some(child.clone()),
            _ => {}
        }
        match (&mut self.para_shape, &other.para_shape) {
            (Some(base), Some(child)) => base.merge(child),
            (None, Some(child)) => self.para_shape = Some(child.clone()),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Resolved (full) types
// ---------------------------------------------------------------------------

/// A fully-resolved character shape (all fields present).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharShape {
    /// Font name.
    pub font: String,
    /// Font size.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub size: HwpUnit,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strikethrough.
    pub strikethrough: bool,
    /// Text color.
    #[serde(serialize_with = "ser_color", deserialize_with = "de_color")]
    pub color: Color,
    /// Superscript.
    pub superscript: bool,
    /// Subscript.
    pub subscript: bool,
}

/// A fully-resolved paragraph shape (all fields present).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParaShape {
    /// Text alignment.
    pub alignment: Alignment,
    /// Line spacing type.
    pub line_spacing_type: LineSpacingType,
    /// Line spacing value (percentage or fixed).
    pub line_spacing_value: f64,
    /// Space before paragraph.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub space_before: HwpUnit,
    /// Space after paragraph.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub space_after: HwpUnit,
    /// Left indentation.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub indent_left: HwpUnit,
    /// Right indentation.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub indent_right: HwpUnit,
    /// First-line indentation.
    #[serde(serialize_with = "ser_dim", deserialize_with = "de_dim")]
    pub indent_first_line: HwpUnit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn partial_char_shape_default_is_all_none() {
        let p = PartialCharShape::default();
        assert!(p.font.is_none());
        assert!(p.size.is_none());
        assert!(p.bold.is_none());
        assert!(p.italic.is_none());
        assert!(p.underline.is_none());
        assert!(p.strikethrough.is_none());
        assert!(p.color.is_none());
        assert!(p.superscript.is_none());
        assert!(p.subscript.is_none());
    }

    #[test]
    fn partial_char_shape_merge_overrides() {
        let mut base = PartialCharShape {
            font: Some("Arial".into()),
            size: Some(HwpUnit::from_pt(10.0).unwrap()),
            bold: Some(false),
            ..Default::default()
        };
        let child = PartialCharShape {
            size: Some(HwpUnit::from_pt(16.0).unwrap()),
            bold: Some(true),
            ..Default::default()
        };
        base.merge(&child);
        assert_eq!(base.font, Some("Arial".into()));
        assert_eq!(base.size, Some(HwpUnit::from_pt(16.0).unwrap()));
        assert_eq!(base.bold, Some(true));
    }

    #[test]
    fn partial_char_shape_merge_none_does_not_override() {
        let mut base = PartialCharShape { font: Some("Batang".into()), ..Default::default() };
        let child = PartialCharShape::default();
        base.merge(&child);
        assert_eq!(base.font, Some("Batang".into()));
    }

    #[test]
    fn partial_char_shape_resolve_success() {
        let partial = PartialCharShape {
            font: Some("한컴바탕".into()),
            size: Some(HwpUnit::from_pt(10.0).unwrap()),
            ..Default::default()
        };
        let resolved = partial.resolve("body").unwrap();
        assert_eq!(resolved.font, "한컴바탕");
        assert_eq!(resolved.size, HwpUnit::from_pt(10.0).unwrap());
        assert!(!resolved.bold);
        assert_eq!(resolved.color, Color::BLACK);
    }

    #[test]
    fn partial_char_shape_resolve_missing_font() {
        let partial =
            PartialCharShape { size: Some(HwpUnit::from_pt(10.0).unwrap()), ..Default::default() };
        let err = partial.resolve("heading1").unwrap_err();
        assert!(err.to_string().contains("font"));
        assert!(err.to_string().contains("heading1"));
    }

    #[test]
    fn partial_char_shape_resolve_missing_size() {
        let partial = PartialCharShape { font: Some("Arial".into()), ..Default::default() };
        let err = partial.resolve("body").unwrap_err();
        assert!(err.to_string().contains("size"));
    }

    #[test]
    fn partial_para_shape_default_is_all_none() {
        let p = PartialParaShape::default();
        assert!(p.alignment.is_none());
        assert!(p.line_spacing.is_none());
        assert!(p.spacing.is_none());
        assert!(p.indent.is_none());
    }

    #[test]
    fn partial_para_shape_merge_overrides() {
        let mut base = PartialParaShape {
            alignment: Some(Alignment::Left),
            line_spacing: Some(LineSpacing {
                spacing_type: Some(LineSpacingType::Percentage),
                value: Some(160.0),
            }),
            ..Default::default()
        };
        let child = PartialParaShape {
            line_spacing: Some(LineSpacing { spacing_type: None, value: Some(170.0) }),
            ..Default::default()
        };
        base.merge(&child);
        assert_eq!(base.alignment, Some(Alignment::Left));
        let ls = base.line_spacing.unwrap();
        assert_eq!(ls.value, Some(170.0)); // Overridden by child
        assert_eq!(ls.spacing_type, Some(LineSpacingType::Percentage)); // Preserved from base (deep merge)
    }

    #[test]
    fn partial_para_shape_deep_merge_spacing() {
        let mut base = PartialParaShape {
            spacing: Some(Spacing { before: Some(HwpUnit::from_pt(6.0).unwrap()), after: None }),
            ..Default::default()
        };
        let child = PartialParaShape {
            spacing: Some(Spacing { before: None, after: Some(HwpUnit::from_pt(12.0).unwrap()) }),
            ..Default::default()
        };
        base.merge(&child);
        let sp = base.spacing.unwrap();
        assert_eq!(sp.before, Some(HwpUnit::from_pt(6.0).unwrap())); // Preserved from base
        assert_eq!(sp.after, Some(HwpUnit::from_pt(12.0).unwrap())); // Added by child
    }

    #[test]
    fn partial_para_shape_deep_merge_indent() {
        let mut base = PartialParaShape {
            indent: Some(Indent {
                left: Some(HwpUnit::from_pt(10.0).unwrap()),
                right: None,
                first_line: Some(HwpUnit::from_pt(5.0).unwrap()),
            }),
            ..Default::default()
        };
        let child = PartialParaShape {
            indent: Some(Indent {
                left: None,
                right: Some(HwpUnit::from_pt(8.0).unwrap()),
                first_line: None,
            }),
            ..Default::default()
        };
        base.merge(&child);
        let indent = base.indent.unwrap();
        assert_eq!(indent.left, Some(HwpUnit::from_pt(10.0).unwrap())); // Preserved
        assert_eq!(indent.right, Some(HwpUnit::from_pt(8.0).unwrap())); // Added
        assert_eq!(indent.first_line, Some(HwpUnit::from_pt(5.0).unwrap())); // Preserved
    }

    #[test]
    fn partial_para_shape_resolve_defaults() {
        let partial = PartialParaShape::default();
        let resolved = partial.resolve();
        assert_eq!(resolved.alignment, Alignment::Left);
        assert_eq!(resolved.line_spacing_type, LineSpacingType::Percentage);
        assert_eq!(resolved.line_spacing_value, 160.0);
        assert_eq!(resolved.space_before, HwpUnit::ZERO);
        assert_eq!(resolved.indent_left, HwpUnit::ZERO);
    }

    #[test]
    fn partial_style_merge_both_present() {
        let mut base = PartialStyle {
            char_shape: Some(PartialCharShape {
                font: Some("Arial".into()),
                size: Some(HwpUnit::from_pt(10.0).unwrap()),
                ..Default::default()
            }),
            para_shape: Some(PartialParaShape {
                alignment: Some(Alignment::Left),
                ..Default::default()
            }),
        };
        let child = PartialStyle {
            char_shape: Some(PartialCharShape {
                size: Some(HwpUnit::from_pt(16.0).unwrap()),
                bold: Some(true),
                ..Default::default()
            }),
            para_shape: None,
        };
        base.merge(&child);
        let cs = base.char_shape.unwrap();
        assert_eq!(cs.font, Some("Arial".into()));
        assert_eq!(cs.size, Some(HwpUnit::from_pt(16.0).unwrap()));
        assert_eq!(cs.bold, Some(true));
    }

    #[test]
    fn partial_style_merge_none_base() {
        let mut base = PartialStyle::default();
        let child = PartialStyle {
            char_shape: Some(PartialCharShape { font: Some("Dotum".into()), ..Default::default() }),
            para_shape: None,
        };
        base.merge(&child);
        assert_eq!(base.char_shape.unwrap().font, Some("Dotum".into()));
    }

    #[test]
    fn char_shape_serde_roundtrip() {
        let original = CharShape {
            font: "한컴바탕".into(),
            size: HwpUnit::from_pt(16.0).unwrap(),
            bold: true,
            italic: false,
            underline: false,
            strikethrough: false,
            color: Color::from_rgb(0x00, 0x33, 0x66),
            superscript: false,
            subscript: false,
        };
        let yaml = serde_yaml::to_string(&original).unwrap();
        let back: CharShape = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn char_shape_yaml_contains_human_readable() {
        let cs = CharShape {
            font: "Arial".into(),
            size: HwpUnit::from_pt(12.0).unwrap(),
            bold: false,
            italic: true,
            underline: false,
            strikethrough: false,
            color: Color::RED,
            superscript: false,
            subscript: false,
        };
        let yaml = serde_yaml::to_string(&cs).unwrap();
        assert!(yaml.contains("12pt"), "Expected '12pt' in: {yaml}");
        assert!(yaml.contains("#FF0000"), "Expected '#FF0000' in: {yaml}");
    }

    #[test]
    fn para_shape_serde_roundtrip() {
        let original = ParaShape {
            alignment: Alignment::Justify,
            line_spacing_type: LineSpacingType::Percentage,
            line_spacing_value: 170.0,
            space_before: HwpUnit::from_pt(6.0).unwrap(),
            space_after: HwpUnit::from_pt(6.0).unwrap(),
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
        };
        let yaml = serde_yaml::to_string(&original).unwrap();
        let back: ParaShape = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn partial_char_shape_from_yaml() {
        let yaml = "font: 한컴바탕\nsize: 16pt\nbold: true\ncolor: '#003366'\n";
        let partial: PartialCharShape = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(partial.font, Some("한컴바탕".into()));
        assert_eq!(partial.size, Some(HwpUnit::from_pt(16.0).unwrap()));
        assert_eq!(partial.bold, Some(true));
        assert_eq!(partial.color, Some(Color::from_rgb(0x00, 0x33, 0x66)));
        assert!(partial.italic.is_none());
    }

    #[test]
    fn partial_para_shape_from_yaml() {
        let yaml = "alignment: Justify\nline_spacing:\n  value: '170%'\nspacing:\n  before: '6pt'\n  after: '6pt'\n";
        let partial: PartialParaShape = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(partial.alignment, Some(Alignment::Justify));
        assert_eq!(partial.line_spacing.unwrap().value, Some(170.0));
        assert_eq!(partial.spacing.unwrap().before, Some(HwpUnit::from_pt(6.0).unwrap()));
    }

    #[test]
    fn partial_style_from_yaml() {
        let yaml = "char_shape:\n  font: Arial\n  size: '10pt'\npara_shape:\n  alignment: Left\n";
        let style: PartialStyle = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(style.char_shape.as_ref().unwrap().font, Some("Arial".into()));
        assert_eq!(style.para_shape.as_ref().unwrap().alignment, Some(Alignment::Left));
    }

    #[test]
    fn spacing_from_yaml() {
        let yaml = "before: '6pt'\nafter: '12pt'\n";
        let spacing: Spacing = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spacing.before, Some(HwpUnit::from_pt(6.0).unwrap()));
        assert_eq!(spacing.after, Some(HwpUnit::from_pt(12.0).unwrap()));
    }

    #[test]
    fn indent_from_yaml() {
        let yaml = "left: '20mm'\nfirst_line: '10pt'\n";
        let indent: Indent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(indent.left, Some(HwpUnit::from_mm(20.0).unwrap()));
        assert_eq!(indent.first_line, Some(HwpUnit::from_pt(10.0).unwrap()));
        assert!(indent.right.is_none());
    }

    #[test]
    fn line_spacing_from_yaml() {
        let yaml = "spacing_type: Percentage\nvalue: '160%'\n";
        let ls: LineSpacing = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ls.spacing_type, Some(LineSpacingType::Percentage));
        assert_eq!(ls.value, Some(160.0));
    }
}
