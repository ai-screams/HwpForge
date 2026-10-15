//! The readable views' last check: the finished Markdown is parsed again
//! (pulldown-cmark — tables, footnotes, strikethrough) and must hold only
//! what the encoder itself writes — raw HTML from its own tag list,
//! links and images to the destinations it wrote, no link reference
//! definition. A block that holds anything else is written again as
//! escaped plain text ([`plain_text`]) and reported. The escaping rules
//! (`markup.rs`) are meant to make this never happen; this catches what a
//! rule misses.

use std::collections::HashSet;

use pulldown_cmark::{Event, Options, Parser, Tag};

/// What a view may write besides text.
pub(crate) struct Allowed<'a> {
    /// HTML tag names (lower case); comments are always allowed.
    pub(crate) tags: &'static [&'static str],
    /// Link and image destinations the encoder wrote (`a href`, `img src`).
    pub(crate) destinations: &'a HashSet<String>,
}

/// The tags the styled view writes.
pub(crate) const STYLED_TAGS: &[&str] =
    &["table", "tbody", "tr", "td", "br", "strong", "em", "del", "u", "sup", "sub", "a", "img"];

/// The tags the lossy view writes.
pub(crate) const LOSSY_TAGS: &[&str] = &["br"];

/// Whether `markdown` holds markup the encoder does not write.
/// 이것을 실패시키는 것: 갈래 하나씩 삭제(`guard_finds_what_the_encoder_does_not_write`).
pub(crate) fn foreign_markup(markdown: &str, allowed: &Allowed<'_>) -> bool {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
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

/// Whether an HTML fragment holds a tag or attribute the encoder does not
/// write. Comments are skipped whole.
fn foreign_html(html: &str, allowed: &Allowed<'_>) -> bool {
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if let Some(body) = rest.strip_prefix("<!--") {
            match body.find("-->") {
                Some(end) => rest = &body[end + 3..],
                None => return true,
            }
            continue;
        }
        let Some(end) = tag_end(rest) else { return true };
        let tag = rest[1..end].trim_start_matches('/').trim_end_matches('/');
        rest = &rest[end + 1..];
        let name: String = tag.chars().take_while(char::is_ascii_alphanumeric).collect();
        if !allowed.tags.contains(&name.to_ascii_lowercase().as_str()) {
            return true;
        }
        let Some(attrs) = attributes(&tag[name.len()..]) else { return true };
        for (attr, value) in attrs {
            let value = unescape_attr(&value);
            let ok = match (name.as_str(), attr.as_str()) {
                ("td", "colspan" | "rowspan") => value.chars().all(|c| c.is_ascii_digit()),
                ("a", "href") | ("img", "src") => allowed.destinations.contains(&value),
                ("img", "alt") => true,
                _ => false,
            };
            if !ok {
                return true;
            }
        }
    }
    false
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
fn attributes(s: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.trim().is_empty() {
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        rest = rest.trim_start();
        let eq = rest.find('=')?;
        let name = rest[..eq].trim_end();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let after = rest[eq + 1..].trim_start().strip_prefix('"')?;
        let close = after.find('"')?;
        out.push((name.to_string(), after[..close].to_string()));
        rest = &after[close + 1..];
    }
    Some(out)
}

/// An attribute value as the encoder escaped it, decoded.
fn unescape_attr(s: &str) -> String {
    s.replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Document text written so nothing in it is markup: every ASCII
/// punctuation character escaped with `\`, line endings as spaces, leading
/// whitespace dropped (no block structure).
/// 이것을 실패시키는 것: escape 삭제(`guard_falls_back_to_plain_text`).
pub(crate) fn plain_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for c in raw.trim_start().chars() {
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

/// Paragraph text for [`plain_text`], with the same container-depth limit
/// as styled rendering. A cut-off subtree contributes only its notice;
/// groups are spread without consuming a level, as in the renderer.
pub(crate) fn deep_text(paragraph: &hwpforge_core::Paragraph, depth: usize) -> String {
    deep_text_at(paragraph, depth, &mut || {})
}

fn deep_text_at(
    paragraph: &hwpforge_core::Paragraph,
    depth: usize,
    visit: &mut impl FnMut(),
) -> String {
    visit();
    use hwpforge_core::Control;
    let mut out = String::new();
    let mut paras = |out: &mut String, ps: &[hwpforge_core::Paragraph]| {
        let Some(inner) = enter_depth(depth) else {
            out.push_str(TOO_DEEP);
            return;
        };
        for p in ps {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&deep_text_at(p, inner, visit));
        }
    };
    for (part, _) in super::spread_groups(&paragraph.runs) {
        match part {
            super::Part::Text(content) => {
                if let Some(t) = content.plain_text() {
                    out.push_str(&t);
                }
            }
            super::Part::Image(image) => {
                if let Some(c) = &image.caption {
                    paras(&mut out, &c.paragraphs);
                }
            }
            super::Part::Table(table) => {
                if enter_depth(depth).is_none() {
                    out.push_str(TOO_DEEP);
                } else {
                    for cell in table.rows.iter().flat_map(|r| &r.cells) {
                        paras(&mut out, &cell.paragraphs);
                    }
                }
                if let Some(c) = &table.caption {
                    paras(&mut out, &c.paragraphs);
                }
            }
            super::Part::Control(control) => match control {
                Control::Hyperlink { text, .. } => out.push_str(text),
                Control::Footnote { paragraphs, .. }
                | Control::Endnote { paragraphs, .. }
                | Control::TextBox { paragraphs, .. }
                | Control::Ellipse { paragraphs, .. }
                | Control::Polygon { paragraphs, .. } => paras(&mut out, paragraphs),
                Control::Memo { content, .. } => paras(&mut out, content),
                Control::Dutmal { main_text, sub_text, .. } => {
                    out.push_str(&format!("{main_text}({sub_text})"));
                }
                Control::Compose { compose_text, .. } => out.push_str(compose_text),
                Control::CrossRef { display_text, .. } => out.push_str(display_text),
                Control::Field { display_text, hint_text, .. } => {
                    out.push_str(if display_text.is_empty() {
                        hint_text.as_deref().unwrap_or("")
                    } else {
                        display_text
                    });
                }
                Control::Equation { script, .. } => out.push_str(script),
                Control::AutoNumber { num, format, .. } => {
                    out.push_str(&super::auto_number_text(*num, format));
                }
                _ => {}
            },
        }
    }
    out
}

/// One block of a view's output and the top-level paragraphs it came from
/// (`None` for the encoder's own blocks: section markers).
pub(crate) struct Block {
    /// The block's Markdown.
    pub(crate) markdown: String,
    /// What to write instead (the paragraphs' plain text, or a note
    /// definition's label and text), with where it came from.
    pub(crate) source: Option<Source>,
}

/// Where a block came from, for its plain-text fallback.
pub(crate) struct Source {
    /// The fallback's prefix (`[^1]: ` for a note definition, else empty).
    pub(crate) prefix: String,
    /// The raw text to escape, per paragraph.
    pub(crate) texts: Vec<String>,
    /// The (section, top-level paragraph) of each.
    pub(crate) at: Vec<(usize, usize)>,
}

/// The view's blocks joined, after the check: each block holding foreign
/// markup on its own is written as plain text; if the whole still fails
/// (a block's markup working with another's), every block is. One
/// warning per paragraph written as plain text.
/// 이것을 실패시키는 것: 검사 삭제 · 대체 삭제(`guard_falls_back_to_plain_text`).
pub(crate) fn checked(
    mut blocks: Vec<Block>,
    allowed: &Allowed<'_>,
    warnings: &mut Vec<super::MdWarning>,
) -> String {
    // An empty block (a note with no text) writes nothing, not a gap.
    let join = |blocks: &[Block]| {
        let parts: Vec<&str> =
            blocks.iter().map(|b| b.markdown.as_str()).filter(|m| !m.is_empty()).collect();
        parts.join("\n\n")
    };
    let all = join(&blocks);
    if !foreign_markup(&all, allowed) {
        return all;
    }
    let fall_back = |b: &mut Block, warnings: &mut Vec<super::MdWarning>| {
        if let Some(src) = b.source.take() {
            let text: Vec<String> = src.texts.iter().map(|t| plain_text(t)).collect();
            b.markdown = format!("{}{}", src.prefix, text.join("\n\n"));
            for (section, paragraph) in src.at {
                warnings.push(super::MdWarning::MarkupWrittenAsText { section, paragraph });
            }
        }
    };
    for b in &mut blocks {
        if b.source.is_some() && foreign_markup(&b.markdown, allowed) {
            fall_back(b, warnings);
        }
    }
    let all = join(&blocks);
    if !foreign_markup(&all, allowed) {
        return all;
    }
    for b in &mut blocks {
        fall_back(b, warnings);
    }
    join(&blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing errors cannot be accepted as an arbitrary image alt value.
    /// 이것을 실패시키는 것: 속성 실패를 값으로 반환 · 따옴표 안에서 태그 종료.
    #[test]
    fn guard_rejects_malformed_image_attributes() {
        let destinations = HashSet::new();
        let allowed = Allowed { tags: STYLED_TAGS, destinations: &destinations };
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
        assert!(!foreign_markup("<img alt=\">\">", &allowed));
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
                let raw = deep_text_at(&p, depth, &mut || visits += 1);
                assert_eq!(visits, (32 - depth).min(containers) + 1);
                let expected = if containers + depth <= 32 { "core" } else { TOO_DEEP };
                assert_eq!(raw, expected);
                let block = Block {
                    markdown: "<script>bad</script>".into(),
                    source: Some(Source {
                        prefix: String::new(),
                        texts: vec![raw],
                        at: vec![(0, 0)],
                    }),
                };
                let destinations = HashSet::new();
                let allowed = Allowed { tags: STYLED_TAGS, destinations: &destinations };
                let mut warnings = Vec::new();
                assert_eq!(checked(vec![block], &allowed, &mut warnings), plain_text(expected));
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
        let allowed = Allowed { tags: STYLED_TAGS, destinations: &dests };
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
        let lossy = Allowed { tags: LOSSY_TAGS, destinations: &dests };
        assert!(foreign_markup("<strong>b</strong>", &lossy));
        assert!(!foreign_markup("a<br>b <!-- hwpforge:section -->", &lossy));
    }

    /// A block with foreign markup is written as plain text and reported;
    /// a clean one is left as it is.
    ///
    /// 이것을 실패시키는 것: 대체 삭제 · 경고 삭제 · 깨끗한 블록까지 대체하기 ·
    /// 문서 전체 재검사 삭제.
    #[test]
    fn guard_falls_back_to_plain_text() {
        let dests = HashSet::new();
        let allowed = Allowed { tags: LOSSY_TAGS, destinations: &dests };
        let block = |md: &str, raw: &str, at| Block {
            markdown: md.to_string(),
            source: Some(Source {
                prefix: String::new(),
                texts: vec![raw.to_string()],
                at: vec![at],
            }),
        };
        let mut warnings = Vec::new();
        let out = checked(
            vec![
                block("ok", "ok", (0, 0)),
                block("<img src=x onerror=y>", "<img src=x onerror=y>", (0, 1)),
            ],
            &allowed,
            &mut warnings,
        );
        assert_eq!(out, "ok\n\n\\<img src\\=x onerror\\=y\\>");
        assert_eq!(
            warnings,
            vec![super::super::MdWarning::MarkupWrittenAsText { section: 0, paragraph: 1 }]
        );
        // A definition in one block used by another: neither fails alone
        // as a link, the definition does; both are checked as a whole too.
        let mut warnings = Vec::new();
        let out = checked(
            vec![
                block("[a]", "[a]", (0, 0)),
                block("[a]: https://x.invalid", "[a]: https://x.invalid", (0, 1)),
            ],
            &allowed,
            &mut warnings,
        );
        assert_eq!(out, "[a]\n\n\\[a\\]\\: https\\:\\/\\/x\\.invalid");
        assert_eq!(warnings.len(), 1);
        let out = checked(vec![block("x", "x", (0, 0))], &allowed, &mut Vec::new());
        assert_eq!(out, "x");
    }
}
