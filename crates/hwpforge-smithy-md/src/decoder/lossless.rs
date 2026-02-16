//! Lossless body decoder.

use hwpforge_core::{
    Control, Image, ImageFormat, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use crate::error::{MdError, MdResult};

const ROOT_TAG: &str = "hwpforge-lossless-root";

/// Decodes a lossless body into Core sections.
pub(super) fn decode_lossless_sections(content: &str) -> MdResult<Vec<Section>> {
    let wrapped = format!("<{ROOT_TAG}>{content}</{ROOT_TAG}>");
    let mut reader = Reader::from_str(&wrapped);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut stack = Vec::new();
    let mut sections = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(start)) => {
                if start.name().as_ref() == ROOT_TAG.as_bytes() {
                    buf.clear();
                    continue;
                }
                let node = parse_start_tag(&reader, &start, &stack)?;
                stack.push(node);
            }
            Ok(Event::Empty(empty)) => {
                if empty.name().as_ref() == ROOT_TAG.as_bytes() {
                    buf.clear();
                    continue;
                }
                parse_empty_tag(&reader, &empty, &mut stack)?;
            }
            Ok(Event::End(end)) => {
                let tag = end.name().as_ref().to_vec();
                if tag.as_slice() == ROOT_TAG.as_bytes() {
                    buf.clear();
                    continue;
                }

                let node = stack.pop().ok_or_else(|| MdError::LosslessParse {
                    detail: format!(
                        "unexpected closing tag </{}>",
                        String::from_utf8_lossy(tag.as_slice())
                    ),
                })?;

                if node.tag_name().as_bytes() != tag.as_slice() {
                    return Err(MdError::LosslessParse {
                        detail: format!(
                            "tag mismatch: opened <{}> but closed </{}>",
                            node.tag_name(),
                            String::from_utf8_lossy(tag.as_slice())
                        ),
                    });
                }

                attach_closed_node(node, &mut stack, &mut sections)?;
            }
            Ok(Event::Text(text)) => {
                let value = text.unescape().map_err(|err| MdError::LosslessParse {
                    detail: format!("text decode failed: {err}"),
                })?;
                append_text(&mut stack, value.as_ref())?;
            }
            Ok(Event::CData(cdata)) => {
                let value = String::from_utf8_lossy(cdata.as_ref());
                append_text(&mut stack, value.as_ref())?;
            }
            Ok(Event::Comment(_))
            | Ok(Event::Decl(_))
            | Ok(Event::DocType(_))
            | Ok(Event::PI(_)) => {}
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(MdError::LosslessParse {
                    detail: format!("xml parse error at {}: {err}", reader.error_position()),
                });
            }
        }
        buf.clear();
    }

    if !stack.is_empty() {
        let open = stack.into_iter().map(|n| n.tag_name()).collect::<Vec<_>>().join(" -> ");
        return Err(MdError::LosslessParse { detail: format!("unclosed lossless tags: {open}") });
    }

    Ok(sections)
}

#[derive(Debug)]
enum OpenNode {
    Section(SectionNode),
    Paragraph(ParagraphNode),
    Table(TableNode),
    Row(RowNode),
    Cell(CellNode),
    Span(SpanNode),
    Link(LinkNode),
    TextBox(TextBoxNode),
    Footnote(FootnoteNode),
    UnknownControl(UnknownControlNode),
}

impl OpenNode {
    fn tag_name(&self) -> &'static str {
        match self {
            Self::Section(_) => "section",
            Self::Paragraph(_) => "p",
            Self::Table(_) => "table",
            Self::Row(_) => "tr",
            Self::Cell(_) => "td",
            Self::Span(_) => "span",
            Self::Link(_) => "a",
            Self::TextBox(_) => "textbox",
            Self::Footnote(_) => "footnote",
            Self::UnknownControl(_) => "control",
        }
    }
}

#[derive(Debug)]
struct SectionNode {
    page_settings: PageSettings,
    paragraphs: Vec<Paragraph>,
}

#[derive(Debug)]
struct ParagraphNode {
    para_shape_id: ParaShapeIndex,
    runs: Vec<Run>,
}

#[derive(Debug)]
struct TableNode {
    char_shape_id: CharShapeIndex,
    rows: Vec<TableRow>,
    width: Option<HwpUnit>,
    caption: Option<String>,
}

#[derive(Debug)]
struct RowNode {
    cells: Vec<TableCell>,
    height: Option<HwpUnit>,
}

#[derive(Debug)]
struct CellNode {
    col_span: u16,
    row_span: u16,
    width: HwpUnit,
    background: Option<Color>,
    paragraphs: Vec<Paragraph>,
}

#[derive(Debug)]
struct SpanNode {
    char_shape_id: CharShapeIndex,
    text: String,
}

#[derive(Debug)]
struct LinkNode {
    char_shape_id: CharShapeIndex,
    href: String,
    text: String,
}

#[derive(Debug)]
struct TextBoxNode {
    char_shape_id: CharShapeIndex,
    width: HwpUnit,
    height: HwpUnit,
    text: String,
    paragraphs: Vec<Paragraph>,
}

#[derive(Debug)]
struct FootnoteNode {
    char_shape_id: CharShapeIndex,
    text: String,
    paragraphs: Vec<Paragraph>,
}

#[derive(Debug)]
struct UnknownControlNode {
    char_shape_id: CharShapeIndex,
    kind: String,
    data: String,
}

fn parse_start_tag(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    stack: &[OpenNode],
) -> MdResult<OpenNode> {
    validate_parent(start.name().as_ref(), stack)?;
    match start.name().as_ref() {
        b"section" => Ok(OpenNode::Section(parse_section_node(reader, start)?)),
        b"p" => Ok(OpenNode::Paragraph(ParagraphNode {
            para_shape_id: parse_para_index_attr(reader, start, "p", "data-para-shape")?,
            runs: Vec::new(),
        })),
        b"table" => Ok(OpenNode::Table(parse_table_node(reader, start)?)),
        b"tr" => Ok(OpenNode::Row(RowNode {
            cells: Vec::new(),
            height: parse_optional_length_attr(
                reader,
                start,
                "tr",
                "data-height-unit",
                "data-height-mm",
            )?,
        })),
        b"td" => Ok(OpenNode::Cell(CellNode {
            col_span: parse_optional_u16_attr(reader, start, "td", "data-col-span")?.unwrap_or(1),
            row_span: parse_optional_u16_attr(reader, start, "td", "data-row-span")?.unwrap_or(1),
            width: parse_length_attr(reader, start, "td", "data-width-unit", "data-width-mm")?,
            background: parse_optional_color_attr(reader, start, "td", "data-background")?,
            paragraphs: Vec::new(),
        })),
        b"span" => Ok(OpenNode::Span(SpanNode {
            char_shape_id: parse_index_attr(reader, start, "span", "data-char-shape")?,
            text: String::new(),
        })),
        b"a" => Ok(OpenNode::Link(LinkNode {
            char_shape_id: parse_index_attr(reader, start, "a", "data-char-shape")?,
            href: required_attr(reader, start, "a", "href")?,
            text: String::new(),
        })),
        b"textbox" => Ok(OpenNode::TextBox(TextBoxNode {
            char_shape_id: parse_index_attr(reader, start, "textbox", "data-char-shape")?,
            width: parse_length_attr(reader, start, "textbox", "data-width-unit", "data-width-mm")?,
            height: parse_length_attr(
                reader,
                start,
                "textbox",
                "data-height-unit",
                "data-height-mm",
            )?,
            text: String::new(),
            paragraphs: Vec::new(),
        })),
        b"footnote" => Ok(OpenNode::Footnote(FootnoteNode {
            char_shape_id: parse_index_attr(reader, start, "footnote", "data-char-shape")?,
            text: String::new(),
            paragraphs: Vec::new(),
        })),
        b"control" => Ok(OpenNode::UnknownControl(UnknownControlNode {
            char_shape_id: parse_index_attr(reader, start, "control", "data-char-shape")?,
            kind: required_attr(reader, start, "control", "data-kind")?,
            data: String::new(),
        })),
        other => Err(MdError::LosslessParse {
            detail: format!("unsupported lossless tag <{}>", String::from_utf8_lossy(other)),
        }),
    }
}

fn parse_empty_tag(
    reader: &Reader<&[u8]>,
    empty: &BytesStart<'_>,
    stack: &mut [OpenNode],
) -> MdResult<()> {
    validate_parent(empty.name().as_ref(), stack)?;
    match empty.name().as_ref() {
        b"img" => {
            let char_shape_id = parse_index_attr(reader, empty, "img", "data-char-shape")?;
            let src = required_attr(reader, empty, "img", "src")?;
            let format = parse_image_format(&required_attr(reader, empty, "img", "data-format")?);
            let width =
                parse_length_attr(reader, empty, "img", "data-width-unit", "data-width-mm")?;
            let height =
                parse_length_attr(reader, empty, "img", "data-height-unit", "data-height-mm")?;

            let image = Image::new(src, width, height, format);
            push_run_to_parent(stack, Run::image(image, char_shape_id))
        }
        b"span" => {
            let char_shape_id = parse_index_attr(reader, empty, "span", "data-char-shape")?;
            push_run_to_parent(stack, Run::text("", char_shape_id))
        }
        b"a" => {
            let char_shape_id = parse_index_attr(reader, empty, "a", "data-char-shape")?;
            let href = required_attr(reader, empty, "a", "href")?;
            let link = Control::Hyperlink { text: String::new(), url: href };
            push_run_to_parent(stack, Run::control(link, char_shape_id))
        }
        b"p" => {
            let para_shape_id = parse_para_index_attr(reader, empty, "p", "data-para-shape")?;
            let paragraph =
                Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], para_shape_id);
            push_paragraph_to_parent(stack, paragraph)
        }
        other => Err(MdError::LosslessParse {
            detail: format!(
                "unsupported empty lossless tag <{} />",
                String::from_utf8_lossy(other)
            ),
        }),
    }
}

fn validate_parent(tag: &[u8], stack: &[OpenNode]) -> MdResult<()> {
    let parent = stack.last().map(OpenNode::tag_name);
    let valid = match tag {
        b"section" => parent.is_none(),
        b"p" => matches!(parent, Some("section" | "td" | "textbox" | "footnote")),
        b"table" => matches!(parent, Some("p")),
        b"tr" => matches!(parent, Some("table")),
        b"td" => matches!(parent, Some("tr")),
        b"span" | b"img" | b"a" | b"textbox" | b"footnote" | b"control" => {
            matches!(parent, Some("p"))
        }
        _ => true,
    };

    if valid {
        return Ok(());
    }

    Err(MdError::LosslessParse {
        detail: format!(
            "invalid nesting: <{}> cannot be inside <{}>",
            String::from_utf8_lossy(tag),
            parent.unwrap_or("<root>")
        ),
    })
}

fn append_text(stack: &mut [OpenNode], text: &str) -> MdResult<()> {
    if let Some(node) = stack.last_mut() {
        match node {
            OpenNode::Span(span) => span.text.push_str(text),
            OpenNode::Link(link) => link.text.push_str(text),
            OpenNode::TextBox(text_box) => text_box.text.push_str(text),
            OpenNode::Footnote(footnote) => footnote.text.push_str(text),
            OpenNode::UnknownControl(control) => control.data.push_str(text),
            _ => {
                if !text.trim().is_empty() {
                    return Err(MdError::LosslessParse {
                        detail: format!("unexpected text '{text}' under <{}>", node.tag_name()),
                    });
                }
            }
        }
    } else if !text.trim().is_empty() {
        return Err(MdError::LosslessParse {
            detail: format!("unexpected root-level text '{text}'"),
        });
    }

    Ok(())
}

fn attach_closed_node(
    node: OpenNode,
    stack: &mut [OpenNode],
    sections: &mut Vec<Section>,
) -> MdResult<()> {
    match node {
        OpenNode::Section(section) => {
            let paragraphs = if section.paragraphs.is_empty() {
                vec![Paragraph::with_runs(
                    vec![Run::text("", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]
            } else {
                section.paragraphs
            };
            sections.push(Section::with_paragraphs(paragraphs, section.page_settings));
            Ok(())
        }
        OpenNode::Paragraph(paragraph) => {
            let runs = if paragraph.runs.is_empty() {
                vec![Run::text("", CharShapeIndex::new(0))]
            } else {
                paragraph.runs
            };
            let paragraph = Paragraph::with_runs(runs, paragraph.para_shape_id);
            push_paragraph_to_parent(stack, paragraph)
        }
        OpenNode::Table(table) => {
            let rows = if table.rows.is_empty() {
                vec![TableRow {
                    cells: vec![TableCell::new(
                        vec![Paragraph::with_runs(
                            vec![Run::text("", CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        )],
                        HwpUnit::from_mm(10.0)?,
                    )],
                    height: None,
                }]
            } else {
                table.rows
            };
            let mut core_table = Table::new(rows);
            core_table.width = table.width;
            core_table.caption = table.caption;
            push_run_to_parent(stack, Run::table(core_table, table.char_shape_id))
        }
        OpenNode::Row(row) => {
            if let Some(OpenNode::Table(table)) = stack.last_mut() {
                table.rows.push(TableRow { cells: row.cells, height: row.height });
                Ok(())
            } else {
                Err(MdError::LosslessParse {
                    detail: "<tr> must be nested inside <table>".to_string(),
                })
            }
        }
        OpenNode::Cell(cell) => {
            let paragraphs = if cell.paragraphs.is_empty() {
                vec![Paragraph::with_runs(
                    vec![Run::text("", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]
            } else {
                cell.paragraphs
            };

            let mut table_cell =
                TableCell::with_span(paragraphs, cell.width, cell.col_span, cell.row_span);
            table_cell.background = cell.background;

            if let Some(OpenNode::Row(row)) = stack.last_mut() {
                row.cells.push(table_cell);
                Ok(())
            } else {
                Err(MdError::LosslessParse {
                    detail: "<td> must be nested inside <tr>".to_string(),
                })
            }
        }
        OpenNode::Span(span) => push_run_to_parent(stack, Run::text(span.text, span.char_shape_id)),
        OpenNode::Link(link) => push_run_to_parent(
            stack,
            Run::control(
                Control::Hyperlink { text: link.text, url: link.href },
                link.char_shape_id,
            ),
        ),
        OpenNode::TextBox(text_box) => {
            let paragraphs = if text_box.paragraphs.is_empty() {
                vec![Paragraph::with_runs(
                    vec![Run::text(text_box.text, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]
            } else {
                text_box.paragraphs
            };

            push_run_to_parent(
                stack,
                Run::control(
                    Control::TextBox { paragraphs, width: text_box.width, height: text_box.height },
                    text_box.char_shape_id,
                ),
            )
        }
        OpenNode::Footnote(footnote) => {
            let paragraphs = if footnote.paragraphs.is_empty() {
                vec![Paragraph::with_runs(
                    vec![Run::text(footnote.text, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]
            } else {
                footnote.paragraphs
            };

            push_run_to_parent(
                stack,
                Run::control(Control::Footnote { paragraphs }, footnote.char_shape_id),
            )
        }
        OpenNode::UnknownControl(control) => push_run_to_parent(
            stack,
            Run::control(
                Control::Unknown {
                    tag: control.kind,
                    data: if control.data.is_empty() { None } else { Some(control.data) },
                },
                control.char_shape_id,
            ),
        ),
    }
}

fn push_run_to_parent(stack: &mut [OpenNode], run: Run) -> MdResult<()> {
    if let Some(OpenNode::Paragraph(paragraph)) = stack.last_mut() {
        paragraph.runs.push(run);
        Ok(())
    } else {
        Err(MdError::LosslessParse {
            detail: format!(
                "inline run must be inside <p>, found parent {}",
                stack.last().map(OpenNode::tag_name).unwrap_or("<root>")
            ),
        })
    }
}

fn push_paragraph_to_parent(stack: &mut [OpenNode], paragraph: Paragraph) -> MdResult<()> {
    if let Some(parent) = stack.last_mut() {
        match parent {
            OpenNode::Section(section) => {
                section.paragraphs.push(paragraph);
                Ok(())
            }
            OpenNode::Cell(cell) => {
                cell.paragraphs.push(paragraph);
                Ok(())
            }
            OpenNode::TextBox(text_box) => {
                text_box.paragraphs.push(paragraph);
                Ok(())
            }
            OpenNode::Footnote(footnote) => {
                footnote.paragraphs.push(paragraph);
                Ok(())
            }
            _ => Err(MdError::LosslessParse {
                detail: format!("<p> cannot be nested inside <{}>", parent.tag_name()),
            }),
        }
    } else {
        Err(MdError::LosslessParse {
            detail: "<p> must be nested in <section> or <td>".to_string(),
        })
    }
}

fn parse_section_node(reader: &Reader<&[u8]>, start: &BytesStart<'_>) -> MdResult<SectionNode> {
    let mut page = PageSettings::a4();
    page.width = parse_length_attr(reader, start, "section", "data-width-unit", "data-width-mm")?;
    page.height =
        parse_length_attr(reader, start, "section", "data-height-unit", "data-height-mm")?;

    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-margin-left-unit",
        "data-margin-left-mm",
    )? {
        page.margin_left = v;
    }
    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-margin-right-unit",
        "data-margin-right-mm",
    )? {
        page.margin_right = v;
    }
    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-margin-top-unit",
        "data-margin-top-mm",
    )? {
        page.margin_top = v;
    }
    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-margin-bottom-unit",
        "data-margin-bottom-mm",
    )? {
        page.margin_bottom = v;
    }
    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-header-margin-unit",
        "data-header-margin-mm",
    )? {
        page.header_margin = v;
    }
    if let Some(v) = parse_optional_length_attr(
        reader,
        start,
        "section",
        "data-footer-margin-unit",
        "data-footer-margin-mm",
    )? {
        page.footer_margin = v;
    }

    Ok(SectionNode { page_settings: page, paragraphs: Vec::new() })
}

fn parse_table_node(reader: &Reader<&[u8]>, start: &BytesStart<'_>) -> MdResult<TableNode> {
    Ok(TableNode {
        char_shape_id: parse_index_attr(reader, start, "table", "data-char-shape")?,
        rows: Vec::new(),
        width: parse_optional_length_attr(
            reader,
            start,
            "table",
            "data-width-unit",
            "data-width-mm",
        )?,
        caption: attr_value(reader, start, "data-caption")?,
    })
}

fn parse_image_format(raw: &str) -> ImageFormat {
    match raw.to_ascii_uppercase().as_str() {
        "PNG" => ImageFormat::Png,
        "JPEG" | "JPG" => ImageFormat::Jpeg,
        "GIF" => ImageFormat::Gif,
        "BMP" => ImageFormat::Bmp,
        "WMF" => ImageFormat::Wmf,
        "EMF" => ImageFormat::Emf,
        _ => ImageFormat::Unknown(raw.to_string()),
    }
}

fn required_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<String> {
    attr_value(reader, start, attribute)?
        .ok_or(MdError::LosslessMissingAttribute { element, attribute })
}

fn attr_value(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    attribute: &'static str,
) -> MdResult<Option<String>> {
    for attr in start.attributes() {
        let attr = attr.map_err(|err| MdError::LosslessParse {
            detail: format!("attribute decode error: {err}"),
        })?;

        if attr.key.as_ref() == attribute.as_bytes() {
            let value = attr.decode_and_unescape_value(reader.decoder()).map_err(|err| {
                MdError::LosslessParse {
                    detail: format!("attribute value decode error ({attribute}): {err}"),
                }
            })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn parse_index_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<CharShapeIndex> {
    let value = required_attr(reader, start, element, attribute)?;
    let idx = value.parse::<usize>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;
    Ok(CharShapeIndex::new(idx))
}

fn parse_para_index_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<ParaShapeIndex> {
    let value = required_attr(reader, start, element, attribute)?;
    let idx = value.parse::<usize>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;
    Ok(ParaShapeIndex::new(idx))
}

fn parse_mm_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<HwpUnit> {
    let value = required_attr(reader, start, element, attribute)?;
    parse_mm_value(element, attribute, value)
}

fn parse_length_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    unit_attribute: &'static str,
    mm_attribute: &'static str,
) -> MdResult<HwpUnit> {
    if let Some(value) = attr_value(reader, start, unit_attribute)? {
        return parse_unit_value(element, unit_attribute, value);
    }
    parse_mm_attr(reader, start, element, mm_attribute)
}

fn parse_optional_length_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    unit_attribute: &'static str,
    mm_attribute: &'static str,
) -> MdResult<Option<HwpUnit>> {
    if let Some(value) = attr_value(reader, start, unit_attribute)? {
        return Ok(Some(parse_unit_value(element, unit_attribute, value)?));
    }
    parse_optional_mm_attr(reader, start, element, mm_attribute)
}

fn parse_optional_mm_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<HwpUnit>> {
    match attr_value(reader, start, attribute)? {
        Some(value) => Ok(Some(parse_mm_value(element, attribute, value)?)),
        None => Ok(None),
    }
}

fn parse_mm_value(
    element: &'static str,
    attribute: &'static str,
    value: String,
) -> MdResult<HwpUnit> {
    let mm = value.parse::<f64>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;

    HwpUnit::from_mm(mm).map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value,
    })
}

fn parse_unit_value(
    element: &'static str,
    attribute: &'static str,
    value: String,
) -> MdResult<HwpUnit> {
    let unit = value.parse::<i32>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;

    HwpUnit::new(unit).map_err(|_| MdError::LosslessInvalidAttribute { element, attribute, value })
}

fn parse_optional_u16_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<u16>> {
    match attr_value(reader, start, attribute)? {
        Some(value) => {
            let parsed = value.parse::<u16>().map_err(|_| MdError::LosslessInvalidAttribute {
                element,
                attribute,
                value,
            })?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

fn parse_optional_color_attr(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<Color>> {
    match attr_value(reader, start, attribute)? {
        Some(value) => {
            let hex = value.strip_prefix('#').unwrap_or(value.as_str());
            if hex.len() != 6 {
                return Err(MdError::LosslessInvalidAttribute { element, attribute, value });
            }
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| {
                MdError::LosslessInvalidAttribute { element, attribute, value: hex.to_string() }
            })?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| {
                MdError::LosslessInvalidAttribute { element, attribute, value: hex.to_string() }
            })?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| {
                MdError::LosslessInvalidAttribute { element, attribute, value: hex.to_string() }
            })?;
            Ok(Some(Color::from_rgb(r, g, b)))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::RunContent;

    #[test]
    fn decode_basic_lossless_section() {
        let input = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="3"><span data-char-shape="5">hello</span></p></section>"#;
        let sections = decode_lossless_sections(input).unwrap();

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].paragraphs.len(), 1);
        assert_eq!(sections[0].paragraphs[0].para_shape_id.get(), 3);
        assert_eq!(sections[0].paragraphs[0].runs[0].char_shape_id.get(), 5);
    }

    #[test]
    fn decode_lossless_table_cell_rich_content() {
        let input = r#"
<section data-index="0" data-width-mm="210.0" data-height-mm="297.0">
<p data-para-shape="1">
<table data-char-shape="2">
<tr>
<td data-col-span="1" data-row-span="1" data-width-mm="20.0">
<p data-para-shape="0"><a data-char-shape="7" href="https://example.com">Rust</a></p>
</td>
</tr>
</table>
</p>
</section>
"#;
        let sections = decode_lossless_sections(input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        let cell_paragraph = &table.rows[0].cells[0].paragraphs[0];

        assert!(matches!(
            cell_paragraph.runs[0].content,
            RunContent::Control(ref ctrl)
                if matches!(
                    ctrl.as_ref(),
                    Control::Hyperlink { text, url }
                        if text == "Rust" && url == "https://example.com"
                )
        ));
    }

    #[test]
    fn decode_invalid_tag_fails() {
        let input = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><unknown /></section>"#;
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_nested_section_fails_fast() {
        let input = r#"
<section data-index="0" data-width-mm="210.0" data-height-mm="297.0">
  <section data-index="1" data-width-mm="210.0" data-height-mm="297.0"></section>
</section>
"#;
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }
}
