//! Readable (lossy) markdown encoder.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::template::Template;
use hwpforge_core::{
    Control, Document, ListItemKind, Paragraph, ParagraphListRef, RunContent, Table, Validated,
};

use super::list_format::{format_list_continuation, format_list_item};
use super::Part;
use crate::error::MdResult;
use crate::frontmatter::{from_metadata, render_frontmatter};
use crate::mapper::{resolve_mapping, MdMapping, ParagraphKind};

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

pub(crate) fn encode_with_template(
    document: &Document<Validated>,
    template: &Template,
) -> MdResult<String> {
    encode_with_template_report(document, template).map(|(md, _)| md)
}

pub(crate) fn encode_with_template_report(
    document: &Document<Validated>,
    template: &Template,
) -> MdResult<(String, Vec<super::MdWarning>)> {
    refuse_too_deep(document)?;
    let mut warnings = Vec::new();
    let (mapping, registry) = resolve_mapping(template)?;
    let body = encode_body(document, Some(&mapping), Some(&registry), &mut warnings);

    let frontmatter = from_metadata(document.metadata(), Some(template.meta.name.as_str()));
    let rendered = render_frontmatter(&frontmatter)?;

    if body.is_empty() {
        Ok((rendered, warnings))
    } else {
        Ok((format!("{}\n{}", rendered, body), warnings))
    }
}

pub(crate) fn encode_without_template(document: &Document<Validated>) -> MdResult<String> {
    encode_without_template_report(document).map(|(md, _)| md)
}

pub(crate) fn encode_without_template_report(
    document: &Document<Validated>,
) -> MdResult<(String, Vec<super::MdWarning>)> {
    refuse_too_deep(document)?;
    let mut warnings = Vec::new();
    let body = encode_body(document, None, None, &mut warnings);
    Ok((body, warnings))
}

/// The lossy view recurses through tables and captions; a document nested
/// deeper than the lossless limit is refused before drawing starts.
/// 이것을 실패시키는 것: 이 검사를 지우기(`lossy_refuses_too_deep_nesting`).
fn refuse_too_deep(document: &Document<Validated>) -> MdResult<()> {
    if super::nests_too_deep(document) {
        return Err(crate::error::MdError::UnsupportedStructure {
            detail: format!(
                "structure nests deeper than {} containers",
                crate::lossless_grammar::MAX_NESTING_DEPTH
            ),
        });
    }
    Ok(())
}

fn encode_body(
    document: &Document<Validated>,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
    warnings: &mut Vec<super::MdWarning>,
) -> String {
    let mut blocks = Vec::new();
    let mut continuation_level: Option<u8> = None;

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(SECTION_MARKER_COMMENT.to_string());
            continuation_level = None;
        }

        for (paragraph_index, paragraph) in section.paragraphs.iter().enumerate() {
            let current_continuation_level = mapping
                .and_then(|mapping| mapping.continuation_level(paragraph.para_shape_id))
                .filter(|level| continuation_level == Some(*level));
            let first_new = warnings.len();
            let markdown = encode_paragraph(paragraph, mapping, registry, warnings);
            // Place the depth warnings this paragraph raised.
            // 이것을 실패시키는 것: 이 자리 채우기 삭제(`nested_tables_stay_small`).
            for warning in &mut warnings[first_new..] {
                if let super::MdWarning::ContainerTooDeep { section, paragraph, .. } = warning {
                    *section = section_index;
                    *paragraph = paragraph_index;
                }
            }
            if !markdown.trim().is_empty() {
                if let Some(level) = current_continuation_level {
                    let indented = format_list_continuation(&markdown, level);
                    if let Some(last) = blocks.last_mut() {
                        last.push_str("\n\n");
                        last.push_str(&indented);
                    } else {
                        blocks.push(indented);
                    }
                } else {
                    blocks.push(markdown);
                }
            }

            if current_continuation_level.is_none() {
                continuation_level = registry
                    .and_then(|registry| format_registry_list_level(paragraph, registry))
                    .or_else(|| mapping.and_then(|mapping| mapping_list_level(paragraph, mapping)));
            }
        }
    }

    blocks.join("\n\n")
}

fn encode_paragraph(
    paragraph: &Paragraph,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
    warnings: &mut Vec<super::MdWarning>,
) -> String {
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(table) => {
                return table_to_markdown(table, warnings, 0)
                    + &lossy_caption(table.caption.as_ref(), warnings, 0);
            }
            RunContent::Image(image) => {
                return format!("![{}]({})", image_alt_text(&image.path), image.path)
                    + &lossy_caption(image.caption.as_ref(), warnings, 0);
            }
            _ => {}
        }
    }

    let text = paragraph_text_markdown(paragraph, warnings);
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
            ParagraphKind::ListContinuation(_) => text,
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
            Some(format_list_item(text, ListItemKind::Number, level, None))
        }
        ParagraphListRef::Bullet { level, .. } => {
            Some(format_list_item(text, ListItemKind::Bullet, level, None))
        }
        ParagraphListRef::CheckBullet { level, checked, .. } => {
            Some(format_list_item(text, ListItemKind::Bullet, level, Some(checked)))
        }
    }
}

fn format_registry_list_level(paragraph: &Paragraph, registry: &StyleRegistry) -> Option<u8> {
    let list = registry.para_shape(paragraph.para_shape_id)?.list?;
    match list {
        ParagraphListRef::Outline { .. } => None,
        ParagraphListRef::Number { level, .. }
        | ParagraphListRef::Bullet { level, .. }
        | ParagraphListRef::CheckBullet { level, .. } => Some(level),
    }
}

fn mapping_list_level(paragraph: &Paragraph, mapping: &MdMapping) -> Option<u8> {
    match mapping.classify_para_shape(paragraph.para_shape_id) {
        ParagraphKind::ListItem => Some(0),
        _ => None,
    }
}

fn paragraph_text_markdown(paragraph: &Paragraph, warnings: &mut Vec<super::MdWarning>) -> String {
    paragraph_text_at(paragraph, warnings, 0)
}

/// How many tables a lossy table may sit inside; past this depth the inner
/// table is replaced by [`TABLE_TOO_DEEP`]. No corpus document reaches it.
/// The output stays linear in the depth below it: text in a cell is escaped
/// once, where it is written, and a table in a cell is drawn cell-safe
/// (`\|`, `<br>`) instead of being escaped again by every enclosing cell.
const MAX_TABLE_NESTING: usize = 8;

/// Written in place of a table nested deeper than [`MAX_TABLE_NESTING`].
const TABLE_TOO_DEEP: &str = "(중첩 표 생략)";

/// [`paragraph_text_markdown`] inside `depth` tables. Inside a table
/// (`depth > 0`) the result is cell text: every piece of text is escaped
/// once here ([`escape_gfm_cell`]) and the cell writes it as it is.
fn paragraph_text_at(
    paragraph: &Paragraph,
    warnings: &mut Vec<super::MdWarning>,
    depth: usize,
) -> String {
    let mut output = String::new();
    // Text pushed through here is escaped in a cell; table and caption
    // markup from the recursion is already cell-safe and pushed directly.
    let text = |output: &mut String, piece: &str| {
        if depth > 0 {
            output.push_str(&escape_gfm_cell(piece));
        } else {
            output.push_str(piece);
        }
    };

    // A group is drawn as its children (WS): body, table cells and captions
    // come through here; the plain-text paths (notes, text boxes) do not.
    for (part, _) in super::spread_groups(&paragraph.runs) {
        match part {
            // Markdown cannot represent inline tab attributes, so any
            // `<hp:tab width=... leader=... type=.../>` collapses back
            // to a literal `\t` via `plain_text()` — same effective
            // output the `Text(String)` arm would produce. See
            // `.docs/debug/2026-05-27_hwpx_decoder_inline_tab_attrs_lost.md`
            // §3a-A4.
            Part::Text(content) => {
                if let Some(cow) = content.plain_text() {
                    text(&mut output, &cow);
                }
            }
            Part::Image(image) => {
                if !output.is_empty() {
                    output.push(' ');
                }
                text(&mut output, &format!("![{}]({})", image_alt_text(&image.path), image.path));
                output.push_str(&lossy_caption(image.caption.as_ref(), warnings, depth));
            }
            Part::Control(control) => match control {
                // Only an allowed URL is written as a link (the text otherwise).
                // Its label escapes `\`, `[` and `]`, so it cannot close the
                // link early and start another (`a](javascript:…)[b`).
                Control::Hyperlink { text: label, url } => {
                    if super::link_url_allowed(url) {
                        let label =
                            label.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
                        text(&mut output, &format!("[{label}]({url})"));
                    } else {
                        text(&mut output, label);
                    }
                }
                Control::Footnote { paragraphs, .. } => {
                    let footnote = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    text(&mut output, &format!("(footnote: {})", footnote.trim()));
                }
                Control::Endnote { paragraphs, .. } => {
                    let endnote = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    text(&mut output, &format!("(endnote: {})", endnote.trim()));
                }
                Control::TextBox { paragraphs, .. } => {
                    let body = paragraphs
                        .iter()
                        .map(Paragraph::text_content)
                        .collect::<Vec<_>>()
                        .join(" ");
                    text(&mut output, body.trim());
                }
                Control::Unknown { tag, .. } => {
                    text(&mut output, &format!("`[{tag}]`"));
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
                        text(&mut output, body.trim());
                    }
                }
                Control::Dutmal { main_text, sub_text, .. } => {
                    text(&mut output, &format!("{main_text}({sub_text})"));
                }
                Control::Compose { compose_text, .. } => {
                    text(&mut output, compose_text);
                }
                Control::NewNumber { .. } | Control::PageHiding { .. } => {
                    // 번호 재시작·감추기 지시 — 본문 텍스트 없음, 명시적
                    // no-op (W2·W3).
                }
                Control::AutoNumber { num, format, .. } => {
                    text(&mut output, &super::auto_number_text(*num, format));
                }
                _ => {}
            },
            Part::Table(table) => {
                if !output.is_empty() {
                    output.push_str(if depth > 0 { "<br>" } else { "\n" });
                }
                output.push_str(&table_to_markdown(table, warnings, depth));
                output.push_str(&lossy_caption(table.caption.as_ref(), warnings, depth));
            }
        }
    }

    output
}

/// A picture's or table's caption as a paragraph after it, empty without
/// one — the caption numbers ("그림 3") live there (§19.16 J, review r1).
/// 이것을 실패시키는 것: 캡션 출력을 지우기
/// (`f17_caption_numbers_reach_the_readable_views`).
fn lossy_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    warnings: &mut Vec<super::MdWarning>,
    depth: usize,
) -> String {
    let Some(caption) = caption else { return String::new() };
    let text: Vec<String> = caption
        .paragraphs
        .iter()
        .map(|p| paragraph_text_at(p, warnings, depth))
        .filter(|t| !t.trim().is_empty())
        .collect();
    // In a cell the paragraph break is `<br><br>` (cell-safe, see
    // [`paragraph_text_at`]).
    let gap = if depth > 0 { "<br><br>" } else { "\n\n" };
    if text.is_empty() {
        String::new()
    } else {
        format!("{gap}{}", text.join(gap))
    }
}

/// A table inside `depth` tables. Inside a table (`depth > 0`) it is drawn
/// cell-safe — `\|` for its bars, `<br>` between its lines — around cell
/// text that [`paragraph_text_at`] has already escaped, so no text is
/// escaped twice. 이것을 실패시키는 것: 깊이 한도 삭제 · 칸에서 깊이 증가 삭제 ·
/// 칸 글자를 다시 escape 하기(`nested_tables_stay_small`).
fn table_to_markdown(table: &Table, warnings: &mut Vec<super::MdWarning>, depth: usize) -> String {
    if depth >= MAX_TABLE_NESTING {
        // The place is filled in by `encode_body` once the top-level
        // paragraph is drawn. 이것을 실패시키는 것: 경고 기록 삭제
        // (`nested_tables_stay_small`).
        warnings.push(super::MdWarning::ContainerTooDeep {
            container: "table",
            limit: MAX_TABLE_NESTING,
            section: 0,
            paragraph: 0,
        });
        return TABLE_TOO_DEEP.to_string();
    }
    let (bar, line) = if depth > 0 { ("\\|", "<br>") } else { ("|", "\n") };
    if table.rows.is_empty() {
        return format!("{bar} {bar}{line}{bar} --- {bar}");
    }

    // Warning-first: this renderer flattens merged regions into a plain GFM
    // grid (spans are not representable in pipe tables).
    let merged_cells = table
        .rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.col_span > 1 || cell.row_span > 1)
        .count();
    if merged_cells > 0 {
        warnings.push(super::MdWarning::MergedCellsFlattened { merged_cells });
    }

    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    cell.paragraphs
                        .iter()
                        .map(|p| paragraph_text_at(p, warnings, depth + 1))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        })
        .collect();

    let header = rows.first().cloned().unwrap_or_else(|| vec![String::new()]);
    let col_count = header.len().max(1);

    let sep = format!(" {bar} ");
    let mut lines = Vec::new();
    lines.push(format!("{bar} {} {bar}", header.join(&sep)));
    lines.push(format!(
        "{bar} {} {bar}",
        (0..col_count).map(|_| "---").collect::<Vec<_>>().join(&sep)
    ));

    for row in rows.iter().skip(1) {
        if row.is_empty() {
            lines.push(format!("{bar} {bar}"));
        } else {
            lines.push(format!("{bar} {} {bar}", row.join(&sep)));
        }
    }

    lines.join(line)
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
        assert_eq!(md, "(footnote: note body)");
    }

    #[test]
    fn merged_table_flattening_is_reported_and_output_unchanged() {
        let text_cell = |t: &str| {
            TableCell::with_span(
                vec![Paragraph::with_runs(
                    vec![Run::text(t, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                1,
                2, // row_span 2 → merged
            )
        };
        let table = Table::new(vec![
            TableRow::new(vec![text_cell("병합")]),
            TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("x", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
            )]),
        ]);
        let paragraph = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![paragraph]);

        let (md, warnings) = encode_without_template_report(&doc).unwrap();
        assert_eq!(
            warnings,
            vec![super::super::MdWarning::MergedCellsFlattened { merged_cells: 1 }]
        );
        // report 표면은 렌더 결과를 바꾸지 않는다.
        assert_eq!(md, encode_without_template(&doc).unwrap());

        // 무병합 표는 경고 없음 — encode_table_to_gfm 의 2×2 표와 동일 조건.
        let plain = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow::new(vec![TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("A", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                )])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let (_, warnings) = encode_without_template_report(&plain).unwrap();
        assert!(warnings.is_empty());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    text_margin: None,
                    paragraphs: vec![textbox_body],
                    width: hwpforge_foundation::HwpUnit::from_mm(80.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    placement: None,
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
        assert_eq!(md, "`[mystery]`");
    }

    #[test]
    fn encode_line_control_renders_as_empty() {
        use hwpforge_core::control::ShapePoint;
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Line {
                    frame: hwpforge_core::ObjectFrame::new(),
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 0),
                    width: hwpforge_foundation::HwpUnit::from_mm(50.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(1.0).unwrap(),
                    placement: None,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
        assert_eq!(md, "shape text");
    }

    #[test]
    fn encode_ellipse_without_text_renders_empty() {
        use hwpforge_core::control::ShapePoint;
        let paragraph = Paragraph::with_runs(
            vec![Run::control(
                Control::Ellipse {
                    frame: hwpforge_core::ObjectFrame::new(),
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: hwpforge_foundation::HwpUnit::from_mm(40.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
                    paragraphs: vec![],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    vertices: vec![
                        ShapePoint::new(0, 1000),
                        ShapePoint::new(500, 0),
                        ShapePoint::new(1000, 1000),
                    ],
                    width: hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    height: hwpforge_foundation::HwpUnit::from_mm(30.0).unwrap(),
                    placement: None,
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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
                    metadata: hwpforge_core::DutmalMetadata::default(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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
                    char_pr_ids: vec![u32::MAX; 10],
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, None, None, &mut Vec::new());
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

        let md = encode_paragraph(&paragraph, Some(&mapping), None, &mut Vec::new());
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

        assert_eq!(
            encode_paragraph(&unchecked, Some(&mapping), Some(&registry), &mut Vec::new()),
            "- [ ] todo"
        );
        assert_eq!(
            encode_paragraph(&checked, Some(&mapping), Some(&registry), &mut Vec::new()),
            "  - [x] done"
        );
    }

    /// WS: a group is drawn as its children in z-order in the body and in a
    /// table cell — picture links, a text box's text, nothing for a rectangle
    /// or an equation, a nested group's picture; inside a text box (plain
    /// text path) a group draws nothing.
    ///
    /// 이것을 실패시키는 것: 그림 갈래 삭제 · 중첩 재귀 삭제 · 자식 순서 뒤집기 ·
    /// 글상자 글자 갈래 삭제 · Equation 을 lossy 에서 렌더 · 표 칸 경로에서 묶음 누락 ·
    /// 평문 경로에서 묶음을 그림.
    #[test]
    fn lossy_group_children_in_order() {
        use hwpforge_core::control::GroupChild;
        use hwpforge_core::{Table, TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let pic =
            |p: &str| hwpforge_core::Image::new(p, u(100), u(100), hwpforge_core::ImageFormat::Png);
        let t = |s: &str| Run::text(s, CharShapeIndex::new(0));
        let para = |runs: Vec<Run>| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let grp = |children: Vec<GroupChild>| Control::Group {
            children,
            width: u(1000),
            height: u(1000),
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        let g = Run::control(
            grp(vec![
                GroupChild::Picture(pic("BinData/a.png")),
                GroupChild::Picture(pic("BinData/b.png")),
                GroupChild::Shape(Control::text_box(vec![para(vec![t("box")])], u(10), u(10))),
                GroupChild::Shape(Control::rect(u(10), u(10)).unwrap()),
                GroupChild::Shape(Control::equation("a over b")),
                GroupChild::Shape(grp(vec![GroupChild::Picture(pic("BinData/c.png"))])),
            ]),
            CharShapeIndex::new(0),
        );
        let doc = |p: Paragraph| {
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![p], hwpforge_core::PageSettings::a4()));
            encode_without_template(&d.validate().unwrap()).unwrap()
        };
        assert_eq!(
            doc(para(vec![t("x"), g.clone()])),
            "x ![a](BinData/a.png) ![b](BinData/b.png)box ![c](BinData/c.png)"
        );
        let cell_group = Run::control(
            grp(vec![GroupChild::Picture(pic("BinData/a.png"))]),
            CharShapeIndex::new(0),
        );
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![para(vec![cell_group.clone()])],
            u(1000),
        )])]);
        assert_eq!(
            doc(para(vec![Run::table(table, CharShapeIndex::new(0))])),
            "| ![a](BinData/a.png) |\n| --- |"
        );
        let in_box = Run::control(
            Control::text_box(vec![para(vec![t("in"), cell_group])], u(10), u(10)),
            CharShapeIndex::new(0),
        );
        assert_eq!(doc(para(vec![in_box])), "in");
    }

    /// S1 and S2 in lossy: a group holding a text box with only a table, or a
    /// text box of spaces, draws nothing (the box's plain text, trimmed).
    ///
    /// 이것을 실패시키는 것: lossy 묶음 글상자에 `text_content` 대신 표 렌더 ·
    /// 도형 글자 잘라내기 삭제.
    #[test]
    fn lossy_false_positive_shapes() {
        use hwpforge_core::control::GroupChild;
        use hwpforge_core::{Table, TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let t = |s: &str| Run::text(s, CharShapeIndex::new(0));
        let para = |runs: Vec<Run>| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let grp = |shape: Control| {
            Run::control(
                Control::Group {
                    children: vec![GroupChild::Shape(shape)],
                    width: u(1000),
                    height: u(1000),
                    frame: hwpforge_core::ObjectFrame::new(),
                    placement: None,
                    inst_id: None,
                },
                CharShapeIndex::new(0),
            )
        };
        let doc = |p: Paragraph| {
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![p], hwpforge_core::PageSettings::a4()));
            encode_without_template(&d.validate().unwrap()).unwrap()
        };
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![para(vec![t("칸")])],
            u(1000),
        )])]);
        let s1 = grp(Control::text_box(
            vec![para(vec![Run::table(table, CharShapeIndex::new(0))])],
            u(10),
            u(10),
        ));
        assert_eq!(doc(para(vec![t("앞"), s1])), "앞");
        let s2 = grp(Control::text_box(vec![para(vec![t("  ")])], u(10), u(10)));
        assert_eq!(doc(para(vec![t("앞"), s2, t("뒤")])), "앞뒤");
    }

    /// A document nested past the lossless limit is refused before
    /// drawing; at the limit it is drawn. (Notes are the nesting here: lossy
    /// draws nested tables in time that doubles per level, an older cost
    /// kept out of this test.)
    ///
    /// 이것을 실패시키는 것: 이 검사를 지우기.
    #[test]
    fn lossy_refuses_too_deep_nesting() {
        let nest = |n: usize| {
            let mut run = Run::text("core", CharShapeIndex::new(0));
            for _ in 0..n {
                run = Run::control(
                    Control::footnote(vec![Paragraph::with_runs(
                        vec![run],
                        ParaShapeIndex::new(0),
                    )]),
                    CharShapeIndex::new(0),
                );
            }
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                hwpforge_core::PageSettings::a4(),
            ));
            d.validate().unwrap()
        };
        assert!(encode_without_template(&nest(32)).is_ok());
        assert!(matches!(
            encode_without_template(&nest(33)),
            Err(crate::error::MdError::UnsupportedStructure { .. })
        ));
    }

    /// Tables nested in cells stay small: the output of 24 nested tables is
    /// bounded (each level past the limit is not drawn, one warning with its
    /// place), while 8 nested tables are drawn whole with no warning.
    ///
    /// 이것을 실패시키는 것: 깊이 한도 삭제 · 칸에서 깊이 증가 삭제 · 경고 기록 삭제 ·
    /// 경고 자리 채우기 삭제.
    #[test]
    fn nested_tables_stay_small() {
        use hwpforge_core::{Table, TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let nest = |n: usize| {
            let mut run = Run::text("core", CharShapeIndex::new(0));
            for _ in 0..n {
                let cell = TableCell::new(
                    vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                    u(1000),
                );
                run =
                    Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0));
            }
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(
                vec![
                    Paragraph::with_runs(
                        vec![Run::text("x", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    ),
                    Paragraph::with_runs(vec![run], ParaShapeIndex::new(0)),
                ],
                hwpforge_core::PageSettings::a4(),
            ));
            encode_without_template_report(&d.validate().unwrap()).unwrap()
        };
        assert_eq!(nest(2).0, "x\n\n| \\| core \\|<br>\\| --- \\| |\n| --- |");
        let (eight, w) = nest(8);
        assert!(eight.contains("core") && !eight.contains(TABLE_TOO_DEEP));
        assert!(w.is_empty());
        let (deep, w) = nest(24);
        assert!(deep.contains(TABLE_TOO_DEEP) && !deep.contains("core"));
        assert!(deep.len() < 4 * eight.len(), "{} vs {}", deep.len(), eight.len());
        assert_eq!(
            w,
            vec![super::super::MdWarning::ContainerTooDeep {
                container: "table",
                limit: MAX_TABLE_NESTING,
                section: 0,
                paragraph: 1,
            }]
        );
    }

    /// Text in a nested table is escaped once, where it is written, so the
    /// output grows linearly with the depth: 8 nested tables around 1,000
    /// backslashes stay near 2,000 bytes (escaping per level would make it
    /// 2^8 times that).
    ///
    /// 이것을 실패시키는 것: 칸 글자를 칸마다 다시 escape 하기 · 칸 안 표를
    /// cell-safe 가 아닌 꼴(`|`, 줄바꿈)로 쓰기.
    #[test]
    fn nested_table_text_is_escaped_once() {
        use hwpforge_core::{Table, TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let nest = |core: &str, n: usize| {
            let mut run = Run::text(core, CharShapeIndex::new(0));
            for _ in 0..n {
                let cell = TableCell::new(
                    vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                    u(1000),
                );
                run =
                    Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0));
            }
            encode_without_template(&validated_document(vec![Paragraph::with_runs(
                vec![run],
                ParaShapeIndex::new(0),
            )]))
            .unwrap()
        };
        assert_eq!(nest("a\\b|c\nd", 1), "| a\\\\b\\|c<br>d |\n| --- |");
        assert_eq!(nest("a\\b|c", 2), "| \\| a\\\\b\\|c \\|<br>\\| --- \\| |\n| --- |");
        let deep = nest(&"\\".repeat(1000), MAX_TABLE_NESTING);
        assert!(deep.len() < 2000 + 200, "{}", deep.len());
    }

    /// Lossy writes a link only for an allowed URL; otherwise its text. A
    /// label cannot close the link and start another.
    ///
    /// 이것을 실패시키는 것: lossy 의 허용 검사 삭제 · 링크 글자 escape 삭제.
    #[test]
    fn lossy_links_follow_the_allow_list() {
        let doc_label = |label: &str, url: &str| {
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::control(
                        Control::Hyperlink { text: label.into(), url: url.into() },
                        CharShapeIndex::new(0),
                    )],
                    ParaShapeIndex::new(0),
                )],
                hwpforge_core::PageSettings::a4(),
            ));
            encode_without_template(&d.validate().unwrap()).unwrap()
        };
        let doc = |url: &str| doc_label("t", url);
        assert_eq!(doc(" javascript:x"), "t");
        assert_eq!(doc("https://e.invalid"), "[t](https://e.invalid)");
        assert_eq!(doc(""), "[t]()");
        assert_eq!(
            doc_label("a](javascript:x)[b\\", "https://e.invalid"),
            "[a\\](javascript:x)\\[b\\\\](https://e.invalid)"
        );
    }
}
