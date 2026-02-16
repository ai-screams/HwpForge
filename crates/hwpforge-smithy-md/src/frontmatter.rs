//! YAML frontmatter parsing and rendering.

use std::collections::BTreeMap;

use hwpforge_core::Metadata;
use serde::{Deserialize, Serialize};

use crate::error::{MdError, MdResult};

/// Parsed YAML frontmatter used by smithy-md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Frontmatter {
    /// Optional template name (e.g. `gov_proposal`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,

    /// Optional document title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Optional author name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// Optional document date string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    /// Additional metadata payload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_yaml::Value>,
}

impl Frontmatter {
    fn has_content(&self) -> bool {
        self.template.is_some()
            || self.title.is_some()
            || self.author.is_some()
            || self.date.is_some()
            || !self.metadata.is_empty()
    }
}

/// Extracted frontmatter + markdown body.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedFrontmatter<'a> {
    /// Parsed frontmatter block (if present).
    pub frontmatter: Option<Frontmatter>,
    /// Markdown body with frontmatter removed.
    pub content: &'a str,
}

/// Extracts YAML frontmatter from a markdown string.
///
/// Recognizes a frontmatter block only when the very first line is `---`.
/// The block ends at the first line that is exactly `---` or `...`.
pub fn extract_frontmatter(markdown: &str) -> MdResult<ExtractedFrontmatter<'_>> {
    let content = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);

    let Some(first_newline) = content.find('\n') else {
        return Ok(ExtractedFrontmatter { frontmatter: None, content });
    };

    let first_line = content[..first_newline].trim_end_matches('\r');
    if first_line != "---" {
        return Ok(ExtractedFrontmatter { frontmatter: None, content });
    }

    let mut cursor = first_newline + 1;
    let mut yaml_block = String::new();

    while cursor <= content.len() {
        let next =
            content[cursor..].find('\n').map(|offset| cursor + offset + 1).unwrap_or(content.len());

        let line = &content[cursor..next];
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed == "---" || trimmed == "..." {
            let frontmatter: Frontmatter = match serde_yaml::from_str(&yaml_block) {
                Ok(parsed) => parsed,
                Err(err) => {
                    if looks_like_frontmatter(&yaml_block) {
                        return Err(MdError::InvalidFrontmatter { detail: err.to_string() });
                    }
                    return Ok(ExtractedFrontmatter { frontmatter: None, content });
                }
            };

            if !frontmatter.has_content() {
                return Ok(ExtractedFrontmatter { frontmatter: None, content });
            }

            return Ok(ExtractedFrontmatter {
                frontmatter: Some(frontmatter),
                content: &content[next..],
            });
        }

        yaml_block.push_str(line);
        if next == content.len() {
            break;
        }
        cursor = next;
    }

    if looks_like_frontmatter(&yaml_block) {
        return Err(MdError::FrontmatterUnclosed);
    }

    Ok(ExtractedFrontmatter { frontmatter: None, content })
}

fn looks_like_frontmatter(yaml_block: &str) -> bool {
    yaml_block
        .lines()
        .map(str::trim)
        .any(|line| !line.is_empty() && !line.starts_with('#') && line.contains(':'))
}

/// Renders frontmatter back to markdown YAML block syntax.
pub fn render_frontmatter(frontmatter: &Frontmatter) -> MdResult<String> {
    let yaml = serde_yaml::to_string(frontmatter)
        .map_err(|err| MdError::InvalidFrontmatter { detail: err.to_string() })?;
    Ok(format!("---\n{}---\n", yaml))
}

/// Builds frontmatter from document metadata and optional template.
pub fn from_metadata(metadata: &Metadata, template: Option<&str>) -> Frontmatter {
    let mut extra = BTreeMap::new();
    if let Some(subject) = &metadata.subject {
        extra.insert("subject".to_string(), serde_yaml::Value::String(subject.clone()));
    }
    if !metadata.keywords.is_empty() {
        let list = metadata.keywords.iter().cloned().map(serde_yaml::Value::String).collect();
        extra.insert("keywords".to_string(), serde_yaml::Value::Sequence(list));
    }
    if let Some(modified) = &metadata.modified {
        extra.insert("modified".to_string(), serde_yaml::Value::String(modified.clone()));
    }

    Frontmatter {
        template: template.map(ToOwned::to_owned),
        title: metadata.title.clone(),
        author: metadata.author.clone(),
        date: metadata.created.clone(),
        metadata: extra,
    }
}

/// Applies frontmatter fields into Core metadata.
pub fn apply_to_metadata(frontmatter: &Frontmatter, metadata: &mut Metadata) {
    if let Some(title) = &frontmatter.title {
        metadata.title = Some(title.clone());
    }
    if let Some(author) = &frontmatter.author {
        metadata.author = Some(author.clone());
    }
    if let Some(date) = &frontmatter.date {
        metadata.created = Some(date.clone());
    }

    if let Some(subject) = frontmatter.metadata.get("subject").and_then(serde_yaml::Value::as_str) {
        metadata.subject = Some(subject.to_string());
    }

    if let Some(modified) = frontmatter.metadata.get("modified").and_then(serde_yaml::Value::as_str)
    {
        metadata.modified = Some(modified.to_string());
    }

    if let Some(keywords) =
        frontmatter.metadata.get("keywords").and_then(serde_yaml::Value::as_sequence)
    {
        metadata.keywords =
            keywords.iter().filter_map(serde_yaml::Value::as_str).map(ToOwned::to_owned).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_without_frontmatter() {
        let input = "# Title\n\nBody";
        let extracted = extract_frontmatter(input).unwrap();
        assert!(extracted.frontmatter.is_none());
        assert_eq!(extracted.content, input);
    }

    #[test]
    fn extract_with_frontmatter() {
        let input = "---\ntitle: Test\nauthor: Kim\n---\n# Body";
        let extracted = extract_frontmatter(input).unwrap();
        let fm = extracted.frontmatter.unwrap();
        assert_eq!(fm.title.as_deref(), Some("Test"));
        assert_eq!(fm.author.as_deref(), Some("Kim"));
        assert_eq!(extracted.content, "# Body");
    }

    #[test]
    fn extract_unclosed_frontmatter_errors() {
        let input = "---\ntitle: Test\n# not closed";
        let err = extract_frontmatter(input).unwrap_err();
        assert!(matches!(err, MdError::FrontmatterUnclosed));
    }

    #[test]
    fn unclosed_thematic_break_block_falls_back() {
        let input = "---\nnot metadata\n# still body";
        let extracted = extract_frontmatter(input).unwrap();
        assert!(extracted.frontmatter.is_none());
        assert_eq!(extracted.content, input);
    }

    #[test]
    fn thematic_break_pair_is_not_frontmatter() {
        let input = "---\n\n---\n# Title";
        let extracted = extract_frontmatter(input).unwrap();
        assert!(extracted.frontmatter.is_none());
        assert_eq!(extracted.content, input);
    }

    #[test]
    fn heading_between_delimiters_is_not_frontmatter() {
        let input = "---\n# Heading\n---\nBody";
        let extracted = extract_frontmatter(input).unwrap();
        assert!(extracted.frontmatter.is_none());
        assert_eq!(extracted.content, input);
    }

    #[test]
    fn non_key_value_block_between_delimiters_falls_back() {
        let input = "---\njust text\n---\nBody";
        let extracted = extract_frontmatter(input).unwrap();
        assert!(extracted.frontmatter.is_none());
        assert_eq!(extracted.content, input);
    }

    #[test]
    fn malformed_key_value_frontmatter_returns_error() {
        let input = "---\ntitle: [\n---\nBody";
        let err = extract_frontmatter(input).unwrap_err();
        assert!(matches!(err, MdError::InvalidFrontmatter { .. }));
    }

    #[test]
    fn render_roundtrip() {
        let mut fm = Frontmatter {
            template: Some("gov_proposal".to_string()),
            title: Some("제안서".to_string()),
            author: None,
            date: Some("2026-02-16".to_string()),
            metadata: BTreeMap::new(),
        };
        fm.metadata
            .insert("category".to_string(), serde_yaml::Value::String("국가과제".to_string()));

        let rendered = render_frontmatter(&fm).unwrap();
        let extracted = extract_frontmatter(&rendered).unwrap();
        assert_eq!(extracted.frontmatter.unwrap(), fm);
    }

    #[test]
    fn apply_to_metadata_copies_fields() {
        let mut metadata = Metadata::default();
        let mut fm = Frontmatter {
            template: Some("default".to_string()),
            title: Some("T".to_string()),
            author: Some("A".to_string()),
            date: Some("2026-02-16".to_string()),
            metadata: BTreeMap::new(),
        };
        fm.metadata.insert(
            "keywords".to_string(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("hwp".to_string()),
                serde_yaml::Value::String("md".to_string()),
            ]),
        );

        apply_to_metadata(&fm, &mut metadata);
        assert_eq!(metadata.title.as_deref(), Some("T"));
        assert_eq!(metadata.author.as_deref(), Some("A"));
        assert_eq!(metadata.created.as_deref(), Some("2026-02-16"));
        assert_eq!(metadata.keywords, vec!["hwp", "md"]);
    }
}
