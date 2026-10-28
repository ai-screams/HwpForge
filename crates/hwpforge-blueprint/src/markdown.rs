//! Shared Markdown role selection and heading paragraph semantics.
use crate::{
    error::{BlueprintError, BlueprintResult},
    registry::StyleRegistry,
    template::{MarkdownMapping, Template},
};
use hwpforge_core::ParagraphListRef;
use hwpforge_foundation::ParaShapeIndex;
use std::collections::HashSet;

/// Selects concrete style names for every Markdown role.
///
/// Explicit mappings must refer to existing definitions. Missing conventions
/// fall back to the preceding heading or body, then the first declared entry.
pub fn resolve_markdown_style_names(template: &Template) -> BlueprintResult<MarkdownMapping> {
    select_names(template, &HashSet::new())
}

pub(crate) fn select_names(
    template: &Template,
    excluded: &HashSet<String>,
) -> BlueprintResult<MarkdownMapping> {
    let first = template.styles.keys().next().ok_or(BlueprintError::EmptyStyleMap)?;
    let mut result = template.markdown_mapping.clone().unwrap_or_default();
    let mut previous = first.clone();
    for (role, value) in [
        ("body", &mut result.body),
        ("heading1", &mut result.heading1),
        ("heading2", &mut result.heading2),
        ("heading3", &mut result.heading3),
        ("heading4", &mut result.heading4),
        ("heading5", &mut result.heading5),
        ("heading6", &mut result.heading6),
    ] {
        if let Some(name) = value.as_ref() {
            if !template.styles.contains_key(name) {
                return Err(BlueprintError::InvalidMappingReference {
                    mapping_field: role.into(),
                    style_name: name.clone(),
                });
            }
        } else {
            *value = Some(if template.styles.contains_key(role) && !excluded.contains(role) {
                role.into()
            } else {
                previous.clone()
            });
        }
        previous = value.clone().unwrap();
    }
    for (role, value) in [
        ("code", &mut result.code),
        ("blockquote", &mut result.blockquote),
        ("list_item", &mut result.list_item),
    ] {
        if let Some(name) = value.as_ref() {
            if !template.styles.contains_key(name) {
                return Err(BlueprintError::InvalidMappingReference {
                    mapping_field: role.into(),
                    style_name: name.clone(),
                });
            }
        } else {
            *value = Some(if template.styles.contains_key(role) {
                role.into()
            } else {
                result.body.clone().unwrap()
            });
        }
    }
    Ok(result)
}

/// Applies heading levels after all role names have been frozen.
///
/// Returns paragraph shapes for H1–H6 and conflicting (level, style name) pairs. Shared roles and explicit list
/// definitions retain their entry shapes; exclusive unlisted headings receive
/// a fresh shape. The original paragraph shapes are never modified.
/// Returns an error for missing roles, unknown names, or missing paragraph shapes.
pub fn apply_markdown_heading_levels(
    registry: &mut StyleRegistry,
    names: &MarkdownMapping,
) -> BlueprintResult<([ParaShapeIndex; 6], Vec<(u8, String)>)> {
    let headings = [
        &names.heading1,
        &names.heading2,
        &names.heading3,
        &names.heading4,
        &names.heading5,
        &names.heading6,
    ];
    let other = [&names.body, &names.code, &names.blockquote, &names.list_item];
    // 이것을 실패시키는 것: 누락 역할 또는 없는 이름을 expect로 처리.
    for (role, name) in [
        ("heading1", &names.heading1),
        ("heading2", &names.heading2),
        ("heading3", &names.heading3),
        ("heading4", &names.heading4),
        ("heading5", &names.heading5),
        ("heading6", &names.heading6),
        ("body", &names.body),
        ("code", &names.code),
        ("blockquote", &names.blockquote),
        ("list_item", &names.list_item),
    ] {
        if name.as_deref().and_then(|n| registry.get_style(n)).is_none() {
            return Err(BlueprintError::InvalidMappingReference {
                mapping_field: role.into(),
                style_name: name.clone().unwrap_or_default(),
            });
        }
    }
    let mut conflicts = Vec::new();
    let mut shapes = [ParaShapeIndex::new(0); 6];
    for index in 0..6 {
        let name = headings[index].as_deref().unwrap();
        let entry =
            *registry.get_style(name).ok_or_else(|| BlueprintError::InvalidMappingReference {
                mapping_field: format!("heading{}", index + 1),
                style_name: name.into(),
            })?;
        let mut para = registry
            .para_shape(entry.para_shape_id)
            .ok_or_else(|| BlueprintError::StyleResolution {
                style_name: name.into(),
                field: "para_shape".into(),
            })?
            .clone();
        let target = Some(ParagraphListRef::Outline { level: index as u8 });
        if para.list == target {
            shapes[index] = entry.para_shape_id;
            continue;
        }
        let exclusive = headings.iter().filter(|n| n.as_deref() == Some(name)).count() == 1
            && !other.iter().any(|n| n.as_deref() == Some(name));
        let unlisted = para.list.is_none();
        if !unlisted {
            conflicts.push((index as u8 + 1, name.to_owned()));
        }
        para.list = target;
        let id = ParaShapeIndex::new(registry.para_shapes.len());
        registry.para_shapes.push(para);
        if exclusive && unlisted {
            registry.register_style(name.into(), entry.char_shape_id, id, entry.font_id)?;
        }
        shapes[index] = id;
    }
    Ok((shapes, conflicts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_or_unknown_mapping_returns_error() {
        // 이것을 실패시키는 것: 역할 검증 삭제 또는 오류를 panic으로 교체.
        for name in [None, Some("absent".to_owned())] {
            let mut registry = StyleRegistry::with_fonts(Vec::new());
            let names = MarkdownMapping { heading1: name.clone(), ..Default::default() };
            let error = apply_markdown_heading_levels(&mut registry, &names).unwrap_err();
            assert!(
                matches!(error, BlueprintError::InvalidMappingReference { mapping_field, style_name }
                if mapping_field == "heading1" && style_name == name.unwrap_or_default())
            );
        }
    }
}
