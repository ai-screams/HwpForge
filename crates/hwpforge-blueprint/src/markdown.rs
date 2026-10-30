//! Shared Markdown role selection and heading and list paragraph semantics.
use crate::{
    error::{BlueprintError, BlueprintResult},
    registry::{StyleEntry, StyleRegistry},
    style::ParaShape,
    template::{MarkdownMapping, Template},
};
use hwpforge_core::ParagraphListRef;
use hwpforge_foundation::{BulletIndex, HwpUnit, ParaShapeIndex};
use std::collections::HashSet;

/// List levels a Markdown list item can have (0 to 9).
const LEVELS: usize = ParagraphListRef::MAX_LEVEL as usize + 1;

const LIST_CONTINUATION_STYLE_PREFIX: &str = "__hwpforge_md_list_continuation_level_";
const CODE_CONTINUATION_STYLE_PREFIX: &str = "__hwpforge_md_code_continuation_level_";
/// The list item paragraph of a kind the template defines no list for.
const LIST_PLAIN_STYLE_NAME: &str = "__hwpforge_md_list_plain";

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
    // A numbered or task item role is only what the mapping names or a style
    // of the role's own name: no other style stands in, so a template with
    // no such style does not support that kind.
    // 이것을 실패시키는 것: body 로 대신하기(`list_roles_fall_back_only_to_their_own_name`).
    for (role, value) in
        [("numbered_item", &mut result.numbered_item), ("check_item", &mut result.check_item)]
    {
        if let Some(name) = value.as_ref() {
            if !template.styles.contains_key(name) {
                return Err(BlueprintError::InvalidMappingReference {
                    mapping_field: role.into(),
                    style_name: name.clone(),
                });
            }
        } else if template.styles.contains_key(role) {
            *value = Some(role.into());
        }
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
    let other = [
        &names.body,
        &names.code,
        &names.blockquote,
        &names.list_item,
        &names.numbered_item,
        &names.check_item,
    ];
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

/// The style name of a Markdown list item's continuation paragraph (its
/// second and later paragraphs, a table it holds) at `level`.
pub fn list_continuation_style_name(level: u8) -> String {
    format!("{LIST_CONTINUATION_STYLE_PREFIX}{level}")
}

/// The level of a [`list_continuation_style_name`], `None` for any other name.
pub fn parse_list_continuation_style_name(name: &str) -> Option<u8> {
    parse_level(name, LIST_CONTINUATION_STYLE_PREFIX)
}

/// The style name of a code block a Markdown list item at `level` holds.
pub fn code_continuation_style_name(level: u8) -> String {
    format!("{CODE_CONTINUATION_STYLE_PREFIX}{level}")
}

/// The level of a [`code_continuation_style_name`], `None` for any other name.
pub fn parse_code_continuation_style_name(name: &str) -> Option<u8> {
    parse_level(name, CODE_CONTINUATION_STYLE_PREFIX)
}

fn parse_level(name: &str, prefix: &str) -> Option<u8> {
    let level = name.strip_prefix(prefix)?.parse::<u8>().ok()?;
    (level <= ParagraphListRef::MAX_LEVEL).then_some(level)
}

/// The paragraph styles of Markdown list items, one per level (0 to 9),
/// derived by [`apply_markdown_list_levels`]. A kind the template defines no
/// list for is `None`; its items are written with [`Self::plain_fallback`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MarkdownListShapes {
    /// Bullet items.
    pub bullet: Option<[StyleEntry; LEVELS]>,
    /// Numbered items.
    pub number: Option<[StyleEntry; LEVELS]>,
    /// Unchecked task items.
    pub unchecked: Option<[StyleEntry; LEVELS]>,
    /// Checked task items.
    pub checked: Option<[StyleEntry; LEVELS]>,
    /// An item's later paragraphs and the tables it holds: at its text's
    /// left margin, no list ([`list_continuation_style_name`]).
    pub continuation: [StyleEntry; LEVELS],
    /// A code block an item holds: the code role's look at the item's left
    /// margin, no list ([`code_continuation_style_name`]).
    pub code_continuation: [StyleEntry; LEVELS],
    /// An item of a kind the template defines no list for: the list item's
    /// look, no list.
    pub plain_fallback: StyleEntry,
}

/// Derives the paragraph shapes of Markdown list items from the roles in
/// `names` (after [`resolve_markdown_style_names`] froze them).
///
/// Each kind's items take their role's look; every kind's left margin at
/// level `n` is the `list_item` margin plus `n` steps (the `list_item` left
/// margin, or 20pt when it has none) and its first-line indent is the
/// `list_item`'s. Bullet items use the `bullet_levels` bullet of their level,
/// or the `list_item` bullet at every level. The role entries' own shapes
/// are not changed; applying again to the same registry derives nothing new.
///
/// # Errors
///
/// [`BlueprintError::InvalidMappingReference`] for a missing `list_item` or
/// `code` role or an unknown style name; [`BlueprintError::StyleResolution`]
/// when a role's list is of another kind (`numbered_item`: a numbering
/// other than the first, which is the outline's, `check_item`: a checkable
/// bullet), or `bullet_levels` is not ten ids or is given with no bullet
/// list on `list_item`;
/// [`BlueprintError::InvalidListReference`] and
/// [`BlueprintError::InvalidPlainBulletDefinition`] for a `bullet_levels` id
/// that is not a declared plain bullet.
pub fn apply_markdown_list_levels(
    registry: &mut StyleRegistry,
    names: &MarkdownMapping,
) -> BlueprintResult<MarkdownListShapes> {
    let (list_entry, list_para) = role(registry, "list_item", &names.list_item)?;
    let (code_entry, code_para) = role(registry, "code", &names.code)?;
    let step = if list_para.indent_left.as_i32() > 0 {
        list_para.indent_left
    } else {
        HwpUnit::from_pt(20.0)?
    };
    let left = |level: usize| list_para.indent_left + step * level as i32;
    let first_line = list_para.indent_first_line;
    let mismatch = |style_name: &Option<String>, field: &str| BlueprintError::StyleResolution {
        style_name: style_name.clone().unwrap_or_default(),
        field: field.into(),
    };

    // The list item role is the bullet item only with a bullet list; with
    // none or another (a heading style shared as the list item), the
    // template has no bullet items.
    let bullet = match list_para.list {
        Some(ParagraphListRef::Bullet { bullet_id, .. }) => Some(bullet_id),
        _ => None,
    };
    let bullet_ids = match (&names.bullet_levels, bullet) {
        (Some(_), None) => {
            return Err(mismatch(
                &names.list_item,
                "para_shape.list (a bullet list, for markdown_mapping.bullet_levels)",
            ));
        }
        (Some(levels), Some(_)) => Some(bullet_levels(registry, levels)?),
        (None, base) => base.map(|id| [id; LEVELS]),
    };

    let number = match names.numbered_item.as_ref() {
        None => None,
        Some(_) => {
            let (entry, para) = role(registry, "numbered_item", &names.numbered_item)?;
            // The first numbering is the outline's (headings refer to it), so
            // a numbered item there would number with the headings.
            // 이것을 실패시키는 것: 이 검사 삭제(`numbered_item_may_not_use_the_outline_slot`).
            match para.list {
                Some(ParagraphListRef::Number { numbering_id, .. }) if numbering_id.get() != 0 => {
                    numbering_levels(registry, &names.numbered_item, numbering_id)?;
                    Some((entry, para, numbering_id))
                }
                _ => {
                    return Err(mismatch(
                        &names.numbered_item,
                        "para_shape.list (a numbering other than the first, the outline's)",
                    ));
                }
            }
        }
    };
    let check = match names.check_item.as_ref() {
        None => None,
        Some(_) => {
            let (entry, para) = role(registry, "check_item", &names.check_item)?;
            match para.list {
                Some(ParagraphListRef::CheckBullet { bullet_id, .. }) => {
                    Some((entry, para, bullet_id))
                }
                _ => return Err(mismatch(&names.check_item, "para_shape.list (a check bullet)")),
            }
        }
    };

    let mut items = |entry: StyleEntry, para: &ParaShape, list: &dyn Fn(u8) -> ParagraphListRef| {
        std::array::from_fn(|level| {
            let mut shape = para.clone();
            shape.indent_left = left(level);
            shape.indent_first_line = first_line;
            shape.list = Some(list(level as u8));
            let mut item = entry;
            item.para_shape_id = list_shape(registry, shape);
            item
        })
    };
    let bullet = bullet_ids.map(|ids| {
        items(list_entry, &list_para, &|level| ParagraphListRef::Bullet {
            bullet_id: ids[usize::from(level)],
            level,
        })
    });
    let number = number.map(|(entry, para, numbering_id)| {
        items(entry, &para, &|level| ParagraphListRef::Number { numbering_id, level })
    });
    let unchecked = check.as_ref().map(|(entry, para, bullet_id)| {
        items(*entry, para, &|level| ParagraphListRef::CheckBullet {
            bullet_id: *bullet_id,
            level,
            checked: false,
        })
    });
    let checked = check.as_ref().map(|(entry, para, bullet_id)| {
        items(*entry, para, &|level| ParagraphListRef::CheckBullet {
            bullet_id: *bullet_id,
            level,
            checked: true,
        })
    });

    let unlisted = |para: &ParaShape, level: usize| {
        let mut shape = para.clone();
        shape.indent_left = left(level);
        shape.indent_first_line = HwpUnit::ZERO;
        shape.list = None;
        shape
    };
    let mut continuation = [list_entry; LEVELS];
    let mut code_continuation = [code_entry; LEVELS];
    for level in 0..LEVELS {
        continuation[level] = named(
            registry,
            list_continuation_style_name(level as u8),
            list_entry,
            unlisted(&list_para, level),
        )?;
        code_continuation[level] = named(
            registry,
            code_continuation_style_name(level as u8),
            code_entry,
            unlisted(&code_para, level),
        )?;
    }
    let plain = ParaShape { list: None, ..list_para };
    let plain_fallback = named(registry, LIST_PLAIN_STYLE_NAME.into(), list_entry, plain)?;

    Ok(MarkdownListShapes {
        bullet,
        number,
        unchecked,
        checked,
        continuation,
        code_continuation,
        plain_fallback,
    })
}

/// A role's entry and paragraph shape.
fn role(
    registry: &StyleRegistry,
    field: &str,
    name: &Option<String>,
) -> BlueprintResult<(StyleEntry, ParaShape)> {
    let entry = name.as_deref().and_then(|n| registry.get_style(n)).copied().ok_or_else(|| {
        BlueprintError::InvalidMappingReference {
            mapping_field: field.into(),
            style_name: name.clone().unwrap_or_default(),
        }
    })?;
    let para = registry.para_shape(entry.para_shape_id).cloned().ok_or_else(|| {
        BlueprintError::StyleResolution {
            style_name: name.clone().unwrap_or_default(),
            field: "para_shape".into(),
        }
    })?;
    Ok((entry, para))
}

/// A numbered item refers to its numbering's level `n` by position, so the
/// numbering must define every level a numbered item can have, in order:
/// level `n` at position `n` with paraHead level `n + 1`. A shorter or
/// reordered definition is rejected, never filled in.
/// 이것을 실패시키는 것: 이 검사 삭제(`numbering_without_every_level_is_rejected`).
fn numbering_levels(
    registry: &StyleRegistry,
    style_name: &Option<String>,
    numbering_id: hwpforge_foundation::NumberingIndex,
) -> BlueprintResult<()> {
    let numbering = &registry.numberings[numbering_id.get()];
    for level in 0..LEVELS {
        if numbering.levels.get(level).map(|head| head.level) != Some(level as u32 + 1) {
            return Err(BlueprintError::StyleResolution {
                style_name: style_name.clone().unwrap_or_default(),
                field: format!(
                    "numbering {} paraHead level {} for list level {level} (levels 1..=10 in order)",
                    numbering.id,
                    level + 1
                ),
            });
        }
    }
    Ok(())
}

/// The bullets of `bullet_levels`, by declared id: ten plain bullets.
fn bullet_levels(registry: &StyleRegistry, ids: &[u32]) -> BlueprintResult<[BulletIndex; LEVELS]> {
    const FIELD: &str = "markdown_mapping.bullet_levels";
    // 이것을 실패시키는 것: 길이 검사 삭제(`invalid_list_roles_are_rejected`).
    if ids.len() != LEVELS {
        return Err(BlueprintError::StyleResolution {
            style_name: FIELD.into(),
            field: format!("one bullet id per level 0..=9 (10 ids, not {})", ids.len()),
        });
    }
    let mut out = [BulletIndex::new(0); LEVELS];
    for (slot, &id) in out.iter_mut().zip(ids) {
        let index =
            registry.bullets.iter().position(|bullet| bullet.id == id).ok_or_else(|| {
                BlueprintError::InvalidListReference {
                    style_name: FIELD.into(),
                    kind: "bullet".into(),
                    id,
                }
            })?;
        if registry.bullets[index].is_checkable() {
            return Err(BlueprintError::InvalidPlainBulletDefinition {
                style_name: FIELD.into(),
                bullet_id: id,
            });
        }
        *slot = BulletIndex::new(index);
    }
    Ok(out)
}

/// A list item's paragraph shape: an equal one with the same list, or a
/// new one. Its list tells it apart, so sharing an equal shape keeps a
/// second application from adding shapes.
fn list_shape(registry: &mut StyleRegistry, shape: ParaShape) -> ParaShapeIndex {
    if let Some(index) = registry.para_shapes.iter().position(|p| *p == shape) {
        return ParaShapeIndex::new(index);
    }
    registry.para_shapes.push(shape);
    ParaShapeIndex::new(registry.para_shapes.len() - 1)
}

/// The entry registered under a reserved name, with its own paragraph shape:
/// a paragraph is told to be one by that name, so the shape is never shared
/// with another, even an equal one. An entry already registered under the
/// name is kept.
fn named(
    registry: &mut StyleRegistry,
    name: String,
    base: StyleEntry,
    shape: ParaShape,
) -> BlueprintResult<StyleEntry> {
    if let Some(entry) = registry.get_style(&name) {
        return Ok(*entry);
    }
    registry.para_shapes.push(shape);
    let id = ParaShapeIndex::new(registry.para_shapes.len() - 1);
    registry.register_style(name.clone(), base.char_shape_id, id, base.font_id)?;
    Ok(*registry.get_style(&name).expect("registered above"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::builtins::{builtin_default, builtin_gov_proposal};
    use crate::inheritance::resolve_template;
    use hwpforge_core::NumberingDef;
    use hwpforge_foundation::NumberingIndex;
    use std::collections::HashMap;

    /// The registry, its frozen roles and its list shapes, as the Markdown
    /// decoder derives them: headings first, then lists.
    fn derived(
        template: &Template,
        others: &[Template],
    ) -> BlueprintResult<(StyleRegistry, MarkdownListShapes)> {
        let provider: HashMap<String, Template> =
            others.iter().map(|t| (t.meta.name.clone(), t.clone())).collect();
        let resolved = resolve_template(template, &provider)?;
        let mut registry = StyleRegistry::from_template(&resolved)?;
        let names = resolved.markdown_mapping.clone().unwrap();
        apply_markdown_heading_levels(&mut registry, &names)?;
        let shapes = apply_markdown_list_levels(&mut registry, &names)?;
        Ok((registry, shapes))
    }

    fn name_of(registry: &StyleRegistry, entry: StyleEntry) -> &str {
        registry.style_entries().iter().find(|(_, e)| **e == entry).unwrap().0
    }

    /// The default template (and the government template inheriting it):
    /// the outline is the first numbering, untouched; numbered items use the
    /// second; each bullet level its own glyph; every level 20pt deeper;
    /// continuations and code at the item's margin with no list; the role
    /// entries' own shapes unchanged; a second application adds nothing.
    /// 이것을 실패시키는 것: 수준 여백 계산 · 수준별 기호 선택 · 개요 슬롯 변경 · 재적용 시
    /// 모양 추가(`list_shape` 를 늘 push).
    #[test]
    fn default_template_derives_list_levels() {
        for (template, others) in [
            (builtin_default().unwrap(), vec![]),
            (builtin_gov_proposal().unwrap(), vec![builtin_default().unwrap()]),
        ] {
            let resolved_before = {
                let provider: HashMap<String, Template> =
                    others.iter().map(|t| (t.meta.name.clone(), t.clone())).collect();
                let resolved = resolve_template(&template, &provider).unwrap();
                StyleRegistry::from_template(&resolved).unwrap()
            };
            let (mut registry, shapes) = derived(&template, &others).unwrap();
            assert_eq!(registry.numberings[0], NumberingDef::default_outline());
            let decimal = &registry.numberings[1];
            assert_eq!((decimal.id, decimal.start, decimal.levels.len()), (2, 0, 10));
            for (i, head) in decimal.levels.iter().enumerate() {
                assert_eq!(head.start, 1);
                assert_eq!(head.level, i as u32 + 1);
                assert_eq!(head.num_format, hwpforge_foundation::NumberFormatType::Digit);
                assert_eq!(head.text, format!("^{}.", i + 1));
                assert!(!head.checkable);
            }
            let glyphs: Vec<&str> =
                registry.bullets.iter().map(|b| b.bullet_char.as_str()).collect();
            assert_eq!(glyphs, ["•", "◦", "▪", "•", "◦", "▪", "•", "◦", "▪", "•", "☐"]);
            assert_eq!(registry.bullets[10].checked_char.as_deref(), Some("☑"));
            let shape = |e: StyleEntry| registry.para_shape(e.para_shape_id).unwrap().clone();
            let check = registry.get_style("check_item").copied().unwrap();
            for level in 0..10u8 {
                let l = usize::from(level);
                let left = 2000 * (i32::from(level) + 1);
                let item = |e: StyleEntry, list| {
                    let p = shape(e);
                    assert_eq!(p.list, Some(list), "level {level}");
                    assert_eq!(
                        (p.indent_left.as_i32(), p.indent_first_line.as_i32()),
                        (left, -1000)
                    );
                };
                item(
                    shapes.bullet.unwrap()[l],
                    ParagraphListRef::Bullet { bullet_id: BulletIndex::new(l), level },
                );
                item(
                    shapes.number.unwrap()[l],
                    ParagraphListRef::Number { numbering_id: NumberingIndex::new(1), level },
                );
                for (array, checked) in [(shapes.unchecked, false), (shapes.checked, true)] {
                    item(
                        array.unwrap()[l],
                        ParagraphListRef::CheckBullet {
                            bullet_id: BulletIndex::new(10),
                            level,
                            checked,
                        },
                    );
                    assert_eq!(array.unwrap()[l].style_ref(), check.style_ref());
                }
                for (entry, name, role) in [
                    (shapes.continuation[l], list_continuation_style_name(level), "list_item"),
                    (shapes.code_continuation[l], code_continuation_style_name(level), "code"),
                ] {
                    let p = shape(entry);
                    assert_eq!(p.list, None);
                    assert_eq!((p.indent_left.as_i32(), p.indent_first_line.as_i32()), (left, 0));
                    assert_eq!(name_of(&registry, entry), name);
                    let role = registry.get_style(role).unwrap();
                    assert_eq!(entry.char_shape_id, role.char_shape_id);
                }
            }
            let plain = shape(shapes.plain_fallback);
            assert_eq!(plain.list, None);
            assert_eq!(plain.indent_left.as_i32(), 2000);
            // The roles' own entries and shapes are as the template made them.
            for name in ["body", "list_item", "numbered_item", "check_item", "code", "blockquote"] {
                let before = resolved_before.get_style(name).unwrap();
                assert_eq!(registry.get_style(name), Some(before), "{name}");
                assert_eq!(
                    registry.para_shape(before.para_shape_id),
                    resolved_before.para_shape(before.para_shape_id),
                    "{name}"
                );
            }
            let count = registry.para_shape_count();
            let names =
                resolve_markdown_style_names(&template_resolved(&template, &others)).unwrap();
            assert_eq!(apply_markdown_list_levels(&mut registry, &names).unwrap(), shapes);
            assert_eq!(registry.para_shape_count(), count);
        }
    }

    fn template_resolved(template: &Template, others: &[Template]) -> Template {
        let provider: HashMap<String, Template> =
            others.iter().map(|t| (t.meta.name.clone(), t.clone())).collect();
        resolve_template(template, &provider).unwrap()
    }

    const STYLE: &str = "    char_shape: { font: Test, size: 10pt }\n";

    fn yaml(styles: &str, defs: &str, mapping: &str) -> Template {
        Template::from_yaml(&format!(
            "meta:\n  name: t\n  version: 1.0.0\nstyles:\n  body:\n{STYLE}    para_shape: {{ alignment: Left }}\n  code:\n{STYLE}    para_shape: {{ alignment: Left }}\n{styles}{defs}markdown_mapping:\n{mapping}"
        ))
        .unwrap()
    }

    fn style(name: &str, list: &str) -> String {
        format!("  {name}:\n{STYLE}    para_shape:\n      indent: {{ left: 10pt }}\n      list: {list}\n")
    }

    const BULLETS: &str = "bullets:\n  - { id: 5, bullet_char: a, use_image: false, para_head: { start: 0, level: 1, num_format: Digit, text: '', checkable: false } }\n  - { id: 30, bullet_char: b, use_image: false, para_head: { start: 0, level: 1, num_format: Digit, text: '', checkable: false } }\n  - { id: 7, bullet_char: c, checked_char: d, use_image: false, para_head: { start: 0, level: 1, num_format: Digit, text: '', checkable: true } }\n";

    /// A numbering with levels 1 to `levels`.
    fn numbering_of(id: u32, levels: u32) -> String {
        let heads: Vec<String> = (1..=levels)
            .map(|l| format!("{{ start: 1, level: {l}, num_format: Digit, text: '^{l}.', checkable: false }}"))
            .collect();
        format!("  - {{ id: {id}, start: 0, levels: [{}] }}\n", heads.join(", "))
    }

    fn numbering(id: u32) -> String {
        numbering_of(id, 10)
    }

    /// A numbering missing a level, or with its levels out of order, cannot
    /// be the numbered item's; the same numbering with all ten levels can.
    /// 이것을 실패시키는 것: 수준 검사 삭제 · 순서 검사 삭제.
    #[test]
    fn numbering_without_every_level_is_rejected() {
        let styles = format!(
            "{}{}",
            style("list_item", "null"),
            style("numbered_item", "{ kind: number, numbering_id: 40, level: 0 }")
        );
        let reversed = numbering_of(40, 10)
            .replace("level: 1,", "level: 0,")
            .replace("level: 2,", "level: 1,")
            .replace("level: 0,", "level: 2,");
        for (defs, ok) in [
            (numbering_of(40, 1), false),
            (numbering_of(40, 9), false),
            (reversed, false),
            (numbering_of(40, 10), true),
        ] {
            let template =
                yaml(&styles, &format!("numberings:\n{}{defs}", numbering(20)), "  body: body\n");
            assert_eq!(derived(&template, &[]).is_ok(), ok, "{defs}");
        }
    }

    /// Definitions are found by their declared ids where the registry put
    /// them, never by the id as a position: numberings 20 (the outline) and
    /// 40 merged in id order are positions 0 and 1; sparse bullet ids in
    /// `bullet_levels` pick their bullets; a style shared by a heading and
    /// the numbered item keeps its own entry and shape.
    /// 이것을 실패시키는 것: 선언 id 를 위치로 쓰기 · 번호 역할 공유 시 entry 덮어쓰기.
    #[test]
    fn list_roles_resolve_declared_ids_by_position() {
        let parent = Template::from_yaml(&format!(
            "meta:\n  name: parent\n  version: 1.0.0\nstyles:\n  body:\n{STYLE}numberings:\n{}",
            numbering(20)
        ))
        .unwrap();
        let mut child = yaml(
            &format!(
                "{}{}",
                style("list_item", "{ kind: bullet, bullet_id: 30, level: 0 }"),
                style("item", "{ kind: number, numbering_id: 40, level: 0 }")
            ),
            &format!("numberings:\n{}{BULLETS}", numbering(40)),
            "  numbered_item: item\n  heading3: item\n  bullet_levels: [30, 5, 30, 5, 30, 5, 30, 5, 30, 5]\n",
        );
        child.meta.extends = Some("parent".into());
        let before =
            StyleRegistry::from_template(&template_resolved(&child, std::slice::from_ref(&parent)))
                .unwrap();
        let (registry, shapes) = derived(&child, &[parent]).unwrap();
        assert_eq!(registry.numberings.iter().map(|n| n.id).collect::<Vec<_>>(), [20, 40]);
        assert_eq!(
            registry.para_shape(shapes.number.unwrap()[2].para_shape_id).unwrap().list,
            Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(1), level: 2 })
        );
        let bullets: Vec<_> = shapes
            .bullet
            .unwrap()
            .iter()
            .map(|e| match registry.para_shape(e.para_shape_id).unwrap().list {
                Some(ParagraphListRef::Bullet { bullet_id, .. }) => bullet_id.get(),
                other => panic!("{other:?}"),
            })
            .collect();
        // Merged in id order, bullets 5, 7 and 30 are positions 0, 1 and 2.
        assert_eq!(registry.bullets.iter().map(|b| b.id).collect::<Vec<_>>(), [5, 7, 30]);
        assert_eq!(bullets, [2, 0, 2, 0, 2, 0, 2, 0, 2, 0]);
        let item = before.get_style("item").unwrap();
        assert_eq!(registry.get_style("item"), Some(item));
        assert_eq!(registry.para_shape(item.para_shape_id), before.para_shape(item.para_shape_id));
    }

    /// Wrong list roles are template errors: an unknown, a checkable or a
    /// missing level in `bullet_levels`, `bullet_levels` with no bullet
    /// list item, a check item without a check bullet, a numbered item on
    /// the first numbering (the outline's, declared id 1 or not), the only
    /// numbering used by the numbered item.
    /// 이것을 실패시키는 것: 길이 검사 · 체크 정의 검사 · 첫 슬롯 검사 · 종류 검사 각각 삭제.
    #[test]
    fn invalid_list_roles_are_rejected() {
        let bullet = style("list_item", "{ kind: bullet, bullet_id: 5, level: 0 }");
        let ten = |ids: &str| format!("  bullet_levels: [{ids}]\n");
        let numbered = |id| {
            style("numbered_item", &format!("{{ kind: number, numbering_id: {id}, level: 0 }}"))
        };
        let cases = [
            (bullet.clone(), BULLETS.to_string(), ten("5, 5, 5, 5, 5, 5, 5, 5, 5, 99")),
            (bullet.clone(), BULLETS.to_string(), ten("5, 5, 5, 5, 5, 5, 5, 5, 5, 7")),
            (bullet.clone(), BULLETS.to_string(), ten("5, 5, 5, 5, 5, 5, 5, 5, 5")),
            (bullet.clone(), BULLETS.to_string(), ten("5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5")),
            (style("list_item", "null"), BULLETS.to_string(), ten("5, 5, 5, 5, 5, 5, 5, 5, 5, 5")),
            (
                format!(
                    "{bullet}{}",
                    style("check_item", "{ kind: bullet, bullet_id: 30, level: 0 }")
                ),
                BULLETS.to_string(),
                String::new(),
            ),
            (
                format!("{bullet}{}", numbered(1)),
                format!("numberings:\n{}{}{BULLETS}", numbering(1), numbering(2)),
                String::new(),
            ),
            (
                format!("{bullet}{}", numbered(3)),
                format!("numberings:\n{}{BULLETS}", numbering(3)),
                String::new(),
            ),
        ];
        for (styles, defs, mapping) in cases {
            let template = yaml(&styles, &defs, &format!("  list_item: list_item\n{mapping}"));
            assert!(derived(&template, &[]).is_err(), "{styles}{mapping}");
        }
        // The same template with valid roles derives.
        let template = yaml(
            &format!("{bullet}{}", numbered(2)),
            &format!("numberings:\n{}{}{BULLETS}", numbering(1), numbering(2)),
            &format!("  list_item: list_item\n{}", ten("5, 30, 5, 30, 5, 30, 5, 30, 5, 30")),
        );
        assert!(derived(&template, &[]).is_ok());
    }

    /// A numbered or check item role is named, or a style of its role's own
    /// name; nothing else stands in, so a template without one has no such
    /// items. The list item role still falls back to body.
    /// 이것을 실패시키는 것: body 로 대신하기.
    #[test]
    fn list_roles_fall_back_only_to_their_own_name() {
        let template = yaml("", "", "  body: body\n");
        let (_, shapes) = derived(&template, &[]).unwrap();
        assert_eq!((shapes.bullet, shapes.number, shapes.checked), (None, None, None));
        let names = resolve_markdown_style_names(&template).unwrap();
        assert_eq!((names.numbered_item, names.check_item), (None, None));
        assert_eq!(names.list_item.as_deref(), Some("body"));
        let template = yaml(
            &style("numbered_item", "{ kind: number, numbering_id: 2, level: 0 }"),
            &format!("numberings:\n{}{}", numbering(1), numbering(2)),
            "  body: body\n",
        );
        let names = resolve_markdown_style_names(&template).unwrap();
        assert_eq!(names.numbered_item.as_deref(), Some("numbered_item"));
        assert!(derived(&template, &[]).unwrap().1.number.is_some());
    }

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
