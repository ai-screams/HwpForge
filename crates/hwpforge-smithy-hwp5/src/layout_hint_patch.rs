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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParagraphLayoutHint {
    line_segments: Vec<Hwp5ParaLineSeg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableLayoutHint {
    height: i32,
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
    let mut paragraphs = VecDeque::new();
    let mut tables = VecDeque::new();
    for paragraph in &section.paragraphs {
        collect_paragraph_layout_hints(paragraph, &mut paragraphs, &mut tables);
    }
    SectionLayoutHints { paragraphs, tables }
}

fn collect_paragraph_layout_hints(
    paragraph: &Hwp5Paragraph,
    paragraphs: &mut VecDeque<ParagraphLayoutHint>,
    tables: &mut VecDeque<TableLayoutHint>,
) {
    paragraphs.push_back(ParagraphLayoutHint { line_segments: paragraph.line_segments.clone() });

    for control in &paragraph.controls {
        match control {
            Hwp5Control::Table(table) => {
                tables
                    .push_back(TableLayoutHint { height: derive_table_height(table).unwrap_or(0) });
                for cell in &table.cells {
                    for paragraph in &cell.paragraphs {
                        collect_paragraph_layout_hints(paragraph, paragraphs, tables);
                    }
                }
            }
            Hwp5Control::Header(subtree) | Hwp5Control::Footer(subtree) => {
                for paragraph in &subtree.paragraphs {
                    collect_paragraph_layout_hints(paragraph, paragraphs, tables);
                }
            }
            Hwp5Control::TextBox(textbox) => {
                for paragraph in &textbox.paragraphs {
                    collect_paragraph_layout_hints(paragraph, paragraphs, tables);
                }
            }
            Hwp5Control::Image(_)
            | Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Polygon(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Unknown { .. } => {}
        }
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

fn patch_section_xml(xml: &str, mut hints: SectionLayoutHints) -> Hwp5Result<String> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(xml.len() + 1024)));
    let mut buf = Vec::new();
    let mut element_stack: Vec<Vec<u8>> = Vec::new();
    let mut paragraph_stack: Vec<ParagraphLayoutHint> = Vec::new();
    let mut table_stack: Vec<TableLayoutHint> = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("parse generated section xml: {e}") })?
        {
            Event::Start(event) => {
                let local = local_name(event.name().as_ref()).to_vec();
                let mut event = event.into_owned();
                if local.as_slice() == b"p" {
                    let hint = hints.paragraphs.pop_front().ok_or_else(|| Hwp5Error::Cfb {
                        detail: "paragraph layout hint count underflow".into(),
                    })?;
                    paragraph_stack.push(hint);
                } else if local.as_slice() == b"tbl" {
                    let hint = hints.tables.pop_front().ok_or_else(|| Hwp5Error::Cfb {
                        detail: "table layout hint count underflow".into(),
                    })?;
                    table_stack.push(hint);
                } else if local.as_slice() == b"sz"
                    && element_stack.last().is_some_and(|parent| parent.as_slice() == b"tbl")
                {
                    let current = table_stack.last().copied().ok_or_else(|| Hwp5Error::Cfb {
                        detail: "table size encountered without active table hint".into(),
                    })?;
                    event = rewrite_element_attr(event, "height", &current.height.to_string())?;
                }

                writer.write_event(Event::Start(event)).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;
                element_stack.push(local);
            }
            Event::Empty(event) => {
                let local = local_name(event.name().as_ref()).to_vec();
                let mut event = event.into_owned();
                if local.as_slice() == b"sz"
                    && element_stack.last().is_some_and(|parent| parent.as_slice() == b"tbl")
                {
                    let current = table_stack.last().copied().ok_or_else(|| Hwp5Error::Cfb {
                        detail: "table size encountered without active table hint".into(),
                    })?;
                    event = rewrite_element_attr(event, "height", &current.height.to_string())?;
                }
                writer.write_event(Event::Empty(event)).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;
            }
            Event::End(event) => {
                let local = local_name(event.name().as_ref()).to_vec();
                if local.as_slice() == b"p" {
                    let hint = paragraph_stack.pop().ok_or_else(|| Hwp5Error::Cfb {
                        detail: "paragraph layout hint stack underflow".into(),
                    })?;
                    write_linesegarray(&mut writer, &hint.line_segments)?;
                }

                writer.write_event(Event::End(event.into_owned())).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;

                let popped = element_stack.pop().ok_or_else(|| Hwp5Error::Cfb {
                    detail: "xml element stack underflow".into(),
                })?;
                if popped != local {
                    return Err(Hwp5Error::Cfb {
                        detail: format!(
                            "xml element stack mismatch: opened '{}' closed '{}'",
                            String::from_utf8_lossy(&popped),
                            String::from_utf8_lossy(&local)
                        ),
                    });
                }
                if local.as_slice() == b"tbl" {
                    table_stack.pop();
                }
            }
            Event::Eof => break,
            event => {
                writer.write_event(event.into_owned()).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;
            }
        }
        buf.clear();
    }

    if !hints.paragraphs.is_empty() || !hints.tables.is_empty() || !paragraph_stack.is_empty() {
        return Err(Hwp5Error::Cfb { detail: "layout hint patch left unconsumed hints".into() });
    }

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

fn write_linesegarray<W: Write>(
    writer: &mut Writer<W>,
    line_segments: &[Hwp5ParaLineSeg],
) -> Hwp5Result<()> {
    if line_segments.is_empty() {
        return Ok(());
    }

    writer
        .write_event(Event::Start(BytesStart::new("hp:linesegarray")))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write linesegarray start: {e}") })?;

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
            tables: VecDeque::from(vec![TableLayoutHint { height: 4482 }]),
        };

        let patched = patch_section_xml(xml, hints).expect("section xml should patch");
        assert!(patched
            .contains(r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000""#));
        assert!(patched.contains(r#"height="4482""#));
    }
}
