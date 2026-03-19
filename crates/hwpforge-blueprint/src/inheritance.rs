//! Template inheritance resolution with DFS and circular detection.
//!
//! This module implements the inheritance chain resolution algorithm for
//! Blueprint templates, supporting the `extends` keyword for template reuse.
//!
//! # Inheritance Model
//!
//! Templates can extend parent templates using the `meta.extends` field:
//!
//! ```yaml
//! # parent.yaml
//! meta:
//!   name: parent
//! styles:
//!   body:
//!     char_shape: { font: "Arial", size: 10pt }
//!
//! # child.yaml
//! meta:
//!   name: child
//!   extends: parent
//! styles:
//!   body:
//!     char_shape: { size: 12pt }  # Overrides only size, inherits font
//! ```
//!
//! After resolution, the child template contains merged styles where child
//! fields override parent fields.
//!
//! # Algorithm
//!
//! The resolution uses **depth-first search (DFS)** with cycle detection:
//!
//! 1. Start from the child template
//! 2. Walk up the `extends` chain collecting ancestors
//! 3. Detect circular inheritance (visited set)
//! 4. Merge from root to child (parent first, child overrides)
//! 5. Return fully resolved template with no `extends` field
//!
//! # Merge Semantics
//!
//! - **Styles**: Field-level merge (child fields override parent fields)
//! - **Page**: Child's page entirely replaces parent's (if present)
//! - **Tabs**: Child tab definitions override parent definitions by id
//! - **Markdown mapping**: Field-level merge (child entries override parent)

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::error::{BlueprintError, BlueprintResult};
use crate::style::PartialStyle;
use crate::template::{MarkdownMapping, Template, TemplateTabDef};

/// Maximum inheritance depth to prevent infinite recursion.
pub const MAX_INHERITANCE_DEPTH: usize = 10;

/// Trait for looking up templates by name during inheritance resolution.
///
/// This abstraction allows different template storage backends (HashMap,
/// Vec, file system, etc.) without coupling the resolution algorithm to
/// a specific implementation.
pub trait TemplateProvider {
    /// Retrieves a template by name.
    ///
    /// Returns `None` if the template does not exist.
    fn get_template(&self, name: &str) -> Option<&Template>;
}

impl TemplateProvider for HashMap<String, Template> {
    fn get_template(&self, name: &str) -> Option<&Template> {
        self.get(name)
    }
}

impl TemplateProvider for Vec<Template> {
    fn get_template(&self, name: &str) -> Option<&Template> {
        self.iter().find(|t| t.meta.name == name)
    }
}

/// Resolves a template's inheritance chain into a fully merged template.
///
/// This function walks up the `extends` chain, merges styles from parent
/// to child, and returns a new template with all inherited fields resolved.
///
/// # Errors
///
/// - [`BlueprintError::CircularInheritance`] if a cycle is detected
/// - [`BlueprintError::TemplateNotFound`] if a parent template is missing
/// - [`BlueprintError::InheritanceDepthExceeded`] if depth exceeds limit
///
/// # Example
///
/// ```ignore
/// let templates = HashMap::from([
///     ("base".into(), base_template),
///     ("child".into(), child_template),
/// ]);
///
/// let resolved = resolve_template(&child_template, &templates)?;
/// assert!(resolved.meta.extends.is_none()); // No extends after resolution
/// ```
pub fn resolve_template(
    template: &Template,
    provider: &dyn TemplateProvider,
) -> BlueprintResult<Template> {
    // No inheritance chain: return clone
    if template.meta.extends.is_none() {
        return Ok(template.clone());
    }

    // Collect ancestors via DFS
    let mut ancestors = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(template.meta.name.clone()); // Mark starting template as visited
    let mut current = template;
    let mut chain = vec![template.meta.name.clone()];

    while let Some(ref parent_name) = current.meta.extends {
        // Depth limit check
        if ancestors.len() >= MAX_INHERITANCE_DEPTH {
            return Err(BlueprintError::InheritanceDepthExceeded {
                depth: ancestors.len() + 1,
                max: MAX_INHERITANCE_DEPTH,
            });
        }

        // Circular detection (check before adding to visited)
        if visited.contains(parent_name) {
            chain.push(parent_name.clone());
            return Err(BlueprintError::CircularInheritance { chain });
        }
        visited.insert(parent_name.clone());

        // Lookup parent
        let parent = provider
            .get_template(parent_name)
            .ok_or_else(|| BlueprintError::TemplateNotFound { name: parent_name.clone() })?;

        ancestors.push(parent.clone());
        chain.push(parent_name.clone());
        current = parent;
    }

    // Merge from root to child (reverse order)
    let mut merged = ancestors
        .into_iter()
        .rev()
        .fold(template.clone(), |acc, parent| merge_templates(&parent, &acc));

    // Clear extends after resolution
    merged.meta.extends = None;

    Ok(merged)
}

/// Merges a base template with a child template.
///
/// - **Styles**: For each style in child, merge with base; inherit base-only styles
/// - **Page**: Child's page replaces base's (if present)
/// - **Markdown mapping**: Field-level merge (child fields override base fields)
///
/// Returns a new template with merged data.
fn merge_templates(base: &Template, child: &Template) -> Template {
    // Merge styles: start with base, apply child overrides
    let mut merged_styles: IndexMap<String, PartialStyle> = base.styles.clone();

    for (name, child_style) in &child.styles {
        if let Some(base_style) = merged_styles.get_mut(name) {
            // Style exists in both: field-level merge
            base_style.merge(child_style);
        } else {
            // New style in child: add it
            merged_styles.insert(name.clone(), child_style.clone());
        }
    }

    // Page: child replaces base entirely
    let merged_page = child.page.clone().or_else(|| base.page.clone());

    // Tabs: child overrides parent definitions by id
    let merged_tabs = merge_tabs(&base.tabs, &child.tabs);

    // Markdown mapping: field-level merge
    let merged_md =
        merge_markdown_mappings(base.markdown_mapping.as_ref(), child.markdown_mapping.as_ref());

    Template {
        meta: child.meta.clone(), // Child's meta takes precedence
        page: merged_page,
        styles: merged_styles,
        tabs: merged_tabs,
        markdown_mapping: merged_md,
    }
}

fn merge_tabs(base: &[TemplateTabDef], child: &[TemplateTabDef]) -> Vec<TemplateTabDef> {
    let mut merged = base.to_vec();
    for tab in child {
        if let Some(existing) = merged.iter_mut().find(|candidate| candidate.id == tab.id) {
            *existing = tab.clone();
        } else {
            merged.push(tab.clone());
        }
    }
    merged.sort_by_key(|tab| tab.id);
    merged
}

/// Merges two MarkdownMapping structs (base + child override).
fn merge_markdown_mappings(
    base: Option<&MarkdownMapping>,
    child: Option<&MarkdownMapping>,
) -> Option<MarkdownMapping> {
    match (base, child) {
        (None, None) => None,
        (Some(b), None) => Some(b.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(b), Some(c)) => {
            let mut merged = b.clone();
            // Child fields override base fields (only if Some)
            if c.body.is_some() {
                merged.body.clone_from(&c.body);
            }
            if c.heading1.is_some() {
                merged.heading1.clone_from(&c.heading1);
            }
            if c.heading2.is_some() {
                merged.heading2.clone_from(&c.heading2);
            }
            if c.heading3.is_some() {
                merged.heading3.clone_from(&c.heading3);
            }
            if c.heading4.is_some() {
                merged.heading4.clone_from(&c.heading4);
            }
            if c.heading5.is_some() {
                merged.heading5.clone_from(&c.heading5);
            }
            if c.heading6.is_some() {
                merged.heading6.clone_from(&c.heading6);
            }
            if c.code.is_some() {
                merged.code.clone_from(&c.code);
            }
            if c.blockquote.is_some() {
                merged.blockquote.clone_from(&c.blockquote);
            }
            if c.list_item.is_some() {
                merged.list_item.clone_from(&c.list_item);
            }
            Some(merged)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{PartialCharShape, PartialParaShape};
    use crate::template::{PageStyle, TemplateMeta};
    use hwpforge_foundation::{Alignment, HwpUnit};
    use pretty_assertions::assert_eq;

    /// Helper to create a minimal template for testing.
    fn make_template(
        name: &str,
        extends: Option<&str>,
        styles: Vec<(&str, PartialStyle)>,
    ) -> Template {
        Template {
            meta: TemplateMeta {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: None,
                extends: extends.map(|s| s.to_string()),
            },
            page: None,
            styles: styles.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            tabs: vec![],
            markdown_mapping: None,
        }
    }

    /// Helper to create a partial style with just char_shape font.
    fn style_font(font: &str) -> PartialStyle {
        PartialStyle {
            char_shape: Some(PartialCharShape {
                font: Some(font.to_string()),
                ..Default::default()
            }),
            para_shape: None,
        }
    }

    /// Helper to create a partial style with just char_shape size.
    fn style_size(size: HwpUnit) -> PartialStyle {
        PartialStyle {
            char_shape: Some(PartialCharShape { size: Some(size), ..Default::default() }),
            para_shape: None,
        }
    }

    /// Helper to create a partial style with para_shape alignment.
    fn style_align(align: Alignment) -> PartialStyle {
        PartialStyle {
            char_shape: None,
            para_shape: Some(PartialParaShape { alignment: Some(align), ..Default::default() }),
        }
    }

    #[test]
    fn no_inheritance_returns_same_template() {
        let tmpl = make_template("base", None, vec![("body", style_font("Arial"))]);
        let provider = HashMap::<String, Template>::new();

        let resolved = resolve_template(&tmpl, &provider).unwrap();

        assert_eq!(resolved.meta.name, "base");
        assert_eq!(resolved.meta.extends, None);
        assert_eq!(resolved.styles.len(), 1);
    }

    #[test]
    fn single_inheritance_merges_styles() {
        let parent = make_template("parent", None, vec![("body", style_font("Arial"))]);
        let child = make_template(
            "child",
            Some("parent"),
            vec![("body", style_size(HwpUnit::from_pt(12.0).unwrap()))],
        );

        let provider = HashMap::from([
            ("parent".to_string(), parent.clone()),
            ("child".to_string(), child.clone()),
        ]);

        let resolved = resolve_template(&child, &provider).unwrap();

        assert_eq!(resolved.meta.name, "child");
        assert_eq!(resolved.meta.extends, None); // Cleared after resolution

        let body_style = resolved.styles.get("body").unwrap();
        assert_eq!(body_style.char_shape.as_ref().unwrap().font, Some("Arial".to_string()));
        assert_eq!(
            body_style.char_shape.as_ref().unwrap().size,
            Some(HwpUnit::from_pt(12.0).unwrap())
        );
    }

    #[test]
    fn two_level_inheritance_merges_grandparent() {
        let grandparent = make_template("grandparent", None, vec![("body", style_font("Times"))]);
        let parent = make_template(
            "parent",
            Some("grandparent"),
            vec![("body", style_size(HwpUnit::from_pt(10.0).unwrap()))],
        );
        let child =
            make_template("child", Some("parent"), vec![("body", style_align(Alignment::Center))]);

        let provider = HashMap::from([
            ("grandparent".to_string(), grandparent),
            ("parent".to_string(), parent),
            ("child".to_string(), child.clone()),
        ]);

        let resolved = resolve_template(&child, &provider).unwrap();

        let body = resolved.styles.get("body").unwrap();
        assert_eq!(body.char_shape.as_ref().unwrap().font, Some("Times".to_string()));
        assert_eq!(body.char_shape.as_ref().unwrap().size, Some(HwpUnit::from_pt(10.0).unwrap()));
        assert_eq!(body.para_shape.as_ref().unwrap().alignment, Some(Alignment::Center));
    }

    #[test]
    fn circular_two_cycle_detected() {
        let a = make_template("a", Some("b"), vec![]);
        let b = make_template("b", Some("a"), vec![]);

        let provider = HashMap::from([("a".to_string(), a.clone()), ("b".to_string(), b)]);

        let err = resolve_template(&a, &provider).unwrap_err();

        match err {
            BlueprintError::CircularInheritance { chain } => {
                assert!(chain.contains(&"a".to_string()));
                assert!(chain.contains(&"b".to_string()));
                assert_eq!(chain.len(), 3); // a -> b -> a
            }
            _ => panic!("Expected CircularInheritance error, got {:?}", err),
        }
    }

    #[test]
    fn circular_self_reference_detected() {
        let a = make_template("a", Some("a"), vec![]);
        let provider = HashMap::from([("a".to_string(), a.clone())]);

        let err = resolve_template(&a, &provider).unwrap_err();

        match err {
            BlueprintError::CircularInheritance { chain } => {
                assert_eq!(chain, vec!["a".to_string(), "a".to_string()]);
            }
            _ => panic!("Expected CircularInheritance error"),
        }
    }

    #[test]
    fn template_not_found_error() {
        let child = make_template("child", Some("missing"), vec![]);
        let provider = HashMap::<String, Template>::new();

        let err = resolve_template(&child, &provider).unwrap_err();

        match err {
            BlueprintError::TemplateNotFound { name } => {
                assert_eq!(name, "missing");
            }
            _ => panic!("Expected TemplateNotFound error"),
        }
    }

    #[test]
    fn depth_limit_exceeded() {
        // Create a chain of 11 templates (exceeds MAX_INHERITANCE_DEPTH = 10)
        let mut templates = HashMap::new();
        templates.insert("t0".to_string(), make_template("t0", None, vec![]));

        for i in 1..=11 {
            let parent_name = format!("t{}", i - 1);
            let tmpl = make_template(&format!("t{}", i), Some(&parent_name), vec![]);
            templates.insert(format!("t{}", i), tmpl);
        }

        let child = templates.get("t11").unwrap();
        let err = resolve_template(child, &templates).unwrap_err();

        match err {
            BlueprintError::InheritanceDepthExceeded { depth, max } => {
                assert!(depth > max);
                assert_eq!(max, MAX_INHERITANCE_DEPTH);
            }
            _ => panic!("Expected InheritanceDepthExceeded error"),
        }
    }

    #[test]
    fn child_overrides_parent_field() {
        let parent = make_template(
            "parent",
            None,
            vec![(
                "body",
                PartialStyle {
                    char_shape: Some(PartialCharShape {
                        font: Some("Arial".to_string()),
                        size: Some(HwpUnit::from_pt(10.0).unwrap()),
                        bold: Some(false),
                        ..Default::default()
                    }),
                    para_shape: None,
                },
            )],
        );

        let child = make_template(
            "child",
            Some("parent"),
            vec![(
                "body",
                PartialStyle {
                    char_shape: Some(PartialCharShape {
                        bold: Some(true), // Override only bold
                        ..Default::default()
                    }),
                    para_shape: None,
                },
            )],
        );

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();
        let body = resolved.styles.get("body").unwrap();

        assert_eq!(body.char_shape.as_ref().unwrap().font, Some("Arial".to_string()));
        assert_eq!(body.char_shape.as_ref().unwrap().size, Some(HwpUnit::from_pt(10.0).unwrap()));
        assert_eq!(body.char_shape.as_ref().unwrap().bold, Some(true)); // Overridden
    }

    #[test]
    fn parent_only_style_inherited() {
        let parent = make_template(
            "parent",
            None,
            vec![("body", style_font("Arial")), ("heading", style_font("Times"))],
        );

        let child = make_template(
            "child",
            Some("parent"),
            vec![("body", style_size(HwpUnit::from_pt(12.0).unwrap()))], // Only overrides body
        );

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();

        assert!(resolved.styles.contains_key("body"));
        assert!(resolved.styles.contains_key("heading")); // Inherited from parent
        assert_eq!(
            resolved.styles.get("heading").unwrap().char_shape.as_ref().unwrap().font,
            Some("Times".to_string())
        );
    }

    #[test]
    fn child_page_replaces_parent_page() {
        let parent = Template {
            meta: TemplateMeta {
                name: "parent".into(),
                version: "1.0.0".into(),
                description: None,
                extends: None,
            },
            page: Some(PageStyle::a4()),
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: None,
        };

        let child = Template {
            meta: TemplateMeta {
                name: "child".into(),
                version: "1.0.0".into(),
                description: None,
                extends: Some("parent".into()),
            },
            page: Some(PageStyle::default()),
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: None,
        };

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();

        // Child's page should be preserved (not parent's)
        assert!(resolved.page.is_some());
        // Child page is default (all None) since we used PageStyle::default()
        assert!(resolved.page.as_ref().unwrap().width.is_none());
    }

    #[test]
    fn no_child_page_inherits_parent_page() {
        let parent = Template {
            meta: TemplateMeta {
                name: "parent".into(),
                version: "1.0.0".into(),
                description: None,
                extends: None,
            },
            page: Some(PageStyle::a4()),
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: None,
        };

        let child = Template {
            meta: TemplateMeta {
                name: "child".into(),
                version: "1.0.0".into(),
                description: None,
                extends: Some("parent".into()),
            },
            page: None, // No page in child
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: None,
        };

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();

        assert!(resolved.page.is_some()); // Inherited from parent
        assert!(resolved.page.as_ref().unwrap().width.is_some()); // A4 width
    }

    #[test]
    fn markdown_mapping_child_overrides_parent_entries() {
        let parent = Template {
            meta: TemplateMeta {
                name: "parent".into(),
                version: "1.0.0".into(),
                description: None,
                extends: None,
            },
            page: None,
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: Some(MarkdownMapping {
                heading1: Some("heading1".to_string()),
                heading2: Some("heading2".to_string()),
                ..Default::default()
            }),
        };

        let child = Template {
            meta: TemplateMeta {
                name: "child".into(),
                version: "1.0.0".into(),
                description: None,
                extends: Some("parent".into()),
            },
            page: None,
            styles: IndexMap::new(),
            tabs: vec![],
            markdown_mapping: Some(MarkdownMapping {
                heading1: Some("custom_h1".to_string()), // Override
                heading3: Some("heading3".to_string()),  // Add new
                ..Default::default()
            }),
        };

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();
        let md = resolved.markdown_mapping.unwrap();

        assert_eq!(md.heading1, Some("custom_h1".to_string())); // Overridden
        assert_eq!(md.heading2, Some("heading2".to_string())); // Inherited
        assert_eq!(md.heading3, Some("heading3".to_string())); // Added
    }

    #[test]
    fn template_provider_hashmap_lookup() {
        let tmpl = make_template("test", None, vec![]);
        let provider = HashMap::from([("test".to_string(), tmpl.clone())]);

        assert!(provider.get_template("test").is_some());
        assert!(provider.get_template("missing").is_none());
    }

    #[test]
    fn template_provider_vec_lookup() {
        let t1 = make_template("t1", None, vec![]);
        let t2 = make_template("t2", None, vec![]);
        let provider = vec![t1, t2];

        assert!(provider.get_template("t1").is_some());
        assert!(provider.get_template("t2").is_some());
        assert!(provider.get_template("missing").is_none());
    }

    #[test]
    fn child_adds_new_style_not_in_parent() {
        let parent = make_template("parent", None, vec![("body", style_font("Arial"))]);
        let child = make_template(
            "child",
            Some("parent"),
            vec![
                ("body", style_size(HwpUnit::from_pt(12.0).unwrap())),
                ("caption", style_font("Times")), // New style
            ],
        );

        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();

        assert_eq!(resolved.styles.len(), 2);
        assert!(resolved.styles.contains_key("body"));
        assert!(resolved.styles.contains_key("caption"));
    }

    #[test]
    fn three_level_inheritance_chain() {
        let root = make_template("root", None, vec![("s", style_font("A"))]);
        let mid = make_template(
            "mid",
            Some("root"),
            vec![("s", style_size(HwpUnit::from_pt(10.0).unwrap()))],
        );
        let leaf = make_template("leaf", Some("mid"), vec![("s", style_align(Alignment::Right))]);

        let provider = HashMap::from([
            ("root".to_string(), root),
            ("mid".to_string(), mid),
            ("leaf".to_string(), leaf.clone()),
        ]);

        let resolved = resolve_template(&leaf, &provider).unwrap();
        let s = resolved.styles.get("s").unwrap();

        assert_eq!(s.char_shape.as_ref().unwrap().font, Some("A".to_string()));
        assert_eq!(s.char_shape.as_ref().unwrap().size, Some(HwpUnit::from_pt(10.0).unwrap()));
        assert_eq!(s.para_shape.as_ref().unwrap().alignment, Some(Alignment::Right));
    }

    #[test]
    fn tabs_are_inherited_and_child_overrides_by_id() {
        let parent = Template {
            meta: TemplateMeta {
                name: "parent".into(),
                version: "1.0.0".into(),
                description: None,
                extends: None,
            },
            page: None,
            styles: IndexMap::new(),
            tabs: vec![TemplateTabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![crate::template::TemplateTabStop {
                    position: HwpUnit::new(7000).unwrap(),
                    align: hwpforge_foundation::TabAlign::Left,
                    leader: hwpforge_foundation::TabLeader::dot(),
                }],
            }],
            markdown_mapping: None,
        };
        let child = Template {
            meta: TemplateMeta {
                name: "child".into(),
                version: "1.0.0".into(),
                description: None,
                extends: Some("parent".into()),
            },
            page: None,
            styles: IndexMap::new(),
            tabs: vec![
                TemplateTabDef {
                    id: 3,
                    auto_tab_left: false,
                    auto_tab_right: false,
                    stops: vec![crate::template::TemplateTabStop {
                        position: HwpUnit::new(9000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Right,
                        leader: hwpforge_foundation::TabLeader::none(),
                    }],
                },
                TemplateTabDef {
                    id: 4,
                    auto_tab_left: false,
                    auto_tab_right: false,
                    stops: vec![crate::template::TemplateTabStop {
                        position: HwpUnit::new(12000).unwrap(),
                        align: hwpforge_foundation::TabAlign::Center,
                        leader: hwpforge_foundation::TabLeader::dot(),
                    }],
                },
            ],
            markdown_mapping: None,
        };
        let provider =
            HashMap::from([("parent".to_string(), parent), ("child".to_string(), child.clone())]);

        let resolved = resolve_template(&child, &provider).unwrap();
        assert_eq!(resolved.tabs.len(), 2);
        assert_eq!(resolved.tabs[0].id, 3);
        assert_eq!(resolved.tabs[0].stops[0].position, HwpUnit::new(9000).unwrap());
        assert_eq!(resolved.tabs[1].id, 4);
    }

    #[test]
    fn circular_three_cycle_detected() {
        let a = make_template("a", Some("b"), vec![]);
        let b = make_template("b", Some("c"), vec![]);
        let c = make_template("c", Some("a"), vec![]);

        let provider = HashMap::from([
            ("a".to_string(), a.clone()),
            ("b".to_string(), b),
            ("c".to_string(), c),
        ]);

        let err = resolve_template(&a, &provider).unwrap_err();

        match err {
            BlueprintError::CircularInheritance { chain } => {
                assert_eq!(chain.len(), 4); // a -> b -> c -> a
            }
            _ => panic!("Expected CircularInheritance error"),
        }
    }
}
