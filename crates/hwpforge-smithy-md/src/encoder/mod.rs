//! Core -> Markdown encoders.

mod list_format;
mod lossless;
mod lossy;
mod styled;

use hwpforge_blueprint::template::Template;
use hwpforge_core::{Document, StyleLookup, Validated};

use crate::error::MdResult;

pub use styled::MdOutput;

/// Markdown encoder entrypoint.
pub struct MdEncoder;

/// Warning emitted by the markdown bridge in **either direction** — lossy
/// markdown encoders (HWPX→MD) and the MD→HWPX image embed loader
/// ([`crate::embed`]). Warning-first: what the output can no longer
/// represent is reported, not silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MdWarning {
    /// A table with merged cells was flattened into a plain GFM pipe grid
    /// (spans are not representable there). The style-aware encoder
    /// (`encode_styled`) renders such tables as HTML with colspan/rowspan
    /// instead and does not emit this warning.
    MergedCellsFlattened {
        /// Number of cells in the table with `col_span > 1` or `row_span > 1`.
        merged_cells: usize,
    },
    /// MD→HWPX (W6 §12b): an image reference could not be embedded — the
    /// image run is dropped so no dangling `binaryItemIDRef` reaches the
    /// package, and the exclusion is declared here.
    ImageEmbedSkipped {
        /// The markdown source string (truncated for display; `data:` URIs
        /// are cut at 64 chars).
        src: String,
        /// Why the reference was excluded.
        reason: crate::embed::ImageEmbedSkipReason,
    },
    /// HWPX→MD (readable views): a container nested deeper than the view
    /// draws was left out and a notice written in its place — styled past
    /// 32 nested containers (`(중첩 깊이 초과로 생략)`), lossy past 8 tables
    /// nested in cells (`(중첩 표 생략)`).
    ContainerTooDeep {
        /// What was left out: `"table"`, `"caption"`, `"shape text"`,
        /// `"note"` or `"memo"`.
        container: &'static str,
        /// The nesting limit that was reached.
        limit: usize,
        /// Index of the section holding it.
        section: usize,
        /// Index of the section's top-level paragraph holding it.
        paragraph: usize,
    },
    /// HWPX→MD (styled view, Markdown text): an equation whose LaTeX cannot
    /// be written in Markdown safely — it would end its `$...$` early or
    /// start a link — was written as its script text instead.
    EquationAsText {
        /// Index of the section holding it.
        section: usize,
        /// Index of the section's top-level paragraph holding it.
        paragraph: usize,
    },
}

impl core::fmt::Display for MdWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MergedCellsFlattened { merged_cells } => write!(
                f,
                "table with {merged_cells} merged cell(s) flattened to a plain GFM grid \
                 (use styled mode to keep spans as HTML)"
            ),
            Self::ImageEmbedSkipped { src, reason } => {
                write!(f, "image \"{src}\" not embedded ({reason}) — image dropped")
            }
            Self::ContainerTooDeep { container, limit, section, paragraph } => write!(
                f,
                "{container} nested deeper than {limit} levels left out \
                 (section {section}, paragraph {paragraph})"
            ),
            Self::EquationAsText { section, paragraph } => write!(
                f,
                "equation written as its script text, its LaTeX cannot be written \
                 in Markdown safely (section {section}, paragraph {paragraph})"
            ),
        }
    }
}

impl MdEncoder {
    /// Encodes a validated document into markdown with frontmatter.
    ///
    /// This method is mapping-aware and uses the provided template to map
    /// paragraph style IDs back into markdown semantics.
    pub fn encode(document: &Document<Validated>, template: &Template) -> MdResult<String> {
        lossy::encode_with_template(document, template)
    }

    /// Encodes a validated document into readable markdown without template mapping.
    pub fn encode_lossy(document: &Document<Validated>) -> MdResult<String> {
        lossy::encode_without_template(document)
    }

    /// Like [`MdEncoder::encode_lossy`], additionally reporting what the
    /// lossy rendering dropped (e.g. merged table cells flattened to GFM).
    pub fn encode_lossy_with_report(
        document: &Document<Validated>,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_without_template_report(document)
    }

    /// Like [`MdEncoder::encode`], additionally reporting what the lossy
    /// rendering dropped.
    pub fn encode_with_report(
        document: &Document<Validated>,
        template: &Template,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_with_template_report(document, template)
    }

    /// Encodes a validated document into lossless markdown (frontmatter + HTML-like body).
    pub fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
        lossless::encode_lossless(document)
    }

    /// Encodes a validated document into style-aware markdown.
    ///
    /// Queries the provided [`StyleLookup`] for character/paragraph/style
    /// properties to emit inline formatting (bold, italic, strikeout),
    /// heading markers, and extracted images.
    pub fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
        styled::encode_styled(document, styles)
    }
}

/// A caption number as text (§19.16 J, P19): the number Hancom cached,
/// between its prefix and suffix characters. Markdown writes the number in
/// digits whatever its number shape (a lossy view; only DIGIT occurs in the
/// corpus).
pub(crate) fn auto_number_text(
    num: u32,
    format: &hwpforge_core::control::AutoNumberFormat,
) -> String {
    let mut out = String::new();
    out.extend(format.prefix);
    out.push_str(&num.to_string());
    out.extend(format.suffix);
    out
}

/// Whether a link may be written as a link by the readable views. The same
/// schemes the Markdown decoder accepts — `http://`, `https://`, `mailto:`
/// (any case), and the empty URL as it does — and nothing that a browser
/// would strip before reading the
/// scheme: a URL holding whitespace or a control character anywhere is
/// refused, so `" javascript:…"` or `"java\tscript:…"` cannot pass as
/// another scheme. A refused link keeps only its visible text.
/// 이것을 실패시키는 것: 공백·제어 문자 검사 삭제 · 허용 목록을 금지 목록으로
/// (`link_urls_follow_the_allow_list`).
pub(crate) fn link_url_allowed(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

/// Document text for the lossy view, escaped only where CommonMark would
/// read it as markup rather than text; everything else is written as it is
/// (`<붙임>`, `<표 1>`, `[참고]`, `AT&T`, `*`, `#` stay). Escaped with `\\`:
///
/// - `<` that starts raw HTML or an autolink ([`markup_after_lt`]): a
///   complete tag, a closing tag, a comment, `<?`, a declaration or CDATA,
///   an HTML block's opening tag name, `<scheme:` or `<local@` — so
///   `<붙임>`, `<표 1>`, `<AI가 바꾼 …>` and `a < b` stay;
/// - `&` that begins an entity or character reference (`&name;`, `&#…;`,
///   `&#x…;`), which would be read as another character;
/// - `]` before `(` — the end of an inline link or image label;
/// - outside a cell, `[` at the start of a line (after at most three
///   spaces) whose line has a later `]:` — a link reference definition.
///
/// `after` is the character the output continues with (the markup written
/// next), so a tag name at the end is judged by what follows it.
///
/// A backslash run just before an escaped character is doubled, so it stays
/// text and the escape still applies. With `label`, every `\\`, `[` and `]`
/// is escaped (a link label, which must not close early). With `cell`, the
/// text is a GFM table cell's: every `\\` is escaped, `|` is `\\|` and a
/// line break `<br>` — in one pass, so no escape is escaped again.
/// 이것을 실패시키는 것: 규칙 하나씩 삭제 · 역슬래시 겹치기 삭제 · 칸에서 두 번
/// escape 하기 (`lossy_text_escapes_only_markup`).
pub(crate) fn escape_inline_markup(
    text: &str,
    label: bool,
    cell: bool,
    after: Option<char>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut line_start = 0;
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && chars[i - 1] == '\n' {
            line_start = i;
        }
        if cell && c == '\n' {
            out.push_str("<br>");
            continue;
        }
        let next = chars.get(i + 1).copied();
        let escape = match c {
            '\\' => label || cell,
            '|' => cell,
            '[' | ']' if label => true,
            '<' => markup_after_lt(&chars[i + 1..], after),
            '&' => entity_follows(&chars[i + 1..]),
            ']' => next == Some('('),
            '[' => {
                !cell
                    && i - line_start <= 3
                    && chars[line_start..i].iter().all(|&s| s == ' ')
                    && chars[i + 1..]
                        .iter()
                        .take_while(|&&n| n != '\n')
                        .collect::<String>()
                        .contains("]:")
            }
            _ => false,
        };
        if escape {
            // Where every backslash is escaped on its own there is no run.
            if !(label || cell) {
                let run = out.chars().rev().take_while(|&b| b == '\\').count();
                out.extend(std::iter::repeat_n('\\', run));
            }
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether the text after a `<` makes CommonMark read raw HTML or an
/// autolink there (`after` = what follows the text). Approximated on the
/// safe side where the exact rule needs more than the text holds:
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
fn markup_after_lt(rest: &[char], after: Option<char>) -> bool {
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
    let at = |i: usize| rest.get(i).copied().or(if i == rest.len() { after } else { None });
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

/// A link destination with `(`, `)`, `<` and `>` percent-encoded, so it
/// cannot end the link early or turn it into text followed by raw HTML
/// (`https://a)<img/src=x/onerror=y>` holds no whitespace and passes the
/// allow-list). The URL means the same to a browser.
/// 이것을 실패시키는 것: 인코딩 하나씩 삭제(`lossy_links_follow_the_allow_list`).
pub(crate) fn link_destination(url: &str) -> String {
    url.replace('(', "%28").replace(')', "%29").replace('<', "%3C").replace('>', "%3E")
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

/// One drawn piece of a paragraph, borrowed from the document: a run's
/// content, or a group child standing in its group's place.
#[derive(Clone, Copy)]
pub(crate) enum Part<'a> {
    /// A text run (`Text` or `InlineText`).
    Text(&'a hwpforge_core::RunContent),
    /// A picture: a picture run or a group's picture child.
    Image(&'a hwpforge_core::image::Image),
    /// A table run.
    Table(&'a hwpforge_core::Table),
    /// A control run or a group's shape child.
    Control(&'a hwpforge_core::Control),
}

/// The runs of a paragraph as the readable views draw them: a group run
/// (묶음) stands for its children in z-order — a picture child as a
/// picture, a shape child as that control, a nested group recursively —
/// each with the group run's char shape. Everything is borrowed (no
/// subtree is copied) and nested groups are walked with an explicit stack:
/// no recursion, and an auxiliary stack of one entry per open group level.
/// 이것을 실패시키는 것: 그림 갈래 삭제 · 중첩 재귀 삭제 · 자식 순서 뒤집기 ·
/// 복사로 되돌리기(`lossy_group_children_in_order`, `styled_group_children_in_order`,
/// `group_children_are_borrowed`).
pub(crate) fn spread_groups(
    runs: &[hwpforge_core::Run],
) -> impl Iterator<Item = (Part<'_>, hwpforge_foundation::CharShapeIndex)> {
    use hwpforge_core::control::GroupChild;
    use hwpforge_core::{Control, Run, RunContent};
    use hwpforge_foundation::CharShapeIndex;
    struct Spread<'a> {
        runs: std::slice::Iter<'a, Run>,
        groups: Vec<(std::slice::Iter<'a, GroupChild>, CharShapeIndex)>,
    }
    impl<'a> Iterator for Spread<'a> {
        type Item = (Part<'a>, CharShapeIndex);
        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if let Some((children, cs)) = self.groups.last_mut() {
                    let cs = *cs;
                    match children.next() {
                        None => {
                            self.groups.pop();
                        }
                        Some(GroupChild::Picture(image)) => return Some((Part::Image(image), cs)),
                        Some(GroupChild::Shape(Control::Group { children, .. })) => {
                            self.groups.push((children.iter(), cs));
                        }
                        Some(GroupChild::Shape(shape)) => return Some((Part::Control(shape), cs)),
                        // A future child kind is not drawn.
                        Some(_) => {}
                    }
                    continue;
                }
                let run = self.runs.next()?;
                let cs = run.char_shape_id;
                match &run.content {
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Group { children, .. } => self.groups.push((children.iter(), cs)),
                        other => return Some((Part::Control(other), cs)),
                    },
                    RunContent::Image(image) => return Some((Part::Image(image), cs)),
                    RunContent::Table(table) => return Some((Part::Table(table), cs)),
                    content => return Some((Part::Text(content), cs)),
                }
            }
        }
    }
    Spread { runs: runs.iter(), groups: Vec::new() }
}

/// Whether some container sits deeper than
/// [`MAX_NESTING_DEPTH`](crate::lossless_grammar::MAX_NESTING_DEPTH)
/// containers, counted as the lossless decoder counts its elements: a table, a
/// picture (and its caption one more), a group, a shape, a note or a memo
/// each adds one. An explicit stack — no recursion — so it is safe on any
/// tree.
pub(crate) fn nests_too_deep(document: &Document<Validated>) -> bool {
    use crate::lossless_grammar as g;
    use hwpforge_core::control::GroupChild;
    use hwpforge_core::image::Image;
    use hwpforge_core::{Control, Paragraph, RunContent};
    enum Item<'a> {
        Paras(&'a [Paragraph]),
        Runs(&'a [hwpforge_core::Run]),
        Image(&'a Image),
        Control(&'a Control),
    }
    let mut stack: Vec<(Item<'_>, usize)> = Vec::new();
    for section in document.sections() {
        stack.push((Item::Paras(&section.paragraphs), 0));
        for hf in section.headers.iter().chain(section.footers.iter()) {
            stack.push((Item::Paras(&hf.paragraphs), 0));
        }
        for mp in section.master_pages.iter().flatten() {
            stack.push((Item::Paras(&mp.paragraphs), 0));
        }
    }
    while let Some((item, depth)) = stack.pop() {
        if depth > g::MAX_NESTING_DEPTH {
            return true;
        }
        match item {
            Item::Paras(ps) => {
                for p in ps {
                    stack.push((Item::Runs(&p.runs), depth));
                }
            }
            Item::Runs(runs) => {
                for run in runs {
                    match &run.content {
                        RunContent::Image(image) => stack.push((Item::Image(image), depth + 1)),
                        RunContent::Table(t) => {
                            // A table counts itself, cells or not (the
                            // decoder counts the `<table>` element).
                            // 이것을 실패시키는 것: 이 검사를 지우기
                            // (`encoder_limits_nesting_depth` 의 빈 표).
                            if depth + 1 > g::MAX_NESTING_DEPTH {
                                return true;
                            }
                            for cell in t.rows.iter().flat_map(|r| &r.cells) {
                                stack.push((Item::Paras(&cell.paragraphs), depth + 1));
                            }
                            if let Some(c) = &t.caption {
                                stack.push((Item::Paras(&c.paragraphs), depth + 1));
                            }
                        }
                        RunContent::Control(c) => stack.push((Item::Control(c), depth)),
                        _ => {}
                    }
                }
            }
            Item::Image(image) => {
                if let Some(c) = &image.caption {
                    stack.push((Item::Paras(&c.paragraphs), depth + 1));
                }
            }
            Item::Control(c) => match c {
                Control::Group { children, .. } => {
                    for child in children {
                        match child {
                            GroupChild::Picture(image) => {
                                stack.push((Item::Image(image), depth + 2))
                            }
                            GroupChild::Shape(shape) => {
                                stack.push((Item::Control(shape), depth + 1))
                            }
                            _ => {}
                        }
                    }
                    if depth + 1 > g::MAX_NESTING_DEPTH {
                        return true;
                    }
                }
                Control::TextBox { paragraphs, caption, .. }
                | Control::Ellipse { paragraphs, caption, .. }
                | Control::Polygon { paragraphs, caption, .. } => {
                    stack.push((Item::Paras(paragraphs), depth + 1));
                    if let Some(cap) = caption {
                        stack.push((Item::Paras(&cap.paragraphs), depth + 2));
                    }
                }
                Control::Line { caption, .. }
                | Control::Rect { caption, .. }
                | Control::Arc { caption, .. }
                | Control::Curve { caption, .. }
                | Control::ConnectLine { caption, .. } => {
                    if depth + 1 > g::MAX_NESTING_DEPTH {
                        return true;
                    }
                    if let Some(cap) = caption {
                        stack.push((Item::Paras(&cap.paragraphs), depth + 2));
                    }
                }
                Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                    stack.push((Item::Paras(paragraphs), depth + 1))
                }
                Control::Memo { content, anchor_runs, .. } => {
                    stack.push((Item::Paras(content), depth + 1));
                    stack.push((Item::Runs(anchor_runs), depth + 1));
                }
                _ => {}
            },
        }
    }
    false
}

#[cfg(test)]
mod spread_tests {
    use super::*;
    use hwpforge_core::control::{Control, GroupChild};
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::{ObjectFrame, Run};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};

    /// Group children come out in z-order, each borrowed from the tree
    /// itself (the same address — nothing is copied), nested groups walked
    /// without recursion; a run that is not a group comes out as it is.
    ///
    /// 이것을 실패시키는 것: 자식을 복사해 내보내기 · 자식 순서 뒤집기 · 중첩 묶음 건너뛰기.
    #[test]
    fn group_children_are_borrowed() {
        let u = |v| HwpUnit::new(v).unwrap();
        let group = |children| Control::Group {
            children,
            width: u(10),
            height: u(10),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        let mut inner =
            group(vec![GroupChild::Picture(Image::new("b", u(1), u(1), ImageFormat::Png))]);
        for _ in 0..200 {
            inner = group(vec![GroupChild::Shape(inner)]);
        }
        let runs = vec![
            Run::text("t", CharShapeIndex::new(1)),
            Run::control(
                group(vec![
                    GroupChild::Picture(Image::new("a", u(1), u(1), ImageFormat::Png)),
                    GroupChild::Shape(inner),
                    GroupChild::Shape(Control::equation("x")),
                ]),
                CharShapeIndex::new(7),
            ),
        ];
        let parts: Vec<_> = spread_groups(&runs).collect();
        assert_eq!(parts.len(), 4);
        assert!(
            matches!(parts[0], (Part::Text(c), cs) if std::ptr::eq(c, &runs[0].content) && cs == CharShapeIndex::new(1))
        );
        let hwpforge_core::RunContent::Control(top) = &runs[1].content else { panic!() };
        let Control::Group { children, .. } = top.as_ref() else { panic!() };
        let GroupChild::Picture(a) = &children[0] else { panic!() };
        assert!(
            matches!(parts[1], (Part::Image(i), cs) if std::ptr::eq(i, a) && cs == CharShapeIndex::new(7))
        );
        let mut node = &children[1];
        let deepest = loop {
            match node {
                GroupChild::Shape(Control::Group { children, .. }) => node = &children[0],
                GroupChild::Picture(image) => break image,
                _ => panic!(),
            }
        };
        assert!(matches!(parts[2], (Part::Image(i), _) if std::ptr::eq(i, deepest)));
        let GroupChild::Shape(eq) = &children[2] else { panic!() };
        assert!(matches!(parts[3], (Part::Control(c), _) if std::ptr::eq(c, eq)));
    }
}
