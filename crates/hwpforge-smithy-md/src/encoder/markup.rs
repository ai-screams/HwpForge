//! The readable views' serialization layer: document text in, markup out.
//!
//! Document text (`&str`) becomes output only through a constructor here
//! that escapes it for its context, and what comes out is typed:
//!
//! - [`Md`] — Markdown the encoder made: escaped text or its own syntax;
//! - [`Cell`] — Markdown made safe inside a GFM table cell (`|`, line
//!   endings), only from an [`Md`] or from document text, once;
//! - [`Html`] — HTML the encoder made (merged and nested table cells).
//!
//! None of them is built from a `String`: syntax goes in as `&'static str`,
//! and the few places that lay out finished markup (list markers, indents)
//! take a closure over it ([`Md::layout`]). So text cannot be written
//! unescaped, and markup cannot be escaped twice — the cell step takes an
//! [`Md`] and gives a [`Cell`], which has no way back.
//!
//! Line endings: CR, LF and CRLF in document text are all line endings
//! (CommonMark reads each as one) and are written as `\n`, or as `<br>` in
//! a cell or in HTML.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Write as _;

thread_local! {
    /// The link and image destinations written since [`start_recording`]:
    /// what the final check ([`super::guard`]) accepts as the encoder's own.
    static DESTINATIONS: RefCell<HashSet<String>> = RefCell::default();
}

/// Starts a fresh record of the destinations an encode writes.
pub(crate) fn start_recording() {
    DESTINATIONS.with(|d| d.borrow_mut().clear());
}

/// The destinations written since [`start_recording`].
pub(crate) fn recorded() -> HashSet<String> {
    DESTINATIONS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

/// A Markdown destination as a parser reads it back from [`destination`]:
/// the URL itself, its line endings percent-encoded.
fn read_back(url: &str) -> String {
    url.replace('\n', "%0A").replace('\r', "%0D")
}

fn record(dest: &str) {
    DESTINATIONS.with(|d| d.borrow_mut().insert(dest.to_string()));
}

/// Markdown the encoder made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Md(String);

/// Markdown safe inside a GFM table cell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Cell(String);

/// HTML the encoder made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Html(String);

// Read access only: the markup can be read as `&str` and compared, never
// built from a string outside this module.
macro_rules! read_only {
    ($($t:ty),*) => {$(
        impl std::ops::Deref for $t {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl PartialEq<&str> for $t {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    )*};
}
read_only!(Md, Cell, Html);

/// Document text with every line ending (CRLF, CR, LF) as `\n`.
fn lines(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.contains('\r') {
        raw.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        raw.into()
    }
}

impl Md {
    /// Empty markup.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The encoder's own syntax.
    pub(crate) fn syntax(s: &'static str) -> Self {
        Self(s.to_string())
    }

    /// A number the encoder writes (a note label, a list number).
    pub(crate) fn number(n: usize) -> Self {
        Self(n.to_string())
    }

    /// Document text for the styled view: every character Markdown could
    /// read as syntax is escaped (`*_`[]~\#` with `\`, `<`·`>` as entities,
    /// `&` before an entity with `\`).
    /// 이것을 실패시키는 것: 갈래 하나씩 삭제(`markup_safety`).
    pub(crate) fn text(raw: &str) -> Self {
        let raw = lines(raw);
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len() + raw.len() / 8);
        for (i, &ch) in chars.iter().enumerate() {
            match ch {
                '*' | '_' | '`' | '[' | ']' | '~' | '\\' | '#' => {
                    out.push('\\');
                    out.push(ch);
                }
                '&' if entity_follows(&chars[i + 1..]) => out.push_str("\\&"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(ch),
            }
        }
        Self(out)
    }

    /// A lossy paragraph: its document text escaped only where CommonMark
    /// reads markup, judged in one pass over the whole paragraph — the
    /// markup the encoder made included, as context ([`escape_pieces`]).
    pub(crate) fn lossy_paragraph(pieces: Vec<Piece<Md>>) -> Self {
        Self(escape_pieces(pieces.into_iter().map(|p| p.map(|m| m.0)).collect(), false))
    }

    /// A lossy link label or alt text: [`Md::lossy`] with every `\`, `[`,
    /// `]` and `` ` `` escaped too, so it cannot close early.
    pub(crate) fn lossy_label(raw: &str) -> Self {
        Self(escape_label(&lines(raw)))
    }

    /// `[label](url)` — the destination written so it is exactly `url`
    /// ([`destination`]). A line ending in the label is a space (a blank
    /// line would break the link apart).
    /// 이것을 실패시키는 것: 줄 끝을 공백으로 바꾸기 삭제(`markup_safety`).
    pub(crate) fn link(label: Md, url: &str) -> Self {
        record(&read_back(url));
        Self(format!("[{}]({})", label.0.replace('\n', " "), destination(url)))
    }

    /// `![alt](path)`, the alt's line endings as spaces, as in [`Md::link`].
    pub(crate) fn image(alt: Md, path: &str) -> Self {
        record(&read_back(path));
        Self(format!("![{}]({})", alt.0.replace('\n', " "), destination(path)))
    }

    /// Document text as a code span: delimited by more backticks than its
    /// longest backtick run, line endings as spaces (a code span cannot
    /// hold a blank line), padded when it starts or ends with a backtick.
    pub(crate) fn code_span(raw: &str) -> Self {
        let text = lines(raw).replace('\n', " ");
        let fence = "`".repeat(longest_run(&text, '`') + 1);
        let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
        Self(format!("{fence}{pad}{text}{pad}{fence}"))
    }

    /// Document text as a fenced code block: the fence is longer than any
    /// backtick run in it (at least three), so no line can close it.
    pub(crate) fn code_block(raw: &str) -> Self {
        let text = lines(raw);
        let fence = "`".repeat((longest_run(&text, '`') + 1).max(3));
        Self(format!("{fence}\n{text}\n{fence}"))
    }

    /// A LaTeX equation as `$…$` in Markdown text, `None` when it cannot be
    /// written there without making a link or raw HTML: `<` and `>` become
    /// `\lt ` and `\gt ` (the same symbols to a math renderer, no tag to a
    /// Markdown one) and line endings spaces; a `$` or a trailing `\` would
    /// end the math early and `](` would start a link. The guarantee is no
    /// executable HTML and no unchecked link — other Markdown (`*x*`) in the
    /// LaTeX is not made literal, as a math renderer reads it first.
    /// 이것을 실패시키는 것: 꺾쇠 치환 삭제 · `None` 조건 하나씩 삭제
    /// (`markdown_equation_writes_no_html_or_link`).
    pub(crate) fn math(latex: &str) -> Option<Self> {
        let inner = latex.strip_prefix('$').and_then(|l| l.strip_suffix('$'))?;
        if inner.contains('$') || inner.contains("](") || inner.ends_with('\\') {
            return None;
        }
        let mut out = String::with_capacity(latex.len() + 8);
        out.push('$');
        for ch in inner.chars() {
            match ch {
                '<' => out.push_str("\\lt "),
                '>' => out.push_str("\\gt "),
                '\n' | '\r' => out.push(' '),
                _ => out.push(ch),
            }
        }
        out.push('$');
        Some(Self(out))
    }

    /// `<tag>` or `</tag>` for one of the encoder's inline tags.
    pub(crate) fn tag(tag: &'static str, close: bool) -> Self {
        Self(if close { format!("</{tag}>") } else { format!("<{tag}>") })
    }

    /// HTML (a table block, a tag) placed in Markdown.
    pub(crate) fn from_html(html: Html) -> Self {
        Self(html.0)
    }

    /// Appends `other` ([`join`]).
    pub(crate) fn push(&mut self, other: Md) {
        join(&mut self.0, &other.0);
    }

    /// Appends the encoder's own syntax.
    pub(crate) fn push_syntax(&mut self, s: &'static str) {
        self.push(Self::syntax(s));
    }

    /// `parts` joined by `sep`.
    pub(crate) fn join(parts: Vec<Md>, sep: &'static str) -> Self {
        let mut out = Self::new();
        for (i, p) in parts.into_iter().enumerate() {
            if i > 0 {
                out.push_syntax(sep);
            }
            out.push(p);
        }
        out
    }

    /// The markup laid out as a block by `f` — a list marker, an indent, a
    /// heading's `#`, joining lines. `f` must add only the encoder's syntax.
    pub(crate) fn layout(self, f: impl FnOnce(String) -> String) -> Self {
        Self(f(self.0))
    }

    /// As a GFM table cell: `|` escaped, line endings `<br>` (markup has only
    /// `\n` ones — [`lines`]). Backslashes stay — the markup's escapes are
    /// already right.
    /// 이것을 실패시키는 것: 역슬래시 두 배로 만들기 · 줄 끝 처리 삭제(`markup_safety`).
    pub(crate) fn into_cell(self) -> Cell {
        Cell(self.0.replace('|', "\\|").replace('\n', "<br>"))
    }

    /// The markup.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The markup as a string, for output.
    pub(crate) fn into_string(self) -> String {
        self.0
    }

    /// Whether nothing was written.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Cell {
    /// Empty cell markup.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// [`Md::lossy_paragraph`] in a lossy cell, with the cell's own escapes
    /// in the same pass (every `\` doubled, `|`, line endings).
    pub(crate) fn lossy_paragraph(pieces: Vec<Piece<Cell>>) -> Self {
        Self(escape_pieces(pieces.into_iter().map(|p| p.map(|c| c.0)).collect(), true))
    }

    /// Appends cell markup ([`join`]).
    pub(crate) fn push(&mut self, other: Cell) {
        join(&mut self.0, &other.0);
    }

    /// Appends Markdown, made cell-safe.
    pub(crate) fn push_md(&mut self, md: Md) {
        self.push(md.into_cell());
    }

    /// A GFM table from its cells: in Markdown (`|` bars, rows on lines).
    /// The first row is the header; a row may be shorter (`| |`).
    pub(crate) fn table(rows: Vec<Vec<Cell>>, width: usize) -> Md {
        let mut lines = Vec::new();
        for (i, row) in rows.into_iter().enumerate() {
            lines.push(row_line(row, "|"));
            if i == 0 {
                lines.push(rule_line(width, "|"));
            }
        }
        Md(lines.join("\n"))
    }

    /// The same table inside a cell: `\|` bars, rows joined by `<br>`.
    pub(crate) fn nested_table(rows: Vec<Vec<Cell>>, width: usize) -> Cell {
        let mut lines = Vec::new();
        for (i, row) in rows.into_iter().enumerate() {
            lines.push(row_line(row, "\\|"));
            if i == 0 {
                lines.push(rule_line(width, "\\|"));
            }
        }
        Cell(lines.join("<br>"))
    }

    /// The markup.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Appends `next` markup to `out`. A lone `\` ending `out` would escape the
/// next character, and a `!` before a link would make it an image, so
/// either is escaped first.
/// 이것을 실패시키는 것: 끝 역슬래시 처리 삭제(`lossy_text_escapes_only_markup`) ·
/// `!` 처리 삭제(`markup_safety`).
fn join(out: &mut String, next: &str) {
    if !next.is_empty() {
        let run = out.chars().rev().take_while(|&c| c == '\\').count();
        if run % 2 == 1 && next.starts_with(|c: char| c.is_ascii_punctuation()) {
            out.push('\\');
        } else if next.starts_with('[') && out.ends_with('!') {
            let bang = out.len() - 1;
            let before = out[..bang].chars().rev().take_while(|&c| c == '\\').count();
            if before % 2 == 0 {
                out.insert(bang, '\\');
            }
        }
    }
    out.push_str(next);
}

fn row_line(row: Vec<Cell>, bar: &str) -> String {
    if row.is_empty() {
        return format!("{bar} {bar}");
    }
    let cells: Vec<String> = row.into_iter().map(|c| c.0).collect();
    format!("{bar} {} {bar}", cells.join(&format!(" {bar} ")))
}

fn rule_line(width: usize, bar: &str) -> String {
    let rules = vec!["---"; width.max(1)];
    format!("{bar} {} {bar}", rules.join(&format!(" {bar} ")))
}

impl Html {
    /// Empty HTML.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The encoder's own markup.
    pub(crate) fn syntax(s: &'static str) -> Self {
        Self(s.to_string())
    }

    /// Document text: `& < > " '` as entities, line endings `<br>` (a blank
    /// line would end the HTML block and let the rest be read as Markdown).
    /// 이것을 실패시키는 것: 줄 끝 처리 삭제(`markup_safety` 의 병합 칸 빈 줄).
    pub(crate) fn text(raw: &str) -> Self {
        Self(escape_html(&lines(raw)).replace('\n', "<br>"))
    }

    /// `<tag>inner</tag>` for one of the encoder's inline tags.
    pub(crate) fn wrap(tag: &'static str, inner: Html) -> Self {
        Self(format!("<{tag}>{}</{tag}>", inner.0))
    }

    /// `<a href="url">label</a>`.
    pub(crate) fn link(label: &str, url: &str) -> Self {
        record(url);
        Self(format!("<a href=\"{}\">{}</a>", escape_attr(url), Html::text(label).0))
    }

    /// `<img src="key" alt="alt"/>`.
    pub(crate) fn image(key: &str, alt: &str) -> Self {
        record(key);
        Self(format!("<img src=\"{}\" alt=\"{}\"/>", escape_attr(key), escape_attr(alt)))
    }

    /// A table cell `  <td attrs>inner</td>` (`colspan`·`rowspan` > 1).
    pub(crate) fn td(colspan: u16, rowspan: u16, inner: Html) -> Self {
        let mut attrs = String::new();
        if colspan > 1 {
            let _ = write!(attrs, " colspan=\"{colspan}\"");
        }
        if rowspan > 1 {
            let _ = write!(attrs, " rowspan=\"{rowspan}\"");
        }
        Self(format!("  <td{attrs}>{}</td>", inner.0.trim()))
    }

    /// Appends HTML.
    pub(crate) fn push(&mut self, other: Html) {
        self.0.push_str(&other.0);
    }

    /// Appends the encoder's own markup.
    pub(crate) fn push_syntax(&mut self, s: &'static str) {
        self.0.push_str(s);
    }

    /// `parts` joined by `sep`.
    pub(crate) fn join(parts: Vec<Html>, sep: &'static str) -> Self {
        Self(parts.into_iter().map(|p| p.0).collect::<Vec<_>>().join(sep))
    }

    /// The HTML.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether nothing was written.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The HTML with surrounding whitespace removed.
    pub(crate) fn trimmed(self) -> Self {
        Self(self.0.trim().to_string())
    }
}

/// `& < > " '` as HTML entities.
pub(crate) fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// An attribute value: [`escape_html`] with line endings as character
/// references, so no line of the HTML block is blank (a blank line ends it).
/// 이것을 실패시키는 것: 줄 끝 처리 삭제(`markup_safety` 의 병합 칸 그림 경로).
fn escape_attr(input: &str) -> String {
    escape_html(input).replace('\n', "&#10;").replace('\r', "&#13;")
}

/// A link or image destination that CommonMark reads back as exactly
/// `url`: `\`, `(`, `)`, `<`, `>` and `&` before an entity are escaped with
/// `\`, so the destination cannot end early or decode to another character
/// — and if the link around it fails (a label broken by a blank line), the
/// destination shown as text makes no link either (its parentheses are
/// escaped);
/// a destination holding whitespace or a control character is written in
/// the `<…>` form (line endings percent-encoded — they cannot appear there).
/// 이것을 실패시키는 것: 갈래 하나씩 삭제(`markup_safety` 의 목적지 정확 비교).
pub(crate) fn destination(url: &str) -> String {
    if url.is_empty() {
        // `[t]()`: an empty destination.
        return String::new();
    }
    let chars: Vec<char> = url.chars().collect();
    let pointy = chars.iter().any(|c| c.is_whitespace() || c.is_control());
    let mut out = String::with_capacity(url.len() + 2);
    if pointy {
        out.push('<');
    }
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '\\' | '<' | '>' | '(' | ')' => {
                out.push('\\');
                out.push(c);
            }
            '&' if entity_follows(&chars[i + 1..]) => out.push_str("\\&"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            _ => out.push(c),
        }
    }
    if pointy {
        out.push('>');
    }
    out
}

fn longest_run(s: &str, c: char) -> usize {
    let mut best = 0;
    let mut run = 0;
    for ch in s.chars() {
        if ch == c {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

/// One piece of a lossy paragraph: document text, or markup the encoder
/// made for the place (`M` = [`Md`] or [`Cell`]).
pub(crate) enum Piece<M> {
    /// Document text.
    Text(String),
    /// The encoder's markup, written as it is.
    Markup(M),
}

impl<M> Piece<M> {
    fn map<N>(self, f: impl FnOnce(M) -> N) -> Piece<N> {
        match self {
            Piece::Text(t) => Piece::Text(t),
            Piece::Markup(m) => Piece::Markup(f(m)),
        }
    }
}

/// A lossy paragraph's pieces written out: the document text escaped only
/// where CommonMark would read it as markup rather than text, everything
/// else as it is (`<붙임>`, `<표 1>`, `[참고]`, `AT&T`, `*`, `#` stay). The
/// rules look at the whole paragraph — markup included — so text on both
/// sides of a link is judged together (`<img title='` · link · `' onerror…>`
/// is one tag). Escaped with `\`:
///
/// - `<` that starts raw HTML or an autolink ([`markup_after_lt`]): a
///   complete tag, a closing tag, a comment, `<?`, a declaration or CDATA,
///   an HTML block's opening tag name, `<scheme:` or `<local@` — so
///   `<붙임>`, `<표 1>`, `<AI가 바꾼 …>` and `a < b` stay;
/// - `&` that begins an entity or character reference (`&name;`, `&#…;`,
///   `&#x…;`), which would be read as another character;
/// - `]` before `(` — the end of an inline link or image label;
/// - outside a cell, `[` that may open a link reference definition: at a
///   line start after any container prefix (up to three spaces, `>`, list
///   markers), with a `]:` later before a blank line — the label may span
///   lines ([`definition_at`]);
/// - text right before markup: a `!` before a link (an image otherwise), a
///   lone `\` before punctuation (an escape otherwise);
/// - a backtick before markup (a code span opened there could close past
///   it — an escaped backtick still closes one), and a code fence at a
///   line's start ([`fence_at`]).
///
/// A backslash run just before an escaped character is doubled, so it stays
/// text and the escape still applies. With `cell`, the text is a GFM table
/// cell's: every `\` is escaped, `|` is `\|` and a line break `<br>` — in
/// one pass, so no escape is escaped again. The text has `\n` line endings.
/// 이것을 실패시키는 것: 규칙 하나씩 삭제 · 역슬래시 겹치기 삭제 · 칸에서 두 번
/// escape 하기 · 링크 너머 글자를 따로 판정하기
/// (`lossy_text_escapes_only_markup`, `markup_safety`).
fn escape_pieces(pieces: Vec<Piece<String>>, cell: bool) -> String {
    let mut chars: Vec<char> = Vec::new();
    let mut markup: Vec<bool> = Vec::new();
    for piece in &pieces {
        let (s, is_markup) = match piece {
            Piece::Text(t) => (lines(t).into_owned(), false),
            Piece::Markup(m) => (m.clone(), true),
        };
        for c in s.chars() {
            chars.push(c);
            markup.push(is_markup);
        }
    }
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if markup[i] {
            // Text right before markup: a `!` before a link and a lone `\`
            // before punctuation would change it.
            if i > 0 && !markup[i - 1] {
                // In a cell every `\` is escaped, so a run is never odd there.
                let run = out.chars().rev().take_while(|&b| b == '\\').count();
                if run % 2 == 1 && c.is_ascii_punctuation() {
                    out.push('\\');
                } else if c == '[' && run == 0 && out.ends_with('!') {
                    let bang = out.len() - 1;
                    let before = out[..bang].chars().rev().take_while(|&b| b == '\\').count();
                    if before % 2 == 0 {
                        out.insert(bang, '\\');
                    }
                }
            }
            out.push(c);
            continue;
        }
        if cell && c == '\n' {
            out.push_str("<br>");
            continue;
        }
        let next = chars.get(i + 1).copied();
        let escape = match c {
            '\\' => cell,
            '|' => cell,
            '`' | '~' if !cell && fence_at(&chars, i) => true,
            // A code span opened here could close past the markup after it
            // (an escaped backtick still closes a span), so before markup
            // every backtick is escaped. 이것을 실패시키는 것: 이 갈래 삭제
            // (`markup_safety` 의 링크 앞뒤 백틱).
            '`' => markup[i + 1..].iter().any(|&m| m),
            '<' => markup_after_lt(&chars[i + 1..]),
            '&' => entity_follows(&chars[i + 1..]),
            ']' => next == Some('('),
            '[' => !cell && definition_at(&chars, i),
            _ => false,
        };
        if escape {
            // Where every backslash is escaped on its own there is no run.
            if !cell {
                let run = out.chars().rev().take_while(|&b| b == '\\').count();
                out.extend(std::iter::repeat_n('\\', run));
            }
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether the `` ` `` or `~` at `i` is in a code fence: a run of three or
/// more at a line's start after up to three spaces (the fence would take in
/// everything after it, markup included). Every one of the run is escaped,
/// so no shorter run is left to open a code span.
/// 이것을 실패시키는 것: 이 검사 삭제(`markup_safety` 의 링크 앞 울타리).
fn fence_at(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    let start = chars[..i].iter().rposition(|&x| x != c).map_or(0, |p| p + 1);
    let line_start = chars[..start].iter().rposition(|&x| x == '\n').map_or(0, |p| p + 1);
    let indent = &chars[line_start..start];
    indent.len() <= 3
        && indent.iter().all(|&x| x == ' ')
        && chars[start..].iter().take_while(|&&x| x == c).count() >= 3
}

/// Whether the `[` at `i` may open a link reference definition: only
/// container prefixes stand between it and its line's start (up to three
/// spaces, `>` with an optional space, list markers `-`·`+`·`*`·`1.`·`1)`
/// with a space), and a `]:` follows before a blank line (a label may span
/// lines). 이것을 실패시키는 것: 접두사 건너뛰기 삭제 · 여러 줄 라벨을 첫 줄만 보기
/// (`markup_safety` 의 인용문·목록·여러 줄 라벨).
fn definition_at(chars: &[char], i: usize) -> bool {
    let line_start = chars[..i].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
    let prefix: String = chars[line_start..i].iter().collect();
    let mut rest = prefix.as_str();
    loop {
        let trimmed = rest.trim_start_matches(' ');
        if rest.len() - trimmed.len() > 3 && !trimmed.is_empty() {
            return false;
        }
        rest = trimmed;
        if rest.is_empty() {
            break;
        }
        if let Some(r) = rest.strip_prefix('>') {
            rest = r;
            continue;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let marker = if matches!(rest.as_bytes().first(), Some(b'-' | b'+' | b'*')) {
            1
        } else if (1..=9).contains(&digits)
            && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        {
            digits + 1
        } else {
            return false;
        };
        match rest[marker..].strip_prefix(' ') {
            Some(r) => rest = r,
            None if rest.len() == marker => rest = "",
            None => return false,
        }
    }
    // A `]:` before a blank line.
    let after: String = chars[i + 1..].iter().collect();
    let block_end = blank_line(&after).unwrap_or(after.len());
    after[..block_end].contains("]:")
}

/// Where the first blank line (a line of only spaces and tabs) starts.
fn blank_line(s: &str) -> Option<usize> {
    let mut pos = 0;
    for line in s.split_inclusive('\n') {
        if pos > 0 && line.trim().is_empty() {
            return Some(pos);
        }
        pos += line.len();
    }
    None
}

/// A lossy link label or alt text: every `\`, `[`, `]` and `` ` `` escaped
/// (so it cannot close early or open a code span over the link), plus the
/// paragraph rules for `<` and `&`.
/// 이것을 실패시키는 것: 라벨 escape 삭제(`markup_safety`).
fn escape_label(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let escape = match c {
            '\\' | '[' | ']' | '`' => true,
            '<' => markup_after_lt(&chars[i + 1..]),
            '&' => entity_follows(&chars[i + 1..]),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether the text after a `<` (the rest of the paragraph, markup
/// included) makes CommonMark read raw HTML or an autolink there.
/// Approximated on the safe side where the exact rule needs more:
///
/// - `!--`, `?`, `!` + ASCII letter, `![CDATA[` — comment, processing
///   instruction, declaration, CDATA (their closing is not looked for);
/// - `/` + ASCII letter — a closing tag;
/// - a tag name (`[A-Za-z][A-Za-z0-9-]*`) that is an HTML block's start
///   condition (`script`, `div`, `table`, … any case) followed by
///   whitespace, `>`, `/>` or the end;
/// - a complete open tag: the name, attributes (`name`, `name=value`,
///   quoted or not, each after whitespace), optional `/`, then `>`;
/// - `scheme:` (2–32 of `[A-Za-z0-9+.-]`, a letter first) — a URI autolink;
/// - `local@` (`[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]`) — an email autolink.
///
/// 이것을 실패시키는 것: 갈래 하나씩 삭제(`lossy_text_escapes_only_markup`).
fn markup_after_lt(rest: &[char]) -> bool {
    // HTML block start conditions 1 and 6 (CommonMark 0.31).
    #[rustfmt::skip]
    const BLOCK: &[&str] = &[
        "script", "pre", "style", "textarea", "address", "article", "aside", "base", "basefont",
        "blockquote", "body", "caption", "center", "col", "colgroup", "dd", "details", "dialog",
        "dir", "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form", "frame",
        "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html",
        "iframe", "legend", "li", "link", "main", "menu", "menuitem", "nav", "noframes", "ol",
        "optgroup", "option", "p", "param", "search", "section", "summary", "table", "tbody",
        "td", "tfoot", "th", "thead", "title", "tr", "track", "ul"
    ];
    let at = |i: usize| rest.get(i).copied();
    match rest {
        ['!', '-', '-', ..] | ['?', ..] | ['!', '[', 'C', 'D', 'A', 'T', 'A', '[', ..] => {
            return true;
        }
        ['!', c, ..] | ['/', c, ..] if c.is_ascii_alphabetic() => return true,
        _ => {}
    }
    let email = rest
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(**c))
        .count();
    if email > 0 && rest.get(email) == Some(&'@') {
        return true;
    }
    if !rest.first().is_some_and(char::is_ascii_alphabetic) {
        return false;
    }
    let scheme =
        rest.iter().take_while(|c| c.is_ascii_alphanumeric() || "+.-".contains(**c)).count();
    if (2..=32).contains(&scheme) && rest.get(scheme) == Some(&':') {
        return true;
    }
    let name = rest.iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '-').count();
    let tag: String = rest[..name].iter().collect::<String>().to_ascii_lowercase();
    let end_of_name = at(name);
    if BLOCK.contains(&tag.as_str())
        && (end_of_name.is_none_or(|c| c.is_whitespace() || c == '>' || c == '/'))
    {
        return true;
    }
    // A complete open tag: attributes, each after whitespace, then `/>` or `>`.
    let mut i = name;
    loop {
        let ws = rest[i.min(rest.len())..].iter().take_while(|c| c.is_whitespace()).count();
        i += ws;
        match at(i) {
            Some('>') => return true,
            Some('/') => return at(i + 1) == Some('>'),
            Some(c) if ws > 0 && (c.is_ascii_alphabetic() || c == '_' || c == ':') => {}
            _ => return false,
        }
        i += rest[i..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || "_.:-".contains(**c))
            .count();
        let ws = rest[i.min(rest.len())..].iter().take_while(|c| c.is_whitespace()).count();
        if rest.get(i + ws) == Some(&'=') {
            i += ws + 1;
            i += rest[i.min(rest.len())..].iter().take_while(|c| c.is_whitespace()).count();
            match rest.get(i) {
                Some(&q) if q == '"' || q == '\'' => {
                    let Some(close) = rest[i + 1..].iter().position(|&c| c == q) else {
                        return false;
                    };
                    i += close + 2;
                }
                _ => {
                    let n = rest[i.min(rest.len())..]
                        .iter()
                        .take_while(|c| !c.is_whitespace() && !"\"'=<>`".contains(**c))
                        .count();
                    if n == 0 {
                        return false;
                    }
                    i += n;
                }
            }
        }
    }
}

/// Whether `rest` (after a `&`) reads as an entity or character reference:
/// `name;`, `#digits;` or `#x hex;` (1–32 ASCII alphanumerics, CommonMark's
/// digit limits loosened — escaping a few more is harmless).
fn entity_follows(rest: &[char]) -> bool {
    let body = match rest {
        ['#', 'x' | 'X', tail @ ..] => tail,
        ['#', tail @ ..] => tail,
        _ => rest,
    };
    let n = body.iter().take_while(|c| c.is_ascii_alphanumeric()).count();
    (1..=32).contains(&n) && body.get(n) == Some(&';')
}
