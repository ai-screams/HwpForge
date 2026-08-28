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
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, StrikeoutShape, StyleIndex};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicase::UniCase;

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
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

fn encode_pending_link_url(url: &str) -> String {
    if is_safe_url(url) {
        url.to_string()
    } else {
        format!("\x00{url}")
    }
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
/// use hwpforge_smithy_hwpx::HwpxRegistryBridge;
/// use hwpforge_smithy_md::MdDecoder;
///
/// let template = builtin_default().unwrap();
/// let result = MdDecoder::decode("# Title\n\nBody text", &template).unwrap();
///
/// // Access the document
/// let bridge = HwpxRegistryBridge::from_registry(&result.style_registry).unwrap();
/// let rebound = bridge.rebind_draft_document(result.document).unwrap();
/// let doc = rebound.validate().unwrap();
///
/// // Use the bridge-built store for HWPX encode
/// let store = bridge.style_store();
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
        let (mapping, mut style_registry) = resolve_mapping(template)?;

        let mut document = Document::new();
        if let Some(frontmatter) = extracted.frontmatter.as_ref() {
            apply_to_metadata(frontmatter, document.metadata_mut());
        }

        let mut state = DecoderState::new(&mapping, &mut style_registry);
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
struct PendingItem {
    prefix: String,
    prefix_pending: bool,
    task_checked: Option<bool>,
    emitted_paragraph: bool,
}

impl PendingItem {
    fn new(prefix: String) -> Self {
        Self { prefix, prefix_pending: true, task_checked: None, emitted_paragraph: false }
    }

    fn mark_task(&mut self, checked: bool) {
        self.task_checked = Some(checked);
        self.prefix_pending = false;
    }

    fn take_prefix(&mut self) -> Option<String> {
        if self.prefix_pending && self.task_checked.is_none() {
            self.prefix_pending = false;
            return Some(self.prefix.clone());
        }
        None
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
        self.push_text_with_style(text, self.style.char_shape_id);
    }

    /// Appends text with an explicit character shape (inline-format runs).
    /// Adjacent text with the same shape merges into one run.
    fn push_text_with_style(&mut self, text: &str, char_shape_id: CharShapeIndex) {
        if text.is_empty() {
            return;
        }

        if let Some(last) = self.runs.last_mut() {
            if let RunContent::Text(existing) = &mut last.content {
                if last.char_shape_id == char_shape_id {
                    existing.push_str(text);
                    return;
                }
            }
        }

        self.runs.push(Run::text(text, char_shape_id));
    }

    fn push_run(&mut self, run: Run) {
        self.runs.push(run);
    }

    fn set_style(&mut self, style: MdStyleRef) {
        self.style = style;
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

                TableRow::new(cells)
            })
            .collect();

        Ok(Table::new(table_rows))
    }
}

/// 전개된 각주/미주 본문의 총량 상한 (H5 — 다중 참조 복제 증폭 방어).
const MAX_EXPANDED_NOTE_BYTES: usize = 8 * 1024 * 1024;

/// 각주/미주 정의의 해석 상태 (H4 — 소비하지 않고 참조 수만 센다).
#[derive(Debug)]
struct NoteDefinition {
    /// 원문 라벨 (에러 메시지용 — canonical 은 맵 키).
    label: String,
    /// 본문 문단들 (다문단 지원).
    paragraphs: Vec<Paragraph>,
    /// resolve 에서 이 정의를 참조한 횟수. 0 이면 고아 (에러).
    reference_count: u32,
}

/// 정의 수집 모드 — 본문 빌더와 **격리**된 캡처 (Codex H1).
///
/// 전역 리스트/인용/표 상태를 읽지도 쓰지도 않는다. 화이트리스트:
/// 문단 · 텍스트 · 인라인 서식(W0 스택 재사용) · soft/hard break ·
/// 인라인 코드(리터럴). 그 외 블록/링크/이미지는 typed 거부.
#[derive(Debug)]
struct DefinitionCapture {
    /// 원문 라벨.
    label: String,
    /// 완성된 본문 문단들.
    paragraphs: Vec<Paragraph>,
    /// 조립 중인 문단.
    current: Option<ParagraphBuilder>,
    /// 본문 문단 스타일 (builtin "각주"/"미주" — 없으면 body 폴백).
    style: MdStyleRef,
}

#[derive(Debug)]
struct DecoderState<'a> {
    mapping: &'a MdMapping,
    /// Style registry the decoder may extend with derived inline-format
    /// character shapes (W0 — bold/italic/strikethrough runs).
    registry: &'a mut StyleRegistry,
    /// Inline-format nesting counters (pulldown-cmark guarantees balanced
    /// start/end tags; counters keep same-tag nesting safe).
    fmt_bold: u32,
    fmt_italic: u32,
    fmt_strike: u32,
    /// Cache of derived char shapes: (base shape index, format flags) → index.
    derived_shapes: std::collections::HashMap<(usize, u8), CharShapeIndex>,
    /// 각주/미주 정의 — 키는 파서(UniCase)와 동일한 case-fold canonical (H3).
    note_definitions: std::collections::HashMap<UniCase<String>, NoteDefinition>,
    /// 참조를 만난 순서 (resolve 가 이 순서로 빈 컨트롤을 채운다 — D1).
    pending_notes: Vec<UniCase<String>>,
    /// 정의 수집 모드 (Some 이면 활성).
    definition_capture: Option<DefinitionCapture>,
    /// builtin "각주" 스타일 (없으면 body 폴백 — D6).
    footnote_style: MdStyleRef,
    /// builtin "미주" 스타일 (없으면 body 폴백 — D6).
    endnote_style: MdStyleRef,
    paragraphs: Vec<Paragraph>,
    current: Option<ParagraphBuilder>,
    table: Option<TableBuilder>,
    blockquote_depth: usize,
    in_code_block: bool,
    in_item: bool,
    pending_items: Vec<PendingItem>,
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
    fn new(mapping: &'a MdMapping, registry: &'a mut StyleRegistry) -> Self {
        let style_of = |name: &str| {
            registry
                .get_style(name)
                .map(|e| MdStyleRef {
                    para_shape_id: e.para_shape_id,
                    char_shape_id: e.char_shape_id,
                })
                .unwrap_or(mapping.body)
        };
        let footnote_style = style_of(crate::internal_styles::FOOTNOTE_STYLE_NAME);
        let endnote_style = style_of(crate::internal_styles::ENDNOTE_STYLE_NAME);
        Self {
            mapping,
            registry,
            note_definitions: std::collections::HashMap::new(),
            pending_notes: Vec::new(),
            definition_capture: None,
            footnote_style,
            endnote_style,
            fmt_bold: 0,
            fmt_italic: 0,
            fmt_strike: 0,
            derived_shapes: std::collections::HashMap::new(),
            paragraphs: Vec::new(),
            current: None,
            table: None,
            blockquote_depth: 0,
            in_code_block: false,
            in_item: false,
            pending_items: Vec::new(),
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
        self.resolve_notes()?;
        Ok(DecodeOutput { paragraphs: self.paragraphs, section_breaks: self.section_breaks })
    }

    /// A안 resolve — 빈 각주/미주 컨트롤을 참조 순서대로 정의 본문으로 채운다.
    ///
    /// 순회는 **소스 순서 보장 전용 in-order run walk** 다 (D1): run 을 순서대로
    /// 걷고 `Table` run 을 만나면 그 자리에서 셀 문단을 재귀한다. 기존
    /// `walk_paragraphs_mut` 는 문단을 먼저 방문(`f(self)` 후 run 재귀)해
    /// `[표, 컨트롤]` 순서 문단에서 소스 순서와 어긋날 수 있어 쓰지 않는다.
    /// 이 보장의 계약 범위는 MD 디코더가 생성 가능한 형상(본문 문단 +
    /// 비병합 GFM 표 셀)뿐이다 (M1).
    fn resolve_notes(&mut self) -> MdResult<()> {
        let mut queue = std::mem::take(&mut self.pending_notes).into_iter();
        let mut expanded_bytes = 0usize;

        fn walk_paragraphs(
            paragraphs: &mut [Paragraph],
            queue: &mut std::vec::IntoIter<UniCase<String>>,
            definitions: &mut std::collections::HashMap<UniCase<String>, NoteDefinition>,
            expanded_bytes: &mut usize,
        ) -> MdResult<()> {
            for paragraph in paragraphs {
                for run in &mut paragraph.runs {
                    match &mut run.content {
                        RunContent::Control(control) => {
                            let is_empty_note = matches!(
                                control.as_ref(),
                                Control::Footnote { paragraphs, .. }
                                | Control::Endnote { paragraphs, .. }
                                    if paragraphs.is_empty()
                            );
                            if !is_empty_note {
                                continue;
                            }
                            let Some(canonical) = queue.next() else {
                                return Err(MdError::UnsupportedStructure {
                                    detail: "note resolve invariant broken: more empty note \
                                             controls than pending references"
                                        .to_string(),
                                });
                            };
                            let Some(def) = definitions.get_mut(&canonical) else {
                                // P2: 정의 없는 참조는 파서가 이벤트를 만들지 않으므로
                                // 여기 도달 = 내부 불변식 위반.
                                return Err(MdError::UnsupportedStructure {
                                    detail: format!(
                                        "note resolve invariant broken: no definition for \
                                         referenced label '{}'",
                                        canonical.as_ref()
                                    ),
                                });
                            };
                            def.reference_count += 1;
                            *expanded_bytes += def
                                .paragraphs
                                .iter()
                                .map(|p| p.text_content().len())
                                .sum::<usize>();
                            if *expanded_bytes > MAX_EXPANDED_NOTE_BYTES {
                                return Err(MdError::NoteExpansionBudgetExceeded {
                                    budget: MAX_EXPANDED_NOTE_BYTES,
                                });
                            }
                            match control.as_mut() {
                                Control::Footnote { paragraphs, .. }
                                | Control::Endnote { paragraphs, .. } => {
                                    *paragraphs = def.paragraphs.clone();
                                }
                                _ => unreachable!("guarded by is_empty_note"),
                            }
                        }
                        RunContent::Table(table) => {
                            for row in &mut table.rows {
                                for cell in &mut row.cells {
                                    walk_paragraphs(
                                        &mut cell.paragraphs,
                                        queue,
                                        definitions,
                                        expanded_bytes,
                                    )?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }

        walk_paragraphs(
            &mut self.paragraphs,
            &mut queue,
            &mut self.note_definitions,
            &mut expanded_bytes,
        )?;

        if queue.next().is_some() {
            return Err(MdError::UnsupportedStructure {
                detail: "note resolve invariant broken: pending references left unfilled"
                    .to_string(),
            });
        }
        if let Some(orphan) = self.note_definitions.values().find(|def| def.reference_count == 0) {
            return Err(MdError::OrphanNoteDefinition { label: orphan.label.clone() });
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event<'_>) -> MdResult<()> {
        if self.definition_capture.is_some() {
            return self.handle_definition_event(event);
        }
        match event {
            Event::Start(tag) => self.start_tag(tag)?,
            Event::End(tag_end) => self.end_tag(tag_end)?,
            Event::Text(text) => self.push_text(text.as_ref())?,
            Event::Code(code) => self.push_inline_code(code.as_ref())?,
            Event::InlineMath(math) | Event::DisplayMath(math) => self.push_text(math.as_ref())?,
            Event::Html(html) | Event::InlineHtml(html) => self.handle_html_event(html.as_ref())?,
            Event::FootnoteReference(label) => self.handle_footnote_reference(label.as_ref())?,
            Event::SoftBreak => self.push_soft_break()?,
            Event::HardBreak => self.push_hard_break()?,
            Event::Rule => self.push_rule(),
            Event::TaskListMarker(checked) => self.mark_task_list_item(checked),
        }

        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>) -> MdResult<()> {
        match tag {
            Tag::Paragraph => self.start_paragraph_tag(),
            Tag::Heading { level, .. } => self.start_heading_tag(level),
            Tag::BlockQuote(_) => self.start_blockquote_tag(),
            Tag::CodeBlock(_) => self.start_code_block_tag(),
            Tag::List(start) => self.start_list_tag(start),
            Tag::Item => self.start_item_tag(),
            Tag::Table(_) => self.start_table_tag(),
            // pulldown-cmark emits the GFM header row's cells inside `TableHead`
            // (not `TableRow`). Treat it as a row so the header cells are
            // captured as table row 0 (Core/HWPX render row 0 as the header).
            Tag::TableHead => self.start_table_row_tag(),
            Tag::TableRow => self.start_table_row_tag(),
            Tag::TableCell => self.start_table_cell_tag(),
            Tag::Link { dest_url, .. } => self.start_link_tag(&dest_url),
            Tag::Image { dest_url, .. } => self.start_image_tag(&dest_url),
            Tag::Strong => self.fmt_bold += 1,
            Tag::Emphasis => self.fmt_italic += 1,
            Tag::Strikethrough => self.fmt_strike += 1,
            Tag::HtmlBlock | Tag::Superscript | Tag::Subscript => {}
            Tag::FootnoteDefinition(label) => self.start_footnote_definition(label.as_ref())?,
            Tag::DefinitionList => return Err(unsupported_markdown_feature("definition list")),
            Tag::DefinitionListTitle => {
                return Err(unsupported_markdown_feature("definition list title"));
            }
            Tag::DefinitionListDefinition => {
                return Err(unsupported_markdown_feature("definition list definition"));
            }
            Tag::MetadataBlock(_) => return Err(unsupported_markdown_feature("metadata block")),
        }
        Ok(())
    }

    fn end_tag(&mut self, tag_end: TagEnd) -> MdResult<()> {
        match tag_end {
            TagEnd::Paragraph => self.finalize_paragraph(),
            TagEnd::Heading(_) => self.finalize_paragraph(),
            TagEnd::BlockQuote(_) => self.end_blockquote_tag(),
            TagEnd::CodeBlock => self.end_code_block_tag(),
            TagEnd::List(_) => self.end_list_tag(),
            TagEnd::Item => self.end_item_tag(),
            TagEnd::Table => self.finalize_table()?,
            TagEnd::TableHead => self.end_table_row_tag(),
            TagEnd::TableRow => self.end_table_row_tag(),
            TagEnd::TableCell => self.end_table_cell_tag(),
            TagEnd::Link => self.end_link_tag(),
            TagEnd::Image => self.end_image_tag()?,
            TagEnd::Strong => self.fmt_bold = self.fmt_bold.saturating_sub(1),
            TagEnd::Emphasis => self.fmt_italic = self.fmt_italic.saturating_sub(1),
            TagEnd::Strikethrough => self.fmt_strike = self.fmt_strike.saturating_sub(1),
            TagEnd::HtmlBlock | TagEnd::Superscript | TagEnd::Subscript => {}
            TagEnd::FootnoteDefinition => {
                // capture 활성 중에는 handle_definition_event 가 소화한다.
                // 여기 도달 = 시작 없이 끝 — 파서 계약 위반 (방어).
                return Err(unsupported_markdown_feature("unbalanced footnote definition end"));
            }
            TagEnd::DefinitionList => return Err(unsupported_markdown_feature("definition list")),
            TagEnd::DefinitionListTitle => {
                return Err(unsupported_markdown_feature("definition list title"));
            }
            TagEnd::DefinitionListDefinition => {
                return Err(unsupported_markdown_feature("definition list definition"));
            }
            TagEnd::MetadataBlock(_) => return Err(unsupported_markdown_feature("metadata block")),
        }

        Ok(())
    }

    fn handle_html_event(&mut self, html: &str) -> MdResult<()> {
        let raw = html.trim();
        if raw == SECTION_MARKER_COMMENT && !self.is_in_table_cell() {
            self.push_section_marker();
            return Ok(());
        }
        Err(unsupported_markdown_feature("raw HTML"))
    }

    /// 각주/미주 참조 — 빈 컨트롤을 참조 지점에 꽂고 라벨을 순서 큐에 기록한다
    /// (A안 — 본문은 문서 빌드 완료 후 resolve 단계에서 채운다).
    fn handle_footnote_reference(&mut self, label: &str) -> MdResult<()> {
        let canonical = UniCase::new(label.to_string());
        let control = if is_endnote_label(&canonical) {
            Control::endnote(Vec::new())
        } else {
            Control::footnote(Vec::new())
        };
        self.pending_notes.push(canonical);
        let char_shape_id = self.current_char_shape_id();
        self.push_run_to_active_context(Run {
            content: RunContent::Control(Box::new(control)),
            char_shape_id,
        });
        Ok(())
    }

    /// 정의 수집 모드 진입 (`[^label]:` 블록 시작).
    fn start_footnote_definition(&mut self, label: &str) -> MdResult<()> {
        let canonical = UniCase::new(label.to_string());
        if self.note_definitions.contains_key(&canonical) {
            return Err(MdError::DuplicateNoteDefinition { label: label.to_string() });
        }
        let style =
            if is_endnote_label(&canonical) { self.endnote_style } else { self.footnote_style };
        self.definition_capture = Some(DefinitionCapture {
            label: label.to_string(),
            paragraphs: Vec::new(),
            current: None,
            style,
        });
        Ok(())
    }

    /// 정의 수집 모드의 이벤트 처리 — 본문 빌더와 격리 (H1).
    fn handle_definition_event(&mut self, event: Event<'_>) -> MdResult<()> {
        match event {
            Event::Start(Tag::Paragraph) => {
                let capture = self.definition_capture.as_mut().expect("capture active");
                let style = capture.style;
                capture.current = Some(ParagraphBuilder::new(style));
                Ok(())
            }
            Event::End(TagEnd::Paragraph) => {
                let capture = self.definition_capture.as_mut().expect("capture active");
                if let Some(builder) = capture.current.take() {
                    capture.paragraphs.push(builder.build());
                }
                Ok(())
            }
            Event::End(TagEnd::FootnoteDefinition) => self.finish_footnote_definition(),
            Event::Text(text) => self.push_definition_text(text.as_ref()),
            Event::Code(code) => {
                self.push_definition_text("`")?;
                self.push_definition_text(code.as_ref())?;
                self.push_definition_text("`")
            }
            Event::SoftBreak => self.push_definition_text(" "),
            Event::HardBreak => self.push_definition_text("\n"),
            Event::Start(Tag::Strong) => {
                self.fmt_bold += 1;
                Ok(())
            }
            Event::End(TagEnd::Strong) => {
                self.fmt_bold = self.fmt_bold.saturating_sub(1);
                Ok(())
            }
            Event::Start(Tag::Emphasis) => {
                self.fmt_italic += 1;
                Ok(())
            }
            Event::End(TagEnd::Emphasis) => {
                self.fmt_italic = self.fmt_italic.saturating_sub(1);
                Ok(())
            }
            Event::Start(Tag::Strikethrough) => {
                self.fmt_strike += 1;
                Ok(())
            }
            Event::End(TagEnd::Strikethrough) => {
                self.fmt_strike = self.fmt_strike.saturating_sub(1);
                Ok(())
            }
            Event::FootnoteReference(label) => {
                Err(MdError::NestedNoteReference { label: label.to_string() })
            }
            other => {
                Err(unsupported_markdown_feature(&format!("{other:?} in footnote definition")))
            }
        }
    }

    /// 정의 본문 텍스트 — 각주 스타일 base 에서 W0 서식 파생을 적용한다.
    fn push_definition_text(&mut self, text: &str) -> MdResult<()> {
        let base = self.definition_capture.as_ref().expect("capture active").style.char_shape_id;
        let char_shape_id = self.derived_char_shape(base);
        let capture = self.definition_capture.as_mut().expect("capture active");
        let style = capture.style;
        let builder = capture.current.get_or_insert_with(|| ParagraphBuilder::new(style));
        builder.push_text_with_style(text, char_shape_id);
        Ok(())
    }

    /// 정의 수집 종료 — 빈 정의 거부 후 등록.
    fn finish_footnote_definition(&mut self) -> MdResult<()> {
        let mut capture = self.definition_capture.take().expect("capture active");
        if let Some(builder) = capture.current.take() {
            capture.paragraphs.push(builder.build());
        }
        let has_content = capture.paragraphs.iter().any(|p| !p.text_content().trim().is_empty());
        if !has_content {
            return Err(MdError::EmptyNoteDefinition { label: capture.label });
        }
        let canonical = UniCase::new(capture.label.clone());
        self.note_definitions.insert(
            canonical,
            NoteDefinition {
                label: capture.label,
                paragraphs: capture.paragraphs,
                reference_count: 0,
            },
        );
        Ok(())
    }

    fn start_paragraph_tag(&mut self) {
        self.ensure_paragraph();
    }

    fn start_heading_tag(&mut self, level: HeadingLevel) {
        let lvl = level_to_u32(level);
        self.start_paragraph(self.mapping.heading(lvl));
        if let Some(current) = self.current.as_mut() {
            current.heading_level = Some(lvl as u8);
        }
    }

    fn start_blockquote_tag(&mut self) {
        self.blockquote_depth += 1;
    }

    fn start_code_block_tag(&mut self) {
        self.in_code_block = true;
        self.start_paragraph(self.mapping.code);
    }

    fn start_list_tag(&mut self, start: Option<u64>) {
        self.list_stack.push(ListState::new(start));
    }

    fn start_item_tag(&mut self) {
        self.finalize_paragraph();
        self.in_item = true;
        let prefix = self.next_item_prefix();
        self.pending_items.push(PendingItem::new(prefix));
    }

    fn start_table_tag(&mut self) {
        self.materialize_pending_item_paragraph_if_needed();
        self.finalize_paragraph();
        self.table = Some(TableBuilder::new());
    }

    fn start_table_row_tag(&mut self) {
        if let Some(table) = self.table.as_mut() {
            table.start_row();
        }
    }

    fn start_table_cell_tag(&mut self) {
        if let Some(table) = self.table.as_mut() {
            table.start_cell();
        }
    }

    fn start_link_tag(&mut self, dest_url: &str) {
        if !self.is_in_table_cell() {
            self.ensure_paragraph();
        }
        self.pending_link =
            Some(PendingLink { dest_url: encode_pending_link_url(dest_url), text: String::new() });
    }

    fn start_image_tag(&mut self, dest_url: &str) {
        if !self.is_in_table_cell() {
            self.ensure_paragraph();
        }
        self.pending_image =
            Some(PendingImage { dest_url: dest_url.to_string(), alt: String::new() });
    }

    fn end_blockquote_tag(&mut self) {
        self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
    }

    fn end_code_block_tag(&mut self) {
        self.in_code_block = false;
        self.finalize_paragraph();
    }

    fn end_list_tag(&mut self) {
        self.list_stack.pop();
    }

    fn end_item_tag(&mut self) {
        self.finalize_paragraph();
        if let Some(item) = self.pending_items.pop() {
            if !item.emitted_paragraph {
                let mut paragraph = ParagraphBuilder::new(self.item_style_for(&item));
                if item.task_checked.is_none() {
                    paragraph.push_text(item.prefix.trim_end());
                }
                self.paragraphs.push(paragraph.build());
            }
        }
        self.in_item = !self.pending_items.is_empty();
    }

    fn end_table_row_tag(&mut self) {
        if let Some(table) = self.table.as_mut() {
            table.end_row();
        }
    }

    fn end_table_cell_tag(&mut self) {
        if let Some(table) = self.table.as_mut() {
            table.end_cell();
        }
    }

    fn end_link_tag(&mut self) {
        if let Some(link) = self.pending_link.take() {
            let char_shape_id = self.current_char_shape_id();
            if link.dest_url.starts_with('\x00') {
                if !link.text.is_empty() {
                    self.push_run_to_active_context(Run::text(link.text, char_shape_id));
                }
            } else {
                self.push_run_to_active_context(Run::control(
                    Control::Hyperlink { text: link.text, url: link.dest_url },
                    char_shape_id,
                ));
            }
        }
    }

    fn end_image_tag(&mut self) -> MdResult<()> {
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

        self.with_materialized_paragraph(|current| {
            current.push_text_with_style(text, char_shape_id);
        });

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

        self.with_materialized_paragraph(|current| {
            current.push_text_with_style("`", char_shape_id);
            current.push_text_with_style(code, char_shape_id);
            current.push_text_with_style("`", char_shape_id);
        });
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
            if let Some(item) = self.pending_items.last() {
                return self.item_style_for(item);
            }
            return self.mapping.list_item;
        }
        if self.blockquote_depth > 0 {
            return self.mapping.blockquote;
        }
        self.mapping.body
    }

    fn current_char_shape_id(&mut self) -> CharShapeIndex {
        let base = self
            .current
            .as_ref()
            .map(|p| p.style.char_shape_id)
            .unwrap_or(self.style_for_context().char_shape_id);
        self.derived_char_shape(base)
    }

    /// Bitset of active inline formats: bit0 bold, bit1 italic, bit2 strike.
    fn format_flags(&self) -> u8 {
        u8::from(self.fmt_bold > 0)
            | (u8::from(self.fmt_italic > 0) << 1)
            | (u8::from(self.fmt_strike > 0) << 2)
    }

    /// Returns `base` when no inline format is active; otherwise returns a
    /// char shape derived from `base` with the active formats applied,
    /// registering it once per (base, flags) combination.
    fn derived_char_shape(&mut self, base: CharShapeIndex) -> CharShapeIndex {
        let flags = self.format_flags();
        if flags == 0 {
            return base;
        }
        let key = (base.get(), flags);
        if let Some(&cached) = self.derived_shapes.get(&key) {
            return cached;
        }
        let Some(base_shape) = self.registry.char_shape(base) else {
            // Unknown base (defensive): keep the base index rather than
            // fabricating a shape from nothing.
            return base;
        };
        let mut derived = base_shape.clone();
        if flags & 1 != 0 {
            derived.bold = true;
        }
        if flags & 2 != 0 {
            derived.italic = true;
        }
        if flags & 4 != 0 && derived.strikeout_shape == StrikeoutShape::None {
            derived.strikeout_shape = StrikeoutShape::Solid;
        }
        let idx = CharShapeIndex::new(self.registry.char_shapes.len());
        self.registry.char_shapes.push(derived);
        self.derived_shapes.insert(key, idx);
        idx
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

        self.with_materialized_paragraph(|current| {
            current.push_run(run);
        });
    }

    fn ensure_paragraph(&mut self) {
        if self.current.is_none() {
            let style = if self.in_item {
                self.start_next_item_paragraph()
            } else {
                self.style_for_context()
            };
            self.current = Some(ParagraphBuilder::new(style));
        }
    }

    fn with_materialized_paragraph(&mut self, apply: impl FnOnce(&mut ParagraphBuilder)) {
        self.ensure_paragraph();
        self.materialize_current_item_prefix_if_needed();
        if let Some(current) = self.current.as_mut() {
            apply(current);
        }
    }

    fn start_paragraph(&mut self, style: MdStyleRef) {
        self.finalize_paragraph();
        if self.in_item {
            if let Some(item) = self.pending_items.last_mut() {
                item.emitted_paragraph = true;
            }
        }
        self.current = Some(ParagraphBuilder::new(style));
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

    fn current_item_level(&self) -> u8 {
        u8::try_from(self.list_stack.len().saturating_sub(1)).unwrap_or(u8::MAX)
    }

    fn item_style_for(&self, item: &PendingItem) -> MdStyleRef {
        self.item_style_for_state(item.task_checked, item.emitted_paragraph)
    }

    fn item_style_for_state(&self, task_checked: Option<bool>, continuation: bool) -> MdStyleRef {
        if continuation {
            return self.mapping.list_continuation(self.current_item_level());
        }

        task_checked
            .map(|checked| self.mapping.task_list(checked, self.current_item_level()))
            .unwrap_or(self.mapping.list_item)
    }

    fn start_next_item_paragraph(&mut self) -> MdStyleRef {
        let continuation =
            self.pending_items.last().map(|item| item.emitted_paragraph).unwrap_or(false);
        let task_checked = self.pending_items.last().and_then(|item| item.task_checked);
        if let Some(item) = self.pending_items.last_mut() {
            item.emitted_paragraph = true;
        }
        self.item_style_for_state(task_checked, continuation)
    }

    fn materialize_current_item_prefix_if_needed(&mut self) {
        if !self.in_item {
            return;
        }

        let prefix = self.pending_items.last_mut().and_then(PendingItem::take_prefix);
        if let (Some(prefix), Some(current)) = (prefix, self.current.as_mut()) {
            if current.runs.is_empty() {
                current.push_text(&prefix);
            }
        }
    }

    fn materialize_pending_item_paragraph_if_needed(&mut self) {
        if !self.in_item || self.current.is_some() {
            return;
        }

        if self.pending_items.last().is_some_and(|item| item.emitted_paragraph) {
            return;
        }

        let style = self.start_next_item_paragraph();
        let prefix = self.pending_items.last_mut().and_then(PendingItem::take_prefix);

        let mut paragraph = ParagraphBuilder::new(style);
        if let Some(prefix) = prefix {
            paragraph.push_text(&prefix);
        }
        self.paragraphs.push(paragraph.build());
    }

    fn mark_task_list_item(&mut self, checked: bool) {
        if let Some(item) = self.pending_items.last_mut() {
            item.mark_task(checked);
        }
        let task_style = self.mapping.task_list(checked, self.current_item_level());
        if let Some(current) = self.current.as_mut() {
            current.set_style(task_style);
        }
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

/// 미주 라벨 판별 — canonical(case-fold) 기준 `e` + 숫자 1개 이상 (D3).
///
/// `e[0-9]+` 는 HwpForge dialect 의 **미주 예약 이름공간**이다 (H2):
/// `to-md` 가 미주를 `[^eN]` 으로 방출하는 규약의 역방향. 사용자가 각주
/// 의도로 `[^e1]` 을 쓰면 미주로 정규화된다 (lossy — CHANGELOG 명시).
fn is_endnote_label(canonical: &UniCase<String>) -> bool {
    // UniCase 비교 기준과 동일하게 fold 된 소문자 형태로 검사한다.
    let folded = canonical.as_ref().to_lowercase();
    let Some(rest) = folded.strip_prefix('e') else { return false };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
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
    fn decode_task_list_restores_checkable_list_semantics() {
        use hwpforge_core::ParagraphListRef;

        let template = default_template();
        let result = MdDecoder::decode("- [ ] todo\n- [x] done", &template).unwrap();
        let paragraphs = &result.document.sections()[0].paragraphs;

        assert_eq!(paragraphs[0].text_content(), "todo");
        assert_eq!(paragraphs[1].text_content(), "done");

        let unchecked_shape = result
            .style_registry
            .para_shape(paragraphs[0].para_shape_id)
            .expect("unchecked para shape");
        let checked_shape = result
            .style_registry
            .para_shape(paragraphs[1].para_shape_id)
            .expect("checked para shape");

        let unchecked_bullet = match unchecked_shape.list {
            Some(ParagraphListRef::CheckBullet { bullet_id, level: 0, checked: false }) => {
                bullet_id
            }
            other => panic!("expected unchecked task list semantics, got {other:?}"),
        };
        assert!(matches!(
            checked_shape.list,
            Some(ParagraphListRef::CheckBullet {
                bullet_id,
                level: 0,
                checked: true,
            }) if bullet_id == unchecked_bullet
        ));
    }

    #[test]
    fn decode_nested_task_list_restores_nested_checkable_levels() {
        use hwpforge_core::ParagraphListRef;

        let template = default_template();
        let result = MdDecoder::decode("- [ ] parent\n  - [x] child", &template).unwrap();
        let paragraphs = &result.document.sections()[0].paragraphs;

        let parent_shape = result
            .style_registry
            .para_shape(paragraphs[0].para_shape_id)
            .expect("parent para shape");
        let child_shape = result
            .style_registry
            .para_shape(paragraphs[1].para_shape_id)
            .expect("child para shape");

        assert!(matches!(
            parent_shape.list,
            Some(ParagraphListRef::CheckBullet { level: 0, checked: false, .. })
        ));
        assert!(matches!(
            child_shape.list,
            Some(ParagraphListRef::CheckBullet { level: 1, checked: true, .. })
        ));
    }

    #[test]
    fn decode_multi_paragraph_task_item_uses_continuation_shape() {
        use hwpforge_core::ParagraphListRef;
        use hwpforge_foundation::HwpUnit;

        let template = default_template();
        let markdown = "- [ ] first paragraph of the same task item\n\n  second paragraph of the same task item\n\n- [x] next real task item";
        let result = MdDecoder::decode(markdown, &template).unwrap();
        let paragraphs = &result.document.sections()[0].paragraphs;

        assert_eq!(paragraphs.len(), 3);
        assert_eq!(paragraphs[0].text_content(), "first paragraph of the same task item");
        assert_eq!(paragraphs[1].text_content(), "second paragraph of the same task item");
        assert_eq!(paragraphs[2].text_content(), "next real task item");

        let first_shape = result
            .style_registry
            .para_shape(paragraphs[0].para_shape_id)
            .expect("first para shape");
        let continuation_shape = result
            .style_registry
            .para_shape(paragraphs[1].para_shape_id)
            .expect("continuation para shape");

        assert!(matches!(
            first_shape.list,
            Some(ParagraphListRef::CheckBullet { level: 0, checked: false, .. })
        ));
        assert_eq!(continuation_shape.list, None);
        assert_eq!(continuation_shape.indent_left, first_shape.indent_left);
        assert_eq!(continuation_shape.indent_first_line, HwpUnit::ZERO);
    }

    #[test]
    fn decode_ordered_task_list_normalizes_to_checkable_bullet() {
        use hwpforge_core::ParagraphListRef;

        let template = default_template();
        let result = MdDecoder::decode("1. [x] done", &template).unwrap();
        let paragraphs = &result.document.sections()[0].paragraphs;

        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0].text_content(), "done");

        let shape = result
            .style_registry
            .para_shape(paragraphs[0].para_shape_id)
            .expect("ordered-task para shape");

        assert!(matches!(
            shape.list,
            Some(ParagraphListRef::CheckBullet { level: 0, checked: true, .. })
        ));
    }

    #[test]
    fn decode_unordered_task_list_nested_under_ordered_parent_is_allowed() {
        use hwpforge_core::ParagraphListRef;

        let template = default_template();
        let result = MdDecoder::decode("1. parent\n   - [x] child", &template).unwrap();
        let paragraphs = &result.document.sections()[0].paragraphs;

        assert_eq!(paragraphs[0].text_content(), "1. parent");
        assert_eq!(paragraphs[1].text_content(), "child");

        let child_shape = result
            .style_registry
            .para_shape(paragraphs[1].para_shape_id)
            .expect("child para shape");

        assert!(matches!(
            child_shape.list,
            Some(ParagraphListRef::CheckBullet { level: 1, checked: true, .. })
        ));
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

        // row 0 is the GFM header ("Link"); the link data cell is row 1.
        let cell_paragraph = &table_run.rows[1].cells[0].paragraphs[0];
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
        // row 0 is the GFM header ("Img"); the image data cell is row 1.
        let cell_runs = &table.rows[1].cells[0].paragraphs[0].runs;
        assert!(cell_runs.iter().any(
            |run| matches!(run.content, RunContent::Image(ref img) if img.path == "logo.png")
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

    // -----------------------------------------------------------------------
    // W0: inline formatting stack (bold/italic/strikethrough)
    // -----------------------------------------------------------------------

    /// Finds the body-style section paragraph containing `needle`.
    fn body_para_with<'a>(
        doc: &'a hwpforge_core::document::Document<hwpforge_core::Draft>,
        needle: &str,
    ) -> &'a hwpforge_core::paragraph::Paragraph {
        doc.sections()[0]
            .paragraphs
            .iter()
            .find(|p| p.text_content().contains(needle))
            .expect("paragraph containing needle")
    }

    #[test]
    fn w0_bold_run_derives_char_shape() {
        let template = default_template();
        let result = MdDecoder::decode("text **bold** tail", &template).unwrap();
        let para = body_para_with(&result.document, "bold");
        let texts: Vec<_> = para.runs.iter().filter_map(|r| r.content.plain_text()).collect();
        assert_eq!(texts, vec!["text ", "bold", " tail"]);
        let base = para.runs[0].char_shape_id;
        let derived = para.runs[1].char_shape_id;
        assert_ne!(base, derived, "bold run must use a derived char shape");
        assert_eq!(para.runs[2].char_shape_id, base, "tail returns to base shape");
        let cs = result.style_registry.char_shape(derived).expect("derived shape registered");
        assert!(cs.bold);
        assert!(!cs.italic);
    }

    #[test]
    fn w0_italic_run_derives_char_shape() {
        let template = default_template();
        let result = MdDecoder::decode("a *it* b", &template).unwrap();
        let para = body_para_with(&result.document, "it");
        let cs = result
            .style_registry
            .char_shape(para.runs[1].char_shape_id)
            .expect("derived shape registered");
        assert!(cs.italic);
        assert!(!cs.bold);
    }

    #[test]
    fn w0_strikethrough_run_derives_char_shape() {
        let template = default_template();
        let result = MdDecoder::decode("a ~~gone~~ b", &template).unwrap();
        let para = body_para_with(&result.document, "gone");
        let cs = result
            .style_registry
            .char_shape(para.runs[1].char_shape_id)
            .expect("derived shape registered");
        assert_ne!(cs.strikeout_shape, hwpforge_foundation::StrikeoutShape::None);
    }

    #[test]
    fn w0_nested_bold_italic_combines() {
        let template = default_template();
        let result = MdDecoder::decode("**bold *both***", &template).unwrap();
        let para = body_para_with(&result.document, "both");
        let texts: Vec<_> = para.runs.iter().filter_map(|r| r.content.plain_text()).collect();
        assert_eq!(texts, vec!["bold ", "both"]);
        let bold_cs = result.style_registry.char_shape(para.runs[0].char_shape_id).unwrap();
        assert!(bold_cs.bold && !bold_cs.italic);
        let both_cs = result.style_registry.char_shape(para.runs[1].char_shape_id).unwrap();
        assert!(both_cs.bold && both_cs.italic);
    }

    #[test]
    fn w0_same_format_combo_reuses_derived_shape() {
        let template = default_template();
        let before =
            MdDecoder::decode("plain", &template).unwrap().style_registry.char_shape_count();
        let result = MdDecoder::decode("**a** x **b**", &template).unwrap();
        let para = body_para_with(&result.document, "a");
        assert_eq!(
            para.runs[0].char_shape_id, para.runs[2].char_shape_id,
            "identical combos share one derived shape"
        );
        assert_eq!(
            result.style_registry.char_shape_count(),
            before + 1,
            "exactly one derived shape for one combo"
        );
    }

    #[test]
    fn w0_unformatted_text_adds_no_derived_shapes() {
        let template = default_template();
        let before =
            MdDecoder::decode("plain", &template).unwrap().style_registry.char_shape_count();
        let after =
            MdDecoder::decode("plain again", &template).unwrap().style_registry.char_shape_count();
        assert_eq!(before, after, "no formatting => registry untouched");
    }

    #[test]
    fn w0_formatting_inside_heading_derives_from_heading_base() {
        let template = default_template();
        let (mapping, _) = resolve_mapping(&template).unwrap();
        let result = MdDecoder::decode("# head **bold**", &template).unwrap();
        let para = &result.document.sections()[0].paragraphs[0];
        assert_eq!(para.runs[0].char_shape_id, mapping.heading1.char_shape_id);
        let derived = result.style_registry.char_shape(para.runs[1].char_shape_id).unwrap();
        assert!(derived.bold);
    }

    // -----------------------------------------------------------------------
    // W1: footnote / endnote decoding
    // -----------------------------------------------------------------------

    use hwpforge_core::control::Control;

    /// Collects `(is_endnote, body_paragraph_texts)` for every note control in
    /// document order (body paragraphs + table cells, source order).
    fn collect_notes(
        doc: &hwpforge_core::document::Document<hwpforge_core::Draft>,
    ) -> Vec<(bool, Vec<String>)> {
        fn walk(paras: &[Paragraph], out: &mut Vec<(bool, Vec<String>)>) {
            for p in paras {
                for run in &p.runs {
                    match &run.content {
                        RunContent::Control(c) => match c.as_ref() {
                            Control::Footnote { paragraphs, .. } => out.push((
                                false,
                                paragraphs.iter().map(|b| b.text_content()).collect(),
                            )),
                            Control::Endnote { paragraphs, .. } => out.push((
                                true,
                                paragraphs.iter().map(|b| b.text_content()).collect(),
                            )),
                            _ => {}
                        },
                        RunContent::Table(t) => {
                            for row in &t.rows {
                                for cell in &row.cells {
                                    walk(&cell.paragraphs, out);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut out = Vec::new();
        for section in doc.sections() {
            walk(&section.paragraphs, &mut out);
        }
        out
    }

    #[test]
    fn w1_basic_footnote_resolves_at_reference_site() {
        let template = default_template();
        let result = MdDecoder::decode("본문[^1] 끝\n\n[^1]: 각주 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0], (false, vec!["각주 본문".to_string()]));
        // 참조 지점: 본문 문단 안 inline control (gotcha #12)
        let para = &result.document.sections()[0].paragraphs[0];
        assert!(para.text_content().contains("본문"));
        assert!(para
            .runs
            .iter()
            .any(|r| matches!(&r.content, RunContent::Control(c) if c.is_footnote())));
    }

    #[test]
    fn w1_undefined_reference_stays_literal_text() {
        // P2 파서 동작: 정의 없는 참조는 이벤트가 아니라 평문으로 강등된다.
        let template = default_template();
        let result = MdDecoder::decode("본문[^nodef] 끝\n", &template).unwrap();
        let text = result.document.sections()[0].paragraphs[0].text_content();
        assert!(text.contains("[^nodef]"), "literal must be preserved: {text}");
        assert!(collect_notes(&result.document).is_empty());
    }

    #[test]
    fn w1_orphan_definition_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문뿐\n\n[^orphan]: 아무도 안 씀\n", &template).unwrap_err();
        assert!(
            matches!(&err, MdError::OrphanNoteDefinition { label } if label == "orphan"),
            "got: {err:?}"
        );
    }

    #[test]
    fn w1_duplicate_definition_is_typed_error() {
        let template = default_template();
        let err =
            MdDecoder::decode("본문[^1]\n\n[^1]: 첫째\n\n[^1]: 둘째\n", &template).unwrap_err();
        assert!(matches!(&err, MdError::DuplicateNoteDefinition { label } if label == "1"));
    }

    #[test]
    fn w1_empty_definition_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문[^1]\n\n[^1]:\n", &template).unwrap_err();
        assert!(matches!(&err, MdError::EmptyNoteDefinition { label } if label == "1"));
    }

    #[test]
    fn w1_nested_reference_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문[^1]\n\n[^1]: 안에서[^2] 참조\n\n[^2]: 둘째\n", &template)
            .unwrap_err();
        assert!(matches!(&err, MdError::NestedNoteReference { label } if label == "2"));
    }

    #[test]
    fn w1_block_content_in_definition_is_rejected() {
        let template = default_template();
        for md in [
            "본문[^1]\n\n[^1]: 머리\n\n    - 항목\n",
            "본문[^1]\n\n[^1]: 머리\n\n    ```\n    code\n    ```\n",
        ] {
            let err = MdDecoder::decode(md, &template).unwrap_err();
            assert!(
                matches!(&err, MdError::UnsupportedStructure { .. }),
                "block content must be rejected: {md:?} → {err:?}"
            );
        }
    }

    #[test]
    fn w1_definition_before_reference_is_fine() {
        let template = default_template();
        let result = MdDecoder::decode("[^1]: 본문 먼저\n\n참조[^1]\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes, vec![(false, vec!["본문 먼저".to_string()])]);
    }

    #[test]
    fn w1_e_prefix_label_is_endnote_others_footnote() {
        let template = default_template();
        let md = "가[^e1] 나[^1] 다[^note] 라[^example]\n\n[^e1]: 미주\n\n[^1]: 각주하나\n\n[^note]: 각주둘\n\n[^example]: 각주셋\n";
        let result = MdDecoder::decode(md, &template).unwrap();
        let kinds: Vec<bool> = collect_notes(&result.document).iter().map(|(e, _)| *e).collect();
        assert_eq!(kinds, vec![true, false, false, false], "only e<digits> is an endnote");
    }

    #[test]
    fn w1_same_label_multi_reference_duplicates_body() {
        let template = default_template();
        let result = MdDecoder::decode("가[^1] 나[^1]\n\n[^1]: 공유 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes.len(), 2, "each reference gets its own note control");
        assert_eq!(notes[0].1, notes[1].1);
    }

    #[test]
    fn w1_table_cell_and_body_references_resolve_in_source_order() {
        let template = default_template();
        let md = "| a | b |\n|---|---|\n| 셀[^1] | x |\n\n본문[^2]\n\n[^1]: 셀 각주\n\n[^2]: 본문 각주\n";
        let result = MdDecoder::decode(md, &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(
            notes,
            vec![(false, vec!["셀 각주".to_string()]), (false, vec!["본문 각주".to_string()])],
            "cell note (source-first) must resolve before body note"
        );
    }

    #[test]
    fn w1_multiparagraph_definition_keeps_paragraphs() {
        let template = default_template();
        let md = "본문[^1]\n\n[^1]: 첫 문단\n\n    둘째 문단\n\n    셋째 문단\n";
        let result = MdDecoder::decode(md, &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(
            notes[0].1,
            vec!["첫 문단".to_string(), "둘째 문단".to_string(), "셋째 문단".to_string()]
        );
    }

    #[test]
    fn w1_definition_body_keeps_inline_formatting_runs() {
        let template = default_template();
        let result = MdDecoder::decode("본문[^1]\n\n[^1]: 여기 **굵게** 끝\n", &template).unwrap();
        let doc = &result.document;
        let mut found = false;
        for p in &doc.sections()[0].paragraphs {
            for run in &p.runs {
                if let RunContent::Control(c) = &run.content {
                    if let Control::Footnote { paragraphs, .. } = c.as_ref() {
                        let body = &paragraphs[0];
                        assert!(body.runs.len() >= 3, "formatting must split runs");
                        let bold_run = &body.runs[1];
                        let cs = result
                            .style_registry
                            .char_shape(bold_run.char_shape_id)
                            .expect("derived shape");
                        assert!(cs.bold, "bold must survive inside note body");
                        found = true;
                    }
                }
            }
        }
        assert!(found, "footnote control must exist");
    }

    #[test]
    fn w1_mixed_notes_preserve_order_and_kind() {
        let template = default_template();
        let md = "가[^1] 나[^e1] 다[^2]\n\n[^1]: 각주 하나\n\n[^e1]: 미주 하나\n\n[^2]: 각주 둘\n";
        let result = MdDecoder::decode(md, &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(
            notes,
            vec![
                (false, vec!["각주 하나".to_string()]),
                (true, vec!["미주 하나".to_string()]),
                (false, vec!["각주 둘".to_string()])
            ]
        );
    }

    #[test]
    fn w1_case_folded_labels_link_and_classify_together() {
        // 파서는 라벨을 UniCase 로 연결한다 (P13) — 분류·매칭도 동일 기준.
        let template = default_template();
        let result = MdDecoder::decode("본문[^E1]\n\n[^e1]: 미주 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes, vec![(true, vec!["미주 본문".to_string()])]);
    }

    #[test]
    fn w1_note_body_style_uses_footnote_style_slot() {
        // D6: 본문 스타일 재사용이 아니라 기존 "각주"(14)/"미주"(15) 스타일 참조.
        let template = default_template();
        let result = MdDecoder::decode("본문[^1]\n\n[^1]: 각주 본문\n", &template).unwrap();
        let notes_style = result
            .style_registry
            .get_style(crate::internal_styles::FOOTNOTE_STYLE_NAME)
            .expect("derived footnote style");
        let doc = &result.document;
        let mut checked = false;
        for p in &doc.sections()[0].paragraphs {
            for run in &p.runs {
                if let RunContent::Control(c) = &run.content {
                    if let Control::Footnote { paragraphs, .. } = c.as_ref() {
                        assert_eq!(paragraphs[0].para_shape_id, notes_style.para_shape_id);
                        checked = true;
                    }
                }
            }
        }
        assert!(checked);
    }

    #[test]
    fn w1_expansion_budget_rejects_pathological_duplication() {
        let template = default_template();
        // 큰 본문 1개를 다수 참조 — 전개 총량이 예산을 넘으면 typed 에러.
        let big = "가".repeat(600_000);
        let refs: String = (0..20).map(|_| "x[^1] ").collect();
        let md = format!("{refs}\n\n[^1]: {big}\n");
        let err = MdDecoder::decode(&md, &template).unwrap_err();
        assert!(matches!(&err, MdError::NoteExpansionBudgetExceeded { .. }), "got: {err:?}");
    }
}
