//! The readable views' last check: the finished Markdown is parsed again
//! (pulldown-cmark — tables, footnotes, strikethrough; styled with the
//! decoder's options) and must hold only what the encoder itself writes —
//! raw HTML from its own tag list, links and images to the destinations it
//! wrote, no link reference definition, and for styled no definition list
//! and no block structure made by a Body block's text. A block that holds anything else is written again as
//! escaped plain text ([`plain_text`]) and reported. The escaping rules
//! (`markup.rs`) are meant to make this never happen; this catches what a
//! rule misses.

use std::collections::HashSet;

use pulldown_cmark::{Event, Options, Parser, Tag};

/// What a view may write besides text.
pub(crate) struct Allowed<'a> {
    /// HTML tag names, from [`STYLED_TAGS`] or [`LOSSY_TAGS`]; the
    /// attributes are [`ATTRIBUTES`], the comments [`Comment`].
    pub(crate) tags: &'static [&'static str],
    /// Link and image destinations the encoder wrote (`a href`, `img src`).
    pub(crate) destinations: &'a HashSet<String>,
    /// Parse as the Markdown decoder does ([`super::md_options`]) and count
    /// a definition list as foreign too: the decoder fails on it. The
    /// styled view sets it; lossy keeps its own check, which also counts a
    /// note definition as foreign (lossy writes none).
    pub(crate) decoder_structure: bool,
}

/// The tags the styled view writes.
pub(crate) const STYLED_TAGS: &[&str] =
    &["table", "tbody", "tr", "td", "br", "strong", "em", "del", "u", "sup", "sub", "a", "img"];

/// The tags the lossy view writes.
pub(crate) const LOSSY_TAGS: &[&str] = &["br"];

/// The tags written without a closing tag: `<br>`, `<img …/>`.
const VOID_TAGS: &[&str] = &["br", "img"];

/// The attributes each tag may carry; a tag not listed carries none.
pub(crate) const ATTRIBUTES: &[(&str, &[&str])] =
    &[("td", &["colspan", "rowspan"]), ("a", &["href"]), ("img", &["src", "alt"])];

/// The comment between sections, in both views.
pub(crate) const SECTION_MARKER: &str = "<!-- hwpforge:section -->";

/// The comment styled writes for a chart.
pub(crate) const CHART_COMMENT: &str = "<!-- chart -->";

/// A memo comment is its body between these, on one line; the body
/// escapes every `-`, so it holds no `--`.
pub(crate) const MEMO_OPEN: &str = "<!-- memo: ";
/// See [`MEMO_OPEN`].
pub(crate) const MEMO_CLOSE: &str = " -->";

/// The comments the encoder writes; any other comment is foreign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comment {
    /// [`SECTION_MARKER`].
    Section,
    /// [`CHART_COMMENT`].
    Chart,
    /// [`MEMO_OPEN`] … [`MEMO_CLOSE`].
    Memo,
}

/// Which of the encoder's comments `comment` is: the whole comment, from
/// `<!--` to `-->`, exactly as written.
/// 이것을 실패시키는 것: 정확 비교를 접두 비교로 · memo 닫기 앞 공백 검사 삭제 ·
/// 빈 본문 허용 · 본문 안 `--` 허용(`comment_kinds_are_exactly_what_the_encoder_writes`).
pub(crate) fn comment_kind(comment: &str) -> Option<Comment> {
    match comment {
        SECTION_MARKER => Some(Comment::Section),
        CHART_COMMENT => Some(Comment::Chart),
        _ => comment
            .strip_prefix(MEMO_OPEN)
            .and_then(|rest| rest.strip_suffix(MEMO_CLOSE))
            .filter(|body| !body.is_empty() && !body.contains("--"))
            .map(|_| Comment::Memo),
    }
}

/// One piece of an HTML fragment in the encoder's schema.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    /// Text between tags as written: the caller decodes it ([`unescape`])
    /// after reading what only raw text tells, such as its line ends.
    Text(&'a str),
    /// An opening tag, or a void one ([`VOID_TAGS`]: `<br>`, `<img …/>`),
    /// with its attributes in order, their values decoded once.
    Open {
        /// The tag name.
        name: &'a str,
        /// `(name, value)` pairs.
        attributes: Vec<(&'a str, String)>,
    },
    /// A closing tag.
    Close(&'a str),
    /// One of the encoder's comments.
    Comment(Comment),
}

/// `html` as tokens when everything in it is written as the encoder writes
/// it: a tag of [`STYLED_TAGS`] or [`LOSSY_TAGS`] in lower case, its
/// attributes from [`ATTRIBUTES`] each once and double-quoted, a closing
/// tag bare, `<br>` and `<img …/>` with no closing tag, comments of
/// [`Comment`]. `None` (unsupported HTML) for anything else. Whether tags
/// nest or close in order is the caller's.
/// 이것을 실패시키는 것: 각 거부 조건 하나씩 삭제(`scanner_takes_only_the_schema`).
pub(crate) fn tokens(html: &str) -> Option<Vec<Token<'_>>> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let start = rest.find('<').unwrap_or(rest.len());
        if start > 0 {
            out.push(Token::Text(&rest[..start]));
        }
        rest = &rest[start..];
        if rest.is_empty() {
            break;
        }
        if rest.starts_with("<!--") {
            let end = rest[4..].find("-->")? + 7;
            out.push(Token::Comment(comment_kind(&rest[..end])?));
            rest = &rest[end..];
            continue;
        }
        let end = tag_end(rest)?;
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        out.push(tag(inner)?);
    }
    Some(out)
}

/// A tag's inside (between `<` and `>`) as a token, if in the schema.
fn tag(inner: &str) -> Option<Token<'_>> {
    let known = |name: &str| STYLED_TAGS.contains(&name) || LOSSY_TAGS.contains(&name);
    if let Some(name) = inner.strip_prefix('/') {
        return (known(name) && !VOID_TAGS.contains(&name)).then_some(Token::Close(name));
    }
    let name_end = inner.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(inner.len());
    let name = &inner[..name_end];
    if !known(name) {
        return None;
    }
    let mut rest = &inner[name_end..];
    // `/>` closes `img` and nothing else, as the encoder writes it.
    match rest.strip_suffix('/') {
        Some(open) if name == "img" => rest = open,
        None if name != "img" => {}
        _ => return None,
    }
    let names = ATTRIBUTES.iter().find(|(tag, _)| *tag == name).map_or(&[][..], |(_, a)| a);
    let mut attributes: Vec<(&str, String)> = Vec::new();
    for (attr, value) in self::attributes(rest)? {
        if !names.contains(&attr) || attributes.iter().any(|(seen, _)| *seen == attr) {
            return None;
        }
        attributes.push((attr, unescape(value)));
    }
    Some(Token::Open { name, attributes })
}

/// Whether `markdown` holds markup the encoder does not write.
/// 이것을 실패시키는 것: 갈래 하나씩 삭제(`guard_finds_what_the_encoder_does_not_write`).
pub(crate) fn foreign_markup(markdown: &str, allowed: &Allowed<'_>) -> bool {
    let options = if allowed.decoder_structure {
        super::md_options()
    } else {
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH
    };
    let parser = Parser::new_ext(markdown, options);
    // Definitions are read before any event: the encoder writes none.
    if parser.reference_definitions().iter().next().is_some() {
        return true;
    }
    let mut html = String::new();
    for event in parser {
        if !matches!(event, Event::Html(_)) && !html.is_empty() {
            if foreign_html(&html, allowed) {
                return true;
            }
            html.clear();
        }
        match event {
            // An HTML block comes line by line: its lines are checked together.
            Event::Html(h) => html.push_str(&h),
            Event::InlineHtml(h) if foreign_html(&h, allowed) => return true,
            // `md_options` reads no metadata block, so a definition list is
            // the one structure the decoder fails on.
            Event::Start(Tag::DefinitionList) if allowed.decoder_structure => return true,
            // Lossy writes notes inline, so a note definition in its output
            // came from document text (styled escapes every `[` of it).
            // 이것을 실패시키는 것: 이 갈래 삭제(`lossy_definition_shapes`).
            Event::Start(Tag::FootnoteDefinition(_)) if !allowed.decoder_structure => return true,
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. })
                if !allowed.destinations.contains(dest_url.as_ref()) =>
            {
                return true;
            }
            _ => {}
        }
    }
    !html.is_empty() && foreign_html(&html, allowed)
}

/// Whether an HTML fragment holds what the encoder does not write: outside
/// the schema ([`tokens`]), a tag this view does not write, a span that is
/// not digits, or a destination the encoder did not write.
fn foreign_html(html: &str, allowed: &Allowed<'_>) -> bool {
    let Some(tokens) = tokens(html) else { return true };
    tokens.iter().any(|token| match token {
        Token::Open { name, attributes } => {
            !allowed.tags.contains(name)
                || attributes.iter().any(|(attr, value)| match (*name, *attr) {
                    ("td", _) => !value.chars().all(|c| c.is_ascii_digit()),
                    ("a", _) | ("img", "src") => !allowed.destinations.contains(value),
                    _ => false,
                })
        }
        Token::Close(name) => !allowed.tags.contains(name),
        Token::Text(_) | Token::Comment(_) => false,
    })
}

/// A tag ends only outside quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if q == c => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// `name="value"` pairs; malformed or unquoted values fail the entire tag.
fn attributes(s: &str) -> Option<Vec<(&str, &str)>> {
    let mut out = Vec::new();
    let mut rest = s;
    // Only the whitespace consumed is read, once: the rest is not scanned
    // again for each attribute.
    // 이것을 실패시키는 것: 남은 글 전체를 매번 trim(`attribute_scan_is_linear`).
    loop {
        let next = rest.trim_start();
        if next.is_empty() {
            break;
        }
        // Each attribute follows whitespace.
        if next.len() == rest.len() {
            return None;
        }
        rest = next;
        let eq = rest.find('=')?;
        let name = rest[..eq].trim_end();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let after = rest[eq + 1..].trim_start().strip_prefix('"')?;
        let close = after.find('"')?;
        out.push((name, &after[..close]));
        rest = &after[close + 1..];
    }
    Some(out)
}

/// HTML text or an attribute value as the encoder escaped it, decoded
/// once: `&amp;` last, so `&amp;lt;` gives `&lt;`. Other references stay
/// as written.
pub(crate) fn unescape(s: &str) -> String {
    s.replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#91;", "[")
        .replace("&amp;", "&")
}

/// Document text written so nothing in it is markup: every ASCII
/// punctuation character escaped with `\`, line endings as spaces, leading
/// whitespace dropped (no block structure).
/// 이것을 실패시키는 것: escape 삭제(`guard_falls_back_to_plain_text`).
pub(crate) fn plain_text(raw: &str) -> String {
    escape_plain(raw.trim_start())
}

/// [`plain_text`] without dropping the leading whitespace (a piece after
/// the first).
pub(crate) fn escape_plain(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for c in raw.chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// The rendered-depth limit shared by the styled renderer and fallback.
pub(crate) fn enter_depth(depth: usize) -> Option<usize> {
    (depth < crate::lossless_grammar::MAX_NESTING_DEPTH).then_some(depth + 1)
}

/// Written instead of content below the rendered-depth limit.
pub(crate) const TOO_DEEP: &str = "(중첩 깊이 초과로 생략)";

/// A piece of a paragraph's plain-text fallback: document text (escaped
/// when written, [`fallback_markdown`]) or markup the view drew and the
/// fallback keeps as it is — a picture, an allowed link, a note reference.
/// The fallback gives up formatting, not content.
pub(crate) enum Fallback {
    Text(String),
    Kept(super::markup::Md),
}

/// What a view's fallback keeps as markup instead of text: what the view
/// drew for this very picture, link or note, looked up, never drawn anew.
pub(crate) trait Keeper {
    /// The picture's Markdown as the view drew it; `None` if it drew none
    /// (no path, or a picture whose asset the view never registered).
    fn picture(&mut self, image: &hwpforge_core::image::Image) -> Option<super::markup::Md>;
    /// The link's Markdown, its text escaped, when its URL is allowed.
    fn link(&mut self, text: &str, url: &str) -> Option<super::markup::Md>;
    /// The reference the view registered for this note occurrence; `None`
    /// for one it wrote inline or did not draw (a cut cell's), whose body
    /// is then written as text.
    fn note(&mut self, note: &hwpforge_core::Control) -> Option<super::markup::Md>;
}

/// Paragraph text as the fallback reads it, nothing kept: for tests.
#[cfg(test)]
pub(crate) fn deep_text(
    paragraph: &hwpforge_core::Paragraph,
    depth: usize,
    styled: bool,
) -> String {
    let mut out = Vec::new();
    walk(&paragraph.runs, depth, styled, &mut None, false, &mut out, &mut || {});
    out.into_iter()
        .map(|p| match p {
            Fallback::Text(t) => t,
            Fallback::Kept(md) => md.into_string(),
        })
        .collect()
}

/// The plain-text fallback of each of `paragraphs`, drawn at `depth`:
/// their text escaped (leading whitespace dropped), with what `keeper`
/// keeps as markup. The same container-depth limit as the renderers: a
/// cut-off subtree contributes only its notice; groups are spread without
/// consuming a level. For the styled view (`styled`) a cross reference,
/// field or unknown control gives the text styled shows for it
/// ([`super::styled_display`]); lossy keeps the value and hint it writes.
/// 이것을 실패시키는 것: 그림·링크·각주 참조를 글로 쓰기
/// (`fallback_keeps_pictures_links_and_notes`, `lossy_fallback_keeps_pictures_and_links`).
pub(crate) fn fallback_markdown(
    paragraphs: &[hwpforge_core::Paragraph],
    depth: usize,
    styled: bool,
    keeper: &mut dyn Keeper,
) -> Vec<super::markup::Md> {
    paragraphs
        .iter()
        .map(|p| {
            let mut out = Vec::new();
            walk(&p.runs, depth, styled, &mut Some(&mut *keeper), false, &mut out, &mut || {});
            if let Some(Fallback::Text(t)) = out.first_mut() {
                *t = t.trim_start().to_string();
            }
            super::markup::Md::plain_parts(&out)
        })
        .collect()
}

/// The addresses of the notes in `runs` outside memo bodies, in the order
/// [`fallback_markdown`] meets them (a memo's anchor in place, its body
/// skipped): a copy and what it was made from give the same order.
pub(crate) fn note_order(runs: &[hwpforge_core::Run]) -> Vec<usize> {
    struct Order(Vec<usize>);
    impl Keeper for Order {
        fn picture(&mut self, _: &hwpforge_core::image::Image) -> Option<super::markup::Md> {
            None
        }
        fn link(&mut self, _: &str, _: &str) -> Option<super::markup::Md> {
            None
        }
        fn note(&mut self, note: &hwpforge_core::Control) -> Option<super::markup::Md> {
            self.0.push(note as *const hwpforge_core::Control as usize);
            None
        }
    }
    let mut order = Order(Vec::new());
    walk(runs, 0, true, &mut Some(&mut order), false, &mut Vec::new(), &mut || {});
    order.0
}

fn push_text(out: &mut Vec<Fallback>, text: &str) {
    if let Some(Fallback::Text(t)) = out.last_mut() {
        t.push_str(text);
    } else if !text.is_empty() {
        out.push(Fallback::Text(text.to_string()));
    }
}

/// The walk behind [`fallback_markdown`] over one paragraph's `runs` (the
/// document's own: the walk makes no copies, so `keeper` can look up what
/// was drawn for each occurrence); `memo` is inside a memo's body, which
/// the view wrote as a comment.
fn walk(
    runs: &[hwpforge_core::Run],
    depth: usize,
    styled: bool,
    keeper: &mut Option<&mut dyn Keeper>,
    memo: bool,
    out: &mut Vec<Fallback>,
    visit: &mut impl FnMut(),
) {
    visit();
    use hwpforge_core::Control;
    // Nested paragraphs (their runs), a space between them and before them.
    let mut paras = |out: &mut Vec<Fallback>,
                     ps: &mut dyn Iterator<Item = &[hwpforge_core::Run]>,
                     memo: bool,
                     keeper: &mut Option<&mut dyn Keeper>| {
        let Some(inner) = enter_depth(depth) else {
            push_text(out, TOO_DEEP);
            return;
        };
        for p in ps {
            // Each paragraph walked on its own, as a separate string before.
            let mut sub = Vec::new();
            walk(p, inner, styled, keeper, memo, &mut sub, visit);
            if !out.is_empty() {
                push_text(out, " ");
            }
            for part in sub {
                match part {
                    Fallback::Text(t) => push_text(out, &t),
                    kept => out.push(kept),
                }
            }
        }
    };
    fn runs_of(ps: &[hwpforge_core::Paragraph]) -> impl Iterator<Item = &[hwpforge_core::Run]> {
        ps.iter().map(|p| p.runs.as_slice())
    }
    for (part, _) in super::spread_groups(runs) {
        match part {
            super::Part::Text(content) => {
                if let Some(t) = content.plain_text() {
                    push_text(out, &t);
                }
            }
            super::Part::Image(image) => {
                if !memo {
                    if let Some(md) = keeper.as_mut().and_then(|k| k.picture(image)) {
                        out.push(Fallback::Kept(md));
                    }
                }
                if let Some(c) = &image.caption {
                    paras(out, &mut runs_of(&c.paragraphs), memo, keeper);
                }
            }
            super::Part::Table(table) => {
                if enter_depth(depth).is_none() {
                    push_text(out, TOO_DEEP);
                } else {
                    for cell in table.rows.iter().flat_map(|r| &r.cells) {
                        paras(out, &mut runs_of(&cell.paragraphs), memo, keeper);
                    }
                }
                if let Some(c) = &table.caption {
                    paras(out, &mut runs_of(&c.paragraphs), memo, keeper);
                }
            }
            super::Part::Control(control) => match control {
                Control::CrossRef { .. } | Control::Field { .. } | Control::Unknown { .. }
                    if styled =>
                {
                    push_text(out, &super::styled_display(control).unwrap_or_default());
                }
                Control::Hyperlink { text, url } => {
                    match keeper.as_mut().filter(|_| !memo).and_then(|k| k.link(text, url)) {
                        Some(md) => out.push(Fallback::Kept(md)),
                        None => push_text(out, text),
                    }
                }
                // A note the view registered is a definition at the
                // document's end: its reference is kept, its body is not
                // written again. Any other (inline, or not drawn) is text.
                Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                    // A memo's body was a comment: its notes are inline.
                    match keeper.as_mut().filter(|_| !memo).and_then(|k| k.note(control)) {
                        Some(mark) => out.push(Fallback::Kept(mark)),
                        None => paras(out, &mut runs_of(paragraphs), memo, keeper),
                    }
                }
                Control::TextBox { paragraphs, .. }
                | Control::Ellipse { paragraphs, .. }
                | Control::Polygon { paragraphs, .. } => {
                    paras(out, &mut runs_of(paragraphs), memo, keeper)
                }
                Control::FieldBoundary {
                    boundary: hwpforge_core::control::FieldBoundary::Start(start),
                } => {
                    if let Some(content) = &start.annotation {
                        paras(out, &mut runs_of(content), true, keeper);
                    }
                }
                // A memo's anchor read as a paragraph, as styled draws it
                // (`super::memo_anchor`): a memo in it is read too. Styled
                // expands a top-level memo first; that changes only the
                // order and, at the depth limit, the level.
                // 이것을 실패시키는 것: 앵커 순회 삭제(`memo_anchor_follows_the_renderer`).
                Control::Memo { content, anchor_runs, .. } if styled => {
                    paras(out, &mut std::iter::once(anchor_runs.as_slice()), memo, keeper);
                    paras(out, &mut runs_of(content), true, keeper);
                }
                Control::Memo { content, anchor_runs, .. } => {
                    for run in anchor_runs {
                        if let Some(text) = run.content.plain_text() {
                            push_text(out, &text);
                        }
                    }
                    paras(out, &mut runs_of(content), true, keeper);
                }
                Control::Dutmal { main_text, sub_text, .. } => {
                    push_text(out, &format!("{main_text}({sub_text})"));
                }
                Control::Compose { compose_text, .. } => push_text(out, compose_text),
                Control::CrossRef { display_text, .. } => push_text(out, display_text),
                Control::Field { display_text, hint_text, .. } => {
                    push_text(
                        out,
                        if display_text.is_empty() {
                            hint_text.as_deref().unwrap_or("")
                        } else {
                            display_text
                        },
                    );
                }
                Control::Equation { script, .. } => push_text(out, script),
                Control::AutoNumber { num, format, .. } => {
                    push_text(out, &super::auto_number_text(*num, format));
                }
                _ => {}
            },
        }
    }
}

/// One block of a view's output and the top-level paragraphs it came from
/// (`None` for the encoder's own blocks: section markers).
pub(crate) struct Block {
    /// The block's Markdown.
    pub(crate) markdown: String,
    /// What to write instead (the paragraphs' plain text, or a note
    /// definition's label and text), with where it came from.
    pub(crate) source: Option<Source>,
    /// A styled Body block: a top-level paragraph, not a continuation,
    /// drawn as body text. Its document text must make no block structure;
    /// [`checked`] parses it alone and writes it as plain text if it does.
    /// Set by the branch that drew it, never guessed again.
    pub(crate) body: bool,
    /// Joined to the block before with one line ending, not a blank line:
    /// the first block an empty list item holds (an item that starts with
    /// a blank line ends there). Set from the list's state
    /// (`ListIndent::take_tight`), never from the output.
    pub(crate) tight: bool,
}

/// Where a block came from, for its plain-text fallback.
pub(crate) struct Source {
    /// The fallback's prefix (`[^1]: ` for a note definition, else empty).
    pub(crate) prefix: String,
    /// The fallback Markdown, per paragraph: its text escaped, with what the
    /// view keeps as markup (pictures, links, note references).
    pub(crate) texts: Vec<String>,
    /// The (section, top-level paragraph) of each.
    pub(crate) at: Vec<(usize, usize)>,
}

/// The view's blocks joined, after the check: a Body block whose text
/// makes block structure ([`body_structure`]) is written as plain text;
/// then each block holding foreign markup on its own is; if the whole
/// still fails (a block's markup working with another's), every block is.
/// One warning per paragraph written as plain text. With the joined
/// Markdown, the (section, paragraph) of each block written as plain text
/// here, so a view can say what else the paragraph lost.
/// 이것을 실패시키는 것: 검사 삭제 · 대체 삭제(`guard_falls_back_to_plain_text`) ·
/// Body 단계 삭제(`body_check_and_table_end_close_the_rest`).
pub(crate) fn checked(
    mut blocks: Vec<Block>,
    allowed: &Allowed<'_>,
    warnings: &mut Vec<super::MdWarning>,
) -> (String, Vec<(usize, usize)>) {
    // An empty block (a note with no text) writes nothing, not a gap.
    // 이것을 실패시키는 것: `tight` 무시(`empty_item_holds_its_first_block`).
    let join = |blocks: &[Block]| {
        let mut out = String::new();
        for b in blocks.iter().filter(|b| !b.markdown.is_empty()) {
            if !out.is_empty() {
                out.push_str(if b.tight { "\n" } else { "\n\n" });
            }
            out.push_str(&b.markdown);
        }
        out
    };
    // Paragraphs already written as plain text by the renderer (a guarded
    // paragraph's emphasis, `styled.rs`) keep their one warning.
    let mut warned: HashSet<(usize, usize)> = warnings
        .iter()
        .filter_map(|w| match w {
            super::MdWarning::MarkupWrittenAsText { section, paragraph } => {
                Some((*section, *paragraph))
            }
            _ => None,
        })
        .collect();
    let mut fell_back = Vec::new();
    let mut fall_back = |b: &mut Block, warnings: &mut Vec<super::MdWarning>| {
        if let Some(src) = b.source.take() {
            b.markdown = format!("{}{}", src.prefix, src.texts.join("\n\n"));
            for at in src.at {
                written_as_text(warnings, &mut warned, at);
                fell_back.push(at);
            }
        }
    };
    for b in &mut blocks {
        if b.body && b.source.is_some() && body_structure(&b.markdown) {
            fall_back(b, warnings);
        }
    }
    let all = join(&blocks);
    if !foreign_markup(&all, allowed) {
        return (all, fell_back);
    }
    for b in &mut blocks {
        if b.source.is_some() && foreign_markup(&b.markdown, allowed) {
            fall_back(b, warnings);
        }
    }
    let all = join(&blocks);
    if !foreign_markup(&all, allowed) {
        return (all, fell_back);
    }
    for b in &mut blocks {
        fall_back(b, warnings);
    }
    (join(&blocks), fell_back)
}

/// Records that the paragraph at `at` was written as plain text: one
/// warning per paragraph, whichever step wrote it (`warned` holds the
/// paragraphs already reported).
/// 이것을 실패시키는 것: 중복 검사 삭제(`one_warning_per_paragraph`).
pub(crate) fn written_as_text(
    warnings: &mut Vec<super::MdWarning>,
    warned: &mut HashSet<(usize, usize)>,
    at: (usize, usize),
) {
    if warned.insert(at) {
        warnings.push(super::MdWarning::MarkupWrittenAsText { section: at.0, paragraph: at.1 });
    }
}

/// Whether a Body block, parsed alone as the decoder parses, holds block
/// structure its text must not make: a list (its items and task markers
/// are inside one), heading, code block, block quote, note definition,
/// definition list (its titles and definitions are inside one) or
/// thematic break — or, as a safety net, text after a comment that opens
/// an HTML block (`styled::comment_ends_line` ends such lines first). The
/// decoder's options read no metadata block. Paragraphs, tables, other
/// HTML blocks and inline events are allowed.
/// 이것을 실패시키는 것: 금지 항목 하나씩 삭제 · `Rule` 삭제 · 주석 꼬리 검사 삭제
/// (`body_structure_finds_what_body_text_must_not_make`).
pub(crate) fn body_structure(markdown: &str) -> bool {
    let mut html: Option<String> = None;
    for event in Parser::new_ext(markdown, super::md_options()) {
        match event {
            Event::Start(
                Tag::List(_)
                | Tag::Heading { .. }
                | Tag::CodeBlock(_)
                | Tag::BlockQuote(_)
                | Tag::FootnoteDefinition(_)
                | Tag::DefinitionList,
            )
            | Event::Rule => return true,
            // A safety net: after `styled::comment_ends_line` no comment's
            // HTML block takes text in, so this does not fire.
            Event::Start(Tag::HtmlBlock) => html = Some(String::new()),
            Event::Html(h) => {
                if let Some(block) = &mut html {
                    block.push_str(&h);
                }
            }
            Event::End(pulldown_cmark::TagEnd::HtmlBlock) => {
                let block = html.take().unwrap_or_default();
                if let Some(comment) = block.trim_start().strip_prefix("<!--") {
                    // The block ends with the line holding `-->`.
                    let tail = comment.find("-->").map_or("", |end| &comment[end + 3..]);
                    if !tail.trim().is_empty() {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each structure Body text must not make is found; a paragraph, a
    /// table, an HTML block, a whole comment and inline markup are not.
    /// 이것을 실패시키는 것: 금지 항목 하나씩 삭제 · `Rule` 삭제.
    #[test]
    fn body_structure_finds_what_body_text_must_not_make() {
        for found in [
            "- x",
            "# h",
            "```\nx\n```",
            "a\n\n    b",
            "> q",
            "[^1]: x",
            "a\n: b",
            "***",
            "<!-- chart -->x",
            "a\n\n<!-- memo: m -->- x",
        ] {
            assert!(body_structure(found), "{found:?}");
        }
        for allowed in [
            "a",
            "a\nb",
            "| a |\n| --- |",
            "<table>\n<tbody>\n</tbody>\n</table>",
            "<!-- chart -->",
            "<!-- chart -->\nx",
            "a <!-- memo: m --> b",
            "**a** [t](https://e.invalid) `c`",
        ] {
            assert!(!body_structure(allowed), "{allowed:?}");
        }
    }

    /// The styled fallback text shows what styled shows for a cross
    /// reference without display text, an empty field and an unknown
    /// control; lossy's keeps the value and hint it writes.
    /// 이것을 실패시키는 것: styled 갈래 삭제 · lossy 에도 styled 표시값.
    #[test]
    fn fallback_text_follows_its_view() {
        use hwpforge_core::control::{Control, RefTarget};
        use hwpforge_core::{Paragraph, Run};
        use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
        let run = |c| Run::control(c, CharShapeIndex::new(0));
        let p = Paragraph::with_runs(
            vec![
                run(Control::cross_ref(
                    RefTarget::Name("ref".into()),
                    Default::default(),
                    Default::default(),
                )),
                run(Control::Field {
                    field_type: FieldType::ClickHere,
                    hint_text: None,
                    help_text: None,
                    name: None,
                    display_text: String::new(),
                }),
                run(Control::Unknown { tag: "x".into(), data: None }),
            ],
            ParaShapeIndex::new(0),
        );
        assert_eq!(deep_text(&p, 0, true), "[ref]____[x]");
        assert_eq!(deep_text(&p, 0, false), "");
    }

    /// Parsing errors cannot be accepted as an arbitrary image alt value.
    /// 이것을 실패시키는 것: 속성 실패를 값으로 반환 · 따옴표 안에서 태그 종료.
    #[test]
    fn guard_rejects_malformed_image_attributes() {
        let destinations = HashSet::new();
        let allowed =
            Allowed { tags: STYLED_TAGS, destinations: &destinations, decoder_structure: true };
        for bad in [
            "<img alt=x src=x onerror=alert(1)>",
            "<img alt=\">\" src=x onerror=alert(1)>",
            "<img alt=\">\" onerror=\"alert(1)\">",
            "<img alt>",
        ] {
            assert!(foreign_html(bad, &allowed), "{bad}");
            assert!(foreign_markup(bad, &allowed), "{bad}");
        }
        assert!(foreign_html("<img alt=\"unfinished>", &allowed));
        assert!(foreign_html("<img alt=\"x\"src=\"x\">", &allowed));
        assert!(!foreign_markup("<img alt=\">\"/>", &allowed));
    }

    /// A deep subtree is not visited, including when collection starts
    /// inside a note. Only the renderer's omission notice survives.
    /// 이것을 실패시키는 것: 수집 깊이 제한 삭제 · 재귀 깊이를 0으로 초기화.
    #[test]
    fn fallback_text_visits_only_rendered_depth() {
        use hwpforge_core::{Control, Paragraph, Run};
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        let paragraph = |run| Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
        for containers in [32, 33, 256] {
            let mut p = paragraph(Run::text("core", CharShapeIndex::new(0)));
            for _ in 0..containers {
                p = paragraph(Run::control(
                    Control::text_box(
                        vec![p],
                        HwpUnit::new(100).unwrap(),
                        HwpUnit::new(100).unwrap(),
                    ),
                    CharShapeIndex::new(0),
                ));
            }
            for depth in [0, 1, 31, 32] {
                let mut visits = 0;
                let mut parts = Vec::new();
                walk(&p.runs, depth, true, &mut None, false, &mut parts, &mut || visits += 1);
                let raw: String = parts
                    .into_iter()
                    .map(|p| match p {
                        Fallback::Text(t) => t,
                        Fallback::Kept(md) => md.into_string(),
                    })
                    .collect();
                assert_eq!(visits, (32 - depth).min(containers) + 1);
                let expected = if containers + depth <= 32 { "core" } else { TOO_DEEP };
                assert_eq!(raw, expected);
                let block = Block {
                    markdown: "<script>bad</script>".into(),
                    source: Some(Source {
                        prefix: String::new(),
                        texts: vec![plain_text(&raw)],
                        at: vec![(0, 0)],
                    }),
                    body: false,
                    tight: false,
                };
                let destinations = HashSet::new();
                let allowed = Allowed {
                    tags: STYLED_TAGS,
                    destinations: &destinations,
                    decoder_structure: true,
                };
                let mut warnings = Vec::new();
                assert_eq!(checked(vec![block], &allowed, &mut warnings).0, plain_text(expected));
                assert_eq!(warnings.len(), 1);
            }
        }
    }

    /// The check finds each kind of foreign markup and passes the encoder's
    /// own.
    ///
    /// 이것을 실패시키는 것: 정의 검사 삭제 · HTML 태그 검사 삭제 · 속성 검사 삭제 ·
    /// 링크·이미지 목적지 검사 삭제 · 주석 건너뛰기 삭제.
    #[test]
    fn guard_finds_what_the_encoder_does_not_write() {
        let dests: HashSet<String> = ["https://e.invalid".to_string()].into();
        let allowed = Allowed { tags: STYLED_TAGS, destinations: &dests, decoder_structure: true };
        for ok in [
            "a [t](https://e.invalid) <strong>b</strong>",
            "<table>\n<tbody>\n<tr>\n  <td colspan=\"2\"><a href=\"https://e.invalid\">t</a></td>\n</tr>\n</tbody>\n</table>",
            "x <!-- memo: <img src=x onerror=y> -->",
            "| a |\n| --- |",
        ] {
            assert!(!foreign_markup(ok, &allowed), "{ok}");
        }
        for bad in [
            "<img src=x onerror=y>",
            "a <b onclick=\"x\">b</b>",
            "<a href=\"javascript:x\">t</a>",
            "[t](javascript:x)",
            "![t](https://tracker.invalid/p.png)",
            "[a]: https://e.invalid\n\n[a]",
            "[a]: https://e.invalid",
            "<table>\n<tr><td>\n<script>x</script></td></tr></table>",
        ] {
            assert!(foreign_markup(bad, &allowed), "{bad}");
        }
        let lossy = Allowed { tags: LOSSY_TAGS, destinations: &dests, decoder_structure: false };
        assert!(foreign_markup("<strong>b</strong>", &lossy));
        assert!(!foreign_markup("a<br>b <!-- hwpforge:section -->", &lossy));
    }

    /// Only the encoder's own comments pass the guard, inline or as a block.
    /// 이것을 실패시키는 것: 분류되지 않은 주석도 통과시키기(`unwrap_or(Comment::Memo)`).
    #[test]
    fn guard_passes_only_the_encoders_comments() {
        let dests = HashSet::new();
        let allowed = Allowed { tags: STYLED_TAGS, destinations: &dests, decoder_structure: true };
        assert!(!foreign_markup("a <!-- chart --> b\n\n<!-- memo: m -->", &allowed));
        assert!(foreign_markup("x <!-- c -->", &allowed));
        assert!(foreign_markup("<!-- c -->", &allowed));
    }

    /// The three comments as the encoder writes them, nothing near them:
    /// one case per condition.
    /// 이것을 실패시키는 것: 정확 비교를 접두 비교로 · memo 닫기 앞 공백 검사 삭제 ·
    /// 빈 본문 허용 · 본문 안 `--` 허용.
    #[test]
    fn comment_kinds_are_exactly_what_the_encoder_writes() {
        for (comment, kind) in [
            ("<!-- hwpforge:section -->", Some(Comment::Section)),
            ("<!-- chart -->", Some(Comment::Chart)),
            ("<!-- memo: \\-\\-\\- m -->", Some(Comment::Memo)),
            ("<!-- chart x -->", None),
            ("<!-- memo: m-->", None),
            ("<!-- memo:  -->", None),
            ("<!-- memo: a -- b -->", None),
        ] {
            assert_eq!(comment_kind(comment), kind, "{comment}");
        }
    }

    /// The scanner takes what the encoder writes and nothing near it: one
    /// case per condition.
    /// 이것을 실패시키는 것: 중복 속성 검사 삭제 · 닫기 void 태그 검사 삭제 ·
    /// `/>` 형태 검사 삭제 · 속성 이름 검사 삭제 · 이름을 소문자로 바꿔 비교 ·
    /// 속성 앞 공백 검사 삭제 ·
    /// `&amp;` 를 먼저 복원 · 분류 안 된 주석 통과.
    #[test]
    fn scanner_takes_only_the_schema() {
        use Token::{Close, Open, Text};
        let open = |name, attributes: &[(&'static str, &str)]| Open {
            name,
            attributes: attributes.iter().map(|(a, v)| (*a, v.to_string())).collect(),
        };
        let html = "<td colspan=\"2\">a &amp;lt;<br><img src=\"x\" alt=\"&amp;quot;\"/>\
                    <a href=\"u?a&amp;b\">t</a></td><!-- chart -->";
        assert_eq!(
            tokens(html),
            Some(vec![
                open("td", &[("colspan", "2")]),
                Text("a &amp;lt;"),
                open("br", &[]),
                open("img", &[("src", "x"), ("alt", "&quot;")]),
                open("a", &[("href", "u?a&b")]),
                Text("t"),
                Close("a"),
                Close("td"),
                Token::Comment(Comment::Chart),
            ])
        );
        for html in [
            "<Td>",
            "<div>",
            "<td alt=\"x\">",
            "<td colspan=\"1\" colspan=\"2\">",
            "<td colspan=\"1\"rowspan=\"2\">",
            "</td colspan=\"2\">",
            "<br/>",
            "<img src=\"x\">",
            "</br>",
            "<!-- c -->",
            "<!-- chart",
            "<td",
            "<!DOCTYPE x>",
            "a < b",
        ] {
            assert_eq!(tokens(html), None, "{html}");
        }
    }

    /// A block with foreign markup is written as plain text and reported;
    /// a clean one is left as it is.
    ///
    /// 이것을 실패시키는 것: 대체 삭제 · 경고 삭제 · 깨끗한 블록까지 대체하기 ·
    /// 문서 전체 재검사 삭제.
    #[test]
    fn guard_falls_back_to_plain_text() {
        let dests = HashSet::new();
        let allowed = Allowed { tags: LOSSY_TAGS, destinations: &dests, decoder_structure: false };
        let block = |md: &str, raw: &str, at| Block {
            markdown: md.to_string(),
            source: Some(Source {
                prefix: String::new(),
                texts: vec![plain_text(raw)],
                at: vec![at],
            }),
            body: false,
            tight: false,
        };
        let mut warnings = Vec::new();
        let (out, fell_back) = checked(
            vec![
                block("ok", "ok", (0, 0)),
                block("<img src=x onerror=y>", "<img src=x onerror=y>", (0, 1)),
            ],
            &allowed,
            &mut warnings,
        );
        assert_eq!(out, "ok\n\n\\<img src\\=x onerror\\=y\\>");
        // Where the paragraphs written as plain text came from, for a view
        // to report what else they lost.
        // 이것을 실패시키는 것: 위치 기록 삭제.
        assert_eq!(fell_back, [(0, 1)]);
        assert_eq!(
            warnings,
            vec![super::super::MdWarning::MarkupWrittenAsText { section: 0, paragraph: 1 }]
        );
        // A definition in one block used by another: neither fails alone
        // as a link, the definition does; both are checked as a whole too.
        let mut warnings = Vec::new();
        let (out, _) = checked(
            vec![
                block("[a]", "[a]", (0, 0)),
                block("[a]: https://x.invalid", "[a]: https://x.invalid", (0, 1)),
            ],
            &allowed,
            &mut warnings,
        );
        assert_eq!(out, "[a]\n\n\\[a\\]\\: https\\:\\/\\/x\\.invalid");
        assert_eq!(warnings.len(), 1);
        let (out, _) = checked(vec![block("x", "x", (0, 0))], &allowed, &mut Vec::new());
        assert_eq!(out, "x");
    }
}
