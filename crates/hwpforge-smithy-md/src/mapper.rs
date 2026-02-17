//! Markdown element to Core/Blueprint mapping utilities.

use std::collections::HashMap;

use hwpforge_blueprint::builtins::{builtin_default, builtin_gov_proposal};
use hwpforge_blueprint::error::BlueprintError;
use hwpforge_blueprint::inheritance::resolve_template;
use hwpforge_blueprint::registry::{StyleEntry, StyleRegistry};
use hwpforge_blueprint::template::Template;
use hwpforge_core::{ImageFormat, PageSettings};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

use crate::error::{MdError, MdResult};

/// Resolved style indices for a markdown semantic element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdStyleRef {
    /// Paragraph shape index.
    pub para_shape_id: ParaShapeIndex,
    /// Character shape index.
    pub char_shape_id: CharShapeIndex,
}

impl MdStyleRef {
    fn from_entry(entry: &StyleEntry) -> Self {
        Self { para_shape_id: entry.para_shape_id, char_shape_id: entry.char_shape_id }
    }
}

/// Resolved markdown mapping based on a blueprint template.
#[derive(Debug, Clone)]
pub struct MdMapping {
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
        ParagraphKind::Body
    }
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
}

/// Resolves template inheritance and compiles markdown mapping style indices.
///
/// Returns the mapping **and** the [`StyleRegistry`] so callers can pass it
/// downstream (e.g. to the HWPX encoder) without re-resolving the template.
pub fn resolve_mapping(template: &Template) -> MdResult<(MdMapping, StyleRegistry)> {
    let resolved = resolve_template_with_builtins(template)?;
    let registry = StyleRegistry::from_template(&resolved)?;
    let fallback = fallback_style(&registry)?;

    let md = resolved.markdown_mapping.as_ref();

    let body = resolve_style(&registry, md.and_then(|m| m.body.as_deref()), "body", fallback);
    let heading1 =
        resolve_style(&registry, md.and_then(|m| m.heading1.as_deref()), "heading1", body);
    let heading2 =
        resolve_style(&registry, md.and_then(|m| m.heading2.as_deref()), "heading2", heading1);
    let heading3 =
        resolve_style(&registry, md.and_then(|m| m.heading3.as_deref()), "heading3", heading2);
    let heading4 =
        resolve_style(&registry, md.and_then(|m| m.heading4.as_deref()), "heading4", heading3);
    let heading5 =
        resolve_style(&registry, md.and_then(|m| m.heading5.as_deref()), "heading5", heading4);
    let heading6 =
        resolve_style(&registry, md.and_then(|m| m.heading6.as_deref()), "heading6", heading5);
    let code = resolve_style(&registry, md.and_then(|m| m.code.as_deref()), "code", body);
    let blockquote =
        resolve_style(&registry, md.and_then(|m| m.blockquote.as_deref()), "blockquote", body);
    let list_item =
        resolve_style(&registry, md.and_then(|m| m.list_item.as_deref()), "list_item", body);

    let page_settings = resolved
        .page
        .as_ref()
        .map(hwpforge_blueprint::template::PageStyle::to_page_settings)
        .unwrap_or_default();

    Ok((
        MdMapping {
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
            page_settings,
        },
        registry,
    ))
}

fn resolve_template_with_builtins(template: &Template) -> MdResult<Template> {
    if template.meta.extends.is_none() {
        return Ok(template.clone());
    }

    let mut provider = HashMap::new();
    let default_template = builtin_default()?;
    provider.insert(default_template.meta.name.clone(), default_template);

    let gov_template = builtin_gov_proposal()?;
    provider.insert(gov_template.meta.name.clone(), gov_template);

    provider.insert(template.meta.name.clone(), template.clone());

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

fn fallback_style(registry: &StyleRegistry) -> MdResult<MdStyleRef> {
    if let Some(entry) = registry.get_style("body") {
        return Ok(MdStyleRef::from_entry(entry));
    }

    if let Some(entry) = registry.style_entries.values().next() {
        return Ok(MdStyleRef::from_entry(entry));
    }

    Err(MdError::TemplateResolution { detail: "resolved style registry is empty".to_string() })
}

fn resolve_style(
    registry: &StyleRegistry,
    mapped_style_name: Option<&str>,
    conventional_name: &str,
    fallback: MdStyleRef,
) -> MdStyleRef {
    if let Some(name) = mapped_style_name {
        if let Some(entry) = registry.get_style(name) {
            return MdStyleRef::from_entry(entry);
        }
    }

    if let Some(entry) = registry.get_style(conventional_name) {
        return MdStyleRef::from_entry(entry);
    }

    fallback
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
