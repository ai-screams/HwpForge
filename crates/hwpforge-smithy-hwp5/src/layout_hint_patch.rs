use std::collections::{BTreeMap, VecDeque};
use std::io::{Cursor, Read as _, Write};

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::decoder::section::{Hwp5Control, Hwp5Paragraph, Hwp5Table, SectionResult};
use crate::error::{Hwp5Error, Hwp5Result};
use crate::schema::section::Hwp5ParaLineSeg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectionLayoutHints {
    paragraphs: VecDeque<ParagraphLayoutHint>,
    tables: VecDeque<TableLayoutHint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ScopeLayoutHints {
    paragraphs: VecDeque<ParagraphLayoutHint>,
    tables: VecDeque<TableLayoutHint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParagraphLayoutHint {
    line_segments: Vec<Hwp5ParaLineSeg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableLayoutHint {
    height: Option<i32>,
}

#[derive(Debug, Clone)]
struct RawPackage {
    entries: Vec<RawPackageEntry>,
    index_by_path: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct RawPackageEntry {
    path: String,
    bytes: Vec<u8>,
    compression: CompressionMethod,
}

pub(crate) fn capture_layout_hints(sections: &[SectionResult]) -> Vec<SectionLayoutHints> {
    sections.iter().map(collect_section_layout_hints).collect()
}

pub(crate) fn patch_hwpx_layout_hints(
    hwpx_bytes: &[u8],
    sections: &[SectionLayoutHints],
) -> Hwp5Result<Vec<u8>> {
    let mut package = RawPackage::read(hwpx_bytes)?;
    for (section_idx, section) in sections.iter().enumerate() {
        if !section.has_payload() {
            continue;
        }

        let path = format!("Contents/section{section_idx}.xml");
        let xml = package.read_text_entry(&path)?;
        let patched = patch_section_xml(&xml, section.clone())?;
        package.replace_text_entry(&path, patched);
    }
    package.write()
}

impl SectionLayoutHints {
    fn has_payload(&self) -> bool {
        self.paragraphs.iter().any(|hint| !hint.line_segments.is_empty()) || !self.tables.is_empty()
    }
}

impl RawPackage {
    fn read(bytes: &[u8]) -> Hwp5Result<Self> {
        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("open hwpx package: {e}") })?;
        let mut entries = Vec::with_capacity(archive.len());
        let mut index_by_path = BTreeMap::new();

        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|e| Hwp5Error::Cfb { detail: format!("read hwpx entry #{index}: {e}") })?;
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut bytes).map_err(|e| Hwp5Error::Cfb {
                detail: format!("read '{}' bytes: {e}", file.name()),
            })?;
            let path = file.name().to_string();
            index_by_path.insert(path.clone(), entries.len());
            entries.push(RawPackageEntry { path, bytes, compression: file.compression() });
        }

        Ok(Self { entries, index_by_path })
    }

    fn read_text_entry(&self, path: &str) -> Hwp5Result<String> {
        let index = self
            .index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| Hwp5Error::MissingStream { name: path.to_string() })?;
        String::from_utf8(self.entries[index].bytes.clone()).map_err(|e| Hwp5Error::Cfb {
            detail: format!("entry '{path}' is not valid UTF-8: {e}"),
        })
    }

    fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    fn write(&self) -> Hwp5Result<Vec<u8>> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);
        for entry in &self.entries {
            let options = SimpleFileOptions::default().compression_method(entry.compression);
            zip.start_file(&entry.path, options).map_err(|e| Hwp5Error::Cfb {
                detail: format!("zip start '{}': {e}", entry.path),
            })?;
            zip.write_all(&entry.bytes).map_err(|e| Hwp5Error::Cfb {
                detail: format!("zip write '{}': {e}", entry.path),
            })?;
        }
        let cursor = zip
            .finish()
            .map_err(|e| Hwp5Error::Cfb { detail: format!("finish hwpx patch package: {e}") })?;
        Ok(cursor.into_inner())
    }
}

fn collect_section_layout_hints(section: &SectionResult) -> SectionLayoutHints {
    let mut body = ScopeLayoutHints::default();
    let mut header = ScopeLayoutHints::default();
    let mut footer = ScopeLayoutHints::default();

    for paragraph in &section.paragraphs {
        collect_flow_paragraph_layout_hints(paragraph, &mut body, &mut header, &mut footer);
    }

    body.append(&mut header);
    body.append(&mut footer);

    SectionLayoutHints { paragraphs: body.paragraphs, tables: body.tables }
}

impl ScopeLayoutHints {
    fn push_paragraph(&mut self, paragraph: &Hwp5Paragraph) {
        self.paragraphs
            .push_back(ParagraphLayoutHint { line_segments: paragraph.line_segments.clone() });
    }

    fn push_table(&mut self, table: &Hwp5Table) {
        self.tables.push_back(TableLayoutHint { height: derive_table_height(table) });
    }

    fn append(&mut self, other: &mut Self) {
        self.paragraphs.append(&mut other.paragraphs);
        self.tables.append(&mut other.tables);
    }
}

fn collect_flow_paragraph_layout_hints(
    paragraph: &Hwp5Paragraph,
    body: &mut ScopeLayoutHints,
    header: &mut ScopeLayoutHints,
    footer: &mut ScopeLayoutHints,
) {
    body.push_paragraph(paragraph);

    for control in &paragraph.controls {
        match control {
            Hwp5Control::Table(table) => collect_table_layout_hints(table, body),
            Hwp5Control::Header(subtree) => collect_scope_paragraphs(&subtree.paragraphs, header),
            Hwp5Control::Footer(subtree) => collect_scope_paragraphs(&subtree.paragraphs, footer),
            Hwp5Control::Footnote(subtree) | Hwp5Control::Endnote(subtree) => {
                collect_scope_paragraphs(&subtree.paragraphs, body);
            }
            Hwp5Control::TextBox(textbox) => collect_scope_paragraphs(&textbox.paragraphs, body),
            // Memo body paragraphs are emitted as `<hp:subList>` children of
            // the body paragraph in HWPX, so the patcher consumes layout
            // hints for them from the body scope — same pattern as
            // Footnote/Endnote.
            Hwp5Control::Memo(memo) => collect_scope_paragraphs(&memo.paragraphs, body),
            // Group children with text (rect/ellipse drawText) carry their
            // own paragraphs that need linesegarray hints, mirroring TextBox.
            Hwp5Control::Group(group) => collect_group_child_layout_hints(group, body),
            Hwp5Control::Image(_)
            | Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Polygon(_)
            | Hwp5Control::Ellipse(_)
            | Hwp5Control::Arc(_)
            | Hwp5Control::Curve(_)
            | Hwp5Control::TextArt(_)
            | Hwp5Control::ConnectLine(_)
            | Hwp5Control::Equation(_)
            | Hwp5Control::Dutmal(_)
            | Hwp5Control::Compose(_)
            | Hwp5Control::IndexMark(_)
            | Hwp5Control::ClickHere(_)
            | Hwp5Control::SummeryField(_)
            | Hwp5Control::DateCodeField(_)
            | Hwp5Control::PathField(_)
            | Hwp5Control::CrossRef(_)
            | Hwp5Control::InlinePageNumber(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Unknown { .. } => {}
        }
    }
}

/// Collects layout hints from a group's text-bearing children (rect/ellipse
/// with `drawText`). Non-text children carry no paragraphs. A child that is
/// itself a nested group recurses so the inner group's text-bearing leaves
/// (whose `<hp:p>` the encoder still emits) contribute hints — without this
/// the patcher would underflow on nested groups.
fn collect_group_child_layout_hints(
    group: &crate::decoder::section::Hwp5GroupControl,
    scope: &mut ScopeLayoutHints,
) {
    for child in &group.children {
        if let Hwp5Control::Group(nested) = &child.control {
            collect_group_child_layout_hints(nested, scope);
        }
        collect_scope_paragraphs(&child.paragraphs, scope);
    }
}

fn collect_scope_paragraphs(paragraphs: &[Hwp5Paragraph], scope: &mut ScopeLayoutHints) {
    for paragraph in paragraphs {
        collect_scope_paragraph_layout_hints(paragraph, scope);
    }
}

fn collect_scope_paragraph_layout_hints(paragraph: &Hwp5Paragraph, scope: &mut ScopeLayoutHints) {
    scope.push_paragraph(paragraph);

    for control in &paragraph.controls {
        match control {
            Hwp5Control::Table(table) => collect_table_layout_hints(table, scope),
            Hwp5Control::TextBox(textbox) => collect_scope_paragraphs(&textbox.paragraphs, scope),
            Hwp5Control::Footnote(subtree) | Hwp5Control::Endnote(subtree) => {
                collect_scope_paragraphs(&subtree.paragraphs, scope);
            }
            Hwp5Control::Memo(memo) => collect_scope_paragraphs(&memo.paragraphs, scope),
            Hwp5Control::Group(group) => collect_group_child_layout_hints(group, scope),
            Hwp5Control::Header(_)
            | Hwp5Control::Footer(_)
            | Hwp5Control::Image(_)
            | Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Polygon(_)
            | Hwp5Control::Ellipse(_)
            | Hwp5Control::Arc(_)
            | Hwp5Control::Curve(_)
            | Hwp5Control::TextArt(_)
            | Hwp5Control::ConnectLine(_)
            | Hwp5Control::Equation(_)
            | Hwp5Control::Dutmal(_)
            | Hwp5Control::Compose(_)
            | Hwp5Control::IndexMark(_)
            | Hwp5Control::ClickHere(_)
            | Hwp5Control::SummeryField(_)
            | Hwp5Control::DateCodeField(_)
            | Hwp5Control::PathField(_)
            | Hwp5Control::CrossRef(_)
            | Hwp5Control::InlinePageNumber(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Unknown { .. } => {}
        }
    }
}

fn collect_table_layout_hints(table: &Hwp5Table, scope: &mut ScopeLayoutHints) {
    scope.push_table(table);
    for cell in &table.cells {
        collect_scope_paragraphs(&cell.paragraphs, scope);
    }
}

fn derive_table_height(table: &Hwp5Table) -> Option<i32> {
    if table.rows == 0 || table.cells.iter().any(|cell| cell.row_span != 1) {
        return None;
    }

    let mut row_heights = vec![0i32; table.rows as usize];
    for cell in &table.cells {
        let row = usize::from(cell.row);
        if row >= row_heights.len() {
            continue;
        }
        let content_height = cell.paragraphs.iter().map(paragraph_layout_height).max().unwrap_or(0);
        let margin_height =
            i32::from(cell.margin.top).saturating_add(i32::from(cell.margin.bottom));
        let hinted_height = content_height.saturating_add(margin_height);
        row_heights[row] = row_heights[row].max(hinted_height.max(cell.height.max(0)));
    }

    let cell_spacing = i32::from(table.cell_spacing.max(0));
    let total_height = row_heights
        .into_iter()
        .sum::<i32>()
        .saturating_add(cell_spacing.saturating_mul(i32::from(table.rows.saturating_sub(1))));
    (total_height > 0).then_some(total_height)
}

fn paragraph_layout_height(paragraph: &Hwp5Paragraph) -> i32 {
    paragraph
        .line_segments
        .iter()
        .map(|segment| segment.vertical_position.saturating_add(segment.line_height))
        .max()
        .unwrap_or(0)
}

#[derive(Debug)]
struct SectionXmlPatchState {
    hints: SectionLayoutHints,
    element_stack: Vec<Vec<u8>>,
    paragraph_stack: Vec<ParagraphLayoutHint>,
    table_stack: Vec<TableLayoutHint>,
}

impl SectionXmlPatchState {
    fn new(hints: SectionLayoutHints) -> Self {
        Self {
            hints,
            element_stack: Vec::new(),
            paragraph_stack: Vec::new(),
            table_stack: Vec::new(),
        }
    }

    fn handle_start<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        if local.as_slice() == b"p" {
            self.push_paragraph_hint()?;
        } else if local.as_slice() == b"tbl" {
            self.push_table_hint()?;
        }

        let event = self.patch_table_size_event(local.as_slice(), event.into_owned())?;
        writer
            .write_event(Event::Start(event))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;
        self.element_stack.push(local);
        Ok(())
    }

    fn handle_empty<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        let event = self.patch_table_size_event(local.as_slice(), event.into_owned())?;
        writer
            .write_event(Event::Empty(event))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;
        Ok(())
    }

    fn handle_end<W: Write>(
        &mut self,
        event: BytesEnd<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        if local.as_slice() == b"p" {
            let hint = self.pop_paragraph_hint()?;
            write_linesegarray(writer, &hint.line_segments)?;
        }

        writer
            .write_event(Event::End(event.into_owned()))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;

        self.pop_element(local.as_slice())?;
        if local.as_slice() == b"tbl" {
            self.table_stack.pop();
        }
        Ok(())
    }

    fn finish(self) -> Hwp5Result<()> {
        if !self.hints.paragraphs.is_empty()
            || !self.hints.tables.is_empty()
            || !self.paragraph_stack.is_empty()
        {
            return Err(Hwp5Error::Cfb {
                detail: "layout hint patch left unconsumed hints".into(),
            });
        }
        Ok(())
    }

    fn patch_table_size_event(
        &self,
        local: &[u8],
        event: BytesStart<'static>,
    ) -> Hwp5Result<BytesStart<'static>> {
        if !self.is_active_table_size_element(local) {
            return Ok(event);
        }

        let Some(height) = self.active_table_height()? else {
            return Ok(event);
        };
        rewrite_element_attr(event, "height", &height.to_string())
    }

    fn is_active_table_size_element(&self, local: &[u8]) -> bool {
        local == b"sz"
            && self.element_stack.last().is_some_and(|parent| parent.as_slice() == b"tbl")
    }

    fn active_table_height(&self) -> Hwp5Result<Option<i32>> {
        self.table_stack
            .last()
            .copied()
            .ok_or_else(|| Hwp5Error::Cfb {
                detail: "table size encountered without active table hint".into(),
            })
            .map(|hint| hint.height)
    }

    fn push_paragraph_hint(&mut self) -> Hwp5Result<()> {
        let hint = self.hints.paragraphs.pop_front().ok_or_else(|| Hwp5Error::Cfb {
            detail: "paragraph layout hint count underflow".into(),
        })?;
        self.paragraph_stack.push(hint);
        Ok(())
    }

    fn push_table_hint(&mut self) -> Hwp5Result<()> {
        let hint =
            self.hints.tables.pop_front().ok_or_else(|| Hwp5Error::Cfb {
                detail: "table layout hint count underflow".into(),
            })?;
        self.table_stack.push(hint);
        Ok(())
    }

    fn pop_paragraph_hint(&mut self) -> Hwp5Result<ParagraphLayoutHint> {
        self.paragraph_stack.pop().ok_or_else(|| Hwp5Error::Cfb {
            detail: "paragraph layout hint stack underflow".into(),
        })
    }

    fn pop_element(&mut self, local: &[u8]) -> Hwp5Result<()> {
        let popped = self
            .element_stack
            .pop()
            .ok_or_else(|| Hwp5Error::Cfb { detail: "xml element stack underflow".into() })?;
        if popped != local {
            return Err(Hwp5Error::Cfb {
                detail: format!(
                    "xml element stack mismatch: opened '{}' closed '{}'",
                    String::from_utf8_lossy(&popped),
                    String::from_utf8_lossy(local)
                ),
            });
        }
        Ok(())
    }
}

fn patch_section_xml(xml: &str, hints: SectionLayoutHints) -> Hwp5Result<String> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(xml.len() + 1024)));
    let mut buf = Vec::new();
    let mut state = SectionXmlPatchState::new(hints);

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("parse generated section xml: {e}") })?
        {
            Event::Start(event) => state.handle_start(event, &mut writer)?,
            Event::Empty(event) => state.handle_empty(event, &mut writer)?,
            Event::End(event) => state.handle_end(event, &mut writer)?,
            Event::Eof => break,
            event => {
                writer.write_event(event.into_owned()).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;
            }
        }
        buf.clear();
    }

    state.finish()?;

    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| Hwp5Error::Cfb {
        detail: format!("patched section xml is not valid UTF-8: {e}"),
    })
}

fn rewrite_element_attr(
    event: BytesStart<'static>,
    target_attr: &str,
    new_value: &str,
) -> Hwp5Result<BytesStart<'static>> {
    let name = String::from_utf8(event.name().as_ref().to_vec())
        .map_err(|e| Hwp5Error::Cfb { detail: format!("element name is not valid UTF-8: {e}") })?;
    let mut rebuilt = BytesStart::new(name);
    let mut replaced = false;

    for attr in event.attributes().with_checks(false) {
        let attr =
            attr.map_err(|e| Hwp5Error::Cfb { detail: format!("read xml attribute: {e}") })?;
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| Hwp5Error::Cfb {
            detail: format!("attribute key is not valid UTF-8: {e}"),
        })?;
        let value = if local_name(attr.key.as_ref()) == target_attr.as_bytes() {
            replaced = true;
            new_value
        } else {
            std::str::from_utf8(attr.value.as_ref()).map_err(|e| Hwp5Error::Cfb {
                detail: format!("attribute value is not valid UTF-8: {e}"),
            })?
        };
        rebuilt.push_attribute((key, value));
    }

    if !replaced {
        rebuilt.push_attribute((target_attr, new_value));
    }

    Ok(rebuilt)
}

/// Conservative default lineseg values for paragraphs whose HWP5 source
/// does not carry a `ParaLineSeg` (tag `0x45`) record (task #123).
///
/// Without an emitted `<hp:linesegarray>`, Hancom Office flags the file
/// as needing "low-security recovery" because it cannot pre-compute the
/// rendering cache. Emitting a single placeholder segment with native-
/// matching default attributes lets Hancom open the file silently — it
/// recomputes per-line metrics on first render anyway.
///
/// Values empirically derived from native `sample-field-docsummary.hwpx`:
/// - `vertsize/textheight = 1000` (10pt baseline, native default)
/// - `baseline = 850`, `spacing = 600`
/// - `horzsize = 42520` (A4 content width, 1cm L/R margin)
/// - `flags = 393216` (Hancom's default lineseg flag bitmask)
///
/// `vertpos` is intentionally left at `0` — Hancom recomputes the cumulative
/// vertical position from paragraph order on open.
fn write_default_lineseg<W: Write>(writer: &mut Writer<W>) -> Hwp5Result<()> {
    let mut line = BytesStart::new("hp:lineseg");
    line.push_attribute(("textpos", "0"));
    line.push_attribute(("vertpos", "0"));
    line.push_attribute(("vertsize", "1000"));
    line.push_attribute(("textheight", "1000"));
    line.push_attribute(("baseline", "850"));
    line.push_attribute(("spacing", "600"));
    line.push_attribute(("horzpos", "0"));
    line.push_attribute(("horzsize", "42520"));
    line.push_attribute(("flags", "393216"));
    writer
        .write_event(Event::Empty(line))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write default lineseg: {e}") })?;
    Ok(())
}

fn write_linesegarray<W: Write>(
    writer: &mut Writer<W>,
    line_segments: &[Hwp5ParaLineSeg],
) -> Hwp5Result<()> {
    // task #123: always emit linesegarray (even if HWP5 source omitted
    // the ParaLineSeg record). Hancom recomputes accurate metrics on
    // first render, but the element's *presence* is required to skip
    // the "low-security recovery" warning.
    writer
        .write_event(Event::Start(BytesStart::new("hp:linesegarray")))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write linesegarray start: {e}") })?;

    if line_segments.is_empty() {
        write_default_lineseg(writer)?;
    } else {
        for segment in line_segments {
            let textpos = segment.text_start_position.to_string();
            let vertpos = segment.vertical_position.to_string();
            let vertsize = segment.line_height.to_string();
            let textheight = segment.text_height.to_string();
            let baseline = segment.baseline_distance.to_string();
            let spacing = segment.line_spacing.to_string();
            let horzpos = segment.column_start_position.to_string();
            let horzsize = segment.segment_width.to_string();
            let flags = segment.tag.to_string();

            let mut line = BytesStart::new("hp:lineseg");
            line.push_attribute(("textpos", textpos.as_str()));
            line.push_attribute(("vertpos", vertpos.as_str()));
            line.push_attribute(("vertsize", vertsize.as_str()));
            line.push_attribute(("textheight", textheight.as_str()));
            line.push_attribute(("baseline", baseline.as_str()));
            line.push_attribute(("spacing", spacing.as_str()));
            line.push_attribute(("horzpos", horzpos.as_str()));
            line.push_attribute(("horzsize", horzsize.as_str()));
            line.push_attribute(("flags", flags.as_str()));

            writer
                .write_event(Event::Empty(line))
                .map_err(|e| Hwp5Error::Cfb { detail: format!("write lineseg: {e}") })?;
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new("hp:linesegarray")))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write linesegarray end: {e}") })?;
    Ok(())
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::section::Hwp5NestedSubtree;

    fn paragraph(
        text: &str,
        line_segments: Vec<Hwp5ParaLineSeg>,
        controls: Vec<Hwp5Control>,
    ) -> Hwp5Paragraph {
        Hwp5Paragraph {
            text: text.into(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: Vec::new(),
            line_segments,
            controls,
        }
    }

    fn line_segment(
        text_start_position: u32,
        vertical_position: i32,
        line_height: i32,
    ) -> Hwp5ParaLineSeg {
        Hwp5ParaLineSeg {
            text_start_position,
            vertical_position,
            line_height,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 20272,
            tag: 393216,
        }
    }

    #[test]
    fn derive_table_height_uses_line_segments_and_cell_margin() {
        let table = Hwp5Table {
            rows: 1,
            cols: 1,
            page_break: crate::decoder::section::Hwp5TablePageBreak::Cell,
            repeat_header: false,
            cell_spacing: 0,
            border_fill_id: None,
            cells: vec![crate::decoder::section::Hwp5TableCell {
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 1000,
                height: 0,
                margin: crate::decoder::section::Hwp5TableCellMargin {
                    left: 0,
                    right: 0,
                    top: 141,
                    bottom: 141,
                },
                vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                is_header: false,
                border_fill_id: None,
                paragraphs: vec![Hwp5Paragraph {
                    text: "cell".into(),
                    text_segments: Vec::new(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    line_segments: vec![
                        line_segment(0, 0, 1000),
                        line_segment(20, 1600, 1000),
                        line_segment(48, 3200, 1000),
                    ],
                    controls: Vec::new(),
                }],
            }],
            instance_id: 0,
        };

        assert_eq!(derive_table_height(&table), Some(4482));
    }

    #[test]
    fn patch_section_xml_injects_linesegarray_and_table_height() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>P0</hp:t></hp:run></hp:p>"#,
            r#"<hp:p id="1" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:tbl id="1" rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="2"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="0" heightRelTo="ABSOLUTE" protect="0"/><hp:tr><hp:tc borderFillIDRef="2"><hp:subList><hp:p id="2" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>CELL</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint {
                    line_segments: vec![
                        line_segment(0, 0, 1000),
                        line_segment(20, 1600, 1000),
                        line_segment(48, 3200, 1000),
                    ],
                },
            ]),
            tables: VecDeque::from(vec![TableLayoutHint { height: Some(4482) }]),
        };

        let patched = patch_section_xml(xml, hints).expect("section xml should patch");
        assert!(patched
            .contains(r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000""#));
        assert!(patched.contains(r#"height="4482""#));
    }

    #[test]
    fn collect_section_layout_hints_orders_body_before_header_and_footer() {
        let section = SectionResult {
            paragraphs: vec![
                paragraph(
                    "body-a",
                    vec![line_segment(10, 0, 1000)],
                    vec![Hwp5Control::Header(Hwp5NestedSubtree {
                        ctrl_id: 0x6865_6164,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![paragraph(
                            "header",
                            vec![line_segment(30, 0, 1000)],
                            Vec::new(),
                        )],
                    })],
                ),
                paragraph(
                    "body-b",
                    vec![line_segment(20, 0, 1000)],
                    vec![Hwp5Control::Footer(Hwp5NestedSubtree {
                        ctrl_id: 0x666F_6F74,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![paragraph(
                            "footer",
                            vec![line_segment(40, 0, 1000)],
                            Vec::new(),
                        )],
                    })],
                ),
            ],
            page_def: None,
            section_def_properties: None,
            page_border_fills: Vec::new(),
            column_def: None,
            warnings: Vec::new(),
        };

        let hints = collect_section_layout_hints(&section);
        let order = hints
            .paragraphs
            .iter()
            .map(|hint| hint.line_segments[0].text_start_position)
            .collect::<Vec<_>>();

        assert_eq!(order, vec![10, 20, 30, 40]);
    }

    #[test]
    fn patch_section_xml_skips_table_height_when_hint_is_unknown() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:tbl id="1" rowCnt="2" colCnt="1" cellSpacing="0" borderFillIDRef="2"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="7777" heightRelTo="ABSOLUTE" protect="0"/><hp:tr><hp:tc borderFillIDRef="2"><hp:subList><hp:p id="1" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>CELL</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
            ]),
            tables: VecDeque::from(vec![TableLayoutHint { height: None }]),
        };

        let patched = patch_section_xml(xml, hints).expect("section xml should patch");
        assert!(patched.contains(r#"height="7777""#));
        assert!(!patched.contains(r#"height="0""#));
    }
}
