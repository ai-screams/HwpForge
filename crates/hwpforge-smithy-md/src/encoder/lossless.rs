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
                Control::TextBox { paragraphs, width, height } => {
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
                Control::Footnote { paragraphs } => {
                    out.push_str(&format!(
                        "<footnote data-char-shape=\"{}\">",
                        run.char_shape_id.get()
                    ));
                    for paragraph in paragraphs {
                        out.push_str(&encode_paragraph(paragraph)?);
                    }
                    out.push_str("</footnote>");
                }
                Control::Unknown { tag, data } => {
                    out.push_str(&format!(
                        "<control data-char-shape=\"{}\" data-kind=\"{}\">{}</control>",
                        run.char_shape_id.get(),
                        escape_html(tag),
                        escape_html(data.as_deref().unwrap_or(""))
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
        out.push_str(&format!(" data-caption=\"{}\"", escape_html(caption)));
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
        let table = hwpforge_core::Table::new(vec![TableRow {
            cells: vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("A", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::from_mm(20.0).unwrap(),
            )],
            height: None,
        }]);

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
        let table = hwpforge_core::Table::new(vec![TableRow {
            cells: vec![TableCell::new(
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
            )],
            height: None,
        }]);

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
                Control::Footnote { paragraphs: vec![footnote_paragraph] },
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
}
