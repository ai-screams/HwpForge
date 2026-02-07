//! Document metadata.
//!
//! [`Metadata`] holds the document's title, author, subject, keywords,
//! and timestamps. All fields are optional; an empty `Metadata` is valid.
//!
//! Timestamps are stored as `Option<String>` in ISO 8601 format
//! (e.g. `"2026-02-07T10:30:00Z"`). The `chrono` crate is intentionally
//! avoided to keep Core's dependency footprint minimal -- parse dates
//! at the Smithy layer when needed.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::Metadata;
//!
//! let meta = Metadata {
//!     title: Some("Quarterly Report".to_string()),
//!     author: Some("Kim".to_string()),
//!     ..Metadata::default()
//! };
//! assert_eq!(meta.title.as_deref(), Some("Quarterly Report"));
//! assert!(meta.subject.is_none());
//! ```

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Document metadata: title, author, subject, keywords, timestamps.
///
/// All fields are optional. `Default` returns a fully empty metadata
/// (all `None` / empty `Vec`).
///
/// # Design Decision
///
/// Timestamps use `Option<String>` (ISO 8601) instead of `chrono::DateTime`.
/// Rationale: `chrono` adds ~250KB compile weight for two fields that Core
/// never does arithmetic on. Smithy crates parse and validate dates when
/// reading from format-specific sources.
///
/// # Examples
///
/// ```
/// use hwpforge_core::Metadata;
///
/// let meta = Metadata::default();
/// assert!(meta.title.is_none());
/// assert!(meta.keywords.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Metadata {
    /// Document title.
    pub title: Option<String>,
    /// Document author.
    pub author: Option<String>,
    /// Document subject or description.
    pub subject: Option<String>,
    /// Searchable keywords.
    pub keywords: Vec<String>,
    /// Creation timestamp in ISO 8601 format (e.g. `"2026-02-07T10:30:00Z"`).
    pub created: Option<String>,
    /// Last modification timestamp in ISO 8601 format.
    pub modified: Option<String>,
}

impl std::fmt::Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.title {
            Some(t) => write!(f, "Metadata(\"{}\")", t),
            None => write!(f, "Metadata(untitled)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_none_or_empty() {
        let m = Metadata::default();
        assert!(m.title.is_none());
        assert!(m.author.is_none());
        assert!(m.subject.is_none());
        assert!(m.keywords.is_empty());
        assert!(m.created.is_none());
        assert!(m.modified.is_none());
    }

    #[test]
    fn struct_literal_construction() {
        let m = Metadata {
            title: Some("Test".to_string()),
            author: Some("Author".to_string()),
            subject: Some("Subject".to_string()),
            keywords: vec!["rust".to_string(), "hwp".to_string()],
            created: Some("2026-02-07T00:00:00Z".to_string()),
            modified: Some("2026-02-07T12:00:00Z".to_string()),
        };
        assert_eq!(m.title.as_deref(), Some("Test"));
        assert_eq!(m.keywords.len(), 2);
    }

    #[test]
    fn partial_construction_with_defaults() {
        let m = Metadata {
            title: Some("Report".to_string()),
            ..Metadata::default()
        };
        assert_eq!(m.title.as_deref(), Some("Report"));
        assert!(m.author.is_none());
    }

    #[test]
    fn display_with_title() {
        let m = Metadata { title: Some("My Doc".to_string()), ..Metadata::default() };
        assert_eq!(m.to_string(), "Metadata(\"My Doc\")");
    }

    #[test]
    fn display_without_title() {
        let m = Metadata::default();
        assert_eq!(m.to_string(), "Metadata(untitled)");
    }

    #[test]
    fn equality() {
        let a = Metadata { title: Some("A".to_string()), ..Metadata::default() };
        let b = Metadata { title: Some("A".to_string()), ..Metadata::default() };
        let c = Metadata { title: Some("B".to_string()), ..Metadata::default() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn clone_independence() {
        let m = Metadata { title: Some("Original".to_string()), ..Metadata::default() };
        let mut cloned = m.clone();
        cloned.title = Some("Modified".to_string());
        assert_eq!(m.title.as_deref(), Some("Original"));
    }

    #[test]
    fn korean_text() {
        let m = Metadata {
            title: Some("분기 보고서".to_string()),
            author: Some("김철수".to_string()),
            keywords: vec!["한글".to_string(), "보고서".to_string()],
            ..Metadata::default()
        };
        assert_eq!(m.title.as_deref(), Some("분기 보고서"));
    }

    #[test]
    fn serde_roundtrip() {
        let m = Metadata {
            title: Some("Test".to_string()),
            author: Some("Author".to_string()),
            subject: None,
            keywords: vec!["a".to_string(), "b".to_string()],
            created: Some("2026-02-07T00:00:00Z".to_string()),
            modified: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Metadata = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn serde_default_roundtrip() {
        let m = Metadata::default();
        let json = serde_json::to_string(&m).unwrap();
        let back: Metadata = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn empty_keywords_serializes_as_empty_array() {
        let m = Metadata::default();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"keywords\":[]"), "json: {json}");
    }
}
