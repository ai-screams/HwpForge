//! Lossless body decoder.

use hwpforge_core::control::{Fill, GroupChild, ShapePoint, ShapeStyle};
use hwpforge_core::{
    Control, Image, ImageFormat, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use crate::error::{MdError, MdResult};
use crate::lossless_grammar as g;

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
                if start.name().as_ref() == ROOT_TAG {
                    buf.clear();
                    continue;
                }
                let node = parse_start_tag(&start, &stack)?;
                stack.push(node);
            }
            Ok(Event::Empty(empty)) => {
                if empty.name().as_ref() == ROOT_TAG {
                    buf.clear();
                    continue;
                }
                parse_empty_tag(&empty, &mut stack)?;
            }
            Ok(Event::End(end)) => {
                let name = end.name();
                let tag: &str = name.as_ref();
                if tag == ROOT_TAG {
                    buf.clear();
                    continue;
                }

                let node = stack.pop().ok_or_else(|| MdError::LosslessParse {
                    detail: format!("unexpected closing tag </{tag}>"),
                })?;

                if node.tag_name() != tag {
                    return Err(MdError::LosslessParse {
                        detail: format!(
                            "tag mismatch: opened <{}> but closed </{tag}>",
                            node.tag_name()
                        ),
                    });
                }

                attach_closed_node(node, &mut stack, &mut sections)?;
            }
            Ok(Event::Text(text)) => {
                append_text(&mut stack, &text)?;
            }
            Ok(Event::CData(cdata)) => {
                append_text(&mut stack, &cdata)?;
            }
            Ok(Event::Comment(_))
            | Ok(Event::Decl(_))
            | Ok(Event::DocType(_))
            | Ok(Event::PI(_)) => {}
            Ok(Event::GeneralRef(reference)) => {
                let resolved = resolve_general_ref(&reference)?;
                append_text(&mut stack, resolved.encode_utf8(&mut [0u8; 4]))?;
            }
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
    Shape(Box<ShapeNode>),
    Footnote(FootnoteNode),
    UnknownControl(UnknownControlNode),
    Group(Box<GroupNode>),
    Picture(Box<PictureNode>),
    Caption(Box<hwpforge_core::caption::Caption>),
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
            Self::Shape(node) => node.tag,
            Self::Footnote(_) => "footnote",
            Self::UnknownControl(_) => "control",
            Self::Group(_) => "group",
            Self::Picture(_) => "img",
            Self::Caption(_) => "caption",
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
    frame: hwpforge_core::ObjectFrame,
    rows: Vec<TableRow>,
    width: Option<HwpUnit>,
    caption: Option<hwpforge_core::caption::Caption>,
    no_adjust: bool,
    cell_zones: Vec<hwpforge_core::table::CellZone>,
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
    line_wrap: hwpforge_core::table::CellLineWrap,
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

/// An open drawing shape (`<textbox>`, `<ellipse>`, `<polygon>`): its
/// fields, its paragraphs as they close, and — for an old-form text box —
/// its bare text.
#[derive(Debug)]
struct ShapeNode {
    tag: &'static str,
    char_shape_id: Option<CharShapeIndex>,
    control: Control,
    text: String,
    /// No attribute an old encoder could not write: a text box without `<p>`
    /// reads its text the old way.
    old_form: bool,
}

#[derive(Debug)]
struct FootnoteNode {
    char_shape_id: CharShapeIndex,
    text: String,
    paragraphs: Vec<Paragraph>,
}

/// An open `<group>`: its own fields, and its children as they close.
#[derive(Debug)]
struct GroupNode {
    char_shape_id: Option<CharShapeIndex>,
    control: Control,
}

/// An open `<img>` (one with a caption child).
#[derive(Debug)]
struct PictureNode {
    char_shape_id: Option<CharShapeIndex>,
    image: Image,
}

#[derive(Debug)]
struct UnknownControlNode {
    char_shape_id: CharShapeIndex,
    kind: String,
    data: String,
}

fn parse_start_tag(start: &BytesStart<'_>, stack: &[OpenNode]) -> MdResult<OpenNode> {
    let name = start.name();
    let tag: &str = name.as_ref();
    validate_parent(tag, stack)?;
    match tag {
        "section" => Ok(OpenNode::Section(parse_section_node(start)?)),
        "p" => Ok(OpenNode::Paragraph(ParagraphNode {
            para_shape_id: parse_para_index_attr(start, "p", "data-para-shape")?,
            runs: Vec::new(),
        })),
        "table" => Ok(OpenNode::Table(parse_table_node(start)?)),
        "tr" => Ok(OpenNode::Row(RowNode {
            cells: Vec::new(),
            height: parse_optional_length_attr(start, "tr", "data-height-unit", "data-height-mm")?,
        })),
        "td" => Ok(OpenNode::Cell(CellNode {
            col_span: parse_optional_u16_attr(start, "td", "data-col-span")?.unwrap_or(1),
            row_span: parse_optional_u16_attr(start, "td", "data-row-span")?.unwrap_or(1),
            width: parse_length_attr(start, "td", "data-width-unit", "data-width-mm")?,
            background: parse_optional_color_attr(start, "td", "data-background")?,
            line_wrap: parse_line_wrap_attr(start)?,
            paragraphs: Vec::new(),
        })),
        "span" => Ok(OpenNode::Span(SpanNode {
            char_shape_id: parse_index_attr(start, "span", "data-char-shape")?,
            text: String::new(),
        })),
        "a" => Ok(OpenNode::Link(LinkNode {
            char_shape_id: parse_index_attr(start, "a", "data-char-shape")?,
            href: required_attr(start, "a", "href")?,
            text: String::new(),
        })),
        "textbox" | "ellipse" | "polygon" => {
            Ok(OpenNode::Shape(Box::new(parse_shape(start, stack)?)))
        }
        "footnote" => Ok(OpenNode::Footnote(FootnoteNode {
            char_shape_id: parse_index_attr(start, "footnote", "data-char-shape")?,
            text: String::new(),
            paragraphs: Vec::new(),
        })),
        "group" => Ok(OpenNode::Group(Box::new(parse_group(start, stack)?))),
        "img" => {
            let (char_shape_id, image) = parse_picture(start, stack)?;
            Ok(OpenNode::Picture(Box::new(PictureNode { char_shape_id, image })))
        }
        "caption" => Ok(OpenNode::Caption(Box::new(parse_caption(start)?))),
        "control" => Ok(OpenNode::UnknownControl(UnknownControlNode {
            char_shape_id: parse_index_attr(start, "control", "data-char-shape")?,
            kind: required_attr(start, "control", "data-kind")?,
            data: String::new(),
        })),
        other => {
            Err(MdError::LosslessParse { detail: format!("unsupported lossless tag <{}>", other) })
        }
    }
}

fn parse_empty_tag(empty: &BytesStart<'_>, stack: &mut [OpenNode]) -> MdResult<()> {
    let name = empty.name();
    let tag: &str = name.as_ref();
    validate_parent(tag, stack)?;
    match tag {
        "img" => {
            let (char_shape_id, image) = parse_picture(empty, stack)?;
            attach_picture(PictureNode { char_shape_id, image }, stack)
        }
        "group" => {
            let node = parse_group(empty, stack)?;
            attach_group(node, stack)
        }
        "textbox" | "ellipse" | "polygon" | "line" => {
            let node = parse_shape(empty, stack)?;
            attach_shape(node, stack)
        }
        "caption" => {
            let caption = parse_caption(empty)?;
            attach_caption(caption, stack)
        }
        "span" => {
            let char_shape_id = parse_index_attr(empty, "span", "data-char-shape")?;
            push_run_to_parent(stack, Run::text("", char_shape_id))
        }
        "a" => {
            let char_shape_id = parse_index_attr(empty, "a", "data-char-shape")?;
            let href = required_attr(empty, "a", "href")?;
            let link = Control::Hyperlink { text: String::new(), url: href };
            push_run_to_parent(stack, Run::control(link, char_shape_id))
        }
        "p" => {
            let para_shape_id = parse_para_index_attr(empty, "p", "data-para-shape")?;
            let paragraph =
                Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], para_shape_id);
            push_paragraph_to_parent(stack, paragraph)
        }
        other => Err(MdError::LosslessParse {
            detail: format!("unsupported empty lossless tag <{} />", other),
        }),
    }
}

fn validate_parent(tag: &str, stack: &[OpenNode]) -> MdResult<()> {
    let parent = stack.last().map(OpenNode::tag_name);
    let valid = match tag {
        "section" => parent.is_none(),
        "p" => matches!(
            parent,
            Some("section" | "td" | "textbox" | "ellipse" | "polygon" | "footnote" | "caption")
        ),
        "table" => matches!(parent, Some("p")),
        "tr" => matches!(parent, Some("table")),
        "td" => matches!(parent, Some("tr")),
        "span" | "a" | "footnote" | "control" => matches!(parent, Some("p")),
        "img" | "group" | "textbox" | "line" | "ellipse" | "polygon" => {
            matches!(parent, Some("p" | "group"))
        }
        _ => true,
    };

    if valid {
        return Ok(());
    }

    Err(MdError::LosslessParse {
        detail: format!(
            "invalid nesting: <{tag}> cannot be inside <{}>",
            parent.unwrap_or("<root>")
        ),
    })
}

/// 텍스트 안의 `&name;` / `&#NN;` / `&#xNN;` 를 글자 하나로 푼다.
///
/// 미리 정의된 다섯 엔티티와 유효한 숫자 참조만 받고, 나머지는 fail-closed 로 거부한다
/// (엔티티 선언을 읽지 않으므로 알 수 없는 이름을 지어내 치환하지 않는다).
fn resolve_general_ref(reference: &BytesRef<'_>) -> MdResult<char> {
    let name: &str = reference.as_ref();
    let unresolved = || MdError::LosslessParse {
        detail: format!("unresolvable entity or character reference '&{name};'"),
    };
    if reference.is_char_ref() {
        return reference.resolve_char_ref().ok().flatten().ok_or_else(unresolved);
    }
    match name {
        "amp" => Ok('&'),
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        _ => Err(unresolved()),
    }
}

fn append_text(stack: &mut [OpenNode], text: &str) -> MdResult<()> {
    if let Some(node) = stack.last_mut() {
        match node {
            OpenNode::Span(span) => span.text.push_str(text),
            OpenNode::Link(link) => link.text.push_str(text),
            OpenNode::Shape(node) if node.tag == "textbox" => node.text.push_str(text),
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
                vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(10.0)?,
                )])]
            } else {
                table.rows
            };
            let mut core_table =
                Table::new(rows).with_no_adjust(table.no_adjust).with_cell_zones(table.cell_zones);
            core_table.frame = table.frame;
            if let Some(width) = table.width {
                core_table = core_table.with_width(width);
            }
            if let Some(caption) = table.caption {
                core_table = core_table.with_caption(caption);
            }
            push_run_to_parent(stack, Run::table(core_table, table.char_shape_id))
        }
        OpenNode::Row(row) => {
            if let Some(OpenNode::Table(table)) = stack.last_mut() {
                let core_row = match row.height {
                    Some(height) => TableRow::with_height(row.cells, height),
                    None => TableRow::new(row.cells),
                };
                table.rows.push(core_row);
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
                TableCell::with_span(paragraphs, cell.width, cell.col_span, cell.row_span)
                    .with_line_wrap(cell.line_wrap);
            if let Some(background) = cell.background {
                table_cell = table_cell.with_background(background);
            }

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
        OpenNode::Shape(node) => attach_shape(*node, stack),
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
                Run::control(
                    Control::Footnote { inst_id: None, paragraphs },
                    footnote.char_shape_id,
                ),
            )
        }
        OpenNode::Group(group) => attach_group(*group, stack),
        OpenNode::Picture(picture) => attach_picture(*picture, stack),
        OpenNode::Caption(caption) => attach_caption(*caption, stack),
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

/// `<textbox>`, `<line>`, `<ellipse>`, `<polygon>` (P5b W3): CS·BOX·FRAME·
/// PLACE·STYLE, the defining points, VALIGN (shapes with text) and the text
/// margin (text box). A text box's `-mm` sizes are read when its `-unit`
/// ones are missing (an old file).
fn parse_shape(start: &BytesStart<'_>, stack: &[OpenNode]) -> MdResult<ShapeNode> {
    let name = start.name();
    let tag: &'static str = match name.as_ref() {
        "textbox" => "textbox",
        "line" => "line",
        "ellipse" => "ellipse",
        _ => "polygon",
    };
    let mut allowed = g::shape_attributes();
    match tag {
        "line" => allowed.extend(g::START.into_iter().chain(g::END)),
        "ellipse" => allowed.extend(g::CENTER.into_iter().chain(g::AXIS1).chain(g::AXIS2)),
        "polygon" => allowed.push(g::POINTS),
        _ => allowed.extend(g::TEXT_MARGIN),
    }
    if tag != "line" {
        allowed.push(g::TEXT_VALIGN);
    }
    let attrs = Attrs::read(start, tag, &allowed)?;
    let char_shape_id = attrs.char_shape(stack)?;
    let (width, height) = if tag == "textbox" {
        (
            parse_length_attr(start, tag, g::WIDTH, g::BOX_MM[0])?,
            parse_length_attr(start, tag, g::HEIGHT, g::BOX_MM[1])?,
        )
    } else {
        (attrs.unit(g::WIDTH)?, attrs.unit(g::HEIGHT)?)
    };
    let frame = parse_frame_attrs(start, tag, true)?;
    let placement = attrs.placement()?;
    let style = attrs.style()?;
    let valign = match attrs.get(g::TEXT_VALIGN) {
        None => hwpforge_foundation::VerticalAlign::Top,
        Some(_) => attrs.spelled(g::TEXT_VALIGN, g::valign_from)?,
    };
    let point = |names: [&'static str; 2]| -> MdResult<ShapePoint> {
        let coord = |n: &'static str| {
            let v = attrs.required(n)?;
            v.parse::<i32>().map_err(|_| attrs.invalid(n, v))
        };
        Ok(ShapePoint::new(coord(names[0])?, coord(names[1])?))
    };
    // 이것을 실패시키는 것: 네 조건 중 하나를 지우기(`shape_grammar_errors` 의 문단 없는
    // 새 형식 글상자 네 가지).
    let old_form = placement.is_none()
        && style.is_none()
        && attrs.get(g::TEXT_VALIGN).is_none()
        && !attrs.all_or_none(&g::TEXT_MARGIN)?;
    let control = match tag {
        "line" => Control::Line {
            start: point(g::START)?,
            end: point(g::END)?,
            width,
            height,
            frame,
            placement,
            caption: None,
            style,
        },
        "ellipse" => Control::Ellipse {
            center: point(g::CENTER)?,
            axis1: point(g::AXIS1)?,
            axis2: point(g::AXIS2)?,
            width,
            height,
            frame,
            placement,
            paragraphs: Vec::new(),
            caption: None,
            style,
            text_vertical_align: valign,
        },
        "polygon" => Control::Polygon {
            vertices: parse_points(&attrs, g::POINTS)?,
            width,
            height,
            frame,
            placement,
            paragraphs: Vec::new(),
            caption: None,
            style,
            text_vertical_align: valign,
        },
        _ => Control::TextBox {
            paragraphs: Vec::new(),
            width,
            height,
            frame,
            placement,
            caption: None,
            style,
            text_vertical_align: valign,
            text_margin: if attrs.all_or_none(&g::TEXT_MARGIN)? {
                Some(hwpforge_core::table::TableMargin {
                    left: attrs.unit(g::TEXT_MARGIN[0])?,
                    right: attrs.unit(g::TEXT_MARGIN[1])?,
                    top: attrs.unit(g::TEXT_MARGIN[2])?,
                    bottom: attrs.unit(g::TEXT_MARGIN[3])?,
                })
            } else {
                None
            },
        },
    };
    Ok(ShapeNode { tag, char_shape_id, control, text: String::new(), old_form })
}

/// `x,y;x,y…` — an empty value is no points (how many a shape needs is
/// left to Core validation).
/// 이것을 실패시키는 것: 빈 값 거부 · 쌍이 아닌 항목 허용(`shape_grammar_errors`).
fn parse_points(attrs: &Attrs, name: &'static str) -> MdResult<Vec<ShapePoint>> {
    let value = attrs.required(name)?;
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(';')
        .map(|pair| {
            let (x, y) = pair.split_once(',').ok_or_else(|| attrs.invalid(name, value))?;
            Ok(ShapePoint::new(
                x.parse().map_err(|_| attrs.invalid(name, value))?,
                y.parse().map_err(|_| attrs.invalid(name, value))?,
            ))
        })
        .collect()
}

/// Hands a finished shape to its parent. A text box needs `<p>` children —
/// except the old form (no new attribute), whose bare text becomes one
/// paragraph as before; `<p>` mixed with text is refused.
/// 이것을 실패시키는 것: 옛 형식 거부 · `p` 와 텍스트 섞임 허용
/// (`old_text_box_form_reads`, `shape_grammar_errors`).
fn attach_shape(mut node: ShapeNode, stack: &mut [OpenNode]) -> MdResult<()> {
    if let Control::TextBox { paragraphs, .. } = &mut node.control {
        if paragraphs.is_empty() {
            if !node.old_form {
                return Err(MdError::LosslessParse {
                    detail: "<textbox> needs at least one <p>".to_string(),
                });
            }
            paragraphs.push(Paragraph::with_runs(
                vec![Run::text(std::mem::take(&mut node.text), CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ));
        } else if !node.text.trim().is_empty() {
            return Err(MdError::LosslessParse {
                detail: "<textbox> mixes <p> with bare text".to_string(),
            });
        }
    }
    match node.char_shape_id {
        Some(cs) => push_run_to_parent(stack, Run::control(node.control, cs)),
        None => push_group_child(stack, GroupChild::Shape(node.control)),
    }
}

/// Hands a finished picture to its parent: a run under `<p>`, a child under
/// `<group>`.
fn attach_picture(picture: PictureNode, stack: &mut [OpenNode]) -> MdResult<()> {
    match picture.char_shape_id {
        Some(cs) => push_run_to_parent(stack, Run::image(picture.image, cs)),
        None => push_group_child(stack, GroupChild::Picture(picture.image)),
    }
}

/// Hands a finished caption to its `<img>`; a second caption is refused.
/// 이것을 실패시키는 것: 두 번째 caption 거부 삭제(`picture_caption_grammar_errors`).
fn attach_caption(
    caption: hwpforge_core::caption::Caption,
    stack: &mut [OpenNode],
) -> MdResult<()> {
    let Some(OpenNode::Picture(picture)) = stack.last_mut() else {
        return Err(MdError::LosslessParse {
            detail: "<caption> must be inside <img>".to_string(),
        });
    };
    if picture.image.caption.is_some() {
        return Err(MdError::LosslessParse { detail: "<img> has two <caption>s".to_string() });
    }
    picture.image.caption = Some(caption);
    Ok(())
}

/// `<caption>` (P5b W2): side and gap required, width optional (missing =
/// none, kept apart from zero), the full-size flag and saved width.
/// 이것을 실패시키는 것: 측 기본값화 · 너비 없음과 0 혼동
/// (`picture_caption_round_trips`, `picture_caption_grammar_errors`).
fn parse_caption(start: &BytesStart<'_>) -> MdResult<hwpforge_core::caption::Caption> {
    // Its place (inside `<img>`) is checked when it is attached.
    let attrs = Attrs::read(start, "caption", &g::caption_attributes())?;
    let side = attrs.spelled(g::CAPTION_SIDE, g::caption_side_from)?;
    let mut caption = hwpforge_core::caption::Caption::new(Vec::new(), side)
        .with_gap(attrs.unit(g::CAPTION_GAP)?)
        .with_full_size(match attrs.get(g::CAPTION_FULL_SIZE) {
            None => false,
            Some(_) => attrs.flag(g::CAPTION_FULL_SIZE)?,
        })
        .with_last_width(match attrs.get(g::CAPTION_LAST_WIDTH) {
            None => None,
            Some(_) => Some(attrs.unit(g::CAPTION_LAST_WIDTH)?),
        });
    if attrs.get(g::WIDTH).is_some() {
        caption = caption.with_width(attrs.unit(g::WIDTH)?);
    } else if attrs.get(g::BOX_MM[0]).is_some() {
        return Err(MdError::LosslessMissingAttribute { element: "caption", attribute: g::WIDTH });
    }
    Ok(caption)
}

/// Hands a finished group to its parent: a run under `<p>`, a child under
/// `<group>`.
fn attach_group(group: GroupNode, stack: &mut [OpenNode]) -> MdResult<()> {
    match group.char_shape_id {
        Some(cs) => push_run_to_parent(stack, Run::control(group.control, cs)),
        None => push_group_child(stack, GroupChild::Shape(group.control)),
    }
}

fn push_group_child(stack: &mut [OpenNode], child: GroupChild) -> MdResult<()> {
    match stack.last_mut() {
        Some(OpenNode::Group(node)) if matches!(node.control, Control::Group { .. }) => {
            let Control::Group { children, .. } = &mut node.control else { unreachable!() };
            children.push(child);
            Ok(())
        }
        _ => {
            Err(MdError::LosslessParse { detail: "group child must be inside <group>".to_string() })
        }
    }
}

/// The attributes of one object element, read once (P5b closed grammar): a
/// name outside `allowed` or a name met twice is refused before any value
/// is looked at.
struct Attrs {
    element: &'static str,
    values: std::collections::BTreeMap<&'static str, String>,
}

impl Attrs {
    /// 이것을 실패시키는 것: 허용 목록 검사 삭제 · 중복 검사 삭제
    /// (`object_attribute_grammar_errors`).
    fn read(
        start: &BytesStart<'_>,
        element: &'static str,
        allowed: &[&'static str],
    ) -> MdResult<Self> {
        let mut values = std::collections::BTreeMap::new();
        let mut attributes = start.attributes();
        attributes.with_checks(false);
        for attr in attributes {
            let attr = attr.map_err(|err| MdError::LosslessParse {
                detail: format!("attribute decode error: {err}"),
            })?;
            let key: &str = attr.key.as_ref();
            let Some(name) = allowed.iter().copied().find(|n| *n == key) else {
                return Err(MdError::LosslessParse {
                    detail: format!("<{element}> has unknown attribute {key}"),
                });
            };
            let value = attr.normalized_value(XmlVersion::Explicit1_0).map_err(|err| {
                MdError::LosslessParse {
                    detail: format!("attribute value decode error ({name}): {err}"),
                }
            })?;
            if values.insert(name, value.into_owned()).is_some() {
                return Err(MdError::LosslessParse {
                    detail: format!("<{element}> repeats {name}"),
                });
            }
        }
        Ok(Self { element, values })
    }

    fn get(&self, name: &'static str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn required(&self, name: &'static str) -> MdResult<&str> {
        self.get(name)
            .ok_or(MdError::LosslessMissingAttribute { element: self.element, attribute: name })
    }

    fn invalid(&self, name: &'static str, value: &str) -> MdError {
        MdError::LosslessInvalidAttribute {
            element: self.element,
            attribute: name,
            value: value.to_string(),
        }
    }

    fn unit(&self, name: &'static str) -> MdResult<HwpUnit> {
        parse_unit_value(self.element, name, self.required(name)?.to_string())
    }

    fn flag(&self, name: &'static str) -> MdResult<bool> {
        match self.required(name)? {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(self.invalid(name, other)),
        }
    }

    fn spelled<T>(&self, name: &'static str, read: fn(&str) -> Option<T>) -> MdResult<T> {
        let value = self.required(name)?;
        read(value).ok_or_else(|| self.invalid(name, value))
    }

    /// Whether all of `names` are present, none is, or (an error) only some.
    fn all_or_none(&self, names: &[&'static str]) -> MdResult<bool> {
        let present = names.iter().filter(|n| self.values.contains_key(*n)).count();
        if present == 0 {
            return Ok(false);
        }
        if let Some(missing) = names.iter().find(|n| !self.values.contains_key(*n)) {
            return Err(MdError::LosslessMissingAttribute {
                element: self.element,
                attribute: missing,
            });
        }
        Ok(true)
    }

    /// CS: required when the parent is `<p>`, refused inside `<group>`.
    /// 이것을 실패시키는 것: 묶음 자식의 `data-char-shape` 허용
    /// (`object_attribute_grammar_errors`).
    fn char_shape(&self, stack: &[OpenNode]) -> MdResult<Option<CharShapeIndex>> {
        let under_paragraph = matches!(stack.last(), Some(OpenNode::Paragraph(_)));
        match (under_paragraph, self.get(g::CHAR_SHAPE)) {
            (true, Some(v)) => v
                .parse::<usize>()
                .map(|i| Some(CharShapeIndex::new(i)))
                .map_err(|_| self.invalid(g::CHAR_SHAPE, v)),
            (true, None) => Err(MdError::LosslessMissingAttribute {
                element: self.element,
                attribute: g::CHAR_SHAPE,
            }),
            (false, None) => Ok(None),
            (false, Some(_)) => Err(MdError::LosslessParse {
                detail: format!("<{}> inside <group> has data-char-shape", self.element),
            }),
        }
    }

    /// PLACE (all thirteen or none) and OUTMARGIN (all four or none, only
    /// with PLACE).
    /// 이것을 실패시키는 것: PLACE 읽기 삭제 · 일부만 허용 · 모르는 값을 기본값으로 ·
    /// 부호 없는 offset 파싱(`group_round_trips_every_field`,
    /// `object_attribute_grammar_errors`).
    fn placement(&self) -> MdResult<Option<hwpforge_core::placement::ObjectPlacement>> {
        let has_margin = self.all_or_none(&g::OUT_MARGIN)?;
        if !self.all_or_none(&g::PLACE)? {
            if has_margin {
                return Err(MdError::LosslessMissingAttribute {
                    element: self.element,
                    attribute: g::TEXT_WRAP,
                });
            }
            return Ok(None);
        }
        let mut p = hwpforge_core::placement::ObjectPlacement::legacy_inline_defaults();
        p.text_wrap = self.spelled(g::TEXT_WRAP, g::text_wrap_from)?;
        p.text_flow = self.spelled(g::TEXT_FLOW, g::text_flow_from)?;
        p.treat_as_char = self.flag(g::TREAT_AS_CHAR)?;
        p.flow_with_text = self.flag(g::FLOW_WITH_TEXT)?;
        p.allow_overlap = self.flag(g::ALLOW_OVERLAP)?;
        p.vert_rel_to = self.spelled(g::VERT_REL_TO, g::rel_to_from)?;
        p.horz_rel_to = self.spelled(g::HORZ_REL_TO, g::rel_to_from)?;
        p.vert_offset = self.unit(g::VERT_OFFSET)?;
        p.horz_offset = self.unit(g::HORZ_OFFSET)?;
        p.vert_align = self.spelled(g::VERT_ALIGN, g::vert_align_from)?;
        p.horz_align = self.spelled(g::HORZ_ALIGN, g::horz_align_from)?;
        p.affect_line_spacing = self.flag(g::AFFECT_LINE_SPACING)?;
        p.hold_anchor_and_so = self.flag(g::HOLD_ANCHOR)?;
        if has_margin {
            p.out_margin = Some(hwpforge_core::table::TableMargin {
                left: self.unit(g::OUT_MARGIN[0])?,
                right: self.unit(g::OUT_MARGIN[1])?,
                top: self.unit(g::OUT_MARGIN[2])?,
                bottom: self.unit(g::OUT_MARGIN[3])?,
            });
        }
        Ok(Some(p))
    }

    fn optional<T>(
        &self,
        name: &'static str,
        read: impl Fn(&str) -> Option<T>,
    ) -> MdResult<Option<T>> {
        self.get(name).map(|v| read(v).ok_or_else(|| self.invalid(name, v))).transpose()
    }

    fn color(&self, name: &'static str) -> MdResult<Option<Color>> {
        self.get(name).map(|v| g::parse_hex_color(self.element, name, v)).transpose()
    }

    fn arrow(
        &self,
        names: [&'static str; 3],
    ) -> MdResult<Option<hwpforge_core::control::ArrowStyle>> {
        if !self.all_or_none(&names)? {
            return Ok(None);
        }
        Ok(Some(hwpforge_core::control::ArrowStyle {
            arrow_type: self.spelled(names[0], g::arrow_type_from)?,
            size: self.spelled(names[1], g::arrow_size_from)?,
            filled: self.flag(names[2])?,
        }))
    }

    /// STYLE: `None` without the marker (any other STYLE attribute is then
    /// refused); with it, each field from its attribute and the fill from
    /// its kind, whose attributes must be exactly that kind's.
    /// 이것을 실패시키는 것: 표지 없는 속성 허용 · 화살표 일부 허용 · Solid 에 gradient
    /// 섞임 허용 · 빈 stops 거부 · 비유한 rotation 허용 · 부호 있는 `line_width`
    /// (`shape_style_round_trips`, `shape_grammar_errors`).
    fn style(&self) -> MdResult<Option<ShapeStyle>> {
        let others = g::style_attributes();
        match self.get(g::STYLE) {
            None => {
                if let Some(name) = others.iter().find(|n| self.values.contains_key(*n)) {
                    return Err(MdError::LosslessParse {
                        detail: format!("<{}> has {name} without data-style", self.element),
                    });
                }
                return Ok(None);
            }
            Some("true") => {}
            Some(other) => return Err(self.invalid(g::STYLE, other)),
        }
        let mut st = ShapeStyle {
            line_color: self.color(g::LINE_COLOR)?,
            line_width: self.optional(g::LINE_WIDTH, |v| v.parse::<u32>().ok())?,
            line_style: self.optional(g::LINE_STYLE, g::line_style_from)?,
            fill_color: self.color(g::FILL_COLOR)?,
            rotation: self
                .optional(g::ROTATION, |v| v.parse::<f32>().ok().filter(|r| r.is_finite()))?,
            flip: self.optional(g::FLIP, g::flip_from)?,
            head_arrow: self.arrow(g::HEAD_ARROW)?,
            tail_arrow: self.arrow(g::TAIL_ARROW)?,
            ..ShapeStyle::default()
        };
        if let Some(d) = self.optional(g::DROP_CAP, g::drop_cap_from)? {
            st.drop_cap_style = d;
        }
        st.corner_ratio = self.optional(g::CORNER_RATIO, |v| v.parse::<u8>().ok())?;
        let kind = self.get(g::FILL_KIND);
        let (solid, gradient, pattern) = match kind {
            None | Some("None") => (false, false, false),
            Some("Solid") => (true, false, false),
            Some("Gradient") => (false, true, false),
            Some("Pattern") => (false, false, true),
            Some(other) => return Err(self.invalid(g::FILL_KIND, other)),
        };
        let stray = |present: bool, names: &[&'static str]| -> MdResult<()> {
            match names.iter().find(|n| self.values.contains_key(*n)) {
                Some(name) if !present => Err(MdError::LosslessParse {
                    detail: format!("<{}> has {name} for another fill kind", self.element),
                }),
                _ => Ok(()),
            }
        };
        stray(solid, &[g::FILL_SOLID_COLOR])?;
        stray(gradient, &g::GRADIENT)?;
        stray(gradient, &g::GRADIENT_OPTIONAL)?;
        stray(pattern, &g::PATTERN)?;
        st.fill = match kind {
            None => None,
            Some("None") => Some(Fill::None),
            Some("Solid") => Some(Fill::Solid {
                color: g::parse_hex_color(
                    self.element,
                    g::FILL_SOLID_COLOR,
                    self.required(g::FILL_SOLID_COLOR)?,
                )?,
            }),
            Some("Gradient") => {
                let stops = self.required(g::GRADIENT[2])?;
                let colors = if stops.is_empty() {
                    Vec::new()
                } else {
                    stops
                        .split(';')
                        .map(|stop| {
                            let (c, at) = stop
                                .split_once('@')
                                .ok_or_else(|| self.invalid(g::GRADIENT[2], stops))?;
                            Ok((
                                g::parse_hex_color(self.element, g::GRADIENT[2], c)?,
                                at.parse::<u32>()
                                    .map_err(|_| self.invalid(g::GRADIENT[2], stops))?,
                            ))
                        })
                        .collect::<MdResult<Vec<_>>>()?
                };
                let int = |n: &'static str| self.optional(n, |v| v.parse::<i32>().ok());
                let uint = |n: &'static str| self.optional(n, |v| v.parse::<u32>().ok());
                Some(Fill::Gradient {
                    gradient_type: self.spelled(g::GRADIENT[0], g::gradient_type_from)?,
                    angle: {
                        let v = self.required(g::GRADIENT[1])?;
                        v.parse::<i32>().map_err(|_| self.invalid(g::GRADIENT[1], v))?
                    },
                    colors,
                    center_x: int(g::GRADIENT_OPTIONAL[0])?,
                    center_y: int(g::GRADIENT_OPTIONAL[1])?,
                    step: uint(g::GRADIENT_OPTIONAL[2])?,
                    step_center: uint(g::GRADIENT_OPTIONAL[3])?,
                })
            }
            _ => Some(Fill::Pattern {
                pattern_type: self.spelled(g::PATTERN[0], g::pattern_type_from)?,
                fg_color: g::parse_hex_color(
                    self.element,
                    g::PATTERN[1],
                    self.required(g::PATTERN[1])?,
                )?,
                bg_color: g::parse_hex_color(
                    self.element,
                    g::PATTERN[2],
                    self.required(g::PATTERN[2])?,
                )?,
            }),
        };
        Ok(Some(st))
    }

    fn inst_id(&self) -> MdResult<Option<hwpforge_core::ObjectId>> {
        self.get(g::INST_ID)
            .map(|v| {
                v.parse::<u64>()
                    .map(hwpforge_core::ObjectId::new)
                    .map_err(|_| self.invalid(g::INST_ID, v))
            })
            .transpose()
    }
}

/// `<group>` (P5b W1): CS·BOX·FRAME·PLACE·ID.
fn parse_group(start: &BytesStart<'_>, stack: &[OpenNode]) -> MdResult<GroupNode> {
    let attrs = Attrs::read(start, "group", &g::group_attributes())?;
    let char_shape_id = attrs.char_shape(stack)?;
    let control = Control::Group {
        children: Vec::new(),
        width: attrs.unit(g::WIDTH)?,
        height: attrs.unit(g::HEIGHT)?,
        frame: parse_frame_attrs(start, "group", true)?,
        placement: attrs.placement()?,
        inst_id: attrs.inst_id()?,
    };
    Ok(GroupNode { char_shape_id, control })
}

/// `<img>`: the picture under a paragraph (`Some` char shape) or in a
/// group (`None`). An old file's picture without `-unit` sizes reads its
/// `-mm` ones.
fn parse_picture(
    start: &BytesStart<'_>,
    stack: &[OpenNode],
) -> MdResult<(Option<CharShapeIndex>, Image)> {
    let attrs = Attrs::read(start, "img", &g::picture_attributes())?;
    let char_shape_id = attrs.char_shape(stack)?;
    // `alt` repeats the path and is not read (an old file may lack it).
    let src = attrs.required(g::SRC)?.to_string();
    // 이것을 실패시키는 것: 원문 읽기 삭제(`BIN0001` → `bin0001`).
    let format = match attrs.get(g::FORMAT_RAW) {
        Some(raw) => {
            attrs.required(g::FORMAT)?;
            ImageFormat::Unknown(raw.to_string())
        }
        None => parse_image_format(attrs.required(g::FORMAT)?),
    };
    let width = parse_length_attr(start, "img", g::WIDTH, g::BOX_MM[0])?;
    let height = parse_length_attr(start, "img", g::HEIGHT, g::BOX_MM[1])?;
    let mut image = Image::new(src, width, height, format);
    image.frame = parse_frame_attrs(start, "img", true)?;
    image.hyperlink = attrs.get(g::HYPERLINK).map(str::to_string);
    image.placement = attrs.placement()?;
    image.inst_id = attrs.inst_id()?;
    if attrs.all_or_none(&g::CROP)? {
        image.crop = Some(hwpforge_core::image::ImageCrop {
            left: attrs.unit(g::CROP[0])?,
            top: attrs.unit(g::CROP[1])?,
            right: attrs.unit(g::CROP[2])?,
            bottom: attrs.unit(g::CROP[3])?,
        });
    }
    if attrs.all_or_none(&g::EXTENT)? {
        image.image_extent = Some((attrs.unit(g::EXTENT[0])?, attrs.unit(g::EXTENT[1])?));
    }
    Ok((char_shape_id, image))
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
            OpenNode::Shape(node) => match &mut node.control {
                Control::TextBox { paragraphs, .. }
                | Control::Ellipse { paragraphs, .. }
                | Control::Polygon { paragraphs, .. } => {
                    paragraphs.push(paragraph);
                    Ok(())
                }
                _ => Err(MdError::LosslessParse {
                    detail: format!("<p> cannot be nested inside <{}>", node.tag),
                }),
            },
            OpenNode::Footnote(footnote) => {
                footnote.paragraphs.push(paragraph);
                Ok(())
            }
            OpenNode::Caption(caption) => {
                caption.paragraphs.push(paragraph);
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

fn parse_section_node(start: &BytesStart<'_>) -> MdResult<SectionNode> {
    let mut page = PageSettings::a4();
    page.width = parse_length_attr(start, "section", "data-width-unit", "data-width-mm")?;
    page.height = parse_length_attr(start, "section", "data-height-unit", "data-height-mm")?;

    if let Some(v) = parse_optional_length_attr(
        start,
        "section",
        "data-margin-left-unit",
        "data-margin-left-mm",
    )? {
        page.margin_left = v;
    }
    if let Some(v) = parse_optional_length_attr(
        start,
        "section",
        "data-margin-right-unit",
        "data-margin-right-mm",
    )? {
        page.margin_right = v;
    }
    if let Some(v) =
        parse_optional_length_attr(start, "section", "data-margin-top-unit", "data-margin-top-mm")?
    {
        page.margin_top = v;
    }
    if let Some(v) = parse_optional_length_attr(
        start,
        "section",
        "data-margin-bottom-unit",
        "data-margin-bottom-mm",
    )? {
        page.margin_bottom = v;
    }
    if let Some(v) = parse_optional_length_attr(
        start,
        "section",
        "data-header-margin-unit",
        "data-header-margin-mm",
    )? {
        page.header_margin = v;
    }
    if let Some(v) = parse_optional_length_attr(
        start,
        "section",
        "data-footer-margin-unit",
        "data-footer-margin-mm",
    )? {
        page.footer_margin = v;
    }

    Ok(SectionNode { page_settings: page, paragraphs: Vec::new() })
}

fn parse_table_node(start: &BytesStart<'_>) -> MdResult<TableNode> {
    Ok(TableNode {
        frame: parse_frame_attrs(start, "table", false)?,
        char_shape_id: parse_index_attr(start, "table", "data-char-shape")?,
        rows: Vec::new(),
        width: parse_optional_length_attr(start, "table", "data-width-unit", "data-width-mm")?,
        caption: parse_caption_attrs(start)?,
        no_adjust: parse_flag_attr(start, "table", "data-no-adjust")?,
        cell_zones: parse_cell_zones_attr(start)?,
    })
}

/// The value of `attribute`, scanning the whole list once without
/// quick-xml's duplicate check and refusing the name met twice.
fn unique_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<String>> {
    let mut found: Option<String> = None;
    let mut attributes = start.attributes();
    attributes.with_checks(false);
    for attr in attributes {
        let attr = attr.map_err(|err| MdError::LosslessParse {
            detail: format!("attribute decode error: {err}"),
        })?;
        if attr.key.as_ref() != attribute {
            continue;
        }
        if found.is_some() {
            return Err(MdError::LosslessParse {
                detail: format!("<{element}> repeats {attribute}"),
            });
        }
        let value = attr.normalized_value(XmlVersion::Explicit1_0).map_err(|err| {
            MdError::LosslessParse {
                detail: format!("attribute value decode error ({attribute}): {err}"),
            }
        })?;
        found = Some(value.into_owned());
    }
    Ok(found)
}

/// D (Q9): the table caption — `data-caption` (its text; `""` is an empty
/// caption), `data-caption-side` (`top|bottom|left|right`, missing =
/// bottom), `data-caption-full-size` (`"true"`/`"false"`) and
/// `data-caption-last-width-unit` (+ readable `-mm`; `-mm` alone is
/// missing its `-unit`). A caption attribute without `data-caption` is
/// refused. 이것을 실패시키는 것: 측 값 검사 삭제 · `-mm` 단독 허용 · 캡션 없는
/// 캡션 속성 허용(`table_caption_attribute_errors`).
fn parse_caption_attrs(
    start: &BytesStart<'_>,
) -> MdResult<Option<hwpforge_core::caption::Caption>> {
    use hwpforge_core::caption::{Caption, CaptionSide};
    const SIDE: &str = "data-caption-side";
    const FULL: &str = "data-caption-full-size";
    const UNIT: &str = "data-caption-last-width-unit";
    const MM: &str = "data-caption-last-width-mm";
    let text = unique_attr(start, "table", "data-caption")?;
    let side = unique_attr(start, "table", SIDE)?;
    let full_size = parse_flag_attr(start, "table", FULL)?;
    let unit = unique_attr(start, "table", UNIT)?;
    let mm = unique_attr(start, "table", MM)?;
    let Some(text) = text else {
        if side.is_some() || full_size || unit.is_some() || mm.is_some() {
            return Err(MdError::LosslessParse {
                detail: "<table> has caption attributes without data-caption".to_string(),
            });
        }
        return Ok(None);
    };
    let side = match side.as_deref() {
        None | Some("bottom") => CaptionSide::Bottom,
        Some("top") => CaptionSide::Top,
        Some("left") => CaptionSide::Left,
        Some("right") => CaptionSide::Right,
        Some(other) => {
            return Err(MdError::LosslessInvalidAttribute {
                element: "table",
                attribute: SIDE,
                value: other.to_string(),
            })
        }
    };
    let last_width = match (unit, mm) {
        (Some(v), _) => Some(parse_unit_value("table", UNIT, v)?),
        (None, Some(_)) => {
            return Err(MdError::LosslessMissingAttribute { element: "table", attribute: UNIT })
        }
        (None, None) => None,
    };
    Ok(Some(
        Caption::new(
            vec![Paragraph::with_runs(
                vec![Run::text(&text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            side,
        )
        .with_full_size(full_size)
        .with_last_width(last_width),
    ))
}

/// A `"true"`/`"false"` attribute (the FRAME boolean grammar): missing or
/// `"false"` is false, a repeated name is refused, any other value
/// (`"1"`, `"0"` …) is invalid. 이것을 실패시키는 것: 중복 검사 삭제 · `"1"`
/// 을 참으로 받기 · `"false"` 를 참으로(`table_flag_attribute_errors`).
fn parse_flag_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<bool> {
    match unique_attr(start, element, attribute)?.as_deref() {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(other) => {
            Err(MdError::LosslessInvalidAttribute { element, attribute, value: other.to_string() })
        }
    }
}

/// F: `data-cell-zones="r0,c0,r1,c1,bf;…"` — each zone five integers (four
/// `u16` addresses, a `u32` border fill). A missing attribute is no zones; an
/// empty value, a zone without exactly five integers or a value out of range
/// is invalid; a repeat is refused. What a zone means (a start past its end,
/// a range outside the table) is left to Core validation.
/// 이것을 실패시키는 것: 항목 수 검사 삭제 · 행·열 뒤바꿈 · 중복 검사 삭제
/// (`cell_zones_attribute_errors`).
fn parse_cell_zones_attr(start: &BytesStart<'_>) -> MdResult<Vec<hwpforge_core::table::CellZone>> {
    const ATTR: &str = "data-cell-zones";
    let Some(value) = unique_attr(start, "table", ATTR)? else {
        return Ok(Vec::new());
    };
    let invalid = || MdError::LosslessInvalidAttribute {
        element: "table",
        attribute: ATTR,
        value: value.clone(),
    };
    value
        .split(';')
        .map(|zone| {
            let parts: Vec<&str> = zone.split(',').collect();
            let [r0, c0, r1, c1, bf] = parts[..] else { return Err(invalid()) };
            let addr = |v: &str| v.parse::<u16>().map_err(|_| invalid());
            Ok(hwpforge_core::table::CellZone::new(
                addr(r0)?,
                addr(c0)?,
                addr(r1)?,
                addr(c1)?,
                bf.parse::<u32>().map_err(|_| invalid())?,
            ))
        })
        .collect()
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

/// The FRAME attributes (S20, design §19.23.21), in a fixed order.
const FRAME_ATTRIBUTES: [&str; 5] = [
    "data-original-width-unit",
    "data-original-height-unit",
    "data-original-width-mm",
    "data-original-height-mm",
    "data-size-protect",
];

/// Reads an element's object frame (S20, design §19.23.21). The whole
/// attribute list is scanned once — without quick-xml's duplicate check, so
/// the policy for the other attributes stays as it is — and a FRAME name met
/// twice is refused. The original box needs both `-unit` attributes (the
/// `-mm` ones are for reading only and ignored); `original` = whether the
/// element may have one (a table may not). What a value means (a negative
/// axis, a zero axis under a shown one) is left to Core validation.
/// 이것을 실패시키는 것: 중복 검사 / 쌍 규칙 / 표의 원래 크기 거부 / `"false"`
/// 처리 중 하나를 지우기(`frame_attribute_errors`).
fn parse_frame_attrs(
    start: &BytesStart<'_>,
    element: &'static str,
    original: bool,
) -> MdResult<hwpforge_core::ObjectFrame> {
    let mut found: [Option<String>; 5] = Default::default();
    let mut attributes = start.attributes();
    attributes.with_checks(false);
    for attr in attributes {
        let attr = attr.map_err(|err| MdError::LosslessParse {
            detail: format!("attribute decode error: {err}"),
        })?;
        let Some(index) = FRAME_ATTRIBUTES.iter().position(|n| attr.key.as_ref() == *n) else {
            continue;
        };
        if found[index].is_some() {
            return Err(MdError::LosslessParse {
                detail: format!("<{element}> repeats {}", FRAME_ATTRIBUTES[index]),
            });
        }
        let value = attr.normalized_value(XmlVersion::Explicit1_0).map_err(|err| {
            MdError::LosslessParse {
                detail: format!(
                    "attribute value decode error ({}): {err}",
                    FRAME_ATTRIBUTES[index]
                ),
            }
        })?;
        found[index] = Some(value.into_owned());
    }
    let [width, height, width_mm, height_mm, protect] = found;
    if !original
        && (width.is_some() || height.is_some() || width_mm.is_some() || height_mm.is_some())
    {
        return Err(MdError::LosslessParse { detail: format!("<{element}> has no original size") });
    }
    let missing = |attribute| MdError::LosslessMissingAttribute { element, attribute };
    let original_size = match (width, height) {
        (Some(w), Some(h)) => Some(hwpforge_foundation::Size::new(
            parse_unit_value(element, FRAME_ATTRIBUTES[0], w)?,
            parse_unit_value(element, FRAME_ATTRIBUTES[1], h)?,
        )),
        (Some(_), None) => return Err(missing(FRAME_ATTRIBUTES[1])),
        (None, Some(_)) => return Err(missing(FRAME_ATTRIBUTES[0])),
        (None, None) if width_mm.is_some() => return Err(missing(FRAME_ATTRIBUTES[0])),
        (None, None) if height_mm.is_some() => return Err(missing(FRAME_ATTRIBUTES[1])),
        (None, None) => None,
    };
    let size_protect = match protect.as_deref() {
        None | Some("false") => false,
        Some("true") => true,
        Some(other) => {
            return Err(MdError::LosslessInvalidAttribute {
                element,
                attribute: FRAME_ATTRIBUTES[4],
                value: other.to_string(),
            })
        }
    };
    let frame = hwpforge_core::ObjectFrame::new().with_size_protect(size_protect);
    Ok(match original_size {
        Some(o) => frame.with_original_size(o),
        None => frame,
    })
}

fn required_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<String> {
    attr_value(start, attribute)?.ok_or(MdError::LosslessMissingAttribute { element, attribute })
}

fn attr_value(start: &BytesStart<'_>, attribute: &'static str) -> MdResult<Option<String>> {
    for attr in start.attributes() {
        let attr = attr.map_err(|err| MdError::LosslessParse {
            detail: format!("attribute decode error: {err}"),
        })?;

        if attr.key.as_ref() == attribute {
            let value = attr.normalized_value(XmlVersion::Explicit1_0).map_err(|err| {
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
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<CharShapeIndex> {
    let value = required_attr(start, element, attribute)?;
    let idx = value.parse::<usize>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;
    Ok(CharShapeIndex::new(idx))
}

fn parse_para_index_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<ParaShapeIndex> {
    let value = required_attr(start, element, attribute)?;
    let idx = value.parse::<usize>().map_err(|_| MdError::LosslessInvalidAttribute {
        element,
        attribute,
        value: value.clone(),
    })?;
    Ok(ParaShapeIndex::new(idx))
}

fn parse_mm_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<HwpUnit> {
    let value = required_attr(start, element, attribute)?;
    parse_mm_value(element, attribute, value)
}

fn parse_length_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    unit_attribute: &'static str,
    mm_attribute: &'static str,
) -> MdResult<HwpUnit> {
    if let Some(value) = attr_value(start, unit_attribute)? {
        return parse_unit_value(element, unit_attribute, value);
    }
    parse_mm_attr(start, element, mm_attribute)
}

fn parse_optional_length_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    unit_attribute: &'static str,
    mm_attribute: &'static str,
) -> MdResult<Option<HwpUnit>> {
    if let Some(value) = attr_value(start, unit_attribute)? {
        return Ok(Some(parse_unit_value(element, unit_attribute, value)?));
    }
    parse_optional_mm_attr(start, element, mm_attribute)
}

fn parse_optional_mm_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<HwpUnit>> {
    match attr_value(start, attribute)? {
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
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<u16>> {
    match attr_value(start, attribute)? {
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

/// §19.22 D3: `data-line-wrap` (`SQUEEZE`, `KEEP`); missing = the default
/// break. `BREAK` itself is not a written value and is refused like any
/// other unknown spelling; a repeated name is refused.
/// 이것을 실패시키는 것: 읽기 삭제 · 모르는 값을 기본값으로
/// (`cell_line_wrap_round_trips`, `cell_line_wrap_attribute_errors`). The
/// repeat is refused here; quick-xml's own duplicate check also catches it
/// whenever another `<td>` lookup scans the whole list (one for an absent
/// attribute does), so dropping this scan alone still refuses the repeat in
/// `cell_line_wrap_attribute_errors`.
fn parse_line_wrap_attr(start: &BytesStart<'_>) -> MdResult<hwpforge_core::table::CellLineWrap> {
    match unique_attr(start, "td", g::LINE_WRAP)? {
        None => Ok(hwpforge_core::table::CellLineWrap::Break),
        Some(v) => g::line_wrap_from(&v).ok_or(MdError::LosslessInvalidAttribute {
            element: "td",
            attribute: g::LINE_WRAP,
            value: v,
        }),
    }
}

fn parse_optional_color_attr(
    start: &BytesStart<'_>,
    element: &'static str,
    attribute: &'static str,
) -> MdResult<Option<Color>> {
    attr_value(start, attribute)?.map(|v| g::parse_hex_color(element, attribute, &v)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::RunContent;

    fn section_wrap(inner: &str) -> String {
        let mut s = String::from(
            "<section data-index=\"0\" data-width-mm=\"210.0\" data-height-mm=\"297.0\">",
        );
        s.push_str(inner);
        s.push_str("</section>");
        s
    }

    // ── S20 FRAME (design §19.23.21) ──

    const IMG: &str = r#"data-char-shape="0" src="a.png" alt="a.png" data-format="PNG" data-width-unit="3000" data-height-unit="2000""#;

    fn img(frame: &str) -> MdResult<Vec<Section>> {
        decode_lossless_sections(&section_wrap(&format!(
            r#"<p data-para-shape="0"><img {IMG} {frame} /></p>"#
        )))
    }

    fn picture_frame(frame: &str) -> hwpforge_core::ObjectFrame {
        let sections = img(frame).unwrap();
        match &sections[0].paragraphs[0].runs[0].content {
            RunContent::Image(i) => i.frame,
            other => panic!("{other:?}"),
        }
    }

    /// The grammar stage of FRAME: duplicates (first, middle, last place,
    /// same value too), half a pair, `-mm` without `-unit`, a boolean other
    /// than true/false, a value past HWPUNIT, and an original box on a table
    /// are refused; `"false"` is no lock, the `-mm` values are ignored, and
    /// 100,000,000 is in range.
    #[test]
    fn frame_attribute_errors() {
        let w = r#"data-original-width-unit="10""#;
        let h = r#"data-original-height-unit="20""#;
        let p = r#"data-size-protect="true""#;
        for dup in [format!("{p} {w} {h} {p}"), format!("{w} {p} {p} {h}"), format!("{w} {h} {w}")]
        {
            // The FRAME scan names the repeat itself — quick-xml's own check
            // is not relied on (a lookup may stop before reaching it).
            assert!(
                matches!(img(&dup), Err(MdError::LosslessParse { detail }) if detail.contains("repeats")),
                "{dup}"
            );
        }
        assert!(matches!(img(w), Err(MdError::LosslessMissingAttribute { .. })));
        assert!(matches!(img(h), Err(MdError::LosslessMissingAttribute { .. })));
        assert!(matches!(
            img(r#"data-original-width-mm="1.0" data-original-height-mm="2.0""#),
            Err(MdError::LosslessMissingAttribute { .. })
        ));
        assert!(matches!(
            img(r#"data-size-protect="yes""#),
            Err(MdError::LosslessInvalidAttribute { .. })
        ));
        assert!(matches!(
            img(r#"data-original-width-unit="100000001" data-original-height-unit="20""#),
            Err(MdError::LosslessInvalidAttribute { .. })
        ));
        let u = |v| HwpUnit::new(v).unwrap();
        let f = picture_frame(
            r#"data-original-width-unit="100000000" data-original-height-unit="20" data-original-width-mm="1.00" data-original-height-mm="999.00" data-size-protect="false""#,
        );
        assert_eq!(f.original_size, Some(hwpforge_foundation::Size::new(u(100_000_000), u(20))));
        assert!(!f.size_protect);
        assert!(picture_frame(p).size_protect);
        assert!(picture_frame("").is_unset(), "an old file without FRAME");

        let table = |attrs: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><table data-char-shape="0" {attrs}><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000"><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table></p>"#
            )))
        };
        for attrs in [format!("{w} {h}"), r#"data-original-width-mm="1.0""#.to_string()] {
            assert!(matches!(table(&attrs), Err(MdError::LosslessParse { .. })), "{attrs}");
        }
        assert!(table(p).is_ok());
    }

    /// D (Q9): caption attribute errors — a side other than the four, a
    /// full-size flag other than `"true"`/`"false"`, `-mm` without `-unit`,
    /// caption attributes without `data-caption`, a repeated side.
    /// 이것을 실패시키는 것: 측 값 검사 · 플래그 검사 · `-mm` 단독 허용 · 캡션
    /// 없는 속성 허용 · 중복 검사 각각 삭제.
    #[test]
    fn table_caption_attribute_errors() {
        let table = |attrs: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><table data-char-shape="0" {attrs}><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000"><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table></p>"#
            )))
        };
        assert!(matches!(
            table(r#"data-caption="c" data-caption-side="middle""#),
            Err(MdError::LosslessInvalidAttribute { attribute: "data-caption-side", .. })
        ));
        assert!(matches!(
            table(r#"data-caption="c" data-caption-full-size="1""#),
            Err(MdError::LosslessInvalidAttribute { attribute: "data-caption-full-size", .. })
        ));
        assert!(matches!(
            table(r#"data-caption="c" data-caption-last-width-mm="10.00""#),
            Err(MdError::LosslessMissingAttribute {
                attribute: "data-caption-last-width-unit",
                ..
            })
        ));
        assert!(matches!(table(r#"data-caption-side="top""#), Err(MdError::LosslessParse { .. })));
        assert!(matches!(
            table(
                r#"data-width-unit="9000" data-width-mm="31.75" data-caption="c" data-caption-side="top" data-caption-side="top""#
            ),
            Err(MdError::LosslessParse { .. })
        ));
        let sections = table(r#"data-caption="" data-caption-side="top""#).unwrap();
        let RunContent::Table(t) = &sections[0].paragraphs[0].runs[0].content else { panic!() };
        assert_eq!(t.caption.as_ref().unwrap().side, hwpforge_core::caption::CaptionSide::Top);
    }

    /// §19.22 D3: an unknown or default spelling, or a repeat, of
    /// `data-line-wrap` is refused.
    #[test]
    fn cell_line_wrap_attribute_errors() {
        let td = |attrs: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000" {attrs}><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table></p>"#
            )))
        };
        for v in ["squeeze", "BREAK", ""] {
            assert!(
                matches!(
                    td(&format!(r#"data-line-wrap="{v}""#)),
                    Err(MdError::LosslessInvalidAttribute { attribute: "data-line-wrap", .. })
                ),
                "{v}"
            );
        }
        assert!(matches!(
            td(r#"data-line-wrap="KEEP" data-line-wrap="KEEP""#),
            Err(MdError::LosslessParse { .. })
        ));
        assert!(td(r#"data-line-wrap="KEEP""#).is_ok());
    }

    /// F: `data-cell-zones` — each zone exactly five integers in range; an
    /// empty value or a bad zone is invalid, a repeat refused, a missing one
    /// is no zones. Meaning (a reversed range) is left to validation.
    /// 이것을 실패시키는 것: 항목 수 검사 삭제 · 범위 검사 삭제 · 중복 검사 삭제 ·
    /// 빈 값을 빈 목록으로.
    #[test]
    fn cell_zones_attribute_errors() {
        let table = |attrs: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><table data-char-shape="0" {attrs}><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000"><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table></p>"#
            )))
        };
        let zones = |attrs: &str| {
            let sections = table(attrs).unwrap();
            let RunContent::Table(t) = &sections[0].paragraphs[0].runs[0].content else { panic!() };
            t.cell_zones.clone()
        };
        use hwpforge_core::table::CellZone;
        assert!(zones("").is_empty());
        assert_eq!(
            zones(r#"data-cell-zones="1,0,0,0,9;0,0,0,0,4294967295""#),
            [CellZone::new(1, 0, 0, 0, 9), CellZone::new(0, 0, 0, 0, u32::MAX)]
        );
        for value in
            ["", "0,0,0,0", "0,0,0,0,1,2", "0,0,0,65536,1", "0,0,0,0,-1", "0,0,0,0,1;", "a,0,0,0,1"]
        {
            assert!(
                matches!(
                    table(&format!(r#"data-cell-zones="{value}""#)),
                    Err(MdError::LosslessInvalidAttribute { attribute: "data-cell-zones", .. })
                ),
                "{value}"
            );
        }
        assert!(matches!(
            table(
                r#"data-width-unit="9000" data-width-mm="31.75" data-caption="c" data-cell-zones="0,0,0,0,1" data-cell-zones="0,0,0,0,1""#
            ),
            Err(MdError::LosslessParse { .. })
        ));
    }

    /// `data-no-adjust` reads only `"true"`/`"false"`; a repeat is refused.
    /// 이것을 실패시키는 것: 중복 검사 삭제 · `"1"`·`"0"` 허용 · `"false"` 를
    /// 참으로.
    #[test]
    fn table_flag_attribute_errors() {
        let table = |attrs: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><table data-char-shape="0" {attrs}><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000"><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table></p>"#
            )))
        };
        let no_adjust = |attrs: &str| {
            let sections = table(attrs).unwrap();
            let RunContent::Table(t) = &sections[0].paragraphs[0].runs[0].content else { panic!() };
            t.no_adjust
        };
        assert!(no_adjust(r#"data-no-adjust="true""#));
        assert!(!no_adjust(r#"data-no-adjust="false""#));
        assert!(!no_adjust(""));
        for value in ["1", "0", "yes"] {
            assert!(
                matches!(
                    table(&format!(r#"data-no-adjust="{value}""#)),
                    Err(MdError::LosslessInvalidAttribute { attribute: "data-no-adjust", .. })
                ),
                "{value}"
            );
        }
        // Every other attribute found before the repeat, so no other lookup
        // reaches it.
        assert!(matches!(
            table(
                r#"data-width-unit="9000" data-width-mm="31.75" data-caption="c" data-no-adjust="true" data-no-adjust="true""#
            ),
            Err(MdError::LosslessParse { .. })
        ));
    }

    /// The meaning stage: a negative axis, or an original axis of 0 under a
    /// shown one, decodes to a Draft that Core validation refuses — for a
    /// picture and for a text box.
    #[test]
    fn frame_meaning_is_left_to_validation() {
        let text_box = |frame: &str| {
            decode_lossless_sections(&section_wrap(&format!(
                r#"<p data-para-shape="0"><textbox data-char-shape="0" data-width-unit="3000" data-height-unit="2000" {frame}><p data-para-shape="0"><span data-char-shape="0">t</span></p></textbox></p>"#
            )))
        };
        for frame in [
            r#"data-original-width-unit="-5" data-original-height-unit="20""#,
            r#"data-original-width-unit="0" data-original-height-unit="20""#,
        ]
        .into_iter()
        .flat_map(|f| [img(f), text_box(f)])
        {
            let sections = frame.unwrap();
            let mut doc = hwpforge_core::Document::new();
            for s in sections {
                doc.add_section(s);
            }
            assert!(matches!(
                doc.validate(),
                Err(hwpforge_core::error::CoreError::Validation(
                    hwpforge_core::ValidationError::InvalidObjectFrame { .. }
                ))
            ));
        }
    }

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
    fn decode_multiple_sections() {
        let input = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0"><span data-char-shape="0">A</span></p></section><section data-index="1" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0"><span data-char-shape="0">B</span></p></section>"#;
        let sections = decode_lossless_sections(input).unwrap();
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn decode_empty_section_gets_default_paragraph() {
        let input =
            r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"></section>"#;
        let sections = decode_lossless_sections(input).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].paragraphs.len(), 1);
    }

    #[test]
    fn decode_section_page_settings_preserved() {
        let input = r#"<section data-index="0" data-width-mm="297.0" data-height-mm="210.0" data-margin-left-mm="20.0" data-margin-right-mm="15.0" data-margin-top-mm="25.0" data-margin-bottom-mm="30.0" data-header-margin-mm="12.0" data-footer-margin-mm="11.0"></section>"#;
        let sections = decode_lossless_sections(input).unwrap();
        let page = &sections[0].page_settings;
        // Width ~297mm in HWPUNIT
        assert!((page.width.to_mm() - 297.0).abs() < 0.5);
        assert!((page.height.to_mm() - 210.0).abs() < 0.5);
        assert!((page.margin_left.to_mm() - 20.0).abs() < 0.5);
        assert!((page.margin_right.to_mm() - 15.0).abs() < 0.5);
        assert!((page.margin_top.to_mm() - 25.0).abs() < 0.5);
        assert!((page.margin_bottom.to_mm() - 30.0).abs() < 0.5);
        assert!((page.header_margin.to_mm() - 12.0).abs() < 0.5);
        assert!((page.footer_margin.to_mm() - 11.0).abs() < 0.5);
    }

    #[test]
    fn decode_section_page_settings_via_unit_attr() {
        // data-width-unit takes priority over data-width-mm
        let input = r#"<section data-index="0" data-width-unit="59528" data-height-unit="84188" data-width-mm="999.0" data-height-mm="999.0"></section>"#;
        let sections = decode_lossless_sections(input).unwrap();
        let page = &sections[0].page_settings;
        assert_eq!(page.width.as_i32(), 59528);
        assert_eq!(page.height.as_i32(), 84188);
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
    fn decode_table_with_width_and_caption() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><table data-char-shape="0" data-width-mm="120.0" data-caption="표 1"><tr><td data-width-mm="60.0"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        assert!(table.width.is_some());
        assert!((table.width.unwrap().to_mm() - 120.0).abs() < 0.5);
        assert!(table.caption.is_some());
    }

    #[test]
    fn decode_table_with_row_height() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><table data-char-shape="0"><tr data-height-mm="15.0"><td data-width-mm="40.0"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        let row_height = table.rows[0].height;
        assert!(row_height.is_some());
        assert!((row_height.unwrap().to_mm() - 15.0).abs() < 0.5);
    }

    #[test]
    fn decode_table_cell_with_background_color() {
        let input = section_wrap(
            r##"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-width-mm="40.0" data-background="#FF0000"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"##,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        let bg = table.rows[0].cells[0].background;
        assert!(bg.is_some());
    }

    #[test]
    fn decode_table_cell_with_span() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-col-span="3" data-row-span="2" data-width-mm="60.0"><p data-para-shape="0"><span data-char-shape="0">merged</span></p></td></tr></table></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        let cell = &table.rows[0].cells[0];
        assert_eq!(cell.col_span, 3);
        assert_eq!(cell.row_span, 2);
    }

    #[test]
    fn decode_empty_table_gets_default_row() {
        let input =
            section_wrap(r#"<p data-para-shape="0"><table data-char-shape="0"></table></p>"#);
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 1);
    }

    #[test]
    fn decode_image_png() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><img data-char-shape="1" src="image.png" data-format="PNG" data-width-mm="50.0" data-height-mm="30.0" /></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        assert!(matches!(run.content, RunContent::Image(_)));
        if let RunContent::Image(ref img) = run.content {
            assert_eq!(img.path, "image.png");
            assert_eq!(img.format, ImageFormat::Png);
            assert!((img.width.to_mm() - 50.0).abs() < 0.5);
            assert!((img.height.to_mm() - 30.0).abs() < 0.5);
        }
    }

    #[test]
    fn decode_image_jpeg_format_variants() {
        for fmt in &["JPEG", "JPG", "jpeg", "jpg"] {
            let input = section_wrap(&format!(
                r#"<p data-para-shape="0"><img data-char-shape="0" src="img.jpg" data-format="{fmt}" data-width-mm="10.0" data-height-mm="10.0" /></p>"#
            ));
            let sections = decode_lossless_sections(&input).unwrap();
            let run = &sections[0].paragraphs[0].runs[0];
            if let RunContent::Image(ref img) = run.content {
                assert_eq!(img.format, ImageFormat::Jpeg, "format '{fmt}' should be Jpeg");
            }
        }
    }

    #[test]
    fn decode_image_unknown_format() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><img data-char-shape="0" src="img.abc" data-format="abc" data-width-mm="10.0" data-height-mm="10.0" /></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Image(ref img) = run.content {
            assert_eq!(img.format, ImageFormat::Unknown("abc".to_string()));
        }
    }

    #[test]
    fn decode_hyperlink() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><a data-char-shape="2" href="https://rust-lang.org">Rust</a></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            assert!(
                matches!(ctrl.as_ref(), Control::Hyperlink { text, url } if text == "Rust" && url == "https://rust-lang.org")
            );
        } else {
            panic!("expected hyperlink control");
        }
    }

    #[test]
    fn decode_empty_hyperlink_tag() {
        // Self-closing <a /> with no text content
        let input = section_wrap(
            r#"<p data-para-shape="0"><a data-char-shape="0" href="https://example.com" /></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            assert!(matches!(ctrl.as_ref(), Control::Hyperlink { text, .. } if text.is_empty()));
        } else {
            panic!("expected hyperlink control");
        }
    }

    #[test]
    fn decode_empty_span_tag() {
        // Self-closing <span /> becomes empty text run
        let input = section_wrap(r#"<p data-para-shape="0"><span data-char-shape="3" /></p>"#);
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        assert_eq!(run.char_shape_id.get(), 3);
        assert!(matches!(run.content, RunContent::Text(ref t) if t.is_empty()));
    }

    #[test]
    fn decode_empty_paragraph_tag() {
        // Self-closing <p /> creates a paragraph with empty text run
        let input = section_wrap(r#"<p data-para-shape="5" />"#);
        let sections = decode_lossless_sections(&input).unwrap();
        assert_eq!(sections[0].paragraphs.len(), 1);
        assert_eq!(sections[0].paragraphs[0].para_shape_id.get(), 5);
    }

    #[test]
    fn decode_textbox_with_nested_paragraphs() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><textbox data-char-shape="2" data-width-mm="80.0" data-height-mm="40.0"><p data-para-shape="1"><span data-char-shape="3">inside</span></p></textbox></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            assert!(ctrl.is_text_box());
            if let Control::TextBox { paragraphs, width, height, .. } = ctrl.as_ref() {
                assert_eq!(paragraphs.len(), 1);
                assert!((width.to_mm() - 80.0).abs() < 0.5);
                assert!((height.to_mm() - 40.0).abs() < 0.5);
                // Nested paragraph should have the correct para_shape_id
                assert_eq!(paragraphs[0].para_shape_id.get(), 1);
            }
        } else {
            panic!("expected textbox control");
        }
    }

    #[test]
    fn decode_textbox_simple_text() {
        // textbox with only text content (no nested <p>)
        let input = section_wrap(
            r#"<p data-para-shape="0"><textbox data-char-shape="1" data-width-mm="50.0" data-height-mm="20.0">plain text</textbox></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            if let Control::TextBox { paragraphs, .. } = ctrl.as_ref() {
                // text goes into a default paragraph
                let text = paragraphs[0].runs[0].content.as_text().unwrap_or("");
                assert_eq!(text, "plain text");
            }
        } else {
            panic!("expected textbox control");
        }
    }

    #[test]
    fn decode_footnote_with_nested_paragraphs() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><footnote data-char-shape="4"><p data-para-shape="2"><span data-char-shape="5">note text</span></p></footnote></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            if let Control::Footnote { paragraphs, .. } = ctrl.as_ref() {
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(paragraphs[0].para_shape_id.get(), 2);
                let text = paragraphs[0].runs[0].content.as_text().unwrap_or("");
                assert_eq!(text, "note text");
            } else {
                panic!("expected Footnote, got other control");
            }
        } else {
            panic!("expected control run");
        }
    }

    #[test]
    fn decode_footnote_simple_text() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><footnote data-char-shape="1">note body</footnote></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            if let Control::Footnote { paragraphs, .. } = ctrl.as_ref() {
                let text = paragraphs[0].runs[0].content.as_text().unwrap_or("");
                assert_eq!(text, "note body");
            } else {
                panic!("expected Footnote control");
            }
        } else {
            panic!("expected Control run");
        }
    }

    #[test]
    fn decode_unknown_control() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><control data-char-shape="0" data-kind="mystery">some data</control></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            if let Control::Unknown { tag, data } = ctrl.as_ref() {
                assert_eq!(tag, "mystery");
                assert_eq!(data.as_deref(), Some("some data"));
            } else {
                panic!("expected Unknown control");
            }
        }
    }

    #[test]
    fn decode_unknown_control_empty_data() {
        // Empty control has None data
        let input = section_wrap(
            r#"<p data-para-shape="0"><control data-char-shape="0" data-kind="empty"></control></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let run = &sections[0].paragraphs[0].runs[0];
        if let RunContent::Control(ctrl) = &run.content {
            if let Control::Unknown { data, .. } = ctrl.as_ref() {
                assert!(data.is_none());
            } else {
                panic!("expected Unknown control");
            }
        } else {
            panic!("expected Control run");
        }
    }

    #[test]
    fn decode_invalid_tag_fails() {
        let input = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><unknown /></section>"#;
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_unsupported_start_tag_fails() {
        let input = section_wrap(r#"<div data-something="1"></div>"#);
        let err = decode_lossless_sections(&input).unwrap_err();
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

    #[test]
    fn decode_table_outside_p_fails() {
        // table must be inside <p>
        let input = section_wrap(r#"<table data-char-shape="0"></table>"#);
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_span_outside_p_fails() {
        let input = section_wrap(r#"<span data-char-shape="0">text</span>"#);
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_unexpected_root_text_fails() {
        // Text at root level (not inside any section/p) should fail
        let input = "loose text without any tags";
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_missing_href_on_anchor_fails() {
        let input =
            section_wrap(r#"<p data-para-shape="0"><a data-char-shape="0">no href</a></p>"#);
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessMissingAttribute { .. }));
    }

    #[test]
    fn decode_missing_char_shape_fails() {
        let input = section_wrap(r#"<p data-para-shape="0"><span>no char-shape</span></p>"#);
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessMissingAttribute { .. }));
    }

    #[test]
    fn decode_invalid_char_shape_index_fails() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><span data-char-shape="not_a_number">text</span></p>"#,
        );
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessInvalidAttribute { .. }));
    }

    #[test]
    fn decode_invalid_para_shape_index_fails() {
        let input = section_wrap(
            r#"<p data-para-shape="bad_value"><span data-char-shape="0">text</span></p>"#,
        );
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessInvalidAttribute { .. }));
    }

    #[test]
    fn decode_invalid_color_wrong_length_fails() {
        // Background color with wrong hex length
        let input = section_wrap(
            r##"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-width-mm="40.0" data-background="#FFF"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"##,
        );
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessInvalidAttribute { .. }));
    }

    #[test]
    fn decode_invalid_color_bad_hex_fails() {
        let input = section_wrap(
            r##"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-width-mm="40.0" data-background="#GGGGGG"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"##,
        );
        let err = decode_lossless_sections(&input).unwrap_err();
        assert!(matches!(err, MdError::LosslessInvalidAttribute { .. }));
    }

    #[test]
    fn decode_color_without_hash_prefix() {
        // Color parsing should accept both "#RRGGBB" and "RRGGBB"
        let input = section_wrap(
            r#"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-width-mm="40.0" data-background="FF0000"><p data-para-shape="0"><span data-char-shape="0">A</span></p></td></tr></table></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let bg = sections[0].paragraphs[0].runs[0].content.as_table().unwrap().rows[0].cells[0]
            .background;
        assert!(bg.is_some());
    }

    #[test]
    fn decode_invalid_width_mm_fails() {
        let input = r#"<section data-index="0" data-width-mm="not_a_number" data-height-mm="297.0"></section>"#;
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessInvalidAttribute { .. }));
    }

    #[test]
    fn decode_unclosed_tag_fails() {
        let input = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0">"#;
        let err = decode_lossless_sections(input).unwrap_err();
        assert!(matches!(err, MdError::LosslessParse { .. }));
    }

    #[test]
    fn decode_tag_mismatch_fails() {
        // Open <p> but close </span>
        let input = r#"<hwpforge-lossless-root><section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0"></span></section></hwpforge-lossless-root>"#;
        // This should fail due to tag mismatch
        let result = decode_lossless_sections(input);
        assert!(result.is_err(), "tag mismatch should produce an error");
    }

    #[test]
    fn decode_image_all_known_formats() {
        let cases = [
            ("GIF", ImageFormat::Gif),
            ("BMP", ImageFormat::Bmp),
            ("WMF", ImageFormat::Wmf),
            ("EMF", ImageFormat::Emf),
        ];
        for (fmt_str, expected) in &cases {
            let result = parse_image_format(fmt_str);
            assert_eq!(result, *expected, "format '{fmt_str}' mismatch");
        }
    }

    #[test]
    fn decode_paragraph_text_with_whitespace_nodes_ok() {
        // Whitespace between tags at the section level is allowed
        let input = "<section data-index=\"0\" data-width-mm=\"210.0\" data-height-mm=\"297.0\">\n  <p data-para-shape=\"0\"><span data-char-shape=\"0\">text</span></p>\n</section>";
        let sections = decode_lossless_sections(input).unwrap();
        assert_eq!(sections.len(), 1);
    }

    #[test]
    fn decode_table_empty_cell_gets_default_paragraph() {
        let input = section_wrap(
            r#"<p data-para-shape="0"><table data-char-shape="0"><tr><td data-width-mm="40.0"></td></tr></table></p>"#,
        );
        let sections = decode_lossless_sections(&input).unwrap();
        let table = sections[0].paragraphs[0].runs[0].content.as_table().unwrap();
        // Empty cell should get a default paragraph
        assert_eq!(table.rows[0].cells[0].paragraphs.len(), 1);
    }

    #[test]
    fn decode_paragraph_with_empty_runs_gets_default_run() {
        let input = section_wrap(r#"<p data-para-shape="0"></p>"#);
        let sections = decode_lossless_sections(&input).unwrap();
        // Empty paragraph should get a default empty run
        assert_eq!(sections[0].paragraphs[0].runs.len(), 1);
    }
}
