//! Integration tests for the Blueprint crate.
//!
//! These tests exercise the full workflow:
//! YAML → Template → Inheritance → StyleRegistry → indexed access.

use std::collections::HashMap;

use hwpforge_blueprint::builtins::{builtin_default, builtin_gov_proposal};
use hwpforge_blueprint::error::BlueprintError;
use hwpforge_blueprint::inheritance::resolve_template;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::schema::template_schema_json;
use hwpforge_blueprint::template::Template;
use hwpforge_foundation::{Alignment, Color, HwpUnit};

// ---------------------------------------------------------------------------
// Full pipeline: YAML → Template → Registry
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_yaml_to_registry() {
    let yaml = r#"
meta:
  name: test_pipeline
  version: 1.0.0

page:
  width: 210mm
  height: 297mm

styles:
  body:
    char_shape:
      font: 한컴바탕
      size: 10pt
      color: '#000000'
    para_shape:
      alignment: Justify
      line_spacing:
        spacing_type: Percentage
        value: '160%'
  heading:
    char_shape:
      font: 한컴바탕
      size: 16pt
      bold: true
      color: '#003366'
    para_shape:
      alignment: Left
      spacing:
        before: 12pt
        after: 6pt

markdown_mapping:
  body: body
  heading1: heading
"#;

    // Step 1: Parse YAML → Template
    let template = Template::from_yaml(yaml).unwrap();
    assert_eq!(template.meta.name, "test_pipeline");
    assert_eq!(template.styles.len(), 2);

    // Step 2: Template → StyleRegistry
    let registry = StyleRegistry::from_template(&template).unwrap();
    assert_eq!(registry.style_count(), 2);
    assert_eq!(registry.font_count(), 1); // Both use 한컴바탕

    // Step 3: Access by name
    let body = registry.get_style("body").unwrap();
    let body_cs = registry.char_shape(body.char_shape_id).unwrap();
    assert_eq!(body_cs.font, "한컴바탕");
    assert_eq!(body_cs.size, HwpUnit::from_pt(10.0).unwrap());
    assert_eq!(body_cs.color, Color::BLACK);

    let body_ps = registry.para_shape(body.para_shape_id).unwrap();
    assert_eq!(body_ps.alignment, Alignment::Justify);
    assert_eq!(body_ps.line_spacing_value, 160.0);

    let heading = registry.get_style("heading").unwrap();
    let heading_cs = registry.char_shape(heading.char_shape_id).unwrap();
    assert!(heading_cs.bold);
    assert_eq!(heading_cs.color, Color::from_rgb(0x00, 0x33, 0x66));

    let heading_ps = registry.para_shape(heading.para_shape_id).unwrap();
    assert_eq!(heading_ps.space_before, HwpUnit::from_pt(12.0).unwrap());
}

// ---------------------------------------------------------------------------
// Inheritance pipeline: parent + child → resolved → registry
// ---------------------------------------------------------------------------

#[test]
fn inheritance_pipeline_parent_child_to_registry() {
    let parent_yaml = r#"
meta:
  name: base
styles:
  body:
    char_shape:
      font: Arial
      size: 10pt
    para_shape:
      alignment: Left
  heading:
    char_shape:
      font: Arial
      size: 16pt
      bold: true
"#;

    let child_yaml = r#"
meta:
  name: custom
  extends: base
styles:
  body:
    char_shape:
      size: 12pt
      color: '#003366'
    para_shape:
      alignment: Justify
"#;

    let parent = Template::from_yaml(parent_yaml).unwrap();
    let child = Template::from_yaml(child_yaml).unwrap();

    let mut provider = HashMap::new();
    provider.insert("base".to_string(), parent);
    provider.insert("custom".to_string(), child.clone());

    // Resolve inheritance
    let resolved = resolve_template(&child, &provider).unwrap();
    assert!(resolved.meta.extends.is_none());

    // body should merge: font from parent, size/color from child
    let body = resolved.styles.get("body").unwrap();
    assert_eq!(body.char_shape.as_ref().unwrap().font, Some("Arial".to_string()));
    assert_eq!(body.char_shape.as_ref().unwrap().size, Some(HwpUnit::from_pt(12.0).unwrap()));
    assert_eq!(body.char_shape.as_ref().unwrap().color, Some(Color::from_rgb(0, 0x33, 0x66)));

    // heading should be inherited from parent
    assert!(resolved.styles.contains_key("heading"));

    // Build registry from resolved
    let registry = StyleRegistry::from_template(&resolved).unwrap();
    assert_eq!(registry.style_count(), 2);
    assert_eq!(registry.font_count(), 1); // Both use Arial
}

// ---------------------------------------------------------------------------
// Built-in templates full pipeline
// ---------------------------------------------------------------------------

#[test]
fn builtin_default_full_pipeline() {
    let template = builtin_default().unwrap();
    let registry = StyleRegistry::from_template(&template).unwrap();

    // 7 styles, 2 fonts (한컴바탕 + D2Coding)
    assert_eq!(registry.style_count(), 7);
    assert_eq!(registry.font_count(), 2);

    // Verify all styles are accessible
    for name in &["body", "heading1", "heading2", "heading3", "code", "blockquote", "list_item"] {
        let entry = registry.get_style(name).unwrap_or_else(|| panic!("missing style: {name}"));
        assert!(registry.char_shape(entry.char_shape_id).is_some());
        assert!(registry.para_shape(entry.para_shape_id).is_some());
    }
}

#[test]
fn builtin_gov_proposal_full_pipeline() {
    let default = builtin_default().unwrap();
    let gov = builtin_gov_proposal().unwrap();

    let mut provider = HashMap::new();
    provider.insert("default".to_string(), default);
    provider.insert("gov_proposal".to_string(), gov.clone());

    let resolved = resolve_template(&gov, &provider).unwrap();
    let registry = StyleRegistry::from_template(&resolved).unwrap();

    // 7 from default + 2 from gov (title, subtitle) = 9
    assert_eq!(registry.style_count(), 9);

    // Title should be centered
    let title = registry.get_style("title").unwrap();
    let title_ps = registry.para_shape(title.para_shape_id).unwrap();
    assert_eq!(title_ps.alignment, Alignment::Center);
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn invalid_yaml_returns_parse_error() {
    let err = Template::from_yaml("meta:\n  name: [invalid").unwrap_err();
    assert!(matches!(err, BlueprintError::YamlParse { .. }));
}

#[test]
fn empty_styles_registry_error() {
    let yaml = "meta:\n  name: empty\nstyles: {}\n";
    let template = Template::from_yaml(yaml).unwrap();
    let err = StyleRegistry::from_template(&template).unwrap_err();
    assert!(matches!(err, BlueprintError::EmptyStyleMap));
}

#[test]
fn missing_font_registry_error() {
    let yaml = r#"
meta:
  name: broken
styles:
  body:
    char_shape:
      size: 10pt
"#;
    let template = Template::from_yaml(yaml).unwrap();
    let err = StyleRegistry::from_template(&template).unwrap_err();
    match err {
        BlueprintError::StyleResolution { field, .. } => assert_eq!(field, "font"),
        _ => panic!("Expected StyleResolution error"),
    }
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

#[test]
fn schema_json_is_valid_and_describes_template() {
    let json = template_schema_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["title"], "Template");
    assert!(value["properties"]["meta"].is_object());
    assert!(value["properties"]["styles"].is_object());
}
