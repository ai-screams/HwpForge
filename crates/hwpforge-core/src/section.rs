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

use hwpforge_foundation::{ApplyPageType, NumberFormatType, PageNumberPosition};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::page::PageSettings;
use crate::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// HeaderFooter
// ---------------------------------------------------------------------------

/// A header or footer region containing paragraphs.
///
/// In HWPX, headers and footers appear as `<hp:header>` / `<hp:footer>`
/// elements inside `<hp:ctrl>` in the section body. Each contains its own
/// paragraphs and an [`ApplyPageType`] controlling which pages it applies to.
///
/// # Examples
///
/// ```
/// use hwpforge_core::section::HeaderFooter;
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{ApplyPageType, ParaShapeIndex};
///
/// let hf = HeaderFooter::new(
///     vec![Paragraph::new(ParaShapeIndex::new(0))],
///     ApplyPageType::Both,
/// );
/// assert_eq!(hf.paragraphs.len(), 1);
/// assert_eq!(hf.apply_page_type, ApplyPageType::Both);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HeaderFooter {
    /// Paragraphs composing the header/footer content.
    pub paragraphs: Vec<Paragraph>,
    /// Which pages this header/footer applies to.
    pub apply_page_type: ApplyPageType,
}

impl HeaderFooter {
    /// Creates a new header/footer with the given paragraphs and page scope.
    pub fn new(paragraphs: Vec<Paragraph>, apply_page_type: ApplyPageType) -> Self {
        Self { paragraphs, apply_page_type }
    }
}

impl std::fmt::Display for HeaderFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HeaderFooter({} paragraphs, {:?})", self.paragraphs.len(), self.apply_page_type)
    }
}

// ---------------------------------------------------------------------------
// PageNumber
// ---------------------------------------------------------------------------

/// Page number display settings for a section.
///
/// In HWPX, page numbers appear as `<hp:pageNum>` inside `<hp:ctrl>`.
/// This struct controls position, format, and optional decoration characters.
///
/// # Examples
///
/// ```
/// use hwpforge_core::section::PageNumber;
/// use hwpforge_foundation::{NumberFormatType, PageNumberPosition};
///
/// let pn = PageNumber::new(
///     PageNumberPosition::BottomCenter,
///     NumberFormatType::Digit,
/// );
/// assert_eq!(pn.position, PageNumberPosition::BottomCenter);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageNumber {
    /// Where to display the page number.
    pub position: PageNumberPosition,
    /// Numbering format (digits, roman, etc.).
    pub number_format: NumberFormatType,
    /// Optional decoration string placed around the number
    /// (e.g. `"- "` for `"- 1 -"`). Empty means no decoration.
    pub side_char: String,
}

impl PageNumber {
    /// Creates a new page number with no side decoration.
    pub fn new(position: PageNumberPosition, number_format: NumberFormatType) -> Self {
        Self { position, number_format, side_char: String::new() }
    }

    /// Creates a new page number with side decoration characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::PageNumber;
    /// use hwpforge_foundation::{NumberFormatType, PageNumberPosition};
    ///
    /// let pn = PageNumber::with_side_char(
    ///     PageNumberPosition::BottomCenter,
    ///     NumberFormatType::Digit,
    ///     "- ",
    /// );
    /// assert_eq!(pn.side_char, "- ");
    /// ```
    pub fn with_side_char(
        position: PageNumberPosition,
        number_format: NumberFormatType,
        side_char: impl Into<String>,
    ) -> Self {
        Self { position, number_format, side_char: side_char.into() }
    }
}

impl std::fmt::Display for PageNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PageNumber({:?}, {:?})", self.position, self.number_format)
    }
}

// ---------------------------------------------------------------------------
// Section
// ---------------------------------------------------------------------------

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
    /// Optional header for this section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<HeaderFooter>,
    /// Optional footer for this section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<HeaderFooter>,
    /// Optional page number settings for this section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_number: Option<PageNumber>,
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
        Self {
            paragraphs: Vec::new(),
            page_settings,
            header: None,
            footer: None,
            page_number: None,
        }
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
        Self { paragraphs, page_settings, header: None, footer: None, page_number: None }
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
    use crate::run::Run;
    use hwpforge_foundation::{
        ApplyPageType, CharShapeIndex, NumberFormatType, PageNumberPosition, ParaShapeIndex,
    };

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
        let section = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
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
        let section = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
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

    // -----------------------------------------------------------------------
    // HeaderFooter tests
    // -----------------------------------------------------------------------

    #[test]
    fn header_footer_new() {
        let hf =
            HeaderFooter::new(vec![Paragraph::new(ParaShapeIndex::new(0))], ApplyPageType::Both);
        assert_eq!(hf.paragraphs.len(), 1);
        assert_eq!(hf.apply_page_type, ApplyPageType::Both);
    }

    #[test]
    fn header_footer_even_odd() {
        let even = HeaderFooter::new(vec![], ApplyPageType::Even);
        let odd = HeaderFooter::new(vec![], ApplyPageType::Odd);
        assert_eq!(even.apply_page_type, ApplyPageType::Even);
        assert_eq!(odd.apply_page_type, ApplyPageType::Odd);
        assert_ne!(even, odd);
    }

    #[test]
    fn header_footer_display() {
        let hf =
            HeaderFooter::new(vec![Paragraph::new(ParaShapeIndex::new(0))], ApplyPageType::Both);
        let s = hf.to_string();
        assert!(s.contains("1 paragraphs"), "display: {s}");
        assert!(s.contains("Both"), "display: {s}");
    }

    #[test]
    fn header_footer_serde_roundtrip() {
        let hf = HeaderFooter::new(
            vec![Paragraph::with_runs(
                vec![Run::text("Header text", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            ApplyPageType::Both,
        );
        let json = serde_json::to_string(&hf).unwrap();
        let back: HeaderFooter = serde_json::from_str(&json).unwrap();
        assert_eq!(hf, back);
    }

    #[test]
    fn header_footer_clone_independence() {
        let hf =
            HeaderFooter::new(vec![Paragraph::new(ParaShapeIndex::new(0))], ApplyPageType::Both);
        let mut cloned = hf.clone();
        cloned.paragraphs.push(Paragraph::new(ParaShapeIndex::new(1)));
        assert_eq!(hf.paragraphs.len(), 1);
        assert_eq!(cloned.paragraphs.len(), 2);
    }

    // -----------------------------------------------------------------------
    // PageNumber tests
    // -----------------------------------------------------------------------

    #[test]
    fn page_number_new() {
        let pn = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        assert_eq!(pn.position, PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, NumberFormatType::Digit);
        assert!(pn.side_char.is_empty());
    }

    #[test]
    fn page_number_with_side_char() {
        let pn = PageNumber::with_side_char(
            PageNumberPosition::BottomCenter,
            NumberFormatType::RomanCapital,
            "- ",
        );
        assert_eq!(pn.side_char, "- ");
        assert_eq!(pn.number_format, NumberFormatType::RomanCapital);
    }

    #[test]
    fn page_number_display() {
        let pn = PageNumber::new(PageNumberPosition::TopCenter, NumberFormatType::Digit);
        let s = pn.to_string();
        assert!(s.contains("TopCenter"), "display: {s}");
        assert!(s.contains("Digit"), "display: {s}");
    }

    #[test]
    fn page_number_serde_roundtrip() {
        let pn = PageNumber::with_side_char(
            PageNumberPosition::BottomCenter,
            NumberFormatType::CircledDigit,
            "< ",
        );
        let json = serde_json::to_string(&pn).unwrap();
        let back: PageNumber = serde_json::from_str(&json).unwrap();
        assert_eq!(pn, back);
    }

    #[test]
    fn page_number_equality() {
        let a = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        let b = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        assert_eq!(a, b);
    }

    #[test]
    fn page_number_inequality() {
        let a = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        let b = PageNumber::new(PageNumberPosition::TopCenter, NumberFormatType::Digit);
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // Section with header/footer/page_number
    // -----------------------------------------------------------------------

    #[test]
    fn section_new_has_none_fields() {
        let section = Section::new(PageSettings::a4());
        assert!(section.header.is_none());
        assert!(section.footer.is_none());
        assert!(section.page_number.is_none());
    }

    #[test]
    fn section_with_header_footer() {
        let mut section = Section::new(PageSettings::a4());
        section.header = Some(HeaderFooter::new(
            vec![Paragraph::with_runs(
                vec![Run::text("Header", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            ApplyPageType::Both,
        ));
        section.footer = Some(HeaderFooter::new(
            vec![Paragraph::with_runs(
                vec![Run::text("Footer", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            ApplyPageType::Both,
        ));
        assert!(section.header.is_some());
        assert!(section.footer.is_some());
    }

    #[test]
    fn section_with_page_number() {
        let mut section = Section::new(PageSettings::a4());
        section.page_number =
            Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));
        assert!(section.page_number.is_some());
    }

    #[test]
    fn section_serde_with_optional_fields() {
        let mut section = Section::new(PageSettings::a4());
        section.header = Some(HeaderFooter::new(vec![], ApplyPageType::Both));
        section.page_number =
            Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));
        let json = serde_json::to_string(&section).unwrap();
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    #[test]
    fn section_serde_none_fields_skipped() {
        let section = Section::new(PageSettings::a4());
        let json = serde_json::to_string(&section).unwrap();
        // Section-level header/footer/page_number should not appear
        // (PageSettings has header_margin/footer_margin, which is different)
        assert!(!json.contains("\"header\""));
        assert!(!json.contains("\"footer\""));
        assert!(!json.contains("\"page_number\""));
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    #[test]
    fn section_backward_compat_deserialize() {
        // JSON without header/footer/page_number fields (pre-4.5 format)
        let a4 = PageSettings::a4();
        let json = serde_json::to_string(&Section::with_paragraphs(vec![], a4)).unwrap();
        let section: Section = serde_json::from_str(&json).unwrap();
        assert!(section.header.is_none());
        assert!(section.footer.is_none());
        assert!(section.page_number.is_none());
    }
}
