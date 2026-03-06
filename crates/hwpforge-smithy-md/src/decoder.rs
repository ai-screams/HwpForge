//! Markdown -> Core decoder.

use std::path::Path;

use hwpforge_blueprint::builtins::builtin_default;

/// Maximum markdown file size: 50 MB.
const MAX_MD_FILE_SIZE: u64 = 50 * 1024 * 1024;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::template::Template;
use hwpforge_core::{
    Control, Document, Image, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, StyleIndex};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::error::{MdError, MdResult};
use crate::frontmatter::{apply_to_metadata, extract_frontmatter};
use crate::mapper::{image_format_from_path, resolve_mapping, MdMapping, MdStyleRef};

mod lossless;

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

/// Returns `true` if the URL uses a safe scheme for hyperlinks.
///
/// Rejects `javascript:`, `data:`, `file:`, and similar schemes that can
/// execute code or access local resources when rendered.
fn is_safe_url(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
}

/// Result of decoding markdown, containing both the document and the
/// [`StyleRegistry`] resolved from the template.
///
/// Keeping these together lets callers pass the registry downstream
/// (e.g. to the HWPX encoder) without re-resolving the template.
///
/// # Examples
///
/// ```rust,ignore
/// use hwpforge_blueprint::builtins::builtin_default;
/// use hwpforge_smithy_hwpx::HwpxStyleStore;
/// use hwpforge_smithy_md::MdDecoder;
///
/// let template = builtin_default().unwrap();
/// let result = MdDecoder::decode("# Title\n\nBody text", &template).unwrap();
///
/// // Access the document
/// let doc = result.document.validate().unwrap();
///
/// // Bridge styles to HWPX encoder
/// let store = HwpxStyleStore::from_registry(&result.style_registry);
/// ```
#[derive(Debug)]
pub struct MdDocument {
    /// The decoded Core document.
    pub document: Document,
    /// The style registry resolved from the template.
    pub style_registry: StyleRegistry,
}

/// Markdown decoder.
pub struct MdDecoder;

impl MdDecoder {
    /// Decodes markdown into a Core draft document **and** its style registry.
    ///
    /// The template is used for paragraph/character style index mapping.
    /// Built-in template inheritance (`default`/`gov_proposal`) is resolved
    /// automatically.
    pub fn decode(markdown: &str, template: &Template) -> MdResult<MdDocument> {
        let extracted = extract_frontmatter(markdown)?;
        let (mapping, style_registry) = resolve_mapping(template)?;

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

        Ok(MdDocument { document, style_registry })
    }

    /// Decodes lossless markdown output back into a Core draft document.
    ///
    /// This parses the lossless HTML-like body produced by
    /// [`crate::MdEncoder::encode_lossless`], preserving paragraph/run shape IDs
    /// and control/table structures.
    pub fn decode_lossless(markdown: &str) -> MdResult<Document> {
        let extracted = extract_frontmatter(markdown)?;
        let sections = lossless::decode_lossless_sections(extracted.content)?;

        let mut document = Document::new();
        if let Some(frontmatter) = extracted.frontmatter.as_ref() {
            apply_to_metadata(frontmatter, document.metadata_mut());
        }

        if sections.is_empty() {
            document.add_section(default_empty_section());
        } else {
            for section in sections {
                document.add_section(section);
            }
        }

        Ok(document)
    }

    /// Decodes markdown using the built-in default template.
    ///
    /// This is a convenience wrapper around [`Self::decode`] that uses
    /// [`builtin_default()`](hwpforge_blueprint::builtins::builtin_default)
    /// so callers don't need to construct a template manually.
    pub fn decode_with_default(markdown: &str) -> MdResult<MdDocument> {
        let template = builtin_default()?;
        Self::decode(markdown, &template)
    }

    /// Reads a markdown file and decodes it into a Core draft document with styles.
    ///
    /// Files larger than 50 MB are rejected with [`MdError::FileTooLarge`].
    pub fn decode_file(path: impl AsRef<Path>, template: &Template) -> MdResult<MdDocument> {
        let markdown = read_checked(path.as_ref())?;
        Self::decode(&markdown, template)
    }

    /// Reads a markdown file and decodes it using the built-in default template.
    ///
    /// Files larger than 50 MB are rejected with [`MdError::FileTooLarge`].
    pub fn decode_file_with_default(path: impl AsRef<Path>) -> MdResult<MdDocument> {
        let template = builtin_default()?;
        Self::decode_file(path, &template)
    }

    /// Reads a lossless markdown file and decodes it into a Core draft document.
    ///
    /// Files larger than 50 MB are rejected with [`MdError::FileTooLarge`].
    pub fn decode_lossless_file(path: impl AsRef<Path>) -> MdResult<Document> {
        let markdown = read_checked(path.as_ref())?;
        Self::decode_lossless(&markdown)
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
    heading_level: Option<u8>,
}

impl ParagraphBuilder {
    fn new(style: MdStyleRef) -> Self {
        Self { style, runs: Vec::new(), heading_level: None }
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
        let mut para = Paragraph::with_runs(self.runs, self.style.para_shape_id);
        para.heading_level = self.heading_level;
        if let Some(level) = self.heading_level {
            if (1..=7).contains(&level) {
                // 개요 N is at style index N+1 (바탕글=0, 본문=1, 개요1=2, ...)
                para.style_id = Some(StyleIndex::new((level as usize) + 1));
            }
        }
        para
    }
}

#[derive(Debug, Clone)]
struct TableBuilder {
    rows: Vec<Vec<Vec<Run>>>,
    current_row: Vec<Vec<Run>>,
    current_cell: Vec<Run>,
    row_open: bool,
    cell_open: bool,
}

impl TableBuilder {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: Vec::new(),
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
        self.current_cell = Vec::new();
        self.cell_open = true;
    }

    fn end_cell(&mut self) {
        if self.cell_open {
            self.current_row.push(std::mem::take(&mut self.current_cell));
            self.cell_open = false;
        }
    }

    fn push_text_with_style(&mut self, text: &str, char_shape_id: CharShapeIndex) {
        if !self.cell_open || text.is_empty() {
            return;
        }

        if let Some(last) = self.current_cell.last_mut() {
            if let RunContent::Text(existing) = &mut last.content {
                if last.char_shape_id == char_shape_id {
                    existing.push_str(text);
                    return;
                }
            }
        }

        self.current_cell.push(Run::text(text, char_shape_id));
    }

    fn push_run(&mut self, run: Run) {
        if self.cell_open {
            self.current_cell.push(run);
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
            self.rows.push(vec![vec![Run::text("", body_style.char_shape_id)]]);
        }

        let max_cols = self.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        if max_cols > 10_000 {
            return Err(MdError::UnsupportedStructure {
                detail: format!("table has too many columns: {max_cols}"),
            });
        }
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
                    row.push(vec![Run::text("", body_style.char_shape_id)]);
                }
                while row.len() < max_cols {
                    row.push(vec![Run::text("", body_style.char_shape_id)]);
                }

                let cells = row
                    .into_iter()
                    .map(|runs| {
                        let runs = if runs.is_empty() {
                            vec![Run::text("", body_style.char_shape_id)]
                        } else {
                            runs
                        };
                        let paragraph = Paragraph::with_runs(runs, body_style.para_shape_id);
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
    pending_item_prefixes: Vec<Option<String>>,
    list_stack: Vec<ListState>,
    pending_link: Option<PendingLink>,
    pending_image: Option<PendingImage>,
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
            pending_item_prefixes: Vec::new(),
            list_stack: Vec::new(),
            pending_link: None,
            pending_image: None,
            section_breaks: Vec::new(),
        }
    }

    fn decode_markdown(&mut self, content: &str) -> MdResult<()> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_DEFINITION_LIST);
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
                let raw = html.as_ref().trim();
                if raw == SECTION_MARKER_COMMENT && !self.is_in_table_cell() {
                    self.push_section_marker();
                } else {
                    return Err(unsupported_markdown_feature("raw HTML"));
                }
            }
            Event::FootnoteReference(label) => {
                return Err(unsupported_markdown_feature(&format!(
                    "footnote reference '[^{}]'",
                    label.as_ref()
                )));
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
                let lvl = level_to_u32(level);
                self.start_paragraph(self.mapping.heading(lvl));
                if let Some(current) = self.current.as_mut() {
                    current.heading_level = Some(lvl as u8);
                }
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
                let prefix = self.next_item_prefix();
                self.pending_item_prefixes.push(Some(prefix));
            }
            Tag::Table(_) => {
                self.materialize_pending_item_prefix_if_needed();
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
                if !self.is_in_table_cell() {
                    self.ensure_paragraph();
                }
                // Only create a hyperlink for safe URL schemes. Unsafe URLs
                // (javascript:, data:, file:, etc.) are emitted as plain text
                // by leaving pending_link as None and letting the text pass through.
                if is_safe_url(&dest_url) {
                    self.pending_link =
                        Some(PendingLink { dest_url: dest_url.to_string(), text: String::new() });
                } else {
                    // Store the URL in pending_link with an empty sentinel so
                    // we can still collect the link text, but mark it unsafe
                    // by prefixing with '\0' (never a valid URL character).
                    self.pending_link = Some(PendingLink {
                        dest_url: format!("\x00{}", dest_url),
                        text: String::new(),
                    });
                }
            }
            Tag::Image { dest_url, .. } => {
                if !self.is_in_table_cell() {
                    self.ensure_paragraph();
                }
                self.pending_image =
                    Some(PendingImage { dest_url: dest_url.to_string(), alt: String::new() });
            }
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::HtmlBlock
            | Tag::Superscript
            | Tag::Subscript => {}
            Tag::FootnoteDefinition(_) => {
                return Err(unsupported_markdown_feature("footnote definition"));
            }
            Tag::DefinitionList => {
                return Err(unsupported_markdown_feature("definition list"));
            }
            Tag::DefinitionListTitle => {
                return Err(unsupported_markdown_feature("definition list title"));
            }
            Tag::DefinitionListDefinition => {
                return Err(unsupported_markdown_feature("definition list definition"));
            }
            Tag::MetadataBlock(_) => {
                return Err(unsupported_markdown_feature("metadata block"));
            }
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
                if let Some(Some(prefix)) = self.pending_item_prefixes.pop() {
                    let mut paragraph = ParagraphBuilder::new(self.mapping.list_item);
                    paragraph.push_text(prefix.trim_end());
                    self.paragraphs.push(paragraph.build());
                }
                self.in_item = !self.pending_item_prefixes.is_empty();
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
                if let Some(link) = self.pending_link.take() {
                    let char_shape_id = self.current_char_shape_id();
                    if link.dest_url.starts_with('\x00') {
                        // Unsafe URL: emit the link text as plain text only.
                        if !link.text.is_empty() {
                            self.push_run_to_active_context(Run::text(
                                link.text,
                                char_shape_id,
                            ));
                        }
                    } else {
                        self.push_run_to_active_context(Run::control(
                            Control::Hyperlink { text: link.text, url: link.dest_url },
                            char_shape_id,
                        ));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(image) = self.pending_image.take() {
                    let format = image_format_from_path(&image.dest_url);
                    let image = Image::new(
                        image.dest_url,
                        HwpUnit::from_mm(50.0)?,
                        HwpUnit::from_mm(30.0)?,
                        format,
                    );
                    let char_shape_id = self.current_char_shape_id();
                    self.push_run_to_active_context(Run::image(image, char_shape_id));
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::HtmlBlock
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
            TagEnd::FootnoteDefinition => {
                return Err(unsupported_markdown_feature("footnote definition"));
            }
            TagEnd::DefinitionList => {
                return Err(unsupported_markdown_feature("definition list"));
            }
            TagEnd::DefinitionListTitle => {
                return Err(unsupported_markdown_feature("definition list title"));
            }
            TagEnd::DefinitionListDefinition => {
                return Err(unsupported_markdown_feature("definition list definition"));
            }
            TagEnd::MetadataBlock(_) => {
                return Err(unsupported_markdown_feature("metadata block"));
            }
        }

        Ok(())
    }

    fn push_text(&mut self, text: &str) -> MdResult<()> {
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

        let char_shape_id = self.current_char_shape_id();
        if let Some(table) = self.table.as_mut() {
            if table.is_in_cell() {
                table.push_text_with_style(text, char_shape_id);
                return Ok(());
            }
        }

        self.ensure_paragraph();
        if let Some(current) = self.current.as_mut() {
            current.push_text(text);
        }

        Ok(())
    }

    fn push_inline_code(&mut self, code: &str) -> MdResult<()> {
        let char_shape_id = self.current_char_shape_id();
        if let Some(table) = self.table.as_mut() {
            if table.is_in_cell() {
                table.push_text_with_style("`", char_shape_id);
                table.push_text_with_style(code, char_shape_id);
                table.push_text_with_style("`", char_shape_id);
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

    fn current_char_shape_id(&self) -> CharShapeIndex {
        self.current
            .as_ref()
            .map(|p| p.style.char_shape_id)
            .unwrap_or(self.style_for_context().char_shape_id)
    }

    fn is_in_table_cell(&self) -> bool {
        self.table.as_ref().map(TableBuilder::is_in_cell).unwrap_or(false)
    }

    fn push_run_to_active_context(&mut self, run: Run) {
        if self.is_in_table_cell() {
            if let Some(table) = self.table.as_mut() {
                table.push_run(run);
            }
            return;
        }

        self.ensure_paragraph();
        if let Some(current) = self.current.as_mut() {
            current.push_run(run);
        }
    }

    fn take_pending_item_prefix(&mut self) -> Option<String> {
        self.pending_item_prefixes.last_mut().and_then(Option::take)
    }

    fn materialize_pending_item_prefix_if_needed(&mut self) {
        if self.current.is_some() {
            return;
        }

        if let Some(prefix) = self.take_pending_item_prefix() {
            let mut paragraph = ParagraphBuilder::new(self.mapping.list_item);
            paragraph.push_text(&prefix);
            self.paragraphs.push(paragraph.build());
        }
    }

    fn ensure_paragraph(&mut self) {
        if self.current.is_none() {
            let mut paragraph = ParagraphBuilder::new(self.style_for_context());
            if let Some(prefix) = self.take_pending_item_prefix() {
                paragraph.push_text(&prefix);
            }
            self.current = Some(paragraph);
        }
    }

    fn start_paragraph(&mut self, style: MdStyleRef) {
        self.finalize_paragraph();
        let mut paragraph = ParagraphBuilder::new(style);
        if let Some(prefix) = self.take_pending_item_prefix() {
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

fn default_empty_section() -> Section {
    let paragraph =
        Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    Section::with_paragraphs(vec![paragraph], hwpforge_core::PageSettings::a4())
}

fn unsupported_markdown_feature(feature: &str) -> MdError {
    MdError::UnsupportedStructure { detail: format!("unsupported markdown feature: {feature}") }
}

/// Reads a file after checking that its size does not exceed [`MAX_MD_FILE_SIZE`].
fn read_checked(path: &Path) -> MdResult<String> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    if size > MAX_MD_FILE_SIZE {
        return Err(MdError::FileTooLarge { size, limit: MAX_MD_FILE_SIZE });
    }
    Ok(std::fs::read_to_string(path)?)
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
    use crate::MdEncoder;
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_core::PageSettings;

    fn default_template() -> Template {
        builtin_default().unwrap()
    }

    #[test]
    fn decode_heading_and_body() {
        let template = default_template();
        let (mapping, _) = resolve_mapping(&template).unwrap();
        let markdown = "# Hello\n\nBody text";
        let result = MdDecoder::decode(markdown, &template).unwrap();
        let doc = &result.document;

        assert_eq!(doc.sections().len(), 1);
        let section = &doc.sections()[0];
        assert_eq!(section.paragraphs.len(), 2);
        assert_eq!(section.paragraphs[0].para_shape_id, mapping.heading1.para_shape_id);
        assert_eq!(section.paragraphs[1].para_shape_id, mapping.body.para_shape_id);
        assert_eq!(section.paragraphs[0].text_content(), "Hello");
    }

    #[test]
    fn decode_returns_style_registry() {
        let template = default_template();
        let result = MdDecoder::decode("body text", &template).unwrap();
        assert!(result.style_registry.font_count() > 0);
        assert!(result.style_registry.char_shape_count() > 0);
        assert!(result.style_registry.para_shape_count() > 0);
    }

    #[test]
    fn decode_frontmatter_into_metadata() {
        let template = default_template();
        let markdown = "---\ntitle: My Proposal\nauthor: Kim\ndate: 2026-02-16\n---\n\nBody";
        let result = MdDecoder::decode(markdown, &template).unwrap();

        assert_eq!(result.document.metadata().title.as_deref(), Some("My Proposal"));
        assert_eq!(result.document.metadata().author.as_deref(), Some("Kim"));
        assert_eq!(result.document.metadata().created.as_deref(), Some("2026-02-16"));
    }

    #[test]
    fn decode_table_into_table_run() {
        let template = default_template();
        let markdown = "| A | B |\n|---|---|\n| 1 | 2 |";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;

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
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
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
    fn unsafe_url_emitted_as_plain_text() {
        let template = default_template();
        // javascript: URL must NOT produce a Control::Hyperlink
        let markdown = "[click me](javascript:alert(1))";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
        let paragraph = &doc.sections()[0].paragraphs[0];

        // No hyperlink control should be present
        assert!(!paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl) if matches!(ctrl.as_ref(), Control::Hyperlink { .. })
        )));

        // The link text "click me" should appear as plain text
        assert!(paragraph.runs.iter().any(|run| matches!(
            &run.content,
            RunContent::Text(t) if t == "click me"
        )));
    }

    #[test]
    fn unsafe_data_url_emitted_as_plain_text() {
        let template = default_template();
        let markdown = "[xss](data:text/html,<script>alert(1)</script>)";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
        let paragraph = &doc.sections()[0].paragraphs[0];

        assert!(!paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl) if matches!(ctrl.as_ref(), Control::Hyperlink { .. })
        )));
    }

    #[test]
    fn unsafe_file_url_emitted_as_plain_text() {
        let template = default_template();
        let markdown = "[secret](file:///etc/passwd)";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
        let paragraph = &doc.sections()[0].paragraphs[0];

        // Should NOT produce a Hyperlink control
        assert!(!paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl) if matches!(ctrl.as_ref(), Control::Hyperlink { .. })
        )));
        // Should contain the link text as plain text
        assert!(paragraph.runs.iter().any(|run| matches!(
            &run.content,
            RunContent::Text(t) if t == "secret"
        )));
    }

    #[test]
    fn decode_linked_image_keeps_hyperlink_text() {
        let template = default_template();
        let markdown = "[![logo](logo.png)](https://example.com)";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
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
        let doc = MdDecoder::decode("", &template).unwrap().document;

        assert_eq!(doc.sections().len(), 1);
        assert_eq!(doc.sections()[0].paragraphs.len(), 1);
        assert_eq!(doc.sections()[0].paragraphs[0].runs.len(), 1);
    }

    #[test]
    fn decode_ordered_list_prefix_increments() {
        let template = default_template();
        let markdown = "1. alpha\n2. beta";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
        let texts: Vec<String> =
            doc.sections()[0].paragraphs.iter().map(Paragraph::text_content).collect();

        assert_eq!(texts, vec!["1. alpha", "2. beta"]);
    }

    #[test]
    fn decode_section_marker_comment_splits_sections() {
        let template = default_template();
        let markdown = "First\n\n<!-- hwpforge:section -->\n\nSecond";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;

        assert_eq!(doc.sections().len(), 2);
        assert_eq!(doc.sections()[0].paragraphs[0].text_content(), "First");
        assert_eq!(doc.sections()[1].paragraphs[0].text_content(), "Second");
    }

    #[test]
    fn decode_table_cell_link_preserves_control_run() {
        let template = default_template();
        let markdown = "| Link |\n|---|\n| [Rust](https://www.rust-lang.org) |";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;

        let section = &doc.sections()[0];
        let table_run = section
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .find_map(|run| run.content.as_table())
            .expect("table run");

        let cell_paragraph = &table_run.rows[0].cells[0].paragraphs[0];
        assert!(cell_paragraph.runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl)
                if matches!(
                    ctrl.as_ref(),
                    Control::Hyperlink { text, url }
                        if text == "Rust" && url == "https://www.rust-lang.org"
                )
        )));

        let top_level_control_count = section
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .filter(|run| matches!(run.content, RunContent::Control(_)))
            .count();
        assert_eq!(top_level_control_count, 0);
    }

    #[test]
    fn decode_table_cell_image_preserves_image_run() {
        let template = default_template();
        let markdown = "| Img |\n|---|\n| ![logo](logo.png) |";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;

        let table = doc.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap();
        let cell_runs = &table.rows[0].cells[0].paragraphs[0].runs;
        assert!(cell_runs.iter().any(
            |run| matches!(run.content, RunContent::Image(ref img) if img.path == "logo.png")
        ));
    }

    #[test]
    fn decode_footnote_reference_returns_unsupported_structure_error() {
        let template = default_template();
        let markdown = "Body[^1]\n\n[^1]: note";
        let err = MdDecoder::decode(markdown, &template).unwrap_err();

        assert!(matches!(
            err,
            MdError::UnsupportedStructure { ref detail }
                if detail.contains("footnote reference")
        ));
    }

    #[test]
    fn decode_definition_list_returns_unsupported_structure_error() {
        let template = default_template();
        let markdown = "Term\n: Definition";
        let err = MdDecoder::decode(markdown, &template).unwrap_err();

        assert!(matches!(
            err,
            MdError::UnsupportedStructure { ref detail }
                if detail.contains("definition list")
        ));
    }

    #[test]
    fn decode_raw_html_returns_unsupported_structure_error() {
        let template = default_template();
        let markdown = "<div>raw</div>";
        let err = MdDecoder::decode(markdown, &template).unwrap_err();

        assert!(matches!(
            err,
            MdError::UnsupportedStructure { ref detail }
                if detail.contains("raw HTML")
        ));
    }

    #[test]
    fn decode_lossless_reconstructs_core_structure() {
        let mut draft = Document::new();
        draft.metadata_mut().title = Some("Lossless".to_string());
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("A", CharShapeIndex::new(3)),
                    Run::control(
                        Control::Hyperlink {
                            text: "Rust".to_string(),
                            url: "https://www.rust-lang.org".to_string(),
                        },
                        CharShapeIndex::new(4),
                    ),
                ],
                ParaShapeIndex::new(2),
            )],
            PageSettings::a4(),
        ));

        let validated = draft.validate().unwrap();
        let markdown = MdEncoder::encode_lossless(&validated).unwrap();
        let decoded = MdDecoder::decode_lossless(&markdown).unwrap();

        assert_eq!(decoded.metadata().title.as_deref(), Some("Lossless"));
        assert_eq!(decoded.sections().len(), 1);
        assert_eq!(decoded.sections()[0].paragraphs[0].para_shape_id.get(), 2);
        assert!(decoded.sections()[0].paragraphs[0].runs.iter().any(|run| matches!(
            run.content,
            RunContent::Control(ref ctrl)
                if matches!(
                    ctrl.as_ref(),
                    Control::Hyperlink { text, url }
                        if text == "Rust" && url == "https://www.rust-lang.org"
                )
        )));
    }

    #[test]
    fn decode_nested_list_keeps_outer_prefix_progression() {
        let template = default_template();
        let markdown = "1.\n   - child\n2. next";
        let doc = MdDecoder::decode(markdown, &template).unwrap().document;
        let texts: Vec<String> =
            doc.sections()[0].paragraphs.iter().map(Paragraph::text_content).collect();

        assert!(texts.iter().any(|text| text.starts_with("1.")));
        assert!(texts.iter().any(|text| text.starts_with("2. ")));
    }

    #[test]
    fn decode_lossless_preserves_exact_hwpunit_geometry() {
        let mut page = PageSettings::a4();
        page.width = HwpUnit::new(59_529).unwrap();
        page.height = HwpUnit::new(84_190).unwrap();
        page.margin_left = HwpUnit::new(5_671).unwrap();

        let mut draft = Document::new();
        draft.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("x", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            page,
        ));

        let encoded = MdEncoder::encode_lossless(&draft.validate().unwrap()).unwrap();
        let decoded = MdDecoder::decode_lossless(&encoded).unwrap();
        let restored = decoded.sections()[0].page_settings;

        assert_eq!(restored.width.as_i32(), 59_529);
        assert_eq!(restored.height.as_i32(), 84_190);
        assert_eq!(restored.margin_left.as_i32(), 5_671);
    }

    #[test]
    fn decode_with_default_uses_builtin_template() {
        let result = MdDecoder::decode_with_default("# 제목\n\n본문입니다.").unwrap();
        assert!(!result.document.sections().is_empty());
        assert!(result.style_registry.font_count() > 0);
    }

    #[test]
    fn decode_file_with_default_reads_and_decodes() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("simple_body.md");
        let result = MdDecoder::decode_file_with_default(path).unwrap();
        assert_eq!(result.document.metadata().title.as_deref(), Some("Simple Body Test"));
    }

    #[test]
    fn h1_heading_sets_style_id_to_2() {
        use hwpforge_foundation::StyleIndex;
        let template = default_template();
        let result = MdDecoder::decode("# 제목", &template).unwrap();
        let section = &result.document.sections()[0];
        assert_eq!(section.paragraphs[0].style_id, Some(StyleIndex::new(2)));
    }

    #[test]
    fn all_heading_levels_map_to_style_id() {
        use hwpforge_foundation::StyleIndex;
        let template = default_template();
        for level in 1u8..=6 {
            let md = format!("{} 제목{level}", "#".repeat(level as usize));
            let result = MdDecoder::decode(&md, &template).unwrap();
            let section = &result.document.sections()[0];
            assert_eq!(
                section.paragraphs[0].style_id,
                Some(StyleIndex::new((level as usize) + 1)),
                "H{level} should map to style_id {}",
                (level as usize) + 1
            );
        }
    }

    #[test]
    fn body_paragraph_has_no_style_id() {
        let template = default_template();
        let result = MdDecoder::decode("본문입니다.", &template).unwrap();
        let section = &result.document.sections()[0];
        assert_eq!(section.paragraphs[0].style_id, None);
    }
}
