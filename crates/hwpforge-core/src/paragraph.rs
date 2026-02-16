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

use hwpforge_foundation::ParaShapeIndex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
        Self { runs: Vec::new(), para_shape_id }
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
        Self { runs, para_shape_id }
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
}
