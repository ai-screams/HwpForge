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

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Document metadata: title, author, subject, keywords, timestamps.
///
/// All fields are optional. `Default` returns a fully empty metadata
/// (all `None` / empty `Vec` / empty map).
///
/// # Design Decisions
///
/// **Timestamps** use `Option<String>` (ISO 8601) instead of `chrono::DateTime`.
/// Rationale: `chrono` adds ~250KB compile weight for two fields that Core
/// never does arithmetic on. Smithy crates parse and validate dates when
/// reading from format-specific sources.
///
/// **`#[non_exhaustive]`** mirrors the established Core pattern
/// ([`Control`](crate::control::Control), shape enums, etc.). New fields
/// can be added in future versions without breaking external struct
/// literals — callers must use `..Default::default()`. See `CLAUDE.md`
/// "semver-first" working principle.
///
/// **`extras`** carries `<opf:meta name="X">` entries (HWPX) or
/// PropertySet entries (HWP5) that have not yet been promoted to typed
/// fields. Uses [`BTreeMap`] for deterministic ordering so encoder output
/// is byte-stable for round-trip tests. Keys are the wire `name=` value
/// (e.g. `"category"`); values are the raw text content.
///
/// # Examples
///
/// ```
/// use hwpforge_core::Metadata;
///
/// let meta = Metadata::default();
/// assert!(meta.title.is_none());
/// assert!(meta.keywords.is_empty());
/// assert!(meta.extras.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct Metadata {
    /// Document title.
    pub title: Option<String>,
    /// Document author (corresponds to HWPX `<opf:meta name="creator">`).
    pub author: Option<String>,
    /// Document subject (corresponds to HWPX `<opf:meta name="subject">`).
    pub subject: Option<String>,
    /// Free-form description (corresponds to HWPX
    /// `<opf:meta name="description">`).
    ///
    /// Distinct from [`subject`](Self::subject): `subject` is the
    /// canonical "subject" line; `description` carries longer prose
    /// summary text that Hancom stores separately.
    pub description: Option<String>,
    /// Last person to save the document (corresponds to HWPX
    /// `<opf:meta name="lastsaveby">`).
    ///
    /// Distinct from [`author`](Self::author): `author` is the document
    /// creator; `last_saved_by` is the most recent editor. Hancom
    /// surfaces this via the `$lastsaveby` SUMMERY auto-field.
    pub last_saved_by: Option<String>,
    /// Searchable keywords.
    ///
    /// Encoders that target HWPX join with `";"` into a single
    /// `<opf:meta name="keyword">` element (matches Hancom convention).
    pub keywords: Vec<String>,
    /// Creation timestamp in ISO 8601 format (e.g. `"2026-02-07T10:30:00Z"`).
    pub created: Option<String>,
    /// Last modification timestamp in ISO 8601 format.
    pub modified: Option<String>,
    /// Carry slot for `<opf:meta>` keys not yet promoted to typed
    /// fields. Preserves lossless round-trip when Hancom adds new
    /// metadata names that HwpForge has not modeled yet.
    pub extras: BTreeMap<String, String>,
}

impl Metadata {
    /// Returns a fresh [`Metadata`] with all fields at their default
    /// (`None` / empty). Equivalent to [`Metadata::default()`]; provided
    /// as a chainable seed for builder-style construction so external
    /// crates can populate the `#[non_exhaustive]` struct without
    /// struct-literal syntax.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the document [`title`](Self::title).
    #[must_use]
    pub fn with_title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }

    /// Sets the document [`author`](Self::author).
    #[must_use]
    pub fn with_author(mut self, value: impl Into<String>) -> Self {
        self.author = Some(value.into());
        self
    }

    /// Sets the document [`subject`](Self::subject).
    #[must_use]
    pub fn with_subject(mut self, value: impl Into<String>) -> Self {
        self.subject = Some(value.into());
        self
    }

    /// Sets the document [`description`](Self::description).
    #[must_use]
    pub fn with_description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    /// Sets the document [`last_saved_by`](Self::last_saved_by).
    #[must_use]
    pub fn with_last_saved_by(mut self, value: impl Into<String>) -> Self {
        self.last_saved_by = Some(value.into());
        self
    }

    /// Replaces the document [`keywords`](Self::keywords) list.
    #[must_use]
    pub fn with_keywords<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = values.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the [`created`](Self::created) timestamp (ISO 8601).
    #[must_use]
    pub fn with_created(mut self, value: impl Into<String>) -> Self {
        self.created = Some(value.into());
        self
    }

    /// Sets the [`modified`](Self::modified) timestamp (ISO 8601).
    #[must_use]
    pub fn with_modified(mut self, value: impl Into<String>) -> Self {
        self.modified = Some(value.into());
        self
    }

    /// Inserts a single `<opf:meta>` carry entry into
    /// [`extras`](Self::extras). Chainable.
    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extras.insert(key.into(), value.into());
        self
    }
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
        assert!(m.description.is_none());
        assert!(m.last_saved_by.is_none());
        assert!(m.keywords.is_empty());
        assert!(m.created.is_none());
        assert!(m.modified.is_none());
        assert!(m.extras.is_empty());
    }

    #[test]
    fn struct_literal_construction() {
        let mut extras = BTreeMap::new();
        extras.insert("category".to_string(), "Test".to_string());
        let m = Metadata {
            title: Some("Test".to_string()),
            author: Some("Author".to_string()),
            subject: Some("Subject".to_string()),
            description: Some("Long form description".to_string()),
            last_saved_by: Some("Editor".to_string()),
            keywords: vec!["rust".to_string(), "hwp".to_string()],
            created: Some("2026-02-07T00:00:00Z".to_string()),
            modified: Some("2026-02-07T12:00:00Z".to_string()),
            extras,
        };
        assert_eq!(m.title.as_deref(), Some("Test"));
        assert_eq!(m.keywords.len(), 2);
        assert_eq!(m.description.as_deref(), Some("Long form description"));
        assert_eq!(m.last_saved_by.as_deref(), Some("Editor"));
        assert_eq!(m.extras.get("category").map(String::as_str), Some("Test"));
    }

    #[test]
    fn extras_btree_ordering_is_deterministic() {
        // BTreeMap preserves key ordering — important for byte-stable
        // encoder output and predictable round-trip diffs.
        let mut m = Metadata::default();
        m.extras.insert("zeta".to_string(), "z".to_string());
        m.extras.insert("alpha".to_string(), "a".to_string());
        m.extras.insert("mu".to_string(), "m".to_string());
        let keys: Vec<&str> = m.extras.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn partial_construction_with_defaults() {
        let m = Metadata { title: Some("Report".to_string()), ..Metadata::default() };
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
        let mut extras = BTreeMap::new();
        extras.insert("category".to_string(), "draft".to_string());
        let m = Metadata {
            title: Some("Test".to_string()),
            author: Some("Author".to_string()),
            subject: None,
            description: Some("body".to_string()),
            last_saved_by: Some("Editor".to_string()),
            keywords: vec!["a".to_string(), "b".to_string()],
            created: Some("2026-02-07T00:00:00Z".to_string()),
            modified: None,
            extras,
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
