//! Lossless markdown encoder (frontmatter + HTML-like body).

use hwpforge_core::{Control, Document, Paragraph, RunContent, Table, Validated};

use crate::error::{MdError, MdResult};
use crate::frontmatter::{from_metadata, render_frontmatter};

pub(crate) fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
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

fn encode_paragraph(paragraph: &Paragraph) -> MdResult<String> {
    let mut out = format!("<p data-para-shape=\"{}\">", paragraph.para_shape_id.get());

    for run in &paragraph.runs {
        match &run.content {
            RunContent::Text(text) => {
                out.push_str(&format!(
                    "<span data-char-shape=\"{}\">{}</span>",
                    run.char_shape_id.get(),
                    escape_html(text)
                ));
            }
            RunContent::Image(image) => {
                out.push_str(&format!(
                    "<img data-char-shape=\"{}\" src=\"{}\" alt=\"{}\" data-format=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\" />",
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
                Control::TextBox { paragraphs, width, height, .. } => {
                    out.push_str(&format!(
                        "<textbox data-char-shape=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\">",
                        run.char_shape_id.get(),
                        width.as_i32(),
                        height.as_i32(),
                        width.to_mm(),
                        height.to_mm()
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
                Control::Line { start, end, width, height, .. } => {
                    out.push_str(&format!(
                        "<line data-char-shape=\"{}\" data-start-x=\"{}\" data-start-y=\"{}\" data-end-x=\"{}\" data-end-y=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\"/>",
                        run.char_shape_id.get(),
                        start.x, start.y, end.x, end.y,
                        width.as_i32(), height.as_i32()
                    ));
                }
                Control::Ellipse { center, axis1, axis2, width, height, paragraphs, .. } => {
                    out.push_str(&format!(
                        "<ellipse data-char-shape=\"{}\" data-cx=\"{}\" data-cy=\"{}\" data-ax1-x=\"{}\" data-ax1-y=\"{}\" data-ax2-x=\"{}\" data-ax2-y=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\">",
                        run.char_shape_id.get(),
                        center.x, center.y, axis1.x, axis1.y, axis2.x, axis2.y,
                        width.as_i32(), height.as_i32()
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</ellipse>");
                }
                Control::Polygon { vertices, width, height, paragraphs, .. } => {
                    let pts: Vec<String> =
                        vertices.iter().map(|v| format!("{},{}", v.x, v.y)).collect();
                    out.push_str(&format!(
                        "<polygon data-char-shape=\"{}\" data-points=\"{}\" data-width-unit=\"{}\" data-height-unit=\"{}\">",
                        run.char_shape_id.get(),
                        pts.join(";"),
                        width.as_i32(), height.as_i32()
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</polygon>");
                }
                Control::Dutmal { main_text, sub_text, sz_ratio, position, align } => {
                    out.push_str(&format!(
                        "<dutmal data-char-shape=\"{}\" data-sz-ratio=\"{}\" data-position=\"{:?}\" data-align=\"{:?}\">{}</dutmal>",
                        run.char_shape_id.get(),
                        sz_ratio,
                        position,
                        align,
                        escape_html(&format!("{main_text}|{sub_text}"))
                    ));
                }
                Control::Compose { compose_text, circle_type, char_sz, compose_type } => {
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

#[cfg(test)]
mod tests {
    use super::*;
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
                    paragraphs: vec![textbox_paragraph],
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
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
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 500),
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(25.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
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
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
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
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![inner_para],
                    caption: None,
                    style: None,
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
                    vertices,
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(30.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
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
                    horz_offset: 0,
                    vert_offset: 0,
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
}
