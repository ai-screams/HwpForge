//! Run and RunContent: the leaf nodes of the document tree.
//!
//! A [`Run`] is a contiguous segment of content with a single character
//! shape (font, size, etc.). The actual content is held in [`RunContent`],
//! which may be text, a table, an image, or a control element.
//!
//! # Enum Size Optimization
//!
//! [`Table`] and [`Control`] are
//! large types. They are boxed inside [`RunContent`] to keep the common
//! case (`RunContent::Text`) small:
//!
//! - `Text(String)` -- 24 bytes
//! - `Table(Box<Table>)` -- 8 bytes (pointer)
//! - `Image(Image)` -- moderate
//! - `Control(Box<Control>)` -- 8 bytes (pointer)
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::run::{Run, RunContent};
//! use hwpforge_foundation::CharShapeIndex;
//!
//! let run = Run::text("Hello, world!", CharShapeIndex::new(0));
//! assert_eq!(run.content.as_text(), Some("Hello, world!"));
//! assert!(run.content.is_text());
//! ```

use hwpforge_foundation::CharShapeIndex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::control::Control;
use crate::image::Image;
use crate::table::Table;

/// A run: a segment of content with a single character shape reference.
///
/// Runs are the leaf nodes of the document tree. A paragraph contains
/// one or more runs. Adjacent runs with the same `char_shape_id` could
/// theoretically be merged, but Core preserves the original structure.
///
/// # Examples
///
/// ```
/// use hwpforge_core::run::Run;
/// use hwpforge_foundation::CharShapeIndex;
///
/// let run = Run::text("paragraph text", CharShapeIndex::new(0));
/// assert!(run.content.is_text());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Run {
    /// The content of this run.
    pub content: RunContent,
    /// Index into the character shape collection (Blueprint resolves this).
    pub char_shape_id: CharShapeIndex,
}

impl Run {
    /// Creates a text run.
    ///
    /// This is the most common constructor. Most runs in a typical
    /// document are text.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::run::Run;
    /// use hwpforge_foundation::CharShapeIndex;
    ///
    /// let run = Run::text("Hello", CharShapeIndex::new(0));
    /// assert_eq!(run.content.as_text(), Some("Hello"));
    /// ```
    pub fn text(s: impl Into<String>, char_shape_id: CharShapeIndex) -> Self {
        Self { content: RunContent::Text(s.into()), char_shape_id }
    }

    /// Creates a table run. The table is automatically boxed.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::table::Table;
    /// use hwpforge_foundation::CharShapeIndex;
    ///
    /// let table = Table::new(vec![]);
    /// let run = Run::table(table, CharShapeIndex::new(0));
    /// assert!(run.content.is_table());
    /// ```
    pub fn table(table: Table, char_shape_id: CharShapeIndex) -> Self {
        Self { content: RunContent::Table(Box::new(table)), char_shape_id }
    }

    /// Creates an image run.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::image::{Image, ImageFormat};
    /// use hwpforge_foundation::{HwpUnit, CharShapeIndex};
    ///
    /// let img = Image::new("test.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png);
    /// let run = Run::image(img, CharShapeIndex::new(0));
    /// assert!(run.content.is_image());
    /// ```
    pub fn image(image: Image, char_shape_id: CharShapeIndex) -> Self {
        Self { content: RunContent::Image(image), char_shape_id }
    }

    /// Creates a control run. The control is automatically boxed.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::CharShapeIndex;
    ///
    /// let link = Control::Hyperlink {
    ///     text: "Click".to_string(),
    ///     url: "https://example.com".to_string(),
    /// };
    /// let run = Run::control(link, CharShapeIndex::new(0));
    /// assert!(run.content.is_control());
    /// ```
    pub fn control(control: Control, char_shape_id: CharShapeIndex) -> Self {
        Self { content: RunContent::Control(Box::new(control)), char_shape_id }
    }
}

impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Run({})", self.content)
    }
}

/// The content of a run.
///
/// Marked `#[non_exhaustive]` so future content types can be added
/// without a breaking change.
///
/// # Design Decision
///
/// `Table` and `Control` are boxed to keep the enum size small.
/// The common case (`Text`) is 24 bytes (a `String`). Without boxing,
/// the enum would be ~88 bytes (dominated by the `Control` variant).
///
/// # Examples
///
/// ```
/// use hwpforge_core::run::RunContent;
///
/// let text = RunContent::Text("Hello".to_string());
/// assert!(text.is_text());
/// assert_eq!(text.as_text(), Some("Hello"));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum RunContent {
    /// Plain text.
    Text(String),
    /// An inline table (boxed for enum size optimization).
    Table(Box<Table>),
    /// An inline image.
    Image(Image),
    /// A control element (boxed for enum size optimization).
    Control(Box<Control>),
}

impl RunContent {
    /// Returns the text content if this is a `Text` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::run::RunContent;
    ///
    /// let content = RunContent::Text("hello".to_string());
    /// assert_eq!(content.as_text(), Some("hello"));
    ///
    /// let content = RunContent::Text(String::new());
    /// assert_eq!(content.as_text(), Some(""));
    /// ```
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the table if this is a `Table` variant.
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Self::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Returns the image if this is an `Image` variant.
    pub fn as_image(&self) -> Option<&Image> {
        match self {
            Self::Image(i) => Some(i),
            _ => None,
        }
    }

    /// Returns the control if this is a `Control` variant.
    pub fn as_control(&self) -> Option<&Control> {
        match self {
            Self::Control(c) => Some(c),
            _ => None,
        }
    }

    /// Returns `true` if this is a `Text` variant.
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Returns `true` if this is a `Table` variant.
    pub fn is_table(&self) -> bool {
        matches!(self, Self::Table(_))
    }

    /// Returns `true` if this is an `Image` variant.
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image(_))
    }

    /// Returns `true` if this is a `Control` variant.
    pub fn is_control(&self) -> bool {
        matches!(self, Self::Control(_))
    }
}

impl std::fmt::Display for RunContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(s) => {
                if s.len() <= 50 {
                    write!(f, "Text(\"{s}\")")
                } else {
                    let truncated: String = s.chars().take(50).collect();
                    write!(f, "Text(\"{truncated}...\")")
                }
            }
            Self::Table(t) => write!(f, "{t}"),
            Self::Image(i) => write!(f, "{i}"),
            Self::Control(c) => write!(f, "{c}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageFormat;
    use hwpforge_foundation::HwpUnit;

    #[test]
    fn run_text_constructor() {
        let run = Run::text("Hello", CharShapeIndex::new(0));
        assert_eq!(run.content.as_text(), Some("Hello"));
        assert_eq!(run.char_shape_id, CharShapeIndex::new(0));
    }

    #[test]
    fn run_text_from_string() {
        let s = String::from("owned");
        let run = Run::text(s, CharShapeIndex::new(1));
        assert_eq!(run.content.as_text(), Some("owned"));
    }

    #[test]
    fn run_table_constructor() {
        let table = Table::new(vec![]);
        let run = Run::table(table, CharShapeIndex::new(0));
        assert!(run.content.is_table());
        assert!(run.content.as_table().unwrap().is_empty());
    }

    #[test]
    fn run_image_constructor() {
        let img = Image::new("test.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png);
        let run = Run::image(img, CharShapeIndex::new(0));
        assert!(run.content.is_image());
        assert_eq!(run.content.as_image().unwrap().path, "test.png");
    }

    #[test]
    fn run_control_constructor() {
        let ctrl = Control::Hyperlink {
            text: "link".to_string(),
            url: "https://example.com".to_string(),
        };
        let run = Run::control(ctrl, CharShapeIndex::new(0));
        assert!(run.content.is_control());
        assert!(run.content.as_control().unwrap().is_hyperlink());
    }

    // === RunContent type checks ===

    #[test]
    fn run_content_text_checks() {
        let c = RunContent::Text("hi".to_string());
        assert!(c.is_text());
        assert!(!c.is_table());
        assert!(!c.is_image());
        assert!(!c.is_control());
    }

    #[test]
    fn run_content_table_checks() {
        let c = RunContent::Table(Box::new(Table::new(vec![])));
        assert!(!c.is_text());
        assert!(c.is_table());
    }

    #[test]
    fn run_content_image_checks() {
        let c = RunContent::Image(Image::new("x.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png));
        assert!(!c.is_text());
        assert!(c.is_image());
    }

    #[test]
    fn run_content_control_checks() {
        let c = RunContent::Control(Box::new(Control::Unknown {
            tag: "x".to_string(),
            data: None,
        }));
        assert!(!c.is_text());
        assert!(c.is_control());
    }

    // === Accessors return None for wrong variant ===

    #[test]
    fn as_text_returns_none_for_non_text() {
        let c = RunContent::Table(Box::new(Table::new(vec![])));
        assert!(c.as_text().is_none());
    }

    #[test]
    fn as_table_returns_none_for_non_table() {
        let c = RunContent::Text("hi".to_string());
        assert!(c.as_table().is_none());
    }

    #[test]
    fn as_image_returns_none_for_non_image() {
        let c = RunContent::Text("hi".to_string());
        assert!(c.as_image().is_none());
    }

    #[test]
    fn as_control_returns_none_for_non_control() {
        let c = RunContent::Text("hi".to_string());
        assert!(c.as_control().is_none());
    }

    // === Display ===

    #[test]
    fn run_content_display_text_short() {
        let c = RunContent::Text("hello".to_string());
        assert_eq!(c.to_string(), "Text(\"hello\")");
    }

    #[test]
    fn run_content_display_text_long_truncated() {
        let long = "A".repeat(100);
        let c = RunContent::Text(long);
        let s = c.to_string();
        assert!(s.contains(&"A".repeat(50)), "display: {s}");
        assert!(s.ends_with("...\")"), "display: {s}");
    }

    #[test]
    fn run_display() {
        let run = Run::text("test", CharShapeIndex::new(0));
        let s = run.to_string();
        assert!(s.contains("Run("), "display: {s}");
        assert!(s.contains("Text"), "display: {s}");
    }

    // === Empty text ===

    #[test]
    fn empty_text_run() {
        let run = Run::text("", CharShapeIndex::new(0));
        assert_eq!(run.content.as_text(), Some(""));
    }

    // === Korean text ===

    #[test]
    fn korean_text_run() {
        let run = Run::text("안녕하세요", CharShapeIndex::new(0));
        assert_eq!(run.content.as_text(), Some("안녕하세요"));
    }

    // === Equality ===

    #[test]
    fn run_equality() {
        let a = Run::text("hello", CharShapeIndex::new(0));
        let b = Run::text("hello", CharShapeIndex::new(0));
        let c = Run::text("world", CharShapeIndex::new(0));
        let d = Run::text("hello", CharShapeIndex::new(1));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    // === Serde ===

    #[test]
    fn serde_roundtrip_text() {
        let run = Run::text("test", CharShapeIndex::new(5));
        let json = serde_json::to_string(&run).unwrap();
        let back: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(run, back);
    }

    #[test]
    fn serde_roundtrip_table() {
        let run = Run::table(Table::new(vec![]), CharShapeIndex::new(0));
        let json = serde_json::to_string(&run).unwrap();
        let back: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(run, back);
    }

    #[test]
    fn serde_roundtrip_image() {
        let img = Image::new("test.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png);
        let run = Run::image(img, CharShapeIndex::new(0));
        let json = serde_json::to_string(&run).unwrap();
        let back: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(run, back);
    }

    #[test]
    fn serde_roundtrip_control() {
        let ctrl = Control::Hyperlink {
            text: "link".to_string(),
            url: "https://example.com".to_string(),
        };
        let run = Run::control(ctrl, CharShapeIndex::new(0));
        let json = serde_json::to_string(&run).unwrap();
        let back: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(run, back);
    }

    // === Clone ===

    #[test]
    fn run_clone_independence() {
        let run = Run::text("original", CharShapeIndex::new(0));
        let cloned = run.clone();
        assert_eq!(run, cloned);
    }
}
