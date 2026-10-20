//! Lossless markdown encoder (frontmatter + HTML-like body).

use hwpforge_core::control::{Fill, GroupChild, ShapePoint, ShapeStyle};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::placement::ObjectPlacement;
use hwpforge_core::{Control, Document, Paragraph, RunContent, Table, Validated};

use crate::error::{MdError, MdResult};
use crate::frontmatter::{from_metadata, render_frontmatter};
use crate::lossless_grammar as g;

pub(crate) fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
    // Every walk below recurses, so the depth is checked first, without
    // recursion. 이것을 실패시키는 것: 이 검사를 지우기(`nesting_depth_is_limited`).
    if super::nests_too_deep(document) {
        return Err(MdError::UnsupportedStructure {
            detail: format!(
                "structure nests deeper than {} containers (lossless limit)",
                g::MAX_NESTING_DEPTH
            ),
        });
    }
    // §19.16 J (review r1): the grammar has no place for a caption number,
    // and captions are written as plain text (tables) or not at all
    // (pictures) — so a number anywhere, captions included, is refused
    // here rather than dropped. 이것을 실패시키는 것: 이 검사를 지우기
    // (`caption_number_is_written_or_refused` — F17 의 캡션 번호가 사라진 채 성공).
    if holds_auto_number(document) {
        return Err(MdError::UnsupportedStructure {
            detail: "caption number (auto number) is not carried by the lossless encoder"
                .to_string(),
        });
    }
    let mut frontmatter = from_metadata(document.metadata(), None);
    if frontmatter.template.is_none()
        && frontmatter.title.is_none()
        && frontmatter.author.is_none()
        && frontmatter.date.is_none()
        && frontmatter.metadata.is_empty()
    {
        frontmatter
            .metadata
            .insert("format".to_string(), serde_yaml::Value::String("lossless".to_string()));
    }
    let mut output = render_frontmatter(&frontmatter)?;
    output.push('\n');

    let expanded = hwpforge_core::field_view::expand_sections(document.sections());
    for (section_index, section) in expanded.iter().enumerate() {
        output.push_str(&format!(
            "<section data-index=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\" data-margin-left-unit=\"{}\" data-margin-right-unit=\"{}\" data-margin-top-unit=\"{}\" data-margin-bottom-unit=\"{}\" data-header-margin-unit=\"{}\" data-footer-margin-unit=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\" data-margin-left-mm=\"{:.2}\" data-margin-right-mm=\"{:.2}\" data-margin-top-mm=\"{:.2}\" data-margin-bottom-mm=\"{:.2}\" data-header-margin-mm=\"{:.2}\" data-footer-margin-mm=\"{:.2}\">\n",
            section_index,
            section.page_settings.width.as_i32(),
            section.page_settings.height.as_i32(),
            section.page_settings.margin_left.as_i32(),
            section.page_settings.margin_right.as_i32(),
            section.page_settings.margin_top.as_i32(),
            section.page_settings.margin_bottom.as_i32(),
            section.page_settings.header_margin.as_i32(),
            section.page_settings.footer_margin.as_i32(),
            section.page_settings.width.to_mm(),
            section.page_settings.height.to_mm(),
            section.page_settings.margin_left.to_mm(),
            section.page_settings.margin_right.to_mm(),
            section.page_settings.margin_top.to_mm(),
            section.page_settings.margin_bottom.to_mm(),
            section.page_settings.header_margin.to_mm(),
            section.page_settings.footer_margin.to_mm()
        ));

        for paragraph in &section.paragraphs {
            output.push_str(&encode_paragraph(paragraph)?);
            output.push('\n');
        }

        output.push_str("</section>\n");
    }

    Ok(output)
}

/// The object frame (S20, design §19.23.21 FRAME): the original box as
/// `data-original-width-unit`/`-height-unit` (with readable `-mm`) when it
/// is set, and `data-size-protect="true"` when the size is locked. A default
/// frame writes nothing, so its output is unchanged.
/// 이것을 실패시키는 것: 너비·높이·잠금 방출 중 하나를 지우기 / 기본 frame 에도
/// 쓰기 / `Some(표시 크기)` 를 건너뛰기(`object_frame_round_trips`,
/// `default_frame_writes_nothing`, `write_only_shapes_carry_the_frame`).
fn frame_attrs(frame: &hwpforge_core::ObjectFrame) -> String {
    let mut out = String::new();
    if let Some(o) = frame.original_size {
        out.push_str(&format!(
            " data-original-width-unit=\"{}\" data-original-height-unit=\"{}\" data-original-width-mm=\"{:.2}\" data-original-height-mm=\"{:.2}\"",
            o.width.as_i32(),
            o.height.as_i32(),
            o.width.to_mm(),
            o.height.to_mm()
        ));
    }
    if frame.size_protect {
        out.push_str(" data-size-protect=\"true\"");
    }
    out
}

/// One paragraph. Its end-mark char shape (`Paragraph::end_char_shape`,
/// §19.22 D5) is not written: like the layout cache it only sets the height
/// of the last line Hancom computes, which this grammar does not carry
/// (decided for the closed grammar of the lossless view).
fn encode_paragraph(paragraph: &Paragraph) -> MdResult<String> {
    let mut out = format!("<p data-para-shape=\"{}\">", paragraph.para_shape_id.get());

    for run in &paragraph.runs {
        match &run.content {
            // For both text-bearing variants we serialize the plain
            // text payload. `RunContent::InlineText` loses its
            // per-tab `width`/`leader`/`tab_type` attributes on this
            // path — Markdown has no native representation for them
            // and this lossless encoder targets MD round-trip, not
            // HWPX-attribute preservation. Use the HWPX→HWPX path
            // (Wave 4 Phase 3) when those attributes matter. See
            // debug doc §3a-A7.
            RunContent::Text(_) | RunContent::InlineText(_) => {
                let Some(cow) = run.content.plain_text() else {
                    continue;
                };
                out.push_str(&format!(
                    "<span data-char-shape=\"{}\">{}</span>",
                    run.char_shape_id.get(),
                    escape_html(&cow)
                ));
            }
            RunContent::Image(image) => {
                out.push_str(&encode_picture(image, Some(run.char_shape_id.get()))?);
            }
            RunContent::Control(control) => match control.as_ref() {
                Control::Hyperlink { text, url } => {
                    out.push_str(&format!(
                        "<a data-char-shape=\"{}\" href=\"{}\">{}</a>",
                        run.char_shape_id.get(),
                        g::escape_attr(url)?,
                        escape_html(text)
                    ));
                }
                Control::TextBox { .. }
                | Control::Line { .. }
                | Control::Ellipse { .. }
                | Control::Polygon { .. }
                | Control::Rect { .. }
                | Control::Arc { .. }
                | Control::Curve { .. }
                | Control::ConnectLine { .. } => {
                    out.push_str(&encode_shape(control, Some(run.char_shape_id.get()))?);
                }
                Control::Footnote { paragraphs, .. } => {
                    out.push_str(&format!(
                        "<footnote data-char-shape=\"{}\">",
                        run.char_shape_id.get()
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</footnote>");
                }
                Control::Endnote { paragraphs, .. } => {
                    out.push_str(&format!(
                        "<endnote data-char-shape=\"{}\">",
                        run.char_shape_id.get()
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</endnote>");
                }
                Control::Unknown { tag, data } => {
                    out.push_str(&format!(
                        "<control data-char-shape=\"{}\" data-kind=\"{}\">{}</control>",
                        run.char_shape_id.get(),
                        g::escape_attr(tag)?,
                        escape_html(data.as_deref().unwrap_or(""))
                    ));
                }
                Control::Dutmal { main_text, sub_text, sz_ratio, position, align, .. } => {
                    out.push_str(&format!(
                        "<dutmal data-char-shape=\"{}\" data-sz-ratio=\"{}\" data-position=\"{:?}\" data-align=\"{:?}\">{}</dutmal>",
                        run.char_shape_id.get(),
                        sz_ratio,
                        position,
                        align,
                        escape_html(&format!("{main_text}|{sub_text}"))
                    ));
                }
                Control::Compose { compose_text, circle_type, char_sz, compose_type, .. } => {
                    out.push_str(&format!(
                        "<compose data-char-shape=\"{}\" data-circle-type=\"{}\" data-char-sz=\"{}\" data-compose-type=\"{}\">{}</compose>",
                        run.char_shape_id.get(),
                        escape_html(circle_type),
                        char_sz,
                        escape_html(compose_type),
                        escape_html(compose_text)
                    ));
                }
                Control::Group { .. } => {
                    out.push_str(&encode_group(control, Some(run.char_shape_id.get()))?);
                }
                Control::FieldBoundary { boundary } => {
                    out.push_str(&encode_field_boundary(boundary, run.char_shape_id.get())?);
                }
                _ => {
                    return Err(MdError::UnsupportedStructure {
                        detail: "unsupported Control variant for lossless encoder".to_string(),
                    });
                }
            },
            RunContent::Table(table) => {
                out.push_str(&encode_table(table, run.char_shape_id.get())?);
            }
            _ => {
                return Err(MdError::UnsupportedStructure {
                    detail: "unsupported RunContent variant for lossless encoder".to_string(),
                });
            }
        }
    }

    out.push_str("</p>");
    Ok(out)
}

fn encode_field_boundary(
    boundary: &hwpforge_core::control::FieldBoundary,
    cs: usize,
) -> MdResult<String> {
    use super::markup::lossless_element;
    use hwpforge_core::control::{FieldBoundary, FieldParameter};
    let cs = cs.to_string();
    match boundary {
        FieldBoundary::End { begin_id, field_id } => Ok(lossless_element(
            "field-end",
            &[(g::CHAR_SHAPE, &cs), ("data-begin-id", begin_id), ("data-field-id", field_id)],
            true,
        )?
        .to_string()),
        FieldBoundary::Start(s) => {
            let mut attrs = vec![
                (g::CHAR_SHAPE, cs.as_str()),
                ("data-id", s.id.as_str()),
                ("data-kind", s.kind.as_str()),
                ("data-name", s.name.as_str()),
                ("data-editable", s.editable.as_str()),
                ("data-dirty", s.dirty.as_str()),
                ("data-z-order", s.z_order.as_str()),
                ("data-field-id", s.field_id.as_str()),
                ("data-meta-tag", s.meta_tag.as_str()),
            ];
            if s.unclosed {
                attrs.push(("data-unclosed", "true"));
            }
            if let Some(name) = &s.parameter_name {
                attrs.push(("data-parameter-name", name));
            }
            let empty = s.parameters.is_empty() && s.annotation.is_none();
            let mut out = lossless_element("field-start", &attrs, empty)?.to_string();
            if empty {
                return Ok(out);
            }
            for p in &s.parameters {
                let kind = g::field_parameter_kind(p)?;
                let (name, value, space) = match p {
                    FieldParameter::String { name, value, xml_space } => {
                        (name, value, Some(xml_space))
                    }
                    FieldParameter::Integer { name, value }
                    | FieldParameter::Boolean { name, value } => (name, value, None),
                    _ => {
                        return Err(MdError::UnsupportedStructure {
                            detail: "unknown field parameter".into(),
                        })
                    }
                };
                let mut attrs = vec![
                    ("data-kind", kind),
                    ("data-name", name.as_str()),
                    ("data-value", value.as_str()),
                ];
                if let Some(space) = space {
                    attrs.push(("data-xml-space", space));
                }
                out.push_str(&lossless_element("field-param", &attrs, true)?);
            }
            if let Some(paragraphs) = &s.annotation {
                admit_exact(paragraphs)?;
                out.push_str("<field-annotation>");
                for p in paragraphs {
                    out.push_str(&encode_paragraph(p)?);
                }
                out.push_str("</field-annotation>");
            }
            out.push_str("</field-start>");
            Ok(out)
        }
        _ => Err(MdError::UnsupportedStructure { detail: "unknown field boundary".into() }),
    }
}

fn encode_table(table: &Table, char_shape_id: usize) -> MdResult<String> {
    let mut out = format!("<table data-char-shape=\"{}\"", char_shape_id);
    // A table keeps only its size lock (it has no original box — validate).
    // 이것을 실패시키는 것: 표 잠금 방출을 지우기(`table_size_lock_round_trips`).
    if table.frame.size_protect {
        out.push_str(" data-size-protect=\"true\"");
    }
    // E: fixed cell sizes, written only when set.
    // 이것을 실패시키는 것: 방출 삭제(`table_no_adjust_round_trips`).
    if table.no_adjust {
        out.push_str(" data-no-adjust=\"true\"");
    }
    // F: the cell zones, `r0,c0,r1,c1,bf` joined by `;`, only when any.
    // 이것을 실패시키는 것: 방출 삭제 · 칸 순서 바꾸기(`table_cell_zones_round_trip`).
    if !table.cell_zones.is_empty() {
        let zones: Vec<String> = table
            .cell_zones
            .iter()
            .map(|z| {
                format!(
                    "{},{},{},{},{}",
                    z.start_row, z.start_col, z.end_row, z.end_col, z.border_fill_id
                )
            })
            .collect();
        out.push_str(&format!(" data-cell-zones=\"{}\"", zones.join(";")));
    }
    if let Some(width) = table.width {
        out.push_str(&format!(
            " data-width-unit=\"{}\" data-width-mm=\"{:.2}\"",
            width.as_i32(),
            width.to_mm()
        ));
    }
    if let Some(caption) = table.caption.as_ref() {
        // A table caption is written as its text only, so an object anywhere
        // under it (a note, a text box, a cell on the way) would vanish:
        // refused instead. 이것을 실패시키는 것: 이 거부를 지우기
        // (`table_caption_object_is_refused` — 캡션 → 각주 → 그림이 사라진 채 성공).
        if holds_object(&caption.paragraphs) {
            return Err(MdError::UnsupportedStructure {
                detail: "object inside a table caption is not carried by the lossless encoder"
                    .to_string(),
            });
        }
        let text: String = caption
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| r.content.as_text())
            .collect::<Vec<_>>()
            .join("");
        // D (Q9): an empty caption keeps its place (`data-caption=""`); the
        // side is written unless it is bottom (the reading default), the
        // full-size flag and a saved box width only when set — a bottom text
        // caption's markup is the one written before.
        // 이것을 실패시키는 것: 빈 캡션 생략 · 측·`fullSz`·`lastWidth` 방출 삭제
        // (`table_caption_attributes_round_trip`).
        out.push_str(&format!(" data-caption=\"{}\"", g::escape_attr(&text)?));
        let side = match caption.side {
            hwpforge_core::caption::CaptionSide::Top => Some("top"),
            hwpforge_core::caption::CaptionSide::Left => Some("left"),
            hwpforge_core::caption::CaptionSide::Right => Some("right"),
            _ => None,
        };
        if let Some(side) = side {
            out.push_str(&format!(" data-caption-side=\"{side}\""));
        }
        if caption.full_size {
            out.push_str(" data-caption-full-size=\"true\"");
        }
        if let Some(width) = caption.last_width {
            out.push_str(&format!(
                " data-caption-last-width-unit=\"{}\" data-caption-last-width-mm=\"{:.2}\"",
                width.as_i32(),
                width.to_mm()
            ));
        }
    }
    out.push('>');

    let header_rows = table.header_row_count();
    for (row_index, row) in table.rows.iter().enumerate() {
        out.push_str("<tr");
        if row_index < header_rows {
            out.push_str(&format!(" {}=\"true\"", g::ROW_HEADER));
        }
        if let Some(height) = row.height {
            out.push_str(&format!(
                " data-height-unit=\"{}\" data-height-mm=\"{:.2}\"",
                height.as_i32(),
                height.to_mm()
            ));
        }
        out.push('>');
        for cell in &row.cells {
            out.push_str(&format!(
                "<td data-col-span=\"{}\" data-row-span=\"{}\" data-width-unit=\"{}\" data-width-mm=\"{:.2}\"",
                cell.col_span,
                cell.row_span,
                cell.width.as_i32(),
                cell.width.to_mm()
            ));
            // Preserve explicit false as well as true; absence means inheritance.
            if let Some(header) = cell.header {
                out.push_str(&format!(" {}=\"{}\"", g::CELL_HEADER, g::bool_str(header)));
            }
            // §19.22 D3: a cell's line wrap other than the default break,
            // spelled as its Core serde name (`SQUEEZE`, `KEEP`).
            // 이것을 실패시키는 것: 방출 삭제 · 기본값에도 쓰기
            // (`cell_line_wrap_round_trips`).
            if let Some(wrap) = g::line_wrap_str(cell.line_wrap)? {
                out.push_str(&format!(" {}=\"{wrap}\"", g::LINE_WRAP));
            }
            // `#RRGGBB` has no place for the color's reserved high byte:
            // such a color is refused, not written without it.
            // 이것을 실패시키는 것: 이 검사를 지우기(`cell_background_color_is_read_strictly`).
            if let Some(background) = cell.background {
                out.push_str(&format!(" data-background=\"{}\"", g::color_str(background)?));
            }
            out.push('>');

            for paragraph in &cell.paragraphs {
                out.push_str(&encode_paragraph(paragraph)?);
            }

            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }

    out.push_str("</table>");
    Ok(out)
}

/// Whether a picture, a group or a drawing shape sits anywhere under
/// `paragraphs` — notes, text boxes, cells, captions and memo anchors
/// included (a picture is itself one, so its caption need not be entered).
fn holds_object(paragraphs: &[Paragraph]) -> bool {
    fn is_object(run: &hwpforge_core::Run) -> bool {
        match &run.content {
            RunContent::Image(_) => true,
            RunContent::Control(c) => match c.as_ref() {
                Control::Group { .. }
                | Control::Line { .. }
                | Control::Rect { .. }
                | Control::Ellipse { .. }
                | Control::Arc { .. }
                | Control::Polygon { .. }
                | Control::Curve { .. }
                | Control::ConnectLine { .. }
                | Control::TextBox { .. } => true,
                Control::Memo { anchor_runs, .. } => anchor_runs.iter().any(is_object),
                Control::FieldBoundary { .. } => true,
                _ => false,
            },
            _ => false,
        }
    }
    let mut found = false;
    for p in paragraphs {
        p.for_each_paragraph(|q| found |= q.runs.iter().any(is_object));
    }
    found
}

/// BOX: the displayed size, with its readable millimetres.
fn box_attrs(width: hwpforge_foundation::HwpUnit, height: hwpforge_foundation::HwpUnit) -> String {
    format!(
        " {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\"",
        g::WIDTH,
        width.as_i32(),
        g::HEIGHT,
        height.as_i32(),
        g::BOX_MM[0],
        g::mm(width),
        g::BOX_MM[1],
        g::mm(height)
    )
}

/// PLACE and OUTMARGIN of an object, nothing for `None`.
/// 이것을 실패시키는 것: PLACE 방출 삭제 · 바깥 여백 방출 삭제
/// (`group_round_trips_every_field`).
fn place_attrs(placement: Option<&ObjectPlacement>) -> MdResult<String> {
    let Some(p) = placement else { return Ok(String::new()) };
    let mut out = format!(
        " {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{}\"",
        g::TEXT_WRAP,
        g::text_wrap_str(&p.text_wrap)?,
        g::TEXT_FLOW,
        g::text_flow_str(&p.text_flow)?,
        g::TREAT_AS_CHAR,
        g::bool_str(p.treat_as_char),
        g::FLOW_WITH_TEXT,
        g::bool_str(p.flow_with_text),
        g::ALLOW_OVERLAP,
        g::bool_str(p.allow_overlap),
        g::VERT_REL_TO,
        g::rel_to_str(&p.vert_rel_to)?,
        g::HORZ_REL_TO,
        g::rel_to_str(&p.horz_rel_to)?,
        g::VERT_OFFSET,
        p.vert_offset.as_i32(),
        g::HORZ_OFFSET,
        p.horz_offset.as_i32(),
        g::PLACE_MM[0],
        g::mm(p.vert_offset),
        g::PLACE_MM[1],
        g::mm(p.horz_offset),
        g::VERT_ALIGN,
        g::vert_align_str(&p.vert_align)?,
        g::HORZ_ALIGN,
        g::horz_align_str(&p.horz_align)?,
        g::AFFECT_LINE_SPACING,
        g::bool_str(p.affect_line_spacing),
        g::HOLD_ANCHOR,
        g::bool_str(p.hold_anchor_and_so),
    );
    if let Some(m) = p.out_margin {
        for (name, v) in g::OUT_MARGIN.iter().zip([m.left, m.right, m.top, m.bottom]) {
            out.push_str(&format!(" {name}=\"{}\"", v.as_i32()));
        }
    }
    Ok(out)
}

/// `data-inst-id`, when the object has one.
fn inst_id_attr(id: Option<hwpforge_core::ObjectId>) -> String {
    id.map(|id| format!(" {}=\"{}\"", g::INST_ID, id.value())).unwrap_or_default()
}

/// A group (`<group>`): `char_shape` under a paragraph, `None` as a group
/// child. Every child is written exactly or the document is refused — no
/// partial group.
fn encode_group(group: &Control, char_shape: Option<usize>) -> MdResult<String> {
    let Control::Group { children, width, height, frame, placement, inst_id, .. } = group else {
        unreachable!("encode_group takes a group")
    };
    let mut out = String::from("<group");
    if let Some(cs) = char_shape {
        out.push_str(&format!(" {}=\"{cs}\"", g::CHAR_SHAPE));
    }
    out.push_str(&box_attrs(*width, *height));
    out.push_str(&frame_attrs(frame));
    out.push_str(&place_attrs(placement.as_ref())?);
    out.push_str(&inst_id_attr(*inst_id));
    if children.is_empty() {
        out.push_str(" />");
        return Ok(out);
    }
    out.push('>');
    for child in children {
        match child {
            GroupChild::Picture(image) => out.push_str(&encode_picture(image, None)?),
            GroupChild::Shape(shape @ Control::Group { .. }) => {
                out.push_str(&encode_group(shape, None)?)
            }
            GroupChild::Shape(
                shape @ (Control::TextBox { .. }
                | Control::Line { .. }
                | Control::Ellipse { .. }
                | Control::Polygon { .. }
                | Control::Rect { .. }
                | Control::Arc { .. }
                | Control::Curve { .. }
                | Control::ConnectLine { .. }),
            ) => out.push_str(&encode_shape(shape, None)?),
            // Drawing shapes as children come with their own grammar later;
            // until then the group is refused whole.
            // 이것을 실패시키는 것: 이 거부를 지우고 자식을 건너뛰기
            // (`group_with_a_shape_child_is_refused`).
            GroupChild::Shape(shape) => {
                return Err(MdError::UnsupportedStructure {
                    detail: format!(
                        "group child {} is not carried by the lossless encoder",
                        shape.kind_name()
                    ),
                })
            }
            _ => {
                return Err(MdError::UnsupportedStructure {
                    detail: "group child is not carried by the lossless encoder".to_string(),
                })
            }
        }
    }
    out.push_str("</group>");
    Ok(out)
}

/// A picture (`<img>`): `char_shape` under a paragraph, `None` as a group
/// child. Every field of the picture is written; its caption follows as a
/// `<caption>` child.
fn encode_picture(image: &Image, char_shape: Option<usize>) -> MdResult<String> {
    // A group picture's caption has no place yet (W5 places captions in
    // groups along with shapes). 이것을 실패시키는 것: 이 거부를 지우기
    // (`group_with_what_it_cannot_hold_is_refused`).
    if char_shape.is_none() && image.caption.is_some() {
        return Err(MdError::UnsupportedStructure {
            detail: "group picture caption is not carried by the lossless encoder".to_string(),
        });
    }
    let mut out = String::from("<img");
    if let Some(cs) = char_shape {
        out.push_str(&format!(" {}=\"{cs}\"", g::CHAR_SHAPE));
    }
    out.push_str(&format!(
        " {}=\"{}\" {}=\"{}\" {}=\"{}\"",
        g::SRC,
        g::escape_attr(&image.path)?,
        g::ALT,
        g::escape_attr(&image.path)?,
        g::FORMAT,
        g::escape_attr(&image.format.to_string())?
    ));
    // The Display form lower-cases an unknown format (`BIN0001` → `bin0001`),
    // so its raw spelling goes along. 이것을 실패시키는 것: 이 방출 삭제
    // (`group_round_trips_every_field` 의 `BIN0001`).
    if let ImageFormat::Unknown(raw) = &image.format {
        out.push_str(&format!(" {}=\"{}\"", g::FORMAT_RAW, g::escape_attr(raw)?));
    }
    out.push_str(&box_attrs(image.width, image.height));
    out.push_str(&frame_attrs(&image.frame));
    out.push_str(&place_attrs(image.placement.as_ref())?);
    out.push_str(&inst_id_attr(image.inst_id));
    if let Some(r) = image.rotation {
        out.push_str(&format!(" {}=\"{r}\"", g::ROTATION));
    }
    if let Some(f) = image.flip {
        out.push_str(&format!(" {}=\"{}\"", g::FLIP, g::flip_str(f)?));
    }
    if let Some(m) = image.in_margin {
        for (name, value) in g::IN_MARGIN.iter().zip([m.left, m.right, m.top, m.bottom]) {
            out.push_str(&format!(" {name}=\"{}\"", value.as_i32()));
        }
    }
    if let Some(c) = image.crop {
        for (name, v) in g::CROP.iter().zip([c.left, c.top, c.right, c.bottom]) {
            out.push_str(&format!(" {name}=\"{}\"", v.as_i32()));
        }
    }
    if let Some((w, h)) = image.image_extent {
        out.push_str(&format!(
            " {}=\"{}\" {}=\"{}\"",
            g::EXTENT[0],
            w.as_i32(),
            g::EXTENT[1],
            h.as_i32()
        ));
    }
    if let Some(url) = image.hyperlink.as_deref() {
        out.push_str(&format!(" {}=\"{}\"", g::HYPERLINK, g::escape_attr(url)?));
    }
    match &image.caption {
        None => out.push_str(" />"),
        Some(caption) => {
            out.push('>');
            out.push_str(&encode_caption(caption)?);
            out.push_str("</img>");
        }
    }
    Ok(out)
}

/// STYLE: nothing for `None`; the marker and each `Some` field otherwise.
/// 이것을 실패시키는 것: 표지 삭제 · 필드 방출 각각 삭제 · 채우기 종류 섞기
/// (`shape_style_round_trips`, `shape_markup_spells_core_names`).
fn style_attrs(style: Option<&ShapeStyle>) -> MdResult<String> {
    let Some(st) = style else { return Ok(String::new()) };
    let mut out = format!(" {}=\"true\"", g::STYLE);
    let mut put = |name: &str, value: String| out.push_str(&format!(" {name}=\"{value}\""));
    if let Some(c) = st.line_color {
        put(g::LINE_COLOR, g::color_str(c)?);
    }
    if let Some(w) = st.line_width {
        put(g::LINE_WIDTH, w.to_string());
    }
    if let Some(ls) = st.line_style {
        put(g::LINE_STYLE, g::line_style_str(ls)?.to_string());
    }
    if let Some(c) = st.fill_color {
        put(g::FILL_COLOR, g::color_str(c)?);
    }
    if let Some(r) = st.rotation {
        // Display writes the shortest text that reads back to the same
        // bits (`-0` included); a value that is not finite has no form.
        // 이것을 실패시키는 것: `{:.2}` 로 쓰기 · 비유한값 통과(`rotation_is_written_exactly`).
        if !r.is_finite() {
            return Err(MdError::UnsupportedStructure {
                detail: "shape rotation that is not finite has no lossless form".to_string(),
            });
        }
        put(g::ROTATION, r.to_string());
    }
    if let Some(f) = st.flip {
        put(g::FLIP, g::flip_str(f)?.to_string());
    }
    for (names, arrow) in [(g::HEAD_ARROW, st.head_arrow), (g::TAIL_ARROW, st.tail_arrow)] {
        if let Some(a) = arrow {
            put(names[0], g::arrow_type_str(a.arrow_type)?.to_string());
            put(names[1], g::arrow_size_str(a.size)?.to_string());
            put(names[2], g::bool_str(a.filled).to_string());
        }
    }
    match &st.fill {
        None => {}
        Some(Fill::Solid { color }) => {
            put(g::FILL_KIND, "Solid".to_string());
            put(g::FILL_SOLID_COLOR, g::color_str(*color)?);
        }
        Some(Fill::Gradient {
            gradient_type,
            angle,
            colors,
            center_x,
            center_y,
            step,
            step_center,
        }) => {
            put(g::FILL_KIND, "Gradient".to_string());
            put(g::GRADIENT[0], g::gradient_type_str(*gradient_type)?.to_string());
            put(g::GRADIENT[1], angle.to_string());
            let stops = colors
                .iter()
                .map(|(c, at)| Ok(format!("{}@{at}", g::color_str(*c)?)))
                .collect::<MdResult<Vec<_>>>()?;
            put(g::GRADIENT[2], stops.join(";"));
            let optional = [
                center_x.map(|v| v.to_string()),
                center_y.map(|v| v.to_string()),
                step.map(|v| v.to_string()),
                step_center.map(|v| v.to_string()),
            ];
            for (name, value) in g::GRADIENT_OPTIONAL.iter().zip(optional) {
                if let Some(v) = value {
                    put(name, v);
                }
            }
        }
        Some(Fill::Pattern { pattern_type, fg_color, bg_color }) => {
            put(g::FILL_KIND, "Pattern".to_string());
            put(g::PATTERN[0], g::pattern_type_str(*pattern_type)?.to_string());
            put(g::PATTERN[1], g::color_str(*fg_color)?);
            put(g::PATTERN[2], g::color_str(*bg_color)?);
        }
        Some(Fill::None) => put(g::FILL_KIND, "None".to_string()),
        // An image fill refers to a picture the lossless view does not
        // carry (user decision 2026-10-15). 이것을 실패시키는 것: 이 거부를
        // 지우기(`image_fill_is_refused`).
        Some(_) => {
            return Err(MdError::UnsupportedStructure {
                detail: "image fill is not carried by the lossless encoder".to_string(),
            })
        }
    }
    if let Some(d) = g::drop_cap_str(st.drop_cap_style)? {
        put(g::DROP_CAP, d.to_string());
    }
    if let Some(r) = st.corner_ratio {
        put(g::CORNER_RATIO, r.to_string());
    }
    Ok(out)
}

/// `x,y;x,y` — empty for no points.
fn points_str(points: &[ShapePoint]) -> String {
    points.iter().map(|v| format!("{},{}", v.x, v.y)).collect::<Vec<_>>().join(";")
}

fn point_attrs(names: [&str; 2], p: ShapePoint) -> String {
    format!(" {}=\"{}\" {}=\"{}\"", names[0], p.x, names[1], p.y)
}

/// A drawing shape (`<textbox>`, `<line>`, `<ellipse>`, `<polygon>`,
/// `<rect>`, `<arc>`, `<curve>`, `<connectline>`): `char_shape` under a
/// paragraph, `None` as a group child. Its text is an exact subtree; a caption has no place yet
/// and is refused.
fn encode_shape(shape: &Control, char_shape: Option<usize>) -> MdResult<String> {
    let (tag, width, height, frame, placement, caption, style) = match shape {
        Control::TextBox { width, height, frame, placement, caption, style, .. } => {
            ("textbox", width, height, frame, placement, caption, style)
        }
        Control::Line { width, height, frame, placement, caption, style, .. } => {
            ("line", width, height, frame, placement, caption, style)
        }
        Control::Ellipse { width, height, frame, placement, caption, style, .. } => {
            ("ellipse", width, height, frame, placement, caption, style)
        }
        Control::Polygon { width, height, frame, placement, caption, style, .. } => {
            ("polygon", width, height, frame, placement, caption, style)
        }
        Control::Rect { width, height, frame, placement, caption, style, .. } => {
            ("rect", width, height, frame, placement, caption, style)
        }
        Control::Arc { width, height, frame, placement, caption, style, .. } => {
            ("arc", width, height, frame, placement, caption, style)
        }
        Control::Curve { width, height, frame, placement, caption, style, .. } => {
            ("curve", width, height, frame, placement, caption, style)
        }
        Control::ConnectLine { width, height, frame, placement, caption, style, .. } => {
            ("connectline", width, height, frame, placement, caption, style)
        }
        _ => unreachable!("encode_shape takes a drawing shape"),
    };
    // 이것을 실패시키는 것: 이 거부를 지우기(`shape_caption_is_refused`).
    if caption.is_some() {
        return Err(MdError::UnsupportedStructure {
            detail: format!("{tag} caption is not carried by the lossless encoder"),
        });
    }
    // Core validation checks these only where it walks — not inside a
    // picture caption or a nested text — so the encoder checks them itself:
    // a text box without paragraphs would read back as the old form (one
    // made-up paragraph) or not at all, and a polygon needs three points.
    // 이것을 실패시키는 것: 두 거부 각각 지우기(`nested_empty_shapes_are_refused`).
    let malformed = match shape {
        Control::TextBox { paragraphs, .. } => paragraphs.is_empty(),
        Control::Polygon { vertices, .. } => vertices.len() < 3,
        _ => false,
    };
    if malformed {
        return Err(MdError::UnsupportedStructure {
            detail: format!(
                "{tag} without paragraphs or points is not carried by the lossless encoder"
            ),
        });
    }
    let mut out = format!("<{tag}");
    if let Some(cs) = char_shape {
        out.push_str(&format!(" {}=\"{cs}\"", g::CHAR_SHAPE));
    }
    match shape {
        Control::Line { start, end, .. } => {
            out.push_str(&point_attrs(g::START, *start));
            out.push_str(&point_attrs(g::END, *end));
        }
        Control::Ellipse { center, axis1, axis2, .. } => {
            out.push_str(&point_attrs(g::CENTER, *center));
            out.push_str(&point_attrs(g::AXIS1, *axis1));
            out.push_str(&point_attrs(g::AXIS2, *axis2));
        }
        Control::Polygon { vertices, .. } => {
            out.push_str(&format!(" {}=\"{}\"", g::POINTS, points_str(vertices)));
        }
        Control::Arc { arc_type, center, axis1, axis2, start1, end1, start2, end2, .. } => {
            out.push_str(&point_attrs(g::CENTER, *center));
            out.push_str(&point_attrs(g::AXIS1, *axis1));
            out.push_str(&point_attrs(g::AXIS2, *axis2));
            out.push_str(&point_attrs(g::START1, *start1));
            out.push_str(&point_attrs(g::END1, *end1));
            out.push_str(&point_attrs(g::START2, *start2));
            out.push_str(&point_attrs(g::END2, *end2));
            out.push_str(&format!(" {}=\"{}\"", g::ARC_TYPE, g::arc_type_str(*arc_type)?));
        }
        // The two lists are written apart, with no length rule between
        // them (Core has none). 이것을 실패시키는 것: 세그먼트를 점 개수에서 다시
        // 만들기(`w4_shapes_round_trip` 의 점 3·세그먼트 1).
        Control::Curve { points, segment_types, .. } => {
            out.push_str(&format!(" {}=\"{}\"", g::POINTS, points_str(points)));
            let segs =
                segment_types.iter().map(|t| g::segment_str(*t)).collect::<MdResult<Vec<_>>>()?;
            out.push_str(&format!(" {}=\"{}\"", g::SEGMENTS, segs.join(",")));
        }
        Control::ConnectLine { start, end, control_points, connect_type, .. } => {
            out.push_str(&point_attrs(g::START, *start));
            out.push_str(&point_attrs(g::END, *end));
            out.push_str(&format!(" {}=\"{}\"", g::CONTROL_POINTS, points_str(control_points)));
            out.push_str(&format!(" {}=\"{}\"", g::CONNECT_TYPE, g::escape_attr(connect_type)?));
        }
        _ => {}
    }
    out.push_str(&box_attrs(*width, *height));
    out.push_str(&frame_attrs(frame));
    out.push_str(&place_attrs(placement.as_ref())?);
    out.push_str(&style_attrs(style.as_ref())?);
    let (valign, paragraphs) = match shape {
        Control::TextBox { text_vertical_align, paragraphs, text_margin, .. } => {
            if let Some(m) = text_margin {
                for (name, v) in g::TEXT_MARGIN.iter().zip([m.left, m.right, m.top, m.bottom]) {
                    out.push_str(&format!(" {name}=\"{}\"", v.as_i32()));
                }
            }
            (Some(*text_vertical_align), paragraphs.as_slice())
        }
        Control::Ellipse { text_vertical_align, paragraphs, .. }
        | Control::Polygon { text_vertical_align, paragraphs, .. } => {
            (Some(*text_vertical_align), paragraphs.as_slice())
        }
        _ => (None, &[][..]),
    };
    if let Some(v) = valign.map(g::valign_str).transpose()?.flatten() {
        out.push_str(&format!(" {}=\"{v}\"", g::TEXT_VALIGN));
    }
    if paragraphs.is_empty() {
        out.push_str(" />");
        return Ok(out);
    }
    admit_exact(paragraphs)?;
    out.push('>');
    for paragraph in paragraphs {
        out.push_str(&encode_paragraph(paragraph)?);
    }
    out.push_str(&format!("</{tag}>"));
    Ok(out)
}

/// A caption (`<caption>`): side, gap, width, full-size flag and saved
/// width, then its paragraphs — an exact subtree, so what this wave cannot
/// write exactly inside it refuses the document.
fn encode_caption(caption: &hwpforge_core::caption::Caption) -> MdResult<String> {
    admit_exact(&caption.paragraphs)?;
    let mut out = format!(
        "<caption {}=\"{}\" {}=\"{}\" {}=\"{}\"",
        g::CAPTION_SIDE,
        g::caption_side_str(caption.side)?,
        g::CAPTION_GAP,
        caption.gap.as_i32(),
        g::CAPTION_GAP_MM,
        g::mm(caption.gap)
    );
    if let Some(w) = caption.width {
        out.push_str(&format!(
            " {}=\"{}\" {}=\"{}\"",
            g::WIDTH,
            w.as_i32(),
            g::BOX_MM[0],
            g::mm(w)
        ));
    }
    if caption.full_size {
        out.push_str(&format!(" {}=\"true\"", g::CAPTION_FULL_SIZE));
    }
    if let Some(w) = caption.last_width {
        out.push_str(&format!(" {}=\"{}\"", g::CAPTION_LAST_WIDTH, w.as_i32()));
    }
    if caption.paragraphs.is_empty() {
        out.push_str(" />");
        return Ok(out);
    }
    out.push('>');
    for paragraph in &caption.paragraphs {
        out.push_str(&encode_paragraph(paragraph)?);
    }
    out.push_str("</caption>");
    Ok(out)
}

/// The admission of an exact subtree (a picture caption, a shape's text).
/// Every picture, group and drawing shape in it is written exactly (with
/// its own caption or text admitted when it is written), so what remains
/// is that no list may be empty where the decoder would make content up: a
/// paragraph without runs, a table without rows, a cell or a note without
/// paragraphs. Checked before anything is written, so a later comparison
/// of paragraphs by projection cannot hide a lost object.
/// 이것을 실패시키는 것: 빈 목록 거부 각각 삭제 · 표 칸 안으로 내려가기 삭제
/// (`picture_caption_admission`, `shape_text_admission`).
fn admit_exact(paragraphs: &[Paragraph]) -> MdResult<()> {
    let refuse = |what: &str| {
        Err(MdError::UnsupportedStructure {
            detail: format!(
                "{what} inside a picture caption or shape text is not carried by the lossless encoder"
            ),
        })
    };
    for p in paragraphs {
        if p.runs.is_empty() {
            return refuse("a paragraph without runs");
        }
        for run in &p.runs {
            // A picture's caption and a shape's text are admitted when they
            // are written; tables and notes are written as they are, so their
            // contents are admitted here.
            match &run.content {
                RunContent::Table(t) => {
                    if t.rows.is_empty() {
                        return refuse("a table without rows");
                    }
                    for cell in t.rows.iter().flat_map(|r| &r.cells) {
                        if cell.paragraphs.is_empty() {
                            return refuse("a cell without paragraphs");
                        }
                        admit_exact(&cell.paragraphs)?;
                    }
                }
                RunContent::Control(c) => match c.as_ref() {
                    Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                        if paragraphs.is_empty() {
                            return refuse("a note without paragraphs");
                        }
                        admit_exact(paragraphs)?;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}

fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Whether any paragraph — picture captions included, which the Core
/// paragraph walk does not visit — holds a [`Control::AutoNumber`].
pub(crate) fn holds_auto_number(document: &Document<Validated>) -> bool {
    fn paragraph_has(p: &Paragraph) -> bool {
        p.runs.iter().any(|run| match &run.content {
            RunContent::Control(c) => matches!(**c, Control::AutoNumber { .. }),
            RunContent::Image(image) => image.caption.as_ref().is_some_and(|caption| {
                caption.paragraphs.iter().any(|cp| {
                    let mut found = false;
                    cp.for_each_paragraph(|q| found |= paragraph_has(q));
                    found
                })
            }),
            _ => false,
        })
    }
    let mut found = false;
    for section in document.sections() {
        let lists = std::iter::once(&section.paragraphs)
            .chain(section.headers.iter().chain(section.footers.iter()).map(|hf| &hf.paragraphs))
            .chain(section.master_pages.iter().flatten().map(|mp| &mp.paragraphs));
        for paragraphs in lists {
            for p in paragraphs {
                p.for_each_paragraph(|q| found |= paragraph_has(q));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text box with a rounded corner is refused (the grammar cannot
    /// carry it yet); a square one is written.
    #[test]
    fn rounded_text_box_round_trips() {
        use hwpforge_core::control::ShapeStyle;
        let doc = |ratio: Option<u8>| {
            let unit = hwpforge_foundation::HwpUnit::new(3000).unwrap();
            let inner = Paragraph::with_runs(
                vec![Run::text("box", hwpforge_foundation::CharShapeIndex::new(0))],
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
            let mut tb = Control::text_box(vec![inner], unit, unit);
            if let Control::TextBox { style, .. } = &mut tb {
                *style = Some(ShapeStyle { corner_ratio: ratio, ..Default::default() });
            }
            let para = Paragraph::with_runs(
                vec![Run::control(tb, hwpforge_foundation::CharShapeIndex::new(0))],
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![para], hwpforge_core::PageSettings::a4()));
            d.validate().unwrap()
        };
        // Carried since W3 (STYLE `data-corner-ratio`).
        for ratio in [Some(20), Some(0), None] {
            let d = doc(ratio);
            let back = round_trip(&d);
            assert_eq!(back.sections()[0].paragraphs[0].runs, d.sections()[0].paragraphs[0].runs);
        }
    }
    /// S18: a picture's (W2) and a text box's (W3) outer margin round-trip,
    /// zero or not.
    ///
    /// 이것을 실패시키는 것: 바깥 여백 방출 삭제.
    #[test]
    fn object_outer_margin_round_trips() {
        use hwpforge_core::placement::ObjectPlacement;
        use hwpforge_core::table::TableMargin;
        let with_margin = |bottom: i32| {
            let u = |v| HwpUnit::new(v).unwrap();
            let mut p = ObjectPlacement::legacy_inline_defaults();
            p.out_margin =
                Some(TableMargin { left: u(0), right: u(0), top: u(0), bottom: u(bottom) });
            p
        };
        let doc = |run: Run| {
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                hwpforge_core::PageSettings::a4(),
            ));
            d.validate().unwrap()
        };
        for bottom in [140, 0] {
            let unit = HwpUnit::new(3000).unwrap();
            // A picture's outer margin is carried since W2.
            let image = Image::new("BinData/a.png", unit, unit, ImageFormat::Png)
                .with_placement(with_margin(bottom));
            let run = Run::image(image, CharShapeIndex::new(0));
            let back = round_trip(&doc(run.clone()));
            assert_eq!(back.sections()[0].paragraphs[0].runs[0], run, "image");
            let inner = Paragraph::with_runs(
                vec![Run::text("box", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let mut tb = Control::text_box(vec![inner], unit, unit);
            if let Control::TextBox { placement, .. } = &mut tb {
                *placement = Some(with_margin(bottom));
            }
            let run = Run::control(tb, CharShapeIndex::new(0));
            let back = round_trip(&doc(run.clone()));
            assert_eq!(back.sections()[0].paragraphs[0].runs[0], run, "text box");
        }
    }

    // ── S20 FRAME (design §19.23.21) ──

    fn frame_doc(runs: Vec<Run>) -> Document<Validated> {
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(runs, ParaShapeIndex::new(0))],
            hwpforge_core::PageSettings::a4(),
        ));
        d.validate().unwrap()
    }

    fn hu(v: i32) -> HwpUnit {
        HwpUnit::new(v).unwrap()
    }

    fn frame_of(original: Option<(i32, i32)>, lock: bool) -> hwpforge_core::ObjectFrame {
        let f = hwpforge_core::ObjectFrame::new().with_size_protect(lock);
        match original {
            Some((w, h)) => f.with_original_size(hwpforge_foundation::Size::new(hu(w), hu(h))),
            None => f,
        }
    }

    fn round_trip(doc: &Document<Validated>) -> Document {
        let md = encode_lossless(doc).unwrap();
        crate::MdDecoder::decode_lossless(&md).unwrap()
    }

    fn first_run(doc: &Document) -> RunContent {
        doc.sections()[0].paragraphs[0].runs[0].content.clone()
    }

    /// A picture's and a text box's frame comes back as written: no original
    /// box, one equal to the shown size (kept apart from `None`), a smaller
    /// one, an asymmetric one — each locked and unlocked.
    #[test]
    fn object_frame_round_trips() {
        for original in [None, Some((3000, 2000)), Some((1500, 1000)), Some((1, 7000))] {
            for lock in [false, true] {
                let frame = frame_of(original, lock);
                let mut image = Image::new("BinData/a.png", hu(3000), hu(2000), ImageFormat::Png);
                image.frame = frame;
                let back = round_trip(&frame_doc(vec![Run::image(image, CharShapeIndex::new(0))]));
                let RunContent::Image(image) = first_run(&back) else { panic!() };
                assert_eq!(image.frame, frame, "picture");

                let mut tb = Control::text_box(
                    vec![Paragraph::with_runs(
                        vec![Run::text("box", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hu(3000),
                    hu(2000),
                );
                *tb.frame_mut().unwrap() = frame;
                let back = round_trip(&frame_doc(vec![Run::control(tb, CharShapeIndex::new(0))]));
                let RunContent::Control(c) = first_run(&back) else { panic!() };
                assert_eq!(*c.frame().unwrap(), frame, "text box");
            }
        }
    }

    /// The container path of every frame-bearing object in `paragraphs`
    /// (`body`, `cell`, `textbox`, `note`, joined by `/`), with its frame.
    fn frames_by_path(paragraphs: &[Paragraph], path: &str, out: &mut Vec<String>) {
        let at =
            |kind: &str| if path.is_empty() { kind.to_string() } else { format!("{path}/{kind}") };
        for p in paragraphs {
            for r in &p.runs {
                match &r.content {
                    RunContent::Image(i) => out.push(format!(
                        "{} pic {:?} {}",
                        if path.is_empty() { "body" } else { path },
                        i.frame.original_size.map(|o| o.width.as_i32()),
                        i.frame.size_protect
                    )),
                    RunContent::Table(t) => {
                        out.push(format!(
                            "{} table {}",
                            if path.is_empty() { "body" } else { path },
                            t.frame.size_protect
                        ));
                        for row in &t.rows {
                            for cell in &row.cells {
                                frames_by_path(&cell.paragraphs, &at("cell"), out);
                            }
                        }
                    }
                    RunContent::Control(c) => match c.as_ref() {
                        Control::TextBox { paragraphs, .. } => {
                            frames_by_path(paragraphs, &at("textbox"), out)
                        }
                        Control::Footnote { paragraphs, .. } => {
                            frames_by_path(paragraphs, &at("note"), out)
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    /// Frames stay where they are: pictures with different frames in the
    /// body, a cell, a nested cell, a text box and a footnote, and a locked
    /// table in a text box, all in one document — compared by container
    /// path, so an object moved to another container in the same order is
    /// caught.
    /// 이것을 실패시키는 것: 디코더가 글상자 안 그림을 같은 순서로 바깥 문단에
    /// 옮기기(경로 비교가 잡는다).
    #[test]
    fn frames_keep_their_context() {
        use hwpforge_core::table::{TableCell, TableRow};
        let picture = |w: i32, lock: bool| {
            let mut image = Image::new("BinData/a.png", hu(3000), hu(2000), ImageFormat::Png);
            image.frame = frame_of(Some((w, 1000)), lock);
            Run::image(image, CharShapeIndex::new(0))
        };
        let para = |runs: Vec<Run>| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let table = |runs: Vec<Run>, lock: bool| {
            let mut t =
                Table::new(vec![TableRow::new(vec![TableCell::new(vec![para(runs)], hu(9000))])]);
            t.frame = frame_of(None, lock);
            t
        };
        let nested = table(vec![picture(1003, false)], false);
        let outer =
            table(vec![picture(1002, true), Run::table(nested, CharShapeIndex::new(0))], false);
        let locked_in_box = table(vec![Run::text("t", CharShapeIndex::new(0))], true);
        let tb = Control::text_box(
            vec![para(vec![
                picture(1004, false),
                Run::table(locked_in_box, CharShapeIndex::new(0)),
            ])],
            hu(9000),
            hu(9000),
        );
        let note = Control::footnote(vec![para(vec![picture(1005, true)])]);
        let doc = frame_doc(vec![
            picture(1001, false),
            Run::table(outer, CharShapeIndex::new(0)),
            Run::control(tb, CharShapeIndex::new(0)),
            Run::control(note, CharShapeIndex::new(0)),
        ]);
        let mut want = Vec::new();
        frames_by_path(&doc.sections()[0].paragraphs, "", &mut want);
        assert_eq!(
            want,
            [
                "body pic Some(1001) false",
                "body table false",
                "cell pic Some(1002) true",
                "cell table false",
                "cell/cell pic Some(1003) false",
                "textbox pic Some(1004) false",
                "textbox table true",
                "note pic Some(1005) true",
            ]
        );
        let back = round_trip(&doc);
        let mut got = Vec::new();
        frames_by_path(&back.sections()[0].paragraphs, "", &mut got);
        assert_eq!(got, want);
    }

    /// A table keeps its size lock.
    #[test]
    fn table_size_lock_round_trips() {
        use hwpforge_core::table::{TableCell, TableRow};
        for lock in [false, true] {
            let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("t", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hu(9000),
            )])]);
            t.frame = frame_of(None, lock);
            let back = round_trip(&frame_doc(vec![Run::table(t, CharShapeIndex::new(0))]));
            let RunContent::Table(t) = first_run(&back) else { panic!() };
            assert_eq!(t.frame.size_protect, lock);
        }
    }

    /// E: `no_adjust` round-trips with the size lock in all four
    /// combinations, at the top level and in a table nested in a cell; a
    /// table without it writes no `data-no-adjust`.
    /// 이것을 실패시키는 것: 인코더 방출 삭제 · 디코더 미반영 · 중첩 표에서
    /// 값 소실 · `false` 에도 방출.
    #[test]
    fn table_no_adjust_round_trips() {
        use hwpforge_core::table::{TableCell, TableRow};
        let leaf = |no_adjust: bool, lock: bool| {
            let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("t", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hu(9000),
            )])])
            .with_no_adjust(no_adjust);
            t.frame = frame_of(None, lock);
            t
        };
        for no_adjust in [false, true] {
            for lock in [false, true] {
                let inner = leaf(no_adjust, lock);
                let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::table(inner, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hu(9000),
                )])])
                .with_no_adjust(!no_adjust);
                let doc = frame_doc(vec![Run::table(outer, CharShapeIndex::new(0))]);
                let md = encode_lossless(&doc).unwrap();
                assert_eq!(md.matches("data-no-adjust=\"true\"").count(), 1, "{md}");
                let back = round_trip(&doc);
                let RunContent::Table(t) = first_run(&back) else { panic!() };
                assert_eq!(t.no_adjust, !no_adjust);
                let RunContent::Table(inner) = &t.rows[0].cells[0].paragraphs[0].runs[0].content
                else {
                    panic!()
                };
                assert_eq!((inner.no_adjust, inner.frame.size_protect), (no_adjust, lock));
            }
        }
    }

    /// F: cell zones round-trip in order and field by field, at the top
    /// level and in a table nested in a cell, together with `no_adjust`; a
    /// table without zones writes no `data-cell-zones`.
    /// 이것을 실패시키는 것: 인코더 방출 삭제 · 칸 순서 바꾸기 · 디코더 미반영 ·
    /// 중첩 표 소실 · 빈 목록 방출.
    #[test]
    fn table_cell_zones_round_trip() {
        use hwpforge_core::table::{CellZone, TableCell, TableRow};
        let grid = |zones: Vec<CellZone>| {
            let row = || {
                TableRow::new(
                    (0..2)
                        .map(|_| {
                            TableCell::new(
                                vec![Paragraph::with_runs(
                                    vec![Run::text("t", CharShapeIndex::new(0))],
                                    ParaShapeIndex::new(0),
                                )],
                                hu(4500),
                            )
                        })
                        .collect(),
                )
            };
            Table::new((0..32).map(|_| row()).collect()).with_cell_zones(zones)
        };
        let inner_zones = vec![CellZone::new(12, 0, 31, 1, 7), CellZone::new(0, 1, 0, 1, 3)];
        let inner = grid(inner_zones.clone()).with_no_adjust(true);
        let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::table(inner, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hu(9000),
        )])]);
        let doc = frame_doc(vec![Run::table(outer, CharShapeIndex::new(0))]);
        let md = encode_lossless(&doc).unwrap();
        assert_eq!(md.matches("data-cell-zones").count(), 1, "{md}");
        assert!(md.contains(r#"data-cell-zones="12,0,31,1,7;0,1,0,1,3""#), "{md}");
        let back = round_trip(&doc);
        let RunContent::Table(t) = first_run(&back) else { panic!() };
        assert!(t.cell_zones.is_empty());
        let RunContent::Table(inner) = &t.rows[0].cells[0].paragraphs[0].runs[0].content else {
            panic!()
        };
        assert_eq!((&inner.cell_zones, inner.no_adjust), (&inner_zones, true));
    }

    /// D (Q9): a caption's presence (an empty one too), side, `full_size`
    /// and `last_width` round-trip at the top level and in a nested table;
    /// a bottom caption writes no side.
    /// 이것을 실패시키는 것: 빈 캡션 생략 · 측·`full_size`·`last_width` 방출 삭제 ·
    /// 디코더 미반영 · 아래 캡션에도 측 방출.
    #[test]
    fn table_caption_attributes_round_trip() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::table::{TableCell, TableRow};
        let cell = |runs: Vec<Run>| {
            TableCell::new(vec![Paragraph::with_runs(runs, ParaShapeIndex::new(0))], hu(9000))
        };
        let captioned = |text: &str, side, full: bool, last: Option<i32>| {
            Table::new(vec![TableRow::new(vec![cell(vec![Run::text(
                "t",
                CharShapeIndex::new(0),
            )])])])
            .with_caption(
                Caption::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text(text, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    side,
                )
                .with_full_size(full)
                .with_last_width(last.map(hu)),
            )
        };
        let summary = |t: &Table| {
            let c = t.caption.as_ref().unwrap();
            (
                c.paragraphs
                    .iter()
                    .flat_map(|p| p.runs.iter())
                    .filter_map(|r| r.content.as_text())
                    .collect::<String>(),
                c.side,
                c.full_size,
                c.last_width.map(|w| w.as_i32()),
            )
        };
        for (text, side, full, last) in [
            ("", CaptionSide::Bottom, false, None),
            ("c", CaptionSide::Top, true, Some(48190)),
            ("c", CaptionSide::Left, false, Some(1)),
            ("c", CaptionSide::Right, false, None),
        ] {
            let inner = captioned(text, side, full, last);
            let outer = Table::new(vec![TableRow::new(vec![cell(vec![Run::table(
                inner.clone(),
                CharShapeIndex::new(0),
            )])])]);
            let doc = frame_doc(vec![Run::table(outer, CharShapeIndex::new(0))]);
            let back = round_trip(&doc);
            let RunContent::Table(t) = first_run(&back) else { panic!() };
            let RunContent::Table(nested) = &t.rows[0].cells[0].paragraphs[0].runs[0].content
            else {
                panic!()
            };
            assert_eq!(summary(nested), summary(&inner));
        }
        let md = encode_lossless(&frame_doc(vec![Run::table(
            captioned("c", CaptionSide::Bottom, false, None),
            CharShapeIndex::new(0),
        )]))
        .unwrap();
        assert!(md.contains(r#"data-caption="c">"#), "{md}");
    }

    /// A default frame writes nothing: the picture's markup is the one
    /// written before S20.
    #[test]
    fn default_frame_writes_nothing() {
        let image = Image::new("BinData/a.png", hu(3000), hu(2000), ImageFormat::Png);
        let md =
            encode_lossless(&frame_doc(vec![Run::image(image, CharShapeIndex::new(0))])).unwrap();
        assert!(
            md.contains(r#"<img data-char-shape="0" src="BinData/a.png" alt="BinData/a.png" data-format="PNG" data-width-unit="3000" data-height-unit="2000" data-width-mm="10.58" data-height-mm="7.06" />"#),
            "{md}"
        );
    }

    /// Lines, ellipses and polygons carry their frame and read it back
    /// (W3). A line shown flat with a flat original box is a valid frame
    /// (Core, built directly).
    #[test]
    fn shapes_carry_the_frame() {
        use hwpforge_core::control::ShapePoint;
        let mut line =
            Control::line(ShapePoint { x: 0, y: 0 }, ShapePoint { x: 1000, y: 0 }).unwrap();
        *line.frame_mut().unwrap() = frame_of(Some((1000, 0)), true);
        if let Control::Line { width, height, .. } = &mut line {
            *width = hu(2000);
            *height = HwpUnit::ZERO;
        }
        let mut ellipse = Control::ellipse(hu(850), hu(850));
        *ellipse.frame_mut().unwrap() = frame_of(Some((560, 560)), true);
        let mut polygon = Control::polygon(vec![
            ShapePoint { x: 0, y: 0 },
            ShapePoint { x: 100, y: 0 },
            ShapePoint { x: 0, y: 100 },
        ])
        .unwrap();
        *polygon.frame_mut().unwrap() = frame_of(Some((100, 100)), false);
        for (control, expected) in [
            (
                line,
                r#" data-original-width-unit="1000" data-original-height-unit="0" data-original-width-mm="3.53" data-original-height-mm="0.00" data-size-protect="true" />"#,
            ),
            (
                ellipse,
                r#" data-original-width-unit="560" data-original-height-unit="560" data-original-width-mm="1.98" data-original-height-mm="1.98" data-size-protect="true" />"#,
            ),
            (
                polygon,
                r#" data-original-width-unit="100" data-original-height-unit="100" data-original-width-mm="0.35" data-original-height-mm="0.35" />"#,
            ),
        ] {
            let run = Run::control(control, CharShapeIndex::new(0));
            let md = encode_lossless(&frame_doc(vec![run.clone()])).unwrap();
            assert!(md.contains(expected), "{md}");
            let back = crate::MdDecoder::decode_lossless(&md).unwrap();
            assert_eq!(back.sections()[0].paragraphs[0].runs[0], run, "{md}");
        }
    }

    /// A text box whose margin is not 283 everywhere is refused (§19.22 D4);
    /// 283 everywhere is written.
    #[test]
    fn text_box_margin_round_trips() {
        use hwpforge_core::table::TableMargin;
        let doc = |v: i32| {
            let unit = HwpUnit::new(3000).unwrap();
            let inner = Paragraph::with_runs(
                vec![Run::text("box", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let mut tb = Control::text_box(vec![inner], unit, unit);
            if let Control::TextBox { text_margin, .. } = &mut tb {
                let m = HwpUnit::new(283).unwrap();
                *text_margin = Some(TableMargin {
                    left: m,
                    right: m,
                    top: m,
                    bottom: HwpUnit::new(v).unwrap(),
                });
            }
            let para = Paragraph::with_runs(
                vec![Run::control(tb, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![para], hwpforge_core::PageSettings::a4()));
            d.validate().unwrap()
        };
        // Carried since W3 (TEXTMARGIN).
        for v in [400, 283, 0] {
            let d = doc(v);
            let back = round_trip(&d);
            assert_eq!(back.sections()[0].paragraphs[0].runs, d.sections()[0].paragraphs[0].runs);
        }
    }

    /// §19.22 D3: every cell line wrap comes back as written; the default
    /// break writes nothing, so a breaking cell's markup is the one written
    /// before.
    #[test]
    fn cell_line_wrap_round_trips() {
        use hwpforge_core::table::{CellLineWrap, Table};
        let doc = |wrap: CellLineWrap| {
            let para = Paragraph::with_runs(
                vec![Run::text("칸", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let cell = TableCell::new(vec![para], HwpUnit::new(3000).unwrap()).with_line_wrap(wrap);
            let host = Paragraph::with_runs(
                vec![Run::table(
                    Table::new(vec![TableRow::new(vec![cell])]),
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            );
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![host], hwpforge_core::PageSettings::a4()));
            d.validate().unwrap()
        };
        for (wrap, attr) in [
            (CellLineWrap::Break, None),
            (CellLineWrap::Squeeze, Some(r#"data-line-wrap="SQUEEZE""#)),
            (CellLineWrap::Keep, Some(r#"data-line-wrap="KEEP""#)),
        ] {
            let md = encode_lossless(&doc(wrap)).unwrap();
            assert_eq!(attr.map(|a| md.contains(a)), attr.map(|_| true), "{md}");
            assert_eq!(md.contains("data-line-wrap"), attr.is_some(), "{md}");
            let back = crate::MdDecoder::decode_lossless(&md).unwrap();
            let RunContent::Table(t) = first_run(&back) else { panic!() };
            assert_eq!(t.rows[0].cells[0].line_wrap, wrap);
        }
    }
    use hwpforge_core::{
        Control, Document, Image, ImageFormat, Paragraph, Run, Section, TableCell, TableRow,
    };
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn validated_document(paragraphs: Vec<Paragraph>) -> Document<Validated> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, hwpforge_core::PageSettings::a4()));
        doc.validate().unwrap()
    }

    #[test]
    fn lossless_contains_frontmatter_and_shapes() {
        let mut draft = Document::new();
        draft.metadata_mut().title = Some("Report".to_string());
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("<hello>", CharShapeIndex::new(5))],
                ParaShapeIndex::new(3),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let doc = draft.validate().unwrap();

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("title: Report"));
        assert!(out.contains("data-para-shape=\"3\""));
        assert!(out.contains("data-char-shape=\"5\""));
        assert!(out.contains("data-width-unit=\"59528\""));
        assert!(out.contains("&lt;hello&gt;"));
    }

    #[test]
    fn lossless_encodes_table_markup() {
        let table = hwpforge_core::Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("A", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(20.0).unwrap(),
        )])]);

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(2))],
            ParaShapeIndex::new(1),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<table data-char-shape=\"2\">"));
        assert!(out.contains("<td data-col-span=\"1\" data-row-span=\"1\""));
    }

    #[test]
    fn lossless_table_cell_preserves_nested_hyperlink_markup() {
        let table = hwpforge_core::Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::Hyperlink {
                        text: "Rust".to_string(),
                        url: "https://www.rust-lang.org".to_string(),
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(20.0).unwrap(),
        )])]);

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(2))],
            ParaShapeIndex::new(1),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(
            out.contains("<a data-char-shape=\"0\" href=\"https://www.rust-lang.org\">Rust</a>")
        );
    }

    #[test]
    fn lossless_escapes_unknown_image_format_in_attribute() {
        let image = Image::new(
            "img.custom",
            HwpUnit::from_mm(10.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
            ImageFormat::Unknown("bad\"fmt".to_string()),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-format=\"bad&quot;fmt\""));
    }

    #[test]
    fn lossless_textbox_preserves_nested_paragraph_markup() {
        let textbox_paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "Rust".to_string(),
                    url: "https://www.rust-lang.org".to_string(),
                },
                CharShapeIndex::new(7),
            )],
            ParaShapeIndex::new(5),
        );

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    frame: hwpforge_core::ObjectFrame::new(),
                    text_margin: None,
                    paragraphs: vec![textbox_paragraph],
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(3),
            )],
            ParaShapeIndex::new(1),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<textbox data-char-shape=\"3\""));
        assert!(out.contains("<p data-para-shape=\"5\">"));
        assert!(
            out.contains("<a data-char-shape=\"7\" href=\"https://www.rust-lang.org\">Rust</a>")
        );
    }

    #[test]
    fn lossless_footnote_preserves_nested_paragraph_markup() {
        let footnote_paragraph = Paragraph::with_runs(
            vec![Run::text("note", CharShapeIndex::new(9))],
            ParaShapeIndex::new(6),
        );

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote { inst_id: None, paragraphs: vec![footnote_paragraph] },
                CharShapeIndex::new(2),
            )],
            ParaShapeIndex::new(1),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<footnote data-char-shape=\"2\">"));
        assert!(out.contains("<p data-para-shape=\"6\">"));
        assert!(out.contains("<span data-char-shape=\"9\">note</span>"));
    }

    #[test]
    fn escape_html_escapes_all_reserved_chars() {
        let escaped = escape_html("<&>'\"");
        assert_eq!(escaped, "&lt;&amp;&gt;&#39;&quot;");
    }

    #[test]
    fn lossless_encodes_image_run() {
        use hwpforge_core::ImageFormat;
        let image = hwpforge_core::Image::new(
            "path/to/img.png",
            HwpUnit::from_mm(60.0).unwrap(),
            HwpUnit::from_mm(40.0).unwrap(),
            ImageFormat::Png,
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-char-shape=\"1\""));
        assert!(out.contains("src=\"path/to/img.png\""));
        assert!(out.contains("data-format=\"PNG\""));
        assert!(out.contains("data-width-mm=\"60.00\""));
        assert!(out.contains("data-height-mm=\"40.00\""));
    }

    #[test]
    fn lossless_encodes_hyperlink() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "click me".to_string(),
                    url: "https://example.com".to_string(),
                },
                CharShapeIndex::new(3),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains(r#"<a data-char-shape="3" href="https://example.com">click me</a>"#));
    }

    #[test]
    fn lossless_encodes_endnote() {
        let endnote_para = Paragraph::with_runs(
            vec![Run::text("endnote body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Endnote { inst_id: None, paragraphs: vec![endnote_para] },
                CharShapeIndex::new(5),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains(r#"<endnote data-char-shape="5">"#));
        assert!(out.contains("endnote body"));
        assert!(out.contains("</endnote>"));
    }

    #[test]
    fn lossless_encodes_unknown_control() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Unknown {
                    tag: "custom-tag".to_string(),
                    data: Some("some data".to_string()),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains(r#"data-kind="custom-tag""#));
        assert!(out.contains("some data"));
        assert!(out.contains("<control"));
        assert!(out.contains("</control>"));
    }

    #[test]
    fn lossless_encodes_unknown_control_with_no_data() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Unknown { tag: "empty".to_string(), data: None },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains(r#"data-kind="empty""#));
        assert!(out.contains("<control"));
    }

    #[test]
    fn lossless_encodes_line_shape() {
        use hwpforge_core::control::ShapePoint;
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Line {
                    frame: hwpforge_core::ObjectFrame::new(),
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 500),
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(25.0).unwrap(),
                    placement: None,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<line"));
        assert!(out.contains("data-start-x=\"0\""));
        assert!(out.contains("data-start-y=\"0\""));
        assert!(out.contains("data-end-x=\"1000\""));
        assert!(out.contains("data-end-y=\"500\""));
    }

    #[test]
    fn lossless_encodes_ellipse_shape() {
        use hwpforge_core::control::ShapePoint;
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    frame: hwpforge_core::ObjectFrame::new(),
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<ellipse"));
        assert!(out.contains("data-cx=\"500\""));
        assert!(out.contains("data-cy=\"300\""));
        // Without text it is an empty element (W3).
        assert!(out.contains(" />") && !out.contains("</ellipse>"));
    }

    #[test]
    fn lossless_encodes_ellipse_with_inner_paragraphs() {
        use hwpforge_core::control::ShapePoint;
        let inner_para = Paragraph::with_runs(
            vec![Run::text("shape text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    frame: hwpforge_core::ObjectFrame::new(),
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    paragraphs: vec![inner_para],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("shape text"));
    }

    #[test]
    fn lossless_encodes_polygon_shape() {
        use hwpforge_core::control::ShapePoint;
        let vertices =
            vec![ShapePoint::new(0, 1000), ShapePoint::new(500, 0), ShapePoint::new(1000, 1000)];
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Polygon {
                    frame: hwpforge_core::ObjectFrame::new(),
                    vertices,
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(30.0).unwrap(),
                    placement: None,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<polygon"));
        assert!(out.contains("data-points=\"0,1000;500,0;1000,1000\""));
        // Without text it is an empty element (W3).
        assert!(out.contains(" />") && !out.contains("</polygon>"));
    }

    #[test]
    fn lossless_encodes_table_with_row_height() {
        use hwpforge_core::TableRow;
        let table = hwpforge_core::Table::new(vec![TableRow::with_height(
            vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("cell", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::from_mm(40.0).unwrap(),
            )],
            HwpUnit::from_mm(15.0).unwrap(),
        )]);

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-height-mm=\"15.00\""));
        assert!(out.contains("data-height-unit="));
    }

    #[test]
    fn lossless_encodes_table_with_caption() {
        use hwpforge_core::{caption::Caption, TableRow};
        let table = hwpforge_core::Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("data", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(40.0).unwrap(),
        )])])
        .with_caption(Caption::new(
            vec![Paragraph::with_runs(
                vec![Run::text("My Caption", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::caption::CaptionSide::Bottom,
        ));

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-caption=\"My Caption\""));
    }

    #[test]
    fn lossless_encodes_table_with_width() {
        use hwpforge_core::TableRow;
        let table = hwpforge_core::Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("data", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(40.0).unwrap(),
        )])])
        .with_width(HwpUnit::from_mm(120.0).unwrap());

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-width-mm=\"120.00\""));
    }

    #[test]
    fn lossless_encodes_cell_with_background_color() {
        use hwpforge_core::TableRow;
        use hwpforge_foundation::Color;
        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("colored", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(40.0).unwrap(),
        )
        .with_background(Color::from_rgb(255, 0, 0));

        let table = hwpforge_core::Table::new(vec![TableRow::new(vec![cell])]);

        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("data-background="));
    }

    /// 이것을 실패시키는 것: 범위 경계를 버리고 lossless 성공을 반환하기.
    #[test]
    fn lossless_field_boundaries_are_written() {
        use hwpforge_core::control::{FieldBoundary, FieldStart};
        let mut start = FieldStart::default();
        start.id = "7".into();
        start.kind = "FORMULA".into();
        let cs = CharShapeIndex::new(0);
        let doc = frame_doc(vec![
            Run::control(
                Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                cs,
            ),
            Run::text("3", cs),
            Run::control(
                Control::FieldBoundary {
                    boundary: FieldBoundary::End { begin_id: "7".into(), field_id: String::new() },
                },
                cs,
            ),
        ]);
        assert!(encode_lossless(&doc).unwrap().contains("<field-start"));
    }

    #[test]
    fn lossless_unsupported_control_returns_error() {
        // An equation has no lossless grammar (an arc has one since W4).
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(Control::equation("a over b"), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let result = encode_lossless(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn lossless_encodes_dutmal_control() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Dutmal {
                    main_text: "한글".to_string(),
                    sub_text: "hangeul".to_string(),
                    sz_ratio: 50,
                    position: DutmalPosition::Top,
                    align: DutmalAlign::Center,
                    metadata: hwpforge_core::DutmalMetadata::default(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<dutmal"));
        assert!(out.contains("data-sz-ratio=\"50\""));
        assert!(out.contains("한글|hangeul"));
        assert!(out.contains("</dutmal>"));
    }

    #[test]
    fn lossless_encodes_compose_control() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Compose {
                    compose_text: "㊀".to_string(),
                    circle_type: "CIRCLE".to_string(),
                    char_sz: -3,
                    compose_type: "COMPOSED".to_string(),
                    char_pr_ids: vec![u32::MAX; 10],
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("<compose"));
        assert!(out.contains("data-circle-type=\"CIRCLE\""));
        assert!(out.contains("data-char-sz=\"-3\""));
        assert!(out.contains("㊀"));
        assert!(out.contains("</compose>"));
    }

    #[test]
    fn lossless_no_frontmatter_adds_format_key() {
        // Document with no metadata should add format=lossless to frontmatter
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        assert!(out.contains("format: lossless"));
    }

    #[test]
    fn lossless_section_encodes_unit_and_mm_attributes() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let out = encode_lossless(&doc).unwrap();
        // Should contain both -unit and -mm variants
        assert!(out.contains("data-width-unit="));
        assert!(out.contains("data-width-mm="));
        assert!(out.contains("data-height-unit="));
        assert!(out.contains("data-height-mm="));
        assert!(out.contains("data-margin-left-unit="));
        assert!(out.contains("data-margin-left-mm="));
    }

    #[test]
    fn lossless_roundtrip_text() {
        // Verify encode → structural check: key attributes present in output
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("hello world", CharShapeIndex::new(2))],
            ParaShapeIndex::new(1),
        )]);

        let encoded = encode_lossless(&doc).unwrap();
        assert!(encoded.contains("data-para-shape=\"1\""));
        assert!(encoded.contains("data-char-shape=\"2\""));
        assert!(encoded.contains("hello world"));
        assert!(encoded.contains("<section"));
        assert!(encoded.contains("</section>"));
    }

    #[test]
    fn lossless_roundtrip_hyperlink() {
        // Verify encode produces decodable hyperlink markup
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "Example".to_string(),
                    url: "https://example.com".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let encoded = encode_lossless(&doc).unwrap();
        assert!(encoded.contains(r#"href="https://example.com""#));
        assert!(encoded.contains("Example"));
        assert!(encoded.contains("<a "));
        assert!(encoded.contains("</a>"));
    }

    #[test]
    fn lossless_roundtrip_footnote() {
        // Verify encode produces decodable footnote markup
        let note_para = Paragraph::with_runs(
            vec![Run::text("note content", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote { inst_id: None, paragraphs: vec![note_para] },
                CharShapeIndex::new(1),
            )],
            ParaShapeIndex::new(0),
        )]);

        let encoded = encode_lossless(&doc).unwrap();
        assert!(encoded.contains("<footnote "));
        assert!(encoded.contains("note content"));
        assert!(encoded.contains("</footnote>"));
    }

    /// A picture's link survives lossless Markdown (and an `&` in it is
    /// escaped); a picture without one writes no attribute.
    ///
    /// 이것을 실패시키는 것: 인코더의 `data-hyperlink` 방출 삭제 / 디코더의 읽기 삭제
    /// (각각 따로).
    #[test]
    fn lossless_keeps_a_picture_hyperlink() {
        let w = HwpUnit::new(1000).unwrap();
        let mut linked = Image::new("BinData/a.png", w, w, ImageFormat::Png);
        linked.hyperlink = Some("http://www.korea.kr/?a=1&b=2".to_string());
        let plain = Image::new("BinData/b.png", w, w, ImageFormat::Png);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![
                Run::image(linked, CharShapeIndex::new(0)),
                Run::image(plain, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )]);
        let md = encode_lossless(&doc).unwrap();
        assert_eq!(md.matches("data-hyperlink=").count(), 1, "{md}");
        let back = crate::MdDecoder::decode_lossless(&md).unwrap();
        let links: Vec<Option<String>> = back.sections()[0].paragraphs[0]
            .runs
            .iter()
            .filter_map(|r| r.content.as_image().map(|i| i.hyperlink.clone()))
            .collect();
        assert_eq!(links, [Some("http://www.korea.kr/?a=1&b=2".to_string()), None]);
    }
}
