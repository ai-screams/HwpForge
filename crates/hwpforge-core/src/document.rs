//! The Document type with typestate pattern.
//!
//! [`Document<S>`] is the aggregate root of the Core DOM. It uses the
//! **typestate pattern** to enforce document lifecycle at compile time:
//!
//! - [`Draft`] -- mutable, can add/remove sections
//! - [`Validated`] -- immutable structure, safe for serialization/export
//!
//! The transition `Draft -> Validated` is one-way via [`Document::validate()`],
//! which consumes the draft (move semantics prevent reuse).
//!
//! # Design Decisions
//!
//! - **Typestate, not enum** -- invalid operations are compile errors
//!   (not runtime panics). See Appendix D in the detailed plan.
//! - **Deserialize always to Draft** -- serialized data may be modified
//!   externally; re-validation is mandatory.
//! - **No `Styled` state in Phase 1** -- deferred to Phase 2 when
//!   Blueprint (StyleRegistry) is available.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::document::{Document, Draft, Validated};
//! use hwpforge_core::section::Section;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_core::run::Run;
//! use hwpforge_core::PageSettings;
//! use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! let mut doc = Document::new();
//! doc.add_section(Section::with_paragraphs(
//!     vec![Paragraph::with_runs(
//!         vec![Run::text("Hello", CharShapeIndex::new(0))],
//!         ParaShapeIndex::new(0),
//!     )],
//!     PageSettings::a4(),
//! ));
//!
//! let validated: Document<Validated> = doc.validate().unwrap();
//! assert_eq!(validated.section_count(), 1);
//! ```
//!
//! ```compile_fail
//! // A validated document cannot add sections:
//! use hwpforge_core::document::{Document, Validated};
//! use hwpforge_core::section::Section;
//! use hwpforge_core::PageSettings;
//!
//! # fn get_validated() -> Document<Validated> { todo!() }
//! let mut validated = get_validated();
//! validated.add_section(Section::new(PageSettings::a4()));
//! // ERROR: no method named `add_section` found for `Document<Validated>`
//! ```

use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::metadata::Metadata;
use crate::section::Section;
use crate::validate::validate_sections;

/// Marker type: the document is a mutable draft.
///
/// A `Document<Draft>` can be modified (add sections, set metadata)
/// and then validated via [`Document::validate()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draft;

/// Marker type: the document has passed structural validation.
///
/// A `Document<Validated>` is guaranteed to have:
/// - At least 1 section
/// - Every section has at least 1 paragraph
/// - Every paragraph has at least 1 run
/// - All table/control structural invariants hold
///
/// The only way to obtain a `Document<Validated>` is through
/// [`Document<Draft>::validate()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validated;

/// The document aggregate root with compile-time state tracking.
///
/// The generic parameter `S` determines which operations are available:
///
/// | State | Mutable | Serializable | Exportable |
/// |-------|---------|-------------|-----------|
/// | [`Draft`] | Yes | Yes | No (must validate first) |
/// | [`Validated`] | No | Yes | Yes |
///
/// # Typestate Safety
///
/// The `_state` field is private and zero-sized. There is no way to
/// construct a `Document<Validated>` except through `validate()`.
///
/// # Examples
///
/// ```
/// use hwpforge_core::document::Document;
/// use hwpforge_core::Metadata;
///
/// let doc = Document::with_metadata(Metadata {
///     title: Some("Report".to_string()),
///     ..Metadata::default()
/// });
/// assert!(doc.is_empty());
/// ```
pub struct Document<S = Draft> {
    sections: Vec<Section>,
    metadata: Metadata,
    _state: PhantomData<S>,
}

// ---------------------------------------------------------------------------
// Shared methods (any state)
// ---------------------------------------------------------------------------

impl<S> Document<S> {
    /// Returns a slice of all sections.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new();
    /// assert!(doc.sections().is_empty());
    /// ```
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Returns a reference to the document metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns the number of sections.
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Returns `true` if the document has no sections.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Draft-only methods
// ---------------------------------------------------------------------------

impl Document<Draft> {
    /// Creates a new empty draft document with default metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new();
    /// assert!(doc.is_empty());
    /// ```
    pub fn new() -> Self {
        Self { sections: Vec::new(), metadata: Metadata::default(), _state: PhantomData }
    }

    /// Creates a new draft document with the given metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::Metadata;
    ///
    /// let doc = Document::with_metadata(Metadata {
    ///     title: Some("Test".to_string()),
    ///     ..Metadata::default()
    /// });
    /// assert_eq!(doc.metadata().title.as_deref(), Some("Test"));
    /// ```
    pub fn with_metadata(metadata: Metadata) -> Self {
        Self { sections: Vec::new(), metadata, _state: PhantomData }
    }

    /// Appends a section to the draft document.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::PageSettings;
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::new(PageSettings::a4()));
    /// assert_eq!(doc.section_count(), 1);
    /// ```
    pub fn add_section(&mut self, section: Section) {
        self.sections.push(section);
    }

    /// Sets the document metadata.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = metadata;
    }

    /// Returns a mutable reference to the metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Returns a mutable slice of sections.
    pub fn sections_mut(&mut self) -> &mut [Section] {
        &mut self.sections
    }

    /// Validates the document structure and transitions to `Validated`.
    ///
    /// Consumes `self` (move semantics). On success, returns a
    /// `Document<Validated>`. On failure, returns a `CoreError`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Validation`](crate::error::CoreError::Validation) if the document violates any
    /// structural invariant (empty sections, empty paragraphs, etc.).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::PageSettings;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::with_paragraphs(
    ///     vec![Paragraph::with_runs(
    ///         vec![Run::text("Hello", CharShapeIndex::new(0))],
    ///         ParaShapeIndex::new(0),
    ///     )],
    ///     PageSettings::a4(),
    /// ));
    ///
    /// let validated = doc.validate().unwrap();
    /// assert_eq!(validated.section_count(), 1);
    /// ```
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new(); // empty
    /// assert!(doc.validate().is_err());
    /// ```
    pub fn validate(self) -> CoreResult<Document<Validated>> {
        validate_sections(&self.sections)?;
        Ok(Document { sections: self.sections, metadata: self.metadata, _state: PhantomData })
    }
}

impl Default for Document<Draft> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Manual trait impls (avoid T: Trait bounds on phantom type S)
// ---------------------------------------------------------------------------

impl<S> std::fmt::Debug for Document<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("sections", &self.sections)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl<S> Clone for Document<S> {
    fn clone(&self) -> Self {
        Self {
            sections: self.sections.clone(),
            metadata: self.metadata.clone(),
            _state: PhantomData,
        }
    }
}

impl<S> PartialEq for Document<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sections == other.sections && self.metadata == other.metadata
    }
}

impl<S> Eq for Document<S> {}

impl<S> std::fmt::Display for Document<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} sections)", self.sections.len())
    }
}

// ---------------------------------------------------------------------------
// Serde: serialize any state, deserialize only to Draft
// ---------------------------------------------------------------------------

impl<S> Serialize for Document<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Document", 2)?;
        state.serialize_field("sections", &self.sections)?;
        state.serialize_field("metadata", &self.metadata)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Document<Draft> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct DocumentData {
            sections: Vec<Section>,
            metadata: Metadata,
        }

        let data = DocumentData::deserialize(deserializer)?;
        Ok(Document { sections: data.sections, metadata: data.metadata, _state: PhantomData })
    }
}

// ---------------------------------------------------------------------------
// JsonSchema: hide PhantomData
// ---------------------------------------------------------------------------

impl<S> JsonSchema for Document<S> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Document".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "sections": gen.subschema_for::<Vec<Section>>(),
                "metadata": gen.subschema_for::<crate::metadata::Metadata>(),
            },
            "required": ["sections", "metadata"]
        })
    }
}

// ---------------------------------------------------------------------------
// Send + Sync verification
// ---------------------------------------------------------------------------

const _: () = {
    #[allow(dead_code)]
    fn assert_send<T: Send>() {}
    #[allow(dead_code)]
    fn assert_sync<T: Sync>() {}
    #[allow(dead_code)]
    fn verify() {
        assert_send::<Document<Draft>>();
        assert_sync::<Document<Draft>>();
        assert_send::<Document<Validated>>();
        assert_sync::<Document<Validated>>();
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::page::PageSettings;
    use crate::paragraph::Paragraph;
    use crate::run::Run;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn valid_section() -> Section {
        Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("Hello", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )
    }

    // === Construction ===

    #[test]
    fn new_creates_empty_draft() {
        let doc = Document::new();
        assert!(doc.is_empty());
        assert_eq!(doc.section_count(), 0);
        assert!(doc.metadata().title.is_none());
    }

    #[test]
    fn with_metadata() {
        let meta = Metadata { title: Some("Test".to_string()), ..Metadata::default() };
        let doc = Document::with_metadata(meta);
        assert_eq!(doc.metadata().title.as_deref(), Some("Test"));
    }

    #[test]
    fn default_is_new() {
        let a = Document::new();
        let b = Document::default();
        assert_eq!(a, b);
    }

    // === Draft mutations ===

    #[test]
    fn add_section() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        assert_eq!(doc.section_count(), 1);
        assert!(!doc.is_empty());
    }

    #[test]
    fn add_multiple_sections() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        assert_eq!(doc.section_count(), 3);
    }

    #[test]
    fn set_metadata() {
        let mut doc = Document::new();
        doc.set_metadata(Metadata { title: Some("New".to_string()), ..Metadata::default() });
        assert_eq!(doc.metadata().title.as_deref(), Some("New"));
    }

    #[test]
    fn metadata_mut() {
        let mut doc = Document::new();
        doc.metadata_mut().title = Some("Mutated".to_string());
        assert_eq!(doc.metadata().title.as_deref(), Some("Mutated"));
    }

    #[test]
    fn sections_mut() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        assert_eq!(doc.sections_mut().len(), 2);
    }

    // === Validation (Draft -> Validated) ===

    #[test]
    fn validate_success() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        assert_eq!(validated.section_count(), 1);
    }

    #[test]
    fn validate_empty_document_fails() {
        let doc = Document::new();
        let err = doc.validate().unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn validate_empty_section_fails() {
        let mut doc = Document::new();
        doc.add_section(Section::new(PageSettings::a4()));
        assert!(doc.validate().is_err());
    }

    #[test]
    fn validate_consumes_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let _validated = doc.validate().unwrap();
        // doc is moved -- attempting to use it would be a compile error
    }

    // === Validated state ===

    #[test]
    fn validated_has_read_methods() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();

        assert_eq!(validated.section_count(), 1);
        assert!(!validated.is_empty());
        assert_eq!(validated.sections().len(), 1);
        assert!(validated.metadata().title.is_none());
    }

    // === Display ===

    #[test]
    fn display_draft() {
        let doc = Document::new();
        assert_eq!(doc.to_string(), "Document(0 sections)");
    }

    #[test]
    fn display_validated() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        assert_eq!(validated.to_string(), "Document(1 sections)");
    }

    // === Equality ===

    #[test]
    fn equality_draft() {
        let mut a = Document::new();
        a.add_section(valid_section());
        let mut b = Document::new();
        b.add_section(valid_section());
        assert_eq!(a, b);
    }

    #[test]
    fn equality_validated() {
        let mut a = Document::new();
        a.add_section(valid_section());
        let mut b = Document::new();
        b.add_section(valid_section());
        let va = a.validate().unwrap();
        let vb = b.validate().unwrap();
        assert_eq!(va, vb);
    }

    // === Clone ===

    #[test]
    fn clone_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let cloned = doc.clone();
        assert_eq!(doc, cloned);
    }

    #[test]
    fn clone_validated() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        let cloned = validated.clone();
        assert_eq!(validated, cloned);
    }

    // === Serde ===

    #[test]
    fn serde_roundtrip_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.set_metadata(Metadata { title: Some("Test".to_string()), ..Metadata::default() });

        let json = serde_json::to_string(&doc).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn serde_roundtrip_validated_deserializes_to_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();

        let json = serde_json::to_string(&validated).unwrap();
        // Deserialize always produces Draft
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        // Must re-validate
        let re_validated = back.validate().unwrap();
        assert_eq!(validated, re_validated);
    }

    #[test]
    fn serde_empty_document() {
        let doc = Document::new();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    // === Complex document ===

    #[test]
    fn complex_document_roundtrip() {
        use crate::control::Control;
        use crate::image::{Image, ImageFormat};
        use crate::table::{Table, TableCell, TableRow};
        use hwpforge_foundation::HwpUnit;

        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("cell", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(50.0).unwrap(),
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);

        let link = Control::Hyperlink {
            text: "click".to_string(),
            url: "https://example.com".to_string(),
        };

        let img = Image::new(
            "test.png",
            HwpUnit::from_mm(10.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
            ImageFormat::Png,
        );

        let section = Section::with_paragraphs(
            vec![
                Paragraph::with_runs(
                    vec![
                        Run::text("Hello ", CharShapeIndex::new(0)),
                        Run::text("world", CharShapeIndex::new(1)),
                    ],
                    ParaShapeIndex::new(0),
                ),
                Paragraph::with_runs(
                    vec![Run::table(table, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(1),
                ),
                Paragraph::with_runs(
                    vec![Run::control(link, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
                Paragraph::with_runs(
                    vec![Run::image(img, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
            ],
            PageSettings::a4(),
        );

        let mut doc = Document::with_metadata(Metadata {
            title: Some("Complex Doc".to_string()),
            author: Some("Author".to_string()),
            keywords: vec!["test".to_string()],
            ..Metadata::default()
        });
        doc.add_section(section);

        let validated = doc.validate().unwrap();
        let json = serde_json::to_string_pretty(&validated).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        let re_validated = back.validate().unwrap();
        assert_eq!(validated, re_validated);
    }

    // === Debug ===

    #[test]
    fn debug_output() {
        let doc = Document::new();
        let s = format!("{doc:?}");
        assert!(s.contains("Document"), "debug: {s}");
        assert!(s.contains("sections"), "debug: {s}");
    }
}
