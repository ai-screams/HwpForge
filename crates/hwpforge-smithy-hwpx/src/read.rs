//! Read-only document projections (E5): `outline`.
//!
//! `outline` is the navigation map an agent fetches once to learn "what is
//! where" — headings, tables, named fields, bookmarks — before targeted
//! reads or edits. It deliberately does **not** enumerate every paragraph;
//! exhaustive content export stays with `to-json`, and targeted text
//! extraction with `read`.
//!
//! Address contract (agent-editing architecture §2.2 — indexes are the last
//! resort): every entry leads with its **name anchor** (heading text, field
//! name, bookmark name, table ordinal) as the primary key; the `{section,
//! para}` locator is a secondary convenience that goes stale after
//! structural edits.
//!
//! Scope notes (deliberate, documented):
//! - Headings and bookmarks are collected from the **top-level body flow**
//!   only. A "heading" inside a table cell or text box is not part of the
//!   document outline.
//! - Headings whose text is empty are skipped — they cannot serve as
//!   navigation anchors.

use std::collections::BTreeSet;

use hwpforge_core::section::Section;
use hwpforge_core::table::grid::TableGrid;
use hwpforge_core::{
    classify_paragraph, Caption, Control, HeadingSource, ParaKind, RunContent, StyleLookup,
};

use crate::error::HwpxResult;
use crate::fill::FieldInfo;
use crate::table_inventory::{tables_in_document, PathSeg};
use crate::{HwpxDecoder, HwpxFiller};

/// `{section, para}` positional locator (0-based, top-level body flow).
///
/// Secondary address: it goes stale after structural edits. Prefer the name
/// anchor carried by the surrounding entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParaLocator {
    /// Section index.
    pub section: usize,
    /// Top-level paragraph index within the section.
    pub para: usize,
}

/// Axis that produced a heading classification (wire form of
/// [`HeadingSource`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OutlineHeadingSource {
    /// Paragraph-shape outline metadata (1st-priority truth source).
    Outline,
    /// Style registry heading level (style-name based).
    Style,
}

/// One heading in the document body flow.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutlineHeading {
    /// Heading text (primary anchor). Single-line: embedded line breaks are
    /// collapsed to spaces.
    pub text: String,
    /// Heading depth, `1..=6`.
    pub level: u8,
    /// Axis that produced the classification.
    pub source: OutlineHeadingSource,
    /// Secondary positional locator.
    pub at: ParaLocator,
}

/// One table in the shared traversal order.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutlineTable {
    /// Document-order ordinal (primary anchor) — the same "table N" used by
    /// `set-cell`, `stamp` cell specs, and `to-json` address annotation.
    pub ordinal: usize,
    /// Secondary positional locator (the top-level paragraph hosting the
    /// table; nested tables report their host body paragraph).
    pub at: ParaLocator,
    /// Logical grid row count; `None` when the strict grid cannot be
    /// derived.
    pub rows: Option<u32>,
    /// Logical grid column count; `None` when the strict grid cannot be
    /// derived.
    pub cols: Option<u32>,
    /// Whether grid addressing (`addr {row,col}` / `set-cell`) works for
    /// this table.
    pub addressable: bool,
    /// Caption text, if the table has a non-empty caption.
    pub caption: Option<String>,
}

/// One named bookmark (first occurrence per name).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutlineBookmark {
    /// Bookmark name (primary anchor).
    pub name: String,
    /// Secondary positional locator.
    pub at: ParaLocator,
}

/// Per-section content summary.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SectionOutline {
    /// Section index.
    pub section: usize,
    /// Top-level paragraph count.
    pub paragraphs: usize,
    /// Table count in shared-traversal order, **including nested tables** —
    /// always equal to the number of `tables` entries for this section, so
    /// the summary and the ordinal list cannot disagree.
    pub tables: usize,
    /// Top-level image count (body flow).
    pub images: usize,
    /// Top-level chart count (body flow).
    pub charts: usize,
}

/// Document navigation map — the `outline` surface.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DocumentOutline {
    /// Document title from metadata, if present.
    pub title: Option<String>,
    /// Per-section content summaries.
    pub sections: Vec<SectionOutline>,
    /// Headings in body-flow order.
    pub headings: Vec<OutlineHeading>,
    /// Tables in shared-traversal order (ordinal-keyed).
    pub tables: Vec<OutlineTable>,
    /// Named click-here fields (same data as the `fields` surface).
    pub fields: Vec<FieldInfo>,
    /// Bookmarks in body-flow order (first occurrence per name).
    pub bookmarks: Vec<OutlineBookmark>,
}

/// Read-only projection facade.
pub struct HwpxReader;

impl HwpxReader {
    /// Builds the document navigation map from HWPX bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is not a decodable HWPX package.
    pub fn outline(bytes: &[u8]) -> HwpxResult<DocumentOutline> {
        let decoded = HwpxDecoder::decode(bytes)?;
        let document = &decoded.document;
        let styles = &decoded.style_store;

        let mut sections = Vec::new();
        let mut headings = Vec::new();
        let mut bookmarks = Vec::new();
        let mut seen_bookmarks = BTreeSet::new();

        for (section_idx, section) in document.sections().iter().enumerate() {
            let counts = section.content_counts();
            sections.push(SectionOutline {
                section: section_idx,
                paragraphs: section.paragraph_count(),
                tables: counts.tables,
                images: counts.images,
                charts: counts.charts,
            });
            collect_headings(section, section_idx, styles, &mut headings);
            collect_bookmarks(section, section_idx, &mut seen_bookmarks, &mut bookmarks);
        }

        let tables: Vec<OutlineTable> = tables_in_document(document)
            .iter()
            .map(|entry| {
                // First array index in the serde-shaped path is the hosting
                // top-level paragraph (the traversal always starts at
                // `paragraphs[j]`).
                let para = entry
                    .path
                    .iter()
                    .find_map(|seg| match seg {
                        PathSeg::Index(i) => Some(*i),
                        PathSeg::Field(_) => None,
                    })
                    .unwrap_or(0);
                let dims = TableGrid::from_table(entry.table).ok().map(|g| g.dimensions());
                OutlineTable {
                    ordinal: entry.ordinal,
                    at: ParaLocator { section: entry.section, para },
                    rows: dims.map(|(r, _)| r),
                    cols: dims.map(|(_, c)| c),
                    addressable: dims.is_some(),
                    caption: entry.table.caption.as_ref().and_then(caption_text),
                }
            })
            .collect();

        // The summary must agree with the ordinal list: ContentCounts only
        // sees top-level runs, so nested tables would make "N table(s)"
        // disagree with the entries below. Recount from the inventory.
        for summary in &mut sections {
            summary.tables = tables.iter().filter(|t| t.at.section == summary.section).count();
        }

        // Reuses the shipped discovery surface so `outline` and `fields`
        // cannot disagree (single truth for the field walk); costs one extra
        // decode of `bytes`.
        let fields = HwpxFiller::list_fields(bytes)?;

        Ok(DocumentOutline {
            title: document.metadata().title.clone(),
            sections,
            headings,
            tables,
            fields,
            bookmarks,
        })
    }

    /// Reads the text projection of a paragraph range in a section.
    ///
    /// `range` is an inclusive `(from, to)` pair of top-level paragraph
    /// indexes; `None` reads the whole section. Paragraphs whose content is
    /// not plain text are never silently dropped: embedded tables, images,
    /// and controls surface as [`EmbeddedContent`] markers.
    ///
    /// # Errors
    ///
    /// Fails on undecodable input, an out-of-range section, or an invalid
    /// paragraph range.
    pub fn read_paragraphs(
        bytes: &[u8],
        section: usize,
        range: Option<(usize, usize)>,
    ) -> Result<ParagraphsView, ReadError> {
        let decoded = HwpxDecoder::decode(bytes)?;
        let document = &decoded.document;
        let styles = &decoded.style_store;

        let sections = document.sections();
        let Some(sec) = sections.get(section) else {
            return Err(ReadError::SectionOutOfRange {
                requested: section,
                available: sections.len(),
            });
        };

        let available = sec.paragraphs.len();
        let (from, to) = range.unwrap_or((0, available.saturating_sub(1)));
        if available == 0 || from > to || to >= available {
            return Err(ReadError::ParaRangeInvalid { section, from, to, available });
        }

        let tables = tables_in_document(document);
        let paragraphs = sec.paragraphs[from..=to]
            .iter()
            .enumerate()
            .map(|(offset, paragraph)| ParagraphView {
                at: ParaLocator { section, para: from + offset },
                kind: kind_view(classify_paragraph(paragraph, styles)),
                text: paragraph.text_content(),
                contains: embedded_content(paragraph, &tables),
            })
            .collect();

        Ok(ParagraphsView { section, from, to, paragraphs })
    }

    /// Reads the logical-grid text matrix of the table at `ordinal`.
    ///
    /// Cells are the grid **anchors** (merged regions appear once, with
    /// their spans); covered coordinates resolve to their anchor by
    /// construction. Cell text joins the cell's paragraphs with `\n`; non-
    /// text cell content surfaces as [`EmbeddedContent`] markers.
    ///
    /// # Errors
    ///
    /// Fails on undecodable input, an unknown ordinal, or a table whose
    /// strict grid cannot be derived.
    pub fn read_table(bytes: &[u8], ordinal: usize) -> Result<TableView, ReadError> {
        let decoded = HwpxDecoder::decode(bytes)?;
        let document = &decoded.document;

        let entries = tables_in_document(document);
        let Some(entry) = entries.iter().find(|e| e.ordinal == ordinal) else {
            return Err(ReadError::TableOutOfRange {
                requested: ordinal,
                available: entries.len(),
            });
        };

        let grid = TableGrid::from_table(entry.table)
            .map_err(|e| ReadError::TableUnaddressable { ordinal, reason: e.to_string() })?;
        let (rows, cols) = grid.dimensions();

        let cells = grid
            .iter_anchors()
            .map(|anchor| {
                let cell = &entry.table.rows[anchor.row_idx].cells[anchor.cell_idx];
                let text = cell
                    .paragraphs
                    .iter()
                    .map(hwpforge_core::Paragraph::text_content)
                    .collect::<Vec<_>>()
                    .join("\n");
                let contains =
                    cell.paragraphs.iter().flat_map(|p| embedded_content(p, &entries)).collect();
                CellView {
                    row: anchor.anchor.row,
                    col: anchor.anchor.col,
                    row_span: u32::from(anchor.row_span),
                    col_span: u32::from(anchor.col_span),
                    text,
                    contains,
                }
            })
            .collect();

        let para = entry
            .path
            .iter()
            .find_map(|seg| match seg {
                PathSeg::Index(i) => Some(*i),
                PathSeg::Field(_) => None,
            })
            .unwrap_or(0);

        Ok(TableView {
            ordinal,
            at: ParaLocator { section: entry.section, para },
            rows,
            cols,
            cells,
        })
    }

    /// Reads every field named `name` (document order).
    ///
    /// # Errors
    ///
    /// Fails on undecodable input or when no field carries that name; the
    /// error lists the available names.
    pub fn read_field(bytes: &[u8], name: &str) -> Result<Vec<FieldInfo>, ReadError> {
        let fields = HwpxFiller::list_fields(bytes)?;
        let matches: Vec<FieldInfo> =
            fields.iter().filter(|f| f.name.as_deref() == Some(name)).cloned().collect();
        if matches.is_empty() {
            let available = fields.iter().filter_map(|f| f.name.clone()).collect();
            return Err(ReadError::FieldNotFound { name: name.to_string(), available });
        }
        Ok(matches)
    }
}

/// Error surface of the `read` projections.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The input could not be decoded.
    #[error("codec: {0}")]
    Codec(#[from] crate::error::HwpxError),
    /// The requested section does not exist.
    #[error("section {requested} out of range (document has {available} section(s))")]
    SectionOutOfRange {
        /// Requested section index.
        requested: usize,
        /// Number of sections in the document.
        available: usize,
    },
    /// The requested paragraph range is empty or out of bounds.
    #[error(
        "paragraph range {from}..={to} invalid for section {section} ({available} paragraph(s))"
    )]
    ParaRangeInvalid {
        /// Section index.
        section: usize,
        /// Inclusive range start.
        from: usize,
        /// Inclusive range end.
        to: usize,
        /// Number of paragraphs in the section.
        available: usize,
    },
    /// The requested table ordinal does not exist.
    #[error("table {requested} out of range (document has {available} table(s))")]
    TableOutOfRange {
        /// Requested ordinal.
        requested: usize,
        /// Number of tables in the document.
        available: usize,
    },
    /// The table exists but its strict grid cannot be derived.
    #[error("table {ordinal} grid underivable: {reason}")]
    TableUnaddressable {
        /// Table ordinal.
        ordinal: usize,
        /// Grid derivation failure.
        reason: String,
    },
    /// No field carries the requested name.
    #[error("no field named {name:?} (available: {available:?})")]
    FieldNotFound {
        /// Requested field name.
        name: String,
        /// Names that do exist in the document.
        available: Vec<String>,
    },
}

/// Wire form of [`ParaKind`] for read projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParaKindView {
    /// Heading with depth `1..=6`.
    Heading {
        /// Heading depth.
        level: u8,
    },
    /// List item.
    List {
        /// `true` = numbered family, `false` = bullet family.
        numbered: bool,
        /// Zero-based nesting depth.
        level: u8,
        /// Checkbox state for checkable bullets.
        checked: Option<bool>,
    },
    /// Plain body paragraph.
    Body,
}

/// Non-text content embedded in a paragraph or cell (never silently
/// dropped).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmbeddedContent {
    /// Inline table; `ordinal` addresses it for `read --table`/`set-cell`.
    Table {
        /// Shared traversal ordinal (`None` only if inventory lookup fails,
        /// which would be an internal inconsistency).
        ordinal: Option<usize>,
    },
    /// Inline image.
    Image,
    /// Control element, labelled by [`Control::kind_name`].
    Control {
        /// Control kind (snake_case).
        control: String,
    },
    /// Run content this build does not recognize (future variant).
    Other,
}

/// One paragraph in a read projection.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParagraphView {
    /// Positional locator.
    pub at: ParaLocator,
    /// Outline/list classification.
    #[serde(flatten)]
    pub kind: ParaKindView,
    /// Plain text content (text runs only; see `contains`).
    pub text: String,
    /// Embedded non-text content, in run order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contains: Vec<EmbeddedContent>,
}

/// Paragraph-range read result.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParagraphsView {
    /// Section index.
    pub section: usize,
    /// Inclusive range start actually read.
    pub from: usize,
    /// Inclusive range end actually read.
    pub to: usize,
    /// Paragraph projections in order.
    pub paragraphs: Vec<ParagraphView>,
}

/// One anchor cell in a table read (merged regions appear once).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellView {
    /// Logical grid row of the anchor.
    pub row: u32,
    /// Logical grid column of the anchor.
    pub col: u32,
    /// Rows covered by this cell.
    pub row_span: u32,
    /// Columns covered by this cell.
    pub col_span: u32,
    /// Cell text (paragraphs joined with `\n`).
    pub text: String,
    /// Embedded non-text content inside the cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contains: Vec<EmbeddedContent>,
}

/// Table read result (logical grid text matrix).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TableView {
    /// Shared traversal ordinal.
    pub ordinal: usize,
    /// Positional locator of the hosting paragraph.
    pub at: ParaLocator,
    /// Logical grid row count.
    pub rows: u32,
    /// Logical grid column count.
    pub cols: u32,
    /// Anchor cells in grid order.
    pub cells: Vec<CellView>,
}

fn kind_view(kind: ParaKind) -> ParaKindView {
    match kind {
        ParaKind::Heading { level, .. } => ParaKindView::Heading { level },
        ParaKind::ListItem { kind, level, checked, .. } => ParaKindView::List {
            numbered: matches!(kind, hwpforge_core::ListItemKind::Number),
            level,
            checked,
        },
        ParaKind::Body => ParaKindView::Body,
    }
}

fn embedded_content(
    paragraph: &hwpforge_core::Paragraph,
    tables: &[crate::table_inventory::TableEntry<'_>],
) -> Vec<EmbeddedContent> {
    let mut out = Vec::new();
    for run in &paragraph.runs {
        match &run.content {
            RunContent::Text(_) | RunContent::InlineText(_) => {}
            RunContent::Table(table) => {
                let ordinal = tables
                    .iter()
                    .find(|e| std::ptr::eq(e.table, table.as_ref()))
                    .map(|e| e.ordinal);
                out.push(EmbeddedContent::Table { ordinal });
            }
            RunContent::Image(_) => out.push(EmbeddedContent::Image),
            RunContent::Control(control) => {
                out.push(EmbeddedContent::Control { control: control.kind_name().to_string() })
            }
            // RunContent is #[non_exhaustive]; a future variant must surface
            // as an explicit marker, never vanish from the projection.
            _ => out.push(EmbeddedContent::Other),
        }
    }
    out
}

fn collect_headings(
    section: &Section,
    section_idx: usize,
    styles: &dyn StyleLookup,
    out: &mut Vec<OutlineHeading>,
) {
    for (para_idx, paragraph) in section.paragraphs.iter().enumerate() {
        if let ParaKind::Heading { level, source } = classify_paragraph(paragraph, styles) {
            let text = paragraph.text_content();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push(OutlineHeading {
                text: trimmed.replace('\n', " "),
                level,
                source: match source {
                    HeadingSource::ParaShape => OutlineHeadingSource::Outline,
                    HeadingSource::Style => OutlineHeadingSource::Style,
                },
                at: ParaLocator { section: section_idx, para: para_idx },
            });
        }
    }
}

fn collect_bookmarks(
    section: &Section,
    section_idx: usize,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<OutlineBookmark>,
) {
    for (para_idx, paragraph) in section.paragraphs.iter().enumerate() {
        for run in &paragraph.runs {
            if let RunContent::Control(control) = &run.content {
                if let Control::Bookmark { name, .. } = control.as_ref() {
                    if seen.insert(name.clone()) {
                        out.push(OutlineBookmark {
                            name: name.clone(),
                            at: ParaLocator { section: section_idx, para: para_idx },
                        });
                    }
                }
            }
        }
    }
}

fn caption_text(caption: &Caption) -> Option<String> {
    let joined = caption
        .paragraphs
        .iter()
        .map(hwpforge_core::Paragraph::text_content)
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::{Paragraph, Run};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, StyleIndex};
    use std::collections::HashMap;

    // -- unit: heading / bookmark collectors (synthetic sections) ---------

    #[derive(Default)]
    struct MockStyles {
        para_headings: HashMap<usize, u8>,
        style_headings: HashMap<usize, u8>,
    }

    impl StyleLookup for MockStyles {
        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.para_headings.get(&id.get()).copied()
        }
        fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
            self.style_headings.get(&id.get()).copied()
        }
    }

    fn text_para(shape: usize, text: &str) -> Paragraph {
        let mut p = Paragraph::new(ParaShapeIndex::new(shape));
        p.runs.push(Run::text(text, CharShapeIndex::new(0)));
        p
    }

    #[test]
    fn collect_headings_keeps_body_flow_order_and_locators() {
        let mut section = Section::new(PageSettings::default());
        section.paragraphs.push(text_para(0, "머리말 아님"));
        section.paragraphs.push(text_para(1, "1. 개요"));
        section.paragraphs.push(text_para(0, "본문"));
        section.paragraphs.push(text_para(2, "1.1 상세"));

        let styles = MockStyles { para_headings: [(1, 1), (2, 2)].into(), ..Default::default() };

        let mut out = Vec::new();
        collect_headings(&section, 3, &styles, &mut out);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "1. 개요");
        assert_eq!(out[0].level, 1);
        assert_eq!(out[0].source, OutlineHeadingSource::Outline);
        assert_eq!(out[0].at, ParaLocator { section: 3, para: 1 });
        assert_eq!(out[1].text, "1.1 상세");
        assert_eq!(out[1].at, ParaLocator { section: 3, para: 3 });
    }

    #[test]
    fn collect_headings_skips_empty_text_headings() {
        let mut section = Section::new(PageSettings::default());
        section.paragraphs.push(text_para(1, "   "));

        let styles = MockStyles { para_headings: [(1, 1)].into(), ..Default::default() };

        let mut out = Vec::new();
        collect_headings(&section, 0, &styles, &mut out);
        assert!(out.is_empty(), "empty heading text cannot anchor navigation");
    }

    #[test]
    fn collect_headings_collapses_line_breaks() {
        let mut section = Section::new(PageSettings::default());
        section.paragraphs.push(text_para(1, "제목\n둘째 줄"));

        let styles = MockStyles { para_headings: [(1, 1)].into(), ..Default::default() };

        let mut out = Vec::new();
        collect_headings(&section, 0, &styles, &mut out);
        assert_eq!(out[0].text, "제목 둘째 줄");
    }

    #[test]
    fn collect_headings_reports_style_source() {
        let mut section = Section::new(PageSettings::default());
        let mut p = text_para(0, "스타일 제목");
        p.style_id = Some(StyleIndex::new(4));
        section.paragraphs.push(p);

        let styles = MockStyles { style_headings: [(4, 3)].into(), ..Default::default() };

        let mut out = Vec::new();
        collect_headings(&section, 0, &styles, &mut out);
        assert_eq!(out[0].source, OutlineHeadingSource::Style);
        assert_eq!(out[0].level, 3);
    }

    #[test]
    fn collect_bookmarks_dedupes_by_name_first_occurrence_wins() {
        let mut section = Section::new(PageSettings::default());
        let mut p0 = Paragraph::new(ParaShapeIndex::new(0));
        p0.runs.push(Run::control(Control::bookmark("장시작"), CharShapeIndex::new(0)));
        let mut p1 = Paragraph::new(ParaShapeIndex::new(0));
        p1.runs.push(Run::control(Control::bookmark("장시작"), CharShapeIndex::new(0)));
        p1.runs.push(Run::control(Control::bookmark("부록"), CharShapeIndex::new(0)));
        section.paragraphs.push(p0);
        section.paragraphs.push(p1);

        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        collect_bookmarks(&section, 0, &mut seen, &mut out);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "장시작");
        assert_eq!(out[0].at, ParaLocator { section: 0, para: 0 });
        assert_eq!(out[1].name, "부록");
        assert_eq!(out[1].at, ParaLocator { section: 0, para: 1 });
    }

    // -- fixture: end-to-end outline over real HWPX ------------------------

    fn fixture(rel: &str) -> Vec<u8> {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel);
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
    }

    #[test]
    fn outline_basic_table_fixture_reports_grid_dims() {
        let outline = HwpxReader::outline(&fixture("tables/table_01_basic_2x2.hwpx")).unwrap();

        assert_eq!(outline.sections.len(), 1);
        assert_eq!(outline.tables.len(), 1);
        let table = &outline.tables[0];
        assert_eq!(table.ordinal, 0);
        assert!(table.addressable);
        assert_eq!((table.rows, table.cols), (Some(2), Some(2)));
        assert_eq!(table.at.section, 0);
    }

    #[test]
    fn outline_merged_grid_form_is_addressable_with_sequential_ordinals() {
        let outline = HwpxReader::outline(&fixture("tables/merged_grid_form.hwpx")).unwrap();

        assert!(!outline.tables.is_empty());
        for (i, table) in outline.tables.iter().enumerate() {
            assert_eq!(table.ordinal, i, "ordinals must be dense and ordered");
            assert!(table.addressable, "merged grid form tables are addressable");
        }
    }

    #[test]
    fn outline_named_field_fixture_exposes_fields_axis() {
        let outline = HwpxReader::outline(&fixture("fields/clickhere_named.hwpx")).unwrap();

        assert!(!outline.fields.is_empty());
        assert!(
            outline.fields.iter().any(|f| f.name.is_some()),
            "named click-here fixture must expose at least one named field"
        );
    }

    #[test]
    fn outline_rejects_non_hwpx_bytes() {
        assert!(HwpxReader::outline(b"not a zip").is_err());
    }

    // -- fixture: read projections (W2) ---------------------------------

    #[test]
    fn read_paragraphs_reports_table_marker_not_silent_drop() {
        let bytes = fixture("tables/table_01_basic_2x2.hwpx");
        let view = HwpxReader::read_paragraphs(&bytes, 0, None).unwrap();
        assert!(
            view.paragraphs.iter().any(|p| p
                .contains
                .iter()
                .any(|c| matches!(c, EmbeddedContent::Table { ordinal: Some(0) }))),
            "the table-hosting paragraph must carry an explicit marker"
        );
    }

    #[test]
    fn read_paragraphs_range_is_inclusive_and_validated() {
        let bytes = fixture("tables/table_01_basic_2x2.hwpx");
        let full = HwpxReader::read_paragraphs(&bytes, 0, None).unwrap();
        let n = full.paragraphs.len();
        assert!(n >= 1);

        let one = HwpxReader::read_paragraphs(&bytes, 0, Some((0, 0))).unwrap();
        assert_eq!(one.paragraphs.len(), 1);
        assert_eq!(one.paragraphs[0].at, ParaLocator { section: 0, para: 0 });

        assert!(matches!(
            HwpxReader::read_paragraphs(&bytes, 0, Some((1, 0))),
            Err(ReadError::ParaRangeInvalid { .. })
        ));
        assert!(matches!(
            HwpxReader::read_paragraphs(&bytes, 0, Some((0, n))),
            Err(ReadError::ParaRangeInvalid { .. })
        ));
        assert!(matches!(
            HwpxReader::read_paragraphs(&bytes, 9, None),
            Err(ReadError::SectionOutOfRange { .. })
        ));
    }

    #[test]
    fn read_table_anchors_tile_grid_exactly_once() {
        let bytes = fixture("tables/merged_grid_form.hwpx");
        let view = HwpxReader::read_table(&bytes, 0).unwrap();
        assert!(view.rows >= 1 && view.cols >= 1);
        let coverage: u32 = view.cells.iter().map(|c| c.row_span * c.col_span).sum();
        assert_eq!(coverage, view.rows * view.cols, "anchors must tile the grid exactly once");
    }

    #[test]
    fn read_table_unknown_ordinal_reports_available() {
        let bytes = fixture("tables/table_01_basic_2x2.hwpx");
        assert!(matches!(
            HwpxReader::read_table(&bytes, 99),
            Err(ReadError::TableOutOfRange { requested: 99, available: 1 })
        ));
    }

    #[test]
    fn read_field_finds_named_and_reports_missing_with_available() {
        let bytes = fixture("fields/clickhere_named.hwpx");
        let hits = HwpxReader::read_field(&bytes, "user_email").unwrap();
        assert!(!hits.is_empty());

        let err = HwpxReader::read_field(&bytes, "없는이름").unwrap_err();
        match err {
            ReadError::FieldNotFound { available, .. } => {
                assert!(available.contains(&"user_email".to_string()));
            }
            other => panic!("expected FieldNotFound, got {other:?}"),
        }
    }
}
