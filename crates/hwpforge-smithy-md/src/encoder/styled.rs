//! Style-aware markdown encoder.
//!
//! Unlike the lossy encoder which discards all formatting, this encoder
//! queries a [`StyleLookup`] to produce markdown with inline formatting
//! (bold, italic, strikeout), heading detection, and image extraction.

use std::collections::HashMap;

use hwpforge_core::{
    classify_paragraph, Control, Document, ParaKind, Paragraph, RunContent, StyleLookup, Table,
    Validated,
};
use hwpforge_foundation::UnderlineType;

use super::list_format::{format_list_continuation, format_list_item};
use crate::eqn::eqn_to_latex;
use crate::internal_styles::parse_list_continuation_style_name;

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

#[derive(Debug, Clone, Copy)]
struct ListContinuationContext {
    level: u8,
}

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

    /// Reserves a footnote number before recursively encoding its body.
    fn reserve_footnote(&mut self) -> usize {
        let n = self.footnotes.len() + 1;
        self.footnotes.push(String::new());
        n
    }

    /// Fills a reserved footnote body and returns the inline marker `[^N]`.
    fn complete_footnote(&mut self, n: usize, body: &str) -> String {
        self.footnotes[n - 1] = body.to_string();
        format!("[^{n}]")
    }

    /// Reserves an endnote number before recursively encoding its body.
    fn reserve_endnote(&mut self) -> usize {
        let n = self.endnotes.len() + 1;
        self.endnotes.push(String::new());
        n
    }

    /// Fills a reserved endnote body and returns the inline marker `[^eN]`.
    fn complete_endnote(&mut self, n: usize, body: &str) -> String {
        self.endnotes[n - 1] = body.to_string();
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
        let mut continuation: Option<ListContinuationContext> = None;

        for paragraph in &section.paragraphs {
            // Page break
            if paragraph.page_break {
                // Flush code block first
                if !code_block_lines.is_empty() {
                    blocks.push(format!("```\n{}\n```", code_block_lines.join("\n")));
                    code_block_lines.clear();
                }
                blocks.push("---".to_string());
                continuation = None;
            }

            // Code block detection: all text runs with code font.
            // InlineText runs (post-Phase-3 inline-tab carry) fold to
            // their `\t`-bearing plain string — see debug doc
            // §3a-B13.
            if is_code_paragraph(paragraph, styles) {
                let text = paragraph
                    .runs
                    .iter()
                    .filter_map(|r| r.content.plain_text())
                    .map(|cow| cow.into_owned())
                    .collect::<String>();
                code_block_lines.push(text);
                continuation = None;
                continue;
            }

            // Flush accumulated code block
            if !code_block_lines.is_empty() {
                blocks.push(format!("```\n{}\n```", code_block_lines.join("\n")));
                code_block_lines.clear();
            }

            let continuation_level = continuation
                .and_then(|ctx| continuation_level_for_paragraph(paragraph, styles, ctx));
            let (markdown, para_images) =
                encode_paragraph_styled(paragraph, styles, &mut footnotes);
            if !markdown.trim().is_empty() {
                if let Some(level) = continuation_level {
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
            images.extend(para_images);

            if continuation_level.is_none() {
                continuation = list_context_for_paragraph(paragraph, styles);
            }
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

    // Shared outline/list classification (hwpforge-core). Classification is
    // text-blind; the empty-text guards below are rendering concerns (an
    // empty heading/list item renders as nothing).
    match classify_paragraph(paragraph, styles) {
        ParaKind::Heading { level, .. } => {
            // Headings must be single-line: collapse lineBreak-originated newlines.
            let heading_text = text.trim().replace('\n', " ");
            if heading_text.is_empty() {
                return (String::new(), images);
            }
            (format!("{} {}", "#".repeat(level as usize), heading_text), images)
        }
        ParaKind::ListItem { kind, level, checked, .. } => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return (String::new(), images);
            }
            (format_list_item(trimmed, kind, level, checked), images)
        }
        ParaKind::Body => (text.trim_start().to_string(), images),
    }
}

fn list_context_for_paragraph(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
) -> Option<ListContinuationContext> {
    let para_shape_id = paragraph.para_shape_id;
    let _list_type = styles.para_list_type(para_shape_id)?;
    let level = styles.para_list_level(para_shape_id).unwrap_or(0);
    Some(ListContinuationContext { level })
}

fn continuation_level_for_paragraph(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    context: ListContinuationContext,
) -> Option<u8> {
    let para_shape_id = paragraph.para_shape_id;
    if styles.para_heading_level(para_shape_id).is_some()
        || styles.para_list_type(para_shape_id).is_some()
    {
        return None;
    }

    styles
        .para_style_name(para_shape_id)
        .and_then(parse_list_continuation_style_name)
        .filter(|level| *level == context.level)
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
            // InlineText folds to the same `\t`-bearing string via
            // `plain_text()` because Markdown styling has no
            // representation for `<hp:tab>` attributes. The
            // formatting group key still uses `char_shape_id`, so the
            // run gets wrapped exactly like a plain `Text(String)` run
            // — see debug doc §3a-A5.
            RunContent::Text(_) | RunContent::InlineText(_) => {
                let Some(cow) = run.content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(run.char_shape_id, styles);
                if fmt == current_format {
                    current_text.push_str(&cow);
                } else {
                    // Flush previous group.
                    if !current_text.is_empty() {
                        output.push_str(&current_format.wrap(&current_text));
                        current_text.clear();
                    }
                    current_format = fmt;
                    current_text.push_str(&cow);
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
    images: &mut HashMap<String, Vec<u8>>,
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
            let number = footnotes.reserve_footnote();
            let body = encode_nested_paragraphs(paragraphs, styles, images, footnotes);
            footnotes.complete_footnote(number, body.trim())
        }
        Control::Endnote { paragraphs, .. } => {
            let number = footnotes.reserve_endnote();
            let body = encode_nested_paragraphs(paragraphs, styles, images, footnotes);
            footnotes.complete_endnote(number, body.trim())
        }
        Control::TextBox { paragraphs, .. } => {
            let body = encode_nested_paragraphs(paragraphs, styles, images, footnotes);
            body.trim().to_string()
        }
        Control::Equation { script, .. } => eqn_to_latex(script),
        Control::Chart { .. } => "<!-- chart -->".to_string(),
        Control::Line { .. } => String::new(),
        Control::Ellipse { paragraphs, .. } | Control::Polygon { paragraphs, .. } => {
            let body = encode_nested_paragraphs(paragraphs, styles, images, footnotes);
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
        Control::CrossRef { target, display_text, .. } => {
            // Wave 12m Phase 2 Step 4: display_text 추가. 사용자가 본
            // visible body text 가 있으면 우선 사용 (e.g. "1" 페이지
            // 번호, "see Section 1"); 비어 있으면 target 의 as_display()
            // 로 fallback (Name → "bookmark1", SystemId → "#5").
            // Markdown 은 anchor 링크 의미가 없으니 plain text 만 emit.
            if display_text.is_empty() {
                format!("[{}]", target.as_display())
            } else {
                display_text.clone()
            }
        }
        Control::Field { hint_text, display_text, .. } => {
            // 채워진 값(display_text)이 있으면 그것을, 없으면 힌트를 보인다
            // — HWPX 인코더의 hint 폴백과 동일한 우선순위 (Epic 1).
            if display_text.is_empty() {
                hint_text.as_deref().unwrap_or("____").to_string()
            } else {
                display_text.clone()
            }
        }
        Control::Bookmark { .. } => {
            // Bookmarks are invisible anchors — emit nothing.
            String::new()
        }
        Control::Memo { content, .. } => {
            // Memo content is hidden inside an HTML comment. Keep note
            // registrations local as well, otherwise an invisible memo
            // reference leaks its body into document-level definitions.
            let mut memo_footnotes = FootnoteCollector::new();
            let mut memo_images = HashMap::new();
            let body =
                encode_nested_paragraphs(content, styles, &mut memo_images, &mut memo_footnotes);
            let trimmed = body.trim();
            if trimmed.is_empty() {
                String::new()
            } else {
                // Sanitize body to prevent HTML comment breakout via `-->`.
                // Author/date are no longer carried (Wave 12e-Memo): HWPX wire
                // never surfaced them, so omit the `(author)` segment.
                let safe_body = trimmed.replace("--", "\\-\\-");
                format!("<!-- memo: {safe_body} -->")
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

/// Encodes paragraphs nested in controls while retaining their extracted images
/// and any nested footnote/endnote definitions in the document-level collectors.
fn encode_nested_paragraphs(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    images: &mut HashMap<String, Vec<u8>>,
    footnotes: &mut FootnoteCollector,
) -> String {
    paragraphs
        .iter()
        .map(|paragraph| {
            let (markdown, paragraph_images) = paragraph_text_styled(paragraph, styles, footnotes);
            images.extend(paragraph_images);
            markdown
        })
        .collect::<Vec<_>>()
        .join(" ")
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
            // Same as the Markdown styled path (debug doc §3a-A6):
            // `InlineText` folds to its `\t`-bearing plain string via
            // `plain_text()` so HTML output stays consistent.
            RunContent::Text(_) | RunContent::InlineText(_) => {
                let Some(cow) = run.content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(run.char_shape_id, styles);
                if fmt == current_format {
                    current_text.push_str(&cow);
                } else {
                    if !current_text.is_empty() {
                        output.push_str(&current_format.wrap_html(&current_text));
                        current_text.clear();
                    }
                    current_format = fmt;
                    current_text.push_str(&cow);
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
    // `carries_text()` matches both `Text(String)` and `InlineText(...)`
    // so a paragraph that picked up `InlineText` after the HWPX
    // decoder Phase 3 carry is still detected as code if it would
    // have qualified before — see debug doc §3a-B13.
    let text_runs: Vec<_> = paragraph.runs.iter().filter(|r| r.content.carries_text()).collect();
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
        list_para_checked: HashMap<usize, bool>,
        heading_paras: HashMap<usize, u8>,
        heading_styles: HashMap<usize, u8>,
        style_names: HashMap<usize, String>,
        para_style_names: HashMap<usize, String>,
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
                list_para_checked: HashMap::new(),
                heading_paras: HashMap::new(),
                heading_styles: HashMap::new(),
                style_names: HashMap::new(),
                para_style_names: HashMap::new(),
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

        fn para_checked_state(&self, id: ParaShapeIndex) -> Option<bool> {
            self.list_para_checked.get(&id.get()).copied()
        }

        fn para_style_name(&self, id: ParaShapeIndex) -> Option<&str> {
            self.para_style_names.get(&id.get()).map(String::as_str)
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

    #[test]
    fn para_shape_checkable_bullet_formats_as_task_list() {
        let doc = validated_document(vec![
            Paragraph::with_runs(
                vec![Run::text("Todo", CharShapeIndex::new(0))],
                ParaShapeIndex::new(1),
            ),
            Paragraph::with_runs(
                vec![Run::text("Done", CharShapeIndex::new(0))],
                ParaShapeIndex::new(2),
            ),
        ]);
        let mut styles = MockStyles::new();
        styles.list_para_types.insert(1, "BULLET");
        styles.list_para_levels.insert(1, 0);
        styles.list_para_checked.insert(1, false);
        styles.list_para_types.insert(2, "BULLET");
        styles.list_para_levels.insert(2, 1);
        styles.list_para_checked.insert(2, true);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "- [ ] Todo\n\n  - [x] Done");
    }

    #[test]
    fn continuation_paragraph_after_checkable_item_stays_in_same_markdown_item() {
        let doc = validated_document(vec![
            Paragraph::with_runs(
                vec![Run::text("first paragraph of the same task item", CharShapeIndex::new(0))],
                ParaShapeIndex::new(1),
            ),
            Paragraph::with_runs(
                vec![Run::text("second paragraph of the same task item", CharShapeIndex::new(0))],
                ParaShapeIndex::new(2),
            ),
            Paragraph::with_runs(
                vec![Run::text("next real task item", CharShapeIndex::new(0))],
                ParaShapeIndex::new(3),
            ),
        ]);
        let mut styles = MockStyles::new();
        styles.list_para_types.insert(1, "BULLET");
        styles.list_para_levels.insert(1, 0);
        styles.list_para_checked.insert(1, false);
        styles.para_style_names.insert(2, "__hwpforge_md_list_continuation_level_0".to_string());
        styles.list_para_types.insert(3, "BULLET");
        styles.list_para_levels.insert(3, 0);
        styles.list_para_checked.insert(3, true);

        let output = encode_styled(&doc, &styles);
        assert_eq!(
            output.markdown,
            "- [ ] first paragraph of the same task item\n\n  second paragraph of the same task item\n\n- [x] next real task item"
        );
    }

    #[test]
    fn numbered_list_ignores_checked_state() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("Numbered", CharShapeIndex::new(0))],
            ParaShapeIndex::new(3),
        )]);
        let mut styles = MockStyles::new();
        styles.list_para_types.insert(3, "NUMBER");
        styles.list_para_levels.insert(3, 0);
        styles.list_para_checked.insert(3, true);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "1. Numbered");
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
    fn nested_footnote_numbers_follow_visible_reference_order() {
        let inner_body = Paragraph::with_runs(
            vec![Run::text("inner", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let outer_body = Paragraph::with_runs(
            vec![
                Run::text("outer ", CharShapeIndex::new(0)),
                Run::control(
                    Control::Footnote { inst_id: None, paragraphs: vec![inner_body] },
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote { inst_id: None, paragraphs: vec![outer_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let output = encode_styled(&doc, &MockStyles::new());

        assert_eq!(output.markdown, "[^1]\n\n[^1]: outer [^2]\n[^2]: inner");
    }

    #[test]
    fn nested_endnote_numbers_follow_visible_reference_order() {
        let inner_body = Paragraph::with_runs(
            vec![Run::text("inner", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let outer_body = Paragraph::with_runs(
            vec![
                Run::text("outer ", CharShapeIndex::new(0)),
                Run::control(
                    Control::Endnote { inst_id: None, paragraphs: vec![inner_body] },
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Endnote { inst_id: None, paragraphs: vec![outer_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let output = encode_styled(&doc, &MockStyles::new());

        assert_eq!(output.markdown, "[^e1]\n\n[^e1]: outer [^e2]\n[^e2]: inner");
    }

    #[test]
    fn control_endnote_extracts_nested_images() {
        let mut styles = MockStyles::new();
        styles.image_data.insert("BinData/graph.png".to_string(), vec![0x89, 0x50, 0x4E]);
        let graph = Image::new(
            "BinData/graph.png",
            HwpUnit::from_mm(40.0).unwrap(),
            HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Png,
        );
        let endnote_body = Paragraph::with_runs(
            vec![
                Run::text("solution", CharShapeIndex::new(0)),
                Run::image(graph, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Endnote { inst_id: None, paragraphs: vec![endnote_body] },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "[^e1]\n\n[^e1]: solution ![graph](images/graph.png)");
        assert_eq!(output.images.get("images/graph.png"), Some(&vec![0x89, 0x50, 0x4E]));
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
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
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
    fn nested_control_paragraph_styles_remain_inline() {
        let textbox_body = Paragraph::with_runs(
            vec![Run::text("box heading", CharShapeIndex::new(0))],
            ParaShapeIndex::new(1),
        );
        let footnote_body = Paragraph::with_runs(
            vec![Run::text("note item", CharShapeIndex::new(0))],
            ParaShapeIndex::new(2),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![
                Run::control(
                    Control::TextBox {
                        paragraphs: vec![textbox_body],
                        width: HwpUnit::from_mm(80.0).unwrap(),
                        height: HwpUnit::from_mm(40.0).unwrap(),
                        horz_offset: 0,
                        vert_offset: 0,
                        caption: None,
                        style: None,
                        text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                    },
                    CharShapeIndex::new(0),
                ),
                Run::text(" ", CharShapeIndex::new(0)),
                Run::control(
                    Control::Footnote { inst_id: None, paragraphs: vec![footnote_body] },
                    CharShapeIndex::new(0),
                ),
            ],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = MockStyles::new();
        styles.heading_paras.insert(1, 1);
        styles.list_para_types.insert(2, "bullet");
        styles.list_para_levels.insert(2, 0);

        let output = encode_styled(&doc, &styles);

        assert_eq!(output.markdown, "box heading [^1]\n\n[^1]: note item");
        assert!(!output.markdown.contains("# box heading"));
        assert!(!output.markdown.contains("[^1]: - note item"));
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
                    metadata: hwpforge_core::DutmalMetadata::default(),
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
                    char_pr_ids: vec![u32::MAX; 10],
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
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
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
                    text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
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
                    inst_id: None,
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

    // -----------------------------------------------------------------------
    // Additional coverage tests for previously uncovered branches
    // -----------------------------------------------------------------------

    /// A MockStyles extension that supports underline, super/subscript, and font name.
    struct ExtendedMockStyles {
        base: MockStyles,
        underline_ids: Vec<usize>,
        superscript_ids: Vec<usize>,
        subscript_ids: Vec<usize>,
        font_names: HashMap<usize, String>,
        image_filenames: HashMap<String, String>,
    }

    impl ExtendedMockStyles {
        fn new() -> Self {
            Self {
                base: MockStyles::new(),
                underline_ids: Vec::new(),
                superscript_ids: Vec::new(),
                subscript_ids: Vec::new(),
                font_names: HashMap::new(),
                image_filenames: HashMap::new(),
            }
        }
    }

    impl StyleLookup for ExtendedMockStyles {
        fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
            self.base.char_bold(id)
        }
        fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
            self.base.char_italic(id)
        }
        fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
            self.base.char_strikeout(id)
        }
        fn char_underline(&self, id: CharShapeIndex) -> Option<hwpforge_foundation::UnderlineType> {
            if self.underline_ids.contains(&id.get()) {
                Some(hwpforge_foundation::UnderlineType::Bottom)
            } else {
                None
            }
        }
        fn char_superscript(&self, id: CharShapeIndex) -> Option<bool> {
            Some(self.superscript_ids.contains(&id.get()))
        }
        fn char_subscript(&self, id: CharShapeIndex) -> Option<bool> {
            Some(self.subscript_ids.contains(&id.get()))
        }
        fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
            self.font_names.get(&id.get()).map(String::as_str)
        }
        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.base.para_heading_level(id)
        }
        fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
            self.base.para_list_type(id)
        }
        fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.base.para_list_level(id)
        }
        fn para_checked_state(&self, id: ParaShapeIndex) -> Option<bool> {
            self.base.para_checked_state(id)
        }
        fn para_style_name(&self, id: ParaShapeIndex) -> Option<&str> {
            self.base.para_style_name(id)
        }
        fn style_name(&self, id: StyleIndex) -> Option<&str> {
            self.base.style_name(id)
        }
        fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
            self.base.style_heading_level(id)
        }
        fn image_data(&self, key: &str) -> Option<&[u8]> {
            self.base.image_data(key)
        }
        fn image_resolve_filename(&self, key: &str) -> Option<&str> {
            self.image_filenames.get(key).map(String::as_str)
        }
    }

    // --- InlineFormat underline/superscript/subscript wrap paths ---

    #[test]
    fn inline_format_underline_wraps_as_html_u() {
        let fmt = InlineFormat { underline: true, ..Default::default() };
        assert_eq!(fmt.wrap("underlined"), "<u>underlined</u>");
    }

    #[test]
    fn inline_format_superscript_wraps_as_html_sup() {
        let fmt = InlineFormat { superscript: true, ..Default::default() };
        assert_eq!(fmt.wrap("sup text"), "<sup>sup text</sup>");
    }

    #[test]
    fn inline_format_subscript_wraps_as_html_sub() {
        let fmt = InlineFormat { subscript: true, ..Default::default() };
        assert_eq!(fmt.wrap("sub text"), "<sub>sub text</sub>");
    }

    #[test]
    fn inline_format_bold_and_underline_combines_markdown_and_html() {
        let fmt = InlineFormat { bold: true, underline: true, ..Default::default() };
        let result = fmt.wrap("text");
        assert!(result.contains("<u>"), "expected <u> in: {result}");
    }

    #[test]
    fn inline_format_wrap_html_underline() {
        let fmt = InlineFormat { underline: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<u>text</u>");
    }

    #[test]
    fn inline_format_wrap_html_superscript() {
        let fmt = InlineFormat { superscript: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<sup>text</sup>");
    }

    #[test]
    fn inline_format_wrap_html_subscript() {
        let fmt = InlineFormat { subscript: true, ..Default::default() };
        assert_eq!(fmt.wrap_html("text"), "<sub>text</sub>");
    }

    // --- is_code_paragraph via encode_styled ---

    #[test]
    fn code_font_paragraph_encoded_as_code_block() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("let x = 1;", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = ExtendedMockStyles::new();
        styles.font_names.insert(1, "D2Coding".to_string());

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("```"), "expected code block markers");
        assert!(output.markdown.contains("let x = 1;"));
    }

    #[test]
    fn code_block_flushed_at_section_end() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("code line", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = ExtendedMockStyles::new();
        styles.font_names.insert(1, "Consolas".to_string());

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("```\ncode line\n```"));
    }

    // --- Page break handling ---

    #[test]
    fn page_break_paragraph_emits_horizontal_rule() {
        let mut p1 = Paragraph::with_runs(
            vec![Run::text("before break", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        p1.page_break = true;
        let p2 = Paragraph::with_runs(
            vec![Run::text("after break", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![p1, p2]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("---"), "expected --- for page break");
    }

    #[test]
    fn page_break_flushes_pending_code_block() {
        let code_para = Paragraph::with_runs(
            vec![Run::text("code", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        );
        let mut break_para = Paragraph::with_runs(
            vec![Run::text("break", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        break_para.page_break = true;
        let doc = validated_document(vec![code_para, break_para]);
        let mut styles = ExtendedMockStyles::new();
        styles.font_names.insert(1, "D2Coding".to_string());

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("```"), "code block should be present");
        assert!(output.markdown.contains("---"), "page break marker should be present");
    }

    // --- escape_markdown helpers ---

    #[test]
    fn escape_markdown_escapes_special_chars() {
        let result = escape_markdown("*bold* and _italic_ and `code`");
        assert!(result.contains("\\*bold\\*"));
        assert!(result.contains("\\_italic\\_"));
        assert!(result.contains("\\`code\\`"));
    }

    #[test]
    fn escape_markdown_escapes_angle_brackets() {
        let result = escape_markdown("<tag>");
        assert_eq!(result, "&lt;tag&gt;");
    }

    #[test]
    fn escape_markdown_escapes_hash() {
        let result = escape_markdown("# heading");
        assert!(result.contains("\\#"));
    }

    #[test]
    fn escape_markdown_passes_through_normal_text() {
        let result = escape_markdown("hello world 안녕하세요");
        assert_eq!(result, "hello world 안녕하세요");
    }

    // --- escape_html ---

    #[test]
    fn escape_html_escapes_all_special_chars() {
        let result = escape_html("<script>alert('xss\" & stuff');</script>");
        assert!(result.contains("&lt;script&gt;"));
        assert!(result.contains("&amp;"));
        assert!(result.contains("&quot;"));
        assert!(result.contains("&#39;"));
    }

    // --- escape_gfm_cell ---

    #[test]
    fn escape_gfm_cell_escapes_pipe_and_newline() {
        let result = escape_gfm_cell("a|b\nc");
        assert_eq!(result, "a\\|b<br>c");
    }

    // --- image_alt_text and image_rel_path ---

    #[test]
    fn image_alt_text_extracts_stem_from_path() {
        assert_eq!(image_alt_text("images/photo.jpg"), "photo");
        assert_eq!(image_alt_text("BinData/logo.png"), "logo");
        assert_eq!(image_alt_text("simple"), "simple");
    }

    #[test]
    fn image_alt_text_escapes_brackets_in_stem() {
        assert_eq!(image_alt_text("[photo].png"), "\\[photo\\]");
    }

    #[test]
    fn image_rel_path_falls_back_to_basename() {
        let styles = MockStyles::new();
        let result = image_rel_path("BinData/photo.jpg", &styles);
        assert_eq!(result, "images/photo.jpg");
    }

    #[test]
    fn image_rel_path_uses_resolved_filename_when_available() {
        let mut styles = ExtendedMockStyles::new();
        styles.image_filenames.insert("BinData/img1".to_string(), "img1.png".to_string());
        let result = image_rel_path("BinData/img1", &styles);
        assert_eq!(result, "images/img1.png");
    }

    #[test]
    fn image_rel_path_escapes_parentheses_in_filename() {
        let styles = MockStyles::new();
        let result = image_rel_path("path/file(1).jpg", &styles);
        assert_eq!(result, "images/file%281%29.jpg");
    }

    // --- has_nested_table ---

    #[test]
    fn has_nested_table_false_for_plain_table() {
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("cell", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(30.0).unwrap(),
        )])]);
        assert!(!has_nested_table(&table));
    }

    #[test]
    fn has_nested_table_true_for_table_in_cell() {
        let inner_table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            HwpUnit::from_mm(20.0).unwrap(),
        )])]);
        let outer_table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::table(inner_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(60.0).unwrap(),
        )])]);
        assert!(has_nested_table(&outer_table));
    }

    #[test]
    fn nested_table_in_cell_renders_html() {
        let inner_table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("inner", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(20.0).unwrap(),
        )])]);
        let outer_table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::table(inner_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(60.0).unwrap(),
        )])]);
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::table(outer_table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<table>"), "nested table should render as HTML");
    }

    // --- Control variants: Memo, Field, Bookmark, CrossRef, Arc, Curve, ConnectLine, IndexMark ---

    #[test]
    fn control_memo_with_content_renders_comment() {
        use hwpforge_core::control::MemoMetadata;
        let memo_body = Paragraph::with_runs(
            vec![Run::text("memo note", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Memo {
                    content: vec![memo_body],
                    anchor_runs: vec![],
                    metadata: MemoMetadata::default(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(
            output.markdown.contains("<!-- memo:"),
            "memo with content should render as HTML comment, got: {}",
            output.markdown
        );
        assert!(output.markdown.contains("memo note"));
    }

    #[test]
    fn control_memo_discards_nested_footnote_definitions() {
        use hwpforge_core::control::MemoMetadata;
        let hidden_footnote = Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote {
                    inst_id: None,
                    paragraphs: vec![Paragraph::with_runs(
                        vec![Run::text("secret memo footnote", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        let visible_footnote = Paragraph::with_runs(
            vec![Run::control(
                Control::Footnote {
                    inst_id: None,
                    paragraphs: vec![Paragraph::with_runs(
                        vec![Run::text("visible footnote", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![
            Paragraph::with_runs(
                vec![Run::control(
                    Control::Memo {
                        content: vec![hidden_footnote],
                        anchor_runs: vec![],
                        metadata: MemoMetadata::default(),
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            ),
            visible_footnote,
        ]);

        let output = encode_styled(&doc, &MockStyles::new());
        assert!(!output.markdown.contains("secret memo footnote"));
        assert!(output.markdown.contains("[^1]: visible footnote"));
        assert!(!output.markdown.contains("[^2]:"));
    }

    #[test]
    fn control_memo_discards_nested_image_artifacts() {
        use hwpforge_core::control::MemoMetadata;

        let mut styles = MockStyles::new();
        styles.image_data.insert("BinData/secret.png".to_string(), vec![0x89, 0x50, 0x4E]);
        let image = Image::new(
            "BinData/secret.png",
            HwpUnit::from_mm(40.0).unwrap(),
            HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Png,
        );
        let memo_body = Paragraph::with_runs(
            vec![Run::image(image, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Memo {
                    content: vec![memo_body],
                    anchor_runs: vec![],
                    metadata: MemoMetadata::default(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("![secret](images/secret.png)"));
        assert!(output.images.is_empty(), "memo images must remain local to the memo body");
    }

    #[test]
    fn control_memo_with_empty_content_emits_empty() {
        use hwpforge_core::control::MemoMetadata;
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Memo {
                    content: vec![Paragraph::with_runs(
                        vec![Run::text("   ", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    anchor_runs: vec![],
                    metadata: MemoMetadata::default(),
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
    fn control_field_with_hint_text_renders_hint() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text: Some("날짜 입력".to_string()),
                    help_text: None,
                    name: None,
                    display_text: String::new(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "날짜 입력");
    }

    #[test]
    fn control_field_without_hint_text_renders_placeholder() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text: None,
                    help_text: None,
                    name: None,
                    display_text: String::new(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "____");
    }

    #[test]
    fn control_bookmark_emits_nothing() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Bookmark {
                    name: "anchor1".to_string(),
                    bookmark_type: hwpforge_foundation::BookmarkType::Point,
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
    fn control_crossref_with_display_text_emits_display_text() {
        use hwpforge_core::control::RefTarget;
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::CrossRef {
                    target: RefTarget::Name("sec1".to_string()),
                    ref_type: hwpforge_foundation::RefType::Bookmark,
                    content_type: hwpforge_foundation::RefContentType::Page,
                    as_hyperlink: false,
                    display_text: "see Section 1".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "see Section 1");
    }

    #[test]
    fn control_crossref_without_display_text_emits_bracket_target() {
        use hwpforge_core::control::RefTarget;
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::CrossRef {
                    target: RefTarget::Name("anchor1".to_string()),
                    ref_type: hwpforge_foundation::RefType::Bookmark,
                    content_type: hwpforge_foundation::RefContentType::Page,
                    as_hyperlink: false,
                    display_text: String::new(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(
            output.markdown.contains("[anchor1]"),
            "empty display_text should produce bracketed fallback, got: {}",
            output.markdown
        );
    }

    #[test]
    fn control_index_mark_emits_nothing() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::IndexMark { primary: "Rust".to_string(), secondary: None },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    #[test]
    fn control_arc_emits_nothing() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Arc {
                    arc_type: hwpforge_foundation::ArcType::Normal,
                    center: ShapePoint { x: 500, y: 300 },
                    axis1: ShapePoint { x: 1000, y: 300 },
                    axis2: ShapePoint { x: 500, y: 600 },
                    start1: ShapePoint { x: 750, y: 300 },
                    end1: ShapePoint { x: 500, y: 550 },
                    start2: ShapePoint { x: 750, y: 300 },
                    end2: ShapePoint { x: 500, y: 550 },
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
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    #[test]
    fn control_curve_emits_nothing() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Curve {
                    points: vec![
                        ShapePoint { x: 0, y: 0 },
                        ShapePoint { x: 500, y: 200 },
                        ShapePoint { x: 1000, y: 0 },
                    ],
                    segment_types: vec![
                        hwpforge_foundation::CurveSegmentType::Curve,
                        hwpforge_foundation::CurveSegmentType::Line,
                    ],
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
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    #[test]
    fn control_connect_line_emits_nothing() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::ConnectLine {
                    start: ShapePoint { x: 0, y: 0 },
                    end: ShapePoint { x: 1000, y: 500 },
                    control_points: vec![],
                    connect_type: "STRAIGHT".to_string(),
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
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    // --- Style-name list fallback in encode_paragraph_styled ---

    #[test]
    fn style_name_bullet_pattern_formats_as_list() {
        let para = Paragraph::with_runs(
            vec![Run::text("list item", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(5));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.style_names.insert(5, "글머리 기호".to_string());

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "- list item");
    }

    #[test]
    fn style_name_numbered_pattern_formats_as_numbered_list() {
        let para = Paragraph::with_runs(
            vec![Run::text("step", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(6));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.style_names.insert(6, "번호 목록".to_string());

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "1. step");
    }

    // --- Underline/super/subscript via encode_styled ---

    #[test]
    fn underline_text_wraps_as_html_u_in_output() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("underlined text", CharShapeIndex::new(1))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = ExtendedMockStyles::new();
        styles.underline_ids.push(1);

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<u>underlined text</u>"));
    }

    #[test]
    fn superscript_text_wraps_as_html_sup_in_output() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("sup text", CharShapeIndex::new(2))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = ExtendedMockStyles::new();
        styles.superscript_ids.push(2);

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<sup>sup text</sup>"));
    }

    #[test]
    fn subscript_text_wraps_as_html_sub_in_output() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("sub text", CharShapeIndex::new(3))],
            ParaShapeIndex::new(0),
        )]);
        let mut styles = ExtendedMockStyles::new();
        styles.subscript_ids.push(3);

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("<sub>sub text</sub>"));
    }

    // --- empty heading text is skipped ---

    #[test]
    fn heading_with_empty_text_emits_nothing_from_para_shape() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::text("   ", CharShapeIndex::new(0))],
            ParaShapeIndex::new(5),
        )]);
        let mut styles = MockStyles::new();
        styles.heading_paras.insert(5, 2);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    #[test]
    fn heading_with_empty_text_emits_nothing_from_style_id() {
        let para = Paragraph::with_runs(
            vec![Run::text("  ", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(3));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(3, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    // --- Memo body sanitizes double-dash ---

    #[test]
    fn control_memo_sanitizes_double_dash_to_prevent_comment_breakout() {
        use hwpforge_core::control::MemoMetadata;
        let memo_body = Paragraph::with_runs(
            vec![Run::text("bad -- content", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Memo {
                    content: vec![memo_body],
                    anchor_runs: vec![],
                    metadata: MemoMetadata::default(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        // The raw double-dash in the memo body should be escaped to \-\-
        assert!(output.markdown.contains("\\-\\-"), "double-dash should be escaped");
        // The unescaped " -- " sequence (space-dash-dash-space) should not appear in the body
        assert!(
            !output.markdown.contains(" -- "),
            "raw double-dash should not appear in memo body"
        );
    }

    // --- Hyperlink URL escaping ---

    #[test]
    fn hyperlink_url_parentheses_are_percent_escaped() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "link".to_string(),
                    url: "https://example.com/path(1)".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("%28"), "( should be escaped as %28");
        assert!(output.markdown.contains("%29"), ") should be escaped as %29");
    }

    #[test]
    fn hyperlink_vbscript_url_rejected() {
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![Run::control(
                Control::Hyperlink {
                    text: "evil".to_string(),
                    url: "vbscript:msgbox(1)".to_string(),
                },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(!output.markdown.contains("]("), "vbscript: URL should be rejected");
        assert_eq!(output.markdown, "evil");
    }

    // --- Inline image in paragraph (not standalone) ---

    #[test]
    fn inline_image_in_mixed_paragraph_renders_inline() {
        use hwpforge_core::{Image, ImageFormat};
        let image = Image::new(
            "BinData/icon.png",
            HwpUnit::from_mm(20.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
            ImageFormat::Png,
        );
        let doc = validated_document(vec![Paragraph::with_runs(
            vec![
                Run::text("See ", CharShapeIndex::new(0)),
                Run::image(image, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )]);
        let styles = MockStyles::new();

        let output = encode_styled(&doc, &styles);
        assert!(output.markdown.contains("See"), "prefix text should be present");
        assert!(output.markdown.contains("![icon](images/icon.png)"), "inline image should render");
    }
}
