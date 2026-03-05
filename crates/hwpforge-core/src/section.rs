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

use hwpforge_foundation::{
    ApplyPageType, HwpUnit, NumberFormatType, PageNumberPosition, ShowMode, TextDirection,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::column::ColumnSettings;
use crate::page::PageSettings;
use crate::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

/// Controls visibility of headers, footers, master pages, borders, and fills.
///
/// Maps to `<hp:visibility>` inside `<hp:secPr>`. All flags default to
/// the standard 한글 values (show everything, no hiding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Visibility {
    /// Hide header on the first page.
    #[serde(default)]
    pub hide_first_header: bool,
    /// Hide footer on the first page.
    #[serde(default)]
    pub hide_first_footer: bool,
    /// Hide master page on the first page.
    #[serde(default)]
    pub hide_first_master_page: bool,
    /// Hide page number on the first page.
    #[serde(default)]
    pub hide_first_page_num: bool,
    /// Hide empty line on the first page.
    #[serde(default)]
    pub hide_first_empty_line: bool,
    /// Show line numbers in the section.
    #[serde(default)]
    pub show_line_number: bool,
    /// Border visibility mode.
    #[serde(default)]
    pub border: ShowMode,
    /// Fill visibility mode.
    #[serde(default)]
    pub fill: ShowMode,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            hide_first_header: false,
            hide_first_footer: false,
            hide_first_master_page: false,
            hide_first_page_num: false,
            hide_first_empty_line: false,
            show_line_number: false,
            border: ShowMode::ShowAll,
            fill: ShowMode::ShowAll,
        }
    }
}

// ---------------------------------------------------------------------------
// LineNumberShape
// ---------------------------------------------------------------------------

/// Line numbering settings for a section.
///
/// Maps to `<hp:lineNumberShape>` inside `<hp:secPr>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct LineNumberShape {
    /// Restart type: 0 = continuous, 1 = per page, 2 = per section.
    #[serde(default)]
    pub restart_type: u8,
    /// Count by N (show number every N lines, 0 = disabled).
    #[serde(default)]
    pub count_by: u16,
    /// Distance from text to line number (HwpUnit).
    #[serde(default)]
    pub distance: HwpUnit,
    /// Starting line number.
    #[serde(default)]
    pub start_number: u32,
}

// ---------------------------------------------------------------------------
// PageBorderFillEntry
// ---------------------------------------------------------------------------

/// A single page border/fill entry for the section.
///
/// Maps to `<hp:pageBorderFill>` inside `<hp:secPr>`.
/// Standard 한글 documents have 3 entries: BOTH, EVEN, ODD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageBorderFillEntry {
    /// Which pages this border fill applies to: `"BOTH"`, `"EVEN"`, `"ODD"`.
    pub apply_type: String,
    /// Reference to a borderFill definition (1-based index).
    #[serde(default = "PageBorderFillEntry::default_border_fill_id")]
    pub border_fill_id: u32,
    /// Whether the border is relative to text or paper.
    #[serde(default = "PageBorderFillEntry::default_text_border")]
    pub text_border: String,
    /// Whether header is inside the border.
    #[serde(default)]
    pub header_inside: bool,
    /// Whether footer is inside the border.
    #[serde(default)]
    pub footer_inside: bool,
    /// Fill area: `"PAPER"` or `"PAGE"`.
    #[serde(default = "PageBorderFillEntry::default_fill_area")]
    pub fill_area: String,
    /// Offset from page edge (left, right, top, bottom) in HwpUnit.
    #[serde(default = "PageBorderFillEntry::default_offset")]
    pub offset: [HwpUnit; 4],
}

impl PageBorderFillEntry {
    fn default_border_fill_id() -> u32 {
        1
    }
    fn default_text_border() -> String {
        "PAPER".to_string()
    }
    fn default_fill_area() -> String {
        "PAPER".to_string()
    }
    fn default_offset() -> [HwpUnit; 4] {
        // 1417 HwpUnit ≈ 5mm default offset
        [
            HwpUnit::new(1417).unwrap(),
            HwpUnit::new(1417).unwrap(),
            HwpUnit::new(1417).unwrap(),
            HwpUnit::new(1417).unwrap(),
        ]
    }
}

impl Default for PageBorderFillEntry {
    fn default() -> Self {
        Self {
            apply_type: "BOTH".to_string(),
            border_fill_id: 1,
            text_border: "PAPER".to_string(),
            header_inside: false,
            footer_inside: false,
            fill_area: "PAPER".to_string(),
            offset: Self::default_offset(),
        }
    }
}

// ---------------------------------------------------------------------------
// BeginNum
// ---------------------------------------------------------------------------

/// Starting numbers for various auto-numbering sequences.
///
/// Maps to `<hh:beginNum>` in header.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BeginNum {
    /// Starting page number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub page: u32,
    /// Starting footnote number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub footnote: u32,
    /// Starting endnote number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub endnote: u32,
    /// Starting picture number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub pic: u32,
    /// Starting table number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub tbl: u32,
    /// Starting equation number (default: 1).
    #[serde(default = "BeginNum::one")]
    pub equation: u32,
}

impl BeginNum {
    fn one() -> u32 {
        1
    }
}

impl Default for BeginNum {
    fn default() -> Self {
        Self { page: 1, footnote: 1, endnote: 1, pic: 1, tbl: 1, equation: 1 }
    }
}

// ---------------------------------------------------------------------------
// MasterPage
// ---------------------------------------------------------------------------

/// A master page (background/watermark page) for a section.
///
/// Master pages provide background content rendered behind the main body.
/// Maps to `<masterPage>` elements inside `<hp:secPr>`.
///
/// In HWPX, each master page has an `applyPageType` attribute
/// (`BOTH`, `EVEN`, or `ODD`) and contains its own paragraphs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MasterPage {
    /// Which pages this master page applies to.
    pub apply_page_type: ApplyPageType,
    /// Paragraphs composing the master page content.
    pub paragraphs: Vec<Paragraph>,
}

impl MasterPage {
    /// Creates a new master page with the given page type and paragraphs.
    pub fn new(apply_page_type: ApplyPageType, paragraphs: Vec<Paragraph>) -> Self {
        Self { apply_page_type, paragraphs }
    }
}

impl std::fmt::Display for MasterPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.paragraphs.len();
        let word = if n == 1 { "paragraph" } else { "paragraphs" };
        write!(f, "MasterPage({n} {word}, {:?})", self.apply_page_type)
    }
}

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

    /// Creates a header/footer applied to **all** pages (both odd and even).
    ///
    /// This is the most common case for simple documents that use a single
    /// header or footer on every page.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::HeaderFooter;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{ApplyPageType, ParaShapeIndex};
    ///
    /// let hf = HeaderFooter::all_pages(vec![Paragraph::new(ParaShapeIndex::new(0))]);
    /// assert_eq!(hf.apply_page_type, ApplyPageType::Both);
    /// assert_eq!(hf.paragraphs.len(), 1);
    /// ```
    pub fn all_pages(paragraphs: Vec<Paragraph>) -> Self {
        Self { paragraphs, apply_page_type: ApplyPageType::Both }
    }

    /// Creates a header/footer applied to all pages.
    #[deprecated(since = "0.2.0", note = "Use `all_pages()` instead")]
    pub fn both(paragraphs: Vec<Paragraph>) -> Self {
        Self::all_pages(paragraphs)
    }
}

impl std::fmt::Display for HeaderFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.paragraphs.len();
        let word = if n == 1 { "paragraph" } else { "paragraphs" };
        write!(f, "HeaderFooter({n} {word}, {:?})", self.apply_page_type)
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
    pub decoration: String,
}

impl PageNumber {
    /// Creates a new page number with no decoration.
    pub fn new(position: PageNumberPosition, number_format: NumberFormatType) -> Self {
        Self { position, number_format, decoration: String::new() }
    }

    /// Creates a page number at the bottom-center in plain digit format.
    ///
    /// This is the most common page number layout for Korean documents.
    /// Equivalent to `PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit)`
    /// with an empty `decoration`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::PageNumber;
    /// use hwpforge_foundation::{NumberFormatType, PageNumberPosition};
    ///
    /// let pn = PageNumber::bottom_center();
    /// assert_eq!(pn.position, PageNumberPosition::BottomCenter);
    /// assert_eq!(pn.number_format, NumberFormatType::Digit);
    /// assert!(pn.decoration.is_empty());
    /// ```
    pub fn bottom_center() -> Self {
        Self {
            position: PageNumberPosition::BottomCenter,
            number_format: NumberFormatType::Digit,
            decoration: String::new(),
        }
    }

    /// Creates a new page number with decoration characters placed around the number.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::PageNumber;
    /// use hwpforge_foundation::{NumberFormatType, PageNumberPosition};
    ///
    /// let pn = PageNumber::with_decoration(
    ///     PageNumberPosition::BottomCenter,
    ///     NumberFormatType::Digit,
    ///     "- ",
    /// );
    /// assert_eq!(pn.decoration, "- ");
    /// ```
    pub fn with_decoration(
        position: PageNumberPosition,
        number_format: NumberFormatType,
        decoration: impl Into<String>,
    ) -> Self {
        Self { position, number_format, decoration: decoration.into() }
    }

    /// Creates a new page number with side decoration characters.
    #[deprecated(since = "0.2.0", note = "Use `with_decoration()` instead")]
    pub fn with_side_char(
        position: PageNumberPosition,
        number_format: NumberFormatType,
        side_char: impl Into<String>,
    ) -> Self {
        Self::with_decoration(position, number_format, side_char)
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
    /// Multi-column layout. `None` = single column (default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_settings: Option<ColumnSettings>,
    /// Visibility flags for headers, footers, borders, etc.
    /// `None` = default visibility (show everything).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    /// Line numbering settings. `None` = no line numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_number_shape: Option<LineNumberShape>,
    /// Page border/fill entries. `None` = default 3 entries (BOTH/EVEN/ODD with borderFillIDRef=1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_border_fills: Option<Vec<PageBorderFillEntry>>,
    /// Master pages (background content rendered behind the body).
    /// `None` = no master pages (default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_pages: Option<Vec<MasterPage>>,
    /// Starting numbers for auto-numbering sequences.
    /// `None` = default values (all start at 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub begin_num: Option<BeginNum>,
    /// Text writing direction for this section.
    /// Defaults to [`TextDirection::Horizontal`] (가로쓰기).
    #[serde(default)]
    pub text_direction: TextDirection,
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
            column_settings: None,
            visibility: None,
            line_number_shape: None,
            page_border_fills: None,
            master_pages: None,
            begin_num: None,
            text_direction: TextDirection::Horizontal,
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
        Self {
            paragraphs,
            page_settings,
            header: None,
            footer: None,
            page_number: None,
            column_settings: None,
            visibility: None,
            line_number_shape: None,
            page_border_fills: None,
            master_pages: None,
            begin_num: None,
            text_direction: TextDirection::Horizontal,
        }
    }

    /// Sets the text writing direction for this section and returns `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::PageSettings;
    /// use hwpforge_foundation::TextDirection;
    ///
    /// let section = Section::new(PageSettings::a4())
    ///     .with_text_direction(TextDirection::Vertical);
    /// assert_eq!(section.text_direction, TextDirection::Vertical);
    /// ```
    pub fn with_text_direction(mut self, dir: TextDirection) -> Self {
        self.text_direction = dir;
        self
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
        let n = self.paragraphs.len();
        let word = if n == 1 { "paragraph" } else { "paragraphs" };
        write!(f, "Section({n} {word})")
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
    fn display_singular() {
        let section = Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4());
        assert_eq!(section.to_string(), "Section(1 paragraph)");
    }

    #[test]
    fn display_plural() {
        let section = Section::with_paragraphs(
            vec![simple_paragraph(), simple_paragraph()],
            PageSettings::a4(),
        );
        assert_eq!(section.to_string(), "Section(2 paragraphs)");
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
        assert!(s.contains("1 paragraph"), "display: {s}");
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
        assert!(pn.decoration.is_empty());
    }

    #[test]
    fn page_number_with_decoration() {
        let pn = PageNumber::with_decoration(
            PageNumberPosition::BottomCenter,
            NumberFormatType::RomanCapital,
            "- ",
        );
        assert_eq!(pn.decoration, "- ");
        assert_eq!(pn.number_format, NumberFormatType::RomanCapital);
    }

    #[test]
    #[allow(deprecated)]
    fn page_number_with_side_char_deprecated() {
        let pn = PageNumber::with_side_char(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "- ",
        );
        assert_eq!(pn.decoration, "- ");
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
        let pn = PageNumber::with_decoration(
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
        assert!(section.column_settings.is_none());
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
        // Section-level header/footer/page_number/column_settings should not appear
        // (PageSettings has header_margin/footer_margin, which is different)
        assert!(!json.contains("\"header\""));
        assert!(!json.contains("\"footer\""));
        assert!(!json.contains("\"page_number\""));
        assert!(!json.contains("\"column_settings\""));
        let back: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    // -----------------------------------------------------------------------
    // HeaderFooter::all_pages tests
    // -----------------------------------------------------------------------

    #[test]
    fn header_footer_all_pages_apply_page_type() {
        let hf = HeaderFooter::all_pages(vec![Paragraph::new(ParaShapeIndex::new(0))]);
        assert_eq!(hf.apply_page_type, ApplyPageType::Both);
    }

    #[test]
    fn header_footer_all_pages_preserves_paragraphs() {
        let paras = vec![simple_paragraph(), simple_paragraph()];
        let hf = HeaderFooter::all_pages(paras);
        assert_eq!(hf.paragraphs.len(), 2);
    }

    #[test]
    fn header_footer_all_pages_empty_paragraphs() {
        let hf = HeaderFooter::all_pages(vec![]);
        assert_eq!(hf.apply_page_type, ApplyPageType::Both);
        assert!(hf.paragraphs.is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn header_footer_both_deprecated_alias() {
        let hf = HeaderFooter::both(vec![Paragraph::new(ParaShapeIndex::new(0))]);
        assert_eq!(hf.apply_page_type, ApplyPageType::Both);
    }

    // -----------------------------------------------------------------------
    // PageNumber::bottom_center tests
    // -----------------------------------------------------------------------

    #[test]
    fn page_number_bottom_center_position() {
        let pn = PageNumber::bottom_center();
        assert_eq!(pn.position, PageNumberPosition::BottomCenter);
    }

    #[test]
    fn page_number_bottom_center_format() {
        let pn = PageNumber::bottom_center();
        assert_eq!(pn.number_format, NumberFormatType::Digit);
    }

    #[test]
    fn page_number_bottom_center_no_decoration() {
        let pn = PageNumber::bottom_center();
        assert!(pn.decoration.is_empty());
    }

    #[test]
    fn page_number_bottom_center_equals_explicit() {
        let shortcut = PageNumber::bottom_center();
        let explicit = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        assert_eq!(shortcut, explicit);
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

    #[test]
    fn all_pages_equals_new_with_both() {
        let paras = vec![simple_paragraph()];
        let from_all_pages = HeaderFooter::all_pages(paras.clone());
        let from_new = HeaderFooter::new(paras, ApplyPageType::Both);
        assert_eq!(from_all_pages, from_new);
    }
}
