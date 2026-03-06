//! Built-in templates embedded at compile time.
//!
//! These templates are included via [`include_str!`] and parsed on first access.
//! They serve as the defaults for the HwpForge toolchain:
//!
//! - **default**: A4 paper, 한컴바탕 font, body/heading/code/quote styles
//! - **gov_proposal**: Korean government proposal formatting (extends default)

use crate::error::BlueprintResult;
use crate::template::Template;

/// Raw YAML source for the default template.
pub const DEFAULT_YAML: &str = include_str!("../templates/default.yaml");

/// Raw YAML source for the government proposal template.
pub const GOV_PROPOSAL_YAML: &str = include_str!("../templates/gov_proposal.yaml");

/// Parses and returns the built-in default template.
///
/// The default template provides:
/// - A4 page (210mm x 297mm, 20mm margins)
/// - 7 styles: body, heading1-3, code, blockquote, list_item
/// - 한컴바탕 font, 10pt body, justified alignment
///
/// # Errors
///
/// Returns [`crate::error::BlueprintError::YamlParse`] if the embedded YAML is malformed
/// (should never happen for built-in templates).
pub fn builtin_default() -> BlueprintResult<Template> {
    Template::from_yaml(DEFAULT_YAML)
}

/// Parses and returns the built-in government proposal template.
///
/// The government proposal template extends default with:
/// - Wider margins (30mm left for binding)
/// - Larger body text (11pt)
/// - 170% line spacing
/// - Additional title/subtitle styles
///
/// **Note**: This template uses `extends: default`. Use
/// [`crate::inheritance::resolve_template`] to fully resolve it.
///
/// # Errors
///
/// Returns [`crate::error::BlueprintError::YamlParse`] if the embedded YAML is malformed.
pub fn builtin_gov_proposal() -> BlueprintResult<Template> {
    Template::from_yaml(GOV_PROPOSAL_YAML)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inheritance::resolve_template;
    use crate::registry::StyleRegistry;
    use hwpforge_foundation::{Alignment, HwpUnit};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // YAML validity (golden tests for embedded templates)
    // -----------------------------------------------------------------------

    #[test]
    fn default_yaml_is_valid() {
        let tmpl = builtin_default().unwrap();
        assert_eq!(tmpl.meta.name, "default");
        assert_eq!(tmpl.meta.version, "1.0.0");
        assert!(tmpl.meta.extends.is_none());
    }

    #[test]
    fn gov_proposal_yaml_is_valid() {
        let tmpl = builtin_gov_proposal().unwrap();
        assert_eq!(tmpl.meta.name, "gov_proposal");
        assert_eq!(tmpl.meta.extends, Some("default".to_string()));
    }

    // -----------------------------------------------------------------------
    // Default template structure
    // -----------------------------------------------------------------------

    #[test]
    fn default_has_expected_styles() {
        let tmpl = builtin_default().unwrap();
        let expected =
            ["body", "heading1", "heading2", "heading3", "code", "blockquote", "list_item"];
        for name in &expected {
            assert!(tmpl.styles.contains_key(*name), "missing style: {name}");
        }
        assert_eq!(tmpl.styles.len(), expected.len());
    }

    #[test]
    fn default_has_a4_page() {
        let tmpl = builtin_default().unwrap();
        let page = tmpl.page.as_ref().unwrap();
        let settings = page.to_page_settings();
        assert!((settings.width.to_mm() - 210.0).abs() < 0.5);
        assert!((settings.height.to_mm() - 297.0).abs() < 0.5);
    }

    #[test]
    fn default_body_is_10pt_justify() {
        let tmpl = builtin_default().unwrap();
        let body = tmpl.styles.get("body").unwrap();
        let cs = body.char_shape.as_ref().unwrap();
        assert_eq!(cs.font, Some("한컴바탕".to_string()));
        assert_eq!(cs.size, Some(HwpUnit::from_pt(10.0).unwrap()));
        let ps = body.para_shape.as_ref().unwrap();
        assert_eq!(ps.alignment, Some(Alignment::Justify));
    }

    #[test]
    fn default_heading1_is_bold_16pt() {
        let tmpl = builtin_default().unwrap();
        let h1 = tmpl.styles.get("heading1").unwrap();
        let cs = h1.char_shape.as_ref().unwrap();
        assert_eq!(cs.size, Some(HwpUnit::from_pt(16.0).unwrap()));
        assert_eq!(cs.bold, Some(true));
    }

    #[test]
    fn default_has_markdown_mapping() {
        let tmpl = builtin_default().unwrap();
        let md = tmpl.markdown_mapping.as_ref().unwrap();
        assert_eq!(md.body, Some("body".to_string()));
        assert_eq!(md.heading1, Some("heading1".to_string()));
        assert_eq!(md.code, Some("code".to_string()));
    }

    // -----------------------------------------------------------------------
    // Government proposal template
    // -----------------------------------------------------------------------

    #[test]
    fn gov_proposal_has_title_style() {
        let tmpl = builtin_gov_proposal().unwrap();
        assert!(tmpl.styles.contains_key("title"));
        assert!(tmpl.styles.contains_key("subtitle"));
    }

    #[test]
    fn gov_proposal_overrides_body_size() {
        let tmpl = builtin_gov_proposal().unwrap();
        let body = tmpl.styles.get("body").unwrap();
        let cs = body.char_shape.as_ref().unwrap();
        assert_eq!(cs.size, Some(HwpUnit::from_pt(11.0).unwrap()));
    }

    #[test]
    fn gov_proposal_wider_left_margin() {
        let tmpl = builtin_gov_proposal().unwrap();
        let page = tmpl.page.as_ref().unwrap();
        assert_eq!(page.margin_left, Some(HwpUnit::from_mm(30.0).unwrap()));
    }

    // -----------------------------------------------------------------------
    // Inheritance resolution
    // -----------------------------------------------------------------------

    #[test]
    fn gov_proposal_resolves_from_default() {
        let default = builtin_default().unwrap();
        let gov = builtin_gov_proposal().unwrap();

        let mut provider = HashMap::new();
        provider.insert("default".to_string(), default);
        provider.insert("gov_proposal".to_string(), gov.clone());

        let resolved = resolve_template(&gov, &provider).unwrap();

        // Should have merged styles from default + gov_proposal additions
        assert!(resolved.meta.extends.is_none());
        assert!(resolved.styles.contains_key("body")); // From default
        assert!(resolved.styles.contains_key("heading1")); // From default
        assert!(resolved.styles.contains_key("title")); // From gov_proposal
        assert!(resolved.styles.contains_key("subtitle")); // From gov_proposal
        assert!(resolved.styles.contains_key("code")); // Inherited from default

        // body should have gov_proposal overrides merged with default
        let body = resolved.styles.get("body").unwrap();
        let cs = body.char_shape.as_ref().unwrap();
        // Font from default (not overridden in gov)
        assert_eq!(cs.font, Some("한컴바탕".to_string()));
        // Size from gov_proposal (overridden)
        assert_eq!(cs.size, Some(HwpUnit::from_pt(11.0).unwrap()));
    }

    // -----------------------------------------------------------------------
    // StyleRegistry from resolved template
    // -----------------------------------------------------------------------

    #[test]
    fn default_template_creates_valid_registry() {
        let tmpl = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&tmpl).unwrap();

        assert_eq!(registry.style_count(), 7);
        assert!(registry.get_style("body").is_some());
        assert!(registry.get_style("heading1").is_some());
    }

    #[test]
    fn resolved_gov_proposal_creates_valid_registry() {
        let default = builtin_default().unwrap();
        let gov = builtin_gov_proposal().unwrap();

        let mut provider = HashMap::new();
        provider.insert("default".to_string(), default);
        provider.insert("gov_proposal".to_string(), gov.clone());

        let resolved = resolve_template(&gov, &provider).unwrap();
        let registry = StyleRegistry::from_template(&resolved).unwrap();

        // 7 from default + 2 from gov (title, subtitle)
        assert_eq!(registry.style_count(), 9);
        assert!(registry.get_style("title").is_some());

        // Font deduplication: 한컴바탕 + D2Coding = 2 unique fonts
        assert_eq!(registry.font_count(), 2);
    }

    // -----------------------------------------------------------------------
    // Roundtrip: parse → serialize → parse
    // -----------------------------------------------------------------------

    #[test]
    fn default_yaml_roundtrip() {
        let original = builtin_default().unwrap();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let roundtripped = Template::from_yaml(&yaml).unwrap();
        assert_eq!(original.meta.name, roundtripped.meta.name);
        assert_eq!(original.styles.len(), roundtripped.styles.len());
    }

    #[test]
    fn gov_proposal_yaml_roundtrip() {
        let original = builtin_gov_proposal().unwrap();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let roundtripped = Template::from_yaml(&yaml).unwrap();
        assert_eq!(original.meta.name, roundtripped.meta.name);
        assert_eq!(original.styles.len(), roundtripped.styles.len());
    }
}
