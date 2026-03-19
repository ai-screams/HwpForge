//! StyleRegistry: converts Template styles into indexed collections.
//!
//! The registry performs the final **resolution step** in the Blueprint workflow:
//!
//! ```text
//! Template (YAML, all styles by name)
//!     |
//!     v
//! StyleRegistry::from_template()
//!     |
//!     v
//! StyleRegistry (indexed Vecs: fonts, char_shapes, para_shapes, tabs)
//! ```
//!
//! This separation mirrors the **HTML + CSS** model:
//! - Template = CSS (named styles in human-friendly format)
//! - StyleRegistry = compiled CSS (numeric indices for runtime efficiency)
//!
//! Each style in the template gets allocated sequential indices
//! (CharShapeIndex, ParaShapeIndex). Fonts are deduplicated: two styles
//! using "한컴바탕" share a single FontIndex.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use hwpforge_core::TabDef;
use hwpforge_foundation::{CharShapeIndex, FontId, FontIndex, ParaShapeIndex};

use crate::error::{BlueprintError, BlueprintResult};
use crate::style::{CharShape, ParaShape, PartialStyle};
use crate::template::{Template, TemplateTabDef};

/// A resolved style entry with allocated indices.
///
/// This is the result of resolving a named style from the Template.
/// It contains indices pointing into the StyleRegistry's flat collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct StyleEntry {
    /// Index into the character shape collection.
    pub char_shape_id: CharShapeIndex,
    /// Index into the paragraph shape collection.
    pub para_shape_id: ParaShapeIndex,
    /// Index into the font collection (deduplicated).
    pub font_id: FontIndex,
}

/// A registry of resolved styles with index-based access.
///
/// After inheritance resolution, the Template is converted into a
/// StyleRegistry where every style is assigned numeric indices for
/// efficient lookup during document rendering.
///
/// # Font Deduplication
///
/// Multiple styles can reference the same font. The registry deduplicates
/// fonts automatically:
///
/// ```rust,ignore
/// // Two styles with the same font → single FontIndex
/// styles:
///   body: { font: "Batang", size: 10pt }
///   heading: { font: "Batang", size: 16pt }
///
/// // Registry: fonts = ["Batang"] (index 0)
/// //           char_shapes[0].font_id = FontIndex(0)
/// //           char_shapes[1].font_id = FontIndex(0)
/// ```
///
/// # Index Allocation
///
/// Indices are allocated sequentially in the order styles appear in the
/// template (preserving YAML field order via IndexMap):
/// - CharShape 0, CharShape 1, CharShape 2...
/// - ParaShape 0, ParaShape 1, ParaShape 2...
/// - Font 0, Font 1... (deduplicated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct StyleRegistry {
    /// All unique fonts referenced by character shapes.
    pub fonts: Vec<FontId>,
    /// All resolved character shapes.
    pub char_shapes: Vec<CharShape>,
    /// All resolved paragraph shapes.
    pub para_shapes: Vec<ParaShape>,
    /// Shared tab definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabDef>,
    /// Mapping from style name to its indices (insertion-order preserved).
    pub style_entries: IndexMap<String, StyleEntry>,
}

impl StyleRegistry {
    /// Creates an empty StyleRegistry with only fonts.
    ///
    /// Useful when constructing a style store from preset fonts without a full
    /// template. Pass this to [`HwpxStyleStore::from_registry`][crate] to produce a
    /// complete style store with default char shapes, para shapes, and border fills.
    pub fn with_fonts(fonts: Vec<FontId>) -> Self {
        Self {
            fonts,
            char_shapes: vec![],
            para_shapes: vec![],
            tabs: vec![],
            style_entries: IndexMap::new(),
        }
    }

    /// Creates a StyleRegistry from a Template.
    ///
    /// This is the **final resolution step**:
    /// 1. Iterate over template styles (in order)
    /// 2. Resolve each PartialStyle → CharShape + ParaShape
    /// 3. Deduplicate fonts
    /// 4. Allocate sequential indices
    ///
    /// # Errors
    ///
    /// - [`BlueprintError::EmptyStyleMap`] if the template has no styles
    /// - [`BlueprintError::StyleResolution`] if any style is missing required fields
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use hwpforge_blueprint::{Template, StyleRegistry};
    ///
    /// let template = Template::from_yaml(yaml)?;
    /// let registry = StyleRegistry::from_template(&template)?;
    ///
    /// // Access by name
    /// let body_entry = registry.get_style("body").unwrap();
    /// let char_shape = registry.char_shape(body_entry.char_shape_id).unwrap();
    /// assert_eq!(char_shape.font, "한컴바탕");
    /// ```
    pub fn from_template(template: &Template) -> BlueprintResult<Self> {
        if template.styles.is_empty() {
            return Err(BlueprintError::EmptyStyleMap);
        }

        validate_template_style_names(&template.styles)?;

        let mut fonts = Vec::new();
        let mut char_shapes = Vec::new();
        let mut para_shapes = Vec::new();
        let mut style_entries = IndexMap::new();
        let tabs = validate_tabs(&template.tabs)?;
        let mut font_indices: BTreeMap<String, FontIndex> = BTreeMap::new();

        for (style_name, partial_style) in &template.styles {
            let style_entry = build_style_entry(
                style_name,
                partial_style,
                &tabs,
                &mut fonts,
                &mut char_shapes,
                &mut para_shapes,
                &mut font_indices,
            )?;
            style_entries.insert(style_name.clone(), style_entry);
        }

        // Validate markdown mapping references
        if let Some(ref md) = template.markdown_mapping {
            validate_mapping_references(md, &style_entries)?;
        }

        Ok(StyleRegistry { fonts, char_shapes, para_shapes, tabs, style_entries })
    }

    /// Looks up a style by name.
    ///
    /// Returns `None` if the style name does not exist.
    pub fn get_style(&self, name: &str) -> Option<&StyleEntry> {
        self.style_entries.get(name)
    }

    /// Retrieves a character shape by index.
    ///
    /// Returns `None` if the index is out of bounds.
    pub fn char_shape(&self, idx: CharShapeIndex) -> Option<&CharShape> {
        self.char_shapes.get(idx.get())
    }

    /// Retrieves a paragraph shape by index.
    ///
    /// Returns `None` if the index is out of bounds.
    pub fn para_shape(&self, idx: ParaShapeIndex) -> Option<&ParaShape> {
        self.para_shapes.get(idx.get())
    }

    /// Retrieves a font by index.
    ///
    /// Returns `None` if the index is out of bounds.
    pub fn font(&self, idx: FontIndex) -> Option<&FontId> {
        self.fonts.get(idx.get())
    }

    /// Returns the number of unique fonts.
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Returns the number of character shapes.
    pub fn char_shape_count(&self) -> usize {
        self.char_shapes.len()
    }

    /// Returns the number of paragraph shapes.
    pub fn para_shape_count(&self) -> usize {
        self.para_shapes.len()
    }

    /// Returns the number of shared tab definitions.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns the number of named styles.
    pub fn style_count(&self) -> usize {
        self.style_entries.len()
    }
}

fn validate_template_style_names(styles: &IndexMap<String, PartialStyle>) -> BlueprintResult<()> {
    for name in styles.keys() {
        validate_style_name(name)?;
    }
    Ok(())
}

fn build_style_entry(
    style_name: &str,
    partial_style: &PartialStyle,
    tabs: &[TabDef],
    fonts: &mut Vec<FontId>,
    char_shapes: &mut Vec<CharShape>,
    para_shapes: &mut Vec<ParaShape>,
    font_indices: &mut BTreeMap<String, FontIndex>,
) -> BlueprintResult<StyleEntry> {
    let char_shape = resolve_char_shape(style_name, partial_style)?;
    let para_shape = resolve_para_shape(partial_style)?;
    validate_tab_reference(style_name, para_shape.tab_def_id, tabs)?;

    let font_id = intern_font(fonts, font_indices, &char_shape.font)?;
    let char_shape_id = CharShapeIndex::new(char_shapes.len());
    char_shapes.push(char_shape);

    let para_shape_id = ParaShapeIndex::new(para_shapes.len());
    para_shapes.push(para_shape);

    Ok(StyleEntry { char_shape_id, para_shape_id, font_id })
}

fn resolve_char_shape(
    style_name: &str,
    partial_style: &PartialStyle,
) -> BlueprintResult<CharShape> {
    partial_style
        .char_shape
        .as_ref()
        .ok_or_else(|| BlueprintError::StyleResolution {
            style_name: style_name.to_string(),
            field: "char_shape".to_string(),
        })?
        .resolve(style_name)
}

fn resolve_para_shape(partial_style: &PartialStyle) -> BlueprintResult<ParaShape> {
    Ok(partial_style
        .para_shape
        .as_ref()
        .map_or_else(crate::style::PartialParaShape::default, Clone::clone)
        .resolve())
}

fn intern_font(
    fonts: &mut Vec<FontId>,
    font_indices: &mut BTreeMap<String, FontIndex>,
    font_name: &str,
) -> BlueprintResult<FontIndex> {
    if let Some(&existing_idx) = font_indices.get(font_name) {
        return Ok(existing_idx);
    }

    let font_id = FontId::new(font_name.to_string())?;
    let new_idx = FontIndex::new(fonts.len());
    fonts.push(font_id);
    font_indices.insert(font_name.to_string(), new_idx);
    Ok(new_idx)
}

/// Validates a style name: must be non-empty, alphanumeric + underscore, start with letter/underscore.
fn validate_style_name(name: &str) -> BlueprintResult<()> {
    if name.is_empty() {
        return Err(BlueprintError::InvalidStyleName {
            name: name.to_string(),
            reason: "style name cannot be empty".to_string(),
        });
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(BlueprintError::InvalidStyleName {
            name: name.to_string(),
            reason: "must contain only ASCII alphanumeric characters and underscores".to_string(),
        });
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(BlueprintError::InvalidStyleName {
            name: name.to_string(),
            reason: "must not start with a digit".to_string(),
        });
    }
    Ok(())
}

/// Validates that all non-None MarkdownMapping references point to existing styles.
fn validate_mapping_references(
    md: &crate::template::MarkdownMapping,
    styles: &IndexMap<String, StyleEntry>,
) -> BlueprintResult<()> {
    let fields: &[(&str, &Option<String>)] = &[
        ("body", &md.body),
        ("heading1", &md.heading1),
        ("heading2", &md.heading2),
        ("heading3", &md.heading3),
        ("heading4", &md.heading4),
        ("heading5", &md.heading5),
        ("heading6", &md.heading6),
        ("code", &md.code),
        ("blockquote", &md.blockquote),
        ("list_item", &md.list_item),
    ];
    for &(field_name, ref_opt) in fields {
        if let Some(style_name) = ref_opt {
            if !styles.contains_key(style_name) {
                return Err(BlueprintError::InvalidMappingReference {
                    mapping_field: field_name.to_string(),
                    style_name: style_name.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_tabs(tabs: &[TemplateTabDef]) -> BlueprintResult<Vec<TabDef>> {
    let mut seen = BTreeMap::new();
    for tab in tabs {
        if TabDef::is_builtin_id(tab.id) {
            return Err(BlueprintError::InvalidTabDefinition {
                id: tab.id,
                reason: format!(
                    "ids 0..={} are reserved for built-in 한글 tab definitions",
                    TabDef::BUILTIN_COUNT - 1
                ),
            });
        }
        if seen.insert(tab.id, ()).is_some() {
            return Err(BlueprintError::DuplicateTabDefinition { id: tab.id });
        }
        validate_tab_stops(tab)?;
    }
    Ok(tabs.iter().map(Into::into).collect())
}

fn validate_tab_stops(tab: &TemplateTabDef) -> BlueprintResult<()> {
    let mut previous: Option<i32> = None;
    for (idx, stop) in tab.stops.iter().enumerate() {
        let position = stop.position.as_i32();
        if position < 0 {
            return Err(BlueprintError::InvalidTabDefinition {
                id: tab.id,
                reason: format!("tab stop {} has negative position {}", idx, position),
            });
        }
        if let Some(prev) = previous {
            if position <= prev {
                return Err(BlueprintError::InvalidTabDefinition {
                    id: tab.id,
                    reason: format!(
                        "tab stop {} at {} must be strictly greater than previous stop at {}",
                        idx, position, prev
                    ),
                });
            }
        }
        previous = Some(position);
    }
    Ok(())
}

fn validate_tab_reference(
    style_name: &str,
    tab_def_id: u32,
    tabs: &[TabDef],
) -> BlueprintResult<()> {
    if TabDef::reference_is_known(tab_def_id, tabs.iter().map(|tab| tab.id)) {
        return Ok(());
    }
    Err(BlueprintError::InvalidTabReference {
        style_name: style_name.to_string(),
        tab_id: tab_def_id,
        reason: "no matching tab definition exists in template.tabs".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::PartialStyle;
    use crate::template::{TemplateMeta, TemplateTabDef, TemplateTabStop};
    use hwpforge_foundation::{Alignment, HwpUnit, LineSpacingType};
    use pretty_assertions::assert_eq;

    // Helper: create a minimal PartialStyle with font + size
    fn make_partial_style(font: &str, size_pt: f64) -> PartialStyle {
        PartialStyle {
            char_shape: Some(crate::style::PartialCharShape {
                font: Some(font.to_string()),
                size: Some(HwpUnit::from_pt(size_pt).unwrap()),
                ..Default::default()
            }),
            para_shape: None,
        }
    }

    // Helper: create a minimal Template with given styles
    fn make_template(styles: IndexMap<String, PartialStyle>) -> Template {
        Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![],
            markdown_mapping: None,
        }
    }

    #[test]
    fn from_template_single_style() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.style_count(), 1);
        assert_eq!(registry.char_shape_count(), 1);
        assert_eq!(registry.para_shape_count(), 1);
        assert_eq!(registry.font_count(), 1);

        let entry = registry.get_style("body").unwrap();
        assert_eq!(entry.char_shape_id, CharShapeIndex::new(0));
        assert_eq!(entry.para_shape_id, ParaShapeIndex::new(0));
        assert_eq!(entry.font_id, FontIndex::new(0));
    }

    #[test]
    fn from_template_multiple_styles() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Dotum", 16.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.style_count(), 2);
        assert_eq!(registry.char_shape_count(), 2);
        assert_eq!(registry.para_shape_count(), 2);
        assert_eq!(registry.font_count(), 2); // Different fonts

        let body = registry.get_style("body").unwrap();
        let heading = registry.get_style("heading").unwrap();

        assert_eq!(body.char_shape_id, CharShapeIndex::new(0));
        assert_eq!(heading.char_shape_id, CharShapeIndex::new(1));
    }

    #[test]
    fn font_deduplication_same_font() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Batang", 16.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        // 2 styles, same font → 1 FontId
        assert_eq!(registry.font_count(), 1);
        assert_eq!(registry.fonts[0].as_str(), "Batang");

        // Both entries point to the same FontIndex
        let body = registry.get_style("body").unwrap();
        let heading = registry.get_style("heading").unwrap();
        assert_eq!(body.font_id, FontIndex::new(0));
        assert_eq!(heading.font_id, FontIndex::new(0));
    }

    #[test]
    fn font_deduplication_different_fonts() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Dotum", 16.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        // 2 styles, different fonts → 2 FontIds
        assert_eq!(registry.font_count(), 2);
        assert_eq!(registry.fonts[0].as_str(), "Batang");
        assert_eq!(registry.fonts[1].as_str(), "Dotum");

        // Each entry points to its own FontIndex
        let body = registry.get_style("body").unwrap();
        let heading = registry.get_style("heading").unwrap();
        assert_eq!(body.font_id, FontIndex::new(0));
        assert_eq!(heading.font_id, FontIndex::new(1));
    }

    #[test]
    fn get_style_by_name() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        let entry = registry.get_style("body").unwrap();
        assert_eq!(entry.char_shape_id, CharShapeIndex::new(0));

        assert!(registry.get_style("nonexistent").is_none());
    }

    #[test]
    fn char_shape_by_index() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        let cs = registry.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.font, "Batang");
        assert_eq!(cs.size, HwpUnit::from_pt(10.0).unwrap());

        assert!(registry.char_shape(CharShapeIndex::new(99)).is_none());
    }

    #[test]
    fn para_shape_by_index() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        let ps = registry.para_shape(ParaShapeIndex::new(0)).unwrap();
        // Defaults from PartialParaShape::default().resolve()
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.line_spacing_type, LineSpacingType::Percentage);
        assert_eq!(ps.line_spacing_value, 160.0);

        assert!(registry.para_shape(ParaShapeIndex::new(99)).is_none());
    }

    #[test]
    fn empty_template_error() {
        let template = make_template(IndexMap::new());

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::EmptyStyleMap));
    }

    #[test]
    fn missing_font_error() {
        let mut styles = IndexMap::new();
        styles.insert(
            "broken".to_string(),
            PartialStyle {
                char_shape: Some(crate::style::PartialCharShape {
                    font: None, // Missing!
                    size: Some(HwpUnit::from_pt(10.0).unwrap()),
                    ..Default::default()
                }),
                para_shape: None,
            },
        );

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();

        match err {
            BlueprintError::StyleResolution { style_name, field } => {
                assert_eq!(style_name, "broken");
                assert_eq!(field, "font");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn missing_size_error() {
        let mut styles = IndexMap::new();
        styles.insert(
            "broken".to_string(),
            PartialStyle {
                char_shape: Some(crate::style::PartialCharShape {
                    font: Some("Batang".to_string()),
                    size: None, // Missing!
                    ..Default::default()
                }),
                para_shape: None,
            },
        );

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();

        match err {
            BlueprintError::StyleResolution { style_name, field } => {
                assert_eq!(style_name, "broken");
                assert_eq!(field, "size");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn serde_roundtrip_style_registry() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Dotum", 16.0));

        let template = make_template(styles);
        let original = StyleRegistry::from_template(&template).unwrap();

        let yaml = serde_yaml::to_string(&original).unwrap();
        let back: StyleRegistry = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(original.font_count(), back.font_count());
        assert_eq!(original.char_shape_count(), back.char_shape_count());
        assert_eq!(original.para_shape_count(), back.para_shape_count());
        assert_eq!(original.style_count(), back.style_count());
    }

    #[test]
    fn style_entry_serde_roundtrip() {
        let entry = StyleEntry {
            char_shape_id: CharShapeIndex::new(3),
            para_shape_id: ParaShapeIndex::new(7),
            font_id: FontIndex::new(1),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let back: StyleEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry, back);
    }

    #[test]
    fn font_count() {
        let mut styles = IndexMap::new();
        styles.insert("a".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("b".to_string(), make_partial_style("Batang", 12.0)); // Same font
        styles.insert("c".to_string(), make_partial_style("Dotum", 10.0)); // Different

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.font_count(), 2); // Batang, Dotum
    }

    #[test]
    fn char_shape_count() {
        let mut styles = IndexMap::new();
        styles.insert("a".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("b".to_string(), make_partial_style("Batang", 12.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.char_shape_count(), 2); // 2 char shapes even if same font
    }

    #[test]
    fn para_shape_count() {
        let mut styles = IndexMap::new();
        styles.insert("a".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("b".to_string(), make_partial_style("Dotum", 12.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.para_shape_count(), 2);
    }

    #[test]
    fn style_count() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Dotum", 16.0));

        let template = make_template(styles);
        let registry = StyleRegistry::from_template(&template).unwrap();

        assert_eq!(registry.style_count(), 2);
    }

    #[test]
    fn valid_style_names_accepted() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading1".to_string(), make_partial_style("Batang", 16.0));
        styles.insert("_private".to_string(), make_partial_style("Batang", 12.0));
        styles.insert("my_style_2".to_string(), make_partial_style("Batang", 14.0));

        let template = make_template(styles);
        assert!(StyleRegistry::from_template(&template).is_ok());
    }

    #[test]
    fn invalid_style_name_with_spaces() {
        let mut styles = IndexMap::new();
        styles.insert("body style".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidStyleName { .. }));
    }

    #[test]
    fn invalid_style_name_starts_with_digit() {
        let mut styles = IndexMap::new();
        styles.insert("1heading".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidStyleName { .. }));
    }

    #[test]
    fn invalid_style_name_special_chars() {
        let mut styles = IndexMap::new();
        styles.insert("body-style".to_string(), make_partial_style("Batang", 10.0));

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidStyleName { .. }));
    }

    #[test]
    fn markdown_mapping_valid_references() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        styles.insert("heading".to_string(), make_partial_style("Batang", 16.0));

        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![],
            markdown_mapping: Some(crate::template::MarkdownMapping {
                body: Some("body".to_string()),
                heading1: Some("heading".to_string()),
                ..Default::default()
            }),
        };

        // Should succeed — all references are valid
        let registry = StyleRegistry::from_template(&template).unwrap();
        assert_eq!(registry.style_count(), 2);
    }

    #[test]
    fn registry_carries_template_tabs() {
        let mut styles = IndexMap::new();
        styles.insert(
            "body".to_string(),
            PartialStyle {
                char_shape: Some(crate::style::PartialCharShape {
                    font: Some("Batang".to_string()),
                    size: Some(HwpUnit::from_pt(10.0).unwrap()),
                    ..Default::default()
                }),
                para_shape: Some(crate::style::PartialParaShape {
                    tab_def_id: Some(3),
                    ..Default::default()
                }),
            },
        );
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![TemplateTabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![TemplateTabStop {
                    position: HwpUnit::new(8000).unwrap(),
                    align: hwpforge_foundation::TabAlign::Left,
                    leader: hwpforge_foundation::TabLeader::dot(),
                }],
            }],
            markdown_mapping: None,
        };

        let registry = StyleRegistry::from_template(&template).unwrap();
        assert_eq!(registry.tab_count(), 1);
        assert_eq!(registry.tabs[0].id, 3);
        assert_eq!(registry.para_shapes[0].tab_def_id, 3);
    }

    #[test]
    fn template_rejects_missing_custom_tab_definition() {
        let mut styles = IndexMap::new();
        styles.insert(
            "body".to_string(),
            PartialStyle {
                char_shape: Some(crate::style::PartialCharShape {
                    font: Some("Batang".to_string()),
                    size: Some(HwpUnit::from_pt(10.0).unwrap()),
                    ..Default::default()
                }),
                para_shape: Some(crate::style::PartialParaShape {
                    tab_def_id: Some(3),
                    ..Default::default()
                }),
            },
        );

        let template = make_template(styles);
        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidTabReference { .. }));
    }

    #[test]
    fn template_rejects_reserved_tab_definition_ids() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![TemplateTabDef {
                id: 1,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![TemplateTabStop {
                    position: HwpUnit::new(8000).unwrap(),
                    align: hwpforge_foundation::TabAlign::Left,
                    leader: hwpforge_foundation::TabLeader::dot(),
                }],
            }],
            markdown_mapping: None,
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidTabDefinition { .. }));
    }

    #[test]
    fn template_rejects_duplicate_tab_definition_ids() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let mk_tab = || TemplateTabDef {
            id: 3,
            auto_tab_left: false,
            auto_tab_right: false,
            stops: vec![TemplateTabStop {
                position: HwpUnit::new(8000).unwrap(),
                align: hwpforge_foundation::TabAlign::Left,
                leader: hwpforge_foundation::TabLeader::dot(),
            }],
        };
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![mk_tab(), mk_tab()],
            markdown_mapping: None,
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::DuplicateTabDefinition { .. }));
    }

    #[test]
    fn template_rejects_out_of_order_tab_stops() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![TemplateTabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![
                    TemplateTabStop {
                        position: HwpUnit::new(8000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Left,
                        leader: hwpforge_foundation::TabLeader::none(),
                    },
                    TemplateTabStop {
                        position: HwpUnit::new(4000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Right,
                        leader: hwpforge_foundation::TabLeader::from_hwpx_str("DASH"),
                    },
                ],
            }],
            markdown_mapping: None,
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidTabDefinition { .. }));
    }

    #[test]
    fn template_rejects_duplicate_tab_stop_positions() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![TemplateTabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![
                    TemplateTabStop {
                        position: HwpUnit::new(4000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Left,
                        leader: hwpforge_foundation::TabLeader::none(),
                    },
                    TemplateTabStop {
                        position: HwpUnit::new(4000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Center,
                        leader: hwpforge_foundation::TabLeader::dot(),
                    },
                ],
            }],
            markdown_mapping: None,
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidTabDefinition { .. }));
    }

    #[test]
    fn template_rejects_negative_tab_stop_positions() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![TemplateTabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![TemplateTabStop {
                    position: HwpUnit::new(-100).unwrap(),
                    align: hwpforge_foundation::TabAlign::Left,
                    leader: hwpforge_foundation::TabLeader::none(),
                }],
            }],
            markdown_mapping: None,
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidTabDefinition { .. }));
    }

    #[test]
    fn markdown_mapping_invalid_reference_error() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));

        let template = Template {
            meta: TemplateMeta {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: None,
            },
            page: None,
            styles,
            tabs: vec![],
            markdown_mapping: Some(crate::template::MarkdownMapping {
                body: Some("body".to_string()),
                heading1: Some("nonexistent".to_string()), // Invalid!
                ..Default::default()
            }),
        };

        let err = StyleRegistry::from_template(&template).unwrap_err();
        match err {
            BlueprintError::InvalidMappingReference { mapping_field, style_name } => {
                assert_eq!(mapping_field, "heading1");
                assert_eq!(style_name, "nonexistent");
            }
            other => panic!("Expected InvalidMappingReference, got: {other:?}"),
        }
    }
}
