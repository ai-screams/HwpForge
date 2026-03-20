//! Style-aware markdown encoder.
//!
//! Unlike the lossy encoder which discards all formatting, this encoder
//! queries a [`StyleLookup`] to produce markdown with inline formatting
//! (bold, italic, strikeout), heading detection, and image extraction.

use std::collections::HashMap;

use hwpforge_core::{Control, Document, Paragraph, RunContent, StyleLookup, Table, Validated};
use hwpforge_foundation::UnderlineType;

use crate::eqn::eqn_to_latex;

/// Output of style-aware markdown encoding.
///
/// Contains the generated markdown string and any extracted images
/// (keyed by their relative path within the output).
#[derive(Debug, Clone)]
pub struct MdOutput {
    /// The generated markdown string.
    pub markdown: String,
    /// Extracted images: relative path → binary data.
    pub images: HashMap<String, Vec<u8>>,
}

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

// ---------------------------------------------------------------------------
// Footnote/Endnote collector
// ---------------------------------------------------------------------------

/// Collects footnote and endnote references during encoding,
/// then renders GFM-style `[^n]` definitions at document end.
struct FootnoteCollector {
    footnotes: Vec<String>,
    endnotes: Vec<String>,
}

impl FootnoteCollector {
    fn new() -> Self {
        Self { footnotes: Vec::new(), endnotes: Vec::new() }
    }

    /// Adds a footnote body and returns the inline marker `[^N]`.
    fn add_footnote(&mut self, body: &str) -> String {
        let n = self.footnotes.len() + 1;
        self.footnotes.push(body.to_string());
        format!("[^{n}]")
    }

    /// Adds an endnote body and returns the inline marker `[^eN]`.
    fn add_endnote(&mut self, body: &str) -> String {
        let n = self.endnotes.len() + 1;
        self.endnotes.push(body.to_string());
        format!("[^e{n}]")
    }

    /// Renders all collected definitions as a markdown block.
    fn render_definitions(&self) -> String {
        let mut lines = Vec::new();
        for (i, body) in self.footnotes.iter().enumerate() {
            lines.push(format!("[^{}]: {}", i + 1, body));
        }
        for (i, body) in self.endnotes.iter().enumerate() {
            lines.push(format!("[^e{}]: {}", i + 1, body));
        }
        lines.join("\n")
    }
}

/// Encodes a validated document into style-aware markdown.
///
/// Queries the provided [`StyleLookup`] for character/paragraph/style
/// properties to emit inline formatting and heading markers.
pub(crate) fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
    let mut blocks = Vec::new();
    let mut images = HashMap::new();
    let mut footnotes = FootnoteCollector::new();

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(SECTION_MARKER_COMMENT.to_string());
        }

        let mut code_block_lines: Vec<String> = Vec::new();

        for paragraph in &section.paragraphs {
            // Page break
            if paragraph.page_break {
                // Flush code block first
                if !code_block_lines.is_empty() {
                    blocks.push(format!("```\n{}\n```", code_block_lines.join("\n")));
                    code_block_lines.clear();
                }
                blocks.push("---".to_string());
            }

            // Code block detection: all text runs with code font
            if is_code_paragraph(paragraph, styles) {
                let text = paragraph
                    .runs
                    .iter()
                    .filter_map(|r| {
                        if let RunContent::Text(t) = &r.content {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                code_block_lines.push(text);
                continue;
            }

            // Flush accumulated code block
            if !code_block_lines.is_empty() {
                blocks.push(format!("```\n{}\n```", code_block_lines.join("\n")));
                code_block_lines.clear();
            }

            let (markdown, para_images) =
                encode_paragraph_styled(paragraph, styles, &mut footnotes);
            if !markdown.trim().is_empty() {
                blocks.push(markdown);
            }
            images.extend(para_images);
        }

        // Flush remaining code block at section end
        if !code_block_lines.is_empty() {
            blocks.push(format!("```\n{}\n```", code_block_lines.join("\n")));
        }
    }

    // Append footnote/endnote definitions
    let definitions = footnotes.render_definitions();
    let mut markdown = blocks.join("\n\n");
    if !definitions.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&definitions);
    }

    MdOutput { markdown, images }
}

/// Encodes a single paragraph into styled markdown, returning the markdown
/// string and any extracted images.
fn encode_paragraph_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    let mut images = HashMap::new();

    // Handle single-run block elements (table, image) as standalone blocks.
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(table) => {
                let (md, tbl_images) = table_to_styled_markdown(table, styles, false, footnotes);
                images.extend(tbl_images);
                return (md, images);
            }
            RunContent::Image(image) => {
                let alt = image_alt_text(&image.path);
                let rel_path = image_rel_path(&image.path, styles);
                let md = format!("![{alt}]({rel_path})");
                if let Some(data) = styles.image_data(&image.path) {
                    images.insert(rel_path, data.to_vec());
                }
                return (md, images);
            }
            _ => {}
        }
    }

    let (text, para_images) = paragraph_text_styled(paragraph, styles, footnotes);
    images.extend(para_images);

    // Outline semantics on the paragraph shape are the truth source for headings.
    if let Some(level) = styles.para_heading_level(paragraph.para_shape_id) {
        let clamped = level.clamp(1, 6);
        // Headings must be single-line: collapse lineBreak-originated newlines.
        let heading_text = text.trim().replace('\n', " ");
        if heading_text.is_empty() {
            return (String::new(), images);
        }
        return (format!("{} {}", "#".repeat(clamped as usize), heading_text), images);
    }

    // Real paragraph list semantics take priority over style-name heuristics.
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        if let Some(list_type) = styles.para_list_type(paragraph.para_shape_id) {
            let level = styles.para_list_level(paragraph.para_shape_id).unwrap_or(0);
            return (format_list_item(trimmed, list_type, level), images);
        }
    }

    if let Some(level) =
        paragraph.style_id.and_then(|style_id| styles.style_heading_level(style_id))
    {
        let clamped = level.clamp(1, 6);
        let heading_text = trimmed.replace('\n', " ");
        if heading_text.is_empty() {
            return (String::new(), images);
        }
        return (format!("{} {}", "#".repeat(clamped as usize), heading_text), images);
    }

    if let Some(style_id) = paragraph.style_id {
        // Check for list style by style name (fallback).
        if let Some(style_name) = styles.style_name(style_id) {
            if let Some(list_md) = format_as_list(trimmed, style_name) {
                return (list_md, images);
            }
        }
    }

    (text.trim_start().to_string(), images)
}

/// Extracts text from a paragraph with inline formatting applied.
fn paragraph_text_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    let mut output = String::new();
    let mut images = HashMap::new();

    // Group consecutive text runs by InlineFormat for proper wrapping.
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for run in &paragraph.runs {
        match &run.content {
            RunContent::Text(text) => {
                let fmt = InlineFormat::from_style(run.char_shape_id, styles);
                if fmt == current_format {
                    current_text.push_str(text);
                } else {
                    // Flush previous group.
                    if !current_text.is_empty() {
                        output.push_str(&current_format.wrap(&current_text));
                        current_text.clear();
                    }
                    current_format = fmt;
                    current_text.push_str(text);
                }
            }
            RunContent::Image(image) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                if !output.is_empty() {
                    output.push(' ');
                }
                let alt = image_alt_text(&image.path);
                let rel_path = image_rel_path(&image.path, styles);
                output.push_str(&format!("![{alt}]({rel_path})"));
                if let Some(data) = styles.image_data(&image.path) {
                    images.insert(rel_path, data.to_vec());
                }
            }
            RunContent::Table(table) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                if !output.is_empty() {
                    output.push('\n');
                }
                let (tbl_md, tbl_images) =
                    table_to_styled_markdown(table, styles, false, footnotes);
                output.push_str(&tbl_md);
                images.extend(tbl_images);
            }
            RunContent::Control(control) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                output.push_str(&encode_control_styled(control, styles, &mut images, footnotes));
            }
            _ => {}
        }
    }

    // Flush remaining text.
    if !current_text.is_empty() {
        output.push_str(&current_format.wrap(&current_text));
    }

    (output, images)
}

/// Encodes a single control element into styled markdown.
fn encode_control_styled(
    control: &Control,
    styles: &dyn StyleLookup,
    _images: &mut HashMap<String, Vec<u8>>,
    footnotes: &mut FootnoteCollector,
) -> String {
    match control {
        Control::Hyperlink { text, url } => {
            // Reject dangerous URL schemes (case-insensitive)
            let url_lower = url.to_lowercase();
            if url_lower.starts_with("javascript:")
                || url_lower.starts_with("data:")
                || url_lower.starts_with("vbscript:")
                || url_lower.starts_with("file:")
            {
                // Strip the link, emit only the visible text (escaped)
                text.replace(']', "\\]")
            } else {
                // Escape ] in text and ) in url to prevent markdown injection
                let safe_text = text.replace(']', "\\]");
                let safe_url = url.replace('(', "%28").replace(')', "%29");
                format!("[{safe_text}]({safe_url})")
            }
        }
        Control::Footnote { paragraphs, .. } => {
            let body = paragraphs
                .iter()
                .map(|p| extract_paragraph_text(p, styles))
                .collect::<Vec<_>>()
                .join(" ");
            footnotes.add_footnote(body.trim())
        }
        Control::Endnote { paragraphs, .. } => {
            let body = paragraphs
                .iter()
                .map(|p| extract_paragraph_text(p, styles))
                .collect::<Vec<_>>()
                .join(" ");
            footnotes.add_endnote(body.trim())
        }
        Control::TextBox { paragraphs, .. } => {
            let body = paragraphs
                .iter()
                .map(|p| extract_paragraph_text(p, styles))
                .collect::<Vec<_>>()
                .join(" ");
            body.trim().to_string()
        }
        Control::Equation { script, .. } => eqn_to_latex(script),
        Control::Chart { .. } => "<!-- chart -->".to_string(),
        Control::Line { .. } => String::new(),
        Control::Ellipse { paragraphs, .. } | Control::Polygon { paragraphs, .. } => {
            let body = paragraphs
                .iter()
                .map(|p| extract_paragraph_text(p, styles))
                .collect::<Vec<_>>()
                .join(" ");
            if body.trim().is_empty() {
                String::new()
            } else {
                body.trim().to_string()
            }
        }
        Control::Dutmal { main_text, sub_text, .. } => {
            format!("{main_text}({sub_text})")
        }
        Control::Compose { compose_text, .. } => compose_text.clone(),
        Control::CrossRef { target_name, .. } => {
            format!("[{target_name}]")
        }
        Control::Field { hint_text, .. } => hint_text.as_deref().unwrap_or("____").to_string(),
        Control::Bookmark { .. } => {
            // Bookmarks are invisible anchors — emit nothing.
            String::new()
        }
        Control::Memo { content, author, .. } => {
            let body = content
                .iter()
                .map(|p| extract_paragraph_text(p, styles))
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = body.trim();
            if trimmed.is_empty() {
                String::new()
            } else {
                // Sanitize author and body to prevent HTML comment breakout via `-->`
                let safe_author = author.replace("--", "\\-\\-");
                let safe_body = trimmed.replace("--", "\\-\\-");
                format!("<!-- memo({safe_author}): {safe_body} -->")
            }
        }
        Control::IndexMark { .. } => {
            // Index marks are invisible — emit nothing.
            String::new()
        }
        Control::Arc { .. } | Control::Curve { .. } | Control::ConnectLine { .. } => {
            // These shapes rarely contain text; render nothing.
            String::new()
        }
        Control::Unknown { tag, .. } => format!("`[{tag}]`"),
        _ => String::new(),
    }
}

/// Recursively extracts plain text from a paragraph using style-aware formatting.
///
/// Note: Uses a local `FootnoteCollector`, so footnotes nested inside control
/// bodies (e.g. footnote inside a TextBox) will not propagate to the document-level
/// collector. This is acceptable because HWP rarely nests footnotes inside shapes,
/// and threading the collector through all recursive paths would require significant
/// refactoring for marginal benefit.
fn extract_paragraph_text(paragraph: &Paragraph, styles: &dyn StyleLookup) -> String {
    let mut dummy = FootnoteCollector::new();
    let (text, _images) = paragraph_text_styled(paragraph, styles, &mut dummy);
    text
}

// ---------------------------------------------------------------------------
// Inline formatting
// ---------------------------------------------------------------------------

/// Inline formatting state derived from a character shape.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct InlineFormat {
    bold: bool,
    italic: bool,
    strikeout: bool,
    underline: bool,
    superscript: bool,
    subscript: bool,
}

impl InlineFormat {
    /// Queries the style lookup for formatting properties.
    fn from_style(id: hwpforge_foundation::CharShapeIndex, styles: &dyn StyleLookup) -> Self {
        Self {
            bold: styles.char_bold(id).unwrap_or(false),
            italic: styles.char_italic(id).unwrap_or(false),
            strikeout: styles.char_strikeout(id).unwrap_or(false),
            underline: !matches!(styles.char_underline(id), None | Some(UnderlineType::None)),
            superscript: styles.char_superscript(id).unwrap_or(false),
            subscript: styles.char_subscript(id).unwrap_or(false),
        }
    }

    /// Wraps text with inline formatting (hybrid markdown/HTML).
    ///
    /// Receives **raw** (unescaped) text. Strategy:
    /// - If text contains markdown-marker chars (`*`, `_`, `~`), or text
    ///   starts/ends with punctuation (which breaks CommonMark flanking
    ///   rules when adjacent to other text), or formatting has no markdown
    ///   equivalent → HTML tags with `escape_html`.
    /// - Otherwise → markdown markers (`**`, `*`, `~~`) with `escape_markdown`.
    fn wrap(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        let has_basic = self.bold || self.italic || self.strikeout;
        let has_any = has_basic || self.underline || self.superscript || self.subscript;

        if !has_any {
            return escape_markdown(text);
        }

        // Move leading/trailing whitespace outside formatting markers.
        // CommonMark: `** text**` fails (space after opening marker).
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return text.to_string();
        }
        let leading = &text[..text.len() - text.trim_start().len()];
        let trailing = &text[text.trim_end().len()..];

        let needs_html_only = self.underline || self.superscript || self.subscript;
        let has_marker_conflicts = trimmed.chars().any(|c| matches!(c, '*' | '_' | '~'));
        // CommonMark flanking rules: `word***(punct` fails as left-flanking.
        let has_boundary_punct = trimmed.chars().next().is_some_and(|c| c.is_ascii_punctuation())
            || trimmed.chars().next_back().is_some_and(|c| c.is_ascii_punctuation());

        let wrapped =
            if needs_html_only || (has_basic && (has_marker_conflicts || has_boundary_punct)) {
                // HTML path
                let mut result = escape_html(trimmed);
                if self.bold {
                    result = format!("<strong>{result}</strong>");
                }
                if self.italic {
                    result = format!("<em>{result}</em>");
                }
                if self.strikeout {
                    result = format!("<del>{result}</del>");
                }
                if self.underline {
                    result = format!("<u>{result}</u>");
                }
                if self.superscript {
                    result = format!("<sup>{result}</sup>");
                }
                if self.subscript {
                    result = format!("<sub>{result}</sub>");
                }
                result
            } else {
                // Markdown path: no conflicting chars, safe to use markers.
                let mut result = escape_markdown(trimmed);
                if self.bold && self.italic {
                    result = format!("***{result}***");
                } else if self.bold {
                    result = format!("**{result}**");
                } else if self.italic {
                    result = format!("*{result}*");
                }
                if self.strikeout {
                    result = format!("~~{result}~~");
                }
                result
            };

        format!("{leading}{wrapped}{trailing}")
    }

    /// Wraps text with HTML inline formatting tags.
    ///
    /// Used inside HTML table cells where markdown syntax is not valid.
    fn wrap_html(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut result = escape_html(text);

        if self.bold {
            result = format!("<strong>{result}</strong>");
        }
        if self.italic {
            result = format!("<em>{result}</em>");
        }
        if self.strikeout {
            result = format!("<del>{result}</del>");
        }
        if self.underline {
            result = format!("<u>{result}</u>");
        }
        if self.superscript {
            result = format!("<sup>{result}</sup>");
        }
        if self.subscript {
            result = format!("<sub>{result}</sub>");
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Table encoding
// ---------------------------------------------------------------------------

/// Returns `true` if any cell in the table has col_span > 1 or row_span > 1.
fn has_merge(table: &Table) -> bool {
    table.rows.iter().any(|row| row.cells.iter().any(|cell| cell.col_span > 1 || cell.row_span > 1))
}

/// Returns `true` if any cell contains a nested table.
/// Nested HTML tables inside GFM pipe cells break rendering.
fn has_nested_table(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells.iter().any(|cell| {
            cell.paragraphs
                .iter()
                .any(|p| p.runs.iter().any(|r| matches!(&r.content, RunContent::Table(_))))
        })
    })
}

/// Encodes a table into markdown, choosing GFM or HTML based on cell merges.
///
/// When `html_context` is true, cell text uses HTML tags instead of markdown.
/// Returns the markdown string and any extracted images from table cells.
fn table_to_styled_markdown(
    table: &Table,
    styles: &dyn StyleLookup,
    html_context: bool,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    if table.rows.is_empty() {
        return ("| |\n| --- |".to_string(), HashMap::new());
    }

    if has_merge(table) || has_nested_table(table) || html_context {
        table_to_html(table, styles, footnotes)
    } else {
        table_to_gfm(table, styles, footnotes)
    }
}

/// Renders a table as GFM (GitHub Flavored Markdown) pipe table.
fn table_to_gfm(
    table: &Table,
    styles: &dyn StyleLookup,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    let mut images = HashMap::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in &table.rows {
        let mut cells = Vec::new();
        for cell in &row.cells {
            let mut parts = Vec::new();
            for p in &cell.paragraphs {
                let (text, para_images) = paragraph_text_styled(p, styles, footnotes);
                images.extend(para_images);
                parts.push(text);
            }
            cells.push(escape_gfm_cell(&parts.join("\n")));
        }
        rows.push(cells);
    }

    let header = rows.first().cloned().unwrap_or_else(|| vec![String::new()]);
    let col_count = header.len().max(1);

    let mut lines = Vec::new();
    lines.push(format!("| {} |", header.join(" | ")));
    lines.push(format!("| {} |", (0..col_count).map(|_| "---").collect::<Vec<_>>().join(" | ")));

    for row in rows.iter().skip(1) {
        // Pad or truncate row to match header column count for valid GFM.
        let mut padded = row.clone();
        padded.resize(col_count, String::new());
        lines.push(format!("| {} |", padded.join(" | ")));
    }

    (lines.join("\n"), images)
}

/// Renders a table as HTML `<table>` with colspan/rowspan attributes.
fn table_to_html(
    table: &Table,
    styles: &dyn StyleLookup,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    let mut images = HashMap::new();
    let mut lines = Vec::new();
    lines.push("<table>".to_string());
    lines.push("<tbody>".to_string());

    for row in &table.rows {
        lines.push("<tr>".to_string());
        for cell in &row.cells {
            let mut parts = Vec::new();
            for p in &cell.paragraphs {
                let (text, para_images) = extract_paragraph_text_html(p, styles, footnotes);
                images.extend(para_images);
                parts.push(text);
            }
            let text = parts.join("<br>");

            let mut attrs = String::new();
            if cell.col_span > 1 {
                attrs.push_str(&format!(" colspan=\"{}\"", cell.col_span));
            }
            if cell.row_span > 1 {
                attrs.push_str(&format!(" rowspan=\"{}\"", cell.row_span));
            }

            let trimmed = text.trim();
            lines.push(format!("  <td{attrs}>{trimmed}</td>"));
        }
        lines.push("</tr>".to_string());
    }

    lines.push("</tbody>".to_string());
    lines.push("</table>".to_string());

    (lines.join("\n"), images)
}

/// Extracts text from a paragraph using HTML inline formatting.
fn extract_paragraph_text_html(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    footnotes: &mut FootnoteCollector,
) -> (String, HashMap<String, Vec<u8>>) {
    let mut output = String::new();
    let mut images = HashMap::new();
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for run in &paragraph.runs {
        match &run.content {
            RunContent::Text(text) => {
                let fmt = InlineFormat::from_style(run.char_shape_id, styles);
                if fmt == current_format {
                    current_text.push_str(text);
                } else {
                    if !current_text.is_empty() {
                        output.push_str(&current_format.wrap_html(&current_text));
                        current_text.clear();
                    }
                    current_format = fmt;
                    current_text.push_str(text);
                }
            }
            RunContent::Control(control) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                let mut ctrl_images = HashMap::new();
                let ctrl_output =
                    encode_control_styled(control, styles, &mut ctrl_images, footnotes);
                images.extend(ctrl_images);
                // HTML-escape text-bearing control output in HTML table context.
                // Only structural/safe outputs (footnote markers, hyperlinks,
                // equations, chart comments, empty shapes) skip escaping.
                let escaped = match &**control {
                    Control::Hyperlink { .. }
                    | Control::Footnote { .. }
                    | Control::Endnote { .. }
                    | Control::Equation { .. }
                    | Control::Chart { .. }
                    | Control::Line { .. }
                    | Control::Arc { .. }
                    | Control::Curve { .. }
                    | Control::ConnectLine { .. }
                    | Control::Bookmark { .. }
                    | Control::IndexMark { .. } => ctrl_output,
                    _ => escape_html(&ctrl_output),
                };
                output.push_str(&escaped);
            }
            RunContent::Image(image) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                let alt = image_alt_text(&image.path);
                let rel_path = image_rel_path(&image.path, styles);
                output.push_str(&format!("<img src=\"{rel_path}\" alt=\"{alt}\"/>"));
                if let Some(data) = styles.image_data(&image.path) {
                    images.insert(rel_path, data.to_vec());
                }
            }
            RunContent::Table(table) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                let (tbl_md, tbl_images) = table_to_html(table, styles, footnotes);
                output.push_str(&tbl_md);
                images.extend(tbl_images);
            }
            _ => {}
        }
    }

    if !current_text.is_empty() {
        output.push_str(&current_format.wrap_html(&current_text));
    }

    (output, images)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the filename stem as alt text from an image path.
fn image_alt_text(path: &str) -> String {
    let name =
        path.rsplit(['/', '\\']).next().and_then(|name| name.split('.').next()).unwrap_or("image");
    // Escape characters that break markdown image syntax.
    name.replace('[', "\\[").replace(']', "\\]")
}

/// Converts an image source path to a relative output path.
///
/// Uses `styles.image_resolve_filename()` to obtain the actual filename
/// with extension (e.g. `"image1.png"`) from a `binaryItemIDRef` like
/// `"BinData/image1"`. Falls back to the raw path basename if unresolved.
fn image_rel_path(path: &str, styles: &dyn StyleLookup) -> String {
    let filename = styles
        .image_resolve_filename(path)
        .unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or("image"));
    // Escape parentheses that break markdown link syntax.
    let safe_filename = filename.replace('(', "%28").replace(')', "%29");
    format!("images/{safe_filename}")
}

/// Escapes markdown-special characters in plain text content so they render
/// literally instead of being interpreted as formatting markers.
///
/// Applied to text extracted from HWPX (which is always plain text), BEFORE
/// wrapping with inline format markers like `**bold**` or `<em>`.
fn escape_markdown(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 8);
    for ch in input.chars() {
        match ch {
            '*' | '_' | '`' | '[' | ']' | '~' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '#' => {
                // Only escape at line start, but simpler to always escape
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escapes characters that break GFM table cell rendering.
///
/// Only handles pipe (`|`) and newline — markdown-special characters are
/// already escaped by [`escape_markdown`] before this point.
fn escape_gfm_cell(input: &str) -> String {
    input.replace('|', "\\|").replace('\n', "<br>")
}

/// Escapes HTML special characters to prevent XSS in HTML table output.
fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Code font names used to detect code block paragraphs.
const CODE_FONTS: &[&str] = &[
    "D2Coding",
    "Consolas",
    "Courier New",
    "Source Code Pro",
    "Fira Code",
    "JetBrains Mono",
    "Monaco",
    "Menlo",
    "Courier",
    "Lucida Console",
    "Nanum Gothic Coding",
];

/// Returns true if all text runs in the paragraph use a monospace/code font.
fn is_code_paragraph(paragraph: &Paragraph, styles: &dyn StyleLookup) -> bool {
    let text_runs: Vec<_> =
        paragraph.runs.iter().filter(|r| matches!(&r.content, RunContent::Text(_))).collect();
    if text_runs.is_empty() {
        return false;
    }
    text_runs.iter().all(|run| {
        styles
            .char_font_name(run.char_shape_id)
            .map(|name| CODE_FONTS.iter().any(|cf| name.contains(cf)))
            .unwrap_or(false)
    })
}

/// Formats paragraph text as a list item if the style name indicates a list.
///
/// Returns `None` if the style is not a list style.
fn format_as_list(text: &str, style_name: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    // Korean list style patterns
    if style_name.contains("글머리") || style_name.contains("개조") {
        Some(format!("- {text}"))
    } else if style_name.contains("번호") {
        Some(format!("1. {text}"))
    } else {
        None
    }
}

fn format_list_item(text: &str, list_type: &str, level: u8) -> String {
    let indent = "  ".repeat(level as usize);
    if list_type == "NUMBER" {
        format!("{indent}1. {text}")
    } else {
        format!("{indent}- {text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::{
        control::{DutmalAlign, DutmalPosition, ShapePoint},
        Document, Image, ImageFormat, Paragraph, Run, Section, Table, TableCell, TableRow,
    };
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, StyleIndex};

    // -----------------------------------------------------------------------
    // MockStyles
    // -----------------------------------------------------------------------

    struct MockStyles {
        bold_ids: Vec<usize>,
        italic_ids: Vec<usize>,
        strikeout_ids: Vec<usize>,
        list_para_types: HashMap<usize, &'static str>,
        list_para_levels: HashMap<usize, u8>,
        heading_paras: HashMap<usize, u8>,
        heading_styles: HashMap<usize, u8>,
        style_names: HashMap<usize, String>,
        image_data: HashMap<String, Vec<u8>>,
    }

    impl MockStyles {
        fn new() -> Self {
            Self {
                bold_ids: Vec::new(),
                italic_ids: Vec::new(),
                strikeout_ids: Vec::new(),
                list_para_types: HashMap::new(),
                list_para_levels: HashMap::new(),
                heading_paras: HashMap::new(),
                heading_styles: HashMap::new(),
                style_names: HashMap::new(),
                image_data: HashMap::new(),
            }
        }
    }

    impl StyleLookup for MockStyles {
        fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
            Some(self.bold_ids.contains(&id.get()))
        }

        fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
            Some(self.italic_ids.contains(&id.get()))
        }

        fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
            Some(self.strikeout_ids.contains(&id.get()))
        }

        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.heading_paras.get(&id.get()).copied()
        }

        fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
            self.list_para_types.get(&id.get()).copied()
        }

        fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.list_para_levels.get(&id.get()).copied()
        }

        fn style_name(&self, id: StyleIndex) -> Option<&str> {
            self.style_names.get(&id.get()).map(String::as_str)
        }

        fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
            self.heading_styles.get(&id.get()).copied()
        }

        fn image_data(&self, key: &str) -> Option<&[u8]> {
            self.image_data.get(key).map(|v| v.as_slice())
        }
    }

    fn validated_document(paragraphs: Vec<Paragraph>) -> Document<Validated> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, hwpforge_core::PageSettings::a4()));
        doc.validate().unwrap()
    }

    // -----------------------------------------------------------------------
    // Task 4: Basic encode_styled skeleton
    // -----------------------------------------------------------------------

    #[test]
    fn encode_styled_plain_text() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("hello world", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "hello world");
        assert!(output.images.is_empty());
    }

    #[test]
    fn encode_styled_multiple_sections() {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("first", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("second", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let doc = doc.validate().unwrap();
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<!-- hwpforge:section -->"));
        assert!(output.markdown.contains("first"));
        assert!(output.markdown.contains("second"));
    }

    #[test]
    fn encode_styled_empty_paragraph_skipped() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("   ", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    // -----------------------------------------------------------------------
    // Task 5: Inline formatting
    // -----------------------------------------------------------------------

    #[test]
    fn inline_format_bold() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("bold text", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "**bold text**");
    }

    #[test]
    fn inline_format_italic() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("italic text", CharShapeIndex::new(2))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.italic_ids.push(2);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "*italic text*");
    }

    #[test]
    fn inline_format_bold_italic() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("both", CharShapeIndex::new(3))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(3);
        styles.italic_ids.push(3);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "***both***");
    }

    #[test]
    fn inline_format_strikeout() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("deleted", CharShapeIndex::new(4))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.strikeout_ids.push(4);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "~~deleted~~");
    }

    #[test]
    fn inline_format_strikeout_bold() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("strike bold", CharShapeIndex::new(5))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(5);
        styles.strikeout_ids.push(5);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "~~**strike bold**~~");
    }

    #[test]
    fn inline_format_mixed_runs() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![
                Run::text("normal ", CharShapeIndex::new(0)),
                Run::text("bold", CharShapeIndex::new(1)),
                Run::text(" normal", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "normal **bold** normal");
    }

    #[test]
    fn inline_format_consecutive_same_format_merged() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![
                Run::text("hello ", CharShapeIndex::new(1)),
                Run::text("world", CharShapeIndex::new(1)),
            ],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "**hello world**");
    }

    #[test]
    fn inline_format_bold_falls_back_to_html_when_text_has_asterisk() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap("**산·학·연 협력** 가점"), "<strong>**산·학·연 협력** 가점</strong>");
    }

    #[test]
    fn inline_format_no_conflict_uses_markdown() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap("기관명칭 기입 요망"), "**기관명칭 기입 요망**");
    }

    // -----------------------------------------------------------------------
    // Task 6: Heading detection
    // -----------------------------------------------------------------------

    #[test]
    fn heading_level_1() {
        let para = Paragraph::with_runs(
            vec![Run::text("Title", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(1));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(1, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "# Title");
    }

    #[test]
    fn heading_level_3() {
        let para = Paragraph::with_runs(
            vec![Run::text("Subsection", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(3));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(3, 3);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "### Subsection");
    }

    #[test]
    fn no_style_id_plain_text() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("body text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(1, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "body text");
    }

    #[test]
    fn para_shape_heading_without_style_id_emits_heading() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("Outline only", CharShapeIndex::new(0))],
            ParaShapeIndex::new(7),
        )]);
        let mut styles = MockStyles::new();
        styles.heading_paras.insert(7, 3);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "### Outline only");
    }

    #[test]
    fn para_shape_heading_takes_priority_over_style_fallback() {
        let para = Paragraph::with_runs(
            vec![Run::text("Priority", CharShapeIndex::new(0))],
            ParaShapeIndex::new(7),
        )
        .with_style(StyleIndex::new(2));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_paras.insert(7, 4);
        styles.heading_styles.insert(2, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "#### Priority");
    }

    #[test]
    fn heading_with_bold_text() {
        let para = Paragraph::with_runs(
            vec![Run::text("bold heading", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(2));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);
        styles.heading_styles.insert(2, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "# **bold heading**");
    }

    #[test]
    fn heading_level_clamped_to_6() {
        let para = Paragraph::with_runs(
            vec![Run::text("Deep", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(10));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(10, 7); // exceeds 6

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "###### Deep");
    }

    #[test]
    fn para_shape_list_takes_priority_over_style_heading_fallback() {
        let para = Paragraph::with_runs(
            vec![Run::text("Still a list", CharShapeIndex::new(0))],
            ParaShapeIndex::new(9),
        )
        .with_style(StyleIndex::new(2));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.list_para_types.insert(9, "NUMBER");
        styles.list_para_levels.insert(9, 0);
        styles.heading_styles.insert(2, 2);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "1. Still a list");
    }

    #[test]
    fn para_shape_list_preserves_nested_depth() {
        let doc = validated_document(vec![
            Paragraph::with_runs(
                vec![Run::text("Top", CharShapeIndex::new(0))],
                ParaShapeIndex::new(1),
            ),
            Paragraph::with_runs(
                vec![Run::text("Nested", CharShapeIndex::new(0))],
                ParaShapeIndex::new(2),
            ),
        ]);
        let mut styles = MockStyles::new();
        styles.list_para_types.insert(1, "BULLET");
        styles.list_para_levels.insert(1, 0);
        styles.list_para_types.insert(2, "BULLET");
        styles.list_para_levels.insert(2, 2);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "- Top\n\n    - Nested");
    }

    // -----------------------------------------------------------------------
    // Task 7: Adaptive table encoding
    // -----------------------------------------------------------------------

    #[test]
    fn table_simple_gfm() {
        let table = Table::new(vec![
            TableRow::new(vec![
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("A", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("B", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
            ]),
            TableRow::new(vec![
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("1", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("2", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
            ]),
        ]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("| A | B |"));
        assert!(output.markdown.contains("| --- | --- |"));
        assert!(output.markdown.contains("| 1 | 2 |"));
    }

    #[test]
    fn table_with_colspan_renders_html() {
        let table = Table::new(vec![
            TableRow::new(vec![TableCell::with_span(
                vec![Paragraph::with_runs(
                    vec![Run::text("merged", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::from_mm(60.0).unwrap(),
                2, // col_span
                1,
            )]),
            TableRow::new(vec![
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("A", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("B", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
            ]),
        ]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<table>"));
        assert!(output.markdown.contains("colspan=\"2\""));
        assert!(output.markdown.contains("merged"));
    }

    #[test]
    fn table_with_rowspan_renders_html() {
        let table = Table::new(vec![
            TableRow::new(vec![
                TableCell::with_span(
                    vec![Paragraph::with_runs(
                        vec![Run::text("spans", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                    1,
                    2, // row_span
                ),
                TableCell::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("X", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::from_mm(30.0).unwrap(),
                ),
            ]),
            TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("Y", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::from_mm(30.0).unwrap(),
            )]),
        ]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<table>"));
        assert!(output.markdown.contains("rowspan=\"2\""));
    }

    #[test]
    fn table_bold_cell_in_html_uses_strong() {
        let table = Table::new(vec![TableRow::new(vec![TableCell::with_span(
            vec![Paragraph::with_runs(
                vec![Run::text("bold", CharShapeIndex::new(1))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(60.0).unwrap(),
            2,
            1,
        )])]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<strong>bold</strong>"));
        assert!(!output.markdown.contains("**bold**"));
    }

    #[test]
    fn table_empty_renders_placeholder() {
        // Validation rejects empty tables, so test the helper directly.
        let table = Table::new(vec![]);
        let styles = MockStyles::new();
        let mut notes = FootnoteCollector::new();
        let (result, _images) = table_to_styled_markdown(&table, &styles, false, &mut notes);
        assert_eq!(result, "| |\n| --- |");
    }

    #[test]
    fn table_pipe_in_cell_escaped_gfm() {
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("A|B", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(30.0).unwrap(),
        )])]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("A\\|B"));
    }

    // -----------------------------------------------------------------------
    // Task 8: Content handling
    // -----------------------------------------------------------------------

    #[test]
    fn control_hyperlink() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "Rust".to_string(),
                    url: "https://www.rust-lang.org".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "[Rust](https://www.rust-lang.org)");
    }

    #[test]
    fn control_footnote() {
        let footnote_body = Paragraph::with_runs(
            vec![Run::text("note body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote { inst_id: None, paragraphs: vec![footnote_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "[^1]\n\n[^1]: note body");
    }

    #[test]
    fn control_endnote() {
        let endnote_body = Paragraph::with_runs(
            vec![Run::text("end body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Endnote { inst_id: None, paragraphs: vec![endnote_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "[^e1]\n\n[^e1]: end body");
    }

    #[test]
    fn control_textbox() {
        let textbox_body = Paragraph::with_runs(
            vec![Run::text("box content", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::TextBox {
                    paragraphs: vec![textbox_body],
                    width: HwpUnit::from_mm(80.0).unwrap(),
                    height: HwpUnit::from_mm(40.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "box content");
    }

    #[test]
    fn control_dutmal() {
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
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "한글(hangeul)");
    }

    #[test]
    fn control_compose() {
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
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "㊀");
    }

    #[test]
    fn control_unknown() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Unknown { tag: "mystery".to_string(), data: None },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "`[mystery]`");
    }

    #[test]
    fn control_line_empty() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Line {
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 0),
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(1.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    #[test]
    fn control_ellipse_with_text() {
        let inner = Paragraph::with_runs(
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
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "shape text");
    }

    #[test]
    fn control_polygon_with_text() {
        let inner = Paragraph::with_runs(
            vec![Run::text("polygon text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Polygon {
                    vertices: vec![
                        ShapePoint::new(0, 1000),
                        ShapePoint::new(500, 0),
                        ShapePoint::new(1000, 1000),
                    ],
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(30.0).unwrap(),
                    horz_offset: 0,
                    vert_offset: 0,
                    paragraphs: vec![inner],
                    caption: None,
                    style: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "polygon text");
    }

    #[test]
    fn image_extraction() {
        let mut styles = MockStyles::new();
        styles.image_data.insert("BinData/photo.jpg".to_string(), vec![0xFF, 0xD8, 0xFF]);

        let image = Image::new(
            "BinData/photo.jpg",
            HwpUnit::from_mm(50.0).unwrap(),
            HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Jpeg,
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "![photo](images/photo.jpg)");
        assert_eq!(output.images.get("images/photo.jpg"), Some(&vec![0xFF, 0xD8, 0xFF]));
    }

    #[test]
    fn equation_placeholder() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Equation {
                    script: "{a+b} over {c+d}".to_string(),
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(10.0).unwrap(),
                    base_line: 70,
                    text_color: hwpforge_foundation::Color::BLACK,
                    font: "HancomEQN".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "$\\frac{a+b}{c+d}$");
    }

    #[test]
    fn chart_placeholder() {
        use hwpforge_core::chart::{
            ChartData, ChartGrouping, ChartSeries, ChartType, LegendPosition,
        };
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Chart {
                    chart_type: ChartType::Bar,
                    data: ChartData::Category {
                        categories: vec!["A".to_string()],
                        series: vec![ChartSeries { name: "S1".to_string(), values: vec![1.0] }],
                    },
                    width: HwpUnit::from_mm(100.0).unwrap(),
                    height: HwpUnit::from_mm(60.0).unwrap(),
                    title: None,
                    legend: LegendPosition::Right,
                    grouping: ChartGrouping::Clustered,
                    bar_shape: None,
                    explosion: None,
                    of_pie_type: None,
                    radar_style: None,
                    wireframe: None,
                    bubble_3d: None,
                    scatter_style: None,
                    show_markers: None,
                    stock_variant: None,
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "<!-- chart -->");
    }

    // -----------------------------------------------------------------------
    // InlineFormat unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn inline_format_wrap_empty() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap(""), "");
    }

    #[test]
    fn inline_format_wrap_html_bold() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<strong>text</strong>");
    }

    #[test]
    fn inline_format_wrap_html_italic() {
        let fmt = InlineFormat { italic: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<em>text</em>");
    }

    #[test]
    fn inline_format_wrap_html_strikeout() {
        let fmt = InlineFormat { strikeout: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<del>text</del>");
    }

    #[test]
    fn inline_format_wrap_html_all() {
        let fmt = InlineFormat { bold: true, italic: true, strikeout: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<del><em><strong>text</strong></em></del>");
    }

    #[test]
    fn inline_format_wrap_html_empty() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap_html(""), "");
    }

    #[test]
    fn has_merge_false_for_simple_table() {
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            HwpUnit::from_mm(30.0).unwrap(),
        )])]);
        assert!(!has_merge(&table));
    }

    #[test]
    fn has_merge_true_for_colspan() {
        let table = Table::new(vec![TableRow::new(vec![TableCell::with_span(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            HwpUnit::from_mm(60.0).unwrap(),
            2,
            1,
        )])]);
        assert!(has_merge(&table));
    }
}
