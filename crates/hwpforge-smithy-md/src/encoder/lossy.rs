//! Readable (lossy) markdown encoder.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::template::Template;
use hwpforge_core::{
    Control, Document, ListItemKind, Paragraph, ParagraphListRef, RunContent, Table, Validated,
};

use super::list_format::ListIndent;
use super::markup::{Cell, Md, Piece};
use super::Part;
use crate::error::MdResult;
use crate::frontmatter::{from_metadata, render_frontmatter};
use crate::mapper::{resolve_mapping, MdMapping, ParagraphKind};

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
    let mut warnings = Vec::new();
    let body = encode_body(document, None, None, &mut warnings);
    Ok((body, warnings))
}

fn encode_body(
    document: &Document<Validated>,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
    warnings: &mut Vec<super::MdWarning>,
) -> String {
    use super::guard::{Block, Source};
    let mut destinations = std::collections::HashSet::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut continuation_level: Option<u8> = None;
    let mut lists = ListIndent::default();

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(Block {
                markdown: super::guard::SECTION_MARKER.to_string(),
                source: None,
                body: false,
                tight: false,
            });
            continuation_level = None;
            lists.reset();
        }

        for (paragraph_index, paragraph) in section.paragraphs.iter().enumerate() {
            let current_continuation_level = mapping
                .and_then(|mapping| mapping.continuation_level(paragraph.para_shape_id))
                .filter(|level| continuation_level == Some(*level));
            // A list item's level, from the paragraph's shape.
            let item_level = registry
                .and_then(|registry| format_registry_list_level(paragraph, registry))
                .or_else(|| mapping.and_then(|mapping| mapping_list_level(paragraph, mapping)));
            let first_new = warnings.len();
            let markdown = encode_paragraph(
                paragraph,
                mapping,
                registry,
                warnings,
                &mut destinations,
                &mut lists,
            );
            // Place the depth warnings this paragraph raised.
            // 이것을 실패시키는 것: 이 자리 채우기 삭제(`nested_tables_stay_small`).
            for warning in &mut warnings[first_new..] {
                if let super::MdWarning::ContainerTooDeep { section, paragraph, .. } = warning {
                    *section = section_index;
                    *paragraph = paragraph_index;
                }
            }
            // An empty item is written as its marker alone.
            // 이것을 실패시키는 것: 빈 항목 쓰지 않기(`lossy_empty_item_holds_its_first_block`).
            let markdown = match item_level {
                Some(_) if markdown.trim().is_empty() => registry
                    .and_then(|registry| {
                        format_registry_list_item("", paragraph, registry, &mut lists)
                    })
                    .unwrap_or_else(|| lists.mapped_item("")),
                _ => markdown,
            };
            if !markdown.trim().is_empty() {
                let text = fallback_text(paragraph, &mut destinations);
                let at = (section_index, paragraph_index);
                match (current_continuation_level, blocks.last_mut()) {
                    (Some(_), Some(last)) => {
                        let laid = lists.continuation(&markdown);
                        last.markdown.push_str(if lists.take_tight() { "\n" } else { "\n\n" });
                        last.markdown.push_str(&laid);
                        if let Some(src) = &mut last.source {
                            src.texts.push(text);
                            src.at.push(at);
                        }
                    }
                    (level, _) => {
                        let markdown = match level {
                            Some(_) => lists.continuation(&markdown),
                            None => {
                                if item_level.is_none() {
                                    lists.reset();
                                }
                                markdown
                            }
                        };
                        let source =
                            Source { prefix: String::new(), texts: vec![text], at: vec![at] };
                        let tight = lists.take_tight();
                        blocks.push(Block { markdown, source: Some(source), body: false, tight });
                    }
                }
            }

            if current_continuation_level.is_none() {
                continuation_level = item_level;
            }
        }
    }

    // The finished Markdown, parsed again, may hold only what lossy writes.
    let allowed = super::guard::Allowed {
        tags: super::guard::LOSSY_TAGS,
        destinations: &destinations,
        decoder_structure: false,
    };
    super::guard::checked(blocks, &allowed, warnings)
}

/// A paragraph's plain-text fallback in the lossy view: its text escaped,
/// its pictures and allowed links kept as lossy writes them ([`Kept`]).
/// 이것을 실패시키는 것: 그림·링크를 글로 쓰기(`lossy_fallback_keeps_pictures_and_links`).
fn fallback_text(
    paragraph: &Paragraph,
    destinations: &mut std::collections::HashSet<String>,
) -> String {
    let mut keeper = Kept { destinations };
    super::guard::fallback_markdown(std::slice::from_ref(paragraph), 0, false, &mut keeper)
        .remove(0)
        .into_string()
}

/// The lossy view's [`super::guard::Keeper`]: pictures and allowed links
/// as lossy writes them; notes are inline text in lossy.
struct Kept<'a> {
    destinations: &'a mut std::collections::HashSet<String>,
}

impl super::guard::Keeper for Kept<'_> {
    fn picture(&mut self, image: &hwpforge_core::image::Image) -> Option<Md> {
        (!image.path.is_empty()).then(|| {
            let alt = Md::lossy_label(&image_alt_text(&image.path));
            Md::image(alt, &image.path, self.destinations)
        })
    }

    fn link(&mut self, text: &str, url: &str) -> Option<Md> {
        super::link_url_allowed(url)
            .then(|| Md::link(Md::lossy_label(text), url, self.destinations))
    }

    fn note(&mut self, _: &hwpforge_core::Control) -> Option<Md> {
        None
    }
}

fn encode_paragraph(
    paragraph: &Paragraph,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    lists: &mut ListIndent,
) -> String {
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(_) | RunContent::Image(_) => {
                // Standalone objects keep their existing style exemption, but
                // use the same piece writer as mixed paragraphs and containers.
                // Written at column 0 unless a continuation indents it, it
                // ends the list (`ListIndent`).
                // 이것을 실패시키는 것: 닫기 삭제(`lossy_blocks_in_item_paragraph`).
                if !is_continuation(paragraph, mapping) {
                    lists.reset();
                }
                return paragraph_text_markdown(paragraph, warnings, destinations)
                    .as_str()
                    .trim_matches('\n')
                    .to_string();
            }
            _ => {}
        }
    }

    // Keep the existing raw-text code projection and unstyled paragraph path.
    if mapping.is_none()
        || mapping
            .is_some_and(|m| m.classify_para_shape(paragraph.para_shape_id) == ParagraphKind::Code)
    {
        return format_paragraph_text(
            paragraph_text_markdown(paragraph, warnings, destinations).into_string(),
            paragraph,
            mapping,
            registry,
            lists,
        );
    }

    let pieces = paragraph_pieces_at::<Md>(paragraph, warnings, destinations, 0);
    let mut out = Vec::new();
    let mut inline = Vec::new();
    // A block piece is written at column 0, after the item text before it,
    // and ends the list there; text after it starts again (`ListIndent`). A
    // continuation's blocks are indented with it, inside its item.
    // 이것을 실패시키는 것: 블록 조각에서 닫기 삭제(`lossy_blocks_in_item_paragraph`).
    let continuation = is_continuation(paragraph, mapping);
    // A list item's or a continuation's text makes no structure it did not
    // mean and does not close its item, a template item's first line too.
    // 이것을 실패시키는 것: 보호 삭제(`lossy_item_text_stays_in_its_item`).
    let list = continuation
        || registry.and_then(|registry| format_registry_list_level(paragraph, registry)).is_some()
        || mapping.and_then(|m| mapping_list_level(paragraph, m)).is_some();
    let flush_inline =
        |inline: &mut Vec<Piece<Md>>, out: &mut Vec<Piece<Md>>, lists: &mut ListIndent| {
            let pieces = std::mem::take(inline);
            let text =
                if list { Md::lossy_list_paragraph(pieces) } else { Md::from_pieces(pieces) };
            if !text.as_str().trim().is_empty() {
                out.push(Piece::Markup(text.layout(|text| {
                    format_paragraph_text(text, paragraph, mapping, registry, lists)
                })));
            }
        };
    for piece in pieces {
        match piece {
            // Preserve block identity until the common piece writer joins it
            // to the formatted inline pieces; do not infer it from whitespace.
            // 이것을 실패시키는 것: 블록을 Markup으로 낮춰 캡션 뒤 접두사 경계 우회.
            Piece::Block(block) if !block.as_str().trim().is_empty() => {
                flush_inline(&mut inline, &mut out, lists);
                out.push(Piece::Block(block));
                if !continuation {
                    lists.reset();
                }
            }
            Piece::Block(_) => {}
            piece => inline.push(piece),
        }
    }
    flush_inline(&mut inline, &mut out, lists);
    Md::from_pieces(out).into_string()
}

fn format_paragraph_text(
    text: String,
    paragraph: &Paragraph,
    mapping: Option<&MdMapping>,
    registry: Option<&StyleRegistry>,
    lists: &mut ListIndent,
) -> String {
    let trimmed = text.trim();

    if !trimmed.is_empty() {
        if let Some(markdown) = registry
            .and_then(|registry| format_registry_list_item(trimmed, paragraph, registry, lists))
        {
            return markdown;
        }
    }

    if let Some(hwpforge_core::ParagraphListRef::Outline { level }) =
        registry.and_then(|r| r.para_shape(paragraph.para_shape_id)).and_then(|p| p.list)
    {
        return format!(
            "{} {}",
            "#".repeat(usize::from(hwpforge_core::outline::outline_heading_depth(level))),
            trimmed
        );
    }
    if let Some(mapping) = mapping {
        match mapping.classify_para_shape(paragraph.para_shape_id) {
            ParagraphKind::Heading(level) => {
                format!("{} {}", "#".repeat(level as usize), trimmed)
            }
            // The paragraph's own text in a fence no line of it can close.
            ParagraphKind::Code => Md::code_block(&raw_text(paragraph)).into_string(),
            ParagraphKind::BlockQuote => {
                text.lines().map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n")
            }
            ParagraphKind::ListItem => lists.mapped_item(text.trim_start()),
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
    lists: &mut ListIndent,
) -> Option<String> {
    let list = registry.para_shape(paragraph.para_shape_id)?.list?;
    match list {
        ParagraphListRef::Outline { .. } => None,
        ParagraphListRef::Number { level, .. } => {
            Some(lists.item(text, ListItemKind::Number, level, None))
        }
        ParagraphListRef::Bullet { level, .. } => {
            Some(lists.item(text, ListItemKind::Bullet, level, None))
        }
        ParagraphListRef::CheckBullet { level, checked, .. } => {
            Some(lists.item(text, ListItemKind::Bullet, level, Some(checked)))
        }
    }
}

/// Whether the template mapping draws `paragraph` as a list continuation.
fn is_continuation(paragraph: &Paragraph, mapping: Option<&MdMapping>) -> bool {
    mapping.is_some_and(|m| m.continuation_level(paragraph.para_shape_id).is_some())
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

fn paragraph_text_markdown(
    paragraph: &Paragraph,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
) -> Md {
    paragraph_text_at(paragraph, warnings, destinations, 0)
}

/// A paragraph's text runs as they are (a code block's content).
fn raw_text(paragraph: &Paragraph) -> String {
    paragraph.runs.iter().filter_map(|r| r.content.plain_text()).collect()
}

/// Where lossy markup is written: Markdown ([`Md`], outside tables) or a
/// GFM cell ([`Cell`], inside one). Document text goes in only through a
/// paragraph's pieces ([`Sink::from_pieces`]), escaped once for the place
/// it lands.
trait Sink: Default + Sized {
    /// Tables open blocks only outside a GFM cell.
    const BLOCK_TABLES: bool;
    /// The paragraph written from its pieces.
    fn from_pieces(pieces: Vec<Piece<Self>>) -> Self;
    /// Markup the encoder made, for this place.
    fn from_md(md: Md) -> Self;
    /// Markup already written for this place, appended.
    fn append(&mut self, other: Self);
    /// The markup written (blank checks).
    fn as_str(&self) -> &str;
    /// A table of `rows` written for this place.
    fn table(rows: Vec<Vec<Cell>>, width: usize) -> Self;
    /// Content-preserving fallback without a rectangular grid.
    fn lines(rows: Vec<Vec<Cell>>, notice: &str) -> Self;
}

impl Sink for Md {
    const BLOCK_TABLES: bool = true;

    fn from_pieces(pieces: Vec<Piece<Self>>) -> Self {
        Md::lossy_paragraph(pieces)
    }
    fn from_md(md: Md) -> Self {
        md
    }
    fn append(&mut self, other: Self) {
        self.push(other);
    }
    fn as_str(&self) -> &str {
        Md::as_str(self)
    }
    fn table(rows: Vec<Vec<Cell>>, width: usize) -> Self {
        Cell::table(rows, width)
    }
    fn lines(rows: Vec<Vec<Cell>>, notice: &str) -> Self {
        Cell::lines(rows, notice)
    }
}

impl Sink for Cell {
    const BLOCK_TABLES: bool = false;

    fn from_pieces(pieces: Vec<Piece<Self>>) -> Self {
        Cell::lossy_paragraph(pieces)
    }
    fn from_md(md: Md) -> Self {
        md.into_cell()
    }
    fn append(&mut self, other: Self) {
        self.push(other);
    }
    fn as_str(&self) -> &str {
        Cell::as_str(self)
    }
    fn table(rows: Vec<Vec<Cell>>, width: usize) -> Self {
        Cell::nested_table(rows, width)
    }
    fn lines(rows: Vec<Vec<Cell>>, notice: &str) -> Self {
        Cell::cell_lines(rows, notice)
    }
}

/// How many tables a lossy table may sit inside; past this depth the inner
/// table is replaced by [`TABLE_TOO_DEEP`]. No corpus document reaches it.
/// The output stays linear in the depth below it: text in a cell is escaped
/// once, where it is written, and a table in a cell is drawn cell-safe
/// (`\|`, `<br>`) instead of being escaped again by every enclosing cell.
const MAX_TABLE_NESTING: usize = 8;

/// Bound both serialized padding and the GFM parser's implicit empty-cell
/// allocation by actual input cells, even well below the absolute grid cap.
const MAX_TABLE_GRID_EXPANSION: u64 = 16;

/// Written in place of a table nested deeper than [`MAX_TABLE_NESTING`].
const TABLE_TOO_DEEP: &str = "(중첩 표 생략)";

/// [`paragraph_text_markdown`] inside `depth` tables, written for `S` —
/// a cell's markup when `depth > 0`.
///
/// The paragraph is gathered as pieces — document text and the encoder's
/// markup — and written in one pass ([`Sink::from_pieces`]), so a sequence
/// split across runs (`[a]` then `(javascript:…)`) or around markup
/// (`<img title='` · link · `' onerror…>`) is judged whole.
fn paragraph_text_at<S: Sink>(
    paragraph: &Paragraph,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    depth: usize,
) -> S {
    S::from_pieces(paragraph_pieces_at(paragraph, warnings, destinations, depth))
}

fn paragraph_pieces_at<S: Sink>(
    paragraph: &Paragraph,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    depth: usize,
) -> Vec<Piece<S>> {
    let paragraph = hwpforge_core::field_view::expand_memos(paragraph);
    let mut pieces: Vec<Piece<S>> = Vec::new();
    let text = |pieces: &mut Vec<Piece<S>>, t: &str| match pieces.last_mut() {
        Some(Piece::Text(last)) => last.push_str(t),
        _ => pieces.push(Piece::Text(t.to_string())),
    };
    let written = |pieces: &[Piece<S>]| {
        pieces.iter().any(|p| match p {
            Piece::Text(t) => !t.is_empty(),
            Piece::Markup(m) | Piece::Block(m) => !m.as_str().is_empty(),
        })
    };

    // Every readable container uses this traversal, including groups inside
    // shapes and notes. The document preflight bounds total container depth;
    // `depth` separately tracks the table budget, even through shapes/notes.
    for (part, _) in super::spread_groups(&paragraph.runs) {
        match part {
            // Markdown cannot represent inline tab attributes, so any
            // `<hp:tab width=... leader=... type=.../>` collapses back
            // to a literal `\t` via `plain_text()` — same effective
            // output the `Text(String)` arm would produce.
            Part::Text(content) => {
                if let Some(cow) = content.plain_text() {
                    text(&mut pieces, &cow);
                }
            }
            Part::Image(image) => {
                // 이것을 실패시키는 것: 빈 경로 가드 삭제(`![]()` 방출).
                if !image.path.is_empty() {
                    if written(&pieces) {
                        pieces.push(Piece::Markup(S::from_md(Md::syntax(" "))));
                    }
                    let alt = Md::lossy_label(&image_alt_text(&image.path));
                    pieces.push(Piece::Markup(S::from_md(Md::image(
                        alt,
                        &image.path,
                        destinations,
                    ))));
                }
                pieces.push(Piece::Block(lossy_caption(
                    image.caption.as_ref(),
                    warnings,
                    destinations,
                    depth,
                )));
            }
            Part::Control(control) => match control {
                // Only an allowed URL is written as a link (the text otherwise).
                // Its label escapes `\`, `[` and `]`, so it cannot close the
                // link early and start another (`a](javascript:…)[b`), and its
                // destination reads back as the URL.
                Control::Hyperlink { text: label, url } => {
                    if super::link_url_allowed(url) {
                        let link = Md::link(Md::lossy_label(label), url, destinations);
                        pieces.push(Piece::Markup(S::from_md(link)));
                    } else {
                        text(&mut pieces, label);
                    }
                }
                Control::Footnote { paragraphs, .. }
                | Control::Endnote { paragraphs, .. }
                | Control::TextBox { paragraphs, .. }
                | Control::Ellipse { paragraphs, .. }
                | Control::Polygon { paragraphs, .. } => {
                    let prefix = match control {
                        Control::Footnote { .. } => Some("(footnote: "),
                        Control::Endnote { .. } => Some("(endnote: "),
                        _ => None,
                    };
                    if let Some(prefix) = prefix {
                        text(&mut pieces, prefix);
                    }
                    pieces.extend(container_pieces_at(paragraphs, warnings, destinations, depth));
                    if prefix.is_some() {
                        text(&mut pieces, ")");
                    }
                }
                Control::Unknown { tag, .. } => {
                    let code = Md::code_span(&format!("[{tag}]"));
                    pieces.push(Piece::Markup(S::from_md(code)));
                }
                Control::Line { .. } => {
                    // Lines have no text content to render in lossy mode
                }
                Control::Dutmal { main_text, sub_text, .. } => {
                    text(&mut pieces, &format!("{main_text}({sub_text})"));
                }
                Control::Compose { compose_text, .. } => {
                    text(&mut pieces, compose_text);
                }
                Control::NewNumber { .. } | Control::PageHiding { .. } => {
                    // 번호 재시작·감추기 지시 — 본문 텍스트 없음, 명시적
                    // no-op.
                }
                Control::AutoNumber { num, format, .. } => {
                    text(&mut pieces, &super::auto_number_text(*num, format));
                }
                other => {
                    if let Some(value) = hwpforge_core::field_view::scalar_text(other) {
                        text(&mut pieces, value);
                    }
                }
            },
            Part::Table(table) => {
                // The common piece writer owns Markdown block boundaries.
                // Cells retain their existing inline nested-table separator.
                let mut block = S::default();
                if !S::BLOCK_TABLES && written(&pieces) {
                    block.append(S::from_md(Md::syntax("\n")));
                }
                block.append(table_to_markdown(table, warnings, destinations, depth));
                pieces.push(Piece::Block(block));
                pieces.push(Piece::Block(lossy_caption(
                    table.caption.as_ref(),
                    warnings,
                    destinations,
                    depth,
                )));
            }
        }
    }
    pieces
}

/// Keep child text as pieces until its enclosing paragraph escapes it once.
/// This also checks markup split across container boundaries as one paragraph.
/// 이것을 실패시키는 것: 얕은 text_content 복귀, 표 깊이 초기화, 재escape.
fn container_pieces_at<S: Sink>(
    paragraphs: &[Paragraph],
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    depth: usize,
) -> Vec<Piece<S>> {
    let mut pieces = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if index > 0 {
            pieces.push(Piece::Text(" ".into()));
        }
        pieces.extend(paragraph_pieces_at::<S>(paragraph, warnings, destinations, depth));
    }
    // Preserve the old shape/note body's trim, including whitespace-only boxes.
    for piece in &mut pieces {
        match piece {
            Piece::Text(text) => {
                *text = text.trim_start().to_string();
                if !text.is_empty() {
                    break;
                }
            }
            Piece::Markup(_) | Piece::Block(_) => break,
        }
    }
    for piece in pieces.iter_mut().rev() {
        match piece {
            Piece::Text(text) => {
                *text = text.trim_end().to_string();
                if !text.is_empty() {
                    break;
                }
            }
            Piece::Markup(_) | Piece::Block(_) => break,
        }
    }
    pieces
}

/// A picture's or table's caption as a paragraph after it, empty without
/// one — the caption numbers ("그림 3") live there.
/// In a cell the paragraph break is `<br><br>` (the cell's line endings).
/// 이것을 실패시키는 것: 캡션 출력을 지우기
/// (`f17_caption_numbers_reach_the_readable_views`).
fn lossy_caption<S: Sink>(
    caption: Option<&hwpforge_core::caption::Caption>,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    depth: usize,
) -> S {
    let mut pieces = Vec::new();
    let Some(caption) = caption else { return S::default() };
    for p in &caption.paragraphs {
        let text: S = paragraph_text_at(p, warnings, destinations, depth);
        if !text.as_str().trim().is_empty() {
            if !S::BLOCK_TABLES {
                pieces.push(Piece::Markup(S::from_md(Md::syntax("\n\n"))));
            }
            pieces.push(Piece::Block(text));
        }
    }
    S::from_pieces(pieces)
}

/// A table inside `depth` tables, written for `S`: Markdown at the top, a
/// cell's markup (`\|` bars, `<br>` between lines) inside a table — its
/// cells' text escaped once, where it is written, so the output grows
/// linearly with the depth. 이것을 실패시키는 것: 깊이 한도 삭제 · 칸에서 깊이
/// 증가 삭제 · 칸 글자를 다시 escape 하기(`nested_tables_stay_small`,
/// `nested_table_text_is_escaped_once`).
fn table_to_markdown<S: Sink>(
    table: &Table,
    warnings: &mut Vec<super::MdWarning>,
    destinations: &mut std::collections::HashSet<String>,
    depth: usize,
) -> S {
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
        return S::from_md(Md::syntax(TABLE_TOO_DEEP));
    }
    if table.rows.is_empty() {
        return S::table(vec![vec![]], 1);
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

    let rows: Vec<Vec<Cell>> = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    let mut text = Cell::new();
                    for (i, p) in cell.paragraphs.iter().enumerate() {
                        if i > 0 {
                            text.push_md(Md::syntax(" "));
                        }
                        text.push(paragraph_text_at(p, warnings, destinations, depth + 1));
                    }
                    text
                })
                .collect()
        })
        .collect();
    // GFM always uses the first physical row as its header; row/cell title
    // markers do not change this existing readable projection.
    // 이것을 실패시키는 것: 첫 행 너비로 복귀하거나 헤더 너비 패딩 생략.
    let width = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    render_table::<S>(rows, width, crate::decoder::MAX_MD_FILE_SIZE)
}

fn render_table<S: Sink>(rows: Vec<Vec<Cell>>, width: usize, byte_limit: u64) -> S {
    // Check before materializing padding or asking the GFM parser to expand
    // short rows. Byte count bounds characters too, including nested syntax.
    // 이것을 실패시키는 것: 격자/출력 바이트 한도 중 하나를 생략.
    let positions = (rows.len() as u64).saturating_mul(width as u64);
    let cells = rows.iter().fold(0_u64, |count, row| count.saturating_add(row.len() as u64));
    let bytes = table_bytes(&rows, width, !S::BLOCK_TABLES);
    if positions > hwpforge_core::table::grid::MAX_GRID_POSITIONS
        || positions > cells.saturating_mul(MAX_TABLE_GRID_EXPANSION)
        || bytes > byte_limit
    {
        let notice = format!(
            "(표 출력 한도 초과: 격자 {positions}칸, 실제 셀 {cells}개, 출력 {bytes}바이트; \
             한도 {}칸 / 셀당 {MAX_TABLE_GRID_EXPANSION}칸 / {byte_limit}바이트. \
             셀별 내용 표시)",
            hwpforge_core::table::grid::MAX_GRID_POSITIONS,
        );
        // Preserve source order, closing each cell's inline syntax at its
        // source and using separate paragraphs when outside a GFM cell.
        return S::lines(rows, &notice);
    }
    S::table(rows, width)
}

/// Exact serialized byte count without constructing padding or the table.
fn table_bytes(rows: &[Vec<Cell>], width: usize, nested: bool) -> u64 {
    let bar = if nested { 2_u64 } else { 1 };
    let newline = if nested { 4_u64 } else { 1 };
    let mut bytes = (width as u64).saturating_mul(5 + bar) + bar;
    for row in rows {
        let columns = width;
        bytes = bytes.saturating_add(newline + bar);
        bytes = bytes.saturating_add((columns as u64).saturating_mul(2 + bar));
        for cell in row {
            bytes = bytes.saturating_add(cell.as_str().len() as u64);
        }
    }
    bytes
}

fn image_alt_text(path: &str) -> String {
    path.rsplit('/').next().and_then(|name| name.split('.').next()).unwrap_or("image").to_string()
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

    fn sparse_table(n: usize) -> Table {
        let cell = || {
            TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("KEEP", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                hwpforge_foundation::HwpUnit::new(1000).unwrap(),
            )
        };
        Table::new(
            (0..n)
                .map(|row| {
                    TableRow::new((0..if row == 1 { n } else { 1 }).map(|_| cell()).collect())
                })
                .collect(),
        )
    }

    /// 이것을 실패시키는 것: 격자 팽창률 제한 삭제 또는 파싱 전 셀 내용 손실.
    #[test]
    fn sparse_tables_keep_gfm_cells_with_linear_output() {
        let mut lengths = Vec::new();
        for n in [8, 16, 32, 64] {
            let table = sparse_table(n);
            let (md, warnings) =
                encode_without_template_report(&validated_document(vec![Paragraph::with_runs(
                    vec![Run::table(table, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]))
                .unwrap();
            if n <= 16 {
                assert!(warnings.is_empty(), "{warnings:?}");
                let blocks = gfm_blocks(&md);
                let GfmBlock::Table(rows) = &blocks[0] else { panic!("{blocks:?}") };
                assert_eq!(rows.len(), n);
                assert!(rows.iter().all(|r| r.len() == n));
                assert_eq!(
                    rows.iter().flatten().filter(|s| s.as_str() == "KEEP").count(),
                    2 * n - 1
                );
            } else {
                assert!(warnings.is_empty());
                assert!(md.contains("표 출력 한도 초과"));
                let text: String =
                    pulldown_cmark::Parser::new_ext(&md, pulldown_cmark::Options::ENABLE_TABLES)
                        .filter_map(|event| match event {
                            pulldown_cmark::Event::Text(text) => Some(text.into_string()),
                            _ => None,
                        })
                        .collect();
                assert_eq!(text.matches("KEEP").count(), 2 * n - 1);
                assert!(md.len() < (2 * n - 1) * 16, "{}", md.len());
                lengths.push(md.len());
            }
        }
        assert!(lengths[1] < lengths[0] * 3);
    }

    /// 이것을 실패시키는 것: 공용 격자 한도/위치 경고 생략, 셀 fallback 재escape.
    #[test]
    fn oversized_table_keeps_every_cell_in_body_container_and_cell() {
        use hwpforge_foundation::HwpUnit;
        let p = |run| Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
        for host in 0..3 {
            let table = Run::table(sparse_table(1025), CharShapeIndex::new(0));
            let run = match host {
                0 => table,
                1 => Run::control(
                    Control::text_box(
                        vec![p(table)],
                        HwpUnit::new(1000).unwrap(),
                        HwpUnit::new(1000).unwrap(),
                    ),
                    CharShapeIndex::new(0),
                ),
                _ => Run::table(
                    Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![p(table)],
                        HwpUnit::new(1000).unwrap(),
                    )])]),
                    CharShapeIndex::new(0),
                ),
            };
            let (md, warnings) = encode_without_template_report(&validated_document(vec![
                p(Run::text("LEAD", CharShapeIndex::new(0))),
                p(run),
            ]))
            .unwrap();
            assert!(md.len() < 25_000, "{}", md.len());
            assert_eq!(md.matches("KEEP").count(), 2049);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(md.contains("격자 1050625칸, 실제 셀 2049개"), "{md}");
            let text: String =
                pulldown_cmark::Parser::new_ext(&md, pulldown_cmark::Options::ENABLE_TABLES)
                    .filter_map(|event| match event {
                        pulldown_cmark::Event::Text(text) => Some(text.into_string()),
                        _ => None,
                    })
                    .collect();
            assert_eq!(text.matches("KEEP").count(), 2049);
        }
    }

    /// 이것을 실패시키는 것: 절대 격자 한도/팽창률 한도 삭제 또는 경계 변경.
    #[test]
    fn table_grid_limits_are_inclusive_and_independent() {
        // Keep the expansion below 16 so only the absolute grid cap fires.
        let rows =
            |count| (0..count).map(|i| vec![Cell::new(); if i == 0 { 1024 } else { 64 }]).collect();
        let at = render_table::<Md>(rows(1024), 1024, crate::decoder::MAX_MD_FILE_SIZE);
        assert!(!at.as_str().contains("표 출력 한도 초과"));
        let over = render_table::<Md>(rows(1025), 1024, crate::decoder::MAX_MD_FILE_SIZE);
        assert!(over.as_str().contains("표 출력 한도 초과"));
        let rows = || vec![vec![Cell::new()]; 16];
        assert!(!render_table::<Md>(rows(), 16, crate::decoder::MAX_MD_FILE_SIZE)
            .as_str()
            .contains("표 출력 한도 초과"));
        assert!(render_table::<Md>(rows(), 17, crate::decoder::MAX_MD_FILE_SIZE)
            .as_str()
            .contains("표 출력 한도 초과"));
    }

    /// 이것을 실패시키는 것: 출력량 경계의 >를 >=로 변경/검사 삭제, fallback 재escape.
    #[test]
    fn table_byte_limit_is_inclusive_and_fallback_keeps_markup() {
        let rows = || vec![vec![Cell::lossy_paragraph(vec![Piece::Text("a\\b|c".into())])]];
        for nested in [false, true] {
            let bytes = table_bytes(&rows(), 1, nested);
            let exact = if nested {
                render_table::<Cell>(rows(), 1, bytes).as_str().to_owned()
            } else {
                render_table::<Md>(rows(), 1, bytes).as_str().to_owned()
            };
            assert_eq!(exact.len() as u64, bytes);
            let fallback = if nested {
                render_table::<Cell>(rows(), 1, bytes - 1).as_str().to_owned()
            } else {
                render_table::<Md>(rows(), 1, bytes - 1).as_str().to_owned()
            };
            assert!(fallback.contains("표 출력 한도 초과"));
            assert!(fallback.contains(r"a\\b\|c"), "{fallback}");
            assert!(!fallback.contains("---"));
        }
    }

    /// 이것을 실패시키는 것: 대체 표현에서 그림 참조 누락/셀 텍스트 재escape.
    #[test]
    fn table_fallback_preserves_images_and_text_after_gfm_parse() {
        use pulldown_cmark::{Event, Options, Parser, Tag};
        for nested in [false, true] {
            let mut destinations = std::collections::HashSet::new();
            let image = Md::image(Md::text("picture"), "image.png", &mut destinations).into_cell();
            let rows = vec![vec![Cell::lossy_paragraph(vec![Piece::Text(r"a\b|c".into())]), image]];
            let md = if nested {
                Cell::table(vec![vec![render_table::<Cell>(rows, 2, 0)]], 1)
            } else {
                render_table::<Md>(rows, 2, 0)
            };
            let mut images = Vec::new();
            let mut text = String::new();
            for event in Parser::new_ext(md.as_str(), Options::ENABLE_TABLES) {
                match event {
                    Event::Start(Tag::Image { dest_url, .. }) => {
                        images.push(dest_url.into_string())
                    }
                    Event::Text(value) => text.push_str(&value),
                    _ => {}
                }
            }
            assert_eq!(images, ["image.png"]);
            assert!(text.contains(r"a\b|c"), "{text}");
            assert!(!super::super::guard::foreign_markup(
                md.as_str(),
                &super::super::guard::Allowed {
                    tags: super::super::guard::LOSSY_TAGS,
                    destinations: &destinations,
                    decoder_structure: false,
                }
            ));
        }
    }

    /// 이것을 실패시키는 것: 셀 원문 구문 보호 삭제, 대체 표현 문단/접두사 제거,
    /// 생성된 그림/링크를 재escape하거나 정상 중첩 격자의 셀 경계를 무시하기.
    #[test]
    fn table_cell_inline_boundaries_survive_gfm_in_every_host() {
        use hwpforge_core::{Image, ImageFormat};
        use hwpforge_foundation::HwpUnit;
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let unit = HwpUnit::new(1000).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        for (open, close) in [
            ("`", "`"),
            ("``", "``"),
            ("`OPEN", "CLOSE`"),
            ("[OPEN", "CLOSE](https://foreign.invalid)"),
            ("[OPEN", "CLOSE][ref]"),
            ("*OPEN", "CLOSE*"),
            ("_OPEN", "CLOSE_"),
            ("~~OPEN", "CLOSE~~"),
            ("<a title='OPEN", "CLOSE'>"),
        ] {
            for n in [8, 32] {
                for host in 0..3 {
                    let mut table = sparse_table(n);
                    table.rows[0].cells[0].paragraphs = vec![p(vec![t(open)])];
                    table.rows[1].cells[0].paragraphs = vec![p(vec![
                        Run::image(
                            Image::new("image.png", unit, unit, ImageFormat::Png),
                            CharShapeIndex::new(0),
                        ),
                        Run::control(
                            Control::Hyperlink {
                                text: "LINK".into(),
                                url: "https://example.invalid/".into(),
                            },
                            CharShapeIndex::new(0),
                        ),
                        t(r"RAW\a|b"),
                    ])];
                    table.rows[1].cells[1].paragraphs = vec![p(vec![t(close)])];
                    // These become block openers only if fallback loses its inline prefix.
                    for (cell, raw) in table.rows[1].cells[2..].iter_mut().zip([
                        "# heading",
                        "- list",
                        "1. item",
                        "> quote",
                        "[ref]: https://foreign.invalid",
                    ]) {
                        cell.paragraphs = vec![p(vec![t(raw)])];
                    }
                    let expected: Vec<_> = table
                        .rows
                        .iter()
                        .flat_map(|r| &r.cells)
                        .flat_map(|c| &c.paragraphs)
                        .flat_map(|p| &p.runs)
                        .filter_map(|r| r.content.plain_text().map(|t| t.into_owned()))
                        .collect();
                    let run = Run::table(table, CharShapeIndex::new(0));
                    let run = match host {
                        0 => run,
                        1 => Run::control(
                            Control::text_box(vec![p(vec![run])], unit, unit),
                            CharShapeIndex::new(0),
                        ),
                        _ => Run::table(
                            Table::new(vec![TableRow::new(vec![TableCell::new(
                                vec![p(vec![t("OUTER`"), run, t("`TAIL")])],
                                unit,
                            )])]),
                            CharShapeIndex::new(0),
                        ),
                    };
                    let (md, warnings) = encode_without_template_report(&validated_document(vec![
                        p(vec![t("LEAD")]),
                        p(vec![run]),
                        p(vec![t("END")]),
                    ]))
                    .unwrap();
                    assert!(warnings.is_empty(), "{warnings:?}");
                    assert_eq!(md.contains("표 출력 한도 초과"), n == 32, "{md}");
                    let mut text = String::new();
                    let mut images = Vec::new();
                    let mut links = Vec::new();
                    let mut paragraphs = Vec::<String>::new();
                    let mut in_paragraph = false;
                    for event in
                        Parser::new_ext(&md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH)
                    {
                        match event {
                            Event::Start(Tag::Paragraph) => {
                                paragraphs.push(String::new());
                                in_paragraph = true;
                            }
                            Event::End(pulldown_cmark::TagEnd::Paragraph) => in_paragraph = false,
                            Event::Start(Tag::Image { dest_url, .. }) => {
                                images.push(dest_url.into_string())
                            }
                            Event::Start(Tag::Link { dest_url, .. }) => {
                                links.push(dest_url.into_string())
                            }
                            Event::Text(value) => {
                                text.push_str(&value);
                                if in_paragraph {
                                    paragraphs.last_mut().unwrap().push_str(&value);
                                }
                            }
                            Event::Code(_)
                            | Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough) => {
                                panic!("cross-cell syntax: {md}")
                            }
                            _ => {}
                        }
                    }
                    assert_eq!(images, ["image.png"], "{md}");
                    assert_eq!(links, ["https://example.invalid/"], "{md}");
                    for raw in &expected {
                        assert!(text.contains(raw), "missing {raw:?}: {text}");
                    }
                    assert_eq!(
                        text.matches('`').count(),
                        expected.iter().map(|raw| raw.matches('`').count()).sum::<usize>()
                            + if host == 2 { 2 } else { 0 },
                        "{text}"
                    );
                    assert_eq!(
                        text.matches("KEEP").count(),
                        expected.iter().filter(|t| t.as_str() == "KEEP").count()
                    );
                    assert!(text.starts_with("LEAD") && text.ends_with("END"), "{text}");
                    if host == 2 {
                        assert!(text.contains("OUTER`") && text.contains("`TAIL"), "{text}");
                    }
                    if n == 32 && host != 2 {
                        assert!(paragraphs.iter().any(|s| s == open), "{paragraphs:?}");
                        assert!(paragraphs.iter().any(|s| s == close), "{paragraphs:?}");
                    }
                }
            }
        }
    }

    /// 이것을 실패시키는 것: 셀 안 문단/컨테이너/캡션의 원문 구문 보호 삭제.
    #[test]
    fn cell_fragment_inline_boundaries_preserve_text_and_objects() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::{Image, ImageFormat};
        use hwpforge_foundation::HwpUnit;
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let unit = HwpUnit::new(1000).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        for (open, close) in [
            ("`OPEN", "CLOSE`"),
            ("[OPEN", "CLOSE](https://foreign.invalid)"),
            ("*OPEN", "CLOSE*"),
            ("[OPEN", "CLOSE]"),
        ] {
            for fragment in 0..4 {
                let mut image = Image::new("image.png", unit, unit, ImageFormat::Png);
                if fragment == 2 {
                    image.caption = Some(Caption::new(vec![p(vec![t(open)])], CaptionSide::Bottom));
                }
                if fragment == 3 {
                    image.caption =
                        Some(Caption::new(vec![p(vec![t(close)])], CaptionSide::Bottom));
                }
                let objects = vec![
                    Run::image(image, CharShapeIndex::new(0)),
                    Run::control(
                        Control::Hyperlink {
                            text: "LINK".into(),
                            url: "https://example.invalid/".into(),
                        },
                        CharShapeIndex::new(0),
                    ),
                ];
                let paragraphs = match fragment {
                    0 => vec![p(vec![t(open)]), p(objects), p(vec![t(close)])],
                    1 => vec![p(vec![Run::control(
                        Control::text_box(
                            vec![p(vec![t(open)]), p(objects), p(vec![t(close)])],
                            unit,
                            unit,
                        ),
                        CharShapeIndex::new(0),
                    )])],
                    2 => vec![p(objects), p(vec![t(close)])],
                    _ => vec![p(vec![
                        objects[1].clone(),
                        t(open),
                        objects[0].clone(),
                        t("(https://foreign.invalid)"),
                    ])],
                };
                let table = Table::new(vec![TableRow::new(vec![TableCell::new(paragraphs, unit)])]);
                let md = encode_without_template(&validated_document(vec![p(vec![Run::table(
                    table,
                    CharShapeIndex::new(0),
                )])]))
                .unwrap();
                let mut text = String::new();
                let mut images = Vec::new();
                let mut links = Vec::new();
                for event in Parser::new_ext(&md, Options::ENABLE_TABLES) {
                    match event {
                        Event::Start(Tag::Image { dest_url, .. }) => {
                            images.push(dest_url.into_string())
                        }
                        Event::Start(Tag::Link { dest_url, .. }) => {
                            links.push(dest_url.into_string())
                        }
                        Event::Text(value) => text.push_str(&value),
                        Event::Code(_) | Event::Start(Tag::Emphasis | Tag::Strong) => {
                            panic!("cross-fragment syntax: {md}")
                        }
                        _ => {}
                    }
                }
                assert_eq!(images, ["image.png"], "{md}");
                assert_eq!(links, ["https://example.invalid/"], "{md}");
                assert!(
                    text.contains(open) && text.contains(close) && text.contains("LINK"),
                    "{text}"
                );
            }
        }
        // A serialized caption's tail must not form an entity with later
        // raw text, or turn a generated link into an image.
        for (caption, tail, expected) in [("CAP&am", "p;", "CAP&amp;"), ("!", "", "!")] {
            let mut image = Image::new("image.png", unit, unit, ImageFormat::Png);
            image.caption = Some(Caption::new(vec![p(vec![t(caption)])], CaptionSide::Bottom));
            let runs = vec![
                Run::image(image, CharShapeIndex::new(0)),
                t(tail),
                Run::control(
                    Control::Hyperlink {
                        text: "LINK".into(),
                        url: "https://example.invalid/".into(),
                    },
                    CharShapeIndex::new(0),
                ),
            ];
            let table = Table::new(vec![TableRow::new(vec![TableCell::new(vec![p(runs)], unit)])]);
            let (md, warnings) = encode_without_template_report(&validated_document(vec![p(
                vec![Run::table(table, CharShapeIndex::new(0))],
            )]))
            .unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            let mut text = String::new();
            let mut images = Vec::new();
            let mut links = Vec::new();
            for event in Parser::new_ext(&md, Options::ENABLE_TABLES) {
                match event {
                    Event::Start(Tag::Image { dest_url, .. }) => {
                        images.push(dest_url.into_string())
                    }
                    Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.into_string()),
                    Event::Text(value) => text.push_str(&value),
                    _ => {}
                }
            }
            assert_eq!(images, ["image.png"], "{md}");
            assert_eq!(links, ["https://example.invalid/"], "{md}");
            assert!(text.contains(expected), "{text}");
        }
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
            resolve_mapping(&template).unwrap().0.heading1.para_shape_id,
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
        assert_eq!(md, "㊀");
    }

    /// The lossy fallback keeps a picture and an allowed link as lossy
    /// writes them; a link not allowed and a note are their text, the rest
    /// escaped, leading whitespace dropped. Through the check: the block
    /// falls back to exactly that.
    /// 이것을 실패시키는 것: 그림을 남기지 않기 · 링크를 남기지 않기 · 첫 공백 남기기.
    #[test]
    fn lossy_fallback_keeps_pictures_and_links() {
        use super::super::guard::{checked, Allowed, Block, Source, LOSSY_TAGS};
        let image = hwpforge_core::Image::new(
            "BinData/p.png",
            hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
            hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
            hwpforge_core::ImageFormat::Png,
        );
        let c = |control| Run::control(control, CharShapeIndex::new(0));
        let paragraph = Paragraph::with_runs(
            vec![
                Run::text("-\t[r]: /u\n\n", CharShapeIndex::new(0)),
                Run::image(image, CharShapeIndex::new(0)),
                c(Control::Hyperlink { text: " L".into(), url: "https://e.invalid/".into() }),
                c(Control::Hyperlink { text: "J".into(), url: "javascript:x".into() }),
                c(Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text("n", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )])),
            ],
            ParaShapeIndex::new(0),
        );
        let mut destinations = std::collections::HashSet::new();
        let text = fallback_text(&paragraph, &mut destinations);
        assert_eq!(text, "\\-\t\\[r\\]\\: \\/u  ![p](BinData/p.png)[ L](https://e.invalid/)J n");
        let block = Block {
            markdown: "<script>x</script>".into(),
            source: Some(Source {
                prefix: String::new(),
                texts: vec![text.clone()],
                at: vec![(0, 0)],
            }),
            body: false,
            tight: false,
        };
        let allowed =
            Allowed { tags: LOSSY_TAGS, destinations: &destinations, decoder_structure: false };
        let mut warnings = Vec::new();
        assert_eq!(checked(vec![block], &allowed, &mut warnings), text);
        assert_eq!(warnings.len(), 1);
        // Its leading whitespace is dropped (four spaces would make code).
        let indented = Paragraph::with_runs(
            vec![Run::text("    a", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        assert_eq!(fallback_text(&indented, &mut destinations), "a");
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = paragraph_text_markdown(&paragraph, &mut Vec::new(), &mut Default::default());
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
        // 이것을 실패시키는 것: 빈 행 패딩 중 행/셀 누락.
        assert_eq!(
            gfm_blocks(&md),
            vec![GfmBlock::Table(vec![vec!["header".into()], vec!["".into()]])]
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

        let md = encode_paragraph(
            &paragraph,
            None,
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
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

    /// A template item (no registry list) is always `- ` then its text:
    /// a marker in its text (`10. `, `* `), a thematic break (`*\t*\t*`), a
    /// marker alone on its first line, with any line ending, is text, so a
    /// child after it nests under it. An earlier decoder wrote numbers as
    /// such text; `10. x` is now written `- 10\. x`, by intent.
    /// 이것을 실패시키는 것: 첫 줄 보호 삭제 · 원문 표지 보존 경로 되살리기.
    #[test]
    fn lossy_mapped_item_text_is_text() {
        for (text, read) in [
            ("no marker here", "no marker here"),
            ("- already a list", "- already a list"),
            ("* bullet", "* bullet"),
            ("1. ordered", "1. ordered"),
            ("10. x", "10. x"),
            ("*\t*\t*", "*\t*\t*"),
            ("* * *", "* * *"),
            ("-\t-\t-", "-\t-\t-"),
        ] {
            let (md, out) = lossy_list(vec![(t(text), Li::Item), (t("child"), Li::Task(1))]);
            assert_eq!(out, depths(&[(read, 1), ("child", 2)]), "{md}");
        }
        assert_eq!(lossy_list(vec![(t("10. x"), Li::Item)]).0, "- 10\\. x");
        // A marker alone on the first line, then a blank line: text in the
        // item, whatever the line ending.
        for ending in ["\n", "\r\n", "\r"] {
            let text = format!("10.{ending}{ending}text");
            let (md, out) = lossy_list(vec![(t(&text), Li::Item), (t("child"), Li::Task(1))]);
            assert_eq!(out, depths(&[("10.", 1), ("text", 1), ("child", 2)]), "{md}");
            let (md, out) = lossy_list(vec![(t(&format!("10.{ending}")), Li::Item)]);
            assert_eq!(out, depths(&[("10.", 1)]), "{md}");
        }
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

        let md = encode_paragraph(
            &paragraph,
            Some(&mapping),
            None,
            &mut Vec::new(),
            &mut Default::default(),
            &mut Default::default(),
        );
        assert!(md.starts_with("```\n"));
        assert!(md.ends_with("\n```"));
        assert!(md.contains("let x = 1;"));
    }

    /// A paragraph of [`lossy_list`].
    enum Li {
        /// A numbered item at a level (a registry list).
        Num(u8),
        /// The template's plain list item (no registry list).
        Item,
        /// An unchecked task item at a level.
        Task(usize),
        /// A continuation at a level.
        Cont(u8),
        Body,
    }

    /// Lossy Markdown of one paragraph per `(runs, kind)` with the default
    /// template, and each text with how many list items hold it.
    fn lossy_list(paras: Vec<(Vec<Run>, Li)>) -> (String, Vec<(String, Option<usize>)>) {
        let template = builtin_default().unwrap();
        let (mapping, mut registry) = resolve_mapping(&template).unwrap();
        let base = registry.para_shape(mapping.body.para_shape_id).unwrap().clone();
        let paragraphs = paras
            .into_iter()
            .map(|(runs, kind)| {
                let shape = match kind {
                    Li::Num(level) => {
                        let mut para = base.clone();
                        para.list = Some(ParagraphListRef::Number {
                            numbering_id: hwpforge_foundation::NumberingIndex::new(0),
                            level,
                        });
                        registry.para_shapes.push(para);
                        ParaShapeIndex::new(registry.para_shapes.len() - 1)
                    }
                    Li::Item => mapping.list_item.para_shape_id,
                    Li::Task(level) => mapping.task_list.unchecked[level],
                    Li::Cont(level) => mapping.list_continuation(level).para_shape_id,
                    Li::Body => mapping.body.para_shape_id,
                };
                Paragraph::with_runs(runs, shape)
            })
            .collect();
        let doc = validated_document(paragraphs);
        let md = encode_body(&doc, Some(&mapping), Some(&registry), &mut Vec::new());
        let depths = super::super::list_format::item_depths(&md);
        (md, depths)
    }

    fn t(text: &str) -> Vec<Run> {
        vec![Run::text(text, CharShapeIndex::new(0))]
    }

    fn depths(pairs: &[(&str, usize)]) -> Vec<(String, Option<usize>)> {
        pairs.iter().map(|&(text, depth)| (text.to_string(), Some(depth))).collect()
    }

    /// The lossy encoder nests numbered items, keeps a numbered item's
    /// continuation inside it, and starts a new list after a Body paragraph
    /// at column 0, as the styled one does.
    /// 이것을 실패시키는 것: lossy 에서 항목 들여쓰기를 수준당 두 칸으로 ·
    /// 목록이 아닌 블록에서 열린 항목 비우기 삭제.
    #[test]
    fn lossy_numbered_lists_nest_and_reset() {
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (t("b"), Li::Num(1)),
            (t("c"), Li::Num(2)),
            (t("c2"), Li::Cont(2)),
            (t("x"), Li::Body),
            (t("d"), Li::Num(2)),
        ]);
        let want = depths(&[("a", 1), ("b", 2), ("c", 3), ("c2", 3), ("x", 0), ("d", 1)]);
        assert_eq!(out, want, "{md}");
    }

    /// The template's plain list item, which has no registry list, is a
    /// parent like any other: a task child nests under it, a continuation
    /// stays in it, and a source marker (`10. `) remains text after a bullet.
    /// 이것을 실패시키는 것: `mapped_item` 등록 삭제.
    #[test]
    fn lossy_mapped_item_parents_children() {
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Item),
            (t("b"), Li::Task(1)),
            (t("x"), Li::Body),
            (t("c"), Li::Item),
            (t("c2"), Li::Cont(0)),
            (t("y"), Li::Body),
            (t("10. d"), Li::Item),
            (t("e"), Li::Task(1)),
            (t("z"), Li::Body),
            (t("1.2 version"), Li::Item),
            (t("f"), Li::Task(1)),
        ]);
        let want = depths(&[
            ("a", 1),
            ("b", 2),
            ("x", 0),
            ("c", 1),
            ("c2", 1),
            ("y", 0),
            // The number is text after `- `, by intent.
            ("10. d", 1),
            ("e", 2),
            ("z", 0),
            ("1.2 version", 1),
            ("f", 2),
        ]);
        assert_eq!(out, want, "{md}");
    }

    /// An item paragraph's table is written as its own block at column 0:
    /// after it the list is closed, and item text after it starts a new
    /// one, whatever the order (text then table, table then text, text,
    /// table, text). The deeper item after the paragraph nests under the
    /// last item written.
    /// 이것을 실패시키는 것: 블록 조각에서 닫기 삭제 · 단독 표 문단에서 닫기 삭제 ·
    /// continuation 의 블록에서도 닫기.
    #[test]
    fn lossy_blocks_in_item_paragraph() {
        let table = || Run::table(sparse_table(1), CharShapeIndex::new(0));
        let text = |s: &str| t(s).remove(0);
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (vec![text("b"), table()], Li::Num(1)),
            (t("c"), Li::Num(2)),
        ]);
        assert_eq!(out, depths(&[("a", 1), ("b", 2), ("KEEP", 0), ("c", 1)]), "{md}");
        let item = |runs: Vec<Run>| lossy_list(vec![(runs, Li::Item), (t("c"), Li::Task(1))]);
        let (md, out) = item(vec![text("p"), table()]);
        assert_eq!(out, depths(&[("p", 1), ("KEEP", 0), ("c", 1)]), "{md}");
        let (md, out) = item(vec![table(), text("p")]);
        assert_eq!(out, depths(&[("KEEP", 0), ("p", 1), ("c", 2)]), "{md}");
        let (md, out) = item(vec![text("p"), table(), text("q")]);
        assert_eq!(out, depths(&[("p", 1), ("KEEP", 0), ("q", 1), ("c", 2)]), "{md}");
        // A registry item after the table starts at column 0.
        let (md, _) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (vec![table(), text("b")], Li::Num(1)),
            (t("c"), Li::Num(2)),
        ]);
        assert_eq!(md, "1. a\n\n\n\n| KEEP |\n| --- |\n\n1. b\n\n   1. c");
        // An item paragraph that is one table is a block at column 0.
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (t("b"), Li::Num(1)),
            (vec![table()], Li::Num(1)),
            (t("c"), Li::Num(2)),
        ]);
        assert_eq!(out, depths(&[("a", 1), ("b", 2), ("KEEP", 0), ("c", 1)]), "{md}");
        // A continuation's table is inside its item: the item stays open.
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (vec![text("x"), table()], Li::Cont(0)),
            (t("b"), Li::Num(1)),
        ]);
        assert_eq!(out, depths(&[("a", 1), ("x", 1), ("KEEP", 1), ("b", 2)]), "{md}");
    }

    /// An item's text stays in its item and makes no structure, read back with its
    /// characters: a blank line then `- x` is text in the item (not a list
    /// that closes it), a blank line then indented text is text in the
    /// item (not a code block); the next deeper item nests under it. A
    /// template item is always a bullet, with its source marker as text.
    /// 이것을 실패시키는 것: 줄 머리 보호 삭제 · 뒷줄 들여쓰기 삭제 · 앞 공백 남기기.
    #[test]
    fn lossy_item_text_stays_in_its_item() {
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (t("b\n\n- x"), Li::Num(1)),
            (t("c"), Li::Num(2)),
        ]);
        assert_eq!(out, depths(&[("a", 1), ("b", 2), ("- x", 2), ("c", 3)]), "{md}");
        let (md, out) = lossy_list(vec![
            (t("a"), Li::Num(0)),
            (t("b"), Li::Num(1)),
            (t("c\n\n   x\n\n    y"), Li::Num(2)),
            (t("d"), Li::Num(3)),
        ]);
        assert_eq!(
            out,
            depths(&[("a", 1), ("b", 2), ("c", 3), ("x", 3), ("y", 3), ("d", 4)]),
            "{md}"
        );
        // Pieces joined into a marker (`1` then a composed `. x`) are judged
        // together.
        let joined =
            vec![t("1").remove(0), Run::control(Control::compose(". x"), CharShapeIndex::new(0))];
        let (md, out) =
            lossy_list(vec![(t("a"), Li::Num(0)), (joined, Li::Num(1)), (t("c"), Li::Num(2))]);
        assert_eq!(out, depths(&[("a", 1), ("1. x", 2), ("c", 3)]), "{md}");
        let (md, out) = lossy_list(vec![(t("p\n\n# q"), Li::Item), (t("c"), Li::Task(1))]);
        assert_eq!(out, depths(&[("p", 1), ("# q", 1), ("c", 2)]), "{md}");
    }

    /// Protection sees the same line starts as list layout, including a
    /// continuation's first line. Source indentation is layout, not text.
    // 이것을 실패시키는 것: 보호 전 공백 제거 삭제 · 탭만 남기기 · 공백만 남기기 ·
    // 줄 머리 조건 삭제 · 첫 줄/후속 줄 공백 남기기.
    #[test]
    fn lossy_indented_list_syntax_stays_text() {
        use pulldown_cmark::{Event, Parser, Tag};
        for syntax in ["```", "~~~", "[ref]: /target"] {
            for indent in ["    ", "        ", "\t"] {
                for continuation in [false, true] {
                    let text = format!("{indent}{syntax}");
                    let gap = if syntax.starts_with('[') { "\n\n" } else { "\n" };
                    let paras = if continuation {
                        vec![(t("parent"), Li::Item), (t(&text), Li::Cont(0))]
                    } else {
                        vec![(t(&format!("parent{gap}{text}")), Li::Item)]
                    };
                    let (md, out) =
                        lossy_list(paras.into_iter().chain([(t("child"), Li::Task(1))]).collect());
                    let want = if continuation || gap == "\n\n" {
                        depths(&[("parent", 1), (syntax, 1), ("child", 2)])
                    } else {
                        depths(&[(&format!("parent\n{syntax}"), 1), ("child", 2)])
                    };
                    assert_eq!(
                        out, want,
                        "{indent:?} {syntax:?} continuation={continuation}: {md}"
                    );
                    assert!(
                        !Parser::new_ext(&md, super::super::md_options())
                            .any(|event| matches!(event, Event::Start(Tag::CodeBlock(_)))),
                        "{md}"
                    );
                }
            }
        }
    }

    /// An empty item is written as its marker alone and holds what follows
    /// it, its first block joined with one line ending: a deeper item, a
    /// continuation (`- child` as text), a template item's too. A sibling
    /// is not joined, and the items closed before it stay closed.
    /// 이것을 실패시키는 것: 빈 항목 쓰지 않기 · `tight` 무시(이어 붙이는 continuation
    /// 포함) · 닫기 삭제.
    #[test]
    fn lossy_empty_item_holds_its_first_block() {
        let (md, _) = lossy_list(vec![(t(""), Li::Num(0)), (t("child"), Li::Num(1))]);
        assert_eq!(md, "1.\n   1. child");
        let (md, _) = lossy_list(vec![(t(""), Li::Item), (t("child"), Li::Task(1))]);
        assert_eq!(md, "-\n  - [ ] child");
        let (md, _) = lossy_list(vec![(t(""), Li::Num(0)), (t("b"), Li::Num(0))]);
        assert_eq!(md, "1.\n\n1. b");
        // The first block's join is set by its first piece: a table after
        // the child's text does not undo it.
        let table = || Run::table(sparse_table(1), CharShapeIndex::new(0));
        let (md, out) =
            lossy_list(vec![(t(""), Li::Num(0)), (vec![t("b").remove(0), table()], Li::Num(1))]);
        assert_eq!(out, depths(&[("b", 2), ("KEEP", 0)]), "{md}");
        let (md, out) = lossy_list(vec![
            (t(""), Li::Num(0)),
            (vec![t("b").remove(0), table(), t("q").remove(0)], Li::Num(1)),
        ]);
        assert_eq!(out, depths(&[("b", 2), ("KEEP", 0), ("q", 1)]), "{md}");
        // A continuation's leading blank lines and leading table stay in
        // the empty item.
        let (md, out) = lossy_list(vec![(t(""), Li::Num(0)), (t("\n\ntext"), Li::Cont(0))]);
        assert_eq!(out, depths(&[("text", 1)]), "{md}");
        let (md, out) =
            lossy_list(vec![(t(""), Li::Num(0)), (vec![table(), t("t").remove(0)], Li::Cont(0))]);
        assert_eq!(out, depths(&[("KEEP", 1), ("t", 1)]), "{md}");
        for (paras, want) in [
            (
                vec![
                    (t("a"), Li::Num(0)),
                    (t("b"), Li::Num(1)),
                    (t(""), Li::Num(0)),
                    (t("c"), Li::Cont(0)),
                ],
                depths(&[("a", 1), ("b", 2), ("c", 1)]),
            ),
            (
                vec![
                    (t("a"), Li::Item),
                    (t("b"), Li::Task(1)),
                    (t(""), Li::Item),
                    (t("c"), Li::Cont(0)),
                ],
                depths(&[("a", 1), ("b", 2), ("c", 1)]),
            ),
            (
                vec![(t("a"), Li::Num(0)), (t(""), Li::Num(1)), (t("c"), Li::Cont(1))],
                depths(&[("a", 1), ("c", 2)]),
            ),
            (vec![(t(""), Li::Num(1)), (t("- child"), Li::Cont(1))], depths(&[("- child", 1)])),
            (
                vec![
                    (t("a"), Li::Num(0)),
                    (t("b"), Li::Num(1)),
                    (t(""), Li::Num(0)),
                    (t("d"), Li::Num(1)),
                ],
                depths(&[("a", 1), ("b", 2), ("d", 2)]),
            ),
        ] {
            let (md, out) = lossy_list(paras);
            assert_eq!(out, want, "{md}");
        }
    }

    /// A number and a tab at a template item's start are text after `- `
    /// (no longer kept as the item's marker, by intent): a child nests
    /// under the item.
    /// 이것을 실패시키는 것: 원문 표지 보존 경로 되살리기.
    #[test]
    fn lossy_mapped_item_tab_marker_is_text() {
        for text in ["10.\tparent", "1.\tparent"] {
            let (md, out) = lossy_list(vec![(t(text), Li::Item), (t("c"), Li::Task(1))]);
            assert_eq!(out, depths(&[(text, 1), ("c", 2)]), "{md}");
        }
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
        // The level-1 item nests under the level-0 item before it.
        let mut lists = ListIndent::default();

        assert_eq!(
            encode_paragraph(
                &unchecked,
                Some(&mapping),
                Some(&registry),
                &mut Vec::new(),
                &mut Default::default(),
                &mut lists,
            ),
            "- [ ] todo"
        );
        assert_eq!(
            encode_paragraph(
                &checked,
                Some(&mapping),
                Some(&registry),
                &mut Vec::new(),
                &mut Default::default(),
                &mut lists,
            ),
            "  - [x] done"
        );
    }

    /// WS: a group is drawn as its children in z-order in the body and in a
    /// table cell — picture links, a text box's text, nothing for a rectangle
    /// or an equation, a nested group's picture; the same traversal applies
    /// inside a text box.
    ///
    /// 이것을 실패시키는 것: 그림 갈래 삭제 · 중첩 재귀 삭제 · 자식 순서 뒤집기 ·
    /// 글상자 글자 갈래 삭제 · Equation 을 lossy 에서 렌더 · 표 칸 경로에서 묶음 누락 ·
    /// 글상자 내부 묶음 누락.
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
        assert_eq!(doc(para(vec![in_box])), "in ![a](BinData/a.png)");
    }

    /// A grouped text box retains its table; a whitespace-only box stays empty.
    /// 이것을 실패시키는 것: 묶음 글상자 안 표 누락 · 표 경계 삭제 · 도형 공백 트림 삭제.
    #[test]
    fn lossy_grouped_textbox_table_and_empty_shape() {
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
        assert_eq!(
            gfm_blocks(&doc(para(vec![t("앞"), s1, t("뒤")]))),
            vec![
                GfmBlock::Paragraph("앞".into()),
                GfmBlock::Table(vec![vec!["칸".into()]]),
                GfmBlock::Paragraph("뒤".into()),
            ]
        );
        let s2 = grp(Control::text_box(vec![para(vec![t("  ")])], u(10), u(10)));
        assert_eq!(
            gfm_blocks(&doc(para(vec![t("앞"), s2, t("뒤")]))),
            vec![GfmBlock::Paragraph("앞뒤".into())]
        );
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum GfmBlock {
        Paragraph(String),
        Table(Vec<Vec<String>>),
    }

    /// Read consumer-visible paragraphs and table cells, not source substrings.
    fn gfm_blocks(md: &str) -> Vec<GfmBlock> {
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let mut blocks = Vec::new();
        for event in Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS) {
            match event {
                Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item) => {
                    if !matches!(blocks.last(), Some(GfmBlock::Paragraph(text)) if text.is_empty())
                    {
                        blocks.push(GfmBlock::Paragraph(String::new()));
                    }
                }
                Event::Start(Tag::List(_) | Tag::BlockQuote(_) | Tag::Image { .. })
                | Event::TaskListMarker(_) => {}
                Event::Start(Tag::Table(_)) => blocks.push(GfmBlock::Table(Vec::new())),
                Event::Start(Tag::TableHead | Tag::TableRow) => {
                    let Some(GfmBlock::Table(rows)) = blocks.last_mut() else {
                        panic!("row outside table: {md}");
                    };
                    rows.push(Vec::new());
                }
                Event::Start(Tag::TableCell) => {
                    let Some(GfmBlock::Table(rows)) = blocks.last_mut() else {
                        panic!("cell outside table: {md}");
                    };
                    rows.last_mut().unwrap().push(String::new());
                }
                Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => {
                    match blocks.last_mut().unwrap() {
                        GfmBlock::Paragraph(out) => out.push_str(&text),
                        GfmBlock::Table(rows) => {
                            rows.last_mut().unwrap().last_mut().unwrap().push_str(&text);
                        }
                    }
                }
                Event::SoftBreak | Event::HardBreak => match blocks.last_mut().unwrap() {
                    GfmBlock::Paragraph(out) => out.push('\n'),
                    GfmBlock::Table(rows) => {
                        rows.last_mut().unwrap().last_mut().unwrap().push('\n');
                    }
                },
                other => assert!(matches!(other, Event::End(_)), "unexpected {other:?}: {md}"),
            }
        }
        blocks
    }

    #[test]
    fn lossy_ragged_tables_preserve_all_parsed_cells() {
        // 이것을 실패시키는 것: 첫 행 너비 사용, 헤더/본문 패딩 생략, 연속 표 경계 삭제.
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        let table = |rows: Vec<Vec<&str>>| {
            Table::new(
                rows.into_iter()
                    .map(|row| {
                        TableRow::new(
                            row.into_iter()
                                .map(|text| TableCell::new(vec![p(vec![t(text)])], u(1000)))
                                .collect(),
                        )
                    })
                    .collect(),
            )
        };
        let wide = table(vec![vec!["head"], vec!["left", "LOST", "last"], vec!["short"]]);
        let expected = GfmBlock::Table(vec![
            vec!["head".into(), "".into(), "".into()],
            vec!["left".into(), "LOST".into(), "last".into()],
            vec!["short".into(), "".into(), "".into()],
        ]);
        // Direct body, inline body and container all use the same width policy.
        for run in [
            Run::table(wide.clone(), CharShapeIndex::new(0)),
            Run::control(
                Control::text_box(
                    vec![p(vec![Run::table(wide.clone(), CharShapeIndex::new(0))])],
                    u(1000),
                    u(1000),
                ),
                CharShapeIndex::new(0),
            ),
        ] {
            for lead in [false, true] {
                let mut runs = Vec::new();
                if lead {
                    runs.push(t("LEAD"));
                }
                runs.push(run.clone());
                let md = encode_without_template(&validated_document(vec![p(runs)])).unwrap();
                let parsed = gfm_blocks(&md);
                let mut wanted = Vec::new();
                if lead {
                    wanted.push(GfmBlock::Paragraph("LEAD".into()));
                }
                wanted.push(expected.clone());
                assert_eq!(parsed, wanted, "{md}");
            }
        }
        let md = encode_without_template(&validated_document(vec![p(vec![
            Run::table(table(vec![vec!["A", "B"]]), CharShapeIndex::new(0)),
            Run::table(wide.clone(), CharShapeIndex::new(0)),
            Run::table(table(vec![vec!["END"]]), CharShapeIndex::new(0)),
        ])]))
        .unwrap();
        assert_eq!(
            gfm_blocks(&md),
            vec![
                GfmBlock::Table(vec![vec!["A".into(), "B".into()]]),
                expected,
                GfmBlock::Table(vec![vec!["END".into()]]),
            ]
        );
        // A nested table stays in one outer cell; padded pipes are escaped once.
        let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![p(vec![Run::control(
                Control::text_box(
                    vec![p(vec![Run::table(wide, CharShapeIndex::new(0))])],
                    u(1000),
                    u(1000),
                ),
                CharShapeIndex::new(0),
            )])],
            u(1000),
        )])]);
        let md = encode_without_template(&validated_document(vec![p(vec![Run::table(
            outer,
            CharShapeIndex::new(0),
        )])]))
        .unwrap();
        assert_eq!(gfm_blocks(&md), vec![GfmBlock::Table(vec![vec![
            "| head |  |  |<br>| --- | --- | --- |<br>| left | LOST | last |<br>| short |  |  |".into(),
        ]])]);
    }

    #[test]
    fn lossy_template_prefixes_only_format_text_around_tables() {
        // 이것을 실패시키는 것: 표 Piece까지 제목/목록 포맷팅, 표뿐인 문단에 빈 접두사 생성.
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let template = builtin_default().unwrap();
        let (mapping, _) = resolve_mapping(&template).unwrap();
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, mapping.body.para_shape_id);
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        let table = || {
            p(vec![Run::table(
                Table::new(vec![
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("head")])], u(1000))]),
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("cell")])], u(1000))]),
                ]),
                CharShapeIndex::new(0),
            )])
        };
        for style in [
            mapping.heading1.para_shape_id,
            mapping.list_item.para_shape_id,
            mapping.task_list.unchecked[0],
            mapping.task_list.checked[1],
        ] {
            for (lead, tail) in [(false, false), (false, true), (true, true)] {
                let mut children = vec![table()];
                if tail {
                    children.push(p(vec![t("TAIL")]));
                }
                let mut runs = Vec::new();
                if lead {
                    runs.push(t("LEAD"));
                }
                runs.push(Run::control(
                    Control::text_box(children, u(1000), u(1000)),
                    CharShapeIndex::new(0),
                ));
                let paragraph = Paragraph::with_runs(runs, style);
                let doc = validated_document(vec![paragraph.clone()]);
                let md = encode_with_template(&doc, &template).unwrap();
                let body = crate::frontmatter::extract_frontmatter(&md).unwrap();
                let mut expected = Vec::new();
                if lead {
                    expected.push(GfmBlock::Paragraph("LEAD".into()));
                }
                expected.push(GfmBlock::Table(vec![vec!["head".into()], vec!["cell".into()]]));
                if tail {
                    expected.push(GfmBlock::Paragraph("TAIL".into()));
                }
                assert_eq!(gfm_blocks(body.content), expected, "{md}");
                let formatted = Parser::new_ext(
                    body.content,
                    Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
                )
                .filter(|event| matches!(event, Event::Start(Tag::Heading { .. } | Tag::Item)))
                .count();
                assert_eq!(formatted, usize::from(lead) + usize::from(tail), "{md}");
                // Exercise the mapping-only list fallback as well as registry lists.
                if style == mapping.list_item.para_shape_id {
                    let fallback = encode_paragraph(
                        &paragraph,
                        Some(&mapping),
                        None,
                        &mut Vec::new(),
                        &mut Default::default(),
                        &mut Default::default(),
                    );
                    assert_eq!(gfm_blocks(&fallback), expected);
                }
            }
        }
    }

    #[test]
    fn lossy_template_block_joins_preserve_captions_and_prefixes() {
        // 이것을 실패시키는 것: 캡션/표의 Block 표시 제거, 블록 앞/뒤 경계 삭제,
        // 포맷팅 후 공통 Piece 연결 우회, 캡션 본문에 부모 접두사 적용.
        use hwpforge_core::caption::{Caption, CaptionSide};
        use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
        for numbered in [false, true] {
            let mut template = builtin_default().unwrap();
            if numbered {
                template.numberings.push(hwpforge_core::NumberingDef::default_outline());
                template.styles.get_mut("list_item").unwrap().para_shape.as_mut().unwrap().list =
                    Some(hwpforge_blueprint::style::PartialParagraphListRef::Number {
                        numbering_id: 1,
                        level: 0,
                    });
            }
            let (mapping, _) = resolve_mapping(&template).unwrap();
            let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
            let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
            let p = |runs| Paragraph::with_runs(runs, mapping.body.para_shape_id);
            let table = || {
                Table::new(vec![
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("head")])], u(1000))]),
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("cell")])], u(1000))]),
                ])
            };
            for (style, heading, checked) in [
                (mapping.heading1.para_shape_id, true, None),
                (mapping.list_item.para_shape_id, false, None),
                (mapping.task_list.unchecked[0], false, Some(false)),
                (mapping.task_list.checked[1], false, Some(true)),
            ] {
                for kind in ["table-caption", "image-caption", "container-table"] {
                    for lead in [false, true] {
                        let caption = Caption::new(
                            vec![p(vec![t("CAP")]), p(vec![t("CAP2")])],
                            CaptionSide::Bottom,
                        );
                        let object = match kind {
                            "table-caption" => {
                                let mut table = table();
                                table.caption = Some(caption);
                                Run::table(table, CharShapeIndex::new(0))
                            }
                            "image-caption" => {
                                let mut image = hwpforge_core::Image::new(
                                    "pic.png",
                                    u(1000),
                                    u(1000),
                                    hwpforge_core::ImageFormat::Png,
                                );
                                image.caption = Some(caption);
                                Run::image(image, CharShapeIndex::new(0))
                            }
                            _ => Run::control(
                                Control::text_box(
                                    vec![p(vec![Run::table(table(), CharShapeIndex::new(0))])],
                                    u(1000),
                                    u(1000),
                                ),
                                CharShapeIndex::new(0),
                            ),
                        };
                        let mut runs = Vec::new();
                        if lead {
                            runs.push(t("LEAD"));
                        }
                        runs.extend([object, t("TAIL")]);
                        let doc = validated_document(vec![Paragraph::with_runs(runs, style)]);
                        let md = encode_with_template(&doc, &template).unwrap();
                        let body = crate::frontmatter::extract_frontmatter(&md).unwrap();
                        let mut expected = Vec::new();
                        if kind == "image-caption" {
                            expected.push(GfmBlock::Paragraph(
                                if lead { "LEAD pic" } else { "pic" }.into(),
                            ));
                        } else {
                            if lead {
                                expected.push(GfmBlock::Paragraph("LEAD".into()));
                            }
                            expected.push(GfmBlock::Table(vec![
                                vec!["head".into()],
                                vec!["cell".into()],
                            ]));
                        }
                        if kind != "container-table" {
                            expected.extend([
                                GfmBlock::Paragraph("CAP".into()),
                                GfmBlock::Paragraph("CAP2".into()),
                            ]);
                        }
                        expected.push(GfmBlock::Paragraph("TAIL".into()));
                        assert_eq!(gfm_blocks(body.content), expected, "{kind}: {md}");
                        let plain = encode_without_template(&doc).unwrap();
                        assert_eq!(gfm_blocks(&plain), expected, "{kind}: {plain}");
                        let events: Vec<_> = Parser::new_ext(
                            body.content,
                            Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
                        )
                        .collect();
                        let formatted = 1 + usize::from(lead || kind == "image-caption");
                        // Counts alone miss CAP becoming the heading while
                        // TAIL falls back to a body paragraph.
                        // 이것을 실패시키는 것: 표 캡션 Block 표시를 제거해 CAP에 접두사 적용.
                        let mut formatted_texts = Vec::<String>::new();
                        let mut in_formatted = false;
                        for event in &events {
                            match event {
                                Event::Start(Tag::Heading { .. } | Tag::Item) => {
                                    formatted_texts.push(String::new());
                                    in_formatted = true;
                                }
                                Event::Text(text) if in_formatted => {
                                    formatted_texts.last_mut().unwrap().push_str(text);
                                }
                                Event::End(TagEnd::Heading(_) | TagEnd::Item) => {
                                    in_formatted = false;
                                }
                                _ => {}
                            }
                        }
                        let mut expected_formatted = Vec::new();
                        if kind == "image-caption" {
                            expected_formatted.push(if lead { "LEAD pic" } else { "pic" });
                        } else if lead {
                            expected_formatted.push("LEAD");
                        }
                        expected_formatted.push("TAIL");
                        assert_eq!(formatted_texts, expected_formatted, "{kind}: {md}");
                        assert_eq!(
                            events
                                .iter()
                                .filter(|e| matches!(e, Event::Start(Tag::Heading { .. })))
                                .count(),
                            if heading { formatted } else { 0 },
                            "{md}"
                        );
                        assert_eq!(
                            events.iter().filter(|e| matches!(e, Event::Start(Tag::Item))).count(),
                            if heading { 0 } else { formatted },
                            "{md}"
                        );
                        if style == mapping.list_item.para_shape_id {
                            assert_eq!(
                                events
                                    .iter()
                                    .any(|e| matches!(e, Event::Start(Tag::List(Some(1))))),
                                numbered,
                                "{md}"
                            );
                        }
                        let tasks: Vec<_> = events
                            .iter()
                            .filter_map(|e| match e {
                                Event::TaskListMarker(value) => Some(*value),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(
                            tasks,
                            checked.map_or_else(Vec::new, |value| vec![value; formatted]),
                            "{md}"
                        );
                        let images: Vec<_> = events
                            .iter()
                            .filter_map(|e| match e {
                                Event::Start(Tag::Image { dest_url, .. }) => {
                                    Some(dest_url.as_ref())
                                }
                                _ => None,
                            })
                            .collect();
                        assert_eq!(
                            images,
                            if kind == "image-caption" { vec!["pic.png"] } else { vec![] },
                            "{md}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn lossy_empty_captions_keep_inline_text_together() {
        // 이것을 실패시키는 것: 빈 Block이 인라인을 나누거나 블록 경계를 생성.
        use hwpforge_core::caption::{Caption, CaptionSide};
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let template = builtin_default().unwrap();
        let (mapping, _) = resolve_mapping(&template).unwrap();
        for caption in [
            None,
            Some(Caption::default()),
            Some(Caption::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("  ", CharShapeIndex::new(0))],
                    mapping.body.para_shape_id,
                )],
                CaptionSide::Bottom,
            )),
        ] {
            let mut image = hwpforge_core::Image::new(
                "pic.png",
                hwpforge_foundation::HwpUnit::new(1000).unwrap(),
                hwpforge_foundation::HwpUnit::new(1000).unwrap(),
                hwpforge_core::ImageFormat::Png,
            );
            image.caption = caption;
            let doc = validated_document(vec![Paragraph::with_runs(
                vec![
                    Run::text("LEAD", CharShapeIndex::new(0)),
                    Run::image(image, CharShapeIndex::new(0)),
                    Run::text("TAIL", CharShapeIndex::new(0)),
                ],
                mapping.heading1.para_shape_id,
            )]);
            let plain = encode_without_template(&doc).unwrap();
            assert_eq!(gfm_blocks(&plain), vec![GfmBlock::Paragraph("LEAD picTAIL".into())]);
            let md = encode_with_template(&doc, &template).unwrap();
            let body = crate::frontmatter::extract_frontmatter(&md).unwrap();
            assert_eq!(gfm_blocks(body.content), vec![GfmBlock::Paragraph("LEAD picTAIL".into())]);
            assert_eq!(
                Parser::new_ext(body.content, Options::ENABLE_TABLES)
                    .filter(|e| matches!(e, Event::Start(Tag::Heading { .. })))
                    .count(),
                1,
                "{md}"
            );
        }
    }

    #[test]
    fn lossy_caption_blocks_remain_cell_safe() {
        // 이것을 실패시키는 것: 공통 Block 연결이 Cell에도 실제 빈 줄을 삽입하거나 재escape.
        use hwpforge_core::caption::{Caption, CaptionSide};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        let caption = Caption::new(vec![p(vec![t(r"CAP\a|b")])], CaptionSide::Bottom);
        let mut image =
            hwpforge_core::Image::new("pic.png", u(1000), u(1000), hwpforge_core::ImageFormat::Png);
        image.caption = Some(caption.clone());
        let mut table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![p(vec![t("inner")])],
            u(1000),
        )])]);
        table.caption = Some(caption);
        for (object, expected) in [
            (Run::image(image, CharShapeIndex::new(0)), r"LEAD pic<br><br>CAP\a|bTAIL"),
            (
                Run::table(table, CharShapeIndex::new(0)),
                r"LEAD<br>| inner |<br>| --- |<br><br>CAP\a|bTAIL",
            ),
        ] {
            let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![p(vec![t("LEAD"), object, t("TAIL")])],
                u(1000),
            )])]);
            let md = encode_without_template(&validated_document(vec![p(vec![Run::table(
                outer,
                CharShapeIndex::new(0),
            )])]))
            .unwrap();
            assert_eq!(gfm_blocks(&md), vec![GfmBlock::Table(vec![vec![expected.into()]])], "{md}");
        }
    }

    #[test]
    fn lossy_container_table_block_boundaries() {
        // 이것을 실패시키는 것: 표 앞/뒤 빈 줄 제거, 자식 전체를 평문 공백으로 연결.
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let t = |text: &str| Run::text(text, CharShapeIndex::new(0));
        let c = |control| Run::control(control, CharShapeIndex::new(0));
        let table = || {
            p(vec![Run::table(
                Table::new(vec![
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("head")])], u(1000))]),
                    TableRow::new(vec![TableCell::new(vec![p(vec![t("cell")])], u(1000))]),
                ]),
                CharShapeIndex::new(0),
            )])
        };
        let textbox = |children| c(Control::text_box(children, u(1000), u(1000)));
        let expected_table = || GfmBlock::Table(vec![vec!["head".into()], vec!["cell".into()]]);
        let body = |text: &str| GfmBlock::Paragraph(text.into());
        let cases = [
            // The second child must not become a discarded excess cell.
            (
                vec![textbox(vec![table(), p(vec![t("TAIL")])])],
                vec![expected_table(), body("TAIL")],
            ),
            (
                vec![t("LEAD"), textbox(vec![table()]), t("TAIL")],
                vec![body("LEAD"), expected_table(), body("TAIL")],
            ),
            (
                vec![
                    textbox(vec![table()]),
                    textbox(vec![table()]),
                    textbox(vec![p(vec![t("TAIL")])]),
                ],
                vec![expected_table(), expected_table(), body("TAIL")],
            ),
            (
                vec![c(Control::footnote(vec![table()]))],
                vec![body("(footnote:"), expected_table(), body(")")],
            ),
            (
                vec![c(Control::endnote(vec![table(), p(vec![t("TAIL")])]))],
                vec![body("(endnote:"), expected_table(), body("TAIL)")],
            ),
            (
                vec![textbox(vec![p(vec![t("LEAD")]), table(), p(vec![t("TAIL")])])],
                vec![body("LEAD"), expected_table(), body("TAIL")],
            ),
            (
                vec![t("PARENT"), textbox(vec![p(vec![t(" first")]), p(vec![t("second ")])])],
                vec![body("PARENTfirst second")],
            ),
        ];
        for (runs, expected) in cases {
            let md = encode_without_template(&validated_document(vec![p(runs)])).unwrap();
            assert_eq!(gfm_blocks(&md), expected, "{md}");
        }

        // 이것을 실패시키는 것: 셀 안 표에 본문 블록 경계를 적용하거나 재escape.
        let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![p(vec![t(r"a\b|c"), textbox(vec![table(), p(vec![t("TAIL")])])])],
            u(1000),
        )])]);
        let md = encode_without_template(&validated_document(vec![p(vec![Run::table(
            outer,
            CharShapeIndex::new(0),
        )])]))
        .unwrap();
        assert_eq!(
            gfm_blocks(&md),
            vec![GfmBlock::Table(vec![vec![r"a\b|c| head |<br>| --- |<br>| cell | TAIL".into()]])],
            "{md}"
        );
    }

    #[test]
    fn lossy_container_tables_keep_cells_and_single_escape() {
        // 이것을 실패시키는 것: 각 컨테이너의 내부 제어 생략, 칸에서 재escape.
        use hwpforge_core::{TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let table = Table::new(vec![TableRow::new(vec![
            TableCell::new(vec![p(vec![Run::text("왼쪽", CharShapeIndex::new(0))])], u(1000)),
            TableCell::new(vec![p(vec![Run::text("오른쪽", CharShapeIndex::new(0))])], u(1000)),
        ])]);
        let children = vec![p(vec![Run::table(table, CharShapeIndex::new(0))])];
        let mut polygon = Control::polygon(vec![
            hwpforge_core::control::ShapePoint::new(0, 0),
            hwpforge_core::control::ShapePoint::new(1000, 0),
            hwpforge_core::control::ShapePoint::new(500, 1000),
        ])
        .unwrap();
        if let Control::Polygon { paragraphs, .. } = &mut polygon {
            *paragraphs = children.clone();
        }
        for control in [
            Control::text_box(children.clone(), u(1000), u(1000)),
            Control::footnote(children.clone()),
            Control::endnote(children.clone()),
            Control::ellipse_with_text(u(1000), u(1000), children),
            polygon,
        ] {
            let control_label = match &control {
                Control::Footnote { .. } => Some("(footnote:"),
                Control::Endnote { .. } => Some("(endnote:"),
                _ => None,
            };
            let md = encode_without_template(&validated_document(vec![p(vec![Run::control(
                control,
                CharShapeIndex::new(0),
            )])]))
            .unwrap();
            let expected_table = GfmBlock::Table(vec![vec!["왼쪽".into(), "오른쪽".into()]]);
            let expected = match control_label {
                Some(label) => vec![
                    GfmBlock::Paragraph(label.into()),
                    expected_table,
                    GfmBlock::Paragraph(")".into()),
                ],
                None => vec![expected_table],
            };
            assert_eq!(gfm_blocks(&md), expected, "{md}");
        }
        let textbox = Control::text_box(
            vec![p(vec![Run::text(r"a\b|c", CharShapeIndex::new(0))])],
            u(1000),
            u(1000),
        );
        let outer = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![p(vec![Run::control(textbox, CharShapeIndex::new(0))])],
            u(1000),
        )])]);
        let md = encode_without_template(&validated_document(vec![p(vec![Run::table(
            outer,
            CharShapeIndex::new(0),
        )])]))
        .unwrap();
        assert_eq!(gfm_blocks(&md), vec![GfmBlock::Table(vec![vec![r"a\b|c".into()]])]);
    }

    #[test]
    fn lossy_table_depth_survives_textbox_and_note_boundaries() {
        // 이것을 실패시키는 것: 글상자/각주 재귀에서 표 깊이를 0으로 초기화.
        use hwpforge_core::{TableCell, TableRow};
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let p = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        for count in [8, 9] {
            let mut run = Run::text("deep cell", CharShapeIndex::new(0));
            for index in 0..count {
                let paragraphs = vec![p(vec![run])];
                let container = if index % 2 == 0 {
                    Control::text_box(paragraphs, u(1000), u(1000))
                } else {
                    Control::footnote(paragraphs)
                };
                run = Run::table(
                    Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![p(vec![Run::control(container, CharShapeIndex::new(0))])],
                        u(1000),
                    )])]),
                    CharShapeIndex::new(0),
                );
            }
            let (md, warnings) =
                encode_without_template_report(&validated_document(vec![p(vec![run])])).unwrap();
            assert_eq!(md.contains("deep cell"), count == 8, "{md}");
            assert_eq!(md.contains(TABLE_TOO_DEEP), count == 9);
            assert_eq!(
                warnings
                    .iter()
                    .filter(|w| matches!(w, super::super::MdWarning::ContainerTooDeep { .. }))
                    .count(),
                usize::from(count == 9)
            );
        }
    }

    /// A document nested past the Core limit is refused during validation;
    /// at the limit it is drawn. Notes exercise the container limit
    /// independently of the smaller table rendering budget.
    ///
    /// 이것을 실패시키는 것: Core validate의 깊이 검사 삭제.
    #[test]
    fn validation_limits_lossy_input_depth() {
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
            d
        };
        let accepted = nest(32);
        assert!(encode_without_template(&accepted.validate().unwrap()).is_ok());
        let rejected = nest(33);
        assert!(rejected.validate().is_err());
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

    /// Lossy escapes the document's text only where CommonMark would read
    /// markup — raw HTML and autolinks (`<` before printable ASCII), entity
    /// references, inline link and image labels (`](`), link reference
    /// definitions — judged across runs, and keeps the rest as it is.
    ///
    /// 이것을 실패시키는 것: 규칙(`<`·`&`·`](`·줄 머리 `[…]:`) 하나씩 삭제 · 역슬래시
    /// 겹치기 삭제 · run 마다 따로 판정(모아 두기 삭제) · 칸에서 두 번 escape ·
    /// 다음 마크업 첫 글자를 보지 않기 · 그림 alt·경로 escape 삭제.
    #[test]
    fn lossy_text_escapes_only_markup() {
        let body = |runs: Vec<Run>| {
            encode_without_template(&validated_document(vec![Paragraph::with_runs(
                runs,
                ParaShapeIndex::new(0),
            )]))
            .unwrap()
        };
        let one = |text: &str| body(vec![Run::text(text, CharShapeIndex::new(0))]);
        for (input, expected) in [
            ("<img src=x onerror=y>", "\\<img src=x onerror=y>"),
            ("</b>", "\\</b>"),
            ("<!-- c -->", "\\<!-- c -->"),
            ("<?x ?>", "\\<?x ?>"),
            ("<https://e.invalid>", "\\<https://e.invalid>"),
            ("<a@b.c>", "\\<a@b.c>"),
            ("&amp; &#60; &#x3c;", "\\&amp; \\&#60; \\&#x3c;"),
            ("[x](javascript:y)", "[x\\](javascript:y)"),
            ("![x](y)", "![x\\](y)"),
            ("[a]: javascript:x", "\\[a]: javascript:x"),
            ("\\<img>", "\\\\\\<img>"),
            ("<b>x</b>", "\\<b>x\\</b>"),
            ("<a href=\"x\" title='y' z>", "\\<a href=\"x\" title='y' z>"),
            ("<br/>", "\\<br/>"),
            ("<![CDATA[x]]>", "\\<![CDATA[x]]>"),
            ("<!DOCTYPE x>", "\\<!DOCTYPE x>"),
            ("<div 가나", "\\<div 가나"),
            ("<DIV", "\\<DIV"),
            // Kept: not markup to CommonMark.
            (
                "<붙임> <표 1> a < b [참고] AT&T & *x* # h [a] (b)",
                "<붙임> <표 1> a < b [참고] AT&T & *x* # h [a] (b)",
            ),
            (
                "<AI가 바꾼 것> <AI Agent 기반 공방> <b 가> <x=1> <b_x>",
                "<AI가 바꾼 것> <AI Agent 기반 공방> <b 가> <x=1> <b_x>",
            ),
        ] {
            assert_eq!(one(input), expected, "{input}");
        }
        let t = |s: &str| Run::text(s, CharShapeIndex::new(0));
        assert_eq!(body(vec![t("[a]"), t("(javascript:x)")]), "[a\\](javascript:x)");
        assert_eq!(body(vec![t("<"), t("img>")]), "\\<img>");
        // In a cell, escaped once with the cell's own escaping.
        let cell = TableCell::new(
            vec![Paragraph::with_runs(vec![t("<img>|\\")], ParaShapeIndex::new(0))],
            hwpforge_foundation::HwpUnit::new(1000).unwrap(),
        );
        let table = Run::table(
            hwpforge_core::Table::new(vec![TableRow::new(vec![cell])]),
            CharShapeIndex::new(0),
        );
        assert_eq!(body(vec![table]), "| \\<img>\\|\\\\ |\n| --- |");
        // What follows the text decides at its end: `<div` before the space
        // ahead of a picture opens an HTML block.
        let pic = || {
            Run::image(
                hwpforge_core::Image::new(
                    "p.png",
                    hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
                    hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
                    hwpforge_core::ImageFormat::Png,
                ),
                CharShapeIndex::new(0),
            )
        };
        assert_eq!(body(vec![t("<div"), pic()]), "\\<div ![p](p.png)");
        assert_eq!(body(vec![t("<b"), pic()]), "<b ![p](p.png)");
        let link = Run::control(
            Control::Hyperlink { text: "t".into(), url: "https://e.invalid".into() },
            CharShapeIndex::new(0),
        );
        assert_eq!(body(vec![t("<div"), link.clone()]), "<div[t](https://e.invalid)");
        // A lone backslash before a link is escaped, so the link stays one.
        // 이것을 실패시키는 것: `Md::push` 의 끝 역슬래시 처리 삭제.
        assert_eq!(body(vec![t("x\\"), link.clone()]), "x\\\\[t](https://e.invalid)");
        // `!` before a link is escaped once: an escaped one stays as it is.
        // 이것을 실패시키는 것: `join` 의 `!` 앞 역슬래시 짝수 검사 삭제.
        assert_eq!(body(vec![t("!"), link.clone()]), "\\![t](https://e.invalid)");
        assert_eq!(body(vec![t("\\!"), link]), "\\![t](https://e.invalid)");
        // A picture's alt cannot close its label and its path cannot end early.
        let image = hwpforge_core::Image::new(
            "x/a](b).png",
            hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
            hwpforge_foundation::HwpUnit::from_mm(5.0).unwrap(),
            hwpforge_core::ImageFormat::Png,
        );
        assert_eq!(
            body(vec![Run::image(image.clone(), CharShapeIndex::new(0))]),
            "![a\\](b)](x/a]\\(b\\).png)"
        );
        assert_eq!(
            body(vec![t("x"), Run::image(image, CharShapeIndex::new(0))]),
            "x ![a\\](b)](x/a]\\(b\\).png)"
        );
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
        // The destination cannot end early: no whitespace, so the URL passes.
        assert_eq!(
            doc("https://a)<img/src=x/onerror=y>"),
            "[t](https://a\\)\\<img/src=x/onerror=y\\>)"
        );
    }
}
