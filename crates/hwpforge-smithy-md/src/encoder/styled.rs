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
use super::Part;
use crate::eqn::{render_equation, EqnRender};
use crate::internal_styles::parse_list_continuation_style_name;

/// Output of style-aware markdown encoding.
///
/// Contains the generated markdown string, any extracted images (keyed by
/// their relative path within the output) and what the rendering left out.
/// Built only by the encoder (`#[non_exhaustive]`): read its fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MdOutput {
    /// The generated markdown string.
    pub markdown: String,
    /// Extracted images: relative path → binary data.
    pub images: HashMap<String, Vec<u8>>,
    /// What the rendering left out — today a container nested too deep
    /// ([`super::MdWarning::ContainerTooDeep`]); empty for most documents.
    pub warnings: Vec<super::MdWarning>,
}

const SECTION_MARKER_COMMENT: &str = "<!-- hwpforge:section -->";

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
    footnotes: Vec<Vec<String>>,
    endnotes: Vec<Vec<String>>,
}

impl FootnoteCollector {
    fn new() -> Self {
        Self { footnotes: Vec::new(), endnotes: Vec::new() }
    }

    /// Adds a footnote body and returns the inline marker `[^N]`.
    fn add_footnote(&mut self, body_paragraphs: Vec<String>) -> String {
        let n = self.footnotes.len() + 1;
        self.footnotes.push(body_paragraphs);
        format!("[^{n}]")
    }

    /// Adds an endnote body and returns the inline marker `[^eN]`.
    fn add_endnote(&mut self, body_paragraphs: Vec<String>) -> String {
        let n = self.endnotes.len() + 1;
        self.endnotes.push(body_paragraphs);
        format!("[^e{n}]")
    }

    /// Renders all collected definitions as a markdown block.
    ///
    /// Multi-paragraph bodies follow the GFM continuation form the parser
    /// actually accepts (probe P4/P12): a **blank line** between paragraphs
    /// and 4-space indentation on **every** physical line of continuation
    /// paragraphs. Line-start block markers are escaped so a body that
    /// *looks* like a list stays plain text on re-parse (H6, probe P14).
    fn render_definitions(&self) -> String {
        let mut blocks = Vec::new();
        for (i, body) in self.footnotes.iter().enumerate() {
            blocks.push(render_definition(&format!("{}", i + 1), body));
        }
        for (i, body) in self.endnotes.iter().enumerate() {
            blocks.push(render_definition(&format!("e{}", i + 1), body));
        }
        blocks.join("\n\n")
    }
}

/// Renders one `[^label]:` definition with GFM multi-paragraph continuation.
fn render_definition(label: &str, paragraphs: &[String]) -> String {
    let mut out = String::new();
    for (i, para) in paragraphs.iter().enumerate() {
        let escaped = escape_footnote_paragraph(para);
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

/// Escapes line-start block markers inside a note-definition paragraph (H6).
///
/// A body line beginning with `- ` / `* ` / `+ ` / `1. ` / `#` / `>` would
/// re-parse as a structural block inside the definition, which this decoder
/// rejects — the emitter must keep such text literal.
fn escape_footnote_paragraph(paragraph: &str) -> String {
    paragraph
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_start();
            let needs_escape = trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
                || trimmed.starts_with('#')
                || trimmed.starts_with('>')
                || is_ordered_marker(trimmed);
            if needs_escape {
                let indent_len = line.len() - trimmed.len();
                format!("{}\\{}", &line[..indent_len], trimmed)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `1. ` / `23) ` 형태의 순서 목록 마커 판정.
fn is_ordered_marker(s: &str) -> bool {
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    matches!(s.as_bytes().get(digits), Some(b'.') | Some(b')'))
        && matches!(s.as_bytes().get(digits + 1), Some(b' ') | None)
}

// ---------------------------------------------------------------------------
// Render context and the document's state
// ---------------------------------------------------------------------------

/// Where a piece of content is being drawn. Every recursion passes it on;
/// the places that change it are: an HTML table cell (`html`), a note body
/// (Markdown, `note_depth + 1`) and a memo body (Markdown, `in_comment`).
/// Text boxes, shapes and groups pass it on unchanged.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    /// Inside a merged or nested-table cell: inline HTML, not Markdown.
    html: bool,
    /// How many note bodies enclose this content. Only depth 0 registers a
    /// note definition; deeper notes are written inline as `(주: …)`.
    note_depth: u32,
    /// Inside a memo (`<!-- memo: … -->`): pictures are written as link
    /// text only (no asset), notes inline.
    in_comment: bool,
    /// How many containers (tables, captions, shape texts, note and memo
    /// bodies) enclose this content — each one is a level of recursion.
    depth: usize,
}

/// Written in place of a container nested deeper than
/// [`MAX_NESTING_DEPTH`](crate::lossless_grammar::MAX_NESTING_DEPTH): the
/// styled view is infallible, so it says what it left out instead of
/// recursing without bound.
const TOO_DEEP: &str = "(중첩 깊이 초과로 생략)";

impl Ctx {
    fn markdown(self) -> Self {
        Self { html: false, ..self }
    }

    /// The context one container deeper, or `None` past the limit.
    /// 이것을 실패시키는 것: 한도 검사 삭제 · 깊이 증가 삭제
    /// (`deep_containers_are_cut_off`).
    fn enter(self) -> Option<Self> {
        (self.depth < crate::lossless_grammar::MAX_NESTING_DEPTH)
            .then_some(Self { depth: self.depth + 1, ..self })
    }
}

/// What the whole document collects while it is drawn: the note
/// definitions and the image assets.
struct State {
    notes: FootnoteCollector,
    assets: Assets,
    /// What was left out, with where (`at`).
    warnings: Vec<super::MdWarning>,
    /// The section and top-level paragraph being drawn.
    at: (usize, usize),
}

impl State {
    fn new() -> Self {
        Self {
            notes: FootnoteCollector::new(),
            assets: Assets::default(),
            warnings: Vec::new(),
            at: (0, 0),
        }
    }

    /// The notice written for a `container` past the depth limit, recorded
    /// as a warning at the current place.
    /// 이것을 실패시키는 것: 경고 기록 삭제(`deep_containers_are_cut_off`).
    fn too_deep(&mut self, container: &'static str) -> String {
        self.warnings.push(super::MdWarning::ContainerTooDeep {
            container,
            limit: crate::lossless_grammar::MAX_NESTING_DEPTH,
            section: self.at.0,
            paragraph: self.at.1,
        });
        TOO_DEEP.to_string()
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
        let mut key = candidate.clone();
        let mut n = 1;
        loop {
            match (self.images.get(&key), data) {
                (Some(existing), Some(d)) if existing.as_slice() == d => break,
                (None, Some(_)) if !self.link_only.contains(&key) => break,
                (None, None) => break,
                _ => {
                    n += 1;
                    key = suffixed_key(&candidate, n);
                }
            }
        }
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

/// A picture's link in the context's form — `![alt](key)` in Markdown,
/// `<img src="key" alt="alt"/>` in HTML — with its asset registered unless
/// inside a memo.
fn emit_picture(
    image: &hwpforge_core::image::Image,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let key = st.assets.key(&image.path, styles, !ctx.in_comment);
    if ctx.html {
        // Attribute values: escaped for HTML (the asset key itself is kept
        // as it is), with the plain file stem as alt — no Markdown escaping.
        // 이것을 실패시키는 것: src·alt 이스케이프 삭제 · alt 에 Markdown 이스케이프
        // (`html_picture_attributes_are_escaped`).
        format!(
            "<img src=\"{}\" alt=\"{}\"/>",
            escape_html(&key),
            escape_html(image_stem(&image.path))
        )
    } else {
        format!("![{}]({key})", image_alt_text(&image.path))
    }
}

/// Encodes a validated document into style-aware markdown.
///
/// Queries the provided [`StyleLookup`] for character/paragraph/style
/// properties to emit inline formatting and heading markers.
pub(crate) fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
    let mut blocks = Vec::new();
    let mut st = State::new();

    for (section_index, section) in document.sections().iter().enumerate() {
        if section_index > 0 {
            blocks.push(SECTION_MARKER_COMMENT.to_string());
        }

        let mut code_block_lines: Vec<String> = Vec::new();
        let mut continuation: Option<ListContinuationContext> = None;

        for (paragraph_index, paragraph) in section.paragraphs.iter().enumerate() {
            st.at = (section_index, paragraph_index);
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
            let markdown = encode_paragraph_styled(paragraph, styles, &mut st);
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
    let definitions = st.notes.render_definitions();
    let mut markdown = blocks.join("\n\n");
    if !definitions.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&definitions);
    }

    MdOutput { markdown, images: st.assets.images, warnings: st.warnings }
}

/// Encodes a single top-level paragraph into styled markdown.
fn encode_paragraph_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
) -> String {
    let ctx = Ctx::default();
    // Handle single-run block elements (table, image) as standalone blocks.
    // A paragraph whose one run is a group is not one of them, even when the
    // group holds a single picture (WS: the group is drawn as its children).
    if paragraph.runs.len() == 1 {
        match &paragraph.runs[0].content {
            RunContent::Table(table) => {
                let mut md = table_to_styled_markdown(table, styles, false, st, ctx);
                md.push_str(&styled_caption(table.caption.as_ref(), styles, st, ctx));
                return md;
            }
            RunContent::Image(image) => {
                let mut md = emit_picture(image, styles, st, ctx);
                md.push_str(&styled_caption(image.caption.as_ref(), styles, st, ctx));
                return md;
            }
            _ => {}
        }
    }

    let text = paragraph_text_styled(paragraph, styles, st, ctx);

    // Shared outline/list classification (hwpforge-core). Classification is
    // text-blind; the empty-text guards below are rendering concerns (an
    // empty heading/list item renders as nothing).
    match classify_paragraph(paragraph, styles) {
        ParaKind::Heading { level, .. } => {
            // Headings must be single-line: collapse lineBreak-originated newlines.
            let heading_text = text.trim().replace('\n', " ");
            if heading_text.is_empty() {
                return String::new();
            }
            format!("{} {}", "#".repeat(level as usize), heading_text)
        }
        ParaKind::ListItem { kind, level, checked, .. } => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            format_list_item(trimmed, kind, level, checked)
        }
        ParaKind::Body => text.trim_start().to_string(),
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

/// A paragraph in Markdown with inline formatting applied (`ctx.html` is
/// ignored: this is the Markdown renderer).
fn paragraph_text_styled(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let ctx = ctx.markdown();
    let mut output = String::new();

    // Group consecutive text runs by InlineFormat for proper wrapping.
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for (part, cs) in super::spread_groups(&paragraph.runs) {
        match part {
            // InlineText folds to the same `\t`-bearing string via
            // `plain_text()` because Markdown styling has no
            // representation for `<hp:tab>` attributes. The
            // formatting group key still uses `char_shape_id`, so the
            // run gets wrapped exactly like a plain `Text(String)` run
            // — see debug doc §3a-A5.
            Part::Text(content) => {
                let Some(cow) = content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(cs, styles);
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
            Part::Image(image) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&emit_picture(image, styles, st, ctx));
                output.push_str(&styled_caption(image.caption.as_ref(), styles, st, ctx));
            }
            Part::Table(table) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&table_to_styled_markdown(table, styles, false, st, ctx));
                output.push_str(&styled_caption(table.caption.as_ref(), styles, st, ctx));
            }
            Part::Control(control) => {
                // Flush text group.
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                let text = encode_control_styled(control, styles, st, ctx);
                // A caption number's prefix/suffix characters are text, not
                // Markdown (review r1: a `*` prefix and suffix turned "3"
                // into emphasis). 이것을 실패시키는 것: 이 이스케이프를 지우기
                // (`caption_number_prefix_is_escaped`).
                if matches!(*control, Control::AutoNumber { .. }) {
                    output.push_str(&escape_markdown(&text));
                } else {
                    output.push_str(&text);
                }
            }
        }
    }

    // Flush remaining text.
    if !current_text.is_empty() {
        output.push_str(&current_format.wrap(&current_text));
    }

    output
}

/// Encodes a single control element into styled markdown.
fn encode_control_styled(
    control: &Control,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    match control {
        // The visible text is text in either form; in an HTML cell the link is
        // an `<a>` element with its href attribute-escaped. Only an allowed
        // URL (`link_url_allowed`) is written as a link.
        // 이것을 실패시키는 것: 표시 문자열 이스케이프 삭제(HTML·Markdown 각각) ·
        // href 이스케이프 삭제(`link_text_cannot_inject_markup`).
        Control::Hyperlink { text, url } => {
            match (ctx.html, super::link_url_allowed(url)) {
                // Not an allowed link: only the visible text
                (true, false) => escape_html(text),
                (true, true) => {
                    format!("<a href=\"{}\">{}</a>", escape_html(url), escape_html(text))
                }
                (false, false) => escape_markdown(text),
                (false, true) => {
                    // Escape ) in url to prevent markdown injection
                    let safe_url = url.replace('(', "%28").replace(')', "%29");
                    format!("[{}]({safe_url})", escape_markdown(text))
                }
            }
        }
        // A note at depth 0 outside a memo is a document definition; a note
        // inside a note body (directly or through a shape or group) or inside
        // a memo is written inline. 이것을 실패시키는 것: 깊이 조건 삭제 · 메모
        // 조건 삭제(`notes_follow_depth_and_memo`).
        Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
            let body = extract_note_body(paragraphs, styles, st, ctx);
            if ctx.note_depth > 0 || ctx.in_comment {
                // The body is Markdown text; in an HTML cell it is escaped
                // like other text (the cell passes notes through as-is).
                let inline = format!("(주: {})", body.join(" "));
                if ctx.html {
                    escape_html(&inline)
                } else {
                    inline
                }
            } else if matches!(control, Control::Footnote { .. }) {
                st.notes.add_footnote(body)
            } else {
                st.notes.add_endnote(body)
            }
        }
        Control::TextBox { paragraphs, .. } => shape_text(paragraphs, styles, st, ctx),
        Control::Equation { script, .. } => match render_equation(script) {
            EqnRender::Latex(latex) => latex,
            // Written as ordinary text (whitespace already collapsed):
            // Markdown-escaped here, and table cells apply their usual cell
            // escaping on top, like text runs.
            EqnRender::RawFallback(raw) => escape_markdown(&raw),
        },
        Control::Chart { .. } => "<!-- chart -->".to_string(),
        Control::Line { .. } => String::new(),
        Control::Ellipse { paragraphs, .. } | Control::Polygon { paragraphs, .. } => {
            shape_text(paragraphs, styles, st, ctx)
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
            // Drawn as Markdown (`paragraph_text_styled`); an HTML cell
            // escapes the whole memo after.
            let Some(inner) = ctx.enter() else { return st.too_deep("memo") };
            let inner = Ctx { in_comment: true, ..inner };
            let body = content
                .iter()
                .map(|p| paragraph_text_styled(p, styles, st, inner))
                .collect::<Vec<_>>()
                .join(" ");
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
        Control::NewNumber { .. } | Control::PageHiding { .. } => {
            // 새 번호 지정·감추기는 렌더 지시라 Markdown 본문 표현이
            // 없다 — 명시적 no-op (W2·W3, IndexMark 와 동일 정책).
            String::new()
        }
        // Plain text: the Markdown caller escapes it as Markdown, the HTML
        // table caller as HTML (superscript is not drawn).
        Control::AutoNumber { num, format, .. } => super::auto_number_text(*num, format),
        Control::Arc { .. } | Control::Curve { .. } | Control::ConnectLine { .. } => {
            // These shapes rarely contain text; render nothing.
            String::new()
        }
        Control::Unknown { tag, .. } => format!("`[{tag}]`"),
        _ => String::new(),
    }
}

/// A note body's paragraphs, each drawn on its own (multi-paragraph
/// preserved) in Markdown (`paragraph_text_styled`) one note level deeper.
fn extract_note_body(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> Vec<String> {
    let Some(inner) = ctx.enter() else { return vec![st.too_deep("note")] };
    let inner = Ctx { note_depth: ctx.note_depth + 1, ..inner };
    paragraphs
        .iter()
        .map(|p| paragraph_text_styled(p, styles, st, inner).trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A text box's or shape's text in the context's form: each paragraph drawn
/// in Markdown or HTML, joined by a space and trimmed. In an HTML cell the
/// result is HTML already and is not escaped again.
/// 이것을 실패시키는 것: HTML 분기 삭제(Markdown 렌더러로)
/// (`html_cell_shape_text_is_html`).
fn shape_text(
    paragraphs: &[Paragraph],
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let Some(ctx) = ctx.enter() else { return st.too_deep("shape text") };
    let parts: Vec<String> = paragraphs
        .iter()
        .map(|p| {
            if ctx.html {
                extract_paragraph_text_html(p, styles, st, ctx)
            } else {
                paragraph_text_styled(p, styles, st, ctx)
            }
        })
        .collect();
    parts.join(" ").trim().to_string()
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
    st: &mut State,
    ctx: Ctx,
) -> String {
    if table.rows.is_empty() {
        return "| |\n| --- |".to_string();
    }

    if has_merge(table) || has_nested_table(table) || html_context {
        table_to_html(table, styles, st, ctx)
    } else {
        table_to_gfm(table, styles, st, ctx)
    }
}

/// Renders a table as GFM (GitHub Flavored Markdown) pipe table. The header
/// row fixes the width; a later row's cells past it are not drawn at all,
/// so they leave no picture asset and no note definition behind.
/// 이것을 실패시키는 것: 잘릴 칸도 그린 뒤 버리기(`gfm_cut_cells_leave_nothing`).
fn table_to_gfm(table: &Table, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> String {
    let Some(ctx) = ctx.enter() else { return st.too_deep("table") };
    let ctx = ctx.markdown();
    let width = table.rows.first().map_or(0, |r| r.cells.len()).max(1);
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (ri, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::new();
        for cell in row.cells.iter().take(if ri == 0 { usize::MAX } else { width }) {
            let mut parts = Vec::new();
            for p in &cell.paragraphs {
                parts.push(paragraph_text_styled(p, styles, st, ctx));
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
        // Pad a short row to the header's column count for valid GFM.
        let mut padded = row.clone();
        padded.resize(col_count, String::new());
        lines.push(format!("| {} |", padded.join(" | ")));
    }

    lines.join("\n")
}

/// Renders a table as HTML `<table>` with colspan/rowspan attributes.
fn table_to_html(table: &Table, styles: &dyn StyleLookup, st: &mut State, ctx: Ctx) -> String {
    let Some(ctx) = ctx.enter() else { return st.too_deep("table") };
    let ctx = Ctx { html: true, ..ctx };
    let mut lines = Vec::new();
    lines.push("<table>".to_string());
    lines.push("<tbody>".to_string());

    for row in &table.rows {
        lines.push("<tr>".to_string());
        for cell in &row.cells {
            let mut parts = Vec::new();
            for p in &cell.paragraphs {
                parts.push(extract_paragraph_text_html(p, styles, st, ctx));
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

    lines.join("\n")
}

/// Extracts text from a paragraph using HTML inline formatting.
fn extract_paragraph_text_html(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let ctx = Ctx { html: true, ..ctx };
    let mut output = String::new();
    let mut current_format = InlineFormat::default();
    let mut current_text = String::new();

    for (part, cs) in super::spread_groups(&paragraph.runs) {
        match part {
            // Same as the Markdown styled path (debug doc §3a-A6):
            // `InlineText` folds to its `\t`-bearing plain string via
            // `plain_text()` so HTML output stays consistent.
            Part::Text(content) => {
                let Some(cow) = content.plain_text() else {
                    continue;
                };
                let fmt = InlineFormat::from_style(cs, styles);
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
            Part::Control(control) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                // Equations: the LaTeX is cell text, HTML-escaped (a `<` or
                // `&` in it is a character, not markup — a math renderer reads
                // the decoded text); a script too deep to convert is ordinary
                // cell text (whitespace collapsed), escaped the same way.
                // 이것을 실패시키는 것: LaTeX 이스케이프 삭제(`html_cell_equation_is_escaped`).
                if let Control::Equation { script, .. } = control {
                    match render_equation(script) {
                        EqnRender::Latex(latex) => output.push_str(&escape_html(&latex)),
                        EqnRender::RawFallback(raw) => output.push_str(&escape_html(&raw)),
                    }
                    continue;
                }
                let ctrl_output = encode_control_styled(control, styles, st, ctx);
                // HTML-escape text-bearing control output in HTML table context.
                // Only structural/safe outputs (footnote markers, hyperlinks,
                // chart comments, empty shapes) and shape text — drawn as HTML
                // already — skip escaping.
                let escaped = match control {
                    Control::TextBox { .. }
                    | Control::Ellipse { .. }
                    | Control::Polygon { .. }
                    | Control::Hyperlink { .. }
                    | Control::Footnote { .. }
                    | Control::Endnote { .. }
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
            Part::Image(image) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                output.push_str(&emit_picture(image, styles, st, ctx));
                output.push_str(&html_caption(image.caption.as_ref(), styles, st, ctx));
            }
            Part::Table(table) => {
                if !current_text.is_empty() {
                    output.push_str(&current_format.wrap_html(&current_text));
                    current_text.clear();
                    current_format = InlineFormat::default();
                }
                output.push_str(&table_to_html(table, styles, st, ctx));
                output.push_str(&html_caption(table.caption.as_ref(), styles, st, ctx));
            }
        }
    }

    if !current_text.is_empty() {
        output.push_str(&current_format.wrap_html(&current_text));
    }

    output
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the filename stem as alt text from an image path.
/// The file stem of an image path (alt text before any escaping).
fn image_stem(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().and_then(|name| name.split('.').next()).unwrap_or("image")
}

fn image_alt_text(path: &str) -> String {
    // Escape characters that break markdown image syntax.
    image_stem(path).replace('[', "\\[").replace(']', "\\]")
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

/// A picture's or table's caption as a paragraph after it (`\n\n` + its
/// text) and the images it holds, empty without one. The caption's numbers
/// ("그림 3") live here (§19.16 J, review r1: captions were not written at
/// all; review r2: a picture inside the caption lost its bytes).
/// 이것을 실패시키는 것: 캡션 출력을 지우기(F17 의 3·4·7 이 사라짐 —
/// `f17_caption_numbers_reach_the_readable_views`) / 캡션 그림의 바이트를
/// 버리기(`caption_pictures_and_html_table_captions_are_kept`).
fn styled_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let Some(caption) = caption else { return String::new() };
    let Some(ctx) = ctx.enter() else { return st.too_deep("caption") };
    let mut text = Vec::new();
    for p in &caption.paragraphs {
        let t = paragraph_text_styled(p, styles, st, ctx);
        if !t.trim().is_empty() {
            text.push(t);
        }
    }
    if text.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", text.join("\n\n"))
    }
}

/// [`styled_caption`] inside an HTML table cell: the caption's paragraphs
/// in HTML after a `<br>` (review r2: a picture or table in a merged or
/// nested-table cell lost its caption and number).
/// 이것을 실패시키는 것: HTML 칸의 캡션 출력을 지우기
/// (`caption_pictures_and_html_table_captions_are_kept`).
fn html_caption(
    caption: Option<&hwpforge_core::caption::Caption>,
    styles: &dyn StyleLookup,
    st: &mut State,
    ctx: Ctx,
) -> String {
    let Some(caption) = caption else { return String::new() };
    let Some(ctx) = ctx.enter() else { return st.too_deep("caption") };
    let mut text = Vec::new();
    for p in &caption.paragraphs {
        let t = extract_paragraph_text_html(p, styles, st, ctx);
        if !t.trim().is_empty() {
            text.push(t);
        }
    }
    if text.is_empty() {
        String::new()
    } else {
        format!("<br>{}", text.join("<br>"))
    }
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
    /// same text in the body stays Markdown (S5).
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
    /// is a document definition (S4, numbered in drawing order); a note in a
    /// text box inside a note body is inline (N2) and the output decodes; a
    /// note in a memo is inline with no definition (M3).
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
        assert_eq!(m3.markdown, "a<!-- memo: m(주: f) -->");
    }

    /// A picture in a memo is link text only — no asset — also through a
    /// group; a picture in a note body gets its asset (N4).
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
            assert_eq!(out.markdown, "a<!-- memo: m ![x](images/x.png) -->");
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

    /// The two shapes of the earlier false positives: a group holding a text
    /// box with only a table draws the table (S1 — lossy draws nothing, it
    /// takes the box's plain text); a group holding a text box of spaces
    /// draws nothing once trimmed (S2).
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
    /// inline note in an HTML cell is escaped too.
    ///
    /// 이것을 실패시키는 것: 표시 문자열 이스케이프 삭제(HTML·Markdown 각각) ·
    /// href 이스케이프 삭제 · HTML 칸 인라인 각주 이스케이프 삭제.
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
        let inner_note = note(vec![t("<b>x</b>")]);
        let deep = note(vec![merged_cell_table(vec![inner_note])]);
        let md =
            encode_styled(&validated_document(vec![para(vec![t("a"), deep])]), &styles).markdown;
        assert!(!md.contains("<b>"), "{md}");
        assert!(md.contains("(주: &amp;lt;b&amp;gt;x&amp;lt;/b&amp;gt;)"), "{md}");
    }

    /// Containers nested past the limit are cut off with a visible notice
    /// and one warning naming the container and its place, instead of
    /// recursing (group → text box → group …); a shallower nesting is drawn
    /// whole with no warning.
    ///
    /// 이것을 실패시키는 것: 한도 검사 삭제 · 깊이 증가 삭제 · 경고 기록 삭제 ·
    /// 자리(`at`) 갱신 삭제.
    #[test]
    fn deep_containers_are_cut_off() {
        use hwpforge_core::control::GroupChild;
        let styles = MockStyles::new();
        let nest = |n: usize| {
            let mut run = t("core");
            for _ in 0..n {
                let tb = text_box(vec![para(vec![run])]);
                run = group(vec![GroupChild::Shape(tb)]);
            }
            run
        };
        let out = encode_styled(&validated_document(vec![para(vec![nest(32)])]), &styles);
        assert_eq!(out.markdown, "core");
        assert!(out.warnings.is_empty());
        let out = encode_styled(
            &validated_document(vec![para(vec![t("x")]), para(vec![nest(33)])]),
            &styles,
        );
        assert_eq!(out.markdown, format!("x\n\n{TOO_DEEP}"));
        assert_eq!(
            out.warnings,
            vec![super::super::MdWarning::ContainerTooDeep {
                container: "shape text",
                limit: 32,
                section: 0,
                paragraph: 1,
            }]
        );
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
