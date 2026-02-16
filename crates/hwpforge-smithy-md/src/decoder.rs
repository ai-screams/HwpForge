//! Markdown -> Core decoder.

use std::path::Path;

use hwpforge_blueprint::template::Template;
use hwpforge_core::{
    Control, Document, Image, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::HwpUnit;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::error::{MdError, MdResult};
use crate::frontmatter::{apply_to_metadata, extract_frontmatter};
use crate::mapper::{image_format_from_path, resolve_mapping, MdMapping, MdStyleRef};

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

/// Markdown decoder.
pub struct MdDecoder;

impl MdDecoder {
    /// Decodes markdown into a Core draft document.
    ///
    /// The template is used for paragraph/character style index mapping.
    /// Built-in template inheritance (`default`/`gov_proposal`) is resolved
    /// automatically.
    pub fn decode(markdown: &str, template: &Template) -> MdResult<Document> {
        let extracted = extract_frontmatter(markdown)?;
        let mapping = resolve_mapping(template)?;

        let mut document = Document::new();
        if let Some(frontmatter) = extracted.frontmatter.as_ref() {
            apply_to_metadata(frontmatter, document.metadata_mut());
        }

        let mut state = DecoderState::new(&mapping);
        state.decode_markdown(extracted.content)?;
        let decoded = state.finish()?;

        let mut sections = split_sections(decoded.paragraphs, &decoded.section_breaks);
        if sections.is_empty() {
            sections.push(vec![empty_paragraph(mapping.body)]);
        }

        for mut section_paragraphs in sections {
            if section_paragraphs.is_empty() {
                section_paragraphs.push(empty_paragraph(mapping.body));
            }
            document
                .add_section(Section::with_paragraphs(section_paragraphs, mapping.page_settings));
        }

        Ok(document)
    }

    /// Reads a markdown file and decodes it into a Core draft document.
    pub fn decode_file(path: impl AsRef<Path>, template: &Template) -> MdResult<Document> {
        let markdown = std::fs::read_to_string(path)?;
        Self::decode(&markdown, template)
    }
}

#[derive(Debug, Clone)]
struct ListState {
    ordered: bool,
    next_index: u64,
}

impl ListState {
    fn new(start: Option<u64>) -> Self {
        Self { ordered: start.is_some(), next_index: start.unwrap_or(1) }
    }
}

#[derive(Debug, Clone)]
struct PendingLink {
    dest_url: String,
    text: String,
}

#[derive(Debug, Clone)]
struct PendingImage {
    dest_url: String,
    alt: String,
}

#[derive(Debug, Clone)]
struct ParagraphBuilder {
    style: MdStyleRef,
    runs: Vec<Run>,
}

impl ParagraphBuilder {
    fn new(style: MdStyleRef) -> Self {
        Self { style, runs: Vec::new() }
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        if let Some(last) = self.runs.last_mut() {
            if let RunContent::Text(existing) = &mut last.content {
                if last.char_shape_id == self.style.char_shape_id {
                    existing.push_str(text);
                    return;
                }
            }
        }

        self.runs.push(Run::text(text, self.style.char_shape_id));
    }

    fn push_run(&mut self, run: Run) {
        self.runs.push(run);
    }

    fn build(mut self) -> Paragraph {
        if self.runs.is_empty() {
            self.runs.push(Run::text("", self.style.char_shape_id));
        }
        Paragraph::with_runs(self.runs, self.style.para_shape_id)
    }
}

#[derive(Debug, Clone)]
struct TableBuilder {
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    row_open: bool,
    cell_open: bool,
}

impl TableBuilder {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            row_open: false,
            cell_open: false,
        }
    }

    fn start_row(&mut self) {
        self.end_cell();
        self.end_row();
        self.current_row.clear();
        self.row_open = true;
    }

    fn end_row(&mut self) {
        self.end_cell();
        if self.row_open {
            self.rows.push(std::mem::take(&mut self.current_row));
            self.row_open = false;
        }
    }

    fn start_cell(&mut self) {
        self.end_cell();
        self.current_cell.clear();
        self.cell_open = true;
    }

    fn end_cell(&mut self) {
        if self.cell_open {
            self.current_row.push(std::mem::take(&mut self.current_cell));
            self.cell_open = false;
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.cell_open {
            self.current_cell.push_str(text);
        }
    }

    fn is_in_cell(&self) -> bool {
        self.cell_open
    }

    fn into_table(
        mut self,
        body_style: MdStyleRef,
        page: hwpforge_core::PageSettings,
    ) -> MdResult<Table> {
        self.end_row();

        if self.rows.is_empty() {
            self.rows.push(vec![String::new()]);
        }

        let max_cols = self.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let divisor = i32::try_from(max_cols).unwrap_or(1);
        let mut cell_width = page.printable_width() / divisor;
        if cell_width.as_i32() <= 0 {
            cell_width = HwpUnit::from_mm(40.0)?;
        }

        let table_rows = self
            .rows
            .into_iter()
            .map(|mut row| {
                if row.is_empty() {
                    row.push(String::new());
                }
                while row.len() < max_cols {
                    row.push(String::new());
                }

                let cells = row
                    .into_iter()
                    .map(|text| {
                        let paragraph = Paragraph::with_runs(
                            vec![Run::text(text, body_style.char_shape_id)],
                            body_style.para_shape_id,
                        );
                        TableCell::new(vec![paragraph], cell_width)
                    })
                    .collect();

                TableRow { cells, height: None }
            })
            .collect();

        Ok(Table::new(table_rows))
    }
}

#[derive(Debug)]
struct DecoderState<'a> {
    mapping: &'a MdMapping,
    paragraphs: Vec<Paragraph>,
    current: Option<ParagraphBuilder>,
    table: Option<TableBuilder>,
    blockquote_depth: usize,
    in_code_block: bool,
    in_item: bool,
    pending_item_prefix: Option<String>,
    list_stack: Vec<ListState>,
    pending_link: Option<PendingLink>,
    pending_image: Option<PendingImage>,
    table_link_stack: Vec<String>,
    table_image_stack: Vec<String>,
    section_breaks: Vec<usize>,
}

#[derive(Debug)]
struct DecodeOutput {
    paragraphs: Vec<Paragraph>,
    section_breaks: Vec<usize>,
}

impl<'a> DecoderState<'a> {
    fn new(mapping: &'a MdMapping) -> Self {
        Self {
            mapping,
            paragraphs: Vec::new(),
            current: None,
            table: None,
            blockquote_depth: 0,
            in_code_block: false,
            in_item: false,
            pending_item_prefix: None,
            list_stack: Vec::new(),
            pending_link: None,
            pending_image: None,
            table_link_stack: Vec::new(),
            table_image_stack: Vec::new(),
            section_breaks: Vec::new(),
        }
    }

    fn decode_markdown(&mut self, content: &str) -> MdResult<()> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_GFM);

        let parser = Parser::new_ext(content, options);
        for event in parser {
            self.handle_event(event)?;
        }
        Ok(())
    }

    fn finish(mut self) -> MdResult<DecodeOutput> {
        if self.table.is_some() {
            return Err(MdError::UnsupportedStructure {
                detail: "table was not properly closed".to_string(),
            });
        }

        self.finalize_paragraph();
        if self.paragraphs.is_empty() {
            self.paragraphs.push(ParagraphBuilder::new(self.mapping.body).build());
        }
        Ok(DecodeOutput { paragraphs: self.paragraphs, section_breaks: self.section_breaks })
    }

    fn handle_event(&mut self, event: Event<'_>) -> MdResult<()> {
        match event {
            Event::Start(tag) => self.start_tag(tag)?,
            Event::End(tag_end) => self.end_tag(tag_end)?,
            Event::Text(text) => self.push_text(text.as_ref())?,
            Event::Code(code) => self.push_inline_code(code.as_ref())?,
            Event::InlineMath(math) | Event::DisplayMath(math) => self.push_text(math.as_ref())?,
            Event::Html(html) | Event::InlineHtml(html) => {
                let raw = html.as_ref();
                let in_table_cell =
                    self.table.as_ref().map(TableBuilder::is_in_cell).unwrap_or(false);
                if in_table_cell {
                    self.push_text(raw)?;
                } else if raw.trim() == SECTION_MARKER_COMMENT {
                    self.push_section_marker();
                } else {
                    self.push_text(raw)?;
                }
            }
            Event::FootnoteReference(label) => {
                self.push_text(&format!("[^{}]", label.as_ref()))?;
            }
            Event::SoftBreak => self.push_soft_break()?,
            Event::HardBreak => self.push_hard_break()?,
            Event::Rule => self.push_rule(),
            Event::TaskListMarker(checked) => {
                self.push_text(if checked { "[x] " } else { "[ ] " })?;
            }
        }

        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>) -> MdResult<()> {
        match tag {
            Tag::Paragraph => {
                self.ensure_paragraph();
            }
            Tag::Heading { level, .. } => {
                self.start_paragraph(self.mapping.heading(level_to_u32(level)));
            }
            Tag::BlockQuote(_) => {
                self.blockquote_depth += 1;
            }
            Tag::CodeBlock(_) => {
                self.in_code_block = true;
                self.start_paragraph(self.mapping.code);
            }
            Tag::List(start) => {
                self.list_stack.push(ListState::new(start));
            }
            Tag::Item => {
                self.finalize_paragraph();
                self.in_item = true;
                self.pending_item_prefix = Some(self.next_item_prefix());
            }
            Tag::Table(_) => {
                self.finalize_paragraph();
                self.table = Some(TableBuilder::new());
            }
            Tag::TableHead => {}
            Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.start_row();
                }
            }
            Tag::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.start_cell();
                }
            }
            Tag::Link { dest_url, .. } => {
                let in_table_cell =
                    self.table.as_ref().map(TableBuilder::is_in_cell).unwrap_or(false);
                if in_table_cell {
                    if let Some(table) = self.table.as_mut() {
                        table.push_text("[");
                    }
                    self.table_link_stack.push(dest_url.to_string());
                    return Ok(());
                }

                self.ensure_paragraph();
                self.pending_link =
                    Some(PendingLink { dest_url: dest_url.to_string(), text: String::new() });
            }
            Tag::Image { dest_url, .. } => {
                let in_table_cell =
                    self.table.as_ref().map(TableBuilder::is_in_cell).unwrap_or(false);
                if in_table_cell {
                    if let Some(table) = self.table.as_mut() {
                        table.push_text("![");
                    }
                    self.table_image_stack.push(dest_url.to_string());
                    return Ok(());
                }

                self.ensure_paragraph();
                self.pending_image =
                    Some(PendingImage { dest_url: dest_url.to_string(), alt: String::new() });
            }
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => {}
        }
        Ok(())
    }

    fn end_tag(&mut self, tag_end: TagEnd) -> MdResult<()> {
        match tag_end {
            TagEnd::Paragraph => self.finalize_paragraph(),
            TagEnd::Heading(_) => self.finalize_paragraph(),
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.finalize_paragraph();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.finalize_paragraph();
                self.in_item = false;
                self.pending_item_prefix = None;
            }
            TagEnd::Table => self.finalize_table()?,
            TagEnd::TableHead => {}
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.end_row();
                }
            }
            TagEnd::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.end_cell();
                }
            }
            TagEnd::Link => {
                if let Some(dest_url) = self.table_link_stack.pop() {
                    if let Some(table) = self.table.as_mut() {
                        table.push_text(&format!("]({dest_url})"));
                    }
                    return Ok(());
                }

                if let Some(link) = self.pending_link.take() {
                    self.ensure_paragraph();
                    if let Some(current) = self.current.as_mut() {
                        current.push_run(Run::control(
                            Control::Hyperlink { text: link.text, url: link.dest_url },
                            current.style.char_shape_id,
                        ));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(dest_url) = self.table_image_stack.pop() {
                    if let Some(table) = self.table.as_mut() {
                        table.push_text(&format!("]({dest_url})"));
                    }
                    return Ok(());
                }

                if let Some(image) = self.pending_image.take() {
                    self.ensure_paragraph();
                    if let Some(current) = self.current.as_mut() {
                        let format = image_format_from_path(&image.dest_url);
                        let image = Image::new(
                            image.dest_url,
                            HwpUnit::from_mm(50.0)?,
                            HwpUnit::from_mm(30.0)?,
                            format,
                        );
                        current.push_run(Run::image(image, current.style.char_shape_id));
                    }
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {}
        }

        Ok(())
    }

    fn push_text(&mut self, text: &str) -> MdResult<()> {
        if let Some(table) = self.table.as_mut() {
            if table.is_in_cell() {
                table.push_text(text);
                return Ok(());
            }
        }

        if let Some(image) = self.pending_image.as_mut() {
            image.alt.push_str(text);
            if let Some(link) = self.pending_link.as_mut() {
                link.text.push_str(text);
            }
            return Ok(());
        }

        if let Some(link) = self.pending_link.as_mut() {
            link.text.push_str(text);
            return Ok(());
        }

        self.ensure_paragraph();
        if let Some(current) = self.current.as_mut() {
            current.push_text(text);
        }

        Ok(())
    }

    fn push_inline_code(&mut self, code: &str) -> MdResult<()> {
        if let Some(table) = self.table.as_mut() {
            if table.is_in_cell() {
                table.push_text(code);
                return Ok(());
            }
        }

        if self.in_code_block {
            return self.push_text(code);
        }

        if let Some(link) = self.pending_link.as_mut() {
            link.text.push_str(code);
            return Ok(());
        }

        self.ensure_paragraph();
        if let Some(current) = self.current.as_mut() {
            current.push_text("`");
            current.push_text(code);
            current.push_text("`");
        }
        Ok(())
    }

    fn push_soft_break(&mut self) -> MdResult<()> {
        if self.in_code_block {
            self.push_text("\n")
        } else {
            self.push_text(" ")
        }
    }

    fn push_hard_break(&mut self) -> MdResult<()> {
        self.push_text("\n")
    }

    fn push_rule(&mut self) {
        self.finalize_paragraph();
        let mut builder = ParagraphBuilder::new(self.mapping.body);
        builder.push_text("---");
        self.paragraphs.push(builder.build());
    }

    fn push_section_marker(&mut self) {
        self.finalize_paragraph();
        let split_at = self.paragraphs.len();
        if split_at > 0 && self.section_breaks.last().copied() != Some(split_at) {
            self.section_breaks.push(split_at);
        }
    }

    fn finalize_table(&mut self) -> MdResult<()> {
        let table_builder = self.table.take().ok_or_else(|| MdError::UnsupportedStructure {
            detail: "table end tag without table start".to_string(),
        })?;

        let table = table_builder.into_table(self.mapping.body, self.mapping.page_settings)?;
        let paragraph = Paragraph::with_runs(
            vec![Run::table(table, self.mapping.body.char_shape_id)],
            self.mapping.body.para_shape_id,
        );
        self.paragraphs.push(paragraph);
        Ok(())
    }

    fn style_for_context(&self) -> MdStyleRef {
        if self.in_code_block {
            return self.mapping.code;
        }
        if self.in_item {
            return self.mapping.list_item;
        }
        if self.blockquote_depth > 0 {
            return self.mapping.blockquote;
        }
        self.mapping.body
    }

    fn ensure_paragraph(&mut self) {
        if self.current.is_none() {
            let mut paragraph = ParagraphBuilder::new(self.style_for_context());
            if let Some(prefix) = self.pending_item_prefix.take() {
                paragraph.push_text(&prefix);
            }
            self.current = Some(paragraph);
        }
    }

    fn start_paragraph(&mut self, style: MdStyleRef) {
        self.finalize_paragraph();
        let mut paragraph = ParagraphBuilder::new(style);
        if let Some(prefix) = self.pending_item_prefix.take() {
            paragraph.push_text(&prefix);
        }
        self.current = Some(paragraph);
    }

    fn finalize_paragraph(&mut self) {
        if let Some(link) = self.pending_link.take() {
            self.ensure_paragraph();
            if let Some(current) = self.current.as_mut() {
                current.push_text(&format!("[{}]({})", link.text, link.dest_url));
            }
        }

        if let Some(image) = self.pending_image.take() {
            self.ensure_paragraph();
            if let Some(current) = self.current.as_mut() {
                current.push_text(&format!("![{}]({})", image.alt, image.dest_url));
            }
        }

        if let Some(current) = self.current.take() {
            self.paragraphs.push(current.build());
        }
    }

    fn next_item_prefix(&mut self) -> String {
        if let Some(last) = self.list_stack.last_mut() {
            if last.ordered {
                let prefix = format!("{}. ", last.next_index);
                last.next_index += 1;
                return prefix;
            }
            return "- ".to_string();
        }
        "- ".to_string()
    }
}

fn level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn empty_paragraph(style: MdStyleRef) -> Paragraph {
    Paragraph::with_runs(vec![Run::text("", style.char_shape_id)], style.para_shape_id)
}

fn split_sections(paragraphs: Vec<Paragraph>, section_breaks: &[usize]) -> Vec<Vec<Paragraph>> {
    if paragraphs.is_empty() {
        return Vec::new();
    }

    if section_breaks.is_empty() {
        return vec![paragraphs];
    }

    let mut sections = Vec::new();
    let mut start = 0usize;

    for &break_idx in section_breaks {
        if break_idx > start && break_idx <= paragraphs.len() {
            sections.push(paragraphs[start..break_idx].to_vec());
            start = break_idx;
        }
    }

    if start < paragraphs.len() {
        sections.push(paragraphs[start..].to_vec());
    }

    sections.into_iter().filter(|section| !section.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::builtin_default;

    fn default_template() -> Template {
        builtin_default().unwrap()
    }

    #[test]
    fn decode_heading_and_body() {
        let template = default_template();
        let mapping = resolve_mapping(&template).unwrap();
        let markdown = "# Hello\n\nBody text";
        let doc = MdDecoder::decode(markdown, &template).unwrap();

        assert_eq!(doc.sections().len(), 1);
        let section = &doc.sections()[0];
        assert_eq!(section.paragraphs.len(), 2);
        assert_eq!(section.paragraphs[0].para_shape_id, mapping.heading1.para_shape_id);
        assert_eq!(section.paragraphs[1].para_shape_id, mapping.body.para_shape_id);
        assert_eq!(section.paragraphs[0].text_content(), "Hello");
    }

    #[test]
    fn decode_frontmatter_into_metadata() {
        let template = default_template();
        let markdown = "---\ntitle: My Proposal\nauthor: Kim\ndate: 2026-02-16\n---\n\nBody";
        let doc = MdDecoder::decode(markdown, &template).unwrap();

        assert_eq!(doc.metadata().title.as_deref(), Some("My Proposal"));
        assert_eq!(doc.metadata().author.as_deref(), Some("Kim"));
        assert_eq!(doc.metadata().created.as_deref(), Some("2026-02-16"));
    }

    #[test]
    fn decode_table_into_table_run() {
        let template = default_template();
        let markdown = "| A | B |\n|---|---|\n| 1 | 2 |";
        let doc = MdDecoder::decode(markdown, &template).unwrap();

        let section = &doc.sections()[0];
        let table_run = section
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .find_map(|run| run.content.as_table())
            .expect("table run");

        assert!(table_run.row_count() >= 1);
        assert_eq!(table_run.col_count(), 2);
    }

    #[test]
    fn decode_link_and_image() {
        let template = default_template();
        let markdown = "[Rust](https://www.rust-lang.org) ![logo](logo.png)";
        let doc = MdDecoder::decode(markdown, &template).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];

        assert!(paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl)
                if matches!(
                    ctrl.as_ref(),
                    Control::Hyperlink { url, .. } if url == "https://www.rust-lang.org"
                )
        )));

        assert!(paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Image(ref img) if img.path == "logo.png"
        )));
    }

    #[test]
    fn decode_linked_image_keeps_hyperlink_text() {
        let template = default_template();
        let markdown = "[![logo](logo.png)](https://example.com)";
        let doc = MdDecoder::decode(markdown, &template).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];

        assert!(paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Image(ref img) if img.path == "logo.png"
        )));

        assert!(paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl)
                if matches!(
                    ctrl.as_ref(),
                    Control::Hyperlink { text, url }
                        if text == "logo" && url == "https://example.com"
                )
        )));
    }

    #[test]
    fn decode_empty_markdown_creates_placeholder_paragraph() {
        let template = default_template();
        let doc = MdDecoder::decode("", &template).unwrap();

        assert_eq!(doc.sections().len(), 1);
        assert_eq!(doc.sections()[0].paragraphs.len(), 1);
        assert_eq!(doc.sections()[0].paragraphs[0].runs.len(), 1);
    }

    #[test]
    fn decode_ordered_list_prefix_increments() {
        let template = default_template();
        let markdown = "1. alpha\n2. beta";
        let doc = MdDecoder::decode(markdown, &template).unwrap();
        let texts: Vec<String> =
            doc.sections()[0].paragraphs.iter().map(Paragraph::text_content).collect();

        assert_eq!(texts, vec!["1. alpha", "2. beta"]);
    }

    #[test]
    fn decode_section_marker_comment_splits_sections() {
        let template = default_template();
        let markdown = "First\n\n<!-- hwpforge:section -->\n\nSecond";
        let doc = MdDecoder::decode(markdown, &template).unwrap();

        assert_eq!(doc.sections().len(), 2);
        assert_eq!(doc.sections()[0].paragraphs[0].text_content(), "First");
        assert_eq!(doc.sections()[1].paragraphs[0].text_content(), "Second");
    }

    #[test]
    fn decode_table_cell_link_stays_in_table_cell_text() {
        let template = default_template();
        let markdown = "| Link |\n|---|\n| [Rust](https://www.rust-lang.org) |";
        let doc = MdDecoder::decode(markdown, &template).unwrap();

        let section = &doc.sections()[0];
        let table_run = section
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .find_map(|run| run.content.as_table())
            .expect("table run");

        let cell_text = table_run.rows[0].cells[0].paragraphs[0].text_content();
        assert!(cell_text.contains("[Rust](https://www.rust-lang.org)"));

        let top_level_control_count = section
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .filter(|run| matches!(run.content, RunContent::Control(_)))
            .count();
        assert_eq!(top_level_control_count, 0);
    }
}
