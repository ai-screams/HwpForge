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

use hwpforge_core::{BulletDef, NumberingDef, TabDef};
use hwpforge_foundation::{
    BulletIndex, CharShapeIndex, FontId, FontIndex, NumberingIndex, ParaShapeIndex, StyleRef,
};

use crate::border_fill::{BorderFill, Fill};
use crate::error::{BlueprintError, BlueprintResult};
use crate::style::{CharShape, ParaShape, PartialStyle};
use crate::template::{Template, TemplateTabDef};
use hwpforge_foundation::{BorderLineType, FoundationError};

/// A resolved style entry with allocated indices.
///
/// This is the result of resolving a named style from the Template.
/// It contains indices pointing into the StyleRegistry's flat collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct StyleEntry {
    style_ref: StyleRef,
    /// Index into the character shape collection.
    pub char_shape_id: CharShapeIndex,
    /// Index into the paragraph shape collection.
    pub para_shape_id: ParaShapeIndex,
    /// Index into the font collection (deduplicated).
    pub font_id: FontIndex,
}

impl StyleEntry {
    /// Returns the document-scoped identity of this entry.
    pub fn style_ref(&self) -> StyleRef {
        self.style_ref
    }
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
///
/// Each style registers `font` first, then `font_latin`, then
/// `font_east_asian`. Existing names retain their first allocated index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
#[serde(try_from = "StyleRegistryWire")]
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
    /// Shared numbering definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numberings: Vec<NumberingDef>,
    /// Shared bullet definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bullets: Vec<BulletDef>,
    /// Mapping from style name to its indices (insertion-order preserved).
    style_entries: IndexMap<String, StyleEntry>,
    /// First insertion position for each identity; entries are private and never removed.
    #[serde(skip)]
    style_positions: BTreeMap<StyleIdentity, usize>,
    /// Original display names, keyed by style identity rather than display text.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    style_display_names: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    border_fills: Vec<BorderFill>,
}

// Schemars follows serde's try_from for the deserialize contract. Keep the
// field defaults here in step with `StyleRegistry`.
#[derive(Deserialize, JsonSchema)]
struct StyleRegistryWire {
    /// All unique fonts referenced by character shapes.
    fonts: Vec<FontId>,
    /// All resolved character shapes.
    char_shapes: Vec<CharShape>,
    /// All resolved paragraph shapes.
    para_shapes: Vec<ParaShape>,
    /// Shared tab definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tabs: Vec<TabDef>,
    /// Shared numbering definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    numberings: Vec<NumberingDef>,
    /// Shared bullet definitions referenced by paragraph shapes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bullets: Vec<BulletDef>,
    /// Mapping from style name to its indices (insertion-order preserved).
    style_entries: IndexMap<String, StyleEntry>,
    /// Original display names, keyed by style identity rather than display text.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    style_display_names: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    border_fills: Vec<BorderFill>,
}

impl TryFrom<StyleRegistryWire> for StyleRegistry {
    type Error = BlueprintError;

    fn try_from(wire: StyleRegistryWire) -> BlueprintResult<Self> {
        let registry = Self {
            fonts: wire.fonts,
            char_shapes: wire.char_shapes,
            para_shapes: wire.para_shapes,
            tabs: wire.tabs,
            numberings: wire.numberings,
            bullets: wire.bullets,
            style_positions: style_positions(&wire.style_entries),
            style_entries: wire.style_entries,
            style_display_names: wire.style_display_names,
            border_fills: wire.border_fills,
        };
        for (index, value) in registry.border_fills.iter().enumerate() {
            validate_border_fill(value)?;
            if registry.border_fills[..index].contains(value) {
                return Err(FoundationError::InvalidField {
                    field: "border_fills".into(),
                    reason: "duplicate border definition would change reference identity".into(),
                }
                .into());
            }
        }
        for key in registry.style_display_names.keys() {
            registry.validate_display_name_key(key)?;
        }
        Ok(registry)
    }
}

// This index covers only private identity metadata, never public shape Vecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StyleIdentity(StyleRef);
impl Ord for StyleIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        STYLE_COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0.cmp(&other.0)
    }
}
impl PartialOrd for StyleIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
#[cfg(test)]
thread_local! { static STYLE_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn style_positions(entries: &IndexMap<String, StyleEntry>) -> BTreeMap<StyleIdentity, usize> {
    let mut positions = BTreeMap::new();
    for (position, entry) in entries.values().enumerate() {
        positions.entry(StyleIdentity(entry.style_ref)).or_insert(position);
    }
    positions
}

impl StyleRegistry {
    /// Registers a four-sided border definition and returns its one-based local reference.
    ///
    /// Each side must use `None` or `Solid`, with unspecified width and color.
    /// The fill must equal [`Fill::default`]. These restrictions allow exactly
    /// sixteen definitions. Equal definitions reuse their existing reference.
    /// Pass the registry and its document together to the import bridge; the
    /// reference is not an HWPX store position.
    ///
    /// # Errors
    /// Returns [`FoundationError::InvalidField`] for any other definition.
    /// A failed registration leaves the registry unchanged.
    pub fn register_border_fill(&mut self, value: BorderFill) -> Result<u32, FoundationError> {
        validate_border_fill(&value)?;
        if let Some(index) = self.border_fills.iter().position(|existing| *existing == value) {
            return Ok(index as u32 + 1);
        }
        self.border_fills.push(value);
        Ok(self.border_fills.len() as u32)
    }

    /// Looks up a one-based registry-local border definition.
    /// Zero and references beyond the registered definitions return `None`.
    pub fn border_fill(&self, id: u32) -> Option<&BorderFill> {
        id.checked_sub(1).and_then(|index| self.border_fills.get(index as usize))
    }
    /// Returns registered entries in insertion order.
    pub fn style_entries(&self) -> &IndexMap<String, StyleEntry> {
        &self.style_entries
    }

    /// Registers a named entry, retaining its identity when updating its formatting.
    pub fn register_style(
        &mut self,
        name: String,
        char_shape_id: CharShapeIndex,
        para_shape_id: ParaShapeIndex,
        font_id: FontIndex,
    ) -> BlueprintResult<StyleRef> {
        validate_style_name(&name)?;
        let style_ref = self
            .style_entries
            .get(&name)
            .map_or(StyleRef::new(self.style_entries.len()), |entry| entry.style_ref);
        let position = self.style_entries.len();
        self.style_positions.entry(StyleIdentity(style_ref)).or_insert(position);
        self.style_entries
            .insert(name, StyleEntry { style_ref, char_shape_id, para_shape_id, font_id });
        Ok(style_ref)
    }

    /// Sets the original display name of a registered style, replacing any previous name.
    ///
    /// Names are stored verbatim, including empty strings. They are metadata, not
    /// identifiers: multiple styles may share a name without being merged.
    /// Updating the same key with [`Self::register_style`] retains both its
    /// [`StyleRef`] and its display name.
    ///
    /// # Errors
    ///
    /// Returns [`BlueprintError::InvalidStyleName`] for an unregistered reference,
    /// leaving the registry unchanged.
    ///
    /// Each [`StyleRef`] is expected to belong to one entry. Deserialized registries
    /// are not checked for duplicate references, and [`Self::register_style`]
    /// numbers a new key by the entry count, so registering into a registry read
    /// with non-sequential references can reuse an existing one. If two entries
    /// share a reference, the name attaches to the first entry in insertion order.
    pub fn set_style_display_name(
        &mut self,
        style: StyleRef,
        name: impl Into<String>,
    ) -> BlueprintResult<()> {
        // The key comes from `style_entries`, so it cannot be an orphan.
        let key = self.style_key(style).ok_or_else(|| BlueprintError::InvalidStyleName {
            name: format!("StyleRef({})", style.get()),
            reason: "no registered style has this StyleRef".into(),
        })?;
        self.style_display_names.insert(key.to_owned(), name.into());
        Ok(())
    }

    /// Returns the original display name, or `None` for an unnamed or unknown style.
    ///
    /// Display names do not identify or merge styles; multiple styles may share
    /// one. Updating the same key with [`Self::register_style`] preserves the
    /// identity and display name. An explicitly stored empty name returns `Some("")`.
    pub fn style_display_name(&self, style: StyleRef) -> Option<&str> {
        self.style_display_names.get(self.style_key(style)?).map(String::as_str)
    }

    fn style_key(&self, style: StyleRef) -> Option<&str> {
        self.style_positions
            .get(&StyleIdentity(style))
            .and_then(|&position| self.style_entries.get_index(position))
            .map(|(key, _)| key.as_str())
    }

    fn validate_display_name_key(&self, key: &str) -> BlueprintResult<()> {
        if !self.style_entries.contains_key(key) {
            return Err(BlueprintError::InvalidStyleName {
                name: key.into(),
                reason: "display name references unknown style".into(),
            });
        }
        Ok(())
    }

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
            numberings: vec![],
            bullets: vec![],
            style_entries: IndexMap::new(),
            style_positions: BTreeMap::new(),
            style_display_names: IndexMap::new(),
            border_fills: Vec::new(),
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
        validate_numbering_definitions(&template.numberings)?;
        validate_bullet_definitions(&template.bullets)?;

        let mut fonts = Vec::new();
        let mut char_shapes = Vec::new();
        let mut para_shapes = Vec::new();
        let mut style_entries = IndexMap::new();
        let tabs = validate_tabs(&template.tabs)?;
        let numberings = template.numberings.clone();
        let bullets = template.bullets.clone();
        let mut font_indices: BTreeMap<String, FontIndex> = BTreeMap::new();
        let mut build_ctx = RegistryBuildCtx {
            tabs: &tabs,
            numberings: &numberings,
            bullets: &bullets,
            fonts: &mut fonts,
            char_shapes: &mut char_shapes,
            para_shapes: &mut para_shapes,
            font_indices: &mut font_indices,
        };

        for (style_name, partial_style) in &template.styles {
            let mut style_entry = build_style_entry(&mut build_ctx, style_name, partial_style)?;
            style_entry.style_ref = StyleRef::new(style_entries.len());
            style_entries.insert(style_name.clone(), style_entry);
        }

        // Validate markdown mapping references
        if let Some(ref md) = template.markdown_mapping {
            validate_mapping_references(md, &style_entries)?;
        }

        Ok(StyleRegistry {
            fonts,
            char_shapes,
            para_shapes,
            tabs,
            numberings,
            bullets,
            style_positions: style_positions(&style_entries),
            style_entries,
            style_display_names: IndexMap::new(),
            border_fills: Vec::new(),
        })
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

    /// Retrieves a numbering definition by index.
    pub fn numbering(&self, idx: NumberingIndex) -> Option<&NumberingDef> {
        self.numberings.get(idx.get())
    }

    /// Retrieves a bullet definition by index.
    pub fn bullet(&self, idx: BulletIndex) -> Option<&BulletDef> {
        self.bullets.get(idx.get())
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

    /// Returns the number of shared numbering definitions.
    pub fn numbering_count(&self) -> usize {
        self.numberings.len()
    }

    /// Returns the number of shared bullet definitions.
    pub fn bullet_count(&self) -> usize {
        self.bullets.len()
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

struct RegistryBuildCtx<'a> {
    tabs: &'a [TabDef],
    numberings: &'a [NumberingDef],
    bullets: &'a [BulletDef],
    fonts: &'a mut Vec<FontId>,
    char_shapes: &'a mut Vec<CharShape>,
    para_shapes: &'a mut Vec<ParaShape>,
    font_indices: &'a mut BTreeMap<String, FontIndex>,
}

fn build_style_entry(
    ctx: &mut RegistryBuildCtx<'_>,
    style_name: &str,
    partial_style: &PartialStyle,
) -> BlueprintResult<StyleEntry> {
    let char_shape = resolve_char_shape(style_name, partial_style)?;
    let para_shape = resolve_para_shape(style_name, partial_style, ctx.numberings, ctx.bullets)?;
    validate_tab_reference(style_name, para_shape.tab_def_id, ctx.tabs)?;

    let font_id = intern_font(ctx.fonts, ctx.font_indices, &char_shape.font)?;
    for font in [char_shape.font_latin.as_deref(), char_shape.font_east_asian.as_deref()]
        .into_iter()
        .flatten()
    {
        intern_font(ctx.fonts, ctx.font_indices, font)?;
    }
    let char_shape_id = CharShapeIndex::new(ctx.char_shapes.len());
    ctx.char_shapes.push(char_shape);

    let para_shape_id = ParaShapeIndex::new(ctx.para_shapes.len());
    ctx.para_shapes.push(para_shape);

    Ok(StyleEntry { style_ref: StyleRef::new(0), char_shape_id, para_shape_id, font_id })
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

fn resolve_para_shape(
    style_name: &str,
    partial_style: &PartialStyle,
    numberings: &[NumberingDef],
    bullets: &[BulletDef],
) -> BlueprintResult<ParaShape> {
    partial_style
        .para_shape
        .as_ref()
        .map_or_else(crate::style::PartialParaShape::default, Clone::clone)
        .resolve(style_name, numberings, bullets)
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
        ("numbered_item", &md.numbered_item),
        ("check_item", &md.check_item),
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

fn validate_numbering_definitions(numberings: &[NumberingDef]) -> BlueprintResult<()> {
    let mut seen = BTreeMap::new();
    for numbering in numberings {
        if seen.insert(numbering.id, ()).is_some() {
            return Err(BlueprintError::DuplicateNumberingDefinition { id: numbering.id });
        }
    }
    Ok(())
}

fn validate_bullet_definitions(bullets: &[BulletDef]) -> BlueprintResult<()> {
    let mut seen = BTreeMap::new();
    for bullet in bullets {
        if seen.insert(bullet.id, ()).is_some() {
            return Err(BlueprintError::DuplicateBulletDefinition { id: bullet.id });
        }
        match (bullet.checked_char.is_some(), bullet.para_head.checkable) {
            (true, false) => {
                return Err(BlueprintError::InvalidBulletDefinition {
                    id: bullet.id,
                    reason: "checked_char requires para_head.checkable=true".to_string(),
                });
            }
            (false, true) => {
                return Err(BlueprintError::InvalidBulletDefinition {
                    id: bullet.id,
                    reason: "checkable bullet definitions must provide checked_char".to_string(),
                });
            }
            (false, false) | (true, true) => {}
        }
    }
    Ok(())
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

fn validate_border_fill(value: &BorderFill) -> Result<(), FoundationError> {
    let border = &value.border;
    if [border.left, border.right, border.top, border.bottom].iter().any(|side| {
        !matches!(side.line_type, BorderLineType::None | BorderLineType::Solid)
            || side.width.is_some()
            || side.color.is_some()
    }) || value.fill != Fill::default()
    {
        return Err(FoundationError::InvalidField {
            field: "border_fills".into(),
            reason: "only None/Solid sides without width, color or fill are supported".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::PartialStyle;
    use crate::template::Template;
    use crate::template::{TemplateMeta, TemplateTabDef, TemplateTabStop};
    use hwpforge_core::{BulletDef, ParaHead};
    use hwpforge_foundation::{Alignment, HwpUnit, LineSpacingType, NumberFormatType};
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
            numberings: vec![],
            bullets: vec![],
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
    fn template_rejects_checkable_bullet_without_checked_char() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            bullets: vec![BulletDef {
                id: 7,
                bullet_char: "☐".into(),
                checked_char: None,
                use_image: false,
                para_head: ParaHead {
                    start: 0,
                    level: 1,
                    num_format: NumberFormatType::Digit,
                    text: String::new(),
                    checkable: true,
                },
            }],
            ..make_template(styles)
        };

        assert!(matches!(
            StyleRegistry::from_template(&template),
            Err(BlueprintError::InvalidBulletDefinition { id: 7, .. })
        ));
    }

    #[test]
    fn template_rejects_checked_char_without_checkable_flag() {
        let mut styles = IndexMap::new();
        styles.insert("body".to_string(), make_partial_style("Batang", 10.0));
        let template = Template {
            bullets: vec![BulletDef {
                id: 7,
                bullet_char: "☐".into(),
                checked_char: Some("☑".into()),
                use_image: false,
                para_head: ParaHead {
                    start: 0,
                    level: 1,
                    num_format: NumberFormatType::Digit,
                    text: String::new(),
                    checkable: false,
                },
            }],
            ..make_template(styles)
        };

        assert!(matches!(
            StyleRegistry::from_template(&template),
            Err(BlueprintError::InvalidBulletDefinition { id: 7, .. })
        ));
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
            style_ref: StyleRef::new(0),
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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
            numberings: vec![],
            bullets: vec![],
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

#[cfg(test)]
mod identity_index_tests {
    use super::*;

    // 이것을 실패시키는 것: 역색인 대신 전체 스타일 선형 검색 또는 등록 색인 갱신 누락.
    #[test]
    fn display_name_registration_uses_logarithmic_comparisons() {
        STYLE_COMPARISONS.with(|count| count.set(0));
        let mut r = StyleRegistry::with_fonts(vec![]);
        for i in 0..16_384 {
            let id = r
                .register_style(
                    format!("s{i}"),
                    CharShapeIndex::new(0),
                    ParaShapeIndex::new(0),
                    FontIndex::new(0),
                )
                .unwrap();
            r.set_style_display_name(id, "name").unwrap();
            assert_eq!(r.style_display_name(id), Some("name"));
        }
        let comparisons = STYLE_COMPARISONS.with(|count| count.get());
        eprintln!("display name comparisons: {comparisons}");
        assert!((16_384..2_000_000).contains(&comparisons));
        assert_eq!(r.style_positions.len(), 16_384);
    }

    // 이것을 실패시키는 것: 로드 색인 재구성 누락, 중복 identity의 마지막 항목 선택, wire에 색인 출력.
    #[test]
    fn loaded_index_keeps_first_identity_and_exact_wire() {
        let json = r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{"a":{"style_ref":2,"char_shape_id":0,"para_shape_id":0,"font_id":0},"b":{"style_ref":2,"char_shape_id":0,"para_shape_id":0,"font_id":0}}}"#;
        let mut r: StyleRegistry = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&r).unwrap(), json);
        r.fonts.clear();
        r.char_shapes.clear();
        r.para_shapes.clear();
        r.set_style_display_name(StyleRef::new(2), "first").unwrap();
        let id = r
            .register_style(
                "c".into(),
                CharShapeIndex::new(9),
                ParaShapeIndex::new(9),
                FontIndex::new(9),
            )
            .unwrap();
        assert_eq!(id, StyleRef::new(2));
        r.set_style_display_name(id, "updated").unwrap();
        assert_eq!(r.style_display_names.get("a").map(String::as_str), Some("updated"));
        assert_eq!(r.style_display_names.get("c"), None);
        assert_eq!(r.style_display_name(id), Some("updated"));
        let r2: StyleRegistry = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r2, r);
        assert_eq!(r2.clone().style_display_name(id), Some("updated"));
    }
}
