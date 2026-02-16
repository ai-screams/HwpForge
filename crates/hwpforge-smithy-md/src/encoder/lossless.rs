//! Lossless markdown encoder (frontmatter + HTML-like body).

use hwpforge_core::{Control, Document, Paragraph, RunContent, Table, Validated};

use crate::error::MdResult;
use crate::frontmatter::{from_metadata, render_frontmatter};

pub(crate) fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
    let frontmatter = from_metadata(document.metadata(), None);
    let mut output = render_frontmatter(&frontmatter)?;
    output.push('\n');

    for (section_index, section) in document.sections().iter().enumerate() {
        output.push_str(&format!(
            "<section data-index=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\">\n",
            section_index,
            section.page_settings.width.to_mm(),
            section.page_settings.height.to_mm()
        ));

        for paragraph in &section.paragraphs {
            output.push_str(&encode_paragraph(paragraph));
            output.push('\n');
        }

        output.push_str("</section>\n");
    }

    Ok(output)
}

fn encode_paragraph(paragraph: &Paragraph) -> String {
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
                    "<img data-char-shape=\"{}\" src=\"{}\" alt=\"{}\" data-format=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\" />",
                    run.char_shape_id.get(),
                    escape_html(&image.path),
                    escape_html(&image.path),
                    escape_html(&image.format.to_string()),
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
                    let joined = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!(
                        "<textbox data-char-shape=\"{}\" data-width-mm=\"{:.2}\" data-height-mm=\"{:.2}\">{}</textbox>",
                        run.char_shape_id.get(),
                        width.to_mm(),
                        height.to_mm(),
                        escape_html(joined.trim())
                    ));
                }
                Control::Footnote { paragraphs } => {
                    let joined = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!(
                        "<footnote data-char-shape=\"{}\">{}</footnote>",
                        run.char_shape_id.get(),
                        escape_html(joined.trim())
                    ));
                }
                Control::Unknown { tag, data } => {
                    out.push_str(&format!(
                        "<control data-char-shape=\"{}\" data-kind=\"{}\">{}</control>",
                        run.char_shape_id.get(),
                        escape_html(tag),
                        escape_html(data.as_deref().unwrap_or(""))
                    ));
                }
                _ => {}
            },
            RunContent::Table(table) => {
                out.push_str(&encode_table(table, run.char_shape_id.get()));
            }
            _ => {}
        }
    }

    out.push_str("</p>");
    out
}

fn encode_table(table: &Table, char_shape_id: usize) -> String {
    let mut out = format!("<table data-char-shape=\"{}\">", char_shape_id);

    for row in &table.rows {
        out.push_str("<tr>");
        for cell in &row.cells {
            out.push_str(&format!(
                "<td data-col-span=\"{}\" data-row-span=\"{}\" data-width-mm=\"{:.2}\">",
                cell.col_span,
                cell.row_span,
                cell.width.to_mm()
            ));

            for paragraph in &cell.paragraphs {
                out.push_str(&encode_paragraph(paragraph));
            }

            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }

    out.push_str("</table>");
    out
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
    fn escape_html_escapes_all_reserved_chars() {
        let escaped = escape_html("<&>'\"");
        assert_eq!(escaped, "&lt;&amp;&gt;&#39;&quot;");
    }
}
