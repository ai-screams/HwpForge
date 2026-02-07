//! Document sections.
//!
//! A [`Section`] is a contiguous block of paragraphs sharing the same
//! [`PageSettings`]. Typical HWP documents have one section, but
//! complex reports may mix portrait and landscape sections.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::section::Section;
//! use hwpforge_core::PageSettings;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_core::run::Run;
//! use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! let mut section = Section::new(PageSettings::a4());
//! section.add_paragraph(Paragraph::with_runs(
//!     vec![Run::text("Hello", CharShapeIndex::new(0))],
//!     ParaShapeIndex::new(0),
//! ));
//! assert_eq!(section.paragraph_count(), 1);
//! ```

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::page::PageSettings;
use crate::paragraph::Paragraph;

/// A document section: paragraphs + page geometry.
///
/// # Examples
///
/// ```
/// use hwpforge_core::section::Section;
/// use hwpforge_core::PageSettings;
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::ParaShapeIndex;
///
/// let section = Section::with_paragraphs(
///     vec![Paragraph::new(ParaShapeIndex::new(0))],
///     PageSettings::a4(),
/// );
/// assert_eq!(section.paragraph_count(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    /// Ordered paragraphs in this section.
    pub paragraphs: Vec<Paragraph>,
    /// Page dimensions and margins for this section.
    pub page_settings: PageSettings,
}

impl Section {
    /// Creates an empty section with the given page settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::PageSettings;
    ///
    /// let section = Section::new(PageSettings::a4());
    /// assert!(section.is_empty());
    /// ```
    pub fn new(page_settings: PageSettings) -> Self {
        Self { paragraphs: Vec::new(), page_settings }
    }

    /// Creates a section with pre-built paragraphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::PageSettings;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let section = Section::with_paragraphs(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     PageSettings::letter(),
    /// );
    /// assert_eq!(section.paragraph_count(), 1);
    /// ```
    pub fn with_paragraphs(paragraphs: Vec<Paragraph>, page_settings: PageSettings) -> Self {
        Self { paragraphs, page_settings }
    }

    /// Appends a paragraph to this section.
    pub fn add_paragraph(&mut self, paragraph: Paragraph) {
        self.paragraphs.push(paragraph);
    }

    /// Returns the number of paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.paragraphs.len()
    }

    /// Returns `true` if this section has no paragraphs.
    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }
}

impl std::fmt::Display for Section {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Section({} paragraphs)", self.paragraphs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use crate::run::Run;

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text("text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    }

    #[test]
    fn new_is_empty() {
        let section = Section::new(PageSettings::a4());
        assert!(section.is_empty());
        assert_eq!(section.paragraph_count(), 0);
    }

    #[test]
    fn with_paragraphs() {
        let section = Section::with_paragraphs(
            vec![simple_paragraph(), simple_paragraph()],
            PageSettings::a4(),
        );
        assert_eq!(section.paragraph_count(), 2);
        assert!(!section.is_empty());
    }

    #[test]
    fn add_paragraph() {
        let mut section = Section::new(PageSettings::a4());
        section.add_paragraph(simple_paragraph());
        section.add_paragraph(simple_paragraph());
        assert_eq!(section.paragraph_count(), 2);
    }

    #[test]
    fn page_settings_preserved() {
        let section = Section::new(PageSettings::letter());
        assert_eq!(section.page_settings, PageSettings::letter());
    }

    #[test]
    fn display() {
        let section = Section::with_paragraphs(
            vec![simple_paragraph()],
            PageSettings::a4(),
        );
        assert_eq!(section.to_string(), "Section(1 paragraphs)");
    }

    #[test]
    fn equality() {
        let a = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
        let b = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_different_page_settings() {
        let a = Section::new(PageSettings::a4());
        let b = Section::new(PageSettings::letter());
        assert_ne!(a, b);
    }

    #[test]
    fn clone_independence() {
        let section = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
        let mut cloned = section.clone();
        cloned.add_paragraph(simple_paragraph());
        assert_eq!(section.paragraph_count(), 1);
        assert_eq!(cloned.paragraph_count(), 2);
    }

    #[test]
    fn serde_roundtrip() {
        let section = Section::with_paragraphs(
            vec![simple_paragraph()],
            PageSettings::a4(),
        );
        let json = serde_json::to_string(&section).unwrap();
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    #[test]
    fn serde_empty_section() {
        let section = Section::new(PageSettings::a4());
        let json = serde_json::to_string(&section).unwrap();
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    #[test]
    fn serde_letter_page() {
        let section = Section::new(PageSettings::letter());
        let json = serde_json::to_string(&section).unwrap();
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }
}
