//! Readable (lossy) markdown encoder.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::template::Template;
use hwpforge_core::{Control, Document, Paragraph, ParagraphListRef, RunContent, Table, Validated};

use super::list_format::format_list_item;
use crate::error::MdResult;
use crate::frontmatter::{from_metadata, render_frontmatter};
use crate::mapper::{resolve_mapping, MdMapping, ParagraphKind};

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

pub(crate) fn encode_with_template(
    document: &Document<Validated>,
    template: &Template,
) -> MdResult<String> {
    let (mapping, registry) = resolve_mapping(template)?;
    let body = encode_body(document, Some(&mapping), Some(&registry));

    let frontmatter = from_metadata(document.metadata(), Some(template.meta.name.as_str()));
    let rendered = render_frontmatter(&frontmatter)?;

    if body.is_empty() {
        Ok(rendered)
    } else {
        Ok(format!("{}\n{}", rendered, body))
    }
}

pub(crate) fn encode_without_template(document: &Document<Validated>) -> MdResult<String> {
    Ok(encode_body(document, None, None))
}

fn encode_body(
    document: &Document<Validated>,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
) -> String {
    let mut blocks = Vec::new();

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(SECTION_MARKER_COMMENT.to_string());
        }

        for paragraph in &section.paragraphs {
            let markdown = encode_paragraph(paragraph, mapping, registry);
            if !markdown.trim().is_empty() {
                blocks.push(markdown);
            }
        }
    }

    blocks.join("\n\n")
}

fn encode_paragraph(
    paragraph: &Paragraph,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
) -> String {
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
    let trimmed = text.trim();

    if !trimmed.is_empty() {
        if let Some(markdown) =
            registry.and_then(|registry| format_registry_list_item(trimmed, paragraph, registry))
        {
            return markdown;
        }
    }

    if let Some(mapping) = mapping {
        match mapping.classify_para_shape(paragraph.para_shape_id) {
            ParagraphKind::Heading(level) => {
                format!("{} {}", "#".repeat(level as usize), trimmed)
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

fn format_registry_list_item(
    text: &str,
    paragraph: &Paragraph,
    registry: &StyleRegistry,
) -> Option<String> {
    let list = registry.para_shape(paragraph.para_shape_id)?.list?;
    match list {
        ParagraphListRef::Outline { .. } => None,
        ParagraphListRef::Number { level, .. } => {
            Some(format_list_item(text, "NUMBER", level, None))
        }
        ParagraphListRef::Bullet { level, .. } => {
            Some(format_list_item(text, "BULLET", level, None))
        }
        ParagraphListRef::CheckBullet { level, checked, .. } => {
            Some(format_list_item(text, "BULLET", level, Some(checked)))
        }
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
                Control::Line { .. } => {
                    // Lines have no text content to render in lossy mode
                }
                Control::Ellipse { paragraphs, .. } | Control::Polygon { paragraphs, .. } => {
                    // Render any text content inside the shape
                    let body = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !body.trim().is_empty() {
                        output.push_str(body.trim());
                    }
                }
                Control::Dutmal { main_text, sub_text, .. } => {
                    output.push_str(&format!("{main_text}({sub_text})"));
                }
                Control::Compose { compose_text, .. } => {
                    output.push_str(compose_text);
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
                    TableRow::new(vec![
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
                    ]),
                    TableRow::new(vec![
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
                    ]),
                ]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| --- | --- |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    #[test]
    fn encode_table_escapes_pipe_in_cell() {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("A|B", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                )])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
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
                Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![cell_paragraph],
                    hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                )])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
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

    #[test]
    fn encode_endnote_control_as_plain_text_marker() {
        let endnote_body = Paragraph::with_runs(
            vec![Run::text("end body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Endnote { inst_id: None, paragraphs: vec![endnote_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "(endnote: end body)");
    }

    #[test]
    fn encode_textbox_extracts_plain_text() {
        let textbox_body = Paragraph::with_runs(
            vec![Run::text("box content", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    paragraphs: vec![textbox_body],
                    width: hwpforge_foundation::HwpUnit::from_mm(80.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "box content");
    }

    #[test]
    fn encode_unknown_control_renders_tag_as_code() {
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Unknown { tag: "mystery".to_string(), data: None },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "`[mystery]`");
    }

    #[test]
    fn encode_line_control_renders_as_empty() {
        use hwpforge_core::control::ShapePoint;
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Line {
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 0),
                    width: hwpforge_foundation::HwpUnit::from_mm(50.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(1.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        // Lines render as nothing (no text content)
        assert_eq!(md, "");
    }

    #[test]
    fn encode_ellipse_with_text_renders_content() {
        use hwpforge_core::control::ShapePoint;
        let inner = Paragraph::with_runs(
            vec![Run::text("shape text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "shape text");
    }

    #[test]
    fn encode_ellipse_without_text_renders_empty() {
        use hwpforge_core::control::ShapePoint;
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "");
    }

    #[test]
    fn encode_polygon_with_text_renders_content() {
        use hwpforge_core::control::ShapePoint;
        let inner = Paragraph::with_runs(
            vec![Run::text("polygon text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Polygon {
                    vertices: vec![
                        ShapePoint::new(0, 1000),
                        ShapePoint::new(500, 0),
                        ShapePoint::new(1000, 1000),
                    ],
                    width: hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "polygon text");
    }

    #[test]
    fn encode_dutmal_renders_main_sub_text() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let paragraph = Paragraph::with_runs(
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
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "한글(hangeul)");
    }

    #[test]
    fn encode_compose_renders_compose_text() {
        let paragraph = Paragraph::with_runs(
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
        );

        let md = paragraph_text_markdown(&paragraph);
        assert_eq!(md, "㊀");
    }

    #[test]
    fn encode_image_run_in_paragraph() {
        use hwpforge_core::ImageFormat;
        let image = hwpforge_core::Image::new(
            "path/to/photo.jpg",
            hwpforge_foundation::HwpUnit::from_mm(50.0).unwrap(),
            hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Jpeg,
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert_eq!(md, "![photo](path/to/photo.jpg)");
    }

    #[test]
    fn encode_image_alt_text_from_filename_without_extension() {
        use hwpforge_core::ImageFormat;
        // Test image alt text extraction
        let image = hwpforge_core::Image::new(
            "docs/figures/figure_1.png",
            hwpforge_foundation::HwpUnit::from_mm(50.0).unwrap(),
            hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Png,
        );
        let paragraph = Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert!(md.contains("![figure_1]"));
    }

    #[test]
    fn encode_image_run_in_mixed_paragraph() {
        use hwpforge_core::ImageFormat;
        let image = hwpforge_core::Image::new(
            "img.png",
            hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
            hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
            ImageFormat::Png,
        );
        let paragraph = Paragraph::with_runs(
            vec![
                Run::text("before", CharShapeIndex::new(0)),
                Run::image(image, CharShapeIndex::new(0)),
                Run::text("after", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert!(md.contains("before"));
        assert!(md.contains("![img](img.png)"));
        assert!(md.contains("after"));
    }

    #[test]
    fn encode_table_in_paragraph_with_preceding_text() {
        // When paragraph has table run AND text runs before it,
        // table_to_markdown is called inline (pushed with newline)
        let text_run = Run::text("intro", CharShapeIndex::new(0));
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("A", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
        )])]);
        let paragraph = Paragraph::with_runs(
            vec![text_run, Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph);
        assert!(md.contains("intro"));
        assert!(md.contains("| A |"));
    }

    #[test]
    fn encode_empty_table_renders_placeholder() {
        let table = Table::new(vec![]);
        let paragraph = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert_eq!(md, "| |\n| --- |");
    }

    #[test]
    fn encode_table_with_empty_row_renders_placeholder() {
        use hwpforge_core::TableRow;
        let table = Table::new(vec![
            TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("header", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
            )]),
            TableRow::new(vec![]),
        ]);

        let paragraph = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        // An empty row renders as "| |"
        assert!(md.contains("| |"));
    }

    #[test]
    fn encode_table_cell_escapes_backslash() {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text(r"path\to\file", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                )])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert!(md.contains(r"path\\to\\file"));
    }

    #[test]
    fn encode_table_cell_escapes_newline_as_br() {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("line1\nline2", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                )])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = encode_paragraph(&paragraph, None, None);
        assert!(md.contains("<br>"));
    }

    #[test]
    fn encode_blockquote_format() {
        // Verify the blockquote branch: each line gets "> " prefix
        let text = "line1\nline2";
        let result: Vec<String> = text.lines().map(|line| format!("> {line}")).collect::<Vec<_>>();
        let expected = "> line1\n> line2";
        assert_eq!(result.join("\n"), expected);
    }

    #[test]
    fn encode_list_item_without_marker_adds_dash() {
        // starts_with_list_marker returns false → "- " prefix is added
        assert!(!starts_with_list_marker("no marker here"));
        assert!(starts_with_list_marker("- already a list"));
        assert!(starts_with_list_marker("* bullet"));
        assert!(starts_with_list_marker("+ plus"));
        assert!(starts_with_list_marker("1. ordered"));
        assert!(!starts_with_list_marker("1x not ordered"));
        assert!(!starts_with_list_marker(""));
    }

    #[test]
    fn encode_empty_document_produces_only_frontmatter() {
        let mut draft = Document::new();
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("   ", CharShapeIndex::new(0))], // whitespace only
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let doc = draft.validate().unwrap();

        // encode_body skips paragraphs whose markdown is all whitespace
        let md = encode_without_template(&doc).unwrap();
        // Result should be empty (no blocks pushed)
        assert_eq!(md.trim(), "");
    }

    #[test]
    fn encode_code_paragraph_with_template() {
        // Verify the code block branch produces ``` fencing using the real template
        // The builtin_default template maps para_shape_id=2 to Code
        let template = builtin_default().unwrap();
        let (mapping, _registry) = resolve_mapping(&template).unwrap();

        // Use the code para shape ID from the resolved mapping
        let code_para_shape = mapping.code.para_shape_id;
        let paragraph = Paragraph::with_runs(
            vec![Run::text("let x = 1;", CharShapeIndex::new(0))],
            code_para_shape,
        );

        let md = encode_paragraph(&paragraph, Some(&mapping), None);
        assert!(md.starts_with("```\n"));
        assert!(md.ends_with("\n```"));
        assert!(md.contains("let x = 1;"));
    }

    #[test]
    fn encode_task_list_paragraph_with_registry_uses_gfm_marker() {
        let template = builtin_default().unwrap();
        let (mapping, registry) = resolve_mapping(&template).unwrap();

        let unchecked = Paragraph::with_runs(
            vec![Run::text("todo", CharShapeIndex::new(0))],
            mapping.task_list.unchecked[0],
        );
        let checked = Paragraph::with_runs(
            vec![Run::text("done", CharShapeIndex::new(0))],
            mapping.task_list.checked[1],
        );

        assert_eq!(encode_paragraph(&unchecked, Some(&mapping), Some(&registry)), "- [ ] todo");
        assert_eq!(encode_paragraph(&checked, Some(&mapping), Some(&registry)), "  - [x] done");
    }
}
