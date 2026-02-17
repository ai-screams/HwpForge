//! Readable (lossy) markdown encoder.

use hwpforge_blueprint::template::Template;
use hwpforge_core::{Control, Document, Paragraph, RunContent, Table, Validated};

use crate::error::MdResult;
use crate::frontmatter::{from_metadata, render_frontmatter};
use crate::mapper::{resolve_mapping, MdMapping, ParagraphKind};

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

pub(crate) fn encode_with_template(
    document: &Document<Validated>,
    template: &Template,
) -> MdResult<String> {
    let (mapping, _registry) = resolve_mapping(template)?;
    let body = encode_body(document, Some(&mapping));

    let frontmatter = from_metadata(document.metadata(), Some(template.meta.name.as_str()));
    let rendered = render_frontmatter(&frontmatter)?;

    if body.is_empty() {
        Ok(rendered)
    } else {
        Ok(format!("{}\n{}", rendered, body))
    }
}

pub(crate) fn encode_without_template(document: &Document<Validated>) -> MdResult<String> {
    Ok(encode_body(document, None))
}

fn encode_body(document: &Document<Validated>, mapping: Option<&MdMapping>) -> String {
    let mut blocks = Vec::new();

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(SECTION_MARKER_COMMENT.to_string());
        }

        for paragraph in &section.paragraphs {
            let markdown = encode_paragraph(paragraph, mapping);
            if !markdown.trim().is_empty() {
                blocks.push(markdown);
            }
        }
    }

    blocks.join("\n\n")
}

fn encode_paragraph(paragraph: &Paragraph, mapping: Option<&MdMapping>) -> String {
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(table) => return table_to_markdown(table),
            RunContent::Image(image) => {
                return format!("![{}]({})", image_alt_text(&image.path), image.path);
            }
            _ => {}
        }
    }

    let text = paragraph_text_markdown(paragraph);
    if let Some(mapping) = mapping {
        match mapping.classify_para_shape(paragraph.para_shape_id) {
            ParagraphKind::Heading(level) => {
                format!("{} {}", "#".repeat(level as usize), text.trim())
            }
            ParagraphKind::Code => format!("```\n{}\n```", text),
            ParagraphKind::BlockQuote => {
                text.lines().map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n")
            }
            ParagraphKind::ListItem => {
                let trimmed = text.trim_start();
                if starts_with_list_marker(trimmed) {
                    trimmed.to_string()
                } else {
                    format!("- {trimmed}")
                }
            }
            ParagraphKind::Body => text,
        }
    } else {
        text
    }
}

fn paragraph_text_markdown(paragraph: &Paragraph) -> String {
    let mut output = String::new();

    for run in &paragraph.runs {
        match &run.content {
            RunContent::Text(text) => output.push_str(text),
            RunContent::Image(image) => {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&format!("![{}]({})", image_alt_text(&image.path), image.path));
            }
            RunContent::Control(control) => match control.as_ref() {
                Control::Hyperlink { text, url } => {
                    output.push_str(&format!("[{text}]({url})"));
                }
                Control::Footnote { paragraphs, .. } => {
                    let footnote = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    output.push_str(&format!("(footnote: {})", footnote.trim()));
                }
                Control::Endnote { paragraphs, .. } => {
                    let endnote = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    output.push_str(&format!("(endnote: {})", endnote.trim()));
                }
                Control::TextBox { paragraphs, .. } => {
                    let body = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    output.push_str(body.trim());
                }
                Control::Unknown { tag, .. } => {
                    output.push_str(&format!("`[{tag}]`"));
                }
                _ => {}
            },
            RunContent::Table(table) => {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&table_to_markdown(table));
            }
            _ => {}
        }
    }

    output
}

fn table_to_markdown(table: &Table) -> String {
    if table.rows.is_empty() {
        return "| |\n| --- |".to_string();
    }

    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    let text = cell
                        .paragraphs
                        .iter()
                        .map(paragraph_text_markdown)
                        .collect::<Vec<_>>()
                        .join(" ");
                    escape_gfm_cell(&text)
                })
                .collect()
        })
        .collect();

    let header = rows.first().cloned().unwrap_or_else(|| vec![String::new()]);
    let col_count = header.len().max(1);

    let mut lines = Vec::new();
    lines.push(format!("| {} |", header.join(" | ")));
    lines.push(format!("| {} |", (0..col_count).map(|_| "---").collect::<Vec<_>>().join(" | ")));

    for row in rows.iter().skip(1) {
        if row.is_empty() {
            lines.push("| |".to_string());
        } else {
            lines.push(format!("| {} |", row.join(" | ")));
        }
    }

    lines.join("\n")
}

fn image_alt_text(path: &str) -> String {
    path.rsplit('/').next().and_then(|name| name.split('.').next()).unwrap_or("image").to_string()
}

fn starts_with_list_marker(text: &str) -> bool {
    if text.starts_with("- ") || text.starts_with("* ") || text.starts_with("+ ") {
        return true;
    }

    let mut digits = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        return digits > 0 && ch == '.';
    }

    false
}

fn escape_gfm_cell(input: &str) -> String {
    input.replace('\\', "\\\\").replace('|', "\\|").replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_core::{Document, Paragraph, Run, Section, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn validated_document(paragraphs: Vec<Paragraph>) -> Document<Validated> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, hwpforge_core::PageSettings::a4()));
        doc.validate().unwrap()
    }

    #[test]
    fn encode_without_template_plain_text() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("hello", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let md = encode_without_template(&doc).unwrap();
        assert_eq!(md, "hello");
    }

    #[test]
    fn encode_with_template_adds_frontmatter() {
        let template = builtin_default().unwrap();
        let mut draft = Document::new();
        draft.metadata_mut().title = Some("제안서".to_string());
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("body", CharShapeIndex::new(1))],
                ParaShapeIndex::new(1),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let doc = draft.validate().unwrap();

        let md = encode_with_template(&doc, &template).unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("template: default"));
        assert!(md.contains("title: 제안서"));
    }

    #[test]
    fn encode_heading_when_mapping_available() {
        let template = builtin_default().unwrap();
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("제목", CharShapeIndex::new(3))],
            ParaShapeIndex::new(3),
        )]);

        let md = encode_with_template(&doc, &template).unwrap();
        assert!(md.contains("# 제목"));
    }

    #[test]
    fn encode_link_run() {
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "Rust".to_string(),
                    url: "https://www.rust-lang.org".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "[Rust](https://www.rust-lang.org)");
    }

    #[test]
    fn encode_footnote_control_as_plain_text_marker() {
        let footnote_body = Paragraph::with_runs(
            vec![Run::text("note body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote { inst_id: None, paragraphs: vec![footnote_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "(footnote: note body)");
    }

    #[test]
    fn encode_table_to_gfm() {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![
                    TableRow {
                        cells: vec![
                            TableCell::new(
                                vec![Paragraph::with_runs(
                                    vec![Run::text("A", CharShapeIndex::new(0))],
                                    ParaShapeIndex::new(0),
                                )],
                                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                            ),
                            TableCell::new(
                                vec![Paragraph::with_runs(
                                    vec![Run::text("B", CharShapeIndex::new(0))],
                                    ParaShapeIndex::new(0),
                                )],
                                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                            ),
                        ],
                        height: None,
                    },
                    TableRow {
                        cells: vec![
                            TableCell::new(
                                vec![Paragraph::with_runs(
                                    vec![Run::text("1", CharShapeIndex::new(0))],
                                    ParaShapeIndex::new(0),
                                )],
                                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                            ),
                            TableCell::new(
                                vec![Paragraph::with_runs(
                                    vec![Run::text("2", CharShapeIndex::new(0))],
                                    ParaShapeIndex::new(0),
                                )],
                                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                            ),
                        ],
                        height: None,
                    },
                ]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None);
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| --- | --- |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    #[test]
    fn encode_table_escapes_pipe_in_cell() {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow {
                    cells: vec![TableCell::new(
                        vec![Paragraph::with_runs(
                            vec![Run::text("A|B", CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        )],
                        hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    )],
                    height: None,
                }]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None);
        assert!(md.contains("A\\|B"));
    }

    #[test]
    fn encode_table_cell_preserves_link_markdown() {
        let cell_paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "Rust".to_string(),
                    url: "https://www.rust-lang.org".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow {
                    cells: vec![TableCell::new(
                        vec![cell_paragraph],
                        hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    )],
                    height: None,
                }]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None);
        assert!(md.contains("[Rust](https://www.rust-lang.org)"));
    }

    #[test]
    fn encode_multiple_sections_uses_section_marker_comment() {
        let mut draft = Document::new();
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("first", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("second", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let doc = draft.validate().unwrap();

        let md = encode_without_template(&doc).unwrap();
        assert!(md.contains("<!-- hwpforge:section -->"));
    }
}
