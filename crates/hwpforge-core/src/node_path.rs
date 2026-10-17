//! Shared semantic node addresses for inventories, preservation and editors.
//! Indices always refer to decoded Core, never wire run indices.

/// A section-qualified Core node address, including nested containers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct CorePath {
    section: usize,
    relative: String,
}
impl CorePath {
    /// Address a node using its section and section-relative semantic path.
    pub fn new(section: usize, relative: impl Into<String>) -> Self {
        Self { section, relative: relative.into() }
    }
    /// Section index.
    pub fn section(&self) -> usize {
        self.section
    }
    /// Section-relative path, including nested cell, caption and note indices.
    pub fn relative(&self) -> &str {
        &self.relative
    }
}

/// Extensible node categories independent of any wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeKind {
    /// A paragraph.
    Paragraph,
    /// A run.
    Run,
    /// Text or structured inline text.
    Text,
    /// A control.
    Control,
    /// A field boundary.
    FieldBoundary,
    /// A table.
    Table,
    /// A table row.
    Row,
    /// A table cell.
    Cell,
    /// A text box.
    TextBox,
    /// A header or footer.
    HeaderFooter,
    /// A footnote or endnote.
    Note,
    /// A caption.
    Caption,
    /// A memo anchor.
    MemoAnchor,
}
