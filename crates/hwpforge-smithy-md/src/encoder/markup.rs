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

use std::collections::HashSet;
use std::fmt::Write as _;

/// A Markdown destination as a parser reads it back from [`destination`]:
/// the URL itself, its line endings percent-encoded.
fn read_back(url: &str) -> String {
    url.replace('\n', "%0A").replace('\r', "%0D")
}

/// Markdown the encoder made, whether its last non-whitespace content is a
/// table the encoder made ([`Md::ending_block`]), and how many `\` end it
/// (kept so [`join`] does not count them again for every piece).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Md(String, bool, usize);

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
        Self::of(s.to_string())
    }

    /// A number the encoder writes (a note label, a list number).
    pub(crate) fn number(n: usize) -> Self {
        Self::of(n.to_string())
    }

    /// Document text for the styled view: every character Markdown could
    /// read as syntax is escaped (`*_`[]~\#` with `\`, `<`·`>` as entities,
    /// `&` before an entity with `\`).
    /// 이것을 실패시키는 것: 갈래 하나씩 삭제(`markup_safety`).
    pub(crate) fn text(raw: &str) -> Self {
        let raw = lines(raw);
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len() + raw.len() / 8);
        for i in 0..chars.len() {
            push_text_char(&mut out, &chars, i);
        }
        Self::of(out)
    }

    /// [`Md::text`] where the styled view guarantees that document text
    /// makes no block (`Ctx::guarded`). At the text's start and after each of
    /// its line endings, whitespace is skipped (any length, Unicode too) and
    /// a first character that could open a block is escaped:
    ///
    /// | first | when followed by | escaped |
    /// |---|---|---|
    /// | `-` | space, tab, `-`, `:`, `\|`, line end, end of text | `-` |
    /// | `+` | space, tab, line end, end of text | `+` |
    /// | `=` | `=`, space, tab, line end, end of text | `=` |
    /// | `:` | anything | `:` |
    /// | `\|` | `\|`·space·tab, then `-` or `:` | that `-` or `:` |
    /// | 1–9 digits | `.` or `)`, then space, tab, line end, end of text | the `.` or `)` |
    ///
    /// The end of the text counts as a space: the next piece may start with
    /// one. A `&` followed only by `[A-Za-z0-9#]` up to the text's end is
    /// escaped too, so the next piece cannot complete an entity with it.
    /// 이것을 실패시키는 것: 표의 행 하나씩 삭제 · `:` 를 조건부로 · 공백 집합을
    /// ASCII 로 · 끝을 공백으로 보지 않기 · `&` 규칙 삭제(`guarded_text_*`).
    pub(crate) fn guarded_text(raw: &str) -> Self {
        let raw = lines(raw);
        let chars: Vec<char> = raw.chars().collect();
        let mut escape = vec![false; chars.len()];
        let gap = |i: usize| matches!(chars.get(i), None | Some(' ' | '\t' | '\n'));
        // One cursor: every candidate in a run of whitespace (line endings
        // included) lands on the same first character, so the run is
        // skipped once and the next candidate is after the next line ending.
        // 이것을 실패시키는 것: 후보마다 공백을 다시 훑기(`guarded_text_is_linear`).
        let mut start = 0;
        while let Some(j) = (start..chars.len()).find(|&i| !chars[i].is_whitespace()) {
            start = chars[j..].iter().position(|&c| c == '\n').map_or(chars.len(), |p| j + p + 1);
            let next = chars.get(j + 1).copied();
            match chars[j] {
                '-' if gap(j + 1) || matches!(next, Some('-' | ':' | '|')) => escape[j] = true,
                '+' if gap(j + 1) => escape[j] = true,
                '=' if gap(j + 1) || next == Some('=') => escape[j] = true,
                ':' => escape[j] = true,
                '|' => {
                    let k = (j..chars.len()).find(|&i| !matches!(chars[i], '|' | ' ' | '\t'));
                    if let Some(k) = k.filter(|&k| matches!(chars[k], '-' | ':')) {
                        escape[k] = true;
                    }
                }
                c if c.is_ascii_digit() => {
                    let n = chars[j..].iter().take_while(|c| c.is_ascii_digit()).count();
                    let p = j + n;
                    if n <= 9 && matches!(chars.get(p), Some('.' | ')')) && gap(p + 1) {
                        escape[p] = true;
                    }
                }
                _ => {}
            }
        }
        if let Some(amp) = chars.iter().rposition(|&c| c == '&') {
            if chars[amp + 1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == '#') {
                escape[amp] = true;
            }
        }
        // The `&` marked here has no `;` after it, so `text` leaves it as it is.
        let mut out = String::with_capacity(raw.len() + raw.len() / 8);
        for (i, &escaped) in escape.iter().enumerate() {
            if escaped {
                out.push('\\');
            }
            push_text_char(&mut out, &chars, i);
        }
        Self::of(out)
    }

    /// Plain-text fallback pieces: their text as
    /// [`super::guard::escape_plain`] writes it (every ASCII punctuation mark
    /// escaped, line endings as spaces, no whitespace dropped) with the
    /// markup it keeps in place ([`super::guard::Fallback`]). A paragraph's
    /// leading whitespace is dropped by its caller
    /// ([`super::guard::fallback_markdown`]); a link label keeps all of it.
    /// 이것을 실패시키는 것: 앞 공백 지우기 · 남긴 표지 이스케이프
    /// (`fallback_keeps_pictures_links_and_notes`, `fallback_link_labels_keep_their_spaces`).
    pub(crate) fn plain_parts(parts: &[super::guard::Fallback]) -> Self {
        let mut out = Self::new();
        for part in parts {
            match part {
                super::guard::Fallback::Text(t) => {
                    out.push(Self::of(super::guard::escape_plain(t)));
                }
                super::guard::Fallback::Kept(md) => out.push(md.clone()),
            }
        }
        out
    }

    /// A lossy paragraph: its document text escaped only where CommonMark
    /// reads markup, judged in one pass over the whole paragraph — the
    /// markup the encoder made included, as context ([`escape_pieces`]).
    pub(crate) fn lossy_paragraph(pieces: Vec<Piece<Md>>) -> Self {
        Self::of(escape_pieces(pieces.into_iter().map(|p| p.map(|m| m.0)).collect(), false))
    }

    /// A lossy link label or alt text: [`Md::lossy`] with every `\`, `[`,
    /// `]` and `` ` `` escaped too, so it cannot close early.
    pub(crate) fn lossy_label(raw: &str) -> Self {
        Self::of(escape_label(&lines(raw)))
    }

    /// `[label](url)` — the destination written so it is exactly `url`
    /// ([`destination`]). A line ending in the label is a space (a blank
    /// line would break the link apart).
    /// 이것을 실패시키는 것: 줄 끝을 공백으로 바꾸기 삭제(`markup_safety`).
    pub(crate) fn link(label: Md, url: &str, destinations: &mut HashSet<String>) -> Self {
        destinations.insert(read_back(url));
        Self::of(format!("[{}]({})", label.0.replace('\n', " "), destination(url)))
    }

    /// `![alt](path)`, the alt's line endings as spaces, as in [`Md::link`].
    pub(crate) fn image(alt: Md, path: &str, destinations: &mut HashSet<String>) -> Self {
        destinations.insert(read_back(path));
        Self::of(format!("![{}]({})", alt.0.replace('\n', " "), destination(path)))
    }

    /// Document text as a code span: delimited by more backticks than its
    /// longest backtick run, line endings as spaces (a code span cannot
    /// hold a blank line), padded when it starts or ends with a backtick.
    pub(crate) fn code_span(raw: &str) -> Self {
        let text = lines(raw).replace('\n', " ");
        let fence = "`".repeat(longest_run(&text, '`') + 1);
        let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
        Self::of(format!("{fence}{pad}{text}{pad}{fence}"))
    }

    /// Document text as a fenced code block: the fence is longer than any
    /// backtick run in it (at least three), so no line can close it.
    pub(crate) fn code_block(raw: &str) -> Self {
        let text = lines(raw);
        let fence = "`".repeat((longest_run(&text, '`') + 1).max(3));
        Self::of(format!("{fence}\n{text}\n{fence}"))
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
        Some(Self::of(out))
    }

    /// `<tag>` or `</tag>` for one of the encoder's inline tags.
    pub(crate) fn tag(tag: &'static str, close: bool) -> Self {
        Self::of(if close { format!("</{tag}>") } else { format!("<{tag}>") })
    }

    /// HTML (a table block, a tag) placed in Markdown.
    pub(crate) fn from_html(html: Html) -> Self {
        Self::of(html.0)
    }

    /// Appends `other` ([`join`]). After a table that ends a block
    /// ([`Md::ending_block`]), content goes after a blank line, so the table
    /// (an HTML block or GFM rows) cannot take it in, with its leading
    /// whitespace dropped as at a paragraph's start (after a blank line it
    /// would indent a code block); whitespace alone keeps the table last.
    /// 이것을 실패시키는 것: 빈 줄 넣기 삭제 · 공백에서 표시 끄기 · 앞 공백 남기기
    /// (`body_check_and_table_end_close_the_rest`).
    pub(crate) fn push(&mut self, other: Md) {
        self.push_placed(other);
    }

    /// [`Md::push`], saying where `other` landed: the byte range of its
    /// markup in `self`, and where an escape went in earlier in `self` (a
    /// `!` before a link, [`join`]), shifting what was there.
    pub(crate) fn push_placed(&mut self, other: Md) -> (std::ops::Range<usize>, Option<usize>) {
        let piece = if !other.0.chars().all(char::is_whitespace) && self.1 {
            self.0.push_str("\n\n");
            self.2 = 0;
            other.0.trim_start()
        } else {
            &other.0
        };
        let (shifted, tail) = join(&mut self.0, piece, self.2);
        self.2 = tail;
        let range = self.0.len() - piece.len()..self.0.len();
        if !other.0.chars().all(char::is_whitespace) {
            self.1 = other.1;
        }
        (range, shifted)
    }

    /// The markup marked as ending with a table the encoder made: content
    /// pushed after it starts a new block ([`Md::push`]).
    pub(crate) fn ending_block(mut self) -> Self {
        self.1 = true;
        self
    }

    fn of(s: String) -> Self {
        let tail = trailing_backslashes(&s);
        Self(s, false, tail)
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
    /// Keeps [`Md::ending_block`]: the layouts trim whitespace or wrap a
    /// comment, never add content after the table.
    pub(crate) fn layout(self, f: impl FnOnce(String) -> String) -> Self {
        let s = f(self.0);
        let tail = trailing_backslashes(&s);
        Self(s, self.1, tail)
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
    /// in the same pass (every `\` doubled, `|`, line endings). Raw inline
    /// delimiters stay literal even when cells or paragraphs are flattened.
    pub(crate) fn lossy_paragraph(pieces: Vec<Piece<Cell>>) -> Self {
        Self(escape_pieces(pieces.into_iter().map(|p| p.map(|c| c.0)).collect(), true))
    }

    /// Appends cell markup ([`join`]).
    pub(crate) fn push(&mut self, other: Cell) {
        // Lossy cells count the tail each time (out of the styled fix).
        let run = trailing_backslashes(&self.0);
        join(&mut self.0, &other.0, run);
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
            let columns = width;
            lines.push(row_line(row, columns, "|"));
            if i == 0 {
                lines.push(rule_line(width, "|"));
            }
        }
        Md::of(lines.join("\n"))
    }

    /// The same table inside a cell: `\|` bars, rows joined by `<br>`.
    pub(crate) fn nested_table(rows: Vec<Vec<Cell>>, width: usize) -> Cell {
        let mut lines = Vec::new();
        for (i, row) in rows.into_iter().enumerate() {
            let columns = width;
            lines.push(row_line(row, columns, "\\|"));
            if i == 0 {
                lines.push(rule_line(width, "\\|"));
            }
        }
        Cell(lines.join("<br>"))
    }

    /// Each cell's raw inline delimiters were escaped at its source. `<br>`
    /// alone would not stop code, links or emphasis spanning adjacent cells.
    pub(crate) fn cell_lines(rows: Vec<Vec<Cell>>, notice: &str) -> Cell {
        let mut out = notice.to_owned();
        for cell in rows.into_iter().flatten() {
            out.push_str("<br>");
            out.push_str(cell.as_str());
        }
        Cell(out)
    }

    /// Content-preserving degradation without rectangular grid expansion.
    pub(crate) fn lines(rows: Vec<Vec<Cell>>, notice: &str) -> Md {
        let mut out = notice.to_owned();
        for row in rows {
            for cell in row {
                // Separate parsing contexts; the inline prefix also keeps a
                // cell starting with `#`, `-`, etc. from becoming a block.
                // 이것을 실패시키는 것: 셀 문단 경계/블록 접두사 제거.
                out.push_str("\n\n<br>");
                out.push_str(cell.as_str());
            }
        }
        Md::of(out)
    }

    /// The markup.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Appends `next` markup to `out`. A lone `\` ending `out` would escape the
/// next character, and a `!` before a link would make it an image, so
/// either is escaped first.
/// `run` is how many `\` end `out`; the count ending the result is
/// returned with where an escape went in before the end, if anywhere.
/// 이것을 실패시키는 것: 끝 역슬래시 처리 삭제(`lossy_text_escapes_only_markup`) ·
/// `!` 처리 삭제(`markup_safety`) · 꼬리를 조각마다 다시 세기(`backslash_tail_is_kept`).
fn join(out: &mut String, next: &str, run: usize) -> (Option<usize>, usize) {
    if next.is_empty() {
        return (None, run);
    }
    let mut shifted = None;
    let mut escaped = 0;
    if run % 2 == 1 && next.starts_with(|c: char| c.is_ascii_punctuation()) {
        out.push('\\');
        escaped = 1;
    } else if next.starts_with('[') && out.ends_with('!') {
        let bang = out.len() - 1;
        let before = out[..bang].chars().rev().take_while(|&c| c == '\\').count();
        if before % 2 == 0 {
            out.insert(bang, '\\');
            shifted = Some(bang);
        }
    }
    out.push_str(next);
    let tail = if next.bytes().all(|b| b == b'\\') {
        run + escaped + next.len()
    } else {
        trailing_backslashes(next)
    };
    (shifted, tail)
}

/// How many `\` end `s`.
fn trailing_backslashes(s: &str) -> usize {
    s.len() - s.trim_end_matches('\\').len()
}

fn row_line(row: Vec<Cell>, width: usize, bar: &str) -> String {
    if row.is_empty() && width <= 1 {
        return format!("{bar} {bar}");
    }
    let mut out = String::from(bar);
    for index in 0..width.max(1) {
        out.push(' ');
        if let Some(cell) = row.get(index) {
            out.push_str(cell.as_str());
        }
        out.push(' ');
        out.push_str(bar);
    }
    out
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
    /// line would end the HTML block and let the rest be read as Markdown),
    /// `[` as `&#91;` (a raw `[^label]` in a cell is the encoder's note
    /// reference, [`Html::note_mark`]).
    /// 이것을 실패시키는 것: 줄 끝 처리 삭제(`markup_safety` 의 병합 칸 빈 줄) ·
    /// `[` 이스케이프 삭제(`html_cell_notes_read_back`).
    pub(crate) fn text(raw: &str) -> Self {
        Self(escape_html(&lines(raw)).replace('\n', "<br>").replace('[', "&#91;"))
    }

    /// A note reference the encoder registered (`[^1]`, `[^e1]`), written
    /// as it is: the decoder reads a raw `[^label]` in a cell as one.
    pub(crate) fn note_mark(mark: Md) -> Self {
        Self(mark.into_string())
    }

    /// `<tag>inner</tag>` for one of the encoder's inline tags.
    pub(crate) fn wrap(tag: &'static str, inner: Html) -> Self {
        Self(format!("<{tag}>{}</{tag}>", inner.0))
    }

    /// `<a href="url">label</a>`.
    pub(crate) fn link(label: &str, url: &str, destinations: &mut HashSet<String>) -> Self {
        destinations.insert(url.to_string());
        Self(format!("<a href=\"{}\">{}</a>", escape_attr(url), Html::text(label).0))
    }

    /// `<img src="key" alt="alt"/>`.
    pub(crate) fn image(key: &str, alt: &str, destinations: &mut HashSet<String>) -> Self {
        destinations.insert(key.to_string());
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
        // Layout whitespace only (ASCII, as the decoder reads it): a
        // no-break space at the cell's edge is text.
        // 이것을 실패시키는 것: `trim()` 으로 되돌리기(`html_cell_text_keeps_its_spaces`).
        let inner = inner.0.trim_matches([' ', '\t', '\n', '\r']);
        Self(format!("  <td{attrs}>{inner}</td>"))
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
    /// Block markup. The piece writer supplies both boundaries outside cells;
    /// inside a cell the producer's cell-safe representation is kept as is.
    Block(M),
}

impl<M> Piece<M> {
    fn map<N>(self, f: impl FnOnce(M) -> N) -> Piece<N> {
        match self {
            Piece::Text(t) => Piece::Text(t),
            Piece::Markup(m) => Piece::Markup(f(m)),
            Piece::Block(m) => Piece::Block(f(m)),
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
///   line's start ([`FenceScan`]).
///
/// A backslash run just before an escaped character is doubled, so it stays
/// text and the escape still applies. With `cell`, the text is a GFM table
/// cell's: every `\` is escaped, `|` is `\|` and a line break `<br>` — in
/// one pass, so no escape is escaped again. Raw backticks, brackets and
/// emphasis delimiters are escaped too: separately serialized fragments can
/// share a cell's final inline context. The text has `\n` line endings.
/// 이것을 실패시키는 것: 규칙 하나씩 삭제 · 역슬래시 겹치기 삭제 · 칸에서 두 번
/// escape 하기 · 링크 너머 글자를 따로 판정하기
/// (`lossy_text_escapes_only_markup`, `markup_safety`).
fn escape_pieces(pieces: Vec<Piece<String>>, cell: bool) -> String {
    escape_pieces_scanned(pieces, cell).0
}

fn escape_pieces_scanned(pieces: Vec<Piece<String>>, cell: bool) -> (String, FenceScan) {
    let mut chars: Vec<char> = Vec::new();
    let mut markup: Vec<bool> = Vec::new();
    for piece in &pieces {
        let (s, is_markup) = match piece {
            Piece::Text(t) => (lines(t).into_owned(), false),
            Piece::Markup(m) => (m.clone(), true),
            // One join contract for tables, captions and blocks propagated
            // through containers, including already formatted paragraph text.
            // Empty captions must not split adjacent inline pieces.
            // 이것을 실패시키는 것: 블록 앞/뒤 경계 삭제, 빈 블록 경계 생성, Cell에 경계 적용.
            Piece::Block(m) if !cell && !m.trim().is_empty() => {
                (format!("\n\n{}\n\n", m.trim_matches('\n')), true)
            }
            Piece::Block(m) => (m.clone(), true),
        };
        for c in s.chars() {
            chars.push(c);
            markup.push(is_markup);
        }
    }
    let mut scan = FenceScan::new(&markup);
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        let fence = scan.at(&chars, i);
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
            // Cell paragraphs, captions and nested tables share one final
            // inline context. Close raw syntax here, while text and generated
            // markup are still distinct, never by re-escaping a finished Cell.
            // 이것을 실패시키는 것: 셀 원문의 백틱/링크/강조 구문 보호 삭제.
            '`' | '[' | ']' | '*' | '_' | '~' | '<' | '&' | '!' if cell => true,
            '\\' => cell,
            '|' => cell,
            '`' | '~' if !cell && fence => true,
            // A code span opened here could close past the markup after it
            // (an escaped backtick still closes a span), so before markup
            // every backtick is escaped. 이것을 실패시키는 것: 이 갈래 삭제
            // (`markup_safety` 의 링크 앞뒤 백틱).
            '`' => scan.last_markup.is_some_and(|last| i < last),
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
    (out, scan)
}

/// A paragraph's line start and current backtick/tilde run. Each run is
/// measured once, including markup characters, so every text character in
/// a fence is escaped without rescanning its prefix or suffix.
#[derive(Default)]
struct FenceScan {
    line_start: usize,
    run_end: usize,
    fence: bool,
    last_markup: Option<usize>,
    #[cfg(test)]
    comparisons: usize,
}

impl FenceScan {
    fn new(markup: &[bool]) -> Self {
        let mut scan = Self::default();
        scan.last_markup = markup.iter().rposition(|&m| {
            scan.compared();
            m
        });
        scan
    }

    fn compared(&mut self) {
        #[cfg(test)]
        {
            self.comparisons += 1;
        }
    }

    fn at(&mut self, chars: &[char], i: usize) -> bool {
        self.compared();
        let c = chars[i];
        if c == '\n' {
            self.line_start = i + 1;
        }
        if !matches!(c, '`' | '~') {
            return false;
        }
        if i >= self.run_end {
            self.run_end = i + 1;
            while self.run_end < chars.len() {
                self.compared();
                if chars[self.run_end] != c {
                    break;
                }
                self.run_end += 1;
            }
            self.fence = i - self.line_start <= 3
                && chars[self.line_start..i].iter().all(|&x| {
                    self.compared();
                    x == ' '
                })
                && self.run_end - i >= 3;
        }
        self.fence
    }
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
/// One character of [`Md::text`]: `*_`[]~\#` escaped with `\`, `<`·`>` as
/// entities, `&` before an entity escaped with `\`.
fn push_text_char(out: &mut String, chars: &[char], i: usize) {
    match chars[i] {
        ch @ ('*' | '_' | '`' | '[' | ']' | '~' | '\\' | '#') => {
            out.push('\\');
            out.push(ch);
        }
        '&' if entity_follows(&chars[i + 1..]) => out.push_str("\\&"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        ch => out.push(ch),
    }
}

fn entity_follows(rest: &[char]) -> bool {
    let body = match rest {
        ['#', 'x' | 'X', tail @ ..] => tail,
        ['#', tail @ ..] => tail,
        _ => rest,
    };
    let n = body.iter().take_while(|c| c.is_ascii_alphanumeric()).count();
    (1..=32).contains(&n) && body.get(n) == Some(&';')
}

/// A closed-grammar element opening. Document values enter only as escaped
/// attributes; element and attribute names are static encoder syntax.
pub(crate) fn lossless_element(
    tag: &'static str,
    attrs: &[(&'static str, &str)],
    empty: bool,
) -> crate::error::MdResult<Html> {
    let mut out = format!("<{tag}");
    for (name, value) in attrs {
        write!(out, " {name}=\"{}\"", crate::lossless_grammar::escape_attr(value)?).unwrap();
    }
    out.push_str(if empty { "/>" } else { ">" });
    Ok(Html(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kept count of `\` ending the markup is the count read again,
    /// after every kind of push: all `\`, mixed, an escape put in before
    /// punctuation, a `!` before a link, after a table's blank line, and
    /// after a layout.
    /// 이것을 실패시키는 것: 전부 `\` 일 때 누적하지 않기 · 빈 줄 뒤 0 으로 두지 않기 ·
    /// layout 뒤 다시 세지 않기 · 이스케이프 한 자를 빼기.
    #[test]
    fn backslash_tail_is_kept() {
        let check = |md: &Md| assert_eq!(md.2, trailing_backslashes(&md.0), "{:?}", md.0);
        let mut md = Md::text("a\\");
        check(&md);
        for piece in [
            Md::syntax("\\"),
            Md::syntax("\\"),
            Md::syntax("*"),
            Md::text("b\\"),
            Md::syntax("\\"),
            Md::syntax("!"),
            Md::syntax("[x]"),
            Md::syntax("\\"),
            Md::syntax(""),
            Md::syntax(" "),
        ] {
            md.push(piece);
            check(&md);
        }
        let mut table = Md::syntax("| a |").ending_block();
        table.push(Md::syntax("\\x\\"));
        check(&table);
        // A table ending in `\` (its last cell's text): the blank line
        // after it ends the run, so the next `\` is not escaped.
        let mut table = Md::syntax("| a\\").ending_block();
        table.push(Md::syntax("\\"));
        check(&table);
        assert_eq!(table.as_str(), "| a\\\n\n\\");
        let laid = md.layout(|s| format!("{s}\\"));
        check(&laid);
    }

    /// Long runs and text before later markup need only linear scanning.
    /// 이것을 실패시키는 것: 매 글자에서 run 또는 마크업 suffix 다시 검색하기.
    #[test]
    fn backtick_scan_work_is_linear() {
        for prefix in ["", "x", "   ", "    ", "x\n"] {
            for suffix in ["", "[t](https://e.invalid)"] {
                for cell in [false, true] {
                    let measure = |n| {
                        let raw = format!("{prefix}{}", "`".repeat(n));
                        let (out, scan) = escape_pieces_scanned(
                            vec![Piece::Text(raw), Piece::Markup(suffix.to_string())],
                            cell,
                        );
                        let fenced = matches!(prefix, "" | "   " | "x\n") && !cell;
                        let ticks = if cell || fenced || !suffix.is_empty() {
                            "\\`".repeat(n)
                        } else {
                            "`".repeat(n)
                        };
                        let prefix =
                            if cell { prefix.replace('\n', "<br>") } else { prefix.into() };
                        assert_eq!(out, format!("{prefix}{ticks}{suffix}"));
                        scan.comparisons
                    };
                    let n = measure(4096);
                    let twice = measure(8192);
                    assert!(twice >= n && twice <= 2 * n + 8, "{n} -> {twice}");
                    assert!(twice <= 4 * 8192 + 64, "{twice}");
                }
            }
        }
    }

    /// The cached run decision agrees with the original fence rule, even
    /// when markup and text divide a run or an indentation.
    #[test]
    fn fence_scan_preserves_decisions() {
        for prefix in ["", " ", "   ", "    ", "x", "x\n", "\t", "~", "`"] {
            for run in ["`", "``", "```", "~~~~", "```~~```", "~~~\n```"] {
                let chars: Vec<_> = format!("{prefix}{run}").chars().collect();
                let mut scan = FenceScan::new(&vec![false; chars.len()]);
                let mut fences = vec![false; chars.len()];
                for (i, &c) in chars.iter().enumerate() {
                    let actual = scan.at(&chars, i);
                    if !matches!(c, '`' | '~') {
                        continue;
                    }
                    let start = chars[..i].iter().rposition(|&x| x != c).map_or(0, |p| p + 1);
                    let line = chars[..start].iter().rposition(|&x| x == '\n').map_or(0, |p| p + 1);
                    let expected = start - line <= 3
                        && chars[line..start].iter().all(|&x| x == ' ')
                        && chars[start..].iter().take_while(|&&x| x == c).count() >= 3;
                    assert_eq!(actual, expected, "{prefix:?} {run:?} {i}");
                    fences[i] = expected;
                }
                for start in 0..=chars.len() {
                    for end in start..=chars.len() {
                        let pieces = vec![
                            Piece::Text(chars[..start].iter().collect()),
                            Piece::Markup(chars[start..end].iter().collect()),
                            Piece::Text(chars[end..].iter().collect()),
                        ];
                        let mut expected = String::new();
                        for (i, &c) in chars.iter().enumerate() {
                            if !(start..end).contains(&i)
                                && (fences[i] || (c == '`' && start < end && i < end))
                            {
                                expected.push('\\');
                            }
                            expected.push(c);
                        }
                        assert_eq!(escape_pieces(pieces, false), expected);
                    }
                }
            }
        }
    }
}
