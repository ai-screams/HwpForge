//! Markdown element to Core/Blueprint mapping utilities.

use hwpforge_blueprint::inheritance::TemplateProvider;
use hwpforge_blueprint::markdown::{
    apply_markdown_heading_levels, apply_markdown_list_levels, MarkdownListShapes,
};
use hwpforge_foundation::StyleRef;
use std::collections::HashMap;

use hwpforge_blueprint::builtins::{builtin_default, builtin_gov_proposal};
use hwpforge_blueprint::error::BlueprintError;
use hwpforge_blueprint::inheritance::resolve_template;
use hwpforge_blueprint::registry::{StyleEntry, StyleRegistry};
use hwpforge_blueprint::template::Template;
use hwpforge_core::{ImageFormat, ListItemKind, PageSettings, ParagraphListRef};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

use crate::error::{MdError, MdResult};
use crate::internal_styles::{ENDNOTE_STYLE_NAME, FOOTNOTE_STYLE_NAME};

/// Resolved style indices for a markdown semantic element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdStyleRef {
    /// Selected registry entry identity.
    pub style_ref: StyleRef,
    /// Paragraph shape index.
    pub para_shape_id: ParaShapeIndex,
    /// Character shape index.
    pub char_shape_id: CharShapeIndex,
}

impl MdStyleRef {
    fn from_entry(entry: &StyleEntry) -> Self {
        Self {
            style_ref: entry.style_ref(),
            para_shape_id: entry.para_shape_id,
            char_shape_id: entry.char_shape_id,
        }
    }
}

const LIST_LEVELS: usize = ParagraphListRef::MAX_LEVEL as usize + 1;

/// The list item paragraph styles the template's Blueprint derived
/// ([`MarkdownListShapes`]), one per level (0 to 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdListStyles {
    /// Bullet items, `None` when the template defines no bullet list.
    pub bullet: Option<[MdStyleRef; LIST_LEVELS]>,
    /// Numbered items, `None` when the template defines no numbering list.
    pub number: Option<[MdStyleRef; LIST_LEVELS]>,
    /// Unchecked task items, `None` when the template defines no check list.
    pub unchecked: Option<[MdStyleRef; LIST_LEVELS]>,
    /// Checked task items, `None` when the template defines no check list.
    pub checked: Option<[MdStyleRef; LIST_LEVELS]>,
    /// An item's later paragraphs and its tables.
    pub continuation: [MdStyleRef; LIST_LEVELS],
    /// A code block an item holds.
    pub code_continuation: [MdStyleRef; LIST_LEVELS],
    /// An item of a kind the template defines no list for.
    pub plain_fallback: MdStyleRef,
}

impl MdListStyles {
    fn from_shapes(shapes: &MarkdownListShapes) -> Self {
        let all = |entries: &[StyleEntry; LIST_LEVELS]| entries.map(|e| MdStyleRef::from_entry(&e));
        Self {
            bullet: shapes.bullet.as_ref().map(all),
            number: shapes.number.as_ref().map(all),
            unchecked: shapes.unchecked.as_ref().map(all),
            checked: shapes.checked.as_ref().map(all),
            continuation: all(&shapes.continuation),
            code_continuation: all(&shapes.code_continuation),
            plain_fallback: MdStyleRef::from_entry(&shapes.plain_fallback),
        }
    }
}

/// Resolved markdown mapping based on a blueprint template.
#[derive(Debug, Clone)]
pub struct MdMapping {
    pub warnings: Vec<crate::MdWarning>,
    /// Body text style.
    pub body: MdStyleRef,
    /// H1 style.
    pub heading1: MdStyleRef,
    /// H2 style.
    pub heading2: MdStyleRef,
    /// H3 style.
    pub heading3: MdStyleRef,
    /// H4 style.
    pub heading4: MdStyleRef,
    /// H5 style.
    pub heading5: MdStyleRef,
    /// H6 style.
    pub heading6: MdStyleRef,
    /// Code style.
    pub code: MdStyleRef,
    /// Blockquote style.
    pub blockquote: MdStyleRef,
    /// List item style.
    pub list_item: MdStyleRef,
    /// List item, continuation and code-continuation styles by level.
    pub lists: MdListStyles,
    /// Page settings used when creating sections.
    pub page_settings: PageSettings,
}

impl MdMapping {
    /// Returns the heading style for a heading level.
    pub fn heading(&self, level: u32) -> MdStyleRef {
        match level {
            1 => self.heading1,
            2 => self.heading2,
            3 => self.heading3,
            4 => self.heading4,
            5 => self.heading5,
            6.. => self.heading6,
            0 => self.body,
        }
    }

    /// The style of a list item's first paragraph at `level` (0 to 9): a
    /// task item's (`checked` is `Some`) or its kind's, `None` when the
    /// template defines no list of that kind.
    pub fn list_item_style(
        &self,
        kind: ListItemKind,
        checked: Option<bool>,
        level: u8,
    ) -> Option<MdStyleRef> {
        let shapes = match (checked, kind) {
            (Some(true), _) => self.lists.checked,
            (Some(false), _) => self.lists.unchecked,
            (None, ListItemKind::Bullet) => self.lists.bullet,
            (None, ListItemKind::Number) => self.lists.number,
        };
        shapes.map(|levels| levels[usize::from(level.min(ParagraphListRef::MAX_LEVEL))])
    }

    /// Returns the continuation-paragraph style for the given nesting level.
    pub fn list_continuation(&self, level: u8) -> MdStyleRef {
        self.lists.continuation[usize::from(level.min(ParagraphListRef::MAX_LEVEL))]
    }

    /// The style of a code block a list item at `level` holds.
    pub fn code_continuation(&self, level: u8) -> MdStyleRef {
        self.lists.code_continuation[usize::from(level.min(ParagraphListRef::MAX_LEVEL))]
    }

    /// Returns the continuation nesting level for the para shape, if it is one
    /// of the markdown-generated continuation paragraph shapes.
    pub fn continuation_level(&self, para_shape_id: ParaShapeIndex) -> Option<u8> {
        level_of(&self.lists.continuation, para_shape_id)
    }

    /// The level of the list item whose code block has this para shape.
    pub fn code_continuation_level(&self, para_shape_id: ParaShapeIndex) -> Option<u8> {
        level_of(&self.lists.code_continuation, para_shape_id)
    }

    /// Classifies paragraph shape IDs for markdown encoding.
    pub fn classify_para_shape(&self, para_shape_id: ParaShapeIndex) -> ParagraphKind {
        if para_shape_id == self.heading1.para_shape_id {
            return ParagraphKind::Heading(1);
        }
        if para_shape_id == self.heading2.para_shape_id {
            return ParagraphKind::Heading(2);
        }
        if para_shape_id == self.heading3.para_shape_id {
            return ParagraphKind::Heading(3);
        }
        if para_shape_id == self.heading4.para_shape_id {
            return ParagraphKind::Heading(4);
        }
        if para_shape_id == self.heading5.para_shape_id {
            return ParagraphKind::Heading(5);
        }
        if para_shape_id == self.heading6.para_shape_id {
            return ParagraphKind::Heading(6);
        }
        if para_shape_id == self.code.para_shape_id {
            return ParagraphKind::Code;
        }
        if para_shape_id == self.blockquote.para_shape_id {
            return ParagraphKind::BlockQuote;
        }
        if para_shape_id == self.list_item.para_shape_id {
            return ParagraphKind::ListItem;
        }
        if let Some(level) = self.continuation_level(para_shape_id) {
            return ParagraphKind::ListContinuation(level);
        }
        ParagraphKind::Body
    }
}

fn level_of(levels: &[MdStyleRef; LIST_LEVELS], para_shape_id: ParaShapeIndex) -> Option<u8> {
    levels.iter().position(|s| s.para_shape_id == para_shape_id).map(|level| level as u8)
}

/// Paragraph semantic classification used by encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphKind {
    /// Body paragraph.
    Body,
    /// Heading paragraph with depth level.
    Heading(u8),
    /// Code block paragraph.
    Code,
    /// Blockquote paragraph.
    BlockQuote,
    /// List item paragraph.
    ListItem,
    /// Continuation paragraph within a list item.
    ListContinuation(u8),
}

/// Resolves template inheritance and compiles markdown mapping style indices.
///
/// Returns the mapping **and** the [`StyleRegistry`] so callers can pass it
/// downstream (e.g. to the HWPX encoder) without re-resolving the template.
pub fn resolve_mapping(template: &Template) -> MdResult<(MdMapping, StyleRegistry)> {
    let resolved = resolve_template_with_builtins(template)?;
    let mut registry = StyleRegistry::from_template(&resolved)?;
    let names = resolved.markdown_mapping.as_ref().expect("resolved mapping");
    let (heading_shapes, conflicts) = apply_markdown_heading_levels(&mut registry, names)?;
    let lookup = |name: &Option<String>| {
        MdStyleRef::from_entry(
            registry.get_style(name.as_deref().expect("resolved name")).expect("validated name"),
        )
    };
    let body = lookup(&names.body);
    let code = lookup(&names.code);
    let blockquote = lookup(&names.blockquote);
    let list_item = lookup(&names.list_item);
    let [heading1, heading2, heading3, heading4, heading5, heading6] = [
        &names.heading1,
        &names.heading2,
        &names.heading3,
        &names.heading4,
        &names.heading5,
        &names.heading6,
    ]
    .map(lookup);
    let [heading1, heading2, heading3, heading4, heading5, heading6] =
        std::array::from_fn::<_, 6, _>(|i| MdStyleRef {
            para_shape_id: heading_shapes[i],
            ..[heading1, heading2, heading3, heading4, heading5, heading6][i]
        });
    let lists = MdListStyles::from_shapes(&apply_markdown_list_levels(&mut registry, names)?);
    install_note_styles(&mut registry, body)?;

    let page_settings = resolved
        .page
        .as_ref()
        .map(hwpforge_blueprint::template::PageStyle::to_page_settings)
        .unwrap_or_default();

    Ok((
        MdMapping {
            warnings: conflicts
                .into_iter()
                .map(|(level, style_name)| crate::MdWarning::HeadingListConflict {
                    level,
                    style_name,
                })
                .collect(),
            body,
            heading1,
            heading2,
            heading3,
            heading4,
            heading5,
            heading6,
            code,
            blockquote,
            list_item,
            lists,
            page_settings,
        },
        registry,
    ))
}

/// 각주/미주 본문 스타일을 파생 등록한다 (한컴 default 스타일 정합).
///
/// 한컴 native 실측(2026-08-27): 각주 본문 = 9pt(charPr height 900),
/// 첫 줄 내어쓰기 −655 (−6.55 pt), 줄간격 130%. 템플릿이 같은 이름을 이미 제공하면
/// (향후 커스텀) 그것을 존중하고 파생을 건너뛴다.
fn install_note_styles(registry: &mut StyleRegistry, body: MdStyleRef) -> MdResult<()> {
    use hwpforge_foundation::LineSpacingType;

    if registry.get_style(FOOTNOTE_STYLE_NAME).is_some()
        && registry.get_style(ENDNOTE_STYLE_NAME).is_some()
    {
        return Ok(());
    }

    let font_id = registry
        .style_entries()
        .values()
        .find(|e| e.style_ref() == body.style_ref)
        .map(|e| e.font_id)
        .ok_or_else(|| MdError::TemplateResolution {
            detail: "body style entry missing for note style derivation".to_string(),
        })?;
    let base_char = registry
        .char_shape(body.char_shape_id)
        .ok_or_else(|| MdError::TemplateResolution {
            detail: "body char shape missing for note style derivation".to_string(),
        })?
        .clone();
    let base_para = registry
        .para_shape(body.para_shape_id)
        .ok_or_else(|| MdError::TemplateResolution {
            detail: "body para shape missing for note style derivation".to_string(),
        })?
        .clone();

    let mut note_char = base_char;
    note_char.size = HwpUnit::new(900).unwrap_or(note_char.size);

    let mut note_para = base_para;
    note_para.indent_first_line = HwpUnit::new(-655).unwrap_or(HwpUnit::ZERO);
    note_para.line_spacing_type = LineSpacingType::Percentage;
    note_para.line_spacing_value = 130.0;
    note_para.list = None;

    let char_shape_id = CharShapeIndex::new(registry.char_shapes.len());
    registry.char_shapes.push(note_char);
    let para_shape_id = ParaShapeIndex::new(registry.para_shapes.len());
    registry.para_shapes.push(note_para);

    for name in [FOOTNOTE_STYLE_NAME, ENDNOTE_STYLE_NAME] {
        if registry.get_style(name).is_none() {
            registry
                .register_style(name.to_string(), char_shape_id, para_shape_id, font_id)
                .expect("valid note style");
        }
    }
    Ok(())
}

fn resolve_template_with_builtins(template: &Template) -> MdResult<Template> {
    if template.meta.extends.is_none() {
        return Ok(resolve_template(template, &HashMap::new())?);
    }

    struct Builtins {
        slots: HashMap<String, Template>,
        user: String,
    }
    impl TemplateProvider for Builtins {
        fn get_template(&self, name: &str) -> Option<&Template> {
            self.slots.get(name)
        }
        fn is_builtin_template(&self, name: &str) -> bool {
            name != self.user && self.slots.contains_key(name)
        }
    }
    let mut provider = HashMap::new();
    let default_template = builtin_default()?;
    provider.insert(default_template.meta.name.clone(), default_template);

    let gov_template = builtin_gov_proposal()?;
    provider.insert(gov_template.meta.name.clone(), gov_template);

    provider.insert(template.meta.name.clone(), template.clone());

    let provider = Builtins { slots: provider, user: template.meta.name.clone() };
    match resolve_template(template, &provider) {
        Ok(t) => Ok(t),
        Err(BlueprintError::TemplateNotFound { name }) => Err(MdError::TemplateResolution {
            detail: format!(
                "parent template '{name}' is not available in builtin resolver; pass a pre-resolved template"
            ),
        }),
        Err(err) => Err(err.into()),
    }
}

/// Guesses image format from file extension.
pub fn image_format_from_path(path: &str) -> ImageFormat {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        ImageFormat::Png
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ImageFormat::Jpeg
    } else if lower.ends_with(".gif") {
        ImageFormat::Gif
    } else if lower.ends_with(".bmp") {
        ImageFormat::Bmp
    } else if lower.ends_with(".wmf") {
        ImageFormat::Wmf
    } else if lower.ends_with(".emf") {
        ImageFormat::Emf
    } else {
        ImageFormat::Unknown(path.rsplit('.').next().unwrap_or("image").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::{builtin_default, builtin_gov_proposal};

    #[test]
    fn resolve_default_template_mapping() {
        let template = builtin_default().unwrap();
        let (mapping, registry) = resolve_mapping(&template).unwrap();

        // IndexMap preserves YAML declaration order: body=0, heading1=1, heading2=2, ...
        assert_eq!(mapping.body.char_shape_id.get(), 0);
        assert_eq!(mapping.heading1.char_shape_id.get(), 1);
        assert_eq!(mapping.heading2.char_shape_id.get(), 2);
        assert!(registry.font_count() > 0);
    }

    #[test]
    fn resolve_gov_template_via_builtin_inheritance() {
        let template = builtin_gov_proposal().unwrap();
        let (mapping, _registry) = resolve_mapping(&template).unwrap();
        assert!(mapping.heading1.char_shape_id.get() > 0);
        assert!(mapping.page_settings.margin_left.to_mm() >= 29.9);
    }

    #[test]
    fn unknown_parent_requires_pre_resolved_template() {
        let mut template = builtin_default().unwrap();
        template.meta.extends = Some("custom_parent".to_string());

        let err = resolve_mapping(&template).unwrap_err();
        match err {
            MdError::TemplateResolution { detail } => {
                assert!(detail.contains("custom_parent"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn classify_heading_shape() {
        let template = builtin_default().unwrap();
        let (mapping, _registry) = resolve_mapping(&template).unwrap();
        assert_eq!(
            mapping.classify_para_shape(mapping.heading2.para_shape_id),
            ParagraphKind::Heading(2)
        );
    }

    #[test]
    fn image_format_guessing() {
        assert_eq!(image_format_from_path("a.png"), ImageFormat::Png);
        assert_eq!(image_format_from_path("a.jpeg"), ImageFormat::Jpeg);
        assert_eq!(
            image_format_from_path("a.unknown"),
            ImageFormat::Unknown("unknown".to_string())
        );
    }
}
