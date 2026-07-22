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
}
