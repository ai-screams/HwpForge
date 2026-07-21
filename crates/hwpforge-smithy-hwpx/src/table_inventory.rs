//! Shared table inventory: the single traversal that defines table ordinals.
//!
//! Export address projection, discovery surfaces, and cell editing must all
//! agree on what "table N" means. This module owns that definition so the
//! three surfaces cannot drift: **depth-first pre-order** over the same
//! containers the fill/patch visitors cover (`fill::visit_section_fields`
//! mirror — body paragraphs, table captions and cells, image captions,
//! text-box/shape paragraphs and captions, footnotes and endnotes). A table's
//! ordinal is assigned when it is first encountered; its caption is visited
//! before its cells, so nested tables in a caption order before nested tables
//! in cells.
//!
//! Each entry carries a serde-shaped path (field/index segments) from the
//! enclosing [`Section`] value to the table's JSON node, letting JSON
//! consumers navigate an exported document in lockstep with the typed
//! structure instead of re-guessing the layout.

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_core::table::Table;
use hwpforge_core::Control;

/// One segment of a serde-shaped JSON path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSeg {
    /// Object field (also used for externally-tagged enum variant keys).
    Field(&'static str),
    /// Array index.
    Index(usize),
}

/// Renders a path as `a.b[0].c` for diagnostics.
pub(crate) fn render_path(prefix: &str, path: &[PathSeg]) -> String {
    let mut out = String::from(prefix);
    for seg in path {
        match seg {
            PathSeg::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            PathSeg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// A table discovered by the shared traversal.
#[derive(Debug)]
pub(crate) struct TableEntry<'a> {
    /// Zero-based ordinal in traversal order (continues across sections for
    /// document-level enumeration).
    pub ordinal: usize,
    /// Section index the table lives in.
    pub section: usize,
    /// Serde-shaped path from the enclosing `Section` value to the table
    /// node (ends at the `Table` variant object).
    pub path: Vec<PathSeg>,
    /// The table itself.
    pub table: &'a Table,
}

/// Enumerates every table in a document in the shared traversal order.
pub(crate) fn tables_in_document(document: &Document<Draft>) -> Vec<TableEntry<'_>> {
    let mut entries = Vec::new();
    for (section_idx, section) in document.sections().iter().enumerate() {
        collect_section(section, section_idx, &mut entries);
    }
    entries
}

/// Enumerates every table in a single section (section-local ordinals).
pub(crate) fn tables_in_section(section: &Section, section_idx: usize) -> Vec<TableEntry<'_>> {
    let mut entries = Vec::new();
    collect_section(section, section_idx, &mut entries);
    entries
}

fn collect_section<'a>(
    section: &'a Section,
    section_idx: usize,
    entries: &mut Vec<TableEntry<'a>>,
) {
    let mut path = vec![PathSeg::Field("paragraphs")];
    walk_paragraphs(&section.paragraphs, section_idx, &mut path, entries);
}

fn walk_paragraphs<'a>(
    paragraphs: &'a [Paragraph],
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    entries: &mut Vec<TableEntry<'a>>,
) {
    for (pi, paragraph) in paragraphs.iter().enumerate() {
        path.push(PathSeg::Index(pi));
        path.push(PathSeg::Field("runs"));
        for (ri, run) in paragraph.runs.iter().enumerate() {
            path.push(PathSeg::Index(ri));
            path.push(PathSeg::Field("content"));
            match &run.content {
                RunContent::Table(table) => {
                    path.push(PathSeg::Field("Table"));
                    entries.push(TableEntry {
                        ordinal: entries.len(),
                        section: section_idx,
                        path: path.clone(),
                        table,
                    });
                    if let Some(caption) = table.caption.as_ref() {
                        path.push(PathSeg::Field("caption"));
                        path.push(PathSeg::Field("paragraphs"));
                        walk_paragraphs(&caption.paragraphs, section_idx, path, entries);
                        path.pop();
                        path.pop();
                    }
                    path.push(PathSeg::Field("rows"));
                    for (row_i, row) in table.rows.iter().enumerate() {
                        path.push(PathSeg::Index(row_i));
                        path.push(PathSeg::Field("cells"));
                        for (cell_i, cell) in row.cells.iter().enumerate() {
                            path.push(PathSeg::Index(cell_i));
                            path.push(PathSeg::Field("paragraphs"));
                            walk_paragraphs(&cell.paragraphs, section_idx, path, entries);
                            path.pop();
                            path.pop();
                        }
                        path.pop();
                        path.pop();
                    }
                    path.pop();
                    path.pop();
                }
                RunContent::Image(image) => {
                    if let Some(caption) = image.caption.as_ref() {
                        path.push(PathSeg::Field("Image"));
                        path.push(PathSeg::Field("caption"));
                        path.push(PathSeg::Field("paragraphs"));
                        walk_paragraphs(&caption.paragraphs, section_idx, path, entries);
                        path.pop();
                        path.pop();
                        path.pop();
                    }
                }
                RunContent::Control(control) => {
                    path.push(PathSeg::Field("Control"));
                    walk_control(control, section_idx, path, entries);
                    path.pop();
                }
                _ => {}
            }
            path.pop();
            path.pop();
        }
        path.pop();
        path.pop();
    }
}

fn walk_control<'a>(
    control: &'a Control,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    entries: &mut Vec<TableEntry<'a>>,
) {
    match control {
        Control::TextBox { paragraphs, caption, .. } => {
            walk_container(paragraphs, caption.as_ref(), "TextBox", section_idx, path, entries);
        }
        Control::Ellipse { paragraphs, caption, .. } => {
            walk_container(paragraphs, caption.as_ref(), "Ellipse", section_idx, path, entries);
        }
        Control::Polygon { paragraphs, caption, .. } => {
            walk_container(paragraphs, caption.as_ref(), "Polygon", section_idx, path, entries);
        }
        Control::Footnote { paragraphs, .. } => {
            walk_container(paragraphs, None, "Footnote", section_idx, path, entries);
        }
        Control::Endnote { paragraphs, .. } => {
            walk_container(paragraphs, None, "Endnote", section_idx, path, entries);
        }
        Control::Rect { caption: Some(caption), .. } => {
            walk_caption_only(caption, "Rect", section_idx, path, entries);
        }
        Control::Line { caption: Some(caption), .. } => {
            walk_caption_only(caption, "Line", section_idx, path, entries);
        }
        Control::Arc { caption: Some(caption), .. } => {
            walk_caption_only(caption, "Arc", section_idx, path, entries);
        }
        Control::Curve { caption: Some(caption), .. } => {
            walk_caption_only(caption, "Curve", section_idx, path, entries);
        }
        Control::ConnectLine { caption: Some(caption), .. } => {
            walk_caption_only(caption, "ConnectLine", section_idx, path, entries);
        }
        _ => {}
    }
}

fn walk_container<'a>(
    paragraphs: &'a [Paragraph],
    caption: Option<&'a hwpforge_core::caption::Caption>,
    variant: &'static str,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    entries: &mut Vec<TableEntry<'a>>,
) {
    path.push(PathSeg::Field(variant));
    path.push(PathSeg::Field("paragraphs"));
    walk_paragraphs(paragraphs, section_idx, path, entries);
    path.pop();
    if let Some(caption) = caption {
        path.push(PathSeg::Field("caption"));
        path.push(PathSeg::Field("paragraphs"));
        walk_paragraphs(&caption.paragraphs, section_idx, path, entries);
        path.pop();
        path.pop();
    }
    path.pop();
}

fn walk_caption_only<'a>(
    caption: &'a hwpforge_core::caption::Caption,
    variant: &'static str,
    section_idx: usize,
    path: &mut Vec<PathSeg>,
    entries: &mut Vec<TableEntry<'a>>,
) {
    path.push(PathSeg::Field(variant));
    path.push(PathSeg::Field("caption"));
    path.push(PathSeg::Field("paragraphs"));
    walk_paragraphs(&caption.paragraphs, section_idx, path, entries);
    path.pop();
    path.pop();
    path.pop();
}

/// Mutable mirror of the shared traversal: calls `f(ordinal, &mut Table)` for
/// every table in exactly the enumeration order of [`tables_in_document`].
///
/// Kept in this module so the two walkers cannot drift apart unnoticed; the
/// `mutable_walker_mirrors_enumeration_order` test locks them together over
/// every container type.
pub(crate) fn for_each_table_mut(
    document: &mut Document<Draft>,
    f: &mut impl FnMut(usize, &mut Table),
) {
    let mut ordinal = 0usize;
    for section in document.sections_mut() {
        walk_paragraphs_mut(&mut section.paragraphs, &mut ordinal, f);
    }
}

fn walk_paragraphs_mut(
    paragraphs: &mut [Paragraph],
    ordinal: &mut usize,
    f: &mut impl FnMut(usize, &mut Table),
) {
    for paragraph in paragraphs {
        for run in &mut paragraph.runs {
            match &mut run.content {
                RunContent::Table(table) => {
                    let this = *ordinal;
                    *ordinal += 1;
                    f(this, table);
                    if let Some(caption) = table.caption.as_mut() {
                        walk_paragraphs_mut(&mut caption.paragraphs, ordinal, f);
                    }
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            walk_paragraphs_mut(&mut cell.paragraphs, ordinal, f);
                        }
                    }
                }
                RunContent::Image(image) => {
                    if let Some(caption) = image.caption.as_mut() {
                        walk_paragraphs_mut(&mut caption.paragraphs, ordinal, f);
                    }
                }
                RunContent::Control(control) => match control.as_mut() {
                    Control::TextBox { paragraphs, caption, .. }
                    | Control::Ellipse { paragraphs, caption, .. }
                    | Control::Polygon { paragraphs, caption, .. } => {
                        walk_paragraphs_mut(paragraphs, ordinal, f);
                        if let Some(caption) = caption.as_mut() {
                            walk_paragraphs_mut(&mut caption.paragraphs, ordinal, f);
                        }
                    }
                    Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                        walk_paragraphs_mut(paragraphs, ordinal, f);
                    }
                    Control::Rect { caption: Some(caption), .. }
                    | Control::Line { caption: Some(caption), .. }
                    | Control::Arc { caption: Some(caption), .. }
                    | Control::Curve { caption: Some(caption), .. }
                    | Control::ConnectLine { caption: Some(caption), .. } => {
                        walk_paragraphs_mut(&mut caption.paragraphs, ordinal, f);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, VerticalAlign};

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn one_cell_table(cell_paras: Vec<Paragraph>) -> Table {
        let cell = TableCell::new(cell_paras, HwpUnit::new(8000).unwrap());
        Table::new(vec![TableRow::new(vec![cell])])
    }

    #[test]
    fn empty_document_has_no_tables() {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![text_para("본문")], PageSettings::default()));
        assert!(tables_in_document(&doc).is_empty());
    }

    #[test]
    fn ordinals_are_preorder_and_paths_are_serde_shaped() {
        // Body table #0 whose caption holds table #1 and whose cell holds
        // table #2, followed by a text-box holding table #3 — caption orders
        // before cells, nesting is depth-first.
        let mut caption_host = one_cell_table(vec![text_para("셀")]);
        caption_host.caption = Some(Caption::new(
            vec![{
                let mut p = Paragraph::new(ParaShapeIndex::new(0));
                p.add_run(Run::table(
                    one_cell_table(vec![text_para("캡션표")]),
                    CharShapeIndex::new(0),
                ));
                p
            }],
            CaptionSide::default(),
        ));
        let mut nested_cell_para = Paragraph::new(ParaShapeIndex::new(0));
        nested_cell_para
            .add_run(Run::table(one_cell_table(vec![text_para("중첩")]), CharShapeIndex::new(0)));
        caption_host.rows[0].cells[0].paragraphs = vec![nested_cell_para];

        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(caption_host, CharShapeIndex::new(0)));

        let textbox = Control::TextBox {
            paragraphs: vec![{
                let mut p = Paragraph::new(ParaShapeIndex::new(0));
                p.add_run(Run::table(
                    one_cell_table(vec![text_para("글상자표")]),
                    CharShapeIndex::new(0),
                ));
                p
            }],
            caption: None,
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            style: None,
            text_vertical_align: VerticalAlign::default(),
        };
        let mut tb_para = Paragraph::new(ParaShapeIndex::new(0));
        tb_para.add_run(Run::control(textbox, CharShapeIndex::new(0)));

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host, tb_para], PageSettings::default()));

        let entries = tables_in_document(&doc);
        assert_eq!(entries.len(), 4);
        assert_eq!(render_path("", &entries[0].path), "paragraphs[0].runs[0].content.Table");
        assert_eq!(
            render_path("", &entries[1].path),
            "paragraphs[0].runs[0].content.Table.caption.paragraphs[0].runs[0].content.Table"
        );
        assert_eq!(
            render_path("", &entries[2].path),
            "paragraphs[0].runs[0].content.Table.rows[0].cells[0].paragraphs[0].runs[0].content.Table"
        );
        assert_eq!(
            render_path("", &entries[3].path),
            "paragraphs[1].runs[0].content.Control.TextBox.paragraphs[0].runs[0].content.Table"
        );
        assert!(entries.iter().enumerate().all(|(i, e)| e.ordinal == i && e.section == 0));
    }

    #[test]
    fn mutable_walker_mirrors_enumeration_order() {
        // Same doc shape as `ordinals_are_preorder_and_paths_are_serde_shaped`
        // plus footnote and rect-caption containers: both walkers must agree
        // on ordinal ↔ table identity (locked via per-table cell text).
        let mut caption_host = one_cell_table(vec![text_para("셀")]);
        caption_host.caption = Some(Caption::new(
            vec![{
                let mut p = Paragraph::new(ParaShapeIndex::new(0));
                p.add_run(Run::table(
                    one_cell_table(vec![text_para("캡션표")]),
                    CharShapeIndex::new(0),
                ));
                p
            }],
            CaptionSide::default(),
        ));
        let mut nested_cell_para = Paragraph::new(ParaShapeIndex::new(0));
        nested_cell_para
            .add_run(Run::table(one_cell_table(vec![text_para("중첩")]), CharShapeIndex::new(0)));
        caption_host.rows[0].cells[0].paragraphs = vec![nested_cell_para];
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(caption_host, CharShapeIndex::new(0)));

        let width = HwpUnit::new(1000).unwrap();
        let mut controls = Paragraph::new(ParaShapeIndex::new(0));
        controls.add_run(Run::control(
            Control::Footnote {
                inst_id: None,
                paragraphs: vec![{
                    let mut p = Paragraph::new(ParaShapeIndex::new(0));
                    p.add_run(Run::table(
                        one_cell_table(vec![text_para("각주표")]),
                        CharShapeIndex::new(0),
                    ));
                    p
                }],
            },
            CharShapeIndex::new(0),
        ));
        controls.add_run(Run::control(
            Control::Rect {
                width,
                height: width,
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(Caption::new(
                    vec![{
                        let mut p = Paragraph::new(ParaShapeIndex::new(0));
                        p.add_run(Run::table(
                            one_cell_table(vec![text_para("사각형캡션표")]),
                            CharShapeIndex::new(0),
                        ));
                        p
                    }],
                    CaptionSide::default(),
                )),
                style: None,
            },
            CharShapeIndex::new(0),
        ));

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host, controls], PageSettings::default()));

        let table_text = |t: &Table| -> String {
            t.rows[0].cells[0].paragraphs.first().map(|p| p.text_content()).unwrap_or_default()
        };
        let immutable: Vec<(usize, String)> =
            tables_in_document(&doc).iter().map(|e| (e.ordinal, table_text(e.table))).collect();
        let mut mutable: Vec<(usize, String)> = Vec::new();
        for_each_table_mut(&mut doc, &mut |ordinal, table| {
            mutable.push((ordinal, table_text(table)));
        });
        assert_eq!(immutable.len(), 5, "{immutable:?}");
        assert_eq!(immutable, mutable, "walkers must not drift");
    }

    #[test]
    fn section_paths_navigate_serialized_json() {
        // The path descriptors must actually land on the Table node in the
        // serde output — lockstep contract.
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(one_cell_table(vec![text_para("셀")]), CharShapeIndex::new(0)));
        let section = Section::with_paragraphs(vec![host], PageSettings::default());

        let value = serde_json::to_value(&section).expect("serialize section");
        let entries = tables_in_section(&section, 0);
        assert_eq!(entries.len(), 1);

        let mut node = &value;
        for seg in &entries[0].path {
            node = match seg {
                PathSeg::Field(name) => node.get(*name).expect("field present"),
                PathSeg::Index(i) => node.get(*i).expect("index present"),
            };
        }
        assert!(node.get("rows").is_some(), "path must land on the Table object");
    }
}
