//! Lossless markdown encoder (frontmatter + HTML-like body).

use hwpforge_core::{Control, Document, Paragraph, RunContent, Table, Validated};

use crate::error::{MdError, MdResult};
use crate::frontmatter::{from_metadata, render_frontmatter};

pub(crate) fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
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

    for (section_index, section) in document.sections().iter().enumerate() {
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

/// An object's outer margin (S18) has no place in the grammar yet (P5b):
/// refused rather than dropped.
/// 이것을 실패시키는 것: 이 거부를 지우기(`object_outer_margin_is_refused`).
fn refuse_out_margin(
    placement: Option<&hwpforge_core::placement::ObjectPlacement>,
) -> MdResult<()> {
    if placement
        .and_then(|p| p.out_margin)
        .is_some_and(|m| [m.left, m.right, m.top, m.bottom].iter().any(|v| v.as_i32() != 0))
    {
        return Err(MdError::UnsupportedStructure {
            detail: "object outer margin is not carried by the lossless encoder".to_string(),
        });
    }
    Ok(())
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
                refuse_out_margin(image.placement.as_ref())?;
                let frame = frame_attrs(&image.frame);
                // The picture's link, when it has one (`Image::hyperlink`).
                let link = image
                    .hyperlink
                    .as_deref()
                    .map(|url| format!(" data-hyperlink=\"{}\"", escape_html(url)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "<img data-char-shape=\"{}\" src=\"{}\" alt=\"{}\" data-format=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\"{frame}{link} />",
                    run.char_shape_id.get(),
                    escape_html(&image.path),
                    escape_html(&image.path),
                    escape_html(&image.format.to_string()),
                    image.width.as_i32(),
                    image.height.as_i32(),
                    image.width.to_mm(),
                    image.height.to_mm()
                ));
            }
            RunContent::Control(control) => match control.as_ref() {
                Control::Hyperlink { text, url } => {
                    out.push_str(&format!(
                        "<a data-char-shape=\"{}\" href=\"{}\">{}</a>",
                        run.char_shape_id.get(),
                        escape_html(url),
                        escape_html(text)
                    ));
                }
                Control::TextBox {
                    paragraphs,
                    width,
                    height,
                    style,
                    text_margin,
                    placement,
                    frame,
                    ..
                } => {
                    refuse_out_margin(placement.as_ref())?;
                    // §19.22 D4: nor for a text margin other than 283 (P5b).
                    // 이것을 실패시키는 것: 이 거부를 지우기
                    // (`text_box_margin_is_refused`).
                    if text_margin.is_some_and(|m| {
                        [m.left, m.right, m.top, m.bottom].iter().any(|v| v.as_i32() != 283)
                    }) {
                        return Err(MdError::UnsupportedStructure {
                            detail: "text box text margin is not carried by the lossless encoder"
                                .to_string(),
                        });
                    }
                    // §19.5 1c (review r1): the lossless grammar has no place
                    // for a rounded corner yet (P5b) — refuse rather than
                    // write a square box. 이것을 실패시키는 것: 이 거부를 지우기
                    // (`rounded_text_box_is_refused`).
                    if style.as_ref().is_some_and(|s| s.corner_ratio.is_some()) {
                        return Err(MdError::UnsupportedStructure {
                            detail: "text box corner ratio is not carried by the lossless encoder"
                                .to_string(),
                        });
                    }
                    out.push_str(&format!(
                        "<textbox data-char-shape=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\"{}>",
                        run.char_shape_id.get(),
                        width.as_i32(),
                        height.as_i32(),
                        width.to_mm(),
                        height.to_mm(),
                        frame_attrs(frame)
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</textbox>");
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
                        escape_html(tag),
                        escape_html(data.as_deref().unwrap_or(""))
                    ));
                }
                Control::Line { start, end, width, height, frame, .. } => {
                    out.push_str(&format!(
                        "<line data-char-shape=\"{}\" data-start-x=\"{}\" data-start-y=\"{}\" data-end-x=\"{}\" data-end-y=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\"{}/>",
                        run.char_shape_id.get(),
                        start.x, start.y, end.x, end.y,
                        width.as_i32(), height.as_i32(),
                        frame_attrs(frame)
                    ));
                }
                Control::Ellipse {
                    center, axis1, axis2, width, height, paragraphs, frame, ..
                } => {
                    out.push_str(&format!(
                        "<ellipse data-char-shape=\"{}\" data-cx=\"{}\" data-cy=\"{}\" data-ax1-x=\"{}\" data-ax1-y=\"{}\" data-ax2-x=\"{}\" data-ax2-y=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\"{}>",
                        run.char_shape_id.get(),
                        center.x, center.y, axis1.x, axis1.y, axis2.x, axis2.y,
                        width.as_i32(), height.as_i32(),
                        frame_attrs(frame)
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</ellipse>");
                }
                Control::Polygon { vertices, width, height, paragraphs, frame, .. } => {
                    let pts: Vec<String> =
                        vertices.iter().map(|v| format!("{},{}", v.x, v.y)).collect();
                    out.push_str(&format!(
                        "<polygon data-char-shape=\"{}\" data-points=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\"{}>",
                        run.char_shape_id.get(),
                        pts.join(";"),
                        width.as_i32(), height.as_i32(),
                        frame_attrs(frame)
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</polygon>");
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
    if let Some(width) = table.width {
        out.push_str(&format!(
            " data-width-unit=\"{}\" data-width-mm=\"{:.2}\"",
            width.as_i32(),
            width.to_mm()
        ));
    }
    if let Some(caption) = table.caption.as_ref() {
        let text: String = caption
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| r.content.as_text())
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            out.push_str(&format!(" data-caption=\"{}\"", escape_html(&text)));
        }
    }
    out.push('>');

    for row in &table.rows {
        if let Some(height) = row.height {
            out.push_str(&format!(
                "<tr data-height-unit=\"{}\" data-height-mm=\"{:.2}\">",
                height.as_i32(),
                height.to_mm()
            ));
        } else {
            out.push_str("<tr>");
        }
        for cell in &row.cells {
            // §19.22 D3: the lossless grammar has no place for a cell's line
            // wrap yet (P5b) — refuse rather than write a breaking cell.
            // 이것을 실패시키는 것: 이 거부를 지우기(`squeezed_cell_is_refused`).
            if cell.line_wrap != hwpforge_core::table::CellLineWrap::Break {
                return Err(MdError::UnsupportedStructure {
                    detail: "cell line wrap is not carried by the lossless encoder".to_string(),
                });
            }
            out.push_str(&format!(
                "<td data-col-span=\"{}\" data-row-span=\"{}\" data-width-unit=\"{}\" data-width-mm=\"{:.2}\"",
                cell.col_span,
                cell.row_span,
                cell.width.as_i32(),
                cell.width.to_mm()
            ));
            if let Some(background) = cell.background {
                out.push_str(&format!(" data-background=\"{}\"", background));
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
    fn rounded_text_box_is_refused() {
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
        assert!(matches!(
            encode_lossless(&doc(Some(20))),
            Err(MdError::UnsupportedStructure { .. })
        ));
        assert!(encode_lossless(&doc(None)).is_ok());
    }
    /// S18: a picture or a text box with an outer margin is refused; without
    /// one it is written.
    ///
    /// 이것을 실패시키는 것: 그림·글상자 거부를 각각 지우기.
    #[test]
    fn object_outer_margin_is_refused() {
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
            let image = Image::new("BinData/a.png", unit, unit, ImageFormat::Png)
                .with_placement(with_margin(bottom));
            let r = encode_lossless(&doc(Run::image(image, CharShapeIndex::new(0))));
            assert_eq!(
                matches!(r, Err(MdError::UnsupportedStructure { .. })),
                bottom != 0,
                "image"
            );
            let inner = Paragraph::with_runs(
                vec![Run::text("box", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let mut tb = Control::text_box(vec![inner], unit, unit);
            if let Control::TextBox { placement, .. } = &mut tb {
                *placement = Some(with_margin(bottom));
            }
            let r = encode_lossless(&doc(Run::control(tb, CharShapeIndex::new(0))));
            assert_eq!(
                matches!(r, Err(MdError::UnsupportedStructure { .. })),
                bottom != 0,
                "text box"
            );
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

    /// Lines, ellipses and polygons write their frame (the decoder reads
    /// these tags only from WS W3 on, so the document still fails to decode
    /// — no silent loss). A line shown flat with a flat original box is a
    /// valid frame (Core, built directly).
    #[test]
    fn write_only_shapes_carry_the_frame() {
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
                r#" data-original-width-unit="1000" data-original-height-unit="0" data-original-width-mm="3.53" data-original-height-mm="0.00" data-size-protect="true"/>"#,
            ),
            (
                ellipse,
                r#" data-original-width-unit="560" data-original-height-unit="560" data-original-width-mm="1.98" data-original-height-mm="1.98" data-size-protect="true">"#,
            ),
            (
                polygon,
                r#" data-original-width-unit="100" data-original-height-unit="100" data-original-width-mm="0.35" data-original-height-mm="0.35">"#,
            ),
        ] {
            let md =
                encode_lossless(&frame_doc(vec![Run::control(control, CharShapeIndex::new(0))]))
                    .unwrap();
            assert!(md.contains(expected), "{md}");
            assert!(crate::MdDecoder::decode_lossless(&md).is_err(), "{md}");
        }
    }

    /// A text box whose margin is not 283 everywhere is refused (§19.22 D4);
    /// 283 everywhere is written.
    #[test]
    fn text_box_margin_is_refused() {
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
        assert!(matches!(encode_lossless(&doc(400)), Err(MdError::UnsupportedStructure { .. })));
        assert!(encode_lossless(&doc(283)).is_ok());
    }

    /// A squeezed cell is refused (§19.22 D3 — the grammar cannot carry it
    /// yet); a breaking one is written.
    #[test]
    fn squeezed_cell_is_refused() {
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
        assert!(matches!(
            encode_lossless(&doc(CellLineWrap::Squeeze)),
            Err(MdError::UnsupportedStructure { .. })
        ));
        assert!(encode_lossless(&doc(CellLineWrap::Break)).is_ok());
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
        assert!(out.contains("</ellipse>"));
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
        assert!(out.contains("</polygon>"));
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
        .with_caption(Caption {
            paragraphs: vec![Paragraph::with_runs(
                vec![Run::text("My Caption", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            ..Caption::default()
        });

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

    #[test]
    fn lossless_unsupported_control_returns_error() {
        use hwpforge_core::control::ShapePoint;
        use hwpforge_foundation::{ArcType, HwpUnit};
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Arc {
                    frame: hwpforge_core::ObjectFrame::new(),
                    arc_type: ArcType::Pie,
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    start1: ShapePoint::new(800, 100),
                    end1: ShapePoint::new(200, 100),
                    start2: ShapePoint::new(100, 400),
                    end2: ShapePoint::new(900, 400),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
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
