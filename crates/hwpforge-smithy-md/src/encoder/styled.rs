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

use super::guard::{Block, Source, MEMO_CLOSE, MEMO_OPEN, TOO_DEEP};
use super::list_format::{format_list_continuation, format_list_item};
use super::markup::{Cell, Html, Md};
use super::{MdWarning, Part};
use crate::eqn::{render_equation, EqnRender};
use crate::internal_styles::parse_list_continuation_style_name;

/// Output of style-aware markdown encoding.
///
/// Contains the generated markdown string, any extracted images (keyed by
/// their relative path within the output) and what the rendering left out.
/// Built only by the encoder (`#[non_exhaustive]`): read its fields, or
/// destructure with `..` (`let MdOutput { markdown, images, .. } = out;`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MdOutput {
    /// The generated markdown string.
    pub markdown: String,
    /// Extracted images: relative path → binary data.
    pub images: HashMap<String, Vec<u8>>,
    /// What the rendering left out or wrote differently — an equation
    /// written as its script text ([`super::MdWarning::EquationAsText`]);
    /// empty for most documents.
    pub warnings: Vec<super::MdWarning>,
}

#[derive(Debug, Clone, Copy)]
struct ListContinuationContext {
    level: u8,
}

// ---------------------------------------------------------------------------
// Footnote/Endnote collector
// ---------------------------------------------------------------------------

/// Collects the document's footnote and endnote definitions (`[^N]`,
/// `[^eN]`) during encoding and renders them at the document end. Only notes
/// met at note depth 0 outside a memo are registered (see [`Ctx`]).
struct FootnoteCollector {
    /// Per-note body paragraphs (multi-paragraph definitions).
    footnotes: Vec<Note>,
    endnotes: Vec<Note>,
}

/// A note definition: its body, and its raw text and place for the final
/// check's plain-text fallback ([`super::guard`]).
struct Note {
    body: Vec<Md>,
    raw: String,
    at: (usize, usize),
}

impl FootnoteCollector {
    fn new() -> Self {
        Self { footnotes: Vec::new(), endnotes: Vec::new() }
    }

    /// Adds a footnote body and returns the inline marker `[^N]`.
    fn add_footnote(&mut self, body: Vec<Md>, raw: String, at: (usize, usize)) -> Md {
        self.footnotes.push(Note { body, raw, at });
        note_marker("", self.footnotes.len())
    }

    /// Adds an endnote body and returns the inline marker `[^eN]`.
    fn add_endnote(&mut self, body: Vec<Md>, raw: String, at: (usize, usize)) -> Md {
        self.endnotes.push(Note { body, raw, at });
        note_marker("e", self.endnotes.len())
    }

    /// Renders all collected definitions as a markdown block.
    ///
    /// Multi-paragraph bodies follow the GFM continuation form the parser
    /// actually accepts (probe): a **blank line** between paragraphs
    /// and 4-space indentation on **every** physical line of continuation
    /// paragraphs. Line-start block markers are escaped so a body that
    /// *looks* like a list stays plain text on re-parse (probe).
    fn render_definitions(self) -> Vec<Block> {
        let labels = (1..).map(|i| i.to_string());
        let footnotes = self.footnotes.into_iter().zip(labels);
        let endnotes = self.endnotes.into_iter().zip((1..).map(|i| format!("e{i}")));
        footnotes
            .chain(endnotes)
            .map(|(note, label)| Block {
                markdown: render_definition(&label, &note.body),
                source: Some(Source {
                    prefix: format!("[^{label}]: "),
                    texts: vec![note.raw],
                    at: vec![note.at],
                }),
                body: false,
            })
            .collect()
    }
}

/// `[^N]` (`prefix` "") or `[^eN]` (`prefix` "e").
fn note_marker(prefix: &'static str, n: usize) -> Md {
    let mut m = Md::syntax("[^");
    m.push_syntax(prefix);
    m.push(Md::number(n));
    m.push_syntax("]");
    m
}

/// Renders one `[^label]:` definition with GFM multi-paragraph continuation.
fn render_definition(label: &str, paragraphs: &[Md]) -> String {
    let mut out = String::new();
    for (i, para) in paragraphs.iter().enumerate() {
        let escaped = escape_footnote_paragraph(&comments_after_text(para.as_str()));
        if i == 0 {
            out.push_str(&format!("[^{label}]: {escaped}"));
        } else {
            out.push_str("\n\n");
            for (j, line) in escaped.split('\n').enumerate() {
                if j > 0 {
                    out.push('\n');
                }
                out.push_str("    ");
                out.push_str(line);
            }
        }
    }
    out
}

/// Escapes line-start block markers inside a note-definition paragraph.
///
/// A body line beginning with `- ` / `* ` / `+ ` / `1. ` / `#` / `>` would
/// re-parse as a structural block inside the definition, which this decoder
/// rejects — the emitter must keep such text literal. A tab after a marker
/// opens a list too: there the `\` goes before the punctuation (`1\.\t`,
/// a `\` before a digit would be a character) and only 1–9 digits are a
/// marker. With a space the output is the base's (`\1. `, notes are
/// outside the guarantee: `markup_safety_base.txt`).
/// 이것을 실패시키는 것: 탭 무시 · 탭 뒤 숫자 앞에 `\` · 10자리 숫자를 목록으로
/// (`tab_after_a_marker_makes_no_block`).
fn escape_footnote_paragraph(paragraph: &str) -> String {
    paragraph
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_start();
            let at = if matches!(trimmed.as_bytes(), [b'-' | b'*' | b'+', b' ' | b'\t', ..])
                || trimmed.starts_with('#')
                || trimmed.starts_with('>')
            {
                Some(0)
            } else {
                ordered_marker_escape(trimmed)
            };
            match at {
                Some(at) => {
                    let at = line.len() - trimmed.len() + at;
                    format!("{}\\{}", &line[..at], &line[at..])
                }
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the `\` goes in a line starting `1. ` / `23) ` (or ending there):
/// before it, as the base wrote; with a tab after the marker, before the
/// punctuation of a 1–9 digit marker.
fn ordered_marker_escape(s: &str) -> Option<usize> {
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || !matches!(s.as_bytes().get(digits), Some(b'.' | b')')) {
        return None;
    }
    match s.as_bytes().get(digits + 1) {
        Some(b' ') | None => Some(0),
        Some(b'\t') if digits <= 9 => Some(digits),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Render context and the document's state
// ---------------------------------------------------------------------------

/// Where a piece of content is being drawn. Every recursion passes it on;
/// the places that change it are a note body (`note_depth + 1`), a memo
/// body (`in_comment`) and the guarantee (`guarded`). Markdown or HTML is not part of it: each has its own
/// renderer and output type ([`Md`], [`Html`]).
/// Text boxes, shapes and groups pass it on unchanged.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    /// How many note bodies enclose this content. Only depth 0 registers a
    /// note definition; deeper notes are written inline as `(주: …)`.
    note_depth: u32,
    /// Inside a memo (`<!-- memo: … -->`): pictures are written as link
    /// text only (no asset), notes inline.
    in_comment: bool,
    /// How many containers (tables, captions, shape texts, note and memo
    /// bodies) enclose this content — each one is a level of recursion. A
    /// group or a picture is not one (groups are spread by an iterator), so
    /// this differs from the semantic depth checked by Core validation.
    depth: usize,
    /// Document text drawn here must make no block structure: a top-level
    /// Body paragraph that is not a continuation, and the table of a
    /// top-level one-table paragraph that is not one. Its text goes through
    /// [`Md::guarded_text`] and a table it draws ends its block
    /// ([`Md::ending_block`]). Set in [`encode_paragraph_styled`] only; a
    /// memo, a note body and a one-table or one-picture paragraph's caption
    /// turn it off; everything else passes it on. Outside it, output is as
    /// before.
    guarded: bool,
    /// Drawing a GFM table cell: inline content, so the
    /// emphasis check reads it in a cell ([`emphasis_kept`]) and a comment
    /// opens no HTML block.
    cell: bool,
}

/// Document text in Markdown as the context needs it: [`Md::guarded_text`]
/// where text is guarded, [`Md::text`] elsewhere. Every place the styled
/// view writes document text that can start a line goes through it (link
/// labels and picture alt text cannot: they follow `[` and hold no line
/// ending).
/// 이것을 실패시키는 것: `guarded` 무시(`guarded_text_reaches_every_carrier`).
fn text(raw: &str, ctx: Ctx) -> Md {
    if ctx.guarded {
        Md::guarded_text(raw)
    } else {
        Md::text(raw)
    }
}

impl Ctx {
    /// The context one container deeper, or `None` past the limit.
    /// Validated inputs cannot exceed this defensive rendering budget: Core
    /// validation admits 32 levels and counts each container drawn here as at
    /// least one. So the callers write [`TOO_DEEP`] there without a warning.
    fn enter(self) -> Option<Self> {
        super::guard::enter_depth(self.depth).map(|depth| Self { depth, ..self })
    }
}

/// What the whole document collects while it is drawn: the note
/// definitions and the image assets.
struct State {
    destinations: std::collections::HashSet<String>,
    notes: FootnoteCollector,
    assets: Assets,
    /// What was left out, with where (`at`).
    warnings: Vec<super::MdWarning>,
    /// The section and top-level paragraph being drawn.
    at: (usize, usize),
    /// The paragraphs already reported as written as plain text
    /// ([`super::guard::written_as_text`]).
    warned: std::collections::HashSet<(usize, usize)>,
    /// The reference registered for each note occurrence of the document
    /// (its address, [`State::note_at`]), which a fallback keeps
    /// ([`fallback`]).
    note_marks: HashMap<usize, Md>,
    /// A copy's note occurrences and the document's they stand for
    /// ([`alias_copy`]).
    note_alias: HashMap<usize, usize>,
    /// The copies drawn ([`expanded`]), kept so their addresses stay theirs.
    copies: Vec<std::rc::Rc<Paragraph>>,
}

impl State {
    /// Keeps a copy drawn from until the document is drawn.
    fn keep(&mut self, copy: Paragraph) -> std::rc::Rc<Paragraph> {
        let copy = std::rc::Rc::new(copy);
        self.copies.push(copy.clone());
        copy
    }

    /// The note occurrence `at` stands for ([`alias_copy`]).
    fn note_at(&self, at: usize) -> usize {
        self.note_alias.get(&at).copied().unwrap_or(at)
    }

    fn new() -> Self {
        Self {
            destinations: Default::default(),
            notes: FootnoteCollector::new(),
            assets: Assets::default(),
            warnings: Vec::new(),
            at: (0, 0),
            warned: Default::default(),
            note_marks: HashMap::new(),
            note_alias: HashMap::new(),
            copies: Vec::new(),
        }
    }
}

/// The image assets (`MdOutput::images`): one key per source picture
/// (`Image::path`), assigned in drawing order. A link and its asset are
/// decided in the same call, so every link written outside a memo leads to
/// its own picture's bytes or to no file at all.
#[derive(Default)]
struct Assets {
    images: HashMap<String, Vec<u8>>,
    /// Keys used by links whose picture has no bytes: no file may take them.
    link_only: std::collections::HashSet<String>,
    /// The key each registered source picture got: the same source always
    /// gets the same one, and a fallback writes it ([`Kept`]).
    drawn: HashMap<String, String>,
    /// Per candidate key, its keys already found taken ([`Taken`]).
    taken: HashMap<String, Taken>,
}

/// The first `len` keys of a candidate (`a.png`, `a-2.png` …), all taken —
/// a taken key stays taken, with the same bytes — with where each content
/// first sits among them and the first taken only by a link without bytes,
/// so a picture never walks them again (one walk per key in all).
/// 이것을 실패시키는 것: 매번 처음부터 걷기 · 같은 바이트·바이트 없는 키 조회 삭제
/// (`colliding_pictures_take_linear_time`, `asset_keys_match_the_full_walk`).
#[derive(Default)]
struct Taken {
    len: u32,
    bytes: HashMap<u64, Vec<u32>>,
    link_only: Option<u32>,
}

fn bytes_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

impl Assets {
    /// The key a picture's link uses, registering its bytes. A key already
    /// holding the same bytes is reused (the same source drawn again, or
    /// another with equal bytes); a key holding other bytes, or taken by a
    /// link without bytes, moves on to `-2`, `-3` … before the extension, so
    /// the walk always lands on the same key for the same source. A picture
    /// without bytes gets a key no file holds and keeps it for itself. In a
    /// memo (`register` false) the link is comment text and uses the plain
    /// key.
    /// 이것을 실패시키는 것: 충돌 시 덮어쓰기 · 접미사 규칙 바꾸기 · 바이트 없는 링크가
    /// 남의 파일 키를 쓰기 · 파일이 바이트 없는 링크의 키를 쓰기
    /// (`asset_keys_never_overwrite`).
    fn key(&mut self, path: &str, styles: &dyn StyleLookup, register: bool) -> String {
        let candidate = image_rel_path(path, styles);
        if !register {
            return candidate;
        }
        let data = styles.image_data(path);
        let key = self.free_key(&candidate, data);
        self.drawn.insert(path.to_string(), key.clone());
        match data {
            Some(d) => {
                self.images.entry(key.clone()).or_insert_with(|| d.to_vec());
            }
            None => {
                self.link_only.insert(key.clone());
            }
        }
        key
    }

    /// The first of `candidate`'s keys a picture with `data` may use: one
    /// holding the same bytes, or a free one (not taken by a link without
    /// bytes); without bytes, the first key no file holds. The keys already
    /// found taken are looked up ([`Taken`]), the rest walked once.
    fn free_key(&mut self, candidate: &str, data: Option<&[u8]>) -> String {
        let at = |n: u32| if n == 1 { candidate.to_string() } else { suffixed_key(candidate, n) };
        let taken = self.taken.entry(candidate.to_string()).or_default();
        let hash = data.map(bytes_hash);
        let reused = match (data, hash) {
            (Some(d), Some(h)) => taken.bytes.get(&h).and_then(|ns| {
                ns.iter().copied().find(|&n| self.images.get(&at(n)).is_some_and(|e| e == d))
            }),
            _ => taken.link_only,
        };
        if let Some(n) = reused {
            return at(n);
        }
        let mut n = taken.len + 1;
        loop {
            let key = at(n);
            match (self.images.get(&key), data) {
                (Some(existing), Some(d)) if existing.as_slice() == d => return key,
                (None, Some(_)) if !self.link_only.contains(&key) => return key,
                (None, None) => return key,
                (existing, _) => {
                    // Taken, right after the keys already found taken.
                    taken.len = n;
                    match existing {
                        Some(e) => taken.bytes.entry(bytes_hash(e)).or_default().push(n),
                        None => {
                            taken.link_only.get_or_insert(n);
                        }
                    }
                    n += 1;
                }
            }
        }
    }
}

/// `images/a.png` → `images/a-2.png` (`images/a` → `images/a-2`).
fn suffixed_key(key: &str, n: u32) -> String {
    let name_start = key.rfind('/').map_or(0, |i| i + 1);
    match key[name_start..].rfind('.') {
        Some(dot) if dot > 0 => {
            let at = name_start + dot;
            format!("{}-{n}{}", &key[..at], &key[at..])
        }
        _ => format!("{key}-{n}"),
    }
}

/// A picture's link in Markdown, `![alt](key)` (the stem as Markdown text,
/// the key written to read back exactly), with its asset registered unless
/// inside a memo.
fn emit_picture(
    image: &hwpforge_core::image::Image,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Md {
    // 이것을 실패시키는 것: 빈 경로 가드 삭제(빈 그림 링크/자산 생성).
    if image.path.is_empty() {
        return Md::syntax("");
    }
    let key = st.assets.key(&image.path, styles, !ctx.in_comment);
    Md::image(Md::text(image_stem(&image.path)), &key, &mut st.destinations)
}

/// [`emit_picture`] in an HTML cell: `<img src="key" alt="stem"/>`, both
/// attribute-escaped (the plain file stem as alt — no Markdown escaping).
/// 이것을 실패시키는 것: src·alt 이스케이프 삭제 · alt 에 Markdown 이스케이프
/// (`html_picture_attributes_are_escaped`).
fn emit_picture_html(
    image: &hwpforge_core::image::Image,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Html {
    // 이것을 실패시키는 것: 빈 경로 가드 삭제(HTML 표에 빈 img 생성).
    if image.path.is_empty() {
        return Html::syntax("");
    }
    let key = st.assets.key(&image.path, styles, !ctx.in_comment);
    Html::image(&key, image_stem(&image.path), &mut st.destinations)
}

/// Encodes a validated document into style-aware markdown.
///
/// Queries the provided [`StyleLookup`] for character/paragraph/style
/// properties to emit inline formatting and heading markers.
pub(crate) fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
    let mut blocks: Vec<Block> = Vec::new();
    let mut st = State::new();
    let mut measured = std::collections::BTreeSet::new();
    hwpforge_core::field_inventory::FieldInventory::visit_flows(
        document.sections(),
        |_, paragraphs, _| {
            for paragraph in paragraphs {
                let id = paragraph.para_shape_id;
                if !measured.insert(id.get()) {
                    continue;
                }
                for (field, measure) in [
                    ("margin_left", styles.para_indent_left(id)),
                    ("indent", styles.para_indent_first_line(id)),
                ] {
                    if let Some(measure) =
                        measure.filter(|m| m.as_hwp_unit().is_none_or(|v| v.as_i32() != 0))
                    {
                        st.warnings.push(MdWarning::ParagraphMeasureLost {
                            definition: id.get(),
                            field: field.into(),
                            measure,
                        });
                    }
                }
            }
        },
    );
    // The plain-text fallback of a block: its paragraphs' raw text.
    let source = |texts: Vec<String>, at: Vec<(usize, usize)>| {
        Some(Source { prefix: String::new(), texts, at })
    };

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(Block {
                markdown: super::guard::SECTION_MARKER.to_string(),
                source: None,
                body: false,
            });
        }

        // A code block's lines and their paragraphs.
        let mut code: Vec<(String, (usize, usize))> = Vec::new();
        let flush_code = |code: &mut Vec<(String, (usize, usize))>, blocks: &mut Vec<Block>| {
            if !code.is_empty() {
                let lines: Vec<String> = code.iter().map(|(t, _)| t.clone()).collect();
                let at = code.iter().map(|(_, at)| *at).collect();
                blocks.push(Block {
                    markdown: Md::code_block(&lines.join("\n")).into_string(),
                    source: source(lines.iter().map(|l| super::guard::plain_text(l)).collect(), at),
                    body: false,
                });
                code.clear();
            }
        };
        let mut continuation: Option<ListContinuationContext> = None;

        for (paragraph_index, paragraph) in section.paragraphs.iter().enumerate() {
            st.at = (section_index, paragraph_index);
            // Page break
            if paragraph.page_break {
                flush_code(&mut code, &mut blocks);
                blocks.push(Block { markdown: "---".to_string(), source: None, body: false });
                continuation = None;
            }

            // Code block detection: all text runs with code font.
            // InlineText runs (inline-tab carry) fold to
            // their `\t`-bearing plain string.
            if is_code_paragraph(paragraph, styles) {
                let text = paragraph
                    .runs
                    .iter()
                    .filter_map(|r| r.content.plain_text())
                    .map(|cow| cow.into_owned())
                    .collect::<String>();
                code.push((text, st.at));
                continuation = None;
                continue;
            }
            flush_code(&mut code, &mut blocks);

            let continuation_level = continuation
                .and_then(|ctx| continuation_level_for_paragraph(paragraph, styles, ctx));
            let (markdown, body) =
                encode_paragraph_styled(paragraph, styles, &mut st, continuation_level.is_some());
            if !markdown.trim().is_empty() {
                let text =
                    fallback(std::slice::from_ref(paragraph), 0, &mut st).remove(0).into_string();
                // A continuation is its own block (`join` writes the same
                // blank line before it as appending did), so it can fall
                // back alone and a Body block's check never sees it.
                // 이것을 실패시키는 것: 직전 블록에 붙이기
                // (`continuation_is_its_own_block`).
                let markdown = match continuation_level {
                    Some(level) => format_list_continuation(&markdown, level),
                    None => markdown,
                };
                blocks.push(Block { markdown, source: source(vec![text], vec![st.at]), body });
            }

            if continuation_level.is_none() {
                continuation = list_context_for_paragraph(paragraph, styles);
            }
        }

        // Flush remaining code block at section end
        flush_code(&mut code, &mut blocks);
    }

    // Footnote/endnote definitions at the end.
    let notes = std::mem::replace(&mut st.notes, FootnoteCollector::new());
    blocks.extend(notes.render_definitions());

    // The finished Markdown, parsed again, may hold only what styled writes.
    let allowed = super::guard::Allowed {
        tags: super::guard::STYLED_TAGS,
        destinations: &st.destinations,
        decoder_structure: true,
    };
    let markdown = super::guard::checked(blocks, &allowed, &mut st.warnings);

    MdOutput { markdown, images: st.assets.images, warnings: st.warnings }
}

/// Encodes a single top-level paragraph into styled markdown, and whether
/// it was drawn as a Body block: not a `continuation`, drawn by the body
/// branch (not as a heading, a list item, or a one-table or one-picture
/// paragraph). The answer comes from the branch that drew it, and is the
/// paragraph's `Ctx::guarded`.
/// 이것을 실패시키는 것: 단일 표·그림을 Body 로(`out_of_scope_is_base_output`) ·
/// continuation 을 Body 로(`continuation_is_its_own_block`).
fn encode_paragraph_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    continuation: bool,
) -> (String, bool) {
    let ctx = Ctx::default();
    // Handle single-run block elements (table, image) as standalone blocks.
    // A paragraph whose one run is a group is not one of them, even when the
    // group holds a single picture (WS: the group is drawn as its children).
    // The caption is not guarded (`Ctx::guarded`); a table that is not a
    // continuation is.
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(table) => {
                let table_ctx = Ctx { guarded: !continuation, ..ctx };
                let mut md = table_to_styled_markdown(table, styles, false, st, table_ctx);
                md.push(styled_caption(table.caption.as_ref(), styles, st, ctx));
                return (md.into_string(), false);
            }
            RunContent::Image(image) => {
                let mut md = emit_picture(image, styles, st, ctx);
                md.push(styled_caption(image.caption.as_ref(), styles, st, ctx));
                return (md.into_string(), false);
            }
            _ => {}
        }
    }

    // Shared outline/list classification (hwpforge-core). Classification is
    // text-blind, so it decides the context before drawing; the empty-text
    // guards below are rendering concerns (an empty heading/list item
    // renders as nothing).
    let kind = classify_paragraph(paragraph, styles);
    let body = !continuation && matches!(kind, ParaKind::Body);
    let text =
        paragraph_text_styled(paragraph, styles, st, Ctx { guarded: body, ..ctx }).into_string();
    let markdown = match kind {
        ParaKind::Heading { level, .. } => {
            // Headings must be single-line: collapse lineBreak-originated newlines.
            let heading_text = text.trim().replace('\n', " ");
            if heading_text.is_empty() {
                return (String::new(), false);
            }
            format!("{} {}", "#".repeat(level as usize), heading_text)
        }
        ParaKind::ListItem { kind, level, checked, .. } => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return (String::new(), false);
            }
            format_list_item(comments_after_text(trimmed).trim(), kind, level, checked)
        }
        // A continuation is out of the guarantee: as before (G.3 (라)).
        // 이것을 실패시키는 것: continuation 에도 줄 끝내기
        // (`continuation_line_start_comment_is_base_output`).
        ParaKind::Body if body => comment_ends_line(text.trim_start()).0,
        ParaKind::Body => comments_after_text(text.trim_start()),
    };
    (markdown, body)
}

/// A Body block with each comment the encoder wrote at a line start (a
/// memo or chart placeholder, `<!-- … -->`) ending its line: the text after
/// it starts the next line — a new paragraph, the comment's HTML block
/// ending there — its leading whitespace dropped as at a
/// paragraph's start. A comment at a line start opens an HTML block that
/// would take the rest of the line in as raw HTML. Document text cannot
/// start a line with `<!--` (`<` is written `&lt;`), and a memo's body has
/// its `--` broken up, so the first `-->` after it is the encoder's. It
/// runs on the finished block, so it sees every byte (a table's blank line,
/// a caption's last comment). With the block, where it changed bytes: each
/// `(start, end, shift)` replaced `block[start..end]` and moved what
/// follows by `shift` in all (the emphasis check reads the same text).
/// 이것을 실패시키는 것: 줄 끝내기 삭제 · 뒤 글의 앞 공백 남기기
/// (`body_check_and_table_end_close_the_rest`, `line_start_memo_keeps_comment_and_text`).
fn comment_ends_line(block: &str) -> (String, Vec<(usize, usize, isize)>) {
    let mut out = String::with_capacity(block.len() + 8);
    let mut edits = Vec::new();
    let mut shift = 0isize;
    let mut rest = block;
    // Each step consumes what it looks at, so the walk is linear even for
    // many comments on one line.
    // 이것을 실패시키는 것: 꼬리를 줄 끝까지 다시 훑기(`line_start_comments_are_linear`).
    while !rest.is_empty() {
        let at = block.len() - rest.len();
        let close = if rest.trim_start_matches([' ', '\t']).starts_with("<!--") {
            rest.find("-->")
        } else {
            None
        };
        if let Some(end) = close {
            let after = &rest[end + 3..];
            let tail = after.trim_start_matches(|c: char| c != '\n' && c.is_whitespace());
            out.push_str(&rest[..end + 3]);
            if tail.is_empty() || tail.starts_with('\n') {
                out.push_str(&after[..after.len() - tail.len()]);
            } else {
                out.push('\n');
                let (start, gone) = (at + end + 3, after.len() - tail.len());
                shift += 1 - gone as isize;
                edits.push((start, start + gone, shift));
            }
            rest = tail;
        } else {
            let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
            out.push_str(&rest[..line_end]);
            rest = &rest[line_end..];
        }
    }
    (out, edits)
}

/// Text outside a Body block (a list item, a list continuation, a note
/// definition's paragraph, a caption) with each comment that starts a line
/// (a memo or chart placeholder) moved to the end of the text, in order.
/// A comment at a line start opens an HTML block that would take the rest
/// of the line in as raw HTML; here, unlike a Body block
/// ([`comment_ends_line`]), a line after it would also leave the item or
/// definition. The comment is left out when read back, so only its place
/// moves; a line that held only comments goes, so a text of comments only
/// comes out as it went in.
/// 이것을 실패시키는 것: 이 함수를 거치지 않기 · 주석만 남은 줄 남기기
/// (`comment_led_items_and_notes_read_back`).
fn comments_after_text(text: &str) -> String {
    let mut moved = String::new();
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let mut rest = line;
        let mut took = false;
        while let Some(end) = rest
            .trim_start_matches([' ', '\t'])
            .strip_prefix("<!--")
            .and_then(|body| body.find("-->"))
        {
            let start = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            moved.push_str(&rest[start..start + end + 7]);
            rest = &rest[start + end + 7..];
            took = true;
        }
        if !(took && rest.trim().is_empty()) {
            lines.push(if took { rest.trim_start() } else { line });
        }
    }
    let mut out = lines.join("\n");
    out.push_str(&moved);
    out
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

/// A paragraph in Markdown with inline formatting applied (`ctx.html` is
/// ignored: this is the Markdown renderer).
fn paragraph_text_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Md {
    if ctx.guarded {
        let mut line = Line::default();
        guarded_runs(paragraph, styles, st, ctx, &mut line);
        let (md, kept) = line.finish(ctx);
        if kept {
            return md;
        }
        // The markers could not be placed so every character keeps its
        // emphasis: this Body paragraph as plain text. Cells finish and
        // fall back locally in `table_to_gfm`; captions join the parent.
        // 이것을 실패시키는 것: 블록 전체로 넓히기(`emphasis_fallback_stays_in_its_paragraph`).
        super::guard::written_as_text(&mut st.warnings, &mut st.warned, st.at);
        return fallback(std::slice::from_ref(paragraph), ctx.depth, st).remove(0);
    }
    let mut kept = None;
    let paragraph = expanded(paragraph, st, &mut kept);
    let mut output = Md::new();

    // Group consecutive text runs by InlineFormat for proper wrapping.
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for (part, cs) in super::spread_groups(&paragraph.runs) {
        // Anything but text first flushes the text group.
        if !matches!(part, Part::Text(_)) && !current_text.is_empty() {
            output.push(current_format.wrap(&current_text, ctx));
            current_text.clear();
            current_format = InlineFormat::default();
        }
        match part {
            // InlineText folds to the same `\t`-bearing string via
            // `plain_text()` because Markdown styling has no
            // representation for `<hp:tab>` attributes. The
            // formatting group key still uses `char_shape_id`, so the
            // run gets wrapped exactly like a plain `Text(String)` run.
            Part::Text(content) => {
                let Some(cow) = content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(cs, styles);
                if fmt != current_format {
                    if !current_text.is_empty() {
                        output.push(current_format.wrap(&current_text, ctx));
                        current_text.clear();
                    }
                    current_format = fmt;
                }
                current_text.push_str(&cow);
            }
            Part::Image(image) => {
                if !image.path.is_empty() && !output.is_empty() {
                    output.push_syntax(" ");
                }
                output.push(emit_picture(image, styles, st, ctx));
                output.push(styled_caption(image.caption.as_ref(), styles, st, ctx));
            }
            Part::Table(table) => {
                if !output.is_empty() {
                    output.push_syntax("\n");
                }
                output.push(table_to_styled_markdown(table, styles, false, st, ctx));
                output.push(styled_caption(table.caption.as_ref(), styles, st, ctx));
            }
            Part::Control(control) => output.push(control_md(control, styles, st, ctx)),
        }
    }

    // Flush remaining text.
    if !current_text.is_empty() {
        output.push(current_format.wrap(&current_text, ctx));
    }

    output
}

/// A guarded paragraph's runs collected into `line`, in drawing order:
/// text by its format, everything else drawn as it is. A text box's or
/// shape's text and a memo's anchor (a memo left in another memo's anchor)
/// are drawn into the same line, one container deeper, so their emphasis
/// is written together with the text around them ([`Line::finish`]).
/// 이것을 실패시키는 것: 글상자 글을 따로 그리기(`adjacent_formatted_text_keeps_its_characters`) ·
/// 메모 앵커를 따로 그리기(`memo_anchor_follows_the_renderer`).
fn guarded_runs(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
    line: &mut Line,
) {
    let mut kept = None;
    let paragraph = expanded(paragraph, st, &mut kept);
    for (part, cs) in super::spread_groups(&paragraph.runs) {
        match part {
            Part::Text(content) => {
                if let Some(cow) = content.plain_text() {
                    line.text(&cow, InlineFormat::from_style(cs, styles));
                }
            }
            Part::Image(image) => {
                let mut picture = Line::default();
                picture.raw(emit_picture(image, styles, st, ctx));
                picture.joined(guarded_caption(image.caption.as_ref(), styles, st, ctx));
                if !image.path.is_empty() && !line.is_empty() {
                    line.raw(Md::syntax(" "));
                }
                line.joined(picture);
            }
            Part::Table(table) => {
                if !line.is_empty() {
                    line.raw(Md::syntax("\n"));
                }
                // A guarded table ends its block: text after it starts a new
                // one instead of joining the table (an HTML block runs to a
                // blank line, GFM rows take the next line).
                // 이것을 실패시키는 것: 표시 삭제 · 범위 밖에도 표시
                // (`body_check_and_table_end_close_the_rest`, `out_of_scope_is_base_output`).
                line.raw(table_to_styled_markdown(table, styles, false, st, ctx).ending_block());
                line.joined(guarded_caption(table.caption.as_ref(), styles, st, ctx));
            }
            Part::Control(
                Control::TextBox { paragraphs, .. }
                | Control::Ellipse { paragraphs, .. }
                | Control::Polygon { paragraphs, .. },
            ) => {
                let Some(inner) = ctx.enter() else {
                    line.raw(Md::syntax(TOO_DEEP));
                    continue;
                };
                // Joined by a space and trimmed, as `shape_text` writes it.
                let mut shape = Line::default();
                for (i, p) in paragraphs.iter().enumerate() {
                    if i > 0 {
                        shape.raw(Md::syntax(" "));
                    }
                    guarded_runs(p, styles, st, inner, &mut shape);
                }
                shape.trim();
                line.append(shape);
            }
            Part::Control(Control::Memo { content, anchor_runs, .. }) => {
                let Some(inner) = ctx.enter() else {
                    line.raw(Md::syntax(TOO_DEEP));
                    continue;
                };
                let anchor = st.keep(super::memo_anchor(anchor_runs));
                alias_copy(anchor_runs, &anchor.runs, st);
                guarded_runs(&anchor, styles, st, inner, line);
                line.raw(memo(content, styles, st, ctx));
            }
            // A control whose output is document text (a link's text, a
            // compose, a field, a cross reference, a caption number, an
            // equation's script, an unknown control's code): checked like
            // plain text, so it cannot take on the emphasis around it.
            // 이것을 실패시키는 것: 글 컨트롤을 검사에서 빼기(`plain_text_keeps_no_emphasis`).
            // A link with no text draws `[](url)`: nothing to read as text.
            // 이것을 실패시키는 것: 빈 라벨 링크에 글 읽기 요구(`text_controls_with_nothing_or_math`).
            Part::Control(control @ Control::Hyperlink { text, .. }) if text.is_empty() => {
                line.raw(control_md(control, styles, st, ctx))
            }
            // An equation: a math renderer reads `$…$`, so its own `*` is not
            // the document's emphasis; emphasis from outside must not reach
            // into it (`Item::Math`). Its script written as text instead
            // (`math_free_text`) is escaped throughout, so it opens nothing.
            // 이것을 실패시키는 것: 수식을 평문으로 검사(`text_controls_with_nothing_or_math`).
            Part::Control(control @ Control::Equation { .. }) => {
                line.math(control_md(control, styles, st, ctx))
            }
            Part::Control(
                control @ (Control::Hyperlink { .. }
                | Control::Compose { .. }
                | Control::Dutmal { .. }
                | Control::CrossRef { .. }
                | Control::Field { .. }
                | Control::AutoNumber { .. }
                | Control::Unknown { .. }),
            ) => line.plain(control_md(control, styles, st, ctx)),
            Part::Control(control) => line.raw(control_md(control, styles, st, ctx)),
        }
    }
}

/// An emphasis written with Markdown markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Strike,
    Bold,
    Italic,
}

impl Mark {
    /// The marker, italic written with `italic` (`*` or `_`).
    fn syntax(self, italic: &'static str) -> &'static str {
        match self {
            Self::Strike => "~~",
            Self::Bold => "**",
            Self::Italic => italic,
        }
    }
}

/// What a guarded paragraph draws, in order: text with its format, or
/// markup drawn already.
enum Item {
    Text(String, InlineFormat),
    Raw(Md),
    /// Document text drawn already (a text control's output): written as
    /// it is, checked to read with no emphasis.
    Plain(Md),
    /// An equation as `$…$`: written as it is; what is inside is not
    /// checked, but no emphasis from outside may be open over it.
    Math(Md),
    /// Preserve an existing Md join boundary, but carry its emphasis into
    /// the enclosing line's check instead of checking the child alone.
    Joined(Line),
}

/// A guarded paragraph's text and markup, collected first and written at
/// once, so its emphasis is written as markers over the whole line rather
/// than piece by piece: pieces of the same emphasis are one span (nothing
/// drawn between them, or a text box's text, does not end it), and a span
/// common to neighbouring pieces stays open across them. Where the
/// markers still have to close and open at the same place with the same
/// character (`**a***b**` shapes), CommonMark may read them as one
/// delimiter run: the line is parsed again, and if any marker reads as
/// text or any text lost its emphasis, the paragraph is written as plain
/// text with a warning instead.
#[derive(Default)]
struct Line {
    items: Vec<Item>,
}

impl Line {
    /// Text in `fmt`; next to text of the same format it is one piece.
    fn text(&mut self, raw: &str, fmt: InlineFormat) {
        if raw.is_empty() {
            return;
        }
        if let Some(Item::Text(text, f)) = self.items.last_mut() {
            if *f == fmt {
                text.push_str(raw);
                return;
            }
        }
        self.items.push(Item::Text(raw.to_string(), fmt));
    }

    /// Markup drawn already; nothing drawn is left out, so it does not
    /// split the text around it.
    /// 이것을 실패시키는 것: 빈 조각 건너뛰기 삭제(`adjacent_formatted_text_keeps_its_characters`).
    fn raw(&mut self, md: Md) {
        if !md.is_empty() {
            self.items.push(Item::Raw(md));
        }
    }

    /// [`Line::raw`] for an equation ([`Item::Math`]).
    fn math(&mut self, md: Md) {
        if !md.is_empty() {
            self.items.push(Item::Math(md));
        }
    }

    /// [`Line::raw`] for a control's document text ([`Item::Plain`]).
    fn plain(&mut self, md: Md) {
        if !md.is_empty() {
            self.items.push(Item::Plain(md));
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn is_blank(&self) -> bool {
        self.items.iter().all(Item::is_blank)
    }

    fn joined(&mut self, line: Line) {
        if !line.is_empty() {
            self.items.push(Item::Joined(line));
        }
    }

    /// The whitespace at both ends dropped: text trimmed, whitespace-only
    /// markup left out.
    fn trim(&mut self) {
        let blank = Item::is_blank;
        while self.items.last().is_some_and(blank) {
            self.items.pop();
        }
        let lead = self.items.iter().take_while(|item| blank(item)).count();
        self.items.drain(..lead);
        if let Some(Item::Text(t, _)) = self.items.first_mut() {
            *t = t.trim_start().to_string();
        }
        if let Some(Item::Text(t, _)) = self.items.last_mut() {
            *t = t.trim_end().to_string();
        }
    }

    /// `other`'s items after these, text next to text of its format joined.
    fn append(&mut self, other: Line) {
        for item in other.items {
            match item {
                Item::Text(t, fmt) => self.text(&t, fmt),
                Item::Raw(md) => self.raw(md),
                Item::Plain(md) => self.plain(md),
                Item::Math(md) => self.math(md),
                Item::Joined(line) => self.joined(line),
            }
        }
    }

    fn pieces(self, ctx: Ctx) -> Vec<Piece> {
        // Text written with markers, or drawn as `wrap` draws it.
        self.items
            .into_iter()
            .flat_map(|item| match item {
                Item::Raw(md) => vec![Piece::Raw(md)],
                Item::Plain(md) => vec![Piece::plain(md)],
                Item::Math(md) => vec![Piece::Math(md)],
                Item::Joined(line) => vec![Piece::Joined(line.pieces(ctx))],
                Item::Text(raw, fmt) => match fmt.marks(&raw) {
                    Some(marks) => marked(&raw, marks, ctx),
                    // Text without markers (plain, or HTML tags) is checked
                    // to read with no marker emphasis, so a neighbour's
                    // markers cannot reach it (`**a。**b**z**`).
                    // 이것을 실패시키는 것: 평문을 검사에서 빼기(`plain_text_keeps_no_emphasis`).
                    None => vec![Piece::plain(fmt.wrap(&raw, ctx))],
                },
            })
            .collect()
    }

    /// The line's Markdown, and whether its emphasis was kept. Only this
    /// final composition is checked: a rendered Body, or a complete cell.
    fn finish(self, ctx: Ctx) -> (Md, bool) {
        let pieces = self.pieces(ctx);
        // Spans common to neighbouring pieces stay open, those opened at one
        // place ordered so the one lasting longer is outside, or else in a
        // fixed order (strikeout, bold, italic — what `wrap` writes); or each
        // piece closes its spans, as `wrap` writes them. The first that reads
        // back right is kept; if none does, the paragraph is written as
        // plain text (`paragraph_text_styled`).
        // 이것을 실패시키는 것: 평문 대체 끄기(`three_emphasized_pieces_keep_their_emphasis`).
        // Last, the first again with italic as `_`, which does not run
        // together with a bold `**` (`**a_b_**_c_`).
        // 이것을 실패시키는 것: 넷째 후보 삭제(`three_emphasized_pieces_keep_their_emphasis`).
        let mut last = Md::new();
        for (keep_open, by_length, italic) in
            [(true, true, "*"), (true, false, "*"), (false, false, "*"), (true, true, "_")]
        {
            let w = emit(&pieces, keep_open, by_length, italic);
            let kept = w.markers.is_empty()
                || emphasis_kept(w.out.as_str(), &w.markers, &w.texts, &w.math, ctx.cell);
            if kept {
                return (w.out, true);
            }
            last = w.out;
        }
        (last, false)
    }
}

impl Item {
    fn is_blank(&self) -> bool {
        match self {
            Self::Text(t, _) => t.trim().is_empty(),
            Self::Raw(md) | Self::Plain(md) | Self::Math(md) => md.as_str().trim().is_empty(),
            Self::Joined(line) => line.is_blank(),
        }
    }
}

/// Text written with `marks`: one piece per paragraph of it — a span
/// cannot go on across a blank line, so it is closed before one and opened
/// again after it.
/// 이것을 실패시키는 것: 빈 줄에서 나누지 않기(`markup_safety` 의 굵은 `a\n\nb`).
fn marked(raw: &str, marks: Vec<Mark>, ctx: Ctx) -> Vec<Piece> {
    let piece = |part: &str| Piece::Marked {
        lead: part[..part.len() - part.trim_start().len()].to_string(),
        core: text(part.trim(), ctx),
        trail: part[part.trim_end().len()..].to_string(),
        marks: marks.clone(),
    };
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while let Some(c) = raw[i..].chars().next() {
        if !c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let run = raw[i..].len() - raw[i..].trim_start().len();
        let blank = raw[i..i + run].matches(['\n', '\r']).count() >= 2;
        if blank && !raw[start..i].trim().is_empty() && i + run < raw.len() {
            out.push(piece(&raw[start..i]));
            out.push(Piece::Raw(Md::text(&raw[i..i + run])));
            start = i + run;
        }
        i += run;
    }
    out.push(piece(&raw[start..]));
    out
}

/// A piece of a guarded line written with markers: its edge whitespace,
/// its text, its emphasis; or markup drawn already.
enum Piece {
    Marked {
        lead: String,
        core: Md,
        trail: String,
        marks: Vec<Mark>,
    },
    Raw(Md),
    /// Document text with no marker emphasis (whitespace alone is `Raw`:
    /// it may read as no text at all).
    Plain(Md),
    /// An equation ([`Item::Math`]).
    Math(Md),
    Joined(Vec<Piece>),
}

impl Piece {
    fn plain(md: Md) -> Self {
        if md.as_str().trim().is_empty() {
            Self::Raw(md)
        } else {
            Self::Plain(md)
        }
    }
}

/// Writes pieces and records their emphasis, including joined children.
/// No child is parsed alone: `Line::finish` checks the completed unit.
fn emit(pieces: &[Piece], keep_open: bool, by_length: bool, italic: &'static str) -> Writer {
    let mut w = Writer::default();
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Joined(pieces) => {
                w.close(0, italic);
                w.flush_space();
                w.place_joined(emit(pieces, keep_open, by_length, italic));
            }
            Piece::Raw(md) => {
                w.close(0, italic);
                w.flush_space();
                w.place(md.clone());
            }
            Piece::Plain(md) => {
                w.close(0, italic);
                w.flush_space();
                let range = w.place(md.clone());
                w.texts.push((range, Vec::new()));
            }
            Piece::Math(md) => {
                w.close(0, italic);
                w.flush_space();
                let range = w.place(md.clone());
                w.math.push(range);
            }
            Piece::Marked { lead, core, trail, marks } => {
                w.space.push_str(lead);
                let keep = if keep_open {
                    w.open.iter().position(|m| !marks.contains(m)).unwrap_or(w.open.len())
                } else {
                    0
                };
                w.close(keep, italic);
                let mut opening: Vec<Mark> =
                    marks.iter().copied().filter(|m| !w.open.contains(m)).collect();
                if by_length {
                    // How many pieces from here on an emphasis lasts.
                    let lasts = |m: Mark| {
                        pieces[i..]
                            .iter()
                            .take_while(
                                |p| matches!(p, Piece::Marked { marks, .. } if marks.contains(&m)),
                            )
                            .count()
                    };
                    opening.sort_by_key(|&m| std::cmp::Reverse(lasts(m)));
                }
                w.flush_space();
                for m in opening {
                    let range = w.place(Md::syntax(m.syntax(italic)));
                    w.markers.push(range);
                    w.open.push(m);
                }
                let range = w.place(core.clone());
                w.texts.push((range, marks.clone()));
                w.space = trail.clone();
            }
        }
    }
    w.close(0, italic);
    w.flush_space();
    w
}

/// [`emit`]'s state: the markdown, the spans open (outside first),
/// whitespace not yet written, and where markers and texts went.
#[derive(Default)]
struct Writer {
    out: Md,
    open: Vec<Mark>,
    space: String,
    markers: Vec<std::ops::Range<usize>>,
    texts: Vec<(std::ops::Range<usize>, Vec<Mark>)>,
    /// Where equations went ([`Piece::Math`]).
    math: Vec<std::ops::Range<usize>>,
}

impl Writer {
    /// Keep the old Md composition (including table-end trim and escapes),
    /// moving every child's marker/text range to its actual parent offset.
    /// 이것을 실패시키는 것: 자식 범위 버리기 · trim 보정 빼기
    /// (`captions_are_checked_in_the_final_body`).
    fn place_joined(&mut self, child: Writer) {
        let len = child.out.as_str().len();
        let at = self.place(child.out);
        let trimmed = len - at.len();
        let shift = |r: std::ops::Range<usize>| {
            at.start + r.start.saturating_sub(trimmed)..at.start + r.end.saturating_sub(trimmed)
        };
        self.markers.extend(child.markers.into_iter().map(shift));
        self.texts.extend(child.texts.into_iter().map(|(r, marks)| (shift(r), marks)));
        self.math.extend(child.math.into_iter().map(shift));
    }

    /// Appends `md`, keeping the recorded places right when an escape goes
    /// in before them ([`Md::push_placed`]); where it went.
    fn place(&mut self, md: Md) -> std::ops::Range<usize> {
        let (range, shifted) = self.out.push_placed(md);
        // Both lists are in order: only the places after the escape move.
        // 이것을 실패시키는 것: 모든 자리를 다시 훑기(`emphasis_writer_is_linear`).
        if let Some(at) = shifted {
            for r in self.markers.iter_mut().rev().take_while(|r| r.start >= at) {
                *r = r.start + 1..r.end + 1;
            }
            for (r, _) in self.texts.iter_mut().rev().take_while(|(r, _)| r.start >= at) {
                *r = r.start + 1..r.end + 1;
            }
            for r in self.math.iter_mut().rev().take_while(|r| r.start >= at) {
                *r = r.start + 1..r.end + 1;
            }
        }
        range
    }

    /// Closes the open spans down to `keep`.
    fn close(&mut self, keep: usize, italic: &'static str) {
        while self.open.len() > keep {
            let m = self.open.pop().expect("open span");
            let range = self.place(Md::syntax(m.syntax(italic)));
            self.markers.push(range);
        }
    }

    /// Writes the held whitespace (outside the markers around it).
    fn flush_space(&mut self) {
        let space = std::mem::take(&mut self.space);
        self.place(Md::text(&space));
    }
}

/// Whether `markdown`, parsed as the decoder parses, reads no marker as
/// text and every text range with exactly its emphasis — each range read
/// as text at least once: one hidden in raw HTML or code is not kept
/// (fail closed). A cell's line is read in a one-cell table, as written
/// there (`|` escaped, line endings as `<br>`); a block's after its leading
/// whitespace. Both lists are in order, as the parser's text is.
/// 이것을 실패시키는 것: 읽지 않은 글을 통과시키기 · 칸을 칸 밖에서 읽기
/// (`hidden_emphasis_is_not_passed`).
fn emphasis_kept(
    markdown: &str,
    markers: &[std::ops::Range<usize>],
    texts: &[(std::ops::Range<usize>, Vec<Mark>)],
    math: &[std::ops::Range<usize>],
    cell: bool,
) -> bool {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    // The text read, and where each byte of `markdown` went in it: a
    // cell's line in a one-cell table, as written there; a block's line
    // after its leading whitespace, with its comments ending their lines
    // ([`comment_ends_line`]), as the block is written.
    let (view, cell_map, edits, off) = if cell {
        let mut view = String::from("| ");
        let mut map = Vec::with_capacity(markdown.len() + 1);
        for c in markdown.chars() {
            for _ in 0..c.len_utf8() {
                map.push(view.len());
            }
            match c {
                '|' => view.push_str("\\|"),
                '\n' => view.push_str("<br>"),
                c => view.push(c),
            }
        }
        map.push(view.len());
        view.push_str(" |\n| --- |");
        (view, map, Vec::new(), 0)
    } else {
        let off = markdown.len() - markdown.trim_start().len();
        let (view, edits) = comment_ends_line(&markdown[off..]);
        (view, Vec::new(), edits, off)
    };
    // Each sorted, disjoint range list is merged with the edits once.
    // Two forward cursors cost O(Q + E), not a binary search per lookup;
    // parser comparisons below read the adjusted ranges in constant time.
    // 이것을 실패시키는 것: 누적 이동량 무시 · 두 목록의 커서 공유
    // (`emphasis_offsets_follow_all_g2_edits`).
    let adjust = || {
        let mut k = 0;
        let (cell_map, edits) = (&cell_map, &edits);
        let mut place = move |x: usize| -> usize {
            if cell {
                return cell_map[x];
            }
            let x = x.saturating_sub(off);
            while k < edits.len() && edits[k].1 <= x {
                k += 1;
            }
            let before = if k == 0 { 0 } else { edits[k - 1].2 };
            match edits.get(k) {
                Some(&(start, _, _)) if start < x => (start as isize + before) as usize,
                _ => (x as isize + before) as usize,
            }
        };
        move |r: &std::ops::Range<usize>| place(r.start)..place(r.end)
    };
    let markers: Vec<_> = markers.iter().map(adjust()).collect();
    let ranges: Vec<_> = texts.iter().map(|(r, _)| r).map(adjust()).collect();
    let math: Vec<_> = math.iter().map(adjust()).collect();
    let (mut strike, mut bold, mut italic) = (0usize, 0usize, 0usize);
    let (mut m, mut t, mut q) = (0, 0, 0);
    // Where each open emphasis started: one opened inside an equation is
    // the equation's own; one opened before it reaches into it.
    let mut opened: Vec<usize> = Vec::new();
    let mut seen = vec![false; texts.len()];
    for (event, range) in Parser::new_ext(&view, super::md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Strikethrough) => {
                strike += 1;
                opened.push(range.start);
            }
            Event::End(TagEnd::Strikethrough) => {
                strike -= 1;
                opened.pop();
            }
            Event::Start(Tag::Strong) => {
                bold += 1;
                opened.push(range.start);
            }
            Event::End(TagEnd::Strong) => {
                bold -= 1;
                opened.pop();
            }
            Event::Start(Tag::Emphasis) => {
                italic += 1;
                opened.push(range.start);
            }
            Event::End(TagEnd::Emphasis) => {
                italic -= 1;
                opened.pop();
            }
            // A code span is text too (an unknown control's `[tag]`).
            Event::Text(_) | Event::Code(_) => {
                while q < math.len() && math[q].end <= range.start {
                    q += 1;
                }
                if math.get(q).is_some_and(|r| r.start < range.end)
                    // Nested spans start in order: the outermost is the first.
                    // 이것을 실패시키는 것: 열린 강조를 전부 훑기(`math_check_is_linear`).
                    && opened.first().is_some_and(|&at| at < math[q].start)
                {
                    return false;
                }
                while m < markers.len() && markers[m].end <= range.start {
                    m += 1;
                }
                if markers.get(m).is_some_and(|r| r.start < range.end) {
                    return false;
                }
                while t < texts.len() && ranges[t].end <= range.start {
                    t += 1;
                }
                let mut k = t;
                while let Some((_, marks)) = texts.get(k).filter(|_| ranges[k].start < range.end) {
                    if (strike > 0) != marks.contains(&Mark::Strike)
                        || (bold > 0) != marks.contains(&Mark::Bold)
                        || (italic > 0) != marks.contains(&Mark::Italic)
                    {
                        return false;
                    }
                    seen[k] = true;
                    k += 1;
                }
            }
            _ => {}
        }
    }
    seen.iter().all(|&s| s)
}

/// `original`, or its memo-expanded copy (`expand_memos`) kept for the
/// whole document ([`State::keep`]): the paragraph to draw.
fn expanded<'a>(
    original: &'a Paragraph,
    st: &mut State,
    kept: &'a mut Option<std::rc::Rc<Paragraph>>,
) -> &'a Paragraph {
    match hwpforge_core::field_view::expand_memos(original) {
        std::borrow::Cow::Borrowed(_) => original,
        std::borrow::Cow::Owned(copy) => {
            let copy = kept.insert(st.keep(copy));
            alias_copy(&original.runs, &copy.runs, st);
            copy
        }
    }
}

/// A copy's note occurrences stand for the document's (`original`'s, or
/// what those stand for), paired in the order the fallback walk meets them
/// ([`super::guard::note_order`]: a memo's anchor in place, its body
/// skipped, as `expand_memos` keeps it). The copy lives as long as the
/// document is drawn ([`State::keep`]), so no address is taken twice.
/// Unpaired (the orders differ), the copy's notes stand for themselves,
/// which no fallback walk of the document meets: their text.
/// 이것을 실패시키는 것: 별칭 삭제 · 별칭을 문서 주소로 풀지 않기
/// (`fallback_keeps_notes_by_occurrence`).
fn alias_copy(original: &[hwpforge_core::Run], copy: &[hwpforge_core::Run], st: &mut State) {
    let from = super::guard::note_order(copy);
    let to = super::guard::note_order(original);
    if from.len() == to.len() {
        for (c, o) in from.into_iter().zip(to) {
            let o = st.note_at(o);
            st.note_alias.insert(c, o);
        }
    }
}

/// The styled plain-text fallback of `paragraphs` drawn at `depth`
/// ([`super::guard::fallback_markdown`]): it keeps the pictures the view
/// registered, allowed links, and the references of the notes it
/// registered.
fn fallback(paragraphs: &[Paragraph], depth: usize, st: &mut State) -> Vec<Md> {
    super::guard::fallback_markdown(paragraphs, depth, true, &mut Kept { st })
}

/// The styled view's [`super::guard::Keeper`]: pictures with the key the
/// view gave their source (none is registered anew), allowed links, the
/// reference registered for the very note.
struct Kept<'a> {
    st: &'a mut State,
}

impl super::guard::Keeper for Kept<'_> {
    fn picture(&mut self, image: &hwpforge_core::image::Image) -> Option<Md> {
        let key = self.st.assets.drawn.get(&image.path)?;
        Some(Md::image(Md::text(image_stem(&image.path)), key, &mut self.st.destinations))
    }

    fn link(&mut self, text: &str, url: &str) -> Option<Md> {
        super::link_url_allowed(url).then(|| {
            let label = Md::plain_parts(&[super::guard::Fallback::Text(text.to_string())]);
            Md::link(label, url, &mut self.st.destinations)
        })
    }

    fn note(&mut self, note: &Control) -> Option<Md> {
        self.st.note_marks.get(&self.st.note_at(note as *const Control as usize)).cloned()
    }
}

/// Encodes a single control element into styled markdown.
fn control_md(control: &Control, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Md {
    match control {
        // Only an allowed URL (`link_url_allowed`) is written as a link; its
        // text is Markdown text either way.
        // 이것을 실패시키는 것: 표시 문자열 이스케이프 삭제 · 허용 검사 삭제
        // (`link_text_cannot_inject_markup`).
        Control::Hyperlink { text, url } => {
            if super::link_url_allowed(url) {
                Md::link(Md::text(text), url, &mut st.destinations)
            } else {
                self::text(text, ctx)
            }
        }
        // A note at depth 0 outside a memo is a document definition; a note
        // inside a note body (directly or through a shape or group) or inside
        // a memo is written inline. 이것을 실패시키는 것: 깊이 조건 삭제 · 메모
        // 조건 삭제(`notes_follow_depth_and_memo`).
        Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
            let body = extract_note_body(paragraphs, styles, st, ctx);
            if ctx.note_depth > 0 || ctx.in_comment {
                let mut md = Md::syntax("(주: ");
                md.push(Md::join(body, " "));
                md.push_syntax(")");
                md
            } else {
                note_definition(control, body, paragraphs, st, ctx)
            }
        }
        Control::TextBox { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => shape_text(paragraphs, styles, st, ctx),
        Control::Equation { script, .. } => match render_equation(script) {
            EqnRender::Latex(latex) => match Md::math(&latex) {
                Some(latex) => latex,
                // The LaTeX would end its math early or write a link: the
                // script as ordinary text instead (`$` escaped too, so it
                // opens no math), reported.
                None => {
                    st.warnings.push(super::MdWarning::EquationAsText {
                        section: st.at.0,
                        paragraph: st.at.1,
                    });
                    math_free_text(&script.split_whitespace().collect::<Vec<_>>().join(" "), ctx)
                }
            },
            // Too deep to convert: ordinary text (whitespace already
            // collapsed, `$` escaped as above). Not reported as
            // `EquationAsText`, which is for the safety fallback only.
            EqnRender::RawFallback(raw) => math_free_text(&raw, ctx),
        },
        Control::Chart { .. } => Md::syntax(super::guard::CHART_COMMENT),
        Control::Dutmal { main_text, sub_text, .. } => {
            let mut md = text(main_text, ctx);
            md.push_syntax("(");
            md.push(text(sub_text, ctx));
            md.push_syntax(")");
            md
        }
        Control::Compose { compose_text, .. } => text(compose_text, ctx),
        // the visible text (display_text) if any,
        // else the target's display form in brackets (Name → "bookmark1",
        // SystemId → "#5"). Markdown has no anchor link here: text only.
        // The brackets are text too: as syntax they made a link with a
        // following `(…)`. 이것을 실패시키는 것: 대괄호를 문법으로
        // 쓰기(`markup_safety` 의 빈 상호 참조).
        // A field: the filled value if any, else the hint — the HWPX
        // encoder's hint fallback order.
        Control::CrossRef { .. } | Control::Field { .. } => {
            text(&super::styled_display(control).unwrap_or_default(), ctx)
        }
        Control::FieldBoundary {
            boundary: hwpforge_core::control::FieldBoundary::Start(start),
        } => {
            start.annotation.as_ref().map_or_else(Md::new, |content| memo(content, styles, st, ctx))
        }
        // A memo still standing after `expand_memos` (it sits in another
        // memo's anchor): the anchor's text runs, as before (G.3 (라)).
        // Where text is guarded, `guarded_runs` draws it instead.
        // 이것을 실패시키는 것: 문단 렌더러로(`memo_anchor_out_of_scope_is_base_output`).
        Control::Memo { content, anchor_runs, .. } => {
            let mut md = Md::text(
                &anchor_runs.iter().filter_map(|r| r.content.plain_text()).collect::<String>(),
            );
            md.push(memo(content, styles, st, ctx));
            md
        }
        // A caption number's prefix/suffix characters are text, not
        // Markdown (a `*` prefix and suffix turned "3" into
        // emphasis). 이것을 실패시키는 것: 텍스트 escape 삭제
        // (`caption_number_prefix_is_escaped`).
        Control::AutoNumber { num, format, .. } => {
            text(&super::auto_number_text(*num, format), ctx)
        }
        Control::Unknown { .. } => {
            Md::code_span(&super::styled_display(control).unwrap_or_default())
        }
        // Invisible or textless: bookmarks, index marks, number restarts and
        // hiding, lines, arcs, curves, connectors.
        _ => Md::new(),
    }
}

/// [`control_md`] in an HTML cell. Text-bearing controls are HTML text;
/// links are `<a>` with the href attribute-escaped; inline notes and shape
/// text are drawn as HTML from the start.
/// 이것을 실패시키는 것: 표시 문자열 이스케이프 삭제 · href 이스케이프 삭제
/// (`link_text_cannot_inject_markup`) · HTML 칸 인라인 각주를 Markdown 으로 그린
/// 뒤 HTML escape 하기.
fn control_html(control: &Control, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Html {
    match control {
        Control::Hyperlink { text, url } => {
            if super::link_url_allowed(url) {
                Html::link(text, url, &mut st.destinations)
            } else {
                Html::text(text)
            }
        }
        Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
            if ctx.note_depth > 0 || ctx.in_comment {
                let mut html = Html::syntax("(주: ");
                html.push(Html::join(extract_note_body_html(paragraphs, styles, st, ctx), " "));
                html.push_syntax(")");
                html
            } else {
                // A definition is Markdown; its marker is written as it is.
                let body = extract_note_body(paragraphs, styles, st, ctx);
                Html::note_mark(note_definition(control, body, paragraphs, st, ctx))
            }
        }
        Control::TextBox { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => shape_text_html(paragraphs, styles, st, ctx),
        // The LaTeX is cell text (a `<` or `&` in it is a character — a math
        // renderer reads the decoded text); a script too deep to convert is
        // ordinary cell text. 이것을 실패시키는 것: LaTeX 이스케이프 삭제
        // (`html_cell_equation_is_escaped`).
        Control::Equation { script, .. } => match render_equation(script) {
            EqnRender::Latex(latex) => Html::text(&latex),
            EqnRender::RawFallback(raw) => Html::text(&raw),
        },
        Control::Chart { .. } => Html::syntax(super::guard::CHART_COMMENT),
        Control::Dutmal { main_text, sub_text, .. } => {
            Html::text(&format!("{main_text}({sub_text})"))
        }
        Control::Compose { compose_text, .. } => Html::text(compose_text),
        Control::CrossRef { .. } | Control::Field { .. } => {
            Html::text(&super::styled_display(control).unwrap_or_default())
        }
        // The memo comment shown as cell text, as before.
        Control::FieldBoundary {
            boundary: hwpforge_core::control::FieldBoundary::Start(start),
        } => start
            .annotation
            .as_ref()
            .map_or_else(Html::new, |content| Html::text(memo(content, styles, st, ctx).as_str())),
        // HTML text makes no block, so the anchor's text runs as before.
        // 이것을 실패시키는 것: 문단 렌더러로(`memo_anchor_out_of_scope_is_base_output`).
        Control::Memo { content, anchor_runs, .. } => {
            let mut html = Html::text(
                &anchor_runs.iter().filter_map(|r| r.content.plain_text()).collect::<String>(),
            );
            html.push(Html::text(memo(content, styles, st, ctx).as_str()));
            html
        }
        Control::AutoNumber { num, format, .. } => {
            Html::text(&super::auto_number_text(*num, format))
        }
        Control::Unknown { .. } => {
            Html::text(&format!("`{}`", super::styled_display(control).unwrap_or_default()))
        }
        _ => Html::new(),
    }
}

/// Registers a note definition (its raw text and place kept for the final
/// check) and returns its marker.
fn note_definition(
    control: &Control,
    body: Vec<Md>,
    paragraphs: &[Paragraph],
    st: &mut State,
    ctx: Ctx,
) -> Md {
    let raw = match ctx.enter() {
        // Notes in a note body are written inline, not registered: none to
        // keep as references.
        Some(inner) => fallback(paragraphs, inner.depth, st)
            .into_iter()
            .map(Md::into_string)
            .collect::<Vec<_>>()
            .join(" "),
        None => super::guard::plain_text(TOO_DEEP),
    };
    let mark = if matches!(control, Control::Footnote { .. }) {
        st.notes.add_footnote(body, raw, st.at)
    } else {
        st.notes.add_endnote(body, raw, st.at)
    };
    let at = st.note_at(control as *const Control as usize);
    st.note_marks.insert(at, mark.clone());
    mark
}

/// Equation script as Markdown text with `$` escaped too, so it opens no
/// math. 이것을 실패시키는 것: `$` escape 삭제
/// (`markdown_equation_writes_no_html_or_link`).
fn math_free_text(raw: &str, ctx: Ctx) -> Md {
    let mut md = Md::new();
    for (i, piece) in raw.split('$').enumerate() {
        if i > 0 {
            md.push_syntax("\\$");
        }
        md.push(text(piece, ctx));
    }
    md
}

/// A memo as a one-line `<!-- memo: … -->`, its body drawn as Markdown:
/// line endings (blank lines too) folded to one space, so the parser reads
/// one comment and not text, and every `-` escaped, so the body holds no
/// `--` and cannot close the comment ([`super::guard::comment_kind`]).
/// Author/date are no longer carried (Memo): HWPX wire never surfaced them.
fn memo(content: &[Paragraph], styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Md {
    let Some(inner) = ctx.enter() else { return Md::syntax(TOO_DEEP) };
    let inner = Ctx { in_comment: true, guarded: false, ..inner };
    let body: Vec<Md> =
        content.iter().map(|p| paragraph_text_styled(p, styles, st, inner)).collect();
    let body = Md::join(body, " ");
    if body.as_str().trim().is_empty() {
        return Md::new();
    }
    body.layout(|b| {
        let line: Vec<&str> =
            b.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect();
        format!("{MEMO_OPEN}{}{MEMO_CLOSE}", line.join(" ").replace('-', "\\-"))
    })
}

/// A note body's paragraphs in Markdown, each drawn on its own
/// (multi-paragraph preserved) one note level deeper.
fn extract_note_body(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Vec<Md> {
    let Some(inner) = ctx.enter() else { return vec![Md::syntax(TOO_DEEP)] };
    let inner = Ctx { note_depth: ctx.note_depth + 1, guarded: false, ..inner };
    paragraphs
        .iter()
        .map(|p| paragraph_text_styled(p, styles, st, inner).layout(|s| s.trim().to_string()))
        .filter(|t| !t.is_empty())
        .collect()
}

/// [`extract_note_body`] in HTML, for a note written inline in an HTML cell
/// (escaped once).
fn extract_note_body_html(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Vec<Html> {
    let Some(inner) = ctx.enter() else { return vec![Html::syntax(TOO_DEEP)] };
    let inner = Ctx { note_depth: ctx.note_depth + 1, guarded: false, ..inner };
    paragraphs
        .iter()
        .map(|p| extract_paragraph_text_html(p, styles, st, inner).trimmed())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A text box's or shape's text in Markdown: each paragraph drawn, joined
/// by a space and trimmed.
fn shape_text(paragraphs: &[Paragraph], styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Md {
    let Some(ctx) = ctx.enter() else { return Md::syntax(TOO_DEEP) };
    let parts: Vec<Md> =
        paragraphs.iter().map(|p| paragraph_text_styled(p, styles, st, ctx)).collect();
    Md::join(parts, " ").layout(|s| s.trim().to_string())
}

/// [`shape_text`] in an HTML cell: HTML already, not escaped again.
/// 이것을 실패시키는 것: Markdown 렌더러로 그리기(`html_cell_shape_text_is_html`).
fn shape_text_html(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Html {
    let Some(ctx) = ctx.enter() else { return Html::syntax(TOO_DEEP) };
    let parts: Vec<Html> =
        paragraphs.iter().map(|p| extract_paragraph_text_html(p, styles, st, ctx)).collect();
    Html::join(parts, " ").trimmed()
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
    /// Receives **raw** (unescaped) text, Markdown-escaped either way.
    /// Strategy:
    /// - If text contains markdown-marker chars (`*`, `_`, `~`), or text
    ///   starts/ends with punctuation (which breaks CommonMark flanking
    ///   rules when adjacent to other text), or formatting has no markdown
    ///   equivalent → HTML tags.
    /// - Otherwise → markdown markers (`**`, `*`, `~~`).
    fn wrap(&self, text: &str, ctx: Ctx) -> Md {
        let md = |raw: &str| self::text(raw, ctx);
        if text.is_empty() {
            return Md::new();
        }

        let has_basic = self.bold || self.italic || self.strikeout;
        let has_any = has_basic || self.underline || self.superscript || self.subscript;

        if !has_any {
            return md(text);
        }

        // Move leading/trailing whitespace outside formatting markers.
        // CommonMark: `** text**` fails (space after opening marker).
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Md::text(text);
        }
        let leading = &text[..text.len() - text.trim_start().len()];
        let trailing = &text[text.trim_end().len()..];

        let needs_html_only = self.underline || self.superscript || self.subscript;
        let has_marker_conflicts = trimmed.chars().any(|c| matches!(c, '*' | '_' | '~'));
        // CommonMark flanking rules: `word***(punct` fails as left-flanking.
        let has_boundary_punct = trimmed.chars().next().is_some_and(|c| c.is_ascii_punctuation())
            || trimmed.chars().next_back().is_some_and(|c| c.is_ascii_punctuation());

        // Whitespace alone: no rule can fire on it.
        let mut out = Md::text(leading);
        if needs_html_only || (has_basic && (has_marker_conflicts || has_boundary_punct)) {
            // HTML tags around Markdown text: a renderer still reads Markdown
            // inside inline HTML, so the text is Markdown-escaped, not
            // HTML-escaped. 이것을 실패시키는 것: 태그 안을 `escape_html` 로
            // (`markup_safety` 의 밑줄 링크).
            let tags: Vec<&'static str> = [
                (self.subscript, "sub"),
                (self.superscript, "sup"),
                (self.underline, "u"),
                (self.strikeout, "del"),
                (self.italic, "em"),
                (self.bold, "strong"),
            ]
            .into_iter()
            .filter_map(|(on, tag)| on.then_some(tag))
            .collect();
            for tag in &tags {
                out.push(Md::tag(tag, false));
            }
            out.push(md(trimmed));
            for tag in tags.iter().rev() {
                out.push(Md::tag(tag, true));
            }
        } else {
            // Markdown path: no conflicting chars, safe to use markers.
            let marker = match (self.bold, self.italic) {
                (true, true) => "***",
                (true, false) => "**",
                (false, true) => "*",
                (false, false) => "",
            };
            if self.strikeout {
                out.push_syntax("~~");
            }
            out.push_syntax(marker);
            out.push(md(trimmed));
            out.push_syntax(marker);
            if self.strikeout {
                out.push_syntax("~~");
            }
        }
        out.push(Md::text(trailing));
        out
    }

    /// The markers [`InlineFormat::wrap`] would write `text` with, `None`
    /// where it writes no markers (plain text, HTML tags, whitespace only).
    fn marks(&self, text: &str) -> Option<Vec<Mark>> {
        let trimmed = text.trim();
        let has_basic = self.bold || self.italic || self.strikeout;
        let html = self.underline
            || self.superscript
            || self.subscript
            || trimmed.chars().any(|c| matches!(c, '*' | '_' | '~'))
            || trimmed.chars().next().is_some_and(|c| c.is_ascii_punctuation())
            || trimmed.chars().next_back().is_some_and(|c| c.is_ascii_punctuation());
        if !has_basic || html || trimmed.is_empty() {
            return None;
        }
        let marks =
            [(self.strikeout, Mark::Strike), (self.bold, Mark::Bold), (self.italic, Mark::Italic)];
        Some(marks.into_iter().filter_map(|(on, m)| on.then_some(m)).collect())
    }

    /// Wraps text with HTML inline formatting tags.
    ///
    /// Used inside HTML table cells where markdown syntax is not valid.
    fn wrap_html(&self, text: &str) -> Html {
        if text.is_empty() {
            return Html::new();
        }
        let mut result = Html::text(text);
        for (on, tag) in [
            (self.bold, "strong"),
            (self.italic, "em"),
            (self.strikeout, "del"),
            (self.underline, "u"),
            (self.superscript, "sup"),
            (self.subscript, "sub"),
        ] {
            if on {
                result = Html::wrap(tag, result);
            }
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
    st: &mut State,
    ctx: Ctx,
) -> Md {
    if table.rows.is_empty() {
        return Md::syntax("| |\n| --- |");
    }

    if has_merge(table) || has_nested_table(table) || html_context {
        Md::from_html(table_to_html(table, styles, st, ctx))
    } else {
        table_to_gfm(table, styles, st, ctx)
    }
}

/// Renders a table as GFM (GitHub Flavored Markdown) pipe table. The header
/// row is always the first physical row, irrespective of row/cell title
/// markers (GFM cannot represent per-cell titles). Existing output is retained.
/// It fixes the width; a later row's cells past it are not drawn at all,
/// so they leave no picture asset and no note definition behind.
/// 이것을 실패시키는 것: 잘릴 칸도 그린 뒤 버리기(`gfm_cut_cells_leave_nothing`).
fn table_to_gfm(table: &Table, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Md {
    let Some(ctx) = ctx.enter() else { return Md::syntax(TOO_DEEP) };
    let width = table.rows.first().map_or(0, |r| r.cells.len()).max(1);
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for (ri, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::new();
        for cell in row.cells.iter().take(if ri == 0 { usize::MAX } else { width }) {
            let ctx = Ctx { cell: true, ..ctx };
            if ctx.guarded {
                let mut line = Line::default();
                for (i, p) in cell.paragraphs.iter().enumerate() {
                    if i > 0 {
                        line.raw(Md::syntax("\n"));
                    }
                    let mut part = Line::default();
                    guarded_runs(p, styles, st, ctx, &mut part);
                    line.joined(part);
                }
                let (md, kept) = line.finish(ctx);
                let md = if kept {
                    md
                } else {
                    // The final cell is the check and fallback unit. Other
                    // cells, the table and its outer caption stay intact.
                    // 이것을 실패시키는 것: 칸 실패를 부모 Body로 전파
                    // (`joined_cell_emphasis_stays_local`).
                    super::guard::written_as_text(&mut st.warnings, &mut st.warned, st.at);
                    Md::join(fallback(&cell.paragraphs, ctx.depth, st), "\n")
                };
                cells.push(md.into_cell());
                continue;
            }
            let parts: Vec<Md> =
                cell.paragraphs.iter().map(|p| paragraph_text_styled(p, styles, st, ctx)).collect();
            cells.push(Md::join(parts, "\n").into_cell());
        }
        rows.push(cells);
    }
    let col_count = rows.first().map_or(0, Vec::len).max(1);
    // Pad every row (the header too) to the header's column count for
    // valid GFM.
    for row in &mut rows {
        row.resize(col_count, Cell::new());
    }
    Cell::table(rows, col_count)
}

/// Renders a table as HTML `<table>` with colspan/rowspan attributes.
fn table_to_html(table: &Table, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> Html {
    let Some(ctx) = ctx.enter() else { return Html::syntax(TOO_DEEP) };
    let mut lines = vec![Html::syntax("<table>"), Html::syntax("<tbody>")];
    for row in &table.rows {
        lines.push(Html::syntax("<tr>"));
        for cell in &row.cells {
            let parts: Vec<Html> = cell
                .paragraphs
                .iter()
                .map(|p| extract_paragraph_text_html(p, styles, st, ctx))
                .collect();
            lines.push(Html::td(cell.col_span, cell.row_span, Html::join(parts, "<br>")));
        }
        lines.push(Html::syntax("</tr>"));
    }
    lines.push(Html::syntax("</tbody>"));
    lines.push(Html::syntax("</table>"));
    Html::join(lines, "\n")
}

/// Extracts text from a paragraph using HTML inline formatting.
fn extract_paragraph_text_html(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Html {
    let mut kept = None;
    let paragraph = expanded(paragraph, st, &mut kept);
    let mut output = Html::new();
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for (part, cs) in super::spread_groups(&paragraph.runs) {
        if !matches!(part, Part::Text(_)) && !current_text.is_empty() {
            output.push(current_format.wrap_html(&current_text));
            current_text.clear();
            current_format = InlineFormat::default();
        }
        match part {
            // Same as the Markdown styled path:
            // `InlineText` folds to its `\t`-bearing plain string via
            // `plain_text()` so HTML output stays consistent.
            Part::Text(content) => {
                let Some(cow) = content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(cs, styles);
                if fmt != current_format {
                    if !current_text.is_empty() {
                        output.push(current_format.wrap_html(&current_text));
                        current_text.clear();
                    }
                    current_format = fmt;
                }
                current_text.push_str(&cow);
            }
            Part::Control(control) => output.push(control_html(control, styles, st, ctx)),
            Part::Image(image) => {
                output.push(emit_picture_html(image, styles, st, ctx));
                output.push(html_caption(image.caption.as_ref(), styles, st, ctx));
            }
            Part::Table(table) => {
                output.push(table_to_html(table, styles, st, ctx));
                output.push(html_caption(table.caption.as_ref(), styles, st, ctx));
            }
        }
    }

    if !current_text.is_empty() {
        output.push(current_format.wrap_html(&current_text));
    }

    output
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A guarded caption retains its original paragraph/join boundaries but
/// does not finish (or parse) a child line. All its ranges reach the final
/// Body/cell check, including nested captions and shape/memo-anchor text.
/// 이것을 실패시키는 것: 캡션을 독립 검사한 Raw로 붙이기
/// (`captions_are_checked_in_the_final_body`).
fn guarded_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Line {
    let mut line = Line::default();
    let Some(caption) = caption else { return line };
    let Some(ctx) = ctx.enter() else {
        line.raw(Md::syntax(TOO_DEEP));
        return line;
    };
    for p in &caption.paragraphs {
        let mut part = Line::default();
        guarded_runs(p, styles, st, ctx, &mut part);
        if !part.is_blank() {
            line.raw(Md::syntax("\n\n"));
            line.joined(part);
        }
    }
    line
}

/// Extracts the filename stem as alt text from an image path.
/// The file stem of an image path (alt text before any escaping).
fn image_stem(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().and_then(|name| name.split('.').next()).unwrap_or("image")
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
    // Parentheses percent-encoded in the file name (the asset key; the
    // Markdown destination is escaped separately — `markup::destination`).
    let safe_filename = filename.replace('(', "%28").replace(')', "%29");
    format!("images/{safe_filename}")
}

/// A picture's or table's caption as a paragraph after it (`\n\n` + its
/// text) and the images it holds, empty without one. The caption's numbers
/// ("그림 3") live here (captions were not written at
/// all; a picture inside the caption lost its bytes).
/// 이것을 실패시키는 것: 캡션 출력을 지우기(캡션 번호 3·4·7 이 사라짐 —
/// `f17_caption_numbers_reach_the_readable_views`) / 캡션 그림의 바이트를
/// 버리기(`caption_pictures_and_html_table_captions_are_kept`).
fn styled_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Md {
    let Some(caption) = caption else { return Md::new() };
    let Some(ctx) = ctx.enter() else { return Md::syntax(TOO_DEEP) };
    let mut out = Md::new();
    for p in &caption.paragraphs {
        let t = paragraph_text_styled(p, styles, st, ctx);
        if !t.as_str().trim().is_empty() {
            out.push_syntax("\n\n");
            out.push(t.layout(|t| comments_after_text(&t)));
        }
    }
    out
}

/// [`styled_caption`] inside an HTML table cell: the caption's paragraphs
/// in HTML after a `<br>` (a picture or table in a merged or
/// nested-table cell lost its caption and number).
/// 이것을 실패시키는 것: HTML 칸의 캡션 출력을 지우기
/// (`caption_pictures_and_html_table_captions_are_kept`).
fn html_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Html {
    let Some(caption) = caption else { return Html::new() };
    let Some(ctx) = ctx.enter() else { return Html::syntax(TOO_DEEP) };
    let mut out = Html::new();
    for p in &caption.paragraphs {
        let t = extract_paragraph_text_html(p, styles, st, ctx);
        if !t.as_str().trim().is_empty() {
            out.push_syntax("<br>");
            out.push(t);
        }
    }
    out
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
    // decoder carry is still detected as code if it would
    // have qualified before.
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
    use crate::encoder::markup::escape_html;
    use hwpforge_core::{
        control::{DutmalAlign, DutmalPosition, ShapePoint},
        Document, Image, ImageFormat, Paragraph, Run, Section, Table, TableCell, TableRow,
    };
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, StyleRef};

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

        fn style_name(&self, id: StyleRef) -> Option<&str> {
            self.style_names.get(&id.get()).map(String::as_str)
        }

        fn style_heading_level(&self, id: StyleRef) -> Option<u8> {
            self.heading_styles.get(&id.get()).copied()
        }

        fn image_data(&self, key: &str) -> Option<&[u8]> {
            self.image_data.get(key).map(|v| v.as_slice())
        }
    }

    fn draft_document(paragraphs: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, hwpforge_core::PageSettings::a4()));
        doc
    }

    fn validated_document(paragraphs: Vec<Paragraph>) -> Document<Validated> {
        draft_document(paragraphs).validate().unwrap()
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
        // Markdown is still read inside inline HTML: the text is escaped.
        assert_eq!(
            fmt.wrap("**산·학·연 협력** 가점", Ctx::default()),
            "<strong>\\*\\*산·학·연 협력\\*\\* 가점</strong>"
        );
    }

    #[test]
    fn inline_format_no_conflict_uses_markdown() {
        let fmt = InlineFormat { bold: true, ..Default::default() };
        assert_eq!(fmt.wrap("기관명칭 기입 요망", Ctx::default()), "**기관명칭 기입 요망**");
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
        .with_style(StyleRef::new(1));
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
        .with_style(StyleRef::new(3));
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
        .with_style(StyleRef::new(2));
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
        .with_style(StyleRef::new(2));
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
        .with_style(StyleRef::new(10));
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
        .with_style(StyleRef::new(2));
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
        let mut st = State::new();
        let result = table_to_styled_markdown(&table, &styles, false, &mut st, Ctx::default());
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    text_margin: None,
                    paragraphs: vec![textbox_body],
                    width: HwpUnit::from_mm(80.0).unwrap(),
                    height: HwpUnit::from_mm(40.0).unwrap(),
                    placement: None,
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    start: ShapePoint::new(0, 0),
                    end: ShapePoint::new(1000, 0),
                    width: HwpUnit::from_mm(50.0).unwrap(),
                    height: HwpUnit::from_mm(1.0).unwrap(),
                    placement: None,
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    center: ShapePoint::new(500, 300),
                    axis1: ShapePoint::new(1000, 300),
                    axis2: ShapePoint::new(500, 600),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    vertices: vec![
                        ShapePoint::new(0, 1000),
                        ShapePoint::new(500, 0),
                        ShapePoint::new(1000, 1000),
                    ],
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(30.0).unwrap(),
                    placement: None,
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
        assert_eq!(fmt.wrap("", Ctx::default()), "");
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
        fn style_name(&self, id: StyleRef) -> Option<&str> {
            self.base.style_name(id)
        }
        fn style_heading_level(&self, id: StyleRef) -> Option<u8> {
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
        assert_eq!(fmt.wrap("underlined", Ctx::default()), "<u>underlined</u>");
    }

    #[test]
    fn inline_format_superscript_wraps_as_html_sup() {
        let fmt = InlineFormat { superscript: true, ..Default::default() };
        assert_eq!(fmt.wrap("sup text", Ctx::default()), "<sup>sup text</sup>");
    }

    #[test]
    fn inline_format_subscript_wraps_as_html_sub() {
        let fmt = InlineFormat { subscript: true, ..Default::default() };
        assert_eq!(fmt.wrap("sub text", Ctx::default()), "<sub>sub text</sub>");
    }

    #[test]
    fn inline_format_bold_and_underline_combines_markdown_and_html() {
        let fmt = InlineFormat { bold: true, underline: true, ..Default::default() };
        let result = fmt.wrap("text", Ctx::default());
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

    // --- Markdown text (`Md::text`) ---

    #[test]
    fn markdown_text_escapes_special_chars() {
        let result = Md::text("*bold* and _italic_ and `code`");
        assert!(result.contains("\\*bold\\*"));
        assert!(result.contains("\\_italic\\_"));
        assert!(result.contains("\\`code\\`"));
    }

    #[test]
    fn markdown_text_escapes_angle_brackets() {
        let result = Md::text("<tag>");
        assert_eq!(result, "&lt;tag&gt;");
    }

    #[test]
    fn markdown_text_escapes_hash() {
        let result = Md::text("# heading");
        assert!(result.contains("\\#"));
    }

    #[test]
    fn markdown_text_passes_through_normal_text() {
        let result = Md::text("hello world 안녕하세요");
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

    // --- GFM cell (`Md::into_cell`) ---

    #[test]
    fn cell_escapes_pipe_and_newline() {
        let result = Md::text("a|b\nc").into_cell();
        assert_eq!(result, "a\\|b<br>c");
    }

    // --- image_alt_text and image_rel_path ---

    #[test]
    fn image_alt_text_extracts_stem_from_path() {
        assert_eq!(image_stem("images/photo.jpg"), "photo");
        assert_eq!(image_stem("BinData/logo.png"), "logo");
        assert_eq!(image_stem("simple"), "simple");
    }

    #[test]
    fn image_alt_text_escapes_brackets_in_stem() {
        assert_eq!(Md::text(image_stem("[photo].png")), "\\[photo\\]");
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

    /// A picture path cannot end the image early or hold raw HTML: the key
    /// is written as a destination that reads back as it is (here in the
    /// `<…>` form, for its space).
    ///
    /// 이것을 실패시키는 것: 목적지 escape 삭제(`markup::destination`).
    #[test]
    fn picture_path_cannot_end_early() {
        let styles = MockStyles::new();
        let key = image_rel_path("x/a)<img src=y>.png", &styles);
        assert_eq!(key, "images/a%29<img src=y>.png");
        assert_eq!(
            Md::image(Md::text("a"), &key, &mut Default::default()),
            "![a](<images/a%29\\<img src=y\\>.png>)"
        );
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
        // Markdown text: escaped, so the blank is not a thematic break.
        assert_eq!(output.markdown, "\\_\\_\\_\\_");
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
            // Brackets as text (escaped), so they cannot open a link.
            output.markdown.contains("\\[anchor1\\]"),
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
                    definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
                    frame: hwpforge_core::ObjectFrame::new(),
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
                    placement: None,
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
                    frame: hwpforge_core::ObjectFrame::new(),
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
                    placement: None,
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
                    frame: hwpforge_core::ObjectFrame::new(),
                    start: ShapePoint { x: 0, y: 0 },
                    end: ShapePoint { x: 1000, y: 500 },
                    control_points: vec![],
                    connect_type: "STRAIGHT".to_string(),
                    width: HwpUnit::from_mm(40.0).unwrap(),
                    height: HwpUnit::from_mm(20.0).unwrap(),
                    placement: None,
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
        .with_style(StyleRef::new(5));
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
        .with_style(StyleRef::new(6));
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
        .with_style(StyleRef::new(3));
        let doc = validated_document(vec![para]);
        let mut styles = MockStyles::new();
        styles.heading_styles.insert(3, 1);

        let output = encode_styled(&doc, &styles);
        assert_eq!(output.markdown, "");
    }

    /// Every comment at a line start moves after the text, in order; a line
    /// left empty goes; a text of comments only stays as it is.
    /// 이것을 실패시키는 것: 주석만 남은 줄 남기기.
    #[test]
    fn comments_after_text_moves_line_start_comments() {
        let (c, m) = ("<!-- chart -->", "<!-- memo: m -->");
        assert_eq!(comments_after_text(&format!("{c} a\n{m}\nb")), format!("a\nb{c}{m}"));
        assert_eq!(comments_after_text(&format!("{c}{m}")), format!("{c}{m}"));
    }

    // --- Memo body is one well-formed comment ---

    /// Every `-` is escaped (an odd run too: no `--` is left to close the
    /// comment) and line endings, a blank line too, fold to one space, so
    /// the parser reads one comment, which the guard classifies as a memo.
    /// 이것을 실패시키는 것: `--` 쌍만 이스케이프 · 줄바꿈 접기 삭제.
    #[test]
    fn control_memo_sanitizes_double_dash_to_prevent_comment_breakout() {
        use hwpforge_core::control::MemoMetadata;
        let memo_body = Paragraph::with_runs(
            vec![Run::text("bad --- con\n\ntent", CharShapeIndex::new(0))],
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
        assert_eq!(output.markdown, "<!-- memo: bad \\-\\-\\- con tent -->");
        let events: Vec<_> =
            pulldown_cmark::Parser::new_ext(&output.markdown, super::super::md_options()).collect();
        assert!(
            matches!(events.as_slice(), [_, pulldown_cmark::Event::Html(h), _] if h.as_ref() == output.markdown),
            "{events:?}"
        );
        assert_eq!(
            super::super::guard::comment_kind(&output.markdown),
            Some(super::super::guard::Comment::Memo)
        );
    }

    // --- Hyperlink URL escaping ---

    #[test]
    fn hyperlink_url_parentheses_are_escaped() {
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
        // Escaped with `\\`, so the destination reads back as the URL.
        assert_eq!(output.markdown, "[link](https://example.com/path\\(1\\))");
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

    // -----------------------------------------------------------------------
    // WS: groups, the render context and the asset table
    // -----------------------------------------------------------------------

    fn u(v: i32) -> HwpUnit {
        HwpUnit::new(v).unwrap()
    }

    fn pic(path: &str) -> Image {
        Image::new(path, u(100), u(100), ImageFormat::Png)
    }

    fn t(text: &str) -> Run {
        Run::text(text, CharShapeIndex::new(0))
    }

    fn para(runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(0))
    }

    fn group(children: Vec<hwpforge_core::control::GroupChild>) -> Run {
        Run::control(
            Control::Group {
                children,
                width: u(1000),
                height: u(1000),
                frame: hwpforge_core::ObjectFrame::new(),
                placement: None,
                inst_id: None,
            },
            CharShapeIndex::new(0),
        )
    }

    fn text_box(paragraphs: Vec<Paragraph>) -> Control {
        Control::text_box(paragraphs, u(100), u(100))
    }

    fn note(text: Vec<Run>) -> Run {
        Run::control(Control::footnote(vec![para(text)]), CharShapeIndex::new(0))
    }

    fn styles_with(paths: &[(&str, &[u8])]) -> MockStyles {
        let mut s = MockStyles::new();
        for (p, d) in paths {
            s.image_data.insert(p.to_string(), d.to_vec());
        }
        s
    }

    fn merged_cell_table(runs: Vec<Run>) -> Run {
        let mut cell = TableCell::new(vec![para(runs)], u(1000));
        cell.col_span = 2;
        Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0))
    }

    /// A group is drawn as its children in z-order, at the place it stands,
    /// like consecutive runs: a picture link (with its bytes), a text box's
    /// text right after it, an equation, a nested group's picture.
    ///
    /// 이것을 실패시키는 것: 그림 갈래 삭제 · 중첩 재귀 삭제 · 자식 순서 뒤집기 ·
    /// 자식 사이에 구분자 추가 · Equation 갈래 누락.
    #[test]
    fn styled_group_children_in_order() {
        use hwpforge_core::control::GroupChild;
        let styles = styles_with(&[("BinData/a.png", b"A"), ("BinData/c.png", b"C")]);
        let g = group(vec![
            GroupChild::Picture(pic("BinData/a.png")),
            GroupChild::Shape(text_box(vec![para(vec![t("abc")])])),
            GroupChild::Shape(Control::equation("a over b")),
            GroupChild::Shape(Control::Group {
                children: vec![GroupChild::Picture(pic("BinData/c.png"))],
                width: u(10),
                height: u(10),
                frame: hwpforge_core::ObjectFrame::new(),
                placement: None,
                inst_id: None,
            }),
        ]);
        let out = encode_styled(&validated_document(vec![para(vec![t("x"), g])]), &styles);
        assert_eq!(out.markdown, "x ![a](images/a.png)abc$\\frac{a}{b}$ ![c](images/c.png)");
        assert_eq!(out.images.get("images/a.png").map(Vec::as_slice), Some(&b"A"[..]));
        assert_eq!(out.images.get("images/c.png").map(Vec::as_slice), Some(&b"C"[..]));
    }

    /// A heading whose only run is a group holding one picture is drawn by
    /// the group path (`# ![a](…)`), not as a standalone picture block.
    ///
    /// 이것을 실패시키는 것: 묶음을 단독 그림 블록으로 처리.
    #[test]
    fn single_group_paragraph_is_not_a_picture_block() {
        use hwpforge_core::control::GroupChild;
        let mut styles = styles_with(&[("BinData/a.png", b"A")]);
        styles.heading_paras.insert(1, 1);
        let mut p = para(vec![group(vec![GroupChild::Picture(pic("BinData/a.png"))])]);
        p.para_shape_id = ParaShapeIndex::new(1);
        let out = encode_styled(&validated_document(vec![p]), &styles);
        assert_eq!(out.markdown, "# ![a](images/a.png)");
    }

    /// In a merged (HTML) cell a group's picture is a real `<img>` element
    /// and a text box's text is HTML (`<strong>`), not escaped Markdown; the
    /// same text in the body stays Markdown.
    ///
    /// 이것을 실패시키는 것: HTML 문맥 분기 삭제(Markdown 링크가 칸에 escape 되어 들어감) ·
    /// 묶음 출력을 다시 escape · HTML 칸 도형 글자에 Markdown 렌더러.
    #[test]
    fn html_cell_shape_text_is_html() {
        use hwpforge_core::control::GroupChild;
        let mut styles = styles_with(&[("BinData/a.png", b"A")]);
        styles.bold_ids.push(1);
        let bold = Run::text("x", CharShapeIndex::new(1));
        let cell = merged_cell_table(vec![
            group(vec![GroupChild::Picture(pic("BinData/a.png"))]),
            Run::control(text_box(vec![para(vec![bold])]), CharShapeIndex::new(0)),
            Run::control(text_box(vec![para(vec![t("a*b")])]), CharShapeIndex::new(0)),
        ]);
        let out = encode_styled(
            &validated_document(vec![para(vec![t("a*b")]), para(vec![cell])]),
            &styles,
        );
        assert!(out.markdown.starts_with("a\\*b\n\n"), "{}", out.markdown);
        assert!(
            out.markdown.contains(
                "<td colspan=\"2\"><img src=\"images/a.png\" alt=\"a\"/><strong>x</strong>a*b</td>"
            ),
            "{}",
            out.markdown
        );
        assert!(out.images.contains_key("images/a.png"));
    }

    /// An inner shape inside an HTML-cell shape's text is HTML too (HWPX 2880
    /// shape): `<strong>1단계</strong><strong> 서비스 구축</strong>`.
    ///
    /// 이것을 실패시키는 것: 안쪽 도형에 HTML 문맥 전달을 끊음 · 안쪽 출력을 다시 escape.
    #[test]
    fn nested_html_cell_shape_text_is_html() {
        let mut styles = MockStyles::new();
        styles.bold_ids.push(1);
        let b = |s: &str| Run::text(s, CharShapeIndex::new(1));
        let inner = Run::control(text_box(vec![para(vec![b("1단계")])]), CharShapeIndex::new(0));
        let outer = Run::control(
            text_box(vec![para(vec![inner, b(" 서비스 구축")])]),
            CharShapeIndex::new(0),
        );
        let out = encode_styled(
            &validated_document(vec![para(vec![merged_cell_table(vec![outer])])]),
            &styles,
        );
        assert!(
            out.markdown.contains(
                "<td colspan=\"2\"><strong>1단계</strong><strong> 서비스 구축</strong></td>"
            ),
            "{}",
            out.markdown
        );
    }

    /// Notes follow the depth and memo rules: a note in a text box at depth 0
    /// is a document definition (numbered in drawing order); a note in a
    /// text box inside a note body is inline and the output decodes; a
    /// note in a memo is inline with no definition.
    ///
    /// 이것을 실패시키는 것: 깊이 조건 삭제 · 메모 조건 삭제 · 글상자에서 수집기를 새로 만듦 ·
    /// 각주 본문 진입 때 `note_depth` +1 삭제.
    #[test]
    fn notes_follow_depth_and_memo() {
        let styles = MockStyles::new();
        let tb = |runs: Vec<Run>| Run::control(text_box(vec![para(runs)]), CharShapeIndex::new(0));
        let s4 = encode_styled(
            &validated_document(vec![para(vec![
                t("본문"),
                tb(vec![t("글상자"), note(vec![t("각주")])]),
                t("끝"),
                note(vec![t("둘째")]),
            ])]),
            &styles,
        );
        assert_eq!(s4.markdown, "본문글상자[^1]끝[^2]\n\n[^1]: 각주\n\n[^2]: 둘째");
        let n2 = encode_styled(
            &validated_document(vec![para(vec![
                t("a"),
                note(vec![t("b"), tb(vec![t("c"), note(vec![t("d")])])]),
            ])]),
            &styles,
        );
        assert_eq!(n2.markdown, "a[^1]\n\n[^1]: bc(주: d)");
        assert!(crate::MdDecoder::decode_with_default(&n2.markdown).is_ok());
        let memo = Control::Memo {
            content: vec![para(vec![t("m"), note(vec![t("f")])])],
            anchor_runs: vec![t("a")],
            metadata: Default::default(),
        };
        let m3 = encode_styled(
            &validated_document(vec![para(vec![
                t("a"),
                Run::control(memo, CharShapeIndex::new(0)),
            ])]),
            &styles,
        );
        assert_eq!(m3.markdown, "a<!-- memo: m(주: f) -->a");
    }

    /// A picture in a memo is link text only — no asset — also through a
    /// group; a picture in a note body gets its asset.
    ///
    /// 이것을 실패시키는 것: 메모에서 `in_comment` 미설정(자산 등록) · 메모 본문을
    /// Markdown 렌더러가 아닌 HTML 렌더러로.
    #[test]
    fn memo_pictures_have_no_asset() {
        use hwpforge_core::control::GroupChild;
        let styles = styles_with(&[("BinData/x.png", b"X"), ("BinData/a.png", b"A")]);
        for content in [
            vec![t("m"), Run::image(pic("BinData/x.png"), CharShapeIndex::new(0))],
            vec![t("m"), group(vec![GroupChild::Picture(pic("BinData/x.png"))])],
        ] {
            let memo = Control::Memo {
                content: vec![para(content)],
                anchor_runs: vec![t("a")],
                metadata: Default::default(),
            };
            let out = encode_styled(
                &validated_document(vec![para(vec![
                    t("a"),
                    Run::control(memo, CharShapeIndex::new(0)),
                ])]),
                &styles,
            );
            assert_eq!(out.markdown, "a<!-- memo: m ![x](images/x.png) -->a");
            assert!(out.images.is_empty());
        }
        let out = encode_styled(
            &validated_document(vec![para(vec![
                t("a"),
                note(vec![t("b"), Run::image(pic("BinData/a.png"), CharShapeIndex::new(0))]),
            ])]),
            &styles,
        );
        assert_eq!(out.markdown, "a[^1]\n\n[^1]: b ![a](images/a.png)");
        assert_eq!(out.images.get("images/a.png").map(Vec::as_slice), Some(&b"A"[..]));
    }

    /// Keys follow the source picture: the same picture twice is one key and
    /// one asset; `a(1).png` and `a%281%29.png` with different bytes want the
    /// same key, so the second takes `-2` and every link keeps its own bytes,
    /// whichever comes first; a picture without bytes never shares a key with
    /// a file.
    ///
    /// Keys found by looking up the keys already taken are the keys the
    /// full walk from `a.png` finds, for every order of five of seven pictures
    /// (two file names, one another's `-2`; equal bytes, other bytes, none), and
    /// the assets end the same.
    /// 이것을 실패시키는 것: 같은 바이트 재사용 조회 삭제 · 바이트 없는 키 재사용 조회 삭제 ·
    /// 접두 기록에 넘친 키 넣기.
    #[test]
    fn asset_keys_match_the_full_walk() {
        let pool: [(&str, Option<&[u8]>); 7] = [
            ("A/a.png", Some(b"1")),
            ("B/a.png", Some(b"2")),
            ("C/a.png", None),
            ("D/a.png", Some(b"1")),
            ("E/a-2.png", Some(b"2")),
            ("F/a-2.png", None),
            ("G/a.png", None),
        ];
        let mut styles = MockStyles::new();
        for (p, d) in pool {
            if let Some(d) = d {
                styles.image_data.insert(p.to_string(), d.to_vec());
            }
        }
        // The walk before: every key from the candidate on.
        let walk = |images: &mut HashMap<String, Vec<u8>>,
                    link_only: &mut std::collections::HashSet<String>,
                    path: &str| {
            let candidate = image_rel_path(path, &styles);
            let data = styles.image_data(path);
            let mut key = candidate.clone();
            let mut n = 1;
            loop {
                match (images.get(&key), data) {
                    (Some(e), Some(d)) if e.as_slice() == d => break,
                    (None, Some(_)) if !link_only.contains(&key) => break,
                    (None, None) => break,
                    _ => {
                        n += 1;
                        key = suffixed_key(&candidate, n);
                    }
                }
            }
            match data {
                Some(d) => {
                    images.entry(key.clone()).or_insert_with(|| d.to_vec());
                }
                None => {
                    link_only.insert(key.clone());
                }
            }
            key
        };
        let mut checked = 0;
        for code in 0..7usize.pow(5) {
            let order: Vec<&str> = (0..5).map(|k| pool[code / 7usize.pow(k) % 7].0).collect();
            let mut assets = Assets::default();
            let (mut images, mut link_only) = (HashMap::new(), std::collections::HashSet::new());
            for path in &order {
                let want = walk(&mut images, &mut link_only, path);
                assert_eq!(assets.key(path, &styles, true), want, "{order:?}");
            }
            assert_eq!(assets.images, images, "{order:?}");
            assert_eq!(assets.link_only, link_only, "{order:?}");
            checked += 1;
        }
        assert_eq!(checked, 16807);
    }

    /// 이것을 실패시키는 것: 충돌 시 덮어쓰기 · 접미사 규칙 바꾸기 · 바이트 없는 링크가
    /// 남의 파일 키를 쓰기 · 파일이 바이트 없는 링크의 키를 쓰기.
    #[test]
    fn asset_keys_never_overwrite() {
        let styles = styles_with(&[("BinData/a(1).png", b"ONE"), ("BinData/a%281%29.png", b"TWO")]);
        let img = |p: &str| Run::image(pic(p), CharShapeIndex::new(0));
        let out = encode_styled(
            &validated_document(vec![para(vec![
                img("BinData/a(1).png"),
                img("BinData/a%281%29.png"),
                img("BinData/a(1).png"),
            ])]),
            &styles,
        );
        assert_eq!(out.markdown, "![a(1)](images/a%281%29.png) ![a%281%29](images/a%281%29-2.png) ![a(1)](images/a%281%29.png)");
        assert_eq!(out.images.len(), 2);
        assert_eq!(out.images["images/a%281%29.png"], b"ONE");
        assert_eq!(out.images["images/a%281%29-2.png"], b"TWO");
        let out = encode_styled(
            &validated_document(vec![para(vec![
                img("BinData/a%281%29.png"),
                img("BinData/a(1).png"),
            ])]),
            &styles,
        );
        assert_eq!(out.images["images/a%281%29.png"], b"TWO");
        assert_eq!(out.images["images/a%281%29-2.png"], b"ONE");
        // A picture without bytes never shares a key with a file, in either
        // order: its link takes a free key, and a file later wanting that key
        // moves on.
        let out = encode_styled(
            &validated_document(vec![para(vec![img("BinData/a(1).png"), img("Other/a(1).png")])]),
            &styles,
        );
        assert_eq!(out.markdown, "![a(1)](images/a%281%29.png) ![a(1)](images/a%281%29-2.png)");
        assert_eq!(out.images.len(), 1);
        let out = encode_styled(
            &validated_document(vec![para(vec![img("Other/a(1).png"), img("BinData/a(1).png")])]),
            &styles,
        );
        assert_eq!(out.markdown, "![a(1)](images/a%281%29.png) ![a(1)](images/a%281%29-2.png)");
        assert_eq!(out.images["images/a%281%29-2.png"], b"ONE");
        assert!(!out.images.contains_key("images/a%281%29.png"));
    }

    /// A GFM row's cells past the header width are not drawn: their picture
    /// and note leave no link, asset or definition.
    ///
    /// 이것을 실패시키는 것: 잘린 칸을 렌더 후 버림.
    #[test]
    fn gfm_cut_cells_leave_nothing() {
        let styles = styles_with(&[("BinData/a.png", b"A")]);
        let cell = |runs: Vec<Run>| TableCell::new(vec![para(runs)], u(1000));
        let table = Table::new(vec![
            TableRow::new(vec![cell(vec![t("h")])]),
            TableRow::new(vec![
                cell(vec![t("v")]),
                cell(vec![
                    Run::image(pic("BinData/a.png"), CharShapeIndex::new(0)),
                    note(vec![t("n")]),
                ]),
            ]),
        ]);
        let out = encode_styled(
            &validated_document(vec![para(vec![Run::table(table, CharShapeIndex::new(0))])]),
            &styles,
        );
        assert_eq!(out.markdown, "| h |\n| --- |\n| v |");
        assert!(out.images.is_empty());
    }

    /// An equation is not checked inside, but emphasis opened before it
    /// must not be open over it.
    /// 이것을 실패시키는 것: 바깥 강조 침범 검사 삭제.
    #[test]
    fn emphasis_reaching_into_math_fails() {
        // Bold opened before the equation, still open over it.
        let md = "**x$a*b$**";
        let (markers, texts) = (vec![0..2, 8..10], vec![(2..3, vec![Mark::Bold])]);
        let math = [std::ops::Range { start: 3, end: 8 }];
        assert!(!emphasis_kept(md, &markers, &texts, &math, false));
        // Closed before it: the equation's own `*` is not checked.
        let md = "**x**$a*b*c$";
        let (markers, texts) = (vec![0..2, 3..5], vec![(2..3, vec![Mark::Bold])]);
        let math = [std::ops::Range { start: 5, end: 12 }];
        assert!(emphasis_kept(md, &markers, &texts, &math, false));
    }

    /// The emphasis check fails closed: text the parser never reads as text
    /// (here inside an HTML block) is not passed; a line-start comment ends
    /// its line before the check reads (`comment_ends_line`); in a cell HTML
    /// is inline and the text read.
    /// 이것을 실패시키는 것: 읽지 않은 글을 통과시키기 · 칸을 칸 밖에서 읽기 ·
    /// 검사에 줄 끝내기를 적용하지 않기.
    #[test]
    fn emphasis_check_fails_closed() {
        // `a` is in an HTML block (the table runs to a blank line): never
        // read as text.
        let md = "<table>\n**a**";
        let (markers, texts) = (vec![8..10, 11..13], vec![(10..11, vec![Mark::Bold])]);
        assert!(!emphasis_kept(md, &markers, &texts, &[], false));
        // A comment at a line start ends its line before the check reads it,
        // as the block is written: `**a**` after it is read.
        let md = "<!-- c -->**a**";
        let (markers, texts) = (vec![10..12, 13..15], vec![(12..13, vec![Mark::Bold])]);
        assert!(emphasis_kept(md, &markers, &texts, &[], false));
        // In a cell HTML is inline.
        let md = "<table>**a**";
        let (markers, texts) = (vec![7..9, 10..12], vec![(9..10, vec![Mark::Bold])]);
        assert!(emphasis_kept(md, &markers, &texts, &[], true));
    }

    /// Both ordered range lists track insertions and whitespace removals,
    /// including multiple edits between ranges and UTF-8 after a trim.
    /// 이것을 실패시키는 것: 누적 이동량 무시 · 목록 간 커서 공유 · 변경 하나만 건너뛰기.
    #[test]
    fn emphasis_offsets_follow_all_g2_edits() {
        let mut md = String::from(" \u{3000}");
        let (mut markers, mut texts) = (Vec::new(), Vec::new());
        for i in 0..32 {
            for _ in 0..1 + i % 3 {
                md.push_str("<!-- c -->");
                md.push_str(["", " ", "\t \u{3000}", "\n"][i % 4]);
            }
            let at = md.len();
            md.push_str("**가**\n");
            markers.extend([at..at + 2, at + 5..at + 7]);
            texts.push((at + 2..at + 5, vec![Mark::Bold]));
        }
        assert!(emphasis_kept(&md, &markers, &texts, &[], false));
        texts[17].1 = vec![Mark::Italic];
        assert!(!emphasis_kept(&md, &markers, &texts, &[], false));
    }

    /// The two shapes of the earlier false positives: a group holding a text
    /// box with only a table draws the table (lossy draws nothing, it
    /// takes the box's plain text); a group holding a text box of spaces
    /// draws nothing once trimmed.
    ///
    /// 이것을 실패시키는 것: 도형 글자 잘라내기 삭제 · 묶음 글상자 표를 그리지 않기.
    #[test]
    fn false_positive_shapes() {
        use hwpforge_core::control::GroupChild;
        let styles = MockStyles::new();
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![para(vec![t("칸")])],
            u(1000),
        )])]);
        let s1 = group(vec![GroupChild::Shape(text_box(vec![para(vec![Run::table(
            table,
            CharShapeIndex::new(0),
        )])]))]);
        let out = encode_styled(&validated_document(vec![para(vec![t("앞"), s1])]), &styles);
        assert_eq!(out.markdown, "앞| 칸 |\n| --- |");
        let s2 = group(vec![GroupChild::Shape(text_box(vec![para(vec![t("  ")])]))]);
        let out =
            encode_styled(&validated_document(vec![para(vec![t("앞"), s2, t("뒤")])]), &styles);
        assert_eq!(out.markdown, "앞뒤");
    }

    /// In an HTML cell (directly or through a text box) a picture's `src`
    /// and `alt` are attribute-escaped: the decoded `src` is the asset key,
    /// the decoded `alt` the plain stem, and a quote in the name cannot open
    /// another attribute.
    ///
    /// 이것을 실패시키는 것: src·alt 이스케이프 삭제 · alt 에 Markdown 이스케이프.
    #[test]
    fn html_picture_attributes_are_escaped() {
        let unescape = |v: &str| {
            v.replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        };
        let attr = |html: &str, name: &str| {
            let start = html.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
            let end = start + html[start..].find('"').unwrap();
            html[start..end].to_string()
        };
        for path in ["BinData/a\"b.png", "BinData/[x]&y.png", "BinData/x\" onerror=\"alert(1)"] {
            let styles = styles_with(&[(path, b"P")]);
            let pic_run = Run::image(pic(path), CharShapeIndex::new(0));
            for cell in [
                merged_cell_table(vec![pic_run.clone()]),
                merged_cell_table(vec![Run::control(
                    text_box(vec![para(vec![pic_run])]),
                    CharShapeIndex::new(0),
                )]),
            ] {
                let out = encode_styled(&validated_document(vec![para(vec![cell])]), &styles);
                let img = &out.markdown[out.markdown.find("<img").unwrap()..];
                let img = &img[..img.find("/>").unwrap()];
                assert!(!img.contains("onerror=\""), "{img}");
                let key = unescape(&attr(img, "src"));
                assert_eq!(out.images.get(&key).map(Vec::as_slice), Some(&b"P"[..]), "{img}");
                assert_eq!(unescape(&attr(img, "alt")), image_stem(path), "{img}");
            }
        }
    }

    /// A link's visible text cannot inject markup: in an HTML cell — direct,
    /// through a text box, through a shape in a text box — the link is an
    /// `<a>` with an escaped href and escaped text (a dangerous scheme keeps
    /// only the escaped text); in Markdown the text is Markdown-escaped. An
    /// inline note in an HTML cell is drawn as HTML, its text escaped once
    /// (no `&amp;lt;`, no Markdown backslash).
    ///
    /// 이것을 실패시키는 것: 표시 문자열 이스케이프 삭제(HTML·Markdown 각각) ·
    /// href 이스케이프 삭제 · HTML 칸 인라인 각주를 Markdown 으로 그린 뒤 HTML
    /// escape 하기 · HTML 칸 밖 인라인 각주를 HTML 로 그리기.
    #[test]
    fn link_text_cannot_inject_markup() {
        let styles = MockStyles::new();
        let evil = "<img src=x onerror=alert(1)>";
        let link = |url: &str| {
            Run::control(
                Control::Hyperlink { text: evil.into(), url: url.into() },
                CharShapeIndex::new(0),
            )
        };
        let tb = |run: Run| Run::control(text_box(vec![para(vec![run])]), CharShapeIndex::new(0));
        let ellipse = |run: Run| {
            let mut e = Control::ellipse(u(10), u(10));
            if let Control::Ellipse { paragraphs, .. } = &mut e {
                *paragraphs = vec![para(vec![run])];
            }
            Run::control(e, CharShapeIndex::new(0))
        };
        let url = "https://e.invalid/?a=\"b";
        for cell in [
            merged_cell_table(vec![link(url)]),
            merged_cell_table(vec![tb(link(url))]),
            merged_cell_table(vec![tb(ellipse(link(url)))]),
        ] {
            let md = encode_styled(&validated_document(vec![para(vec![cell])]), &styles).markdown;
            assert!(!md.contains("<img"), "{md}");
            assert!(
                md.contains("<a href=\"https://e.invalid/?a=&quot;b\">&lt;img src=x onerror=alert(1)&gt;</a>"),
                "{md}"
            );
        }
        let md = encode_styled(
            &validated_document(vec![para(vec![merged_cell_table(vec![link("javascript:x")])])]),
            &styles,
        )
        .markdown;
        assert!(md.contains("<td colspan=\"2\">&lt;img src=x onerror=alert(1)&gt;</td>"), "{md}");
        let md = encode_styled(
            &validated_document(vec![para(vec![link("https://e.invalid")])]),
            &styles,
        )
        .markdown;
        assert_eq!(md, "[&lt;img src=x onerror=alert(1)&gt;](https://e.invalid)");
        // An allowed URL holds no whitespace but may hold `)` and `<`: its
        // destination cannot end early.
        let md = encode_styled(
            &validated_document(vec![para(vec![link("https://a)<img/src=x/onerror=y>")])]),
            &styles,
        )
        .markdown;
        assert!(md.ends_with("](https://a\\)\\<img/src=x/onerror=y\\>)"), "{md}");
        let inner_note = note(vec![t("<b>x</b>")]);
        let deep = note(vec![merged_cell_table(vec![inner_note])]);
        let md =
            encode_styled(&validated_document(vec![para(vec![t("a"), deep])]), &styles).markdown;
        assert!(!md.contains("<b>"), "{md}");
        assert!(md.contains("(주: &lt;b&gt;x&lt;/b&gt;)"), "{md}");
        let deep = note(vec![merged_cell_table(vec![note(vec![t("*x*")])])]);
        let md =
            encode_styled(&validated_document(vec![para(vec![t("a"), deep])]), &styles).markdown;
        assert!(md.contains("(주: *x*)"), "{md}");
        // Outside an HTML cell an inline note stays Markdown.
        let deep = note(vec![note(vec![t("*x*")])]);
        let md =
            encode_styled(&validated_document(vec![para(vec![t("a"), deep])]), &styles).markdown;
        assert!(md.contains("(주: \\*x\\*)"), "{md}");
    }

    /// A callback may encode another document after the outer link was
    /// drawn. Its destinations belong only to that inner encode.
    /// 이것을 실패시키는 것: 목적지 기록을 중첩 호출에서 초기화하거나 공유하기.
    #[test]
    fn nested_encode_keeps_outer_destinations() {
        struct Reentrant {
            called: std::cell::Cell<bool>,
        }
        impl StyleLookup for Reentrant {
            fn para_heading_level(&self, _: ParaShapeIndex) -> Option<u8> {
                if !self.called.replace(true) {
                    let inner = validated_document(vec![para(vec![t("inner")])]);
                    assert_eq!(crate::MdEncoder::encode_lossy(&inner).unwrap(), "inner");
                }
                None
            }
        }
        let styles = Reentrant { called: std::cell::Cell::new(false) };
        let doc = validated_document(vec![para(vec![Run::control(
            Control::Hyperlink { text: "t".into(), url: "https://e.invalid".into() },
            CharShapeIndex::new(0),
        )])]);
        let out = encode_styled(&doc, &styles);
        assert!(styles.called.get());
        assert_eq!(out.markdown, "[t](https://e.invalid)");
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    /// Note fallback starts at the note's actual container depth, and a
    /// rejected note body never exposes its original text in fallback.
    /// 이것을 실패시키는 것: 각주 원문 깊이를 0으로 초기화 · 각주 진입 검사 삭제.
    #[test]
    fn note_fallback_keeps_rendered_depth() {
        let styles = MockStyles::new();
        for depth in [0, 31, 32] {
            for boxes in [0, 31, 32, 128] {
                let mut run = t("core");
                for _ in 0..boxes {
                    run = Run::control(text_box(vec![para(vec![run])]), CharShapeIndex::new(0));
                }
                let expected = if depth + 1 + boxes <= 32 { "core" } else { TOO_DEEP };
                for control in [
                    Control::footnote(vec![para(vec![run.clone()])]),
                    Control::endnote(vec![para(vec![run])]),
                ] {
                    let mut st = State::new();
                    control_md(&control, &styles, &mut st, Ctx { depth, ..Ctx::default() });
                    let mut blocks = st.notes.render_definitions();
                    assert_eq!(blocks.len(), 1);
                    assert!(blocks[0].markdown.contains(expected));
                    // The fallback Markdown, its text escaped.
                    assert_eq!(
                        blocks[0].source.as_ref().unwrap().texts,
                        vec![super::super::guard::plain_text(expected)]
                    );
                    blocks[0].markdown = "<script>bad</script>".into();
                    let allowed = super::super::guard::Allowed {
                        tags: super::super::guard::STYLED_TAGS,
                        destinations: &st.destinations,
                        decoder_structure: true,
                    };
                    let md = super::super::guard::checked(blocks, &allowed, &mut st.warnings);
                    assert!(md.ends_with(&super::super::guard::plain_text(expected)), "{md}");
                    if expected == TOO_DEEP {
                        assert!(!md.contains("core"));
                    }
                }
            }
        }
    }

    /// Core validation admits depth 32 and rejects deeper styled inputs.
    #[test]
    fn validation_limits_styled_input_depth() {
        use hwpforge_core::control::GroupChild;
        let styles = MockStyles::new();
        let nest = |n: usize| {
            let mut run = t("core");
            for _ in 0..n {
                run = group(vec![GroupChild::Shape(text_box(vec![para(vec![run])]))]);
            }
            run
        };
        let out = encode_styled(&validated_document(vec![para(vec![nest(16)])]), &styles);
        assert_eq!(out.markdown, "core");
        assert!(out.warnings.is_empty());
        for run in [
            nest(17),
            Run::control(Control::memo(vec![para(vec![nest(16)])]), CharShapeIndex::new(0)),
        ] {
            assert!(draft_document(vec![para(vec![t("x")]), para(vec![run])]).validate().is_err());
        }
    }

    /// An equation in Markdown text writes no executable HTML and no
    /// unchecked link: `<` and `>` become `\lt` and `\gt`, and LaTeX that
    /// would end its math or start a link is written as the script text,
    /// with a warning at its place. Other Markdown inside `$…$` (`*x*`) is
    /// left to the math renderer, not made literal. An ordinary equation,
    /// Hancom's backtick space included, is unchanged.
    ///
    /// 이것을 실패시키는 것: 꺾쇠 치환 삭제 · `$`·`](`·끝 `\` 조건 하나씩 삭제 ·
    /// 백틱도 거부하기 · 경고 기록 삭제 · 경고 자리 채우기 삭제 · 깊이 초과
    /// 폴백의 `$` escape 삭제.
    #[test]
    fn markdown_equation_writes_no_html_or_link() {
        let styles = MockStyles::new();
        let eqn = |script: &str| {
            Run::control(
                Control::Equation {
                    script: script.to_string(),
                    width: HwpUnit::from_mm(30.0).unwrap(),
                    height: HwpUnit::from_mm(10.0).unwrap(),
                    base_line: 70,
                    text_color: hwpforge_foundation::Color::BLACK,
                    font: "HancomEQN".to_string(),
                    inst_id: None,
                },
                CharShapeIndex::new(0),
            )
        };
        let draw = |script: &str| {
            encode_styled(
                &validated_document(vec![para(vec![t("x")]), para(vec![eqn(script)])]),
                &styles,
            )
        };
        let out = draw("{a+b} over {c+d}");
        assert_eq!(out.markdown, "x\n\n$\\frac{a+b}{c+d}$");
        assert!(out.warnings.is_empty());
        let out = draw("<img src=x onerror=y>");
        assert!(!out.markdown.contains(['<', '>']), "{}", out.markdown);
        assert!(out.markdown.contains("\\lt ") && out.markdown.contains("\\gt "));
        assert!(out.warnings.is_empty());
        let out = draw("a`b");
        assert_eq!(out.markdown, "x\n\n$a`b$");
        assert!(out.warnings.is_empty());
        for script in ["a $ b", "[a](b)", "a \\"] {
            let out = draw(script);
            assert_eq!(
                out.warnings,
                vec![super::super::MdWarning::EquationAsText { section: 0, paragraph: 1 }],
                "{script}: {}",
                out.markdown
            );
            assert!(!out.markdown.replace("\\$", "").contains('$'), "{script}: {}", out.markdown);
        }
        assert_eq!(Md::math("$a\nb$").as_deref(), Some("$a b$"));
        // A script too deep to convert is text as well, its `$` escaped.
        let deep = format!("{}$x{}", "{".repeat(40), "}".repeat(40));
        let out = draw(&deep);
        assert!(out.markdown.contains("\\$x"), "{}", out.markdown);
        assert!(!out.markdown.replace("\\$", "").contains('$'), "{}", out.markdown);
    }

    /// An equation's LaTeX in an HTML cell is escaped text: a script that
    /// writes markup characters cannot open an element there.
    ///
    /// 이것을 실패시키는 것: LaTeX 이스케이프 삭제.
    #[test]
    fn html_cell_equation_is_escaped() {
        let styles = MockStyles::new();
        let eq =
            Run::control(Control::equation("<img src=x onerror=alert(1)>"), CharShapeIndex::new(0));
        let md = encode_styled(
            &validated_document(vec![para(vec![merged_cell_table(vec![eq])])]),
            &styles,
        )
        .markdown;
        assert!(!md.contains("<img"), "{md}");
        assert!(md.contains("&lt;"), "{md}");
    }

    /// Only an allowed URL becomes a link: a leading space, a tab or newline
    /// inside the scheme, another scheme — each keeps only the text, in an
    /// HTML cell (direct, through a text box, through a group's text box)
    /// and in Markdown; `HTTPS://` and `mailto:` stay links.
    ///
    /// 이것을 실패시키는 것: 공백·제어 문자 검사 삭제 · 허용 목록을 금지 목록으로.
    #[test]
    fn link_urls_follow_the_allow_list() {
        use hwpforge_core::control::GroupChild;
        let styles = MockStyles::new();
        let link = |url: &str| {
            Run::control(
                Control::Hyperlink { text: "클릭".into(), url: url.into() },
                CharShapeIndex::new(0),
            )
        };
        let tb = |run: Run| text_box(vec![para(vec![run])]);
        let paths = |run: Run| {
            vec![
                vec![para(vec![merged_cell_table(vec![run.clone()])])],
                vec![para(vec![merged_cell_table(vec![Run::control(
                    tb(run.clone()),
                    CharShapeIndex::new(0),
                )])])],
                vec![para(vec![merged_cell_table(vec![group(vec![GroupChild::Shape(tb(
                    run.clone()
                ))])])])],
                vec![para(vec![run])],
            ]
        };
        for url in [
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:x",
            "JaVaScRiPt:x",
            "data:text/html,x",
            "ftp://e.invalid",
            "https://e.invalid/a b",
        ] {
            for doc in paths(link(url)) {
                let md = encode_styled(&validated_document(doc), &styles).markdown;
                assert!(!md.contains("<a ") && !md.contains("]("), "{url:?}: {md}");
                assert!(md.contains("클릭"), "{md}");
            }
        }
        for url in ["HTTPS://e.invalid", "mailto:a@e.invalid"] {
            for doc in paths(link(url)) {
                let md = encode_styled(&validated_document(doc), &styles).markdown;
                assert!(md.contains("<a href=") || md.contains("]("), "{url:?}: {md}");
            }
        }
    }
}
