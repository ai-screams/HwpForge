//! The readable views (styled, lossy) cannot be made to write markup by the
//! document's text: every attack string is put in every place text reaches
//! the output, and the output is parsed with pulldown-cmark (tables,
//! footnotes, strikethrough — no math) to check what a renderer would see:
//!
//! - raw HTML is only what the encoder itself writes ([`ALLOWED_TAGS`],
//!   comments), with `a href` an allowed URL put in and `img src` an asset
//!   key;
//! - every link destination is a URL put in and allowed, every image
//!   destination is the picture's own key (styled: an `images` key; lossy:
//!   its path);
//! - a GFM table keeps its one column in every row;
//! - styled: the attack's text comes back whole as text (not for line
//!   breaks, which become breaks, or equations, which have their own test).
//!
//! - the final check (re-parsing the output) never has to step in: the
//!   escaping rules hold on their own (`MarkupWrittenAsText` fails it).
//!
//! Lossy writes `*`, `#` and the like as they are (decision of 2026-10-15:
//! only what CommonMark reads as raw HTML, links, images or entities is
//! escaped), so only the safety properties are checked for it.

use std::collections::BTreeSet;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{Control, RefTarget};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::{
    Document, PageSettings, Paragraph, Run, Section, StyleLookup, Table, TableCell, TableRow,
    Validated,
};
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex, UnderlineType};
use hwpforge_smithy_md::{MdEncoder, MdWarning};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Attack strings: each tries to make the output write markup.
const ATTACKS: &[&str] = &[
    "<img src=x onerror=alert(1)>",
    "<img title='' src=x onerror=alert(1)>",
    "`<img src=x onerror=alert(1)>`",
    "<script>alert(1)</script>",
    "<b>bold</b>",
    "<div 가",
    "<!-- c --> -->",
    "[x](javascript:alert(1))",
    "![x](javascript:alert(1))",
    "a](javascript:x)[b",
    ")[x](javascript:alert(1))",
    "x)<img/src=x/onerror=y>",
    "x\\",
    "\\",
    "x\\)",
    "<https://evil.invalid>",
    "<a@evil.invalid>",
    "[a]: javascript:alert(1)",
    "> [a]: javascript:alert(1)",
    "- [a]: javascript:alert(1)",
    "1. > [a]: javascript:alert(1)",
    "[a\nb]: javascript:alert(1)",
    "(javascript:alert(1))",
    "`",
    "``x``",
    "```",
    "~~~",
    "*x* _y_ **z**",
    "!",
    "&lt;img src=x onerror=y&gt; &amp;",
    "%3Cimg%3E",
    "x|y",
    "|",
    "a\nb",
    "a\rb",
    "a\r\nb",
    "a\n\nb",
    "a\r\rb",
    "a\n \n[x](javascript:z)",
    "```\n<img src=x onerror=y>\n```",
    "$x$",
    "#",
];

/// The tags the encoder writes itself (lower case).
const ALLOWED_TAGS: &[&str] =
    &["table", "tbody", "tr", "td", "br", "strong", "em", "del", "u", "sup", "sub", "a", "img"];

/// The allowed URL a link carrier uses (`ATTACK` is appended for the
/// destination carrier).
const URL: &str = "https://e.invalid/";

/// Character shapes the style lookup answers for.
const PLAIN: usize = 0;
const UNDERLINE: usize = 1;
const CODE: usize = 2;
const BOLD: usize = 3;

struct Styles;

impl StyleLookup for Styles {
    fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
        Some(id.get() == BOLD)
    }
    fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
        (id.get() == UNDERLINE).then_some(UnderlineType::Bottom)
    }
    fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
        Some(if id.get() == CODE { "D2Coding" } else { "Batang" })
    }
}

fn cs(i: usize) -> CharShapeIndex {
    CharShapeIndex::new(i)
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn text(s: &str) -> Run {
    Run::text(s, cs(PLAIN))
}

fn ctrl(c: Control) -> Run {
    Run::control(c, cs(PLAIN))
}

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn picture(path: &str) -> Image {
    Image::new(path, u(1000), u(1000), ImageFormat::Png)
}

/// 빈 틀에는 링크/자산이 없지만 캡션은 그대로 읽을 수 있다.
/// 이것을 실패시키는 것: styled Markdown/HTML 또는 lossy 빈 경로 가드 삭제.
#[test]
fn assetless_pictures_emit_no_readable_links() {
    for container in [Container::Body, Container::Cell, Container::MergedCell] {
        let mut image = picture("");
        image.caption =
            Some(Caption::new(vec![para(vec![text("kept caption")])], CaptionSide::Bottom));
        let doc = document(container, vec![Run::image(image, cs(PLAIN))]);
        let styled = MdEncoder::encode_styled(&doc, &Styles);
        assert!(styled.images.is_empty(), "{container:?}");
        for md in [styled.markdown, MdEncoder::encode_lossy(&doc).unwrap()] {
            assert!(!md.contains("![") && !md.contains("<img"), "{container:?}: {md}");
            assert!(md.contains("kept caption"), "{container:?}: {md}");
        }
    }
}

/// 이것을 실패시키는 것: styled의 그림 앞 공백 조건에서 빈 경로 검사 삭제.
#[test]
fn assetless_picture_does_not_insert_a_separator() {
    for container in [Container::Body, Container::Cell, Container::MergedCell] {
        for tail in [vec![], vec![text("right")]] {
            let mut runs = vec![text("left"), Run::image(picture(""), cs(PLAIN))];
            runs.extend(tail.clone());
            let actual = document(container, runs);
            let mut expected = vec![text("left")];
            expected.extend(tail);
            let expected = document(container, expected);
            assert_eq!(
                MdEncoder::encode_styled(&actual, &Styles).markdown,
                MdEncoder::encode_styled(&expected, &Styles).markdown,
                "{container:?}"
            );
        }
    }
}

/// Where an attack string is put: the runs of one paragraph.
#[derive(Clone, Copy, Debug)]
enum Carrier {
    /// Plain text, split into two runs at every char boundary.
    Split,
    Underline,
    Bold,
    /// A paragraph in a code font (styled: a code block).
    Code,
    LinkLabel,
    LinkUrl,
    /// A raw `!` text run, then a link whose label is the attack.
    BangThenLink,
    /// The attack split at every point around a link (`t`, an allowed URL).
    AroundLink,
    /// The attack split at every point around a cross reference with no
    /// display text (written as `[b]`).
    AroundCrossRef,
    /// The attack split at every point into plain then underlined text.
    FormatSplit,
    PicturePath,
    Compose,
    DutmalMain,
    DutmalSub,
    CrossRef,
    Field,
    FieldBoundary,
    Unknown,
    TextBox,
    Footnote,
    Equation,
}

const CARRIERS: &[Carrier] = &[
    Carrier::Split,
    Carrier::Underline,
    Carrier::Bold,
    Carrier::Code,
    Carrier::LinkLabel,
    Carrier::LinkUrl,
    Carrier::BangThenLink,
    Carrier::AroundLink,
    Carrier::AroundCrossRef,
    Carrier::FormatSplit,
    Carrier::PicturePath,
    Carrier::Compose,
    Carrier::DutmalMain,
    Carrier::DutmalSub,
    Carrier::CrossRef,
    Carrier::Field,
    Carrier::FieldBoundary,
    Carrier::Unknown,
    Carrier::TextBox,
    Carrier::Footnote,
    Carrier::Equation,
];

/// The runs carrying `attack`, one variant per split for [`Carrier::Split`].
fn carry(carrier: Carrier, attack: &str) -> Vec<Vec<Run>> {
    match carrier {
        Carrier::Split => {
            let mut out = vec![vec![text(attack)]];
            for (i, _) in attack.char_indices().skip(1) {
                out.push(vec![text(&attack[..i]), text(&attack[i..])]);
            }
            out
        }
        Carrier::Underline => vec![vec![Run::text(attack, cs(UNDERLINE))]],
        Carrier::Bold => vec![vec![Run::text(attack, cs(BOLD))]],
        Carrier::Code => vec![vec![Run::text(attack, cs(CODE))]],
        Carrier::LinkLabel => vec![vec![ctrl(Control::hyperlink(attack, URL))]],
        Carrier::LinkUrl => vec![vec![ctrl(Control::hyperlink("t", &format!("{URL}{attack}")))]],
        Carrier::BangThenLink => vec![vec![text("!"), ctrl(Control::hyperlink(attack, URL))]],
        Carrier::AroundLink | Carrier::AroundCrossRef | Carrier::FormatSplit => {
            let mut points: Vec<usize> = attack.char_indices().map(|(i, _)| i).collect();
            points.push(attack.len());
            points
                .into_iter()
                .map(|i| {
                    let (l, r) = attack.split_at(i);
                    match carrier {
                        Carrier::AroundLink => {
                            vec![text(l), ctrl(Control::hyperlink("t", URL)), text(r)]
                        }
                        Carrier::AroundCrossRef => vec![text(l), ctrl(empty_cross_ref()), text(r)],
                        _ => vec![text(l), Run::text(r, cs(UNDERLINE))],
                    }
                })
                .collect()
        }
        Carrier::PicturePath => {
            vec![vec![Run::image(picture(&format!("BinData/{attack}.png")), cs(PLAIN))]]
        }
        Carrier::Compose => vec![vec![ctrl(Control::compose(attack))]],
        Carrier::DutmalMain => vec![vec![ctrl(Control::dutmal(attack, "s"))]],
        Carrier::DutmalSub => vec![vec![ctrl(Control::dutmal("m", attack))]],
        Carrier::CrossRef => {
            let mut c = empty_cross_ref();
            if let Control::CrossRef { display_text, .. } = &mut c {
                *display_text = attack.to_string();
            }
            vec![vec![ctrl(c)]]
        }
        Carrier::FieldBoundary => {
            use hwpforge_core::control::{FieldBoundary, FieldParameter, FieldStart};
            let mut s = FieldStart::default();
            s.id = attack.into();
            s.kind = "FORMULA".into();
            s.name = attack.into();
            s.parameters.push(FieldParameter::String {
                name: attack.into(),
                value: attack.into(),
                xml_space: attack.into(),
            });
            vec![vec![
                ctrl(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) }),
                text(attack),
                ctrl(Control::FieldBoundary {
                    boundary: FieldBoundary::End {
                        begin_id: attack.into(),
                        field_id: attack.into(),
                    },
                }),
            ]]
        }
        Carrier::Field => vec![vec![ctrl(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: attack.to_string(),
        })]],
        Carrier::Unknown => {
            vec![vec![ctrl(Control::Unknown { tag: attack.to_string(), data: None })]]
        }
        Carrier::TextBox => {
            vec![vec![ctrl(Control::text_box(vec![para(vec![text(attack)])], u(1000), u(1000)))]]
        }
        Carrier::Footnote => vec![vec![ctrl(Control::footnote(vec![para(vec![text(attack)])]))]],
        Carrier::Equation => vec![vec![ctrl(Control::equation(attack))]],
    }
}

/// A cross reference to bookmark `b` with no display text.
fn empty_cross_ref() -> Control {
    Control::cross_ref(RefTarget::Name("b".into()), Default::default(), Default::default())
}

/// Where the carrying paragraph is put.
#[derive(Clone, Copy, Debug)]
enum Container {
    Body,
    /// A one-cell GFM table.
    Cell,
    /// A table in a cell (styled: an HTML table).
    NestedCell,
    /// A merged cell (styled: an HTML table).
    MergedCell,
    /// A table's caption.
    Caption,
    /// A text box holding the paragraph.
    TextBox,
    /// A footnote holding the paragraph (styled: its definition).
    Footnote,
    Memo,
}

const CONTAINERS: &[Container] = &[
    Container::Body,
    Container::Cell,
    Container::NestedCell,
    Container::MergedCell,
    Container::Caption,
    Container::TextBox,
    Container::Footnote,
    Container::Memo,
];

fn one_cell(p: Paragraph) -> Table {
    Table::new(vec![TableRow::new(vec![TableCell::new(vec![p], u(1000))])])
}

/// The document: the carrying paragraph in `container`, then a paragraph
/// using the reference label `[a]` (so a written definition would link).
fn document(container: Container, runs: Vec<Run>) -> Document<Validated> {
    let p = para(runs);
    let top = match container {
        Container::Body => p,
        Container::Cell => para(vec![Run::table(one_cell(p), cs(PLAIN))]),
        Container::NestedCell => {
            let inner = Run::table(one_cell(p), cs(PLAIN));
            para(vec![Run::table(one_cell(para(vec![inner])), cs(PLAIN))])
        }
        Container::MergedCell => {
            let mut cell = TableCell::new(vec![p], u(1000));
            cell.col_span = 2;
            para(vec![Run::table(Table::new(vec![TableRow::new(vec![cell])]), cs(PLAIN))])
        }
        Container::Caption => {
            let mut table = one_cell(para(vec![text("c")]));
            table.caption = Some(Caption::new(vec![p], CaptionSide::Bottom));
            para(vec![Run::table(table, cs(PLAIN))])
        }
        Container::TextBox => {
            para(vec![ctrl(Control::text_box(vec![p], u(1000), u(1000))), text("z")])
        }
        Container::Footnote => para(vec![text("z"), ctrl(Control::footnote(vec![p]))]),
        Container::Memo => para(vec![text("z"), ctrl(Control::memo(vec![p]))]),
    };
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![top, para(vec![text("[a] [a b]")])],
        PageSettings::a4(),
    ));
    doc.validate().expect("valid")
}

/// What a check found wrong, with the case that made it.
fn fail(view: &str, container: Container, carrier: Carrier, attack: &str, why: String) -> String {
    format!("{view} {container:?} {carrier:?} {attack:?}: {why}")
}

/// The tags of an HTML fragment: comments are skipped, every other `<…>`
/// must be an allowed tag with allowed attributes. Returns the problems.
fn check_html(html: &str, assets: &BTreeSet<String>, urls: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if let Some(body) = rest.strip_prefix("<!--") {
            match body.find("-->") {
                Some(end) => rest = &body[end + 3..],
                None => {
                    problems.push(format!("unclosed comment in {html:?}"));
                    break;
                }
            }
            continue;
        }
        let Some(end) = html_tag_end(rest) else {
            problems.push(format!("unclosed tag in {html:?}"));
            break;
        };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        let tag = tag.trim_start_matches('/').trim_end_matches('/');
        let name: String =
            tag.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>();
        if !ALLOWED_TAGS.contains(&name.to_ascii_lowercase().as_str()) {
            problems.push(format!("tag <{tag}>"));
            continue;
        }
        let Some(attrs) = attributes(&tag[name.len()..]) else {
            problems.push(format!("malformed attributes on <{name}>"));
            continue;
        };
        for (attr, value) in attrs {
            let value = html_unescape(&value);
            let ok = match (name.as_str(), attr.as_str()) {
                ("td", "colspan" | "rowspan") => value.chars().all(|c| c.is_ascii_digit()),
                ("a", "href") => urls.contains(&value),
                ("img", "src") => assets.contains(&value),
                ("img", "alt") => true,
                _ => false,
            };
            if !ok {
                problems.push(format!("attribute {attr}={value:?} on <{name}>"));
            }
        }
    }
    problems
}

fn html_tag_end(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '>' {
            return Some(i);
        }
        if matches!(c, '\'' | '"') {
            chars.find(|&(_, next)| next == c)?;
        }
    }
    None
}

/// `name="value"` pairs (the encoder quotes every value it writes).
fn attributes(s: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.trim().is_empty() {
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let (name, after) = rest.trim_start().split_once('=')?;
        let name = name.trim_end();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let after = after.trim_start();
        let q = after.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let (value, tail) = after[1..].split_once(q)?;
        out.push((name.to_string(), value.to_string()));
        rest = tail;
    }
    Some(out)
}

#[test]
fn html_harness_rejects_malformed_attributes() {
    let empty = BTreeSet::new();
    for html in [
        "<img alt=x src=x onerror=alert(1)>",
        "<img alt=\">\" src=x onerror=alert(1)>",
        "<img alt=\">\" onerror=\"alert(1)\">",
        "<img alt=\"unfinished>",
        "<img alt>",
    ] {
        assert!(!check_html(html, &empty, &empty).is_empty(), "{html}");
    }
    assert!(check_html("<img alt=\">\">", &empty, &empty).is_empty());
}

fn html_unescape(s: &str) -> String {
    s.replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Visible text of HTML: tags and comments dropped, entities decoded.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = if rest.starts_with("<!--") {
            rest.find("-->").map(|e| e + 3)
        } else {
            html_tag_end(rest).map(|e| e + 1)
        };
        match end {
            Some(e) => rest = &rest[e..],
            None => break,
        }
    }
    out.push_str(rest);
    html_unescape(&out)
}

/// Problems a renderer would show for `markdown`, and its visible text.
fn inspect(
    markdown: &str,
    assets: &BTreeSet<String>,
    urls: &BTreeSet<String>,
    expect: Expect,
) -> (Vec<String>, String) {
    let mut problems = Vec::new();
    let (mut tables, mut images, mut links) = (0, 0, 0);
    let mut visible = String::new();
    let mut cells_in_row = 0usize;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    // An HTML block comes line by line: its lines are checked together.
    let mut html = String::new();
    let flush = |html: &mut String, problems: &mut Vec<String>, visible: &mut String| {
        if !html.is_empty() {
            problems.extend(check_html(html, assets, urls));
            visible.push_str(&html_text(html));
            html.clear();
        }
    };
    for event in Parser::new_ext(markdown, options) {
        if !matches!(event, Event::Html(_)) {
            flush(&mut html, &mut problems, &mut visible);
        }
        match event {
            Event::Html(h) => html.push_str(&h),
            Event::InlineHtml(h) => {
                problems.extend(check_html(&h, assets, urls));
                visible.push_str(&html_text(&h));
            }
            Event::Text(t) | Event::Code(t) => visible.push_str(&t),
            Event::Start(Tag::Link { dest_url, .. }) => {
                links += 1;
                if !urls.contains(dest_url.as_ref()) {
                    problems.push(format!("link to {dest_url:?}"));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                images += 1;
                if !assets.contains(dest_url.as_ref()) {
                    problems.push(format!("image from {dest_url:?}"));
                }
            }
            Event::Start(Tag::Table(_)) => tables += 1,
            Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => cells_in_row = 0,
            Event::Start(Tag::TableCell) => cells_in_row += 1,
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                if cells_in_row != 1 {
                    problems.push(format!("table row with {cells_in_row} cells"));
                }
            }
            _ => {}
        }
    }
    flush(&mut html, &mut problems, &mut visible);
    if expect.table && tables == 0 {
        problems.push("the GFM table is gone".into());
    }
    if expect.image && images == 0 {
        problems.push("the picture is not an image".into());
    }
    if expect.link && links == 0 {
        problems.push("the link is not a link".into());
    }
    (problems, visible)
}

/// What must still be there: a GFM table, a Markdown image, a Markdown
/// link.
#[derive(Clone, Copy, Default)]
struct Expect {
    table: bool,
    image: bool,
    link: bool,
}

/// A warning from the final check (`MarkupWrittenAsText`): the escaping
/// rules let markup through, and only the check kept it out. The rules
/// must hold on their own, so this fails the test too.
fn guarded(w: &MdWarning) -> bool {
    matches!(w, MdWarning::MarkupWrittenAsText { .. })
}

/// Whether the carrier writes a link (an allowed URL).
fn linked(carrier: Carrier, urls: &BTreeSet<String>, attack: &str) -> bool {
    match carrier {
        Carrier::LinkLabel | Carrier::BangThenLink | Carrier::AroundLink => true,
        Carrier::LinkUrl => urls.contains(&format!("{URL}{attack}")),
        _ => false,
    }
}

/// The styled key of a picture without bytes: `images/` and the path's
/// file name, parentheses percent-encoded.
fn asset_key(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or("image");
    format!("images/{}", name.replace('(', "%28").replace(')', "%29"))
}

/// Whether the styled view's text for `attack` in `carrier` must come back.
fn text_must_return(container: Container, carrier: Carrier, attack: &str) -> bool {
    // A list marker at a paragraph's start makes a list in styled too (a
    // known gap, not markup made from text: followups).
    let list_start = attack.starts_with("- ") || attack.starts_with("1. ");
    !attack.contains(['\n', '\r'])
        && !list_start
        && !matches!(container, Container::Memo)
        && matches!(
            carrier,
            Carrier::Split
                | Carrier::FormatSplit
                | Carrier::Underline
                | Carrier::Bold
                | Carrier::Code
                | Carrier::LinkLabel
                | Carrier::BangThenLink
                | Carrier::Compose
                | Carrier::DutmalMain
                | Carrier::DutmalSub
                | Carrier::CrossRef
                | Carrier::Field
                | Carrier::FieldBoundary
                | Carrier::Unknown
                | Carrier::TextBox
                | Carrier::Footnote
        )
}

#[test]
fn readable_views_write_no_markup_from_text() {
    let mut problems = Vec::new();
    for &attack in ATTACKS {
        let mut urls: BTreeSet<String> = [URL.to_string()].into();
        let link_url = format!("{URL}{attack}");
        if !link_url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            urls.insert(link_url);
        }
        for &carrier in CARRIERS {
            for runs in carry(carrier, attack) {
                for &container in CONTAINERS {
                    let doc = document(container, runs.clone());

                    let styled = MdEncoder::encode_styled(&doc, &Styles);
                    if styled.warnings.iter().any(guarded) {
                        problems.push(fail(
                            "styled",
                            container,
                            carrier,
                            attack,
                            "the final check had to write plain text".into(),
                        ));
                    }
                    // The picture has no bytes, so its key is not in
                    // `images`: the key the encoder gives it.
                    let mut assets: BTreeSet<String> = styled.images.keys().cloned().collect();
                    let key = asset_key(&format!("BinData/{attack}.png"));
                    assets.insert(key.replace('\n', "%0A").replace('\r', "%0D"));
                    assets.insert(key);
                    // Styled draws a one-cell table (body cell, caption's
                    // table) as GFM; a picture outside a memo and an HTML
                    // cell as a Markdown image.
                    let gfm = matches!(container, Container::Cell | Container::Caption);
                    let html = matches!(container, Container::NestedCell | Container::MergedCell);
                    let picture = matches!(carrier, Carrier::PicturePath)
                        && !matches!(container, Container::Memo);
                    let markdown = !html && !matches!(container, Container::Memo);
                    let link = linked(carrier, &urls, attack);
                    let expect =
                        Expect { table: gfm, image: picture && !html, link: link && markdown };
                    let (found, visible) = inspect(&styled.markdown, &assets, &urls, expect);
                    for why in found {
                        problems.push(fail("styled", container, carrier, attack, why));
                    }
                    if text_must_return(container, carrier, attack) && !visible.contains(attack) {
                        problems.push(fail(
                            "styled",
                            container,
                            carrier,
                            attack,
                            format!("text not returned in {:?}", styled.markdown),
                        ));
                    }

                    let (lossy, lossy_warnings) =
                        MdEncoder::encode_lossy_with_report(&doc).expect("lossy");
                    if lossy_warnings.iter().any(guarded) {
                        problems.push(fail(
                            "lossy",
                            container,
                            carrier,
                            attack,
                            "the final check had to write plain text".into(),
                        ));
                    }
                    // A line ending cannot stand in a destination: it is
                    // percent-encoded, the one change to the path.
                    let path: BTreeSet<String> =
                        [format!("BinData/{attack}.png").replace('\n', "%0A").replace('\r', "%0D")]
                            .into();
                    // Lossy draws every table as GFM, and pictures in body,
                    // cells and captions (not in text boxes or notes).
                    let table = matches!(
                        container,
                        Container::Cell
                            | Container::NestedCell
                            | Container::MergedCell
                            | Container::Caption
                    );
                    let picture = matches!(carrier, Carrier::PicturePath)
                        && !matches!(
                            container,
                            Container::TextBox | Container::Footnote | Container::Memo
                        );
                    let link = linked(carrier, &urls, attack)
                        && !matches!(
                            container,
                            Container::TextBox | Container::Footnote | Container::Memo
                        );
                    let (found, _) =
                        inspect(&lossy, &path, &urls, Expect { table, image: picture, link });
                    for why in found {
                        problems.push(fail("lossy", container, carrier, attack, why));
                    }
                }
            }
        }
    }
    problems.sort();
    problems.dedup();
    if let Ok(path) = std::env::var("MARKUP_SAFETY_DUMP") {
        std::fs::write(path, problems.join("\n")).expect("dump");
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.iter().take(200).cloned().collect::<Vec<_>>().join("\n")
    );
}
