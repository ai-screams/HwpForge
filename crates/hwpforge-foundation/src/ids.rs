//! String-based identifiers for fonts, templates, and styles.
//!
//! Each identifier is a validated, non-empty string wrapped in a distinct
//! newtype for compile-time safety. You cannot accidentally pass a
//! [`FontId`] where a [`StyleName`] is expected.
//!
//! # Phase 1 Migration
//!
//! The internal `String` will migrate to an interned representation
//! (e.g. `lasso::Spur` or `string_cache::Atom`) for O(1) equality
//! and memory deduplication. The public API (`new`, `as_str`) stays
//! identical.
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::FontId;
//!
//! let font = FontId::new("Batang").unwrap();
//! assert_eq!(font.as_str(), "Batang");
//! assert_eq!(font.to_string(), "Batang");
//!
//! // Empty string is rejected
//! assert!(FontId::new("").is_err());
//! ```

use crate::macros::string_newtype;

string_newtype! {
    /// A font identifier (e.g. `"Batang"`, `"Dotum"`, `"Arial"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::FontId;
    ///
    /// let f = FontId::new("한컴바탕").unwrap();
    /// assert_eq!(f.as_str(), "한컴바탕");
    /// ```
    FontId, "FontId"
}

string_newtype! {
    /// A template name (e.g. `"gov_proposal"`, `"letter"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::TemplateName;
    ///
    /// let t = TemplateName::new("gov_proposal").unwrap();
    /// assert_eq!(t.as_str(), "gov_proposal");
    /// ```
    TemplateName, "TemplateName"
}

string_newtype! {
    /// A style name (e.g. `"heading1"`, `"본문"`, `"normal"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::StyleName;
    ///
    /// let s = StyleName::new("heading1").unwrap();
    /// assert_eq!(s.as_str(), "heading1");
    /// ```
    StyleName, "StyleName"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FoundationError;

    // ===================================================================
    // FontId edge cases (10+)
    // ===================================================================

    // Edge Case 1: Empty string -> error
    #[test]
    fn fontid_empty_is_error() {
        let err = FontId::new("").unwrap_err();
        match err {
            FoundationError::EmptyIdentifier { ref item } => {
                assert_eq!(item, "FontId");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    // Edge Case 2: Single character -> OK
    #[test]
    fn fontid_single_char() {
        let f = FontId::new("A").unwrap();
        assert_eq!(f.as_str(), "A");
    }

    // Edge Case 3: Korean characters
    #[test]
    fn fontid_korean() {
        let f = FontId::new("한컴바탕").unwrap();
        assert_eq!(f.as_str(), "한컴바탕");
    }

    // Edge Case 4: Special characters (hyphen, underscore)
    #[test]
    fn fontid_special_chars() {
        let f = FontId::new("D2Coding-Bold_Italic").unwrap();
        assert_eq!(f.as_str(), "D2Coding-Bold_Italic");
    }

    // Edge Case 5: Unicode emoji
    #[test]
    fn fontid_unicode_emoji() {
        let f = FontId::new("Font\u{1F600}").unwrap();
        assert!(f.as_str().contains('\u{1F600}'));
    }

    // Edge Case 6: Very long name
    #[test]
    fn fontid_long_name() {
        let long = "x".repeat(10_000);
        let f = FontId::new(long.clone()).unwrap();
        assert_eq!(f.as_str().len(), 10_000);
    }

    // Edge Case 7: Equality
    #[test]
    fn fontid_equality() {
        let a = FontId::new("Arial").unwrap();
        let b = FontId::new("Arial").unwrap();
        let c = FontId::new("Batang").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // Edge Case 8: Hash
    #[test]
    fn fontid_hash_in_map() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let key = FontId::new("Batang").unwrap();
        map.insert(key.clone(), 42);
        assert_eq!(map[&key], 42);
    }

    // Edge Case 9: Display
    #[test]
    fn fontid_display() {
        let f = FontId::new("Arial").unwrap();
        assert_eq!(f.to_string(), "Arial");
    }

    // Edge Case 10: Serde roundtrip
    #[test]
    fn fontid_serde_roundtrip() {
        let f = FontId::new("Batang").unwrap();
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, "\"Batang\"");
        let back: FontId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    // ===================================================================
    // TemplateName
    // ===================================================================

    #[test]
    fn template_name_empty_is_error() {
        assert!(TemplateName::new("").is_err());
    }

    #[test]
    fn template_name_valid() {
        let t = TemplateName::new("gov_proposal").unwrap();
        assert_eq!(t.as_str(), "gov_proposal");
    }

    // ===================================================================
    // StyleName
    // ===================================================================

    #[test]
    fn style_name_empty_is_error() {
        assert!(StyleName::new("").is_err());
    }

    #[test]
    fn style_name_valid() {
        let s = StyleName::new("heading1").unwrap();
        assert_eq!(s.as_str(), "heading1");
    }

    // ===================================================================
    // Cross-type safety
    // ===================================================================

    // Edge Case: FontId and TemplateName are distinct types
    // (This is a compile-time guarantee; the test below just documents it.)
    #[test]
    fn id_types_are_distinct() {
        fn accept_font(_: &FontId) {}
        fn accept_template(_: &TemplateName) {}
        let f = FontId::new("x").unwrap();
        let t = TemplateName::new("x").unwrap();
        accept_font(&f);
        accept_template(&t);
        // accept_font(&t); // Would not compile -- type safety!
    }

    // ===================================================================
    // AsRef<str>
    // ===================================================================

    #[test]
    fn fontid_as_ref() {
        let f = FontId::new("test").unwrap();
        let s: &str = f.as_ref();
        assert_eq!(s, "test");
    }
}
