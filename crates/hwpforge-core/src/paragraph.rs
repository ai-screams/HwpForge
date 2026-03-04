//! Paragraph: a sequence of runs with a paragraph shape reference.
//!
//! [`Paragraph`] aggregates [`Run`] objects and holds
//! a [`ParaShapeIndex`] reference to the paragraph shape (alignment,
//! spacing, indentation) defined in Blueprint.
//!
//! # Design Decisions
//!
//! - **`Vec<Run>`** not `SmallVec<[Run; 5]>` -- YAGNI. SmallVec would
//!   bloat each Paragraph from ~40 bytes to ~220 bytes with no profiling
//!   evidence that allocation is a bottleneck. Migration to SmallVec is
//!   a non-breaking internal change if needed later.
//!
//! - **No `raw_xml` / `raw_binary`** -- raw preservation belongs in the
//!   Smithy layer, not the format-agnostic domain model.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_core::run::Run;
//! use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! let mut para = Paragraph::new(ParaShapeIndex::new(0));
//! para.add_run(Run::text("Hello ", CharShapeIndex::new(0)));
//! para.add_run(Run::text("world!", CharShapeIndex::new(1)));
//! assert_eq!(para.text_content(), "Hello world!");
//! assert_eq!(para.run_count(), 2);
//! ```

use hwpforge_foundation::{ParaShapeIndex, StyleIndex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::run::{Run, RunContent};

/// A paragraph: an ordered sequence of runs sharing a paragraph shape.
///
/// # Examples
///
/// ```
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_core::run::Run;
/// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
///
/// let para = Paragraph::with_runs(
///     vec![Run::text("Hello", CharShapeIndex::new(0))],
///     ParaShapeIndex::new(0),
/// );
/// assert_eq!(para.run_count(), 1);
/// assert!(!para.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Paragraph {
    /// Ordered sequence of runs.
    pub runs: Vec<Run>,
    /// Index into the paragraph shape collection (Blueprint resolves this).
    pub para_shape_id: ParaShapeIndex,
    /// Whether this paragraph starts a new column (HWPX `columnBreak="1"`).
    #[serde(default)]
    pub column_break: bool,
    /// Optional heading level (1-7) for TOC participation.
    /// Maps to 개요 1-7 styles. Paragraphs with a heading level
    /// will emit `<hp:titleMark>` in HWPX for auto-TOC support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_level: Option<u8>,
    /// Optional reference to a named style (e.g. 개요 1, 본문).
    /// `None` means 바탕글 (style 0, the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<StyleIndex>,
}

impl Paragraph {
    /// Creates an empty paragraph with the given shape reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// assert!(para.is_empty());
    /// ```
    pub fn new(para_shape_id: ParaShapeIndex) -> Self {
        Self {
            runs: Vec::new(),
            para_shape_id,
            column_break: false,
            heading_level: None,
            style_id: None,
        }
    }

    /// Creates a paragraph with pre-built runs.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![Run::text("text", CharShapeIndex::new(0))],
    ///     ParaShapeIndex::new(0),
    /// );
    /// assert_eq!(para.run_count(), 1);
    /// ```
    pub fn with_runs(runs: Vec<Run>, para_shape_id: ParaShapeIndex) -> Self {
        Self { runs, para_shape_id, column_break: false, heading_level: None, style_id: None }
    }

    /// Appends a run to this paragraph.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let mut para = Paragraph::new(ParaShapeIndex::new(0));
    /// para.add_run(Run::text("hello", CharShapeIndex::new(0)));
    /// assert_eq!(para.run_count(), 1);
    /// ```
    pub fn add_run(&mut self, run: Run) {
        self.runs.push(run);
    }

    /// Sets the heading level for TOC participation (1-7).
    ///
    /// Paragraphs with a heading level emit `<hp:titleMark>` in HWPX,
    /// enabling 한글 to auto-build a Table of Contents from document headings.
    ///
    /// # Panics
    ///
    /// Panics if `level` is 0 or greater than 7.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0))
    ///     .with_heading_level(1);
    /// assert_eq!(para.heading_level, Some(1));
    /// ```
    pub fn with_heading_level(mut self, level: u8) -> Self {
        assert!((1..=7).contains(&level), "heading_level must be 1-7, got {level}");
        self.heading_level = Some(level);
        self
    }

    /// Sets the style ID for this paragraph.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{ParaShapeIndex, StyleIndex};
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0))
    ///     .with_style(StyleIndex::new(2));
    /// assert_eq!(para.style_id, Some(StyleIndex::new(2)));
    /// ```
    pub fn with_style(mut self, style_id: StyleIndex) -> Self {
        self.style_id = Some(style_id);
        self
    }

    /// Sets the heading level for TOC participation (1-7), returning an error
    /// if the level is out of range.
    ///
    /// This is the fallible alternative to [`with_heading_level`](Self::with_heading_level),
    /// suitable for user-supplied input where panicking is undesirable.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if `level` is 0 or greater than 7.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0))
    ///     .try_with_heading_level(3)
    ///     .unwrap();
    /// assert_eq!(para.heading_level, Some(3));
    ///
    /// let err = Paragraph::new(ParaShapeIndex::new(0))
    ///     .try_with_heading_level(0);
    /// assert!(err.is_err());
    /// ```
    pub fn try_with_heading_level(mut self, level: u8) -> CoreResult<Self> {
        if !(1..=7).contains(&level) {
            return Err(CoreError::InvalidStructure {
                context: "Paragraph::try_with_heading_level".into(),
                reason: format!("heading_level must be 1-7, got {level}"),
            });
        }
        self.heading_level = Some(level);
        Ok(self)
    }

    /// Concatenates all text runs into a single string.
    ///
    /// Non-text runs (Table, Image, Control) are silently skipped.
    /// This is useful for full-text search and preview generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::table::Table;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![
    ///         Run::text("Hello ", CharShapeIndex::new(0)),
    ///         Run::table(Table::new(vec![]), CharShapeIndex::new(0)),
    ///         Run::text("world", CharShapeIndex::new(0)),
    ///     ],
    ///     ParaShapeIndex::new(0),
    /// );
    /// assert_eq!(para.text_content(), "Hello world");
    /// ```
    pub fn text_content(&self) -> String {
        self.runs
            .iter()
            .filter_map(
                |r| {
                    if let RunContent::Text(s) = &r.content {
                        Some(s.as_str())
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Returns the number of runs.
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Returns `true` if this paragraph has no runs.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

impl std::fmt::Display for Paragraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Paragraph({} runs)", self.runs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Control;
    use crate::table::Table;
    use hwpforge_foundation::CharShapeIndex;

    fn text_run(s: &str) -> Run {
        Run::text(s, CharShapeIndex::new(0))
    }

    #[test]
    fn new_is_empty() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        assert!(para.is_empty());
        assert_eq!(para.run_count(), 0);
        assert_eq!(para.text_content(), "");
    }

    #[test]
    fn with_runs() {
        let para = Paragraph::with_runs(vec![text_run("a"), text_run("b")], ParaShapeIndex::new(0));
        assert_eq!(para.run_count(), 2);
        assert!(!para.is_empty());
    }

    #[test]
    fn add_run() {
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(text_run("first"));
        para.add_run(text_run("second"));
        assert_eq!(para.run_count(), 2);
    }

    #[test]
    fn text_content_concatenation() {
        let para = Paragraph::with_runs(
            vec![text_run("Hello "), text_run("world!")],
            ParaShapeIndex::new(0),
        );
        assert_eq!(para.text_content(), "Hello world!");
    }

    #[test]
    fn text_content_skips_non_text() {
        let para = Paragraph::with_runs(
            vec![
                text_run("before"),
                Run::table(Table::new(vec![]), CharShapeIndex::new(0)),
                text_run("after"),
            ],
            ParaShapeIndex::new(0),
        );
        assert_eq!(para.text_content(), "beforeafter");
    }

    #[test]
    fn text_content_empty_paragraph() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        assert_eq!(para.text_content(), "");
    }

    #[test]
    fn text_content_no_text_runs() {
        let para = Paragraph::with_runs(
            vec![Run::table(Table::new(vec![]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        assert_eq!(para.text_content(), "");
    }

    #[test]
    fn korean_text_content() {
        let para = Paragraph::with_runs(
            vec![text_run("안녕"), text_run("하세요")],
            ParaShapeIndex::new(0),
        );
        assert_eq!(para.text_content(), "안녕하세요");
    }

    #[test]
    fn display() {
        let para = Paragraph::with_runs(
            vec![text_run("a"), text_run("b"), text_run("c")],
            ParaShapeIndex::new(0),
        );
        assert_eq!(para.to_string(), "Paragraph(3 runs)");
    }

    #[test]
    fn equality() {
        let a = Paragraph::with_runs(vec![text_run("x")], ParaShapeIndex::new(0));
        let b = Paragraph::with_runs(vec![text_run("x")], ParaShapeIndex::new(0));
        let c = Paragraph::with_runs(vec![text_run("y")], ParaShapeIndex::new(0));
        let d = Paragraph::with_runs(vec![text_run("x")], ParaShapeIndex::new(1));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn clone_independence() {
        let para = Paragraph::with_runs(vec![text_run("original")], ParaShapeIndex::new(0));
        let mut cloned = para.clone();
        cloned.add_run(text_run("added"));
        assert_eq!(para.run_count(), 1);
        assert_eq!(cloned.run_count(), 2);
    }

    #[test]
    fn many_runs() {
        let runs: Vec<Run> = (0..100).map(|i| text_run(&format!("run{i}"))).collect();
        let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        assert_eq!(para.run_count(), 100);
        assert!(para.text_content().starts_with("run0"));
    }

    #[test]
    fn serde_roundtrip() {
        let para = Paragraph::with_runs(
            vec![text_run("hello"), text_run("world")],
            ParaShapeIndex::new(5),
        );
        let json = serde_json::to_string(&para).unwrap();
        let back: Paragraph = serde_json::from_str(&json).unwrap();
        assert_eq!(para, back);
    }

    #[test]
    fn serde_roundtrip_with_control() {
        let ctrl =
            Control::Hyperlink { text: "link".to_string(), url: "https://example.com".to_string() };
        let para = Paragraph::with_runs(
            vec![text_run("see "), Run::control(ctrl, CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        );
        let json = serde_json::to_string(&para).unwrap();
        let back: Paragraph = serde_json::from_str(&json).unwrap();
        assert_eq!(para, back);
    }

    #[test]
    fn serde_empty_paragraph() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let json = serde_json::to_string(&para).unwrap();
        let back: Paragraph = serde_json::from_str(&json).unwrap();
        assert_eq!(para, back);
    }

    #[test]
    fn with_heading_level_sets_field() {
        let para = Paragraph::new(ParaShapeIndex::new(0)).with_heading_level(1);
        assert_eq!(para.heading_level, Some(1));

        let para7 = Paragraph::new(ParaShapeIndex::new(0)).with_heading_level(7);
        assert_eq!(para7.heading_level, Some(7));
    }

    #[test]
    fn with_heading_level_all_valid_levels() {
        for level in 1u8..=7 {
            let para = Paragraph::new(ParaShapeIndex::new(0)).with_heading_level(level);
            assert_eq!(para.heading_level, Some(level));
        }
    }

    #[test]
    #[should_panic(expected = "heading_level must be 1-7")]
    fn with_heading_level_zero_panics() {
        let _ = Paragraph::new(ParaShapeIndex::new(0)).with_heading_level(0);
    }

    #[test]
    #[should_panic(expected = "heading_level must be 1-7")]
    fn with_heading_level_eight_panics() {
        let _ = Paragraph::new(ParaShapeIndex::new(0)).with_heading_level(8);
    }

    #[test]
    fn new_has_no_heading_level() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        assert_eq!(para.heading_level, None);
    }

    #[test]
    fn serde_roundtrip_with_heading_level() {
        let para = Paragraph::with_runs(vec![text_run("heading text")], ParaShapeIndex::new(0))
            .with_heading_level(2);
        let json = serde_json::to_string(&para).unwrap();
        let back: Paragraph = serde_json::from_str(&json).unwrap();
        assert_eq!(para, back);
        assert_eq!(back.heading_level, Some(2));
    }

    #[test]
    fn serde_heading_level_omitted_when_none() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let json = serde_json::to_string(&para).unwrap();
        assert!(!json.contains("heading_level"), "None should be skipped in serialization");
    }

    #[test]
    fn try_with_heading_level_valid() {
        for level in 1u8..=7 {
            let para =
                Paragraph::new(ParaShapeIndex::new(0)).try_with_heading_level(level).unwrap();
            assert_eq!(para.heading_level, Some(level));
        }
    }

    #[test]
    fn try_with_heading_level_zero_errors() {
        let result = Paragraph::new(ParaShapeIndex::new(0)).try_with_heading_level(0);
        assert!(result.is_err());
    }

    #[test]
    fn try_with_heading_level_eight_errors() {
        let result = Paragraph::new(ParaShapeIndex::new(0)).try_with_heading_level(8);
        assert!(result.is_err());
    }

    #[test]
    fn try_with_heading_level_255_errors() {
        let result = Paragraph::new(ParaShapeIndex::new(0)).try_with_heading_level(255);
        assert!(result.is_err());
    }

    #[test]
    fn serde_roundtrip_all_7_heading_levels() {
        for level in 1u8..=7 {
            let para = Paragraph::with_runs(vec![text_run("heading")], ParaShapeIndex::new(0))
                .with_heading_level(level);
            let json = serde_json::to_string(&para).unwrap();
            let back: Paragraph = serde_json::from_str(&json).unwrap();
            assert_eq!(back.heading_level, Some(level), "level {level} roundtrip failed");
        }
    }

    #[test]
    fn new_has_no_style_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        assert_eq!(para.style_id, None);
    }

    #[test]
    fn with_style_builder_works() {
        let para = Paragraph::new(ParaShapeIndex::new(0)).with_style(StyleIndex::new(2));
        assert_eq!(para.style_id, Some(StyleIndex::new(2)));
    }

    #[test]
    fn with_runs_has_no_style_id() {
        let para = Paragraph::with_runs(vec![text_run("x")], ParaShapeIndex::new(0));
        assert_eq!(para.style_id, None);
    }

    #[test]
    fn serde_roundtrip_with_style_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0)).with_style(StyleIndex::new(5));
        let json = serde_json::to_string(&para).unwrap();
        let back: Paragraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.style_id, Some(StyleIndex::new(5)));
    }

    #[test]
    fn serde_missing_style_id_deserializes_to_none() {
        // JSON without style_id field → backward compat → None
        let json = r#"{"runs":[],"para_shape_id":0,"column_break":false}"#;
        let para: Paragraph = serde_json::from_str(json).unwrap();
        assert_eq!(para.style_id, None);
    }

    #[test]
    fn serde_style_id_omitted_when_none() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let json = serde_json::to_string(&para).unwrap();
        assert!(!json.contains("style_id"), "None should be skipped in serialization");
    }
}
