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

use hwpforge_foundation::{
    Alignment, BorderFillIndex, BreakType, Color, EmbossType, EngraveType, HwpUnit,
    LineSpacingType, OutlineType, ShadowType, StrikeoutShape, UnderlineType, VerticalPosition,
};
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
    /// Text color in `#RRGGBB`.
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub color: Option<Color>,

    /// Underline type (None/Bottom/Center/Top).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline_type: Option<UnderlineType>,
    /// Underline color (inherits text color if None).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub underline_color: Option<Color>,
    /// Strikeout line shape (None/Continuous/Dash/Dot/etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikeout_shape: Option<StrikeoutShape>,
    /// Strikeout color (inherits text color if None).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub strikeout_color: Option<Color>,
    /// Text outline (1pt border around glyphs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineType>,
    /// Drop shadow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowType>,
    /// Emboss effect (raised).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emboss: Option<EmbossType>,
    /// Engrave effect (sunken).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engrave: Option<EngraveType>,
    /// Vertical position (Normal/Superscript/Subscript).
    /// Replaces bool superscript/subscript (backward compat: both supported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_position: Option<VerticalPosition>,
    /// Background shade color (character-level highlight).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub shade_color: Option<Color>,
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
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.underline_type.is_some() {
            self.underline_type = other.underline_type;
        }
        if other.underline_color.is_some() {
            self.underline_color = other.underline_color;
        }
        if other.strikeout_shape.is_some() {
            self.strikeout_shape = other.strikeout_shape;
        }
        if other.strikeout_color.is_some() {
            self.strikeout_color = other.strikeout_color;
        }
        if other.outline.is_some() {
            self.outline = other.outline;
        }
        if other.shadow.is_some() {
            self.shadow = other.shadow;
        }
        if other.emboss.is_some() {
            self.emboss = other.emboss;
        }
        if other.engrave.is_some() {
            self.engrave = other.engrave;
        }
        if other.vertical_position.is_some() {
            self.vertical_position = other.vertical_position;
        }
        if other.shade_color.is_some() {
            self.shade_color = other.shade_color;
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
            color: self.color.unwrap_or(Color::BLACK),
            underline_type: self.underline_type.unwrap_or(UnderlineType::None),
            underline_color: self.underline_color,
            strikeout_shape: self.strikeout_shape.unwrap_or(StrikeoutShape::None),
            strikeout_color: self.strikeout_color,
            outline: self.outline.unwrap_or(OutlineType::None),
            shadow: self.shadow.unwrap_or(ShadowType::None),
            emboss: self.emboss.unwrap_or(EmbossType::None),
            engrave: self.engrave.unwrap_or(EngraveType::None),
            vertical_position: self.vertical_position.unwrap_or(VerticalPosition::Normal),
            shade_color: self.shade_color,
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

    // Advanced paragraph controls (NEW in Phase 5.3)
    /// Page/column break before paragraph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_type: Option<BreakType>,
    /// Keep paragraph with next (prevent page break between).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    /// Keep lines together (prevent page break within paragraph).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_lines_together: Option<bool>,
    /// Widow/orphan control (minimum 2 lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widow_orphan: Option<bool>,
    /// Border/fill reference (for paragraph borders and backgrounds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<BorderFillIndex>,
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
        if other.break_type.is_some() {
            self.break_type = other.break_type;
        }
        if other.keep_with_next.is_some() {
            self.keep_with_next = other.keep_with_next;
        }
        if other.keep_lines_together.is_some() {
            self.keep_lines_together = other.keep_lines_together;
        }
        if other.widow_orphan.is_some() {
            self.widow_orphan = other.widow_orphan;
        }
        if other.border_fill_id.is_some() {
            self.border_fill_id = other.border_fill_id;
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
            break_type: self.break_type.unwrap_or(BreakType::None),
            keep_with_next: self.keep_with_next.unwrap_or(false),
            keep_lines_together: self.keep_lines_together.unwrap_or(false),
            widow_orphan: self.widow_orphan.unwrap_or(true), // Enabled by default in HWPX
            border_fill_id: self.border_fill_id,
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
    /// Text color.
    #[serde(serialize_with = "ser_color", deserialize_with = "de_color")]
    pub color: Color,
    /// Underline type.
    pub underline_type: UnderlineType,
    /// Underline color (None = inherit text color).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub underline_color: Option<Color>,
    /// Strikeout line shape.
    pub strikeout_shape: StrikeoutShape,
    /// Strikeout color (None = inherit text color).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub strikeout_color: Option<Color>,
    /// Text outline.
    pub outline: OutlineType,
    /// Drop shadow.
    pub shadow: ShadowType,
    /// Emboss effect.
    pub emboss: EmbossType,
    /// Engrave effect.
    pub engrave: EngraveType,
    /// Vertical position (replaces superscript/subscript bools).
    pub vertical_position: VerticalPosition,
    /// Background shade color (None = transparent).
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub shade_color: Option<Color>,
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

    // Advanced paragraph controls (NEW in Phase 5.3)
    /// Page/column break type.
    pub break_type: BreakType,
    /// Keep paragraph with next.
    pub keep_with_next: bool,
    /// Keep lines together.
    pub keep_lines_together: bool,
    /// Widow/orphan control (default: true).
    pub widow_orphan: bool,
    /// Border/fill reference (None = no border/fill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<BorderFillIndex>,
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
        assert!(p.color.is_none());
        assert!(p.underline_type.is_none());
        assert!(p.strikeout_shape.is_none());
        assert!(p.vertical_position.is_none());
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
            color: Color::from_rgb(0x00, 0x33, 0x66),
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            outline: OutlineType::None,
            shadow: ShadowType::None,
            emboss: EmbossType::None,
            engrave: EngraveType::None,
            vertical_position: VerticalPosition::Normal,
            shade_color: None,
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
            color: Color::RED,
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            outline: OutlineType::None,
            shadow: ShadowType::None,
            emboss: EmbossType::None,
            engrave: EngraveType::None,
            vertical_position: VerticalPosition::Normal,
            shade_color: None,
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
            break_type: BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
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
