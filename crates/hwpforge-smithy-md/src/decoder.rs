//! Markdown -> Core decoder.

use std::path::Path;

use hwpforge_blueprint::builtins::builtin_default;

/// Maximum markdown file size: 50 MB.
pub(crate) const MAX_MD_FILE_SIZE: u64 = 50 * 1024 * 1024;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::template::Template;
use hwpforge_core::table::grid;
use hwpforge_core::{
    Control, Document, Image, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{
    CharShapeIndex, HwpUnit, ParaShapeIndex, StrikeoutShape, UnderlineType, VerticalPosition,
};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use unicase::UniCase;

use crate::encoder::{guard, MdWarning};
use crate::error::{MdError, MdResult};
use crate::frontmatter::{apply_to_metadata, extract_frontmatter};
use crate::mapper::{image_format_from_path, resolve_mapping, MdMapping, MdStyleRef};

mod lossless;

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
#[non_exhaustive]
pub struct MdDocument {
    /// The decoded Core document.
    pub document: Document,
    /// The style registry resolved from the template.
    pub style_registry: StyleRegistry,
    /// What the Markdown asked for that the document does not hold: an image
    /// with an empty target (`![x]()`) names no file, so it is left out and
    /// reported as [`MdWarning::ImageEmbedSkipped`] with an empty `src`, the
    /// same warning an image that could not be read gives; a chart or memo
    /// comment is left out as [`MdWarning::ContentLost`].
    pub warnings: Vec<MdWarning>,
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
        let mut warnings = mapping.warnings.clone();
        warnings.extend(decoded.warnings);

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

        Ok(MdDocument { document, style_registry, warnings })
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
        para.style_ref = Some(self.style.style_ref);

        para
    }
}

/// One cell being read: its finished paragraphs, the paragraph being
/// written, and its spans (1 for a GFM cell).
#[derive(Debug, Clone)]
struct CellBuilder {
    paragraphs: Vec<Vec<Run>>,
    runs: Vec<Run>,
    col_span: u16,
    row_span: u16,
}

/// Where an HTML table's reading stands: the tag it may meet next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlTableAt {
    /// After `<table>`: `<tbody>`.
    Table,
    /// In `<tbody>`: `<tr>` or `</tbody>`.
    Body,
    /// In `<tr>`: `<td>` or `</tr>`.
    Row,
    /// In `<td>`: its content or `</td>`.
    Cell,
    /// After `</tbody>`: `</table>`.
    Done,
}

/// A table being read. A GFM table (`html` is `None`) fills short rows;
/// an HTML table keeps its rows and spans as written, so Core judges them.
#[derive(Debug, Clone)]
struct TableBuilder {
    rows: Vec<Vec<CellBuilder>>,
    current_row: Vec<CellBuilder>,
    current_cell: Option<CellBuilder>,
    row_open: bool,
    /// An HTML table: where it stands, and how many HTML formats were open
    /// when its cell opened (the cell must close them all).
    html: Option<(HtmlTableAt, usize)>,
}

impl TableBuilder {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: None,
            row_open: false,
            html: None,
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
        self.start_spanned_cell(1, 1);
    }

    fn start_spanned_cell(&mut self, col_span: u16, row_span: u16) {
        self.end_cell();
        self.current_cell =
            Some(CellBuilder { paragraphs: Vec::new(), runs: Vec::new(), col_span, row_span });
    }

    fn end_cell(&mut self) {
        if let Some(mut cell) = self.current_cell.take() {
            cell.paragraphs.push(std::mem::take(&mut cell.runs));
            self.current_row.push(cell);
        }
    }

    /// A `<br>` in the cell: its paragraph ends, the next begins.
    fn break_paragraph(&mut self) {
        if let Some(cell) = self.current_cell.as_mut() {
            cell.paragraphs.push(std::mem::take(&mut cell.runs));
        }
    }

    fn push_text_with_style(&mut self, text: &str, char_shape_id: CharShapeIndex) {
        let Some(cell) = self.current_cell.as_mut().filter(|_| !text.is_empty()) else {
            return;
        };
        if let Some(last) = cell.runs.last_mut() {
            if let RunContent::Text(existing) = &mut last.content {
                if last.char_shape_id == char_shape_id {
                    existing.push_str(text);
                    return;
                }
            }
        }
        cell.runs.push(Run::text(text, char_shape_id));
    }

    fn push_run(&mut self, run: Run) {
        if let Some(cell) = self.current_cell.as_mut() {
            cell.runs.push(run);
        }
    }

    fn is_in_cell(&self) -> bool {
        self.current_cell.is_some()
    }

    fn into_table(
        mut self,
        body_style: MdStyleRef,
        page: hwpforge_core::PageSettings,
    ) -> MdResult<Table> {
        self.end_row();
        let html = self.html.is_some();
        let mut gfm_cols = None;
        let empty = || CellBuilder {
            paragraphs: vec![Vec::new()],
            runs: Vec::new(),
            col_span: 1,
            row_span: 1,
        };
        if !html {
            if self.rows.is_empty() {
                self.rows.push(vec![empty()]);
            }
            let max_cols = self.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            if max_cols > 10_000 {
                return Err(MdError::UnsupportedStructure {
                    detail: format!("table has too many columns: {max_cols}"),
                });
            }
            for row in &mut self.rows {
                row.resize_with(max_cols, empty);
            }
            gfm_cols = Some(max_cols);
        }
        let paragraph = |runs: Vec<Run>| {
            let runs =
                if runs.is_empty() { vec![Run::text("", body_style.char_shape_id)] } else { runs };
            Paragraph::with_runs(runs, body_style.para_shape_id).with_style(body_style.style_ref)
        };
        let mut table = Table::new(
            self.rows
                .into_iter()
                .map(|row| {
                    TableRow::new(
                        row.into_iter()
                            .map(|cell| {
                                let mut out = TableCell::new(
                                    cell.paragraphs.into_iter().map(paragraph).collect(),
                                    HwpUnit::ZERO,
                                );
                                out.col_span = cell.col_span;
                                out.row_span = cell.row_span;
                                out
                            })
                            .collect(),
                    )
                })
                .collect(),
        );
        // Widths are a layout default, an equal share of the printable
        // width per column: they change no span, row or cell.
        let fallback = HwpUnit::from_mm(40.0)?;
        let share = table_columns(&table, gfm_cols)
            .and_then(|cols| i32::try_from(cols).ok())
            .filter(|&c| c > 0)
            .map(|c| page.printable_width() / c)
            .filter(|w| w.as_i32() > 0);
        for cell in table.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
            cell.width = share
                .and_then(|w| w.as_i32().checked_mul(i32::from(cell.col_span.max(1))))
                .and_then(|w| HwpUnit::new(w).ok())
                .unwrap_or(fallback);
        }
        Ok(table)
    }
}

/// The columns a table's widths are shared over: a GFM table's are its
/// widest row (`gfm_cols`, every span 1), however large; an HTML table's
/// are its placed grid's, placed only when the area its spans cover is
/// within Core's limit (`None` past it: nothing is allocated per position).
/// 이것을 실패시키는 것: GFM 에도 면적 검사 · HTML 에 면적 검사 없이 배치
/// (`table_columns_check_the_area_of_html_tables_only`, `huge_spans_take_the_fallback_width`).
fn table_columns(table: &Table, gfm_cols: Option<usize>) -> Option<u32> {
    match gfm_cols {
        Some(cols) => u32::try_from(cols).ok(),
        None => (grid::covered_area(table) <= grid::MAX_GRID_POSITIONS)
            .then(|| grid::grid_placements(table).cols),
    }
}

/// 전개된 각주/미주 본문의 총량 상한 (다중 참조 복제 증폭 방어).
const MAX_EXPANDED_NOTE_BYTES: usize = 8 * 1024 * 1024;

/// 각주/미주 정의의 해석 상태 (소비하지 않고 참조 수만 센다).
#[derive(Debug)]
struct NoteDefinition {
    /// 원문 라벨 (에러 메시지용 — canonical 은 맵 키).
    label: String,
    /// 본문 문단들 (다문단 지원).
    paragraphs: Vec<Paragraph>,
    /// resolve 에서 이 정의를 참조한 횟수. 0 이면 고아 (에러).
    reference_count: u32,
}

/// 정의 수집 모드 — 본문 빌더와 **격리**된 캡처.
///
/// 전역 리스트/인용/표 상태를 읽지도 쓰지도 않는다. 화이트리스트:
/// 문단 · 텍스트 · 인라인 서식(스택 재사용) · soft/hard break ·
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
    /// character shapes (bold/italic/strikethrough runs).
    registry: &'a mut StyleRegistry,
    /// Inline-format nesting counters (pulldown-cmark guarantees balanced
    /// start/end tags; counters keep same-tag nesting safe). HTML formats
    /// (`<strong>` … `<sub>`) count here too; their order is checked on
    /// [`Self::html_open`].
    fmt_bold: u32,
    fmt_italic: u32,
    fmt_strike: u32,
    fmt_underline: u32,
    fmt_sup: u32,
    fmt_sub: u32,
    /// HTML formats and links opened and not yet closed, innermost last.
    html_open: Vec<String>,
    /// The lines of the HTML block being read, read whole at its end.
    html_block: Option<String>,
    /// Cache of derived char shapes: (base shape index, format flags) → index.
    derived_shapes: std::collections::HashMap<(usize, u8), CharShapeIndex>,
    /// 각주/미주 정의 — 키는 파서(UniCase)와 동일한 case-fold canonical.
    note_definitions: std::collections::HashMap<UniCase<String>, NoteDefinition>,
    /// 참조를 만난 순서 (resolve 가 이 순서로 빈 컨트롤을 채운다).
    pending_notes: Vec<UniCase<String>>,
    /// 정의 수집 모드 (Some 이면 활성).
    definition_capture: Option<DefinitionCapture>,
    /// builtin "각주" 스타일 (없으면 body 폴백).
    footnote_style: MdStyleRef,
    /// builtin "미주" 스타일 (없으면 body 폴백).
    endnote_style: MdStyleRef,
    paragraphs: Vec<Paragraph>,
    current: Option<ParagraphBuilder>,
    /// The tables being read, innermost last: a GFM table, or HTML tables
    /// nested in a block (read whole within it).
    tables: Vec<TableBuilder>,
    blockquote_depth: usize,
    in_code_block: bool,
    in_item: bool,
    pending_items: Vec<PendingItem>,
    list_stack: Vec<ListState>,
    pending_link: Option<PendingLink>,
    pending_image: Option<PendingImage>,
    section_breaks: Vec<usize>,
    warnings: Vec<MdWarning>,
}

#[derive(Debug)]
struct DecodeOutput {
    paragraphs: Vec<Paragraph>,
    section_breaks: Vec<usize>,
    warnings: Vec<MdWarning>,
}

impl<'a> DecoderState<'a> {
    fn new(mapping: &'a MdMapping, registry: &'a mut StyleRegistry) -> Self {
        let style_of = |name: &str| {
            registry
                .get_style(name)
                .map(|e| MdStyleRef {
                    style_ref: e.style_ref(),
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
            fmt_underline: 0,
            fmt_sup: 0,
            fmt_sub: 0,
            html_open: Vec::new(),
            html_block: None,
            derived_shapes: std::collections::HashMap::new(),
            paragraphs: Vec::new(),
            current: None,
            tables: Vec::new(),
            blockquote_depth: 0,
            in_code_block: false,
            in_item: false,
            pending_items: Vec::new(),
            list_stack: Vec::new(),
            pending_link: None,
            pending_image: None,
            section_breaks: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn decode_markdown(&mut self, content: &str) -> MdResult<()> {
        let parser = Parser::new_ext(content, crate::encoder::md_options());
        for event in parser {
            self.handle_event(event)?;
        }
        Ok(())
    }

    fn finish(mut self) -> MdResult<DecodeOutput> {
        if !self.tables.is_empty() {
            return Err(MdError::UnsupportedStructure {
                detail: "table was not properly closed".to_string(),
            });
        }

        self.finalize_paragraph();
        if self.paragraphs.is_empty() {
            self.paragraphs.push(ParagraphBuilder::new(self.mapping.body).build());
        }
        self.resolve_notes()?;
        Ok(DecodeOutput {
            paragraphs: self.paragraphs,
            section_breaks: self.section_breaks,
            warnings: self.warnings,
        })
    }

    /// A안 resolve — 빈 각주/미주 컨트롤을 참조 순서대로 정의 본문으로 채운다.
    ///
    /// 순회는 **소스 순서 보장 전용 in-order run walk** 다: run 을 순서대로
    /// 걷고 `Table` run 을 만나면 그 자리에서 셀 문단을 재귀한다. 기존
    /// `walk_paragraphs_mut` 는 문단을 먼저 방문(`f(self)` 후 run 재귀)해
    /// `[표, 컨트롤]` 순서 문단에서 소스 순서와 어긋날 수 있어 쓰지 않는다.
    /// 이 보장의 계약 범위는 MD 디코더가 생성 가능한 형상(본문 문단 +
    /// 비병합 GFM 표 셀)뿐이다.
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
                                // Markdown 의 정의 없는 참조는 파서가 이벤트를 만들지
                                // 않는다. 여기 오는 것은 HTML 칸의 `[^label]` 이다.
                                return Err(MdError::UnsupportedStructure {
                                    detail: format!(
                                        "note reference '[^{}]' has no definition",
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
        // HTML takes one path in every context (body, list item, quote,
        // table cell, note definition): an inline tag at a time, or a block
        // of lines read whole at its end.
        // 이것을 실패시키는 것: 정의 수집 앞에서 HTML 을 가로채지 않기
        // (`note_definition_reads_a_leading_comment_block`).
        let event = match event {
            Event::InlineHtml(html) => return self.read_html(&html),
            Event::Start(Tag::HtmlBlock) => {
                self.html_block = Some(String::new());
                return Ok(());
            }
            Event::Html(html) => {
                self.html_block.get_or_insert_with(String::new).push_str(&html);
                return Ok(());
            }
            Event::End(TagEnd::HtmlBlock) => return self.end_html_block(),
            other => other,
        };
        if self.definition_capture.is_some() {
            return self.handle_definition_event(event);
        }
        match event {
            Event::Start(tag) => self.start_tag(tag)?,
            Event::End(tag_end) => self.end_tag(tag_end)?,
            Event::Text(text) => self.push_text(text.as_ref())?,
            Event::Code(code) => self.push_inline_code(code.as_ref())?,
            Event::InlineMath(math) | Event::DisplayMath(math) => self.push_text(math.as_ref())?,
            // Taken above.
            Event::Html(_) | Event::InlineHtml(_) => {}
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
            Tag::Link { dest_url, .. } => self.start_link_tag(&dest_url)?,
            Tag::Image { dest_url, .. } => self.start_image_tag(&dest_url),
            Tag::Strong => self.fmt_bold += 1,
            Tag::Emphasis => self.fmt_italic += 1,
            Tag::Strikethrough => self.fmt_strike += 1,
            // Taken in `handle_event`.
            Tag::HtmlBlock => {}
            Tag::Superscript | Tag::Subscript => {}
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
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                self.html_closed()?;
                self.finalize_paragraph();
            }
            TagEnd::BlockQuote(_) => self.end_blockquote_tag(),
            TagEnd::CodeBlock => self.end_code_block_tag(),
            TagEnd::List(_) => self.end_list_tag(),
            TagEnd::Item => {
                self.html_closed()?;
                self.end_item_tag();
            }
            TagEnd::Table => self.finalize_table()?,
            TagEnd::TableHead => self.end_table_row_tag(),
            TagEnd::TableRow => self.end_table_row_tag(),
            TagEnd::TableCell => {
                self.html_closed()?;
                self.end_table_cell_tag();
            }
            TagEnd::Link => self.end_link_tag(),
            TagEnd::Image => self.end_image_tag()?,
            TagEnd::Strong => self.fmt_bold = self.fmt_bold.saturating_sub(1),
            TagEnd::Emphasis => self.fmt_italic = self.fmt_italic.saturating_sub(1),
            TagEnd::Strikethrough => self.fmt_strike = self.fmt_strike.saturating_sub(1),
            // Taken in `handle_event`.
            TagEnd::HtmlBlock => {}
            TagEnd::Superscript | TagEnd::Subscript => {}
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

    /// The end of an HTML block: its lines read as one fragment, which must
    /// close what it opens; its text is a paragraph of its own, in the
    /// note definition being read or in the body.
    fn end_html_block(&mut self) -> MdResult<()> {
        let block = self.html_block.take().unwrap_or_default();
        self.read_html(&block)?;
        self.html_closed()?;
        match self.definition_capture.as_mut() {
            Some(capture) => {
                if let Some(builder) = capture.current.take() {
                    capture.paragraphs.push(builder.build());
                }
            }
            None => self.finalize_paragraph(),
        }
        Ok(())
    }

    /// Reads HTML as the encoder writes it ([`guard::tokens`]): formats,
    /// `<br>`, `<a href>`, `<img src alt>`, its comments and tables
    /// ([`Self::html_table_tag`]). Anything else is raw HTML and
    /// fails. In a note definition links, pictures and tables fail as their
    /// Markdown does. In a table cell `<br>` ends the cell's paragraph (in a
    /// link, it is the link text's line break).
    ///
    /// Text (only a block has any) is read as a paragraph's: a raw line end
    /// is a soft break, a space, the line's surrounding spaces dropped; one
    /// written `&#10;` is content. Raw whitespace before the first content
    /// and after the last is left out; whitespace between content is kept,
    /// written before the next text or the next tag that opens or closes,
    /// so it takes the formats open where it stood.
    /// 이것을 실패시키는 것: 표 태그 받기 · 정의 안 `<a>` 받기 · 셀 안 `<br>` 받기 ·
    /// 앞 공백 넣기 · 내용 사이 공백 버리기 · 줄 끝을 글로 넣기 · `&#10;` 를 줄 끝으로 읽기
    /// (`html_*` 시험들).
    fn read_html(&mut self, html: &str) -> MdResult<()> {
        let raw_html = || unsupported_markdown_feature("raw HTML");
        let tokens = guard::tokens(html.trim_end()).ok_or_else(raw_html)?;
        let in_note = self.definition_capture.is_some();
        let tables = self.tables.len();
        // Where the last content is: whitespace after it is left out.
        let last = tokens.iter().rposition(|t| match t {
            guard::Token::Text(raw) => !raw.trim().is_empty(),
            guard::Token::Open { name, .. } => matches!(*name, "br" | "img"),
            _ => false,
        });
        let mut started = false;
        let mut space = String::new();
        for (i, token) in tokens.into_iter().enumerate() {
            let structure = matches!(
                token,
                guard::Token::Open { name: "table" | "tbody" | "tr" | "td", .. }
                    | guard::Token::Close("table" | "tbody" | "tr" | "td")
            );
            let tag = !matches!(token, guard::Token::Text(_) | guard::Token::Comment(_));
            if structure {
                // A cell is a paragraph of its own: whitespace at its edges
                // is layout, as at a block's.
                // 이것을 실패시키는 것: 칸 경계 공백을 칸 글로 넣기
                // (`html_cell_text_keeps_its_spaces`).
                space.clear();
                started = false;
            } else if tag && last.is_some_and(|last| i <= last) {
                self.push_html_text(&std::mem::take(&mut space))?;
            }
            // An HTML table is read with the tag names. Outside its cells
            // only its structure and the layout whitespace between tags may
            // stand; a cell's text is read as a paragraph's.
            // 이것을 실패시키는 것: 칸 밖 글·주석·인라인 태그 받기
            // (`html_table_grammar_is_the_encoders`).
            let at = self.tables.last().and_then(|t| t.html).map(|(at, _)| at);
            let outside_cells = at.is_some() && at != Some(HtmlTableAt::Cell);
            match &token {
                guard::Token::Open {
                    name: name @ ("table" | "tbody" | "tr" | "td"),
                    attributes,
                } => {
                    self.html_table_tag(name, true, attributes, !in_note)?;
                    continue;
                }
                guard::Token::Close(name @ ("table" | "tbody" | "tr" | "td")) => {
                    self.html_table_tag(name, false, &[], !in_note)?;
                    continue;
                }
                guard::Token::Text(raw) if outside_cells => {
                    if !raw.trim().is_empty() {
                        return Err(raw_html());
                    }
                    continue;
                }
                _ if outside_cells => return Err(raw_html()),
                _ => {}
            }
            match token {
                guard::Token::Text(raw) => {
                    // A raw line end is a soft break: it and the spaces
                    // around it fold into one space.
                    let mut text = String::new();
                    for (n, line) in raw.split('\n').enumerate() {
                        if n > 0 {
                            text.truncate(text.trim_end().len());
                            text.push(' ');
                        }
                        text.push_str(if n > 0 { line.trim_start() } else { line });
                    }
                    // Whitespace is raw whitespace: a `&#10;` is content.
                    let core = text.trim();
                    if core.is_empty() {
                        if started {
                            space.push_str(&text);
                        }
                        continue;
                    }
                    if started {
                        space.push_str(&text[..text.len() - text.trim_start().len()]);
                    }
                    self.push_html_text(&std::mem::take(&mut space))?;
                    if at == Some(HtmlTableAt::Cell) {
                        self.push_cell_text(core)?;
                    } else {
                        self.push_html_text(&guard::unescape(core))?;
                    }
                    space.push_str(&text[text.trim_end().len()..]);
                    started = true;
                }
                guard::Token::Comment(guard::Comment::Section) => {
                    if in_note || self.is_in_table_cell() || !self.html_open.is_empty() {
                        return Err(raw_html());
                    }
                    self.push_section_marker();
                }
                guard::Token::Comment(guard::Comment::Chart) => {
                    self.warnings.push(MdWarning::ContentLost { kind: "chart" });
                }
                guard::Token::Comment(guard::Comment::Memo) => {
                    self.warnings.push(MdWarning::ContentLost { kind: "memo" });
                }
                guard::Token::Open { name: "br", .. } => {
                    match self.tables.last_mut() {
                        Some(table) if self.pending_link.is_none() && table.is_in_cell() => {
                            table.break_paragraph();
                        }
                        _ => self.push_html_text("\n")?,
                    }
                    started = true;
                }
                guard::Token::Open { name: "a", attributes } => {
                    let href = attributes.iter().find(|(n, _)| *n == "href");
                    let Some((_, href)) = href.filter(|_| !in_note) else { return Err(raw_html()) };
                    self.start_link_tag(href)?;
                    self.html_open.push("a".into());
                }
                guard::Token::Open { name: "img", attributes } => {
                    let value = |key| attributes.iter().find(|(n, _)| *n == key).map(|(_, v)| v);
                    let Some(src) =
                        value("src").filter(|_| !in_note && self.pending_image.is_none())
                    else {
                        return Err(raw_html());
                    };
                    self.start_image_tag(src);
                    self.push_text(value("alt").map_or("", String::as_str))?;
                    self.end_image_tag()?;
                    started = true;
                }
                guard::Token::Open { name, .. } => {
                    *self.html_format(name).ok_or_else(raw_html)? += 1;
                    if self.fmt_sup > 0 && self.fmt_sub > 0 {
                        return Err(MdError::UnsupportedStructure {
                            detail: "superscript and subscript at once".to_string(),
                        });
                    }
                    self.html_open.push(name.to_string());
                }
                guard::Token::Close(name) => {
                    if self.html_open.pop().as_deref() != Some(name) {
                        return Err(MdError::UnsupportedStructure {
                            detail: format!("HTML </{name}> does not close the innermost open tag"),
                        });
                    }
                    if name == "a" {
                        self.end_link_tag();
                    } else {
                        let count = self.html_format(name).ok_or_else(raw_html)?;
                        *count -= 1;
                    }
                }
            }
        }
        // A table is read within the HTML it opened in, so its text is
        // never Markdown (an inline `<table>` is one tag: it fails here).
        if self.tables.len() > tables {
            return Err(MdError::UnsupportedStructure {
                detail: "HTML table is not closed".to_string(),
            });
        }
        Ok(())
    }

    /// One HTML table tag, in the order the encoder writes them:
    /// `<table><tbody>` then `<tr>` rows of `<td>` cells, `</tbody></table>`;
    /// a `<table>` opens at the top (`allowed`: outside a note definition;
    /// not in a GFM cell) or in an HTML cell, at most
    /// [`crate::lossless_grammar::MAX_NESTING_DEPTH`] tables deep. A cell
    /// closes the HTML formats it opened. Rows and cells are kept as
    /// written, none added: whether the table holds is Core's to judge.
    /// 이것을 실패시키는 것: 순서 검사 하나씩 삭제 · 깊이 검사 삭제 · 칸의 서식 닫힘 검사 삭제
    /// (`html_table_grammar_is_the_encoders`, `html_tables_nest_32_deep`).
    fn html_table_tag(
        &mut self,
        name: &str,
        open: bool,
        attributes: &[(&str, String)],
        allowed: bool,
    ) -> MdResult<()> {
        use HtmlTableAt::{Body, Cell, Done, Row, Table as Start};
        let raw_html = || unsupported_markdown_feature(&format!("raw HTML <{name}> here"));
        let at = self.tables.last().and_then(|t| t.html).map(|(at, _)| at);
        let set = |me: &mut Self, at: HtmlTableAt| {
            if let Some(table) = me.tables.last_mut() {
                table.html = table.html.map(|(_, open)| (at, open));
            }
        };
        match (name, open, at) {
            ("table", true, None) if allowed && !self.is_in_table_cell() => {
                self.materialize_pending_item_paragraph_if_needed();
                self.finalize_paragraph();
            }
            ("table", true, Some(Cell)) => {}
            ("tbody", true, Some(Start)) => set(self, Body),
            ("tr", true, Some(Body)) => {
                set(self, Row);
                self.tables.last_mut().expect("html table").start_row();
            }
            ("td", true, Some(Row)) => {
                let span = |key| match attributes.iter().find(|(n, _)| *n == key) {
                    None => Some(1),
                    Some((_, v)) => v
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                        .then(|| v.parse::<u16>().ok())
                        .flatten()
                        .filter(|&n| n > 0),
                };
                let (Some(cols), Some(rows)) = (span("colspan"), span("rowspan")) else {
                    return Err(raw_html());
                };
                let formats = self.html_open.len();
                let table = self.tables.last_mut().expect("html table");
                table.html = Some((Cell, formats));
                table.start_spanned_cell(cols, rows);
            }
            ("td", false, Some(Cell)) => {
                let formats = self.tables.last().and_then(|t| t.html).map_or(0, |(_, n)| n);
                if self.html_open.len() != formats {
                    return Err(MdError::UnsupportedStructure {
                        detail: "HTML formats left open at the cell's end".to_string(),
                    });
                }
                set(self, Row);
                self.tables.last_mut().expect("html table").end_cell();
            }
            ("tr", false, Some(Row)) => {
                set(self, Body);
                self.tables.last_mut().expect("html table").end_row();
            }
            ("tbody", false, Some(Body)) => set(self, Done),
            ("table", false, Some(Done)) => {
                let builder = self.tables.pop().expect("html table");
                let table = builder.into_table(self.mapping.body, self.mapping.page_settings)?;
                if self.tables.last().is_some_and(|t| t.html.is_some()) {
                    let char_shape_id = self.current_char_shape_id();
                    self.push_run_to_active_context(Run::table(table, char_shape_id));
                } else {
                    self.push_table_paragraph(table);
                }
                return Ok(());
            }
            _ => return Err(raw_html()),
        }
        if name == "table" {
            let depth = self.tables.iter().filter(|t| t.html.is_some()).count() + 1;
            if depth > crate::lossless_grammar::MAX_NESTING_DEPTH {
                return Err(MdError::UnsupportedStructure {
                    detail: format!(
                        "HTML tables nested deeper than {}",
                        crate::lossless_grammar::MAX_NESTING_DEPTH
                    ),
                });
            }
            let mut table = TableBuilder::new();
            table.html = Some((Start, 0));
            self.tables.push(table);
        }
        Ok(())
    }

    /// Text of an HTML cell, raw: a `[^label]` in it is a note reference
    /// (the encoder writes every `[` of document text there as `&#91;`, so
    /// a raw one is its own reference), the rest text decoded once.
    /// 이것을 실패시키는 것: 칸 글의 각주 참조를 글로 넣기 · 엔티티 복원 뒤에 참조 찾기
    /// (`html_cell_notes_read_back`).
    fn push_cell_text(&mut self, raw: &str) -> MdResult<()> {
        // Each character is looked at once: a scan that fails resumes where
        // it stopped, and with no `]` ahead the rest is text.
        // 이것을 실패시키는 것: 실패마다 남은 글을 다시 훑기(`cell_note_scan_is_linear`).
        let (mut text, mut from) = (0, 0);
        while let Some(found) = raw[from..].find("[^") {
            let label = from + found + 2;
            let Some(stop) = raw[label..].find(|c: char| c == ']' || c == '[' || c.is_whitespace())
            else {
                break;
            };
            let stop = label + stop;
            if raw[stop..].starts_with(']') && stop > label {
                self.push_text(&guard::unescape(&raw[text..label - 2]))?;
                self.handle_footnote_reference(&raw[label..stop])?;
                text = stop + 1;
            }
            from = stop + usize::from(raw[stop..].starts_with(']'));
        }
        self.push_text(&guard::unescape(&raw[text..]))
    }

    /// The counter of an HTML format tag; `None` for any other tag.
    fn html_format(&mut self, name: &str) -> Option<&mut u32> {
        Some(match name {
            "strong" => &mut self.fmt_bold,
            "em" => &mut self.fmt_italic,
            "del" => &mut self.fmt_strike,
            "u" => &mut self.fmt_underline,
            "sup" => &mut self.fmt_sup,
            "sub" => &mut self.fmt_sub,
            _ => return None,
        })
    }

    /// Text from HTML (a `<br>` too) into the note definition being read,
    /// or where Markdown text goes.
    fn push_html_text(&mut self, text: &str) -> MdResult<()> {
        if text.is_empty() {
            Ok(())
        } else if self.definition_capture.is_some() {
            self.push_definition_text(text)
        } else {
            self.push_text(text)
        }
    }

    /// Fails when an HTML format or link is still open where its paragraph,
    /// item, cell or block ends: it must not run on into the next one.
    /// 이것을 실패시키는 것: 이 검사 삭제(`html_formats_must_close_in_their_paragraph`).
    fn html_closed(&self) -> MdResult<()> {
        match self.html_open.last() {
            Some(tag) => {
                Err(MdError::UnsupportedStructure { detail: format!("HTML <{tag}> is not closed") })
            }
            None => Ok(()),
        }
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

    /// 정의 수집 모드의 이벤트 처리 — 본문 빌더와 격리.
    fn handle_definition_event(&mut self, event: Event<'_>) -> MdResult<()> {
        match event {
            Event::Start(Tag::Paragraph) => {
                let capture = self.definition_capture.as_mut().expect("capture active");
                let style = capture.style;
                capture.current = Some(ParagraphBuilder::new(style));
                Ok(())
            }
            Event::End(TagEnd::Paragraph) => {
                self.html_closed()?;
                let capture = self.definition_capture.as_mut().expect("capture active");
                if let Some(builder) = capture.current.take() {
                    capture.paragraphs.push(builder.build());
                }
                Ok(())
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.html_closed()?;
                self.finish_footnote_definition()
            }
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

    /// 정의 본문 텍스트 — 각주 스타일 base 에서 서식 파생을 적용한다.
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
        self.tables.push(TableBuilder::new());
    }

    fn start_table_row_tag(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.start_row();
        }
    }

    fn start_table_cell_tag(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.start_cell();
        }
    }

    /// A link inside a link (HTML `<a>` and Markdown either way) fails.
    /// 이것을 실패시키는 것: 이 검사 삭제(`html_link_and_picture_take_the_markdown_paths`).
    fn start_link_tag(&mut self, dest_url: &str) -> MdResult<()> {
        if self.pending_link.is_some() {
            return Err(MdError::UnsupportedStructure { detail: "link inside a link".to_string() });
        }
        if !self.is_in_table_cell() {
            self.ensure_paragraph();
        }
        self.pending_link =
            Some(PendingLink { dest_url: encode_pending_link_url(dest_url), text: String::new() });
        Ok(())
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
        if let Some(table) = self.tables.last_mut() {
            table.end_row();
        }
    }

    fn end_table_cell_tag(&mut self) {
        if let Some(table) = self.tables.last_mut() {
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
            // `![x]()` names no file: report it as an image that could not be
            // read and leave it out. Only here is an empty path known to be
            // Markdown's own; a Hancom assetless frame (`binaryItemIDRef=""`)
            // reaches Core with the same empty path through the HWPX and
            // lossless readers and is kept everywhere, the asset stage too.
            // 이것을 실패시키는 것: 이 갈래 삭제(빈 틀이 경고 없이 남는다) 또는
            // 경고만 남기고 그림 넣기(`empty_image_target_is_left_out_and_reported`).
            if image.dest_url.is_empty() {
                self.warnings.push(MdWarning::ImageEmbedSkipped {
                    src: String::new(),
                    reason: crate::embed::ImageEmbedSkipReason::Unreadable,
                });
                return Ok(());
            }
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
        if let Some(table) = self.tables.last_mut() {
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
        if let Some(table) = self.tables.last_mut() {
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
        let table_builder = self.tables.pop().ok_or_else(|| MdError::UnsupportedStructure {
            detail: "table end tag without table start".to_string(),
        })?;

        let table = table_builder.into_table(self.mapping.body, self.mapping.page_settings)?;
        self.push_table_paragraph(table);
        Ok(())
    }

    /// A table read at the top: a paragraph of its own.
    fn push_table_paragraph(&mut self, table: Table) {
        let paragraph = Paragraph::with_runs(
            vec![Run::table(table, self.mapping.body.char_shape_id)],
            self.mapping.body.para_shape_id,
        )
        .with_style(self.mapping.body.style_ref);
        self.paragraphs.push(paragraph);
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

    /// Bitset of active inline formats: bit0 bold, bit1 italic, bit2 strike,
    /// bit3 underline, bit4 superscript, bit5 subscript.
    fn format_flags(&self) -> u8 {
        u8::from(self.fmt_bold > 0)
            | (u8::from(self.fmt_italic > 0) << 1)
            | (u8::from(self.fmt_strike > 0) << 2)
            | (u8::from(self.fmt_underline > 0) << 3)
            | (u8::from(self.fmt_sup > 0) << 4)
            | (u8::from(self.fmt_sub > 0) << 5)
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
        // An underline the base has keeps its shape and colour.
        if flags & 8 != 0 && derived.underline_type == UnderlineType::None {
            derived.underline_type = UnderlineType::Bottom;
        }
        if flags & 16 != 0 {
            derived.vertical_position = VerticalPosition::Superscript;
        }
        if flags & 32 != 0 {
            derived.vertical_position = VerticalPosition::Subscript;
        }
        let idx = CharShapeIndex::new(self.registry.char_shapes.len());
        self.registry.char_shapes.push(derived);
        self.derived_shapes.insert(key, idx);
        idx
    }

    fn is_in_table_cell(&self) -> bool {
        self.tables.last().is_some_and(TableBuilder::is_in_cell)
    }

    fn push_run_to_active_context(&mut self, run: Run) {
        if self.is_in_table_cell() {
            if let Some(table) = self.tables.last_mut() {
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
        .with_style(style.style_ref)
}

fn default_empty_section() -> Section {
    let paragraph =
        Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    Section::with_paragraphs(vec![paragraph], hwpforge_core::PageSettings::a4())
}

/// 미주 라벨 판별 — canonical(case-fold) 기준 `e` + 숫자 1개 이상.
///
/// `e[0-9]+` 는 HwpForge dialect 의 **미주 예약 이름공간**이다:
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

    /// The section marker splits sections in the body only: in a table
    /// cell, a note definition or across an open HTML format it fails.
    /// 이것을 실패시키는 것: 셀 검사 삭제 · 열린 서식 검사 삭제.
    #[test]
    fn section_marker_comment_splits_only_in_the_body() {
        let template = default_template();
        for markdown in [
            "| a |\n|---|\n| <!-- hwpforge:section --> |",
            "<u>a <!-- hwpforge:section --> b</u>",
            "x[^1]\n\n[^1]: a <!-- hwpforge:section --> b",
        ] {
            assert!(MdDecoder::decode(markdown, &template).is_err(), "{markdown}");
        }
        let doc = MdDecoder::decode("a <!-- hwpforge:section --> b", &template).unwrap().document;
        assert_eq!(doc.sections().len(), 2);
    }

    /// The text of each top-level paragraph of the first section.
    fn texts(md: &MdDocument) -> Vec<String> {
        md.document.sections()[0].paragraphs.iter().map(|p| p.text_content()).collect()
    }

    /// The character shape of each run of the first paragraph.
    fn run_shapes(md: &MdDocument) -> Vec<(String, hwpforge_blueprint::style::CharShape)> {
        md.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .map(|run| {
                let text = match &run.content {
                    RunContent::Text(t) => t.clone(),
                    other => format!("{other:?}"),
                };
                (text, md.style_registry.char_shape(run.char_shape_id).unwrap().clone())
            })
            .collect()
    }

    /// Each HTML format the encoder writes becomes that property of the
    /// run's character shape, and only of that run.
    /// 이것을 실패시키는 것: 태그와 카운터 대응 하나씩 바꾸기 · 밑줄·위첨자·아래첨자
    /// 파생 삭제.
    #[test]
    fn html_formats_become_character_shapes() {
        use hwpforge_blueprint::style::CharShape;
        let checks: [(&str, fn(&CharShape) -> bool); 6] = [
            ("strong", |c| c.bold),
            ("em", |c| c.italic),
            ("del", |c| c.strikeout_shape == StrikeoutShape::Solid),
            ("u", |c| c.underline_type == UnderlineType::Bottom),
            ("sup", |c| c.vertical_position == VerticalPosition::Superscript),
            ("sub", |c| c.vertical_position == VerticalPosition::Subscript),
        ];
        for (tag, has) in checks {
            let md = MdDecoder::decode_with_default(&format!("a<{tag}>b</{tag}>c")).unwrap();
            let runs = run_shapes(&md);
            let found: Vec<(&str, bool)> = runs.iter().map(|(t, c)| (t.as_str(), has(c))).collect();
            assert_eq!(found, [("a", false), ("b", true), ("c", false)], "{tag}");
        }
    }

    /// HTML formats nest, repeat and cross Markdown emphasis: each syntax
    /// closes its own range.
    /// 이것을 실패시키는 것: 닫기에서 카운터를 0 으로(같은 서식 중첩이 풀림).
    #[test]
    fn html_and_markdown_formats_nest_and_cross() {
        let md = MdDecoder::decode_with_default("<u><u>a</u>b</u> <u>c **d</u> e**").unwrap();
        let found: Vec<(String, bool, bool)> = run_shapes(&md)
            .into_iter()
            .map(|(t, c)| (t, c.bold, c.underline_type == UnderlineType::Bottom))
            .collect();
        let expected = [
            ("ab", false, true),
            (" ", false, false),
            ("c ", false, true),
            ("d", true, true),
            (" e", true, false),
        ];
        let expected: Vec<(String, bool, bool)> =
            expected.iter().map(|(t, b, u)| (t.to_string(), *b, *u)).collect();
        assert_eq!(found, expected);
    }

    /// An HTML format or link left open where its paragraph, list item,
    /// table cell, note definition or HTML block ends fails (every inline
    /// scope ends at one of these, the last one in the input too), and so
    /// does a close that is not the innermost open tag.
    /// 이것을 실패시키는 것: `html_closed` 를 늘 통과 · 블록 끝 검사 삭제 · 닫기 순서 검사 삭제.
    #[test]
    fn html_formats_must_close_in_their_paragraph() {
        for markdown in [
            "<u>a\n\nb",
            "- <u>a\n- b",
            "| <u>a |\n|---|",
            "x[^1]\n\n[^1]: <u>a",
            "<a href=\"https://e.invalid\">a\n\nb",
            "<u>\na",
            "a</u>",
            "<u><em>a</u></em>",
        ] {
            assert!(MdDecoder::decode_with_default(markdown).is_err(), "{markdown}");
        }
    }

    /// Superscript and subscript at once have no character shape.
    /// 이것을 실패시키는 것: 동시 검사 삭제.
    #[test]
    fn superscript_and_subscript_at_once_fail() {
        assert!(MdDecoder::decode_with_default("<sup>a<sub>b</sub></sup>").is_err());
    }

    /// `<a href>` and `<img src alt>` take the Markdown link and picture
    /// paths: the same URL policy, a link's `<br>` in its text, the same
    /// picture; a link inside a link and a picture inside a picture fail,
    /// and so does either without its target or in a note definition
    /// (whose Markdown links and pictures fail too).
    /// 이것을 실패시키는 것: 링크 안 링크 검사 삭제 · 그림 안 그림 검사 삭제 ·
    /// 정의 안 거부 삭제.
    #[test]
    fn html_link_and_picture_take_the_markdown_paths() {
        let first = |md: &str| {
            MdDecoder::decode_with_default(md).unwrap().document.sections()[0].paragraphs[0]
                .runs
                .clone()
        };
        let link = &first("<a href=\"https://e.invalid/?a=1&amp;b=2\">t<br>u</a>")[0];
        assert!(matches!(&link.content, RunContent::Control(c) if matches!(
            c.as_ref(),
            Control::Hyperlink { text, url } if text == "t\nu" && url == "https://e.invalid/?a=1&b=2"
        )));
        assert_eq!(
            first("<a href=\"javascript:x\">t</a>"),
            first("[t](javascript:x)"),
            "an unsafe URL is text, as in Markdown"
        );
        assert_eq!(first("<img src=\"images/a.png\" alt=\"x\"/>"), first("![x](images/a.png)"));
        for markdown in [
            "<a href=\"https://a.invalid\"><a href=\"https://b.invalid\">t</a></a>",
            "[a <a href=\"https://b.invalid\">b</a>](https://a.invalid)",
            "![a <img src=\"b.png\" alt=\"b\"/>](a.png)",
            "<a>t</a>",
            "<img alt=\"x\"/>",
            "x[^1]\n\n[^1]: <a href=\"https://e.invalid\">t</a>",
            "x[^1]\n\n[^1]: <img src=\"a.png\" alt=\"a\"/>",
        ] {
            assert!(MdDecoder::decode_with_default(markdown).is_err(), "{markdown}");
        }
    }

    /// `<br>` is a line break in body text and in a note definition, as a
    /// Markdown hard break is (a table cell's is in `cell_line_breaks_*`).
    /// 이것을 실패시키는 것: 정의 안 `<br>` 를 본문으로 보내기.
    #[test]
    fn html_line_break_is_a_hard_break() {
        let md = MdDecoder::decode_with_default("a<br>b[^1]\n\n[^1]: c<br>d").unwrap();
        assert_eq!(texts(&md)[0], "a\nb");
        let note = md.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| match &r.content {
                RunContent::Control(c) => match c.as_ref() {
                    Control::Footnote { paragraphs, .. } => Some(paragraphs[0].text_content()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert_eq!(note, "c\nd");
    }

    /// The paragraphs of each cell of the first table run, row by row.
    fn cell_texts(md: &MdDocument) -> Vec<Vec<Vec<String>>> {
        let table = md.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .find_map(|r| match &r.content {
                RunContent::Table(t) => Some(t),
                _ => None,
            })
            .expect("a table");
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|c| c.paragraphs.iter().map(|p| p.text_content()).collect())
                    .collect()
            })
            .collect()
    }

    /// A `<br>` in a cell ends its paragraph, in an HTML cell and a GFM
    /// one alike; leading, repeated and trailing ones keep their empty
    /// paragraphs; in a link it is the link text's line break.
    /// 이것을 실패시키는 것: 칸 `<br>` 를 줄바꿈 글로 · 링크 안 `<br>` 로 문단 나누기 ·
    /// 빈 문단 버리기.
    #[test]
    fn cell_line_breaks_end_paragraphs() {
        let html = MdDecoder::decode_with_default(
            "<table>\n<tbody>\n<tr>\n  <td><br>a<br><br>b<br></td>\n  \
             <td><a href=\"https://e.invalid\">x<br>y</a></td>\n</tr>\n</tbody>\n</table>",
        )
        .unwrap();
        let s = |v: &[&str]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        assert_eq!(cell_texts(&html), [[s(&["", "a", "", "b", ""]), s(&[""])]]);
        let RunContent::Table(table) = &html.document.sections()[0].paragraphs[0].runs[0].content
        else {
            panic!("a table")
        };
        let link = &table.rows[0].cells[1].paragraphs[0].runs[0].content;
        assert!(
            matches!(link, RunContent::Control(c) if matches!(
                c.as_ref(),
                Control::Hyperlink { text, .. } if text == "x\ny"
            )),
            "{link:?}"
        );
        let gfm = MdDecoder::decode_with_default("| a<br>b |\n|---|").unwrap();
        assert_eq!(cell_texts(&gfm), [[s(&["a", "b"])]]);
    }

    /// Text in an HTML cell is HTML text, read once: Markdown in it stays
    /// characters, and an escaped `&#91;^1]` is text, not a note.
    /// 이것을 실패시키는 것: 칸 글을 Markdown 으로 다시 읽기 · 두 번 복원.
    #[test]
    fn html_cell_text_is_not_markdown() {
        let md = MdDecoder::decode_with_default(
            "<table>\n<tbody>\n<tr>\n  <td>**raw** `c` &#91;^1] a|b &amp;lt;</td>\n</tr>\n</tbody>\n</table>",
        )
        .unwrap();
        assert_eq!(cell_texts(&md), [[["**raw** `c` [^1] a|b &lt;".to_string()]]]);
    }

    /// The table grammar is the encoder's; each way out of it fails, and a
    /// span is 1 to 65535 written in digits, once.
    /// 이것을 실패시키는 것: 순서 검사 하나씩 삭제 · span 0·범위 밖 허용 · 칸 서식 닫힘 검사
    /// 삭제 · 블록 끝에 열린 표 받기 · 인라인·정의 안 표 받기.
    #[test]
    fn html_table_grammar_is_the_encoders() {
        let table = |rows: &str| format!("<table>\n<tbody>\n{rows}\n</tbody>\n</table>");
        let ok =
            MdDecoder::decode_with_default(&table("<tr>\n  <td colspan=\"65535\">a</td>\n</tr>"));
        assert!(ok.is_ok(), "{ok:?}");
        for markdown in [
            "<table>\n<tr>\n  <td>a</td>\n</tr>\n</table>".to_string(),
            "<table>\n<tbody>\n<tbody>\n</tbody>\n</tbody>\n</table>".to_string(),
            "<table>\n<tbody>\n</tbody>\n<tbody>\n</tbody>\n</table>".to_string(),
            table("<td>a</td>\n</tr>"),
            table("<tr>\n<tr>\n</tr>\n</tr>"),
            table("<tr>\n  <td><td>a</td></td>\n</tr>"),
            table("x<tr>\n</tr>"),
            table("<tr>\n  <!-- chart -->\n</tr>"),
            table("<br>\n<tr>\n  <td>x</td>\n</tr>"),
            table("<tr>\n  <td><u>a</td>\n  <td>b</u></td>\n</tr>"),
            "<table>\n<tbody>\n<tr>\n  <td>a\n\nb\n\n</td>\n</tr>\n</tbody>\n</table>".to_string(),
            table("<tr>\n  <td colspan=\"0\">a</td>\n</tr>"),
            table("<tr>\n  <td rowspan=\"65536\">a</td>\n</tr>"),
            table("<tr>\n  <td colspan=\"+1\">a</td>\n</tr>"),
            table("<tr>\n  <td colspan=\"1\" colspan=\"2\">a</td>\n</tr>"),
            "a <table>".to_string(),
            "x[^1]\n\n[^1]: a\n\n    <table><tbody><tr><td>a</td></tr></tbody></table>".to_string(),
        ] {
            assert!(MdDecoder::decode_with_default(&markdown).is_err(), "{markdown}");
        }
    }

    /// A GFM table shares the width over its widest row however large it
    /// is; only an HTML table's spans are checked against Core's area
    /// limit before its grid is placed.
    /// 이것을 실패시키는 것: GFM 에도 면적 검사 · HTML 에 면적 검사 삭제.
    #[test]
    fn table_columns_check_the_area_of_html_tables_only() {
        let mut huge = TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::ZERO);
        huge.col_span = 1024;
        huge.row_span = 1025;
        let table = Table::new(vec![TableRow::new(vec![huge])]);
        assert_eq!(table_columns(&table, Some(1024)), Some(1024));
        assert_eq!(table_columns(&table, None), None);
        let small = Table::new(vec![TableRow::new(vec![
            TableCell::new(
                vec![Paragraph::new(ParaShapeIndex::new(0))],
                HwpUnit::ZERO
            );
            3
        ])]);
        assert_eq!(table_columns(&small, None), Some(3));
    }

    /// HTML tables nest in cells up to the shared depth limit: 32 read,
    /// 33 fail; the wrappers `tbody`·`tr`·`td` are not counted.
    /// 이것을 실패시키는 것: 깊이 검사 삭제 · 상한 하나 어긋내기.
    #[test]
    fn html_tables_nest_32_deep() {
        let nested = |depth: usize| {
            let mut md = String::from("x");
            for _ in 0..depth {
                md = format!("<table>\n<tbody>\n<tr>\n  <td>{md}</td>\n</tr>\n</tbody>\n</table>");
            }
            md
        };
        assert!(MdDecoder::decode_with_default(&nested(32)).is_ok());
        assert!(MdDecoder::decode_with_default(&nested(33)).is_err());
    }

    /// A table whose spans cover more than Core's grid limit (20 × 65535
    /// positions) is read without placing its grid: every cell takes the
    /// fallback width, the spans as written.
    /// 이것을 실패시키는 것: 면적 검사 없이 배치.
    #[test]
    fn huge_spans_take_the_fallback_width() {
        let md = MdDecoder::decode_with_default(
            "<table>\n<tbody>\n<tr>\n  <td colspan=\"20\" rowspan=\"65535\">a</td>\n</tr>\n</tbody>\n</table>",
        )
        .unwrap();
        let table = match &md.document.sections()[0].paragraphs[0].runs[0].content {
            RunContent::Table(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        let cell = &table.rows[0].cells[0];
        assert_eq!((cell.col_span, cell.row_span), (20, 65535));
        assert_eq!(cell.width, HwpUnit::from_mm(40.0).unwrap());
    }

    /// A chart comment is left out with one warning each, inline, on its
    /// own line, and at an item's start with the text after it on the
    /// same line (as the encoder writes it, an HTML block), which is then
    /// the item's text.
    /// 이것을 실패시키는 것: chart 경고 삭제 · 블록 줄 끝을 글로 넣기.
    #[test]
    fn chart_comments_are_left_out_and_reported() {
        let md = MdDecoder::decode_with_default(
            "a <!-- chart --> b\n\n<!-- chart -->\n\n- <!-- chart -->- x",
        )
        .unwrap();
        assert_eq!(md.warnings, vec![MdWarning::ContentLost { kind: "chart" }; 3]);
        // The item keeps its marker as text, as every list item does.
        assert_eq!(texts(&md), ["a  b", "- - x"]);
    }

    /// A memo comment is left out with one warning; the text around it
    /// stays.
    /// 이것을 실패시키는 것: memo 경고 삭제.
    #[test]
    fn memo_comments_are_left_out_and_reported() {
        let md = MdDecoder::decode_with_default("a<!-- memo: \\- m -->b").unwrap();
        assert_eq!(md.warnings, vec![MdWarning::ContentLost { kind: "memo" }]);
        assert_eq!(texts(&md), ["ab"]);
    }

    /// Entities are read once: in HTML by the scanner, in Markdown text by
    /// the parser alone, so escaped markup stays text and warns of nothing,
    /// and an escaped `\&lt;` or one in a code block stays `&lt;`.
    /// 이것을 실패시키는 것: `&amp;` 먼저 복원 · Markdown 글에 엔티티 복원을 다시 적용.
    #[test]
    fn entities_are_read_once() {
        let md = MdDecoder::decode_with_default(
            "&amp;lt; &lt;!-- memo: x --&gt; <a href=\"https://e.invalid/?q=&amp;lt;\">t</a>",
        )
        .unwrap();
        assert!(md.warnings.is_empty());
        let runs = &md.document.sections()[0].paragraphs[0].runs;
        assert!(matches!(&runs[0].content, RunContent::Text(t) if t == "&lt; <!-- memo: x --> "));
        assert!(matches!(&runs[1].content, RunContent::Control(c) if matches!(
            c.as_ref(),
            Control::Hyperlink { url, .. } if url == "https://e.invalid/?q=&lt;"
        )));
        let md = MdDecoder::decode_with_default("\\&lt;\n\n```\n&lt;\n```").unwrap();
        assert_eq!(texts(&md), ["&lt;", "&lt;\n"]);
    }

    /// Text in an HTML block keeps the spaces between its content (between
    /// two formats, before a tag) and a line end written `&#10;`; the
    /// block's own line end and the spaces around it are layout.
    /// 이것을 실패시키는 것: 내용 사이 공백 버리기 · 글 끝·글 앞 공백 자르기 ·
    /// `&#10;` 를 줄 끝으로 읽기 · 마지막 내용 뒤 공백 넣기 · 그림 앞 공백 버리기.
    #[test]
    fn html_block_text_keeps_its_spaces() {
        for (markdown, text) in [
            ("<!-- chart --><strong>hello</strong> <em>world</em>\n", "hello world"),
            ("<!-- chart -->hello <em>world</em>", "hello world"),
            ("<!-- chart --><em>hello</em> world", "hello world"),
            ("<u>\n  a&#10;b\n  c <em>d</em>&#10;\n</u>\n", "a\nb c d\n"),
        ] {
            let md = MdDecoder::decode_with_default(markdown).unwrap();
            assert_eq!(texts(&md), [text], "{markdown}");
        }
        // A picture is content: the space before it stays.
        let md = MdDecoder::decode_with_default("<!-- chart -->a <img src=\"p.png\" alt=\"\"/>")
            .unwrap();
        let runs = &md.document.sections()[0].paragraphs[0].runs;
        assert!(matches!(&runs[0].content, RunContent::Text(t) if t == "a "), "{runs:?}");
        assert!(matches!(&runs[1].content, RunContent::Image(_)), "{runs:?}");
    }

    /// An HTML block in a quote is a quote paragraph: the quote's style, the
    /// underline on its own run only.
    /// 이것을 실패시키는 것: 블록 글을 인용 밖 스타일로 넣기.
    #[test]
    fn html_block_in_a_quote_keeps_the_quote_style() {
        let quote = MdDecoder::decode_with_default("> plain").unwrap();
        let body = MdDecoder::decode_with_default("plain").unwrap();
        assert_ne!(
            quote.document.sections()[0].paragraphs[0].style_ref,
            body.document.sections()[0].paragraphs[0].style_ref,
            "the quote style must differ from the body's for this test to see it"
        );
        let md = MdDecoder::decode_with_default("> <!-- chart --><u>a</u> b").unwrap();
        let (p, plain) =
            (&md.document.sections()[0].paragraphs[0], &quote.document.sections()[0].paragraphs[0]);
        assert_eq!((p.para_shape_id, &p.style_ref), (plain.para_shape_id, &plain.style_ref));
        let found: Vec<(String, bool)> = run_shapes(&md)
            .into_iter()
            .map(|(t, c)| (t, c.underline_type == UnderlineType::Bottom))
            .collect();
        assert_eq!(found, [("a".to_string(), true), (" b".to_string(), false)]);
    }

    /// A comment at the head of a note definition comes as an HTML block
    /// inside it (the encoder writes it there): it is read as in the body,
    /// the text after it is the definition's own paragraph.
    /// 이것을 실패시키는 것: 정의 수집 앞에서 HTML 을 가로채지 않기 · 블록 끝에서 정의
    /// 문단을 닫지 않기.
    #[test]
    fn note_definition_reads_a_leading_comment_block() {
        let md =
            MdDecoder::decode_with_default("x[^1]\n\n[^1]: <!-- chart -->remaining\n\n    more")
                .unwrap();
        assert_eq!(md.warnings, vec![MdWarning::ContentLost { kind: "chart" }]);
        let note = md.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| match &r.content {
                RunContent::Control(c) => match c.as_ref() {
                    Control::Footnote { paragraphs, .. } => {
                        Some(paragraphs.iter().map(|p| p.text_content()).collect::<Vec<_>>())
                    }
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert_eq!(note, ["remaining", "more"]);
    }

    /// How pulldown-cmark hands the encoder's HTML to the decoder, pinned
    /// for #281: an inline tag is its own `InlineHtml` event, the text
    /// between tags ordinary events; an HTML table is one `HtmlBlock` with
    /// one `Html` event per physical line, a cell's formatting inside that
    /// line's string; a comment in a paragraph is `InlineHtml`, one on its
    /// own line an `HtmlBlock`.
    /// 이것을 실패시키는 것: 연속한 `Html` 줄 이벤트를 하나로 합쳐 보기.
    #[test]
    fn html_event_shapes_are_pinned() {
        use pulldown_cmark::CowStr;
        let events = |md: &str| -> Vec<Event<'static>> {
            Parser::new_ext(md, crate::encoder::md_options()).map(Event::into_static).collect()
        };
        let html = |s: &str| Event::Html(CowStr::from(s.to_string()));
        let inline = |s: &str| Event::InlineHtml(CowStr::from(s.to_string()));
        let text = |s: &str| Event::Text(CowStr::from(s.to_string()));
        let para = |inner: Vec<Event<'static>>| {
            let mut out = vec![Event::Start(Tag::Paragraph)];
            out.extend(inner);
            out.push(Event::End(TagEnd::Paragraph));
            out
        };
        let block = |lines: &[&str]| {
            let mut out = vec![Event::Start(Tag::HtmlBlock)];
            out.extend(lines.iter().map(|l| html(l)));
            out.push(Event::End(TagEnd::HtmlBlock));
            out
        };
        assert_eq!(
            events("a <u>b</u><br>z<!-- memo: m -->"),
            para(vec![
                text("a "),
                inline("<u>"),
                text("b"),
                inline("</u>"),
                inline("<br>"),
                text("z"),
                inline("<!-- memo: m -->"),
            ])
        );
        assert_eq!(
            events("<table>\n<tbody>\n<tr>\n  <td>**raw** <strong>x</strong></td>\n</tr>\n</tbody>\n</table>"),
            block(&[
                "<table>\n",
                "<tbody>\n",
                "<tr>\n",
                "  <td>**raw** <strong>x</strong></td>\n",
                "</tr>\n",
                "</tbody>\n",
                "</table>",
            ])
        );
        assert_eq!(events("<!-- chart -->"), block(&["<!-- chart -->"]));
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
        // A table tag outside a table is raw HTML too.
        // 이것을 실패시키는 것: 표 밖 표 태그를 서식처럼 받기.
        for markdown in ["<div>raw</div>", "a <td>b</td>"] {
            let err = MdDecoder::decode(markdown, &template).unwrap_err();
            assert!(
                matches!(
                    err,
                    MdError::UnsupportedStructure { ref detail }
                        if detail.contains("raw HTML")
                ),
                "{markdown}"
            );
        }
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

    /// `![x]()` alone or inside text: no picture, one "could not be read"
    /// warning, and the document still validates. A named target is kept
    /// and warns nothing here.
    /// 이것을 실패시키는 것: `end_image_tag` 의 빈 대상 갈래 삭제, 또는 경고를
    /// 남기고 그림도 넣기.
    #[test]
    fn empty_image_target_is_left_out_and_reported() {
        let pictures = |doc: &Document| {
            let mut n = 0;
            doc.for_each_paragraph(|p| {
                n += p.runs.iter().filter(|r| matches!(r.content, RunContent::Image(_))).count();
            });
            n
        };
        for markdown in ["![로고]()\n\n본문\n", "앞 ![로고]() 뒤\n"] {
            let decoded = MdDecoder::decode_with_default(markdown).unwrap();
            assert_eq!(pictures(&decoded.document), 0, "{markdown:?}");
            assert_eq!(
                decoded.warnings,
                [MdWarning::ImageEmbedSkipped {
                    src: String::new(),
                    reason: crate::embed::ImageEmbedSkipReason::Unreadable,
                }],
                "{markdown:?}"
            );
            assert_eq!(
                decoded.warnings[0].to_string(),
                "image \"\" not embedded (file unreadable) — image dropped"
            );
            decoded.document.validate().unwrap();
        }
        let named = MdDecoder::decode_with_default("![로고](a.png)\n").unwrap();
        assert_eq!(pictures(&named.document), 1);
        assert!(named.warnings.is_empty());
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
    fn h1_heading_uses_selected_entry() {
        let template = default_template();
        let result = MdDecoder::decode("# 제목", &template).unwrap();
        let section = &result.document.sections()[0];
        assert_eq!(
            section.paragraphs[0].style_ref,
            Some(result.style_registry.get_style("heading1").unwrap().style_ref())
        );
    }

    #[test]
    fn all_heading_levels_use_selected_entries() {
        let template = default_template();
        for level in 1u8..=6 {
            let md = format!("{} 제목{level}", "#".repeat(level as usize));
            let result = MdDecoder::decode(&md, &template).unwrap();
            let section = &result.document.sections()[0];
            assert_eq!(
                section.paragraphs[0].style_ref,
                Some(
                    result
                        .style_registry
                        .get_style(&format!("heading{level}"))
                        .unwrap()
                        .style_ref()
                ),
                "H{level} should map to style_ref {}",
                (level as usize) + 1
            );
        }
    }

    #[test]
    fn body_paragraph_uses_selected_entry() {
        let template = default_template();
        let result = MdDecoder::decode("본문입니다.", &template).unwrap();
        let section = &result.document.sections()[0];
        assert_eq!(
            section.paragraphs[0].style_ref,
            Some(result.style_registry.get_style("body").unwrap().style_ref())
        );
    }

    // -----------------------------------------------------------------------
    // inline formatting stack (bold/italic/strikethrough)
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
    fn bold_run_derives_char_shape() {
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
    fn italic_run_derives_char_shape() {
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
    fn strikethrough_run_derives_char_shape() {
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
    fn nested_bold_italic_combines() {
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
    fn same_format_combo_reuses_derived_shape() {
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
    fn unformatted_text_adds_no_derived_shapes() {
        let template = default_template();
        let before =
            MdDecoder::decode("plain", &template).unwrap().style_registry.char_shape_count();
        let after =
            MdDecoder::decode("plain again", &template).unwrap().style_registry.char_shape_count();
        assert_eq!(before, after, "no formatting => registry untouched");
    }

    #[test]
    fn formatting_inside_heading_derives_from_heading_base() {
        let template = default_template();
        let (mapping, _) = resolve_mapping(&template).unwrap();
        let result = MdDecoder::decode("# head **bold**", &template).unwrap();
        let para = &result.document.sections()[0].paragraphs[0];
        assert_eq!(para.runs[0].char_shape_id, mapping.heading1.char_shape_id);
        let derived = result.style_registry.char_shape(para.runs[1].char_shape_id).unwrap();
        assert!(derived.bold);
    }

    // -----------------------------------------------------------------------
    // footnote / endnote decoding
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
    fn basic_footnote_resolves_at_reference_site() {
        let template = default_template();
        let result = MdDecoder::decode("본문[^1] 끝\n\n[^1]: 각주 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0], (false, vec!["각주 본문".to_string()]));
        // 참조 지점: 본문 문단 안 inline control
        let para = &result.document.sections()[0].paragraphs[0];
        assert!(para.text_content().contains("본문"));
        assert!(para
            .runs
            .iter()
            .any(|r| matches!(&r.content, RunContent::Control(c) if c.is_footnote())));
    }

    #[test]
    fn undefined_reference_stays_literal_text() {
        // 파서 동작: 정의 없는 참조는 이벤트가 아니라 평문으로 강등된다.
        let template = default_template();
        let result = MdDecoder::decode("본문[^nodef] 끝\n", &template).unwrap();
        let text = result.document.sections()[0].paragraphs[0].text_content();
        assert!(text.contains("[^nodef]"), "literal must be preserved: {text}");
        assert!(collect_notes(&result.document).is_empty());
    }

    #[test]
    fn orphan_definition_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문뿐\n\n[^orphan]: 아무도 안 씀\n", &template).unwrap_err();
        assert!(
            matches!(&err, MdError::OrphanNoteDefinition { label } if label == "orphan"),
            "got: {err:?}"
        );
    }

    #[test]
    fn duplicate_definition_is_typed_error() {
        let template = default_template();
        let err =
            MdDecoder::decode("본문[^1]\n\n[^1]: 첫째\n\n[^1]: 둘째\n", &template).unwrap_err();
        assert!(matches!(&err, MdError::DuplicateNoteDefinition { label } if label == "1"));
    }

    #[test]
    fn empty_definition_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문[^1]\n\n[^1]:\n", &template).unwrap_err();
        assert!(matches!(&err, MdError::EmptyNoteDefinition { label } if label == "1"));
    }

    #[test]
    fn nested_reference_is_typed_error() {
        let template = default_template();
        let err = MdDecoder::decode("본문[^1]\n\n[^1]: 안에서[^2] 참조\n\n[^2]: 둘째\n", &template)
            .unwrap_err();
        assert!(matches!(&err, MdError::NestedNoteReference { label } if label == "2"));
    }

    #[test]
    fn block_content_in_definition_is_rejected() {
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
    fn definition_before_reference_is_fine() {
        let template = default_template();
        let result = MdDecoder::decode("[^1]: 본문 먼저\n\n참조[^1]\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes, vec![(false, vec!["본문 먼저".to_string()])]);
    }

    #[test]
    fn e_prefix_label_is_endnote_others_footnote() {
        let template = default_template();
        let md = "가[^e1] 나[^1] 다[^note] 라[^example]\n\n[^e1]: 미주\n\n[^1]: 각주하나\n\n[^note]: 각주둘\n\n[^example]: 각주셋\n";
        let result = MdDecoder::decode(md, &template).unwrap();
        let kinds: Vec<bool> = collect_notes(&result.document).iter().map(|(e, _)| *e).collect();
        assert_eq!(kinds, vec![true, false, false, false], "only e<digits> is an endnote");
    }

    #[test]
    fn same_label_multi_reference_duplicates_body() {
        let template = default_template();
        let result = MdDecoder::decode("가[^1] 나[^1]\n\n[^1]: 공유 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes.len(), 2, "each reference gets its own note control");
        assert_eq!(notes[0].1, notes[1].1);
    }

    #[test]
    fn table_cell_and_body_references_resolve_in_source_order() {
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
    fn multiparagraph_definition_keeps_paragraphs() {
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
    fn definition_body_keeps_inline_formatting_runs() {
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
    fn mixed_notes_preserve_order_and_kind() {
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
    fn case_folded_labels_link_and_classify_together() {
        // 파서는 라벨을 UniCase 로 연결한다 — 분류·매칭도 동일 기준.
        let template = default_template();
        let result = MdDecoder::decode("본문[^E1]\n\n[^e1]: 미주 본문\n", &template).unwrap();
        let notes = collect_notes(&result.document);
        assert_eq!(notes, vec![(true, vec!["미주 본문".to_string()])]);
    }

    #[test]
    fn note_body_style_uses_footnote_style_slot() {
        // 본문 스타일 재사용이 아니라 기존 "각주"(14)/"미주"(15) 스타일 참조.
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
    fn expansion_budget_rejects_pathological_duplication() {
        let template = default_template();
        // 큰 본문 1개를 다수 참조 — 전개 총량이 예산을 넘으면 typed 에러.
        let big = "가".repeat(600_000);
        let refs: String = (0..20).map(|_| "x[^1] ").collect();
        let md = format!("{refs}\n\n[^1]: {big}\n");
        let err = MdDecoder::decode(&md, &template).unwrap_err();
        assert!(matches!(&err, MdError::NoteExpansionBudgetExceeded { .. }), "got: {err:?}");
    }
}
