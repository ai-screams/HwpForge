//! The styled view's guarantee for document text (design 018 G): in a Body
//! block — a top-level body paragraph that is not a continuation — and in a
//! guarded cell, document text makes no block structure and comes back as
//! the same characters; what a rule cannot close is written as plain text
//! with one warning per paragraph. Outside it (lists, headings,
//! continuations, notes, memos, one-table or one-picture captions, lossy)
//! the output is the base commit's, byte for byte (`BASE`).
//!
//! The documents are built in [`cases`] with the public API only, so the
//! same builders run against the base commit's crates to give `BASE`.

use pulldown_cmark::{Event, Parser, Tag};

use super::guard::{checked, Allowed, Block, Source};
use super::markup::Md;
use super::styled::encode_styled;
use super::{md_options, MdWarning};
use cases::*;

// ---- cases: public API only (also built against the base commit) ----
#[allow(dead_code)]
mod cases {
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::control::{
        AutoNumberFormat, AutoNumberKind, Control, RefTarget, ShapePoint,
    };
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::{
        Document, PageSettings, Paragraph, Run, Section, StyleLookup, Table, TableCell, TableRow,
        Validated,
    };
    use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex, UnderlineType};

    /// Character shapes.
    pub const PLAIN: usize = 0;
    pub const BOLD: usize = 1;
    pub const UNDERLINE: usize = 2;
    pub const CODE: usize = 3;
    /// Character shapes by emphasis: bold, italic, strikeout and their
    /// combinations (bit 1 bold, bit 2 italic, bit 4 strikeout, plus 16).
    pub fn emphasis(bold: bool, italic: bool, strike: bool) -> usize {
        16 + usize::from(bold) + 2 * usize::from(italic) + 4 * usize::from(strike)
    }
    /// Paragraph shapes.
    pub const BODY: usize = 0;
    pub const BULLET: usize = 1;
    pub const NUMBER: usize = 2;
    pub const BULLET1: usize = 3;
    pub const BULLET2: usize = 4;
    pub const HEADING: usize = 5;
    pub const CONT0: usize = 6;
    pub const CONT1: usize = 7;

    pub struct Styles;

    impl StyleLookup for Styles {
        fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
            Some(id.get() == BOLD || (id.get() >= 16 && id.get() & 1 == 1))
        }
        fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
            Some(id.get() >= 16 && id.get() & 2 == 2)
        }
        fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
            Some(id.get() >= 16 && id.get() & 4 == 4)
        }
        fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
            (id.get() == UNDERLINE).then_some(UnderlineType::Bottom)
        }
        /// Two pictures with the same file name and other bytes; `N/<i>/a.png`
        /// all named `a.png`, each its own bytes (`i + 1` zeros).
        fn image_data(&self, key: &str) -> Option<&[u8]> {
            static ZEROS: [u8; 8192] = [0; 8192];
            match key {
                "A/one.png" => Some(b"1"),
                "B/one.png" => Some(b"2"),
                _ => key
                    .strip_prefix("N/")
                    .and_then(|k| k.split('/').next()?.parse::<usize>().ok())
                    .map(|i| &ZEROS[..=i]),
            }
        }
        fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
            Some(if id.get() == CODE { "D2Coding" } else { "Batang" })
        }
        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
            (id.get() == HEADING).then_some(1)
        }
        fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
            match id.get() {
                BULLET | BULLET1 | BULLET2 => Some("BULLET"),
                NUMBER => Some("NUMBER"),
                _ => None,
            }
        }
        fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8> {
            match id.get() {
                BULLET1 => Some(1),
                BULLET2 => Some(2),
                _ => Some(0),
            }
        }
        fn para_style_name(&self, id: ParaShapeIndex) -> Option<&str> {
            match id.get() {
                CONT0 => Some("__hwpforge_md_list_continuation_level_0"),
                CONT1 => Some("__hwpforge_md_list_continuation_level_1"),
                _ => None,
            }
        }
    }

    pub fn u(v: i32) -> HwpUnit {
        HwpUnit::new(v).unwrap()
    }
    pub fn p(shape: usize, runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(shape))
    }
    pub fn body(runs: Vec<Run>) -> Paragraph {
        p(BODY, runs)
    }
    pub fn t(s: &str) -> Run {
        Run::text(s, CharShapeIndex::new(PLAIN))
    }
    pub fn styled_t(s: &str, shape: usize) -> Run {
        Run::text(s, CharShapeIndex::new(shape))
    }
    pub fn ctrl(c: Control) -> Run {
        Run::control(c, CharShapeIndex::new(PLAIN))
    }
    pub fn compose(s: &str) -> Run {
        ctrl(Control::compose(s))
    }
    pub fn cross_ref(display: &str) -> Run {
        let mut c = Control::cross_ref(
            RefTarget::Name("ref".into()),
            Default::default(),
            Default::default(),
        );
        if let Control::CrossRef { display_text, .. } = &mut c {
            *display_text = display.into();
        }
        ctrl(c)
    }
    pub fn field(display: &str) -> Run {
        ctrl(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: display.into(),
        })
    }
    pub fn unknown(tag: &str) -> Run {
        ctrl(Control::Unknown { tag: tag.into(), data: None })
    }
    pub fn auto_number(suffix: char) -> Run {
        let format = AutoNumberFormat { suffix: Some(suffix), ..Default::default() };
        ctrl(Control::AutoNumber { kind: AutoNumberKind::Table, num: 1, format })
    }
    pub fn one_cell(paragraph: Paragraph) -> Table {
        Table::new(vec![TableRow::new(vec![TableCell::new(vec![paragraph], u(1000))])])
    }
    /// A one-cell table drawn as HTML (its cell spans two columns).
    pub fn merged(paragraph: Paragraph) -> Table {
        let mut cell = TableCell::new(vec![paragraph], u(1000));
        cell.col_span = 2;
        Table::new(vec![TableRow::new(vec![cell])])
    }
    pub fn table(table: Table) -> Run {
        Run::table(table, CharShapeIndex::new(PLAIN))
    }
    pub fn captioned(mut t: Table, caption: Vec<Paragraph>) -> Run {
        t.caption = Some(Caption::new(caption, CaptionSide::Bottom));
        table(t)
    }
    pub fn text_box(paragraphs: Vec<Paragraph>) -> Run {
        ctrl(Control::text_box(paragraphs, u(1000), u(1000)))
    }
    pub fn image(path: &str, caption: Option<Vec<Paragraph>>) -> Run {
        let mut image = Image::new(path, u(1000), u(1000), ImageFormat::Png);
        image.caption = caption.map(|c| Caption::new(c, CaptionSide::Bottom));
        Run::image(image, CharShapeIndex::new(PLAIN))
    }
    pub fn memo(paragraphs: Vec<Paragraph>) -> Run {
        ctrl(Control::memo(paragraphs))
    }
    /// A memo with no body whose anchor is `anchor`.
    pub fn anchored_memo(anchor: Vec<Run>) -> Run {
        ctrl(Control::memo_with_anchor(vec![], anchor))
    }
    /// A memo in another memo's anchor, its own anchor bold `- x`.
    pub fn nested_bold() -> Run {
        anchored_memo(vec![anchored_memo(vec![styled_t("- x", BOLD)])])
    }
    pub fn footnote(paragraphs: Vec<Paragraph>) -> Run {
        ctrl(Control::footnote(paragraphs))
    }
    pub fn chart() -> Run {
        use hwpforge_core::chart::{ChartData, ChartSeries, ChartType};
        ctrl(Control::chart(
            ChartType::Bar,
            ChartData::Category {
                categories: vec!["A".into()],
                series: vec![ChartSeries { name: "S".into(), values: vec![1.0] }],
            },
        ))
    }
    pub fn line() -> Run {
        ctrl(Control::line(ShapePoint::new(0, 0), ShapePoint::new(1000, 0)).unwrap())
    }
    pub fn doc(paragraphs: Vec<Paragraph>) -> Document<Validated> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
        doc.validate().expect("valid")
    }

    /// The fixture `fixture-md-lookalike` in the shape Hancom saved it (no
    /// copy of the file): `auto` with the list paragraphs and the line
    /// shape Hancom's auto-format made, else the literal text.
    pub fn lookalike(auto: bool) -> Document<Validated> {
        let first = if auto {
            vec![p(BULLET, vec![t("항목처럼 보이는 글")]), p(NUMBER, vec![t("번호처럼 보이는 글")])]
        } else {
            vec![body(vec![t("- 항목처럼 보이는 글")]), body(vec![t("1. 번호처럼 보이는 글")])]
        };
        let rule = if auto { body(vec![t(""), line()]) } else { body(vec![t("===")]) };
        let mut paragraphs = first;
        paragraphs.extend([
            body(vec![t(": 정의처럼 보이는 글")]),
            rule,
            body(vec![t("a | b")]),
            body(vec![t("--- | ---")]),
            body(vec![t("")]),
            body(vec![t("    들여쓴 글")]),
        ]);
        doc(paragraphs)
    }

    /// The documents whose styled (and lossy) output must be the base's.
    pub fn base_cases() -> Vec<(&'static str, Document<Validated>)> {
        let amp = || vec![t("&"), compose("amp;")];
        vec![
            ("g0_auto", lookalike(true)),
            ("g2_heading_split", doc(vec![p(HEADING, vec![t("1"), compose(". x")])])),
            (
                "g2_continuation_after_empty_item",
                doc(vec![body(vec![t("a")]), p(BULLET1, vec![t("")]), p(CONT1, vec![t("b")])]),
            ),
            ("g3_item_blank_line", doc(vec![p(BULLET, vec![t("a\n\n    b")])])),
            ("g3_number_continuation", doc(vec![p(NUMBER, vec![t("a")]), p(CONT0, vec![t("b")])])),
            ("g3_bullet_continuation", doc(vec![p(BULLET, vec![t("a")]), p(CONT0, vec![t("b")])])),
            ("g3_list_one_table", doc(vec![p(BULLET, vec![table(one_cell(body(vec![t("x")])))])])),
            ("g3_heading_one_image", doc(vec![p(HEADING, vec![image("BinData/p.png", None)])])),
            (
                "g3_note_paragraphs",
                doc(vec![body(vec![
                    t("z"),
                    footnote(vec![body(vec![t("- x")]), body(vec![t("1. y")])]),
                ])]),
            ),
            ("g3_code", doc(vec![body(vec![styled_t("- x", CODE)])])),
            (
                "g3_body_table_last",
                doc(vec![body(vec![t("a"), table(merged(body(vec![t("c")])))])]),
            ),
            (
                "g3_body_inline_media",
                doc(vec![body(vec![
                    t("a "),
                    image("BinData/p.png", None),
                    t(" b "),
                    ctrl(hwpforge_core::control::Control::hyperlink("t", "https://e.invalid/")),
                    t(" c"),
                    memo(vec![body(vec![t("m")])]),
                    t(" d"),
                ])]),
            ),
            ("g3_list_chart", doc(vec![p(BULLET, vec![chart(), t("- x")])])),
            ("g3_list_level2", doc(vec![p(BULLET2, vec![t("- x")])])),
            (
                "g3_list_table_then_text",
                doc(vec![p(BULLET, vec![t("a"), table(merged(body(vec![t("c")]))), t("\n- x")])]),
            ),
            (
                "g3_one_table_caption",
                doc(vec![body(vec![captioned(
                    one_cell(body(vec![t("c")])),
                    vec![body(vec![t("- x")])],
                )])]),
            ),
            (
                "g3_note_lines",
                doc(vec![body(vec![t("z"), footnote(vec![body(vec![t("- x\n1. y")])])])]),
            ),
            ("g3_memo", doc(vec![body(vec![t("z"), memo(vec![body(vec![t("- x")])])])])),
            ("g5_item", doc(vec![p(BULLET, amp())])),
            ("g5_note", doc(vec![body(vec![t("z"), footnote(vec![body(amp())])])])),
            (
                "g5_caption",
                doc(vec![body(vec![captioned(one_cell(body(vec![t("c")])), vec![body(amp())])])]),
            ),
            ("g5_item_cell", doc(vec![p(BULLET, vec![t("a"), table(one_cell(body(amp())))])])),
            (
                "g9_display",
                doc(vec![body(vec![t("a\n\n    b\n\n"), cross_ref(""), field(""), unknown("x")])]),
            ),
            ("g4_lossy", doc(vec![body(vec![t("a")]), body(vec![t(": b")])])),
            ("g10_heading", doc(vec![p(HEADING, vec![nested_bold()])])),
            ("g10_item", doc(vec![p(BULLET, vec![nested_bold()])])),
            ("g10_continuation", doc(vec![p(BULLET, vec![t("a")]), p(CONT0, vec![nested_bold()])])),
            ("g10_note", doc(vec![body(vec![t("z"), footnote(vec![body(vec![nested_bold()])])])])),
            ("g10_memo_body", doc(vec![body(vec![t("z"), memo(vec![body(vec![nested_bold()])])])])),
            (
                "g10_caption",
                doc(vec![body(vec![captioned(
                    one_cell(body(vec![t("c")])),
                    vec![body(vec![nested_bold()])],
                )])]),
            ),
            (
                "g10_image_caption",
                doc(vec![body(vec![image(
                    "BinData/p.png",
                    Some(vec![body(vec![nested_bold()])]),
                )])]),
            ),
            (
                "g11_cont_memo_list",
                doc(vec![
                    p(BULLET, vec![t("a")]),
                    p(CONT0, vec![memo(vec![body(vec![t("m")])]), t("- x")]),
                ]),
            ),
            (
                "g11_cont_chart_number",
                doc(vec![p(BULLET, vec![t("a")]), p(CONT0, vec![chart(), t("1. x")])]),
            ),
            (
                "g11_cont1_memo_list",
                doc(vec![
                    p(BULLET1, vec![t("a")]),
                    p(CONT1, vec![memo(vec![body(vec![t("m")])]), t("- x")]),
                ]),
            ),
            (
                "g12_table_memo",
                doc(vec![body(vec![
                    t("a"),
                    table(one_cell(body(vec![t("v")]))),
                    memo(vec![body(vec![t("m")])]),
                    t("*x*"),
                ])]),
            ),
            (
                "g12_caption_memo",
                doc(vec![body(vec![
                    t("a"),
                    captioned(
                        one_cell(body(vec![t("v")])),
                        vec![body(vec![t("cap"), memo(vec![body(vec![t("m")])])])],
                    ),
                    t("*x*"),
                ])]),
            ),
            (
                "g12_picture_caption_memo",
                doc(vec![body(vec![
                    t("a"),
                    image("BinData/p.png", Some(vec![body(vec![memo(vec![body(vec![t("m")])])])])),
                    t("*x*"),
                ])]),
            ),
            (
                "g12_text_box_memo",
                doc(vec![body(vec![
                    text_box(vec![body(vec![memo(vec![body(vec![t("m")])])])]),
                    t("*x*"),
                ])]),
            ),
            (
                "g12_comments_lf",
                doc(vec![body(vec![
                    memo(vec![body(vec![t("m")])]),
                    t("\n"),
                    memo(vec![body(vec![t("m")])]),
                    t("x"),
                ])]),
            ),
            (
                "g12_comments_space",
                doc(vec![body(vec![
                    memo(vec![body(vec![t("m")])]),
                    t(" "),
                    memo(vec![body(vec![t("m")])]),
                    t("x"),
                ])]),
            ),
            ("g10_html_cell", doc(vec![body(vec![table(merged(body(vec![nested_bold()])))])])),
        ]
    }
}
// ---- cases end ----

/// The base commit's output for each of [`cases::base_cases`]: (name,
/// styled, lossy). Written as the harness fixture is (see
/// `base_digests` in `tests/markup_safety.rs`): the `cases` module, copied
/// between its markers into `src/cases.rs` of the same scratch crate, with
/// a `main` that prints
/// `println!("    ({name:?}, {styled:?}, {lossy:?}),")` for each case
/// (`MdEncoder::encode_styled(&doc, &cases::Styles).markdown`,
/// `MdEncoder::encode_lossy(&doc).unwrap()`).
const BASE: &[(&str, &str, &str)] = &[
    // Written by the base commit (496371cf) from `cases::base_cases`.
    ("g0_auto", "- 항목처럼 보이는 글\n\n1. 번호처럼 보이는 글\n\n: 정의처럼 보이는 글\n\na | b\n\n--- | ---\n\n들여쓴 글", "항목처럼 보이는 글\n\n번호처럼 보이는 글\n\n: 정의처럼 보이는 글\n\na | b\n\n--- | ---\n\n    들여쓴 글"),
    ("g2_heading_split", "# 1. x", "1. x"),
    ("g2_continuation_after_empty_item", "a\n\n    b", "a\n\nb"),
    ("g3_item_blank_line", "- a\n\n    b", "a\n\n    b"),
    ("g3_number_continuation", "1. a\n\n  b", "a\n\nb"),
    ("g3_bullet_continuation", "- a\n\n  b", "a\n\nb"),
    ("g3_list_one_table", "| x |\n| --- |", "| x |\n| --- |"),
    ("g3_heading_one_image", "![p](images/p.png)", "![p](BinData/p.png)"),
    ("g3_note_paragraphs", "z[^1]\n\n[^1]: \\- x\n\n    \\1. y", "z(footnote: - x 1. y)"),
    ("g3_code", "```\n- x\n```", "- x"),
    ("g3_body_table_last", "a\n<table>\n<tbody>\n<tr>\n  <td colspan=\"2\">c</td>\n</tr>\n</tbody>\n</table>", "a\n\n| c |\n| --- |\n\n"),
    ("g3_body_inline_media", "a  ![p](images/p.png) b [t](https://e.invalid/) c<!-- memo: m --> d", "a  ![p](BinData/p.png) b [t](https://e.invalid/) c d"),
    ("g3_list_chart", "- <!-- chart -->- x", "- x"),
    ("g3_list_level2", "    - - x", "- x"),
    ("g3_list_table_then_text", "- a\n<table>\n<tbody>\n<tr>\n  <td colspan=\"2\">c</td>\n</tr>\n</tbody>\n</table>\n- x", "a\n\n| c |\n| --- |\n\n\n- x"),
    ("g3_one_table_caption", "| c |\n| --- |\n\n- x", "| c |\n| --- |\n\n\n\n- x"),
    ("g3_note_lines", "z[^1]\n\n[^1]: \\- x\n\\1. y", "z(footnote: - x\n1. y)"),
    ("g3_memo", "z<!-- memo: - x -->", "z"),
    ("g5_item", "- &amp;", "\\&amp;"),
    ("g5_note", "z[^1]\n\n[^1]: &amp;", "z(footnote: \\&amp;)"),
    ("g5_caption", "| c |\n| --- |\n\n&amp;", "| c |\n| --- |\n\n\n\n\\&amp;"),
    ("g5_item_cell", "- a\n| &amp; |\n| --- |", "a\n\n| \\&amp; |\n| --- |\n\n"),
    ("g9_display", "a\n\n    b\n\n\\[ref\\]\\_\\_\\_\\_`[x]`", "a\n\n    b\n\n`[x]`"),
    ("g4_lossy", "a\n\n: b", "a\n\n: b"),
    ("g10_heading", "# - x", ""),
    ("g10_item", "- - x", ""),
    ("g10_continuation", "- a\n\n  - x", "a"),
    ("g10_note", "z[^1]\n\n[^1]: \\- x", "z(footnote: )"),
    ("g10_memo_body", "z<!-- memo: - x -->", "z"),
    ("g10_caption", "| c |\n| --- |\n\n- x", "| c |\n| --- |"),
    ("g10_image_caption", "![p](images/p.png)\n\n- x", "![p](BinData/p.png)"),
    // g11: written by fa04fabf (G merged); continuations are drawn as at
    // 496371cf there.
    ("g11_cont_memo_list", "- a\n\n  <!-- memo: m -->- x", "a\n\n- x"),
    ("g11_cont_chart_number", "- a\n\n  <!-- chart -->1. x", "a\n\n1. x"),
    ("g11_cont1_memo_list", "  - a\n\n    <!-- memo: m -->- x", "a\n\n- x"),
    // g12: written by 61d3631d (G2, before this writer).
    ("g12_table_memo", "a\n| v |\n| --- |\n\n<!-- memo: m -->\n\\*x\\*", "a\n\n| v |\n| --- |\n\n*x*"),
    ("g12_caption_memo", "a\n| v |\n| --- |\n\ncap<!-- memo: m -->\\*x\\*", "a\n\n| v |\n| --- |\n\n\n\ncap\n\n*x*"),
    ("g12_picture_caption_memo", "a ![p](images/p.png)\n\n<!-- memo: m -->\n\\*x\\*", "a ![p](BinData/p.png)*x*"),
    ("g12_text_box_memo", "<!-- memo: m -->\n\\*x\\*", "*x*"),
    ("g12_comments_lf", "<!-- memo: m -->\n<!-- memo: m -->\nx", "\nx"),
    ("g12_comments_space", "<!-- memo: m -->\n<!-- memo: m -->\nx", " x"),
    ("g10_html_cell", "<table>\n<tbody>\n<tr>\n  <td colspan=\"2\">- x</td>\n</tr>\n</tbody>\n</table>", "|  |\n| --- |"),
];

fn base(name: &str) -> (&'static str, &'static str) {
    let (_, styled, lossy) = BASE.iter().find(|(n, ..)| *n == name).expect("base case");
    (styled, lossy)
}

fn case(name: &str) -> hwpforge_core::Document<hwpforge_core::Validated> {
    base_cases().into_iter().find(|(n, _)| *n == name).expect("case").1
}

/// Styled output and how many paragraphs were written as plain text.
fn styled(doc: &hwpforge_core::Document<hwpforge_core::Validated>) -> (String, usize) {
    let out = encode_styled(doc, &Styles);
    let n =
        out.warnings.iter().filter(|w| matches!(w, MdWarning::MarkupWrittenAsText { .. })).count();
    (out.markdown, n)
}

/// The block structure document text must not make, found by parsing as
/// the decoder does: each kind found, once per occurrence.
fn forbidden(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    for event in Parser::new_ext(markdown, md_options()) {
        let kind = match &event {
            Event::Start(
                tag @ (Tag::List(_)
                | Tag::Item
                | Tag::Heading { .. }
                | Tag::CodeBlock(_)
                | Tag::BlockQuote(_)
                | Tag::FootnoteDefinition(_)
                | Tag::DefinitionList
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::MetadataBlock(_)),
            ) => format!("{tag:?}"),
            Event::Rule => "Rule".into(),
            Event::TaskListMarker(_) => "TaskListMarker".into(),
            _ => continue,
        };
        out.push(kind);
    }
    out
}

/// How many tables and paragraphs the decoder's parser sees.
fn count(markdown: &str, which: fn(&Tag<'_>) -> bool) -> usize {
    Parser::new_ext(markdown, md_options())
        .filter(|e| matches!(e, Event::Start(tag) if which(tag)))
        .count()
}

/// What a reader sees: text, code, and HTML with its tags dropped and
/// entities decoded.
fn visible(markdown: &str) -> String {
    let mut out = String::new();
    for event in Parser::new_ext(markdown, md_options()) {
        match event {
            Event::Text(s) | Event::Code(s) => out.push_str(&s),
            Event::Html(s) | Event::InlineHtml(s) => {
                let mut rest = s.as_ref();
                while let Some(start) = rest.find('<') {
                    out.push_str(&unescape(&rest[..start]));
                    let end = if rest[start..].starts_with("<!--") {
                        rest[start..].find("-->").map(|e| start + e + 3)
                    } else {
                        rest[start..].find('>').map(|e| start + e + 1)
                    };
                    rest = &rest[end.unwrap_or(rest.len())..];
                }
                out.push_str(&unescape(rest));
            }
            _ => {}
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Without whitespace (`char::is_whitespace`): the comparison the
/// guarantee makes.
fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether the decoder reads the output back. Only asked where the output
/// holds no HTML: the decoder refuses all HTML but the section marker.
fn decodes(markdown: &str) -> Option<bool> {
    let html = Parser::new_ext(markdown, md_options())
        .any(|e| matches!(e, Event::Html(_) | Event::InlineHtml(_)));
    (!html).then(|| crate::MdDecoder::decode_with_default(markdown).is_ok())
}

// ---------------------------------------------------------------------------
// G-0: the fixture's shape
// ---------------------------------------------------------------------------

/// The literal fixture is eight Body paragraphs: seven paragraphs come
/// back, each its own text, and the decoder reads it. The auto-formatted
/// one keeps its list blocks as they were; its Body paragraphs differ only
/// by the rules' `\`.
/// 이것을 실패시키는 것: 규칙 행 `-`·숫자·`:`·`=` 하나씩 삭제.
#[test]
fn fixture_lookalike_stays_text() {
    let (out, fallbacks) = styled(&lookalike(false));
    assert_eq!(
        out,
        "\\- 항목처럼 보이는 글\n\n1\\. 번호처럼 보이는 글\n\n\\: 정의처럼 보이는 글\n\n\\===\n\na | b\n\n\\--- | ---\n\n들여쓴 글"
    );
    assert_eq!(fallbacks, 0);
    assert!(forbidden(&out).is_empty(), "{out}");
    assert_eq!(count(&out, |t| matches!(t, Tag::Paragraph)), 7);
    assert_eq!(count(&out, |t| matches!(t, Tag::Table(_))), 0);
    assert_eq!(decodes(&out), Some(true));
    assert_eq!(
        squeeze(&visible(&out)),
        squeeze("- 항목처럼 보이는 글1. 번호처럼 보이는 글: 정의처럼 보이는 글===a | b--- | ---들여쓴 글")
    );

    let (out, fallbacks) = styled(&lookalike(true));
    let (base, _) = base("g0_auto");
    assert_eq!(fallbacks, 0);
    assert_eq!(out, base.replace("\n: 정의", "\n\\: 정의").replace("\n--- |", "\n\\--- |"));
    assert!(out.starts_with("- 항목처럼 보이는 글\n\n1. 번호처럼 보이는 글\n\n"), "{out}");
}

// ---------------------------------------------------------------------------
// G-1: the rules close every line start
// ---------------------------------------------------------------------------

/// Text that would open a block at a line start.
const MARKERS: &[&str] = &[
    "- x",
    "-",
    "--",
    "-:",
    "-|",
    "+ x",
    "+",
    "1. x",
    "1) x",
    "123456789.",
    "a\n===",
    "a\n=",
    "=",
    "---",
    "- - -",
    "a | b\n--- | ---",
    "|---|",
    "| - |",
    "|:-",
    "a | b\n::--- | ---",
    "a\n: b",
    "a\n:b",
    "::x",
    ":-",
    ":",
    ": ",
];

/// Whitespace before the marker: none, ASCII, a tab, Unicode.
const LEADS: &[&str] = &["", " ", "    ", "\t", "\u{A0}", "\u{3000}"];

/// Where the marker is put in a paragraph's runs.
fn carriers(text: &str) -> Vec<Vec<hwpforge_core::Run>> {
    vec![
        vec![t(text)],
        vec![styled_t(text, BOLD)],
        vec![styled_t(&format!("a{text}b"), BOLD)],
        vec![styled_t(text, UNDERLINE)],
        vec![t("z"), compose(text)],
        vec![ctrl(hwpforge_core::control::Control::dutmal(text, "s"))],
        vec![ctrl(hwpforge_core::control::Control::dutmal("m", text))],
        vec![cross_ref(text)],
        vec![field(text)],
        vec![ctrl(hwpforge_core::control::Control::hyperlink(text, "javascript:x"))],
    ]
}

/// Where the paragraph is put: Body, a text box in Body, a GFM cell in
/// Body, a one-table paragraph's GFM cell, an HTML cell. The tables the
/// encoder made, by kind.
fn containers(
    runs: Vec<hwpforge_core::Run>,
) -> Vec<(hwpforge_core::Document<hwpforge_core::Validated>, usize)> {
    vec![
        (doc(vec![body(runs.clone())]), 0),
        (doc(vec![body(vec![text_box(vec![body(runs.clone())]), t("z")])]), 0),
        (doc(vec![body(vec![t("z"), table(one_cell(body(runs.clone())))])]), 1),
        (doc(vec![body(vec![table(one_cell(body(runs.clone())))])]), 1),
        (doc(vec![body(vec![table(merged(body(runs)))])]), 0),
    ]
}

/// Every marker, after every lead, at a paragraph's start, after a line
/// ending in the same piece and at a piece's start after a line ending,
/// in every carrier and container: no block, the encoder's tables only,
/// the same text, nothing written as plain text; the decoder reads what
/// holds no HTML.
/// 이것을 실패시키는 것: 규칙 행 삭제 · `:` 를 4판 조건으로(`::x`·`:-`) · 공백 집합을
/// ASCII 로(NBSP·전각) · 끝을 공백으로 보지 않기(`-`·`+`·`=`·`123456789.`) ·
/// 운반체 하나가 `text(raw, ctx)` 를 거치지 않기.
#[test]
fn guarded_rules_close_line_starts() {
    let mut problems = Vec::new();
    for marker in MARKERS {
        for lead in LEADS {
            let at_start = format!("{lead}{marker}");
            let after_line = format!("y\n{lead}{marker}");
            let mut placements: Vec<Vec<hwpforge_core::Run>> = Vec::new();
            for text in [&at_start, &after_line] {
                placements.extend(carriers(text));
            }
            for carrier in carriers(&at_start) {
                let mut runs = vec![t("y\n")];
                runs.extend(carrier);
                placements.push(runs);
            }
            for runs in placements {
                for (doc, tables) in containers(runs.clone()) {
                    let (out, fallbacks) = styled(&doc);
                    let case = format!("{runs:?} → {out:?}");
                    if fallbacks != 0 {
                        problems.push(format!("written as plain text: {case}"));
                    }
                    let found = forbidden(&out);
                    if !found.is_empty() {
                        problems.push(format!("{found:?}: {case}"));
                    }
                    if count(&out, |t| matches!(t, Tag::Table(_))) != tables {
                        problems.push(format!("tables: {case}"));
                    }
                    if !squeeze(&visible(&out)).contains(&squeeze(marker)) {
                        problems.push(format!("text: {case}"));
                    }
                    if decodes(&out) == Some(false) {
                        problems.push(format!("from-md: {case}"));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems[..problems.len().min(40)].join("\n")
    );
}

/// `Md::guarded_text` against each row of its table, the end of the text
/// as a space, and the entity rule.
/// 이것을 실패시키는 것: 규칙 행 하나씩 삭제 · `:` 조건부 · `\|` 뒤 `-`·`:` 대신 `\|`
/// 이스케이프 · 숫자 10자리 허용 · `&` 규칙 삭제 · `&` 를 엔티티 앞에서 두 번 이스케이프.
#[test]
fn guarded_text_rules() {
    for (raw, want) in [
        ("- x", "\\- x"),
        ("-", "\\-"),
        ("-x", "-x"),
        ("--", "\\--"),
        ("-:", "\\-:"),
        ("+ x", "\\+ x"),
        ("+x", "+x"),
        ("=", "\\="),
        ("==", "\\=="),
        ("=x", "=x"),
        (":x", "\\:x"),
        ("|-", "|\\-"),
        ("| |:", "| |\\:"),
        ("|x-", "|x-"),
        ("1.", "1\\."),
        ("1) x", "1\\) x"),
        ("1.x", "1.x"),
        ("123456789. x", "123456789\\. x"),
        ("1234567890. x", "1234567890. x"),
        ("a\n\u{3000}- x", "a\n\u{3000}\\- x"),
        ("a\n\n\t:b", "a\n\n\t\\:b"),
        ("a - b", "a - b"),
        ("a &", "a \\&"),
        ("a &amp", "a \\&amp"),
        ("a &#1", "a \\&\\#1"),
        ("a &amp;", "a \\&amp;"),
        ("a & b", "a & b"),
        ("# x", "\\# x"),
    ] {
        assert_eq!(Md::guarded_text(raw).as_str(), want, "{raw:?}");
    }
    // Outside the guarantee, text is as before.
    assert_eq!(Md::text("- x\n: y &").as_str(), "- x\n: y &");
}

/// Each place styled writes document text that can start a line uses the
/// paragraph's context: a caption number and an equation's script too.
/// 이것을 실패시키는 것: 캡션 번호·`math_free_text`·허용되지 않은 링크 글이
/// `text(raw, ctx)` 를 거치지 않기.
#[test]
fn guarded_text_reaches_every_carrier() {
    use hwpforge_core::control::Control;
    for (runs, want) in [
        (vec![auto_number('.'), t(" x")], "1\\. x"),
        (vec![ctrl(Control::hyperlink("- x", "javascript:x"))], "\\- x"),
        (vec![ctrl(Control::equation("- x](y"))], "\\- x\\](y"),
    ] {
        let (out, fallbacks) = styled(&doc(vec![body(runs.clone())]));
        assert_eq!(fallbacks, 0, "{runs:?}");
        assert!(forbidden(&out).is_empty(), "{runs:?}: {out}");
        assert!(out.starts_with(want), "{runs:?}: {out}");
    }
}

// ---------------------------------------------------------------------------
// G-2: the check and the construction close the rest
// ---------------------------------------------------------------------------

/// Pieces the rules cannot see together: a number split from its `.`, a
/// blank line then an indent, text after a comment at a line start —
/// written as plain text with one warning. A table the encoder made ends
/// its block, through whitespace, an empty paragraph and a text box's trim.
/// Each case in Body and in a text box in Body; a heading writes the same
/// split as one heading, as before.
/// 이것을 실패시키는 것: Body 단계 삭제 · 금지 목록 항목(List·CodeBlock) 삭제 ·
/// 주석 꼬리 검사 삭제 · `ending_block` 삭제 · 공백 push 에서 표시 끄기 ·
/// `layout` 에서 표시 버리기.
#[test]
fn body_check_and_table_end_close_the_rest() {
    let html_table = || table(merged(body(vec![t("c")])));
    let html = "<table>\n<tbody>\n<tr>\n  <td colspan=\"2\">c</td>\n</tr>\n</tbody>\n</table>";
    let gfm = || table(one_cell(body(vec![t("c")])));
    let cases: Vec<(Vec<hwpforge_core::Run>, usize, String)> = vec![
        (vec![t("1"), compose(". x")], 1, "1\\. x".into()),
        (vec![t("1"), compose("2. x")], 0, "12\\. x".into()),
        (vec![t("1."), compose(" x")], 0, "1\\. x".into()),
        (vec![t("a\n\n    b")], 1, "a      b".into()),
        (vec![t("a\n\n\tb")], 1, "a  \tb".into()),
        (vec![t("a\n\n  "), compose("  b")], 1, "a      b".into()),
        (vec![t("a\n\n   b")], 0, "a\n\n   b".into()),
        (vec![t("a | b\n::--- | ---")], 0, "a | b\n\\::--- | ---".into()),
        (vec![t("a\n:b")], 0, "a\n\\:b".into()),
        (vec![html_table(), t("\n- x")], 0, format!("{html}\n\n\\- x")),
        (vec![html_table(), t("     ※ x")], 0, format!("{html}\n\n※ x")),
        (vec![html_table(), t("\n\n    b")], 0, format!("{html}\n\nb")),
        (vec![html_table(), t(" "), compose("- x")], 0, format!("{html} \n\n\\- x")),
        (
            vec![text_box(vec![body(vec![html_table(), t(" ")])]), t("\n- x")],
            0,
            format!("{html}\n\n\\- x"),
        ),
        (
            vec![text_box(vec![body(vec![html_table()]), body(vec![])]), t("- x")],
            0,
            format!("{html}\n\n\\- x"),
        ),
        (vec![gfm(), t("z | y")], 0, "| c |\n| --- |\n\nz | y".into()),
        (vec![text_box(vec![body(vec![gfm()])]), t("z")], 0, "| c |\n| --- |\n\nz".into()),
        (vec![memo(vec![body(vec![t("m")])]), t("x")], 0, "<!-- memo: m -->\nx".into()),
        (vec![chart(), t("- x")], 0, "<!-- chart -->\n\\- x".into()),
        (vec![chart(), t("   x")], 0, "<!-- chart -->\nx".into()),
        (vec![t("a\n"), chart(), t("- x")], 0, "a\n<!-- chart -->\n\\- x".into()),
        (
            vec![chart(), memo(vec![body(vec![t("m")])]), t("x")],
            0,
            "<!-- chart -->\n<!-- memo: m -->\nx".into(),
        ),
    ];
    for (runs, warnings, want) in cases {
        for (doc, boxed) in [
            (doc(vec![body(runs.clone())]), false),
            (doc(vec![body(vec![text_box(vec![body(runs.clone())])])]), true),
        ] {
            let (out, fallbacks) = styled(&doc);
            assert_eq!(fallbacks, warnings, "{runs:?} (box {boxed}): {out:?}");
            assert_eq!(out, want, "{runs:?} (box {boxed})");
            assert!(forbidden(&out).is_empty(), "{runs:?}: {out:?}");
        }
    }
    // `z | y` after the GFM table is a paragraph: its characters survive.
    let (out, _) = styled(&doc(vec![body(vec![gfm(), t("z | y")])]));
    assert!(visible(&out).ends_with("z | y"), "{out}");
    // A heading is out of the guarantee: one heading, as before.
    let (out, fallbacks) = styled(&case("g2_heading_split"));
    assert_eq!((out.as_str(), fallbacks), (base("g2_heading_split").0, 0));
    assert_eq!(out, "# 1. x");
}

/// A continuation is its own block (not appended to the block before):
/// a Body paragraph before it is checked alone, and a continuation after a
/// page break falls back alone, its list kept.
/// 이것을 실패시키는 것: continuation 을 직전 블록에 붙이기 · continuation 을
/// Body 로 · 출처 없는 블록에 붙이기.
#[test]
fn continuation_is_its_own_block() {
    // Body `a`, an empty item, its continuation `b`: base output, no warning.
    let (out, fallbacks) = styled(&case("g2_continuation_after_empty_item"));
    assert_eq!((out.as_str(), fallbacks), (base("g2_continuation_after_empty_item").0, 0));
    assert_eq!(out, "a\n\n    b");
    // A page-broken empty item, then a continuation holding `a`, `:b`: the
    // continuation alone falls back, the definition list is gone.
    let mut item = cases::p(BULLET, vec![t("")]);
    item.page_break = true;
    let (out, fallbacks) = styled(&doc(vec![item, cases::p(CONT0, vec![t("a\n\n:b")])]));
    assert_eq!((out.as_str(), fallbacks), ("---\n\na  \\:b", 1));
    // An item, then a continuation holding `x`, `:y`: only the continuation
    // falls back; the item keeps its marker.
    let (out, fallbacks) =
        styled(&doc(vec![cases::p(BULLET, vec![t("a")]), cases::p(CONT0, vec![t("x\n\n:y")])]));
    assert_eq!((out.as_str(), fallbacks), ("- a\n\nx  \\:y", 1));
}

// ---------------------------------------------------------------------------
// G-3: nothing outside the guarantee changes
// ---------------------------------------------------------------------------

/// Lists, headings, continuations, notes, code, memos, one-table captions,
/// and a list paragraph's table: the base output, no warning. Body text
/// with pictures, links and memo comments mid-line, and a table last: the
/// base output too (no rule fires).
/// 이것을 실패시키는 것: Body 검사를 모든 문단 블록에 · `body` 를 분류로 추정 ·
/// 표 앞에도 빈 줄 · `guarded` 무시하고 규칙을 어디서나 · continuation·캡션·각주·메모에
/// 보장 문맥 물려주기 · 범위 밖 문단 안 표에 `ending_block`.
#[test]
fn out_of_scope_is_base_output() {
    for name in [
        "g3_item_blank_line",
        "g3_number_continuation",
        "g3_bullet_continuation",
        "g3_list_one_table",
        "g3_heading_one_image",
        "g3_note_paragraphs",
        "g3_code",
        "g3_body_table_last",
        "g3_body_inline_media",
        "g3_list_chart",
        "g3_list_level2",
        "g3_list_table_then_text",
        "g3_one_table_caption",
        "g3_note_lines",
        "g3_memo",
    ] {
        let (out, fallbacks) = styled(&case(name));
        assert_eq!(out, base(name).0, "{name}");
        assert_eq!(fallbacks, 0, "{name}");
    }
    // The two places a classified-shape check would write as plain text.
    assert_eq!(base("g3_list_chart").0, "- <!-- chart -->- x");
    assert_eq!(base("g3_list_level2").0, "    - - x");
}

// ---------------------------------------------------------------------------
// G-4: no definition list reaches the decoder
// ---------------------------------------------------------------------------

/// (a) The rules: `:` at a line start in Body. (b) The whole-output check
/// on its own: two Body blocks `a`, `: b`. (c) Lossy keeps its check. (d)
/// A continuation after a page break. (e) A one-table caption holding
/// `x`, `:y`. (f) An empty-path picture's caption `:b` after Body `a`: every
/// block falls back, the heading too.
/// 이것을 실패시키는 것: `:` 규칙 삭제(a) · `decoder_structure` 삭제(b·e·f) · lossy 에
/// 적용(c) · continuation 을 출처 없는 블록에 붙이기(d).
#[test]
fn definition_lists_never_reach_the_decoder() {
    let no_list = |out: &str| !forbidden(out).iter().any(|k| k.starts_with("DefinitionList"));
    // (a)
    for paragraphs in [
        vec![body(vec![t("a\n: b")])],
        vec![body(vec![t("a\n:b")])],
        vec![body(vec![t("a\n::b")])],
        vec![body(vec![t("a")]), body(vec![t(": b")])],
        vec![body(vec![t("a")]), body(vec![t(":b")])],
        vec![cases::p(BULLET, vec![t("a")]), body(vec![t(":b")])],
    ] {
        let (out, fallbacks) = styled(&doc(paragraphs));
        assert_eq!(fallbacks, 0, "{out}");
        assert!(no_list(&out), "{out}");
        assert_eq!(decodes(&out), Some(true), "{out}");
    }
    // (b)
    let block = |md: &str, at| Block {
        markdown: md.into(),
        source: Some(Source {
            prefix: String::new(),
            texts: vec![super::guard::plain_text(md)],
            at: vec![at],
        }),
        body: true,
    };
    let dests = std::collections::HashSet::new();
    let allowed =
        Allowed { tags: super::guard::STYLED_TAGS, destinations: &dests, decoder_structure: true };
    let mut warnings = Vec::new();
    let out = checked(vec![block("a", (0, 0)), block(": b", (0, 1))], &allowed, &mut warnings);
    assert!(no_list(&out), "{out}");
    assert_eq!(decodes(&out), Some(true));
    assert_eq!(warnings.len(), 2);
    // (c)
    let doc_c = case("g4_lossy");
    let (lossy, warnings) = crate::MdEncoder::encode_lossy_with_report(&doc_c).unwrap();
    assert_eq!(lossy, base("g4_lossy").1);
    assert!(!warnings.iter().any(|w| matches!(w, MdWarning::MarkupWrittenAsText { .. })));
    // (d): `continuation_is_its_own_block`. (e)
    let caption = vec![body(vec![t("x")]), body(vec![t(":y")])];
    let (out, fallbacks) =
        styled(&doc(vec![body(vec![captioned(one_cell(body(vec![t("c")])), caption)])]));
    assert_eq!((out.as_str(), fallbacks), ("c x \\:y", 1));
    // (f)
    let (out, fallbacks) = styled(&doc(vec![
        cases::p(HEADING, vec![t("h")]),
        body(vec![t("a")]),
        body(vec![image("", Some(vec![body(vec![t(":b")])]))]),
    ]));
    assert_eq!((out.as_str(), fallbacks), ("h\n\na\n\n\\:b", 3));
    assert!(no_list(&out));
    assert_eq!(decodes(&out), Some(true));
}

// ---------------------------------------------------------------------------
// G-5: an entity split across pieces
// ---------------------------------------------------------------------------

/// `&` then `amp;` in the next piece: guarded text keeps `&amp;` as typed
/// (Body, a text box in Body, a GFM cell in Body, a one-table paragraph's
/// cell); outside, the base output.
/// 이것을 실패시키는 것: `&` 규칙 삭제 · 단일 표 칸에 보장 문맥을 주지 않기 · 범위
/// 밖에도 적용.
#[test]
fn entity_split_is_closed_where_guarded() {
    let amp = || body(vec![t("&"), compose("amp;")]);
    for doc in [
        doc(vec![amp()]),
        doc(vec![body(vec![text_box(vec![amp()]), t("z")])]),
        doc(vec![body(vec![t("z"), table(one_cell(amp()))])]),
        doc(vec![body(vec![table(one_cell(amp()))])]),
    ] {
        let (out, fallbacks) = styled(&doc);
        assert_eq!(fallbacks, 0);
        assert!(visible(&out).contains("&amp;"), "{out}");
    }
    for name in ["g5_item", "g5_note", "g5_caption", "g5_item_cell"] {
        assert_eq!(styled(&case(name)).0, base(name).0, "{name}");
    }
}

// ---------------------------------------------------------------------------
// Memo anchors (review 1, HIGH): a memo inside another memo's anchor
// ---------------------------------------------------------------------------

/// A memo left standing in another memo's anchor is drawn the way an
/// expanded anchor is: its text guarded (a list marker escaped, an entity
/// split across pieces kept), and a Body paragraph that falls back for
/// another reason keeps the anchor's characters.
/// 이것을 실패시키는 것: 앵커 가드 우회(첫째·둘째) · 대체 글의 앵커 순회 삭제(셋째).
#[test]
fn memo_anchor_follows_the_renderer() {
    let nested = |anchor: Vec<hwpforge_core::Run>| anchored_memo(vec![anchored_memo(anchor)]);
    let (out, fallbacks) = styled(&doc(vec![body(vec![nested(vec![t("- x")])])]));
    assert_eq!((out.as_str(), fallbacks), ("\\- x", 0));
    let (out, fallbacks) = styled(&doc(vec![body(vec![nested(vec![t("&")]), compose("amp;")])]));
    assert_eq!(fallbacks, 0);
    assert!(visible(&out).contains("&amp;"), "{out}");
    let (out, fallbacks) =
        styled(&doc(vec![body(vec![t("a\n\n    b\n\n"), nested(vec![compose("keep")])])]));
    assert_eq!(fallbacks, 1);
    assert!(out.contains("keep"), "{out}");
}

/// A memo in another memo's anchor outside the guarantee (a heading, a
/// list item, a continuation, a note, a memo body, a one-table or
/// one-picture caption) and in an HTML cell: its anchor's text as before,
/// byte for byte.
/// 이것을 실패시키는 것: 범위 밖 앵커를 문단 렌더러로 · HTML 앵커를 문단 렌더러로 ·
/// 그림 캡션에만 `guarded=true` 전달.
#[test]
fn memo_anchor_out_of_scope_is_base_output() {
    for name in [
        "g10_heading",
        "g10_item",
        "g10_continuation",
        "g10_note",
        "g10_memo_body",
        "g10_caption",
        "g10_image_caption",
        "g10_html_cell",
    ] {
        assert_eq!(styled(&case(name)), (base(name).0.to_string(), 0), "{name}");
    }
}

/// A memo at a paragraph's start (as Hancom saves it, `sample-memo-basic`):
/// its comment stays, the text after it reads as written, and the memo's
/// body does not mix into the text. Also with the memo body over two
/// lines.
/// 이것을 실패시키는 것: 줄머리 주석의 줄 끝내기 삭제 · 뒤 글의 앞 공백 남기기.
#[test]
fn line_start_memo_keeps_comment_and_text() {
    use hwpforge_core::control::Control;
    for (memo_body, comment) in [
        ("쇠부리야 여기가 메모야", "<!-- memo: 쇠부리야 여기가 메모야 -->"),
        ("첫 줄\n둘째 줄", "<!-- memo: 첫 줄\n둘째 줄 -->"),
    ] {
        let memo = ctrl(Control::memo_with_anchor(
            vec![body(vec![t(memo_body)])],
            vec![t("메모 대상 문장")],
        ));
        let (out, fallbacks) = styled(&doc(vec![body(vec![memo, t(" 입니다.")])]));
        assert_eq!(fallbacks, 0, "{out}");
        assert_eq!(out, format!("{comment}\n메모 대상 문장 입니다."));
        assert_eq!(out.matches("<!-- memo:").count(), 1);
        assert!(visible(&out).ends_with("메모 대상 문장 입니다."), "{out}");
        assert!(forbidden(&out).is_empty(), "{out}");
    }
}

/// A memo or chart comment at a continuation's start, then text that looks
/// like a list item: a continuation is out of the guarantee, so its bytes
/// are the base's (no line ended after the comment).
/// 이것을 실패시키는 것: 줄 끝내기 조건을 `ParaKind::Body` 만으로.
#[test]
fn continuation_line_start_comment_is_base_output() {
    for name in ["g11_cont_memo_list", "g11_cont_chart_number", "g11_cont1_memo_list"] {
        assert_eq!(styled(&case(name)), (base(name).0.to_string(), 0), "{name}");
    }
}

/// Many comments on one line, then text: each comment ends its line, and
/// the walk does not scan the rest of the line again for each one.
/// 이것을 실패시키는 것: 꼬리를 줄 끝까지 다시 훑기.
#[test]
fn line_start_comments_are_linear() {
    let mut runs: Vec<hwpforge_core::Run> = (0..100_000).map(|_| chart()).collect();
    runs.push(t("x"));
    let doc = doc(vec![body(runs)]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, 0);
    assert_eq!(out, format!("{}x", "<!-- chart -->\n".repeat(100_000)));
}

/// Each character of the output with the emphasis it is read with
/// (bold, italic, strikeout), whitespace left out.
fn emphasized(markdown: &str) -> Vec<(char, bool, bool, bool)> {
    let (mut b, mut i, mut st) = (0, 0, 0);
    let mut out = Vec::new();
    for event in Parser::new_ext(markdown, md_options()) {
        match event {
            Event::Start(Tag::Strong) => b += 1,
            Event::End(pulldown_cmark::TagEnd::Strong) => b -= 1,
            Event::Start(Tag::Emphasis) => i += 1,
            Event::End(pulldown_cmark::TagEnd::Emphasis) => i -= 1,
            Event::Start(Tag::Strikethrough) => st += 1,
            Event::End(pulldown_cmark::TagEnd::Strikethrough) => st -= 1,
            Event::Text(t) => out.extend(
                t.chars().filter(|c| !c.is_whitespace()).map(|c| (c, b > 0, i > 0, st > 0)),
            ),
            _ => {}
        }
    }
    out
}

/// Two formatted pieces meeting with nothing drawn between them — an
/// empty control, an empty text run, a picture without a path, a bookmark,
/// an empty text box — or through a text box's text, in every pair of
/// emphasis: where text is guarded each character is read with its own
/// emphasis, the output holds no HTML and the decoder reads it. Outside the
/// guarantee, as before. The first case is the generator's failing case of
/// 2026-10-25, kept so it does not depend on the seed.
/// 이것을 실패시키는 것: 병합 삭제 · 글상자 경계에서 병합하지 않기 · `guarded` 제한 삭제.
#[test]
fn adjacent_formatted_text_keeps_its_characters() {
    use hwpforge_core::control::Control;
    let (out, fallbacks) =
        styled(&doc(vec![body(vec![styled_t("a", BOLD), compose(""), styled_t("1", BOLD)])]));
    assert_eq!((out.as_str(), fallbacks), ("**a1**", 0));
    let shapes: Vec<(usize, (bool, bool, bool))> = (0..8)
        .map(|k| (k & 1 == 1, k & 2 == 2, k & 4 == 4))
        .filter(|f| *f != (false, false, false))
        .map(|f| (emphasis(f.0, f.1, f.2), f))
        .collect();
    let gaps = || {
        vec![
            vec![],
            vec![compose("")],
            vec![t("")],
            vec![image("", None)],
            vec![ctrl(Control::bookmark("b"))],
            vec![text_box(vec![body(vec![t("")])])],
        ]
    };
    let mut problems = Vec::new();
    for &(left, lf) in &shapes {
        for &(right, rf) in &shapes {
            let boxed = |s: &str, shape| text_box(vec![body(vec![styled_t(s, shape)])]);
            let mut placements: Vec<Vec<hwpforge_core::Run>> = gaps()
                .into_iter()
                .map(|gap| {
                    let mut runs = vec![styled_t("a", left)];
                    runs.extend(gap);
                    runs.push(styled_t("1", right));
                    runs
                })
                .collect();
            placements.push(vec![styled_t("a", left), boxed("1", right)]);
            placements.push(vec![boxed("a", left), styled_t("1", right)]);
            placements.push(vec![boxed("a", left), boxed("1", right)]);
            for runs in placements {
                let (out, fallbacks) = styled(&doc(vec![body(runs.clone())]));
                let want = vec![('a', lf.0, lf.1, lf.2), ('1', rf.0, rf.1, rf.2)];
                let html = Parser::new_ext(&out, md_options())
                    .any(|e| matches!(e, Event::Html(_) | Event::InlineHtml(_)));
                if fallbacks != 0
                    || html
                    || emphasized(&out) != want
                    || crate::MdDecoder::decode_with_default(&out).is_err()
                {
                    problems.push(format!("{lf:?} {rf:?} {runs:?} → {out:?}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}:\n{}", problems.len(), problems.join("\n"));
    // A `*` of the document's text before italic text is text, not a
    // marker to merge.
    let italic = emphasis(false, true, false);
    let (out, _) = styled(&doc(vec![body(vec![t("a*"), styled_t("1", italic)])]));
    assert_eq!(
        emphasized(&out),
        [('a', false, false, false), ('*', false, false, false), ('1', false, true, false)],
        "{out}"
    );
    // Outside the guarantee, as before.
    let runs = vec![styled_t("a", BOLD), compose(""), styled_t("1", BOLD)];
    assert_eq!(styled(&doc(vec![cases::p(BULLET, runs)])).0, "- **a****1**");
}

/// Whether emphasis over three pieces nests: each emphasis covers a run of
/// neighbouring pieces, and two such runs are nested or apart.
fn nests(pieces: &[(bool, bool, bool)]) -> bool {
    let span = |f: fn(&(bool, bool, bool)) -> bool| {
        let on: Vec<usize> = (0..pieces.len()).filter(|&i| f(&pieces[i])).collect();
        match (on.first(), on.last()) {
            (Some(&a), Some(&b)) if b - a + 1 == on.len() => Some(Some((a, b))),
            (None, None) => Some(None),
            _ => None,
        }
    };
    let spans = [span(|p| p.0), span(|p| p.1), span(|p| p.2)];
    if spans.iter().any(Option::is_none) {
        return false;
    }
    let spans: Vec<(usize, usize)> = spans.into_iter().flatten().flatten().collect();
    spans.iter().all(|&(a, b)| {
        spans.iter().all(|&(c, d)| b < c || d < a || (a <= c && d <= b) || (c <= a && b <= d))
    })
}

/// Three pieces `a`, `b`, `c` in every emphasis (7³), next to each other
/// and with the last two in text boxes: where the emphasis nests, every
/// character is read with its own emphasis, with no HTML and no warning,
/// and the decoder reads it; where it crosses, the same, or the paragraph
/// is written as plain text with one warning and every character kept. The
/// review's four failing cases are among them.
/// 이것을 실패시키는 것: 구성 되돌리기(조각마다 표지) · 평문 대체 끄기 · 빈 조각 건너뛰기 삭제.
#[test]
fn three_emphasized_pieces_keep_their_emphasis() {
    let kinds: Vec<(bool, bool, bool)> =
        (1..8).map(|k| (k & 1 == 1, k & 2 == 2, k & 4 == 4)).collect();
    let shape = |f: (bool, bool, bool)| emphasis(f.0, f.1, f.2);
    let mut problems = Vec::new();
    let mut fell_back = 0;
    for &x in &kinds {
        for &y in &kinds {
            for &z in &kinds {
                let nested = nests(&[x, y, z]);
                let boxed = |s: &str, f| text_box(vec![body(vec![styled_t(s, shape(f))])]);
                for runs in [
                    vec![styled_t("a", shape(x)), styled_t("b", shape(y)), styled_t("c", shape(z))],
                    vec![
                        styled_t("a", shape(x)),
                        compose(""),
                        styled_t("b", shape(y)),
                        styled_t("c", shape(z)),
                    ],
                    vec![styled_t("a", shape(x)), boxed("b", y), boxed("c", z)],
                ] {
                    let (out, fallbacks) = styled(&doc(vec![body(runs.clone())]));
                    let want =
                        vec![('a', x.0, x.1, x.2), ('b', y.0, y.1, y.2), ('c', z.0, z.1, z.2)];
                    let html = Parser::new_ext(&out, md_options())
                        .any(|e| matches!(e, Event::Html(_) | Event::InlineHtml(_)));
                    let exact = fallbacks == 0
                        && !html
                        && emphasized(&out) == want
                        && crate::MdDecoder::decode_with_default(&out).is_ok();
                    let plain = fallbacks == 1 && squeeze(&visible(&out)) == "abc";
                    if plain {
                        fell_back += 1;
                    }
                    if !(exact || (!nested && plain)) {
                        problems.push(format!(
                            "{x:?} {y:?} {z:?} nested={nested} → {out:?} ({fallbacks})"
                        ));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}:\n{}", problems.len(), problems.join("\n"));
    // The review's four cases (bold → bold italic → italic and back, with
    // and without strikeout over all three) are crossing. Some crossing
    // placements this writer does not handle and writes as plain text
    // (with a warning, every character kept, as checked above); a better
    // writer may handle more of them, never more of them fall back.
    // 15 of 1029 now (24 before italic as `_` was tried).
    assert!(fell_back <= 15, "{fell_back}");
}

/// A guarded cell whose emphasis the writer cannot place (bold `a` → bold
/// italic `b` → italic `c`, a crossing it does not handle) is written as
/// plain text in that cell alone, with one warning: the table stays a
/// table, and the one-table paragraph's caption (out of the guarantee) is
/// as before, `**CAP**`. In a Body paragraph the same text falls back in
/// that paragraph only, the paragraph after it as it is.
/// 이것을 실패시키는 것: 대체를 블록 전체로 넓히기.
#[test]
fn emphasis_fallback_stays_in_its_paragraph() {
    let crossing = || {
        vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ]
    };
    let caption = vec![body(vec![styled_t("CAP", BOLD)])];
    let (out, fallbacks) =
        styled(&doc(vec![body(vec![captioned(one_cell(body(crossing())), caption)])]));
    assert_eq!(fallbacks, 1, "{out}");
    assert_eq!(out, "| abc |\n| --- |\n\n**CAP**");
    assert_eq!(count(&out, |t| matches!(t, Tag::Table(_))), 1);
    let (out, fallbacks) = styled(&doc(vec![body(crossing()), body(vec![styled_t("next", BOLD)])]));
    assert_eq!((out.as_str(), fallbacks), ("abc\n\n**next**", 1));
}

/// A caption's final punctuation meets the text outside it, with and
/// without another outer emphasis to force the parent's check. The same
/// check includes pictures, nested captions, shapes and memo anchors.
/// 이것을 실패시키는 것: 캡션 독립 검사 · 자식 범위 누락 · 표 뒤 trim 위치 미보정.
#[test]
fn captions_are_checked_in_the_final_body() {
    // Every placement: Unicode punctuation closing the caption's emphasis
    // right before `b` cannot be written with markers (the closer would not
    // be right-flanking), so each is plain text with its one warning.
    let mut want = [
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b", 1),
        ("a。b", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
        ("h a。b z", 1),
        ("a。b z", 1),
    ]
    .into_iter();
    for shape in [BOLD, emphasis(false, true, false)] {
        for suffix in [false, true] {
            for nesting in 0..4 {
                let run = styled_t("a。", shape);
                let cap = body(vec![match nesting {
                    0 => run,
                    1 => text_box(vec![body(vec![run])]),
                    2 => anchored_memo(vec![anchored_memo(vec![run])]),
                    _ => image("", Some(vec![body(vec![run])])),
                }]);
                for picture in [false, true] {
                    let first = if picture {
                        image("", Some(vec![cap.clone()]))
                    } else {
                        captioned(one_cell(body(vec![t("h")])), vec![cap.clone()])
                    };
                    let mut runs = vec![first, t("b")];
                    if suffix {
                        runs.extend([t(" "), styled_t("z", BOLD)]);
                    }
                    let (out, warnings) = styled(&doc(vec![body(runs)]));
                    assert_eq!((out.as_str(), warnings), want.next().unwrap());
                    let want = format!(
                        "{}a。b{}",
                        if picture { "" } else { "h" },
                        if suffix { "z" } else { "" }
                    );
                    assert_eq!(squeeze(&visible(&out)), want, "{out:?}");
                    assert!(warnings <= 1, "{out:?}");
                    assert!(crate::MdDecoder::decode_with_default(&out).is_ok(), "{out:?}");
                    if warnings == 0 {
                        let a = emphasized(&out).into_iter().find(|(c, ..)| *c == 'a').unwrap();
                        assert_eq!(a, ('a', shape == BOLD, shape != BOLD, false), "{out:?}");
                    }
                }
            }
        }
    }
    // No false positive from child offsets: the whole caption prefix and
    // its leading whitespace are trimmed at the table's existing Md join.
    let (out, warnings) = styled(&doc(vec![body(vec![
        captioned(one_cell(body(vec![t("h")])), vec![body(vec![styled_t("  a", BOLD)])]),
        t("b"),
    ])]));
    assert_eq!((out.as_str(), warnings), ("| h |\n| --- |\n\n**a**b", 0));
}

/// A complete GFM cell is checked after its paragraphs are joined and LF
/// becomes <br>. A failed caption boundary stays in that cell, including
/// in a standalone table with an out-of-scope caption.
/// 이것을 실패시키는 것: 칸 실패시 대체 생략 · 실패를 표 전체로 전파. (칸 문맥 제거는
/// 여기서 죽지 않는다: `fix6_devices_are_pinned` 의 MB 사례.)
#[test]
fn joined_cell_emphasis_stays_local() {
    use hwpforge_core::{Table, TableCell, TableRow};
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(
            vec![
                body(vec![image("", Some(vec![body(vec![styled_t("a。", BOLD)])])), t("b")]),
                body(vec![styled_t("safe", BOLD)]),
            ],
            u(1000),
        ),
        TableCell::new(vec![body(vec![styled_t("next", BOLD)])], u(1000)),
    ])]);
    let (out, warnings) =
        styled(&doc(vec![body(vec![captioned(table, vec![body(vec![styled_t("CAP", BOLD)])])])]));
    assert_eq!(warnings, 1, "{out:?}");
    assert_eq!(squeeze(&visible(&out)), "a。bsafenextCAP", "{out:?}");
    assert!(out.contains("| **next** |"), "{out:?}");
    assert!(out.ends_with("\n\n**CAP**"), "{out:?}");
    assert_eq!(count(&out, |t| matches!(t, Tag::Table(_))), 1);
    let mut table = one_cell(body(vec![styled_t("a。", BOLD)]));
    table.rows[0].cells[0].paragraphs.push(body(vec![styled_t("b", BOLD)]));
    let (out, warnings) = styled(&doc(vec![body(vec![cases::table(table)])]));
    assert_eq!((out.as_str(), warnings), ("| **a。**<br>**b** |\n| --- |", 0));
}

/// The emphasis writer's other long inputs: a text box with many empty
/// paragraphs before its text, and emphasized text, `!` and a link over and
/// over (each `!` before a link is escaped, moving the places after it).
/// 이것을 실패시키는 것: 앞 공백 항목을 하나씩 지우기 · `!` 이스케이프마다 모든 자리 다시 훑기.
#[test]
fn emphasis_writer_is_linear() {
    use hwpforge_core::control::Control;
    let mut paragraphs: Vec<hwpforge_core::Paragraph> =
        (0..100_000).map(|_| body(vec![t("")])).collect();
    paragraphs.push(body(vec![styled_t("x", BOLD)]));
    let boxed = doc(vec![body(vec![text_box(paragraphs)])]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&boxed);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!((out.as_str(), fallbacks), ("**x**", 0));
    let mut runs = Vec::new();
    for _ in 0..100_000 {
        runs.push(styled_t("a", BOLD));
        runs.push(t("!"));
        runs.push(ctrl(Control::hyperlink("b", "https://e.invalid/")));
    }
    let linked = doc(vec![body(runs)]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&linked);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, 0);
    assert!(out.starts_with("**a**\\![b](https://e.invalid/)**a**"), "{}", &out[..60]);
}

/// Many refused links whose text is `\\`: each piece's join reads the kept
/// count of `\\` before it instead of counting the whole tail again.
/// 이것을 실패시키는 것: `join` 이 꼬리를 조각마다 다시 세기.
#[test]
fn backslash_links_are_linear() {
    use hwpforge_core::control::Control;
    let runs: Vec<hwpforge_core::Run> =
        (0..100_000).map(|_| ctrl(Control::hyperlink("\\", "javascript:x"))).collect();
    let doc = doc(vec![body(runs)]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, 0);
    assert_eq!(out, "\\\\".repeat(100_000));
}

/// A comment at a paragraph's start, then emphasis over three pieces in
/// every emphasis (7³): the check reads the text after the comment as it is
/// written (on its own line), so nested emphasis is exact and crossing is
/// exact or plain text with one warning — never markers shown as text. The
/// review's case is a memo, then bold `a`, bold italic `b`, italic `c`.
/// 이것을 실패시키는 것: 주석 뒤 줄 끝내기를 검사 뒤로 미루기 · 읽지 않은 글을 통과시키기.
#[test]
fn hidden_emphasis_is_not_passed() {
    let kinds: Vec<(bool, bool, bool)> =
        (1..8).map(|k| (k & 1 == 1, k & 2 == 2, k & 4 == 4)).collect();
    let shape = |f: (bool, bool, bool)| emphasis(f.0, f.1, f.2);
    let mut problems = Vec::new();
    for lead in [memo(vec![body(vec![t("m")])]), chart()] {
        for &x in &kinds {
            for &y in &kinds {
                for &z in &kinds {
                    let runs = vec![
                        lead.clone(),
                        styled_t("a", shape(x)),
                        styled_t("b", shape(y)),
                        styled_t("c", shape(z)),
                    ];
                    let (out, fallbacks) = styled(&doc(vec![body(runs)]));
                    let want =
                        vec![('a', x.0, x.1, x.2), ('b', y.0, y.1, y.2), ('c', z.0, z.1, z.2)];
                    let exact = fallbacks == 0 && emphasized(&out) == want;
                    let plain = fallbacks == 1 && squeeze(&visible(&out)).ends_with("abc");
                    if !(exact || (!nests(&[x, y, z]) && plain)) {
                        problems.push(format!("{x:?} {y:?} {z:?} → {out:?} ({fallbacks})"));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}:\n{}", problems.len(), problems.join("\n"));
}

/// One paragraph written as plain text twice — a cell whose emphasis the
/// writer cannot place, then the Body check for `1` + `. x` — still has one
/// warning.
/// 이것을 실패시키는 것: 받침 대체가 같은 규칙을 쓰지 않기.
#[test]
fn one_warning_per_paragraph() {
    let crossing = vec![
        styled_t("a", emphasis(true, false, false)),
        styled_t("b", emphasis(true, true, false)),
        styled_t("c", emphasis(false, true, false)),
    ];
    let runs = vec![t("1"), compose(". x"), table(one_cell(body(crossing)))];
    let out = encode_styled(&doc(vec![body(runs)]), &Styles);
    let warned =
        out.warnings.iter().filter(|w| matches!(w, MdWarning::MarkupWrittenAsText { .. })).count();
    assert_eq!(warned, 1, "{:?} {}", out.warnings, out.markdown);
}

/// Many paragraphs written as plain text, each its own warning: whether a
/// paragraph is reported is looked up, not searched for.
/// 이것을 실패시키는 것: 경고 목록을 처음부터 찾기.
#[test]
fn many_fallbacks_are_linear() {
    let crossing = || {
        body(vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ])
    };
    let doc = doc((0..N_FALLBACKS).map(|_| crossing()).collect());
    let started = std::time::Instant::now();
    let (_, fallbacks) = styled(&doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, N_FALLBACKS);
}

const N_FALLBACKS: usize = 100_000;

/// Backslashes of the document's text, then many controls that draw
/// nothing: each is left out once, the text's tail is not scanned again.
/// 이것을 실패시키는 것: 빈 조각마다 앞 출력을 다시 훑기.
#[test]
fn empty_pieces_after_text_are_linear() {
    use hwpforge_core::control::Control;
    let mut runs = vec![t(&"\\".repeat(100_000))];
    runs.extend((0..100_000).map(|_| ctrl(Control::bookmark("b"))));
    runs.push(styled_t("x", BOLD));
    let doc = doc(vec![body(runs)]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, 0);
    assert!(out.ends_with("**x**"), "{}", &out[out.len() - 20..]);
}

/// A comment that comes to a line start through the block's own bytes —
/// after a table's blank line, at a caption's end, in a picture's caption,
/// in a text box — or two comments with a line ending or a space between:
/// the bytes G2 wrote before this writer (no warning).
/// 이것을 실패시키는 것: 줄 끝내기를 완성 블록이 아닌 작성기 안에서 하기 · 줄 끝내기 삭제.
#[test]
fn line_start_comments_are_g2_bytes() {
    for name in [
        "g12_table_memo",
        "g12_caption_memo",
        "g12_picture_caption_memo",
        "g12_text_box_memo",
        "g12_comments_lf",
        "g12_comments_space",
    ] {
        assert_eq!(styled(&case(name)), (base(name).0.to_string(), 0), "{name}");
    }
}

/// Text with no markers of its own (plain text, a compose) between
/// emphasized text: it is checked to read with no emphasis, so markers that
/// cannot close next to it (after Unicode punctuation, `**a。**b**z**`) or
/// that would run over it (`~~**p**~~q~~r~~`) are caught. Each here cannot
/// be written with markers and is plain text with one warning, every
/// character kept (fix5 did the same; fix6 let them through).
/// 이것을 실패시키는 것: 평문을 검사에서 빼기 · 글 컨트롤을 검사에서 빼기 · 코드 스팬을
/// 글로 읽지 않기.
#[test]
fn plain_text_keeps_no_emphasis() {
    let strike = |bold| emphasis(bold, false, true);
    for (runs, want) in [
        (
            vec![
                captioned(one_cell(body(vec![t("v")])), vec![body(vec![styled_t("a。", BOLD)])]),
                t("b"),
                styled_t("z", BOLD),
            ],
            "v a。bz",
        ),
        (vec![styled_t("제1조。", BOLD), t("본문"), styled_t("강조", BOLD)], "제1조。본문강조"),
        (vec![styled_t("p", strike(true)), t("q"), styled_t("r", strike(false))], "pqr"),
        (vec![styled_t("a。", BOLD), compose("b"), styled_t("z", BOLD)], "a。bz"),
    ] {
        assert_eq!(styled(&doc(vec![body(runs)])), (want.to_string(), 1));
    }
    // A code span (an unknown control) is read as text: bold `a` next to it
    // is written as it is, no warning.
    let (out, fallbacks) = styled(&doc(vec![body(vec![styled_t("a", BOLD), unknown("x")])]));
    assert_eq!((out.as_str(), fallbacks), ("**a**`[x]`", 0));
}

/// fix6's devices, pinned by bytes and warnings.
/// - MA, a caption's text ranges carried into the Body's check: caption
///   plain `a`, bold strikeout `b`, plain `c`, then strikeout `p`, plain
///   `q`, `r` cannot be written (plain text, one warning); without the
///   ranges `c` silently took the strikeout.
/// - MB, a GFM cell checked in its cell: `**a**<br><br>    *x*` is right
///   there (in a Body the indent after the blank line would be code).
/// - MC, spans closed before a caption joins: `p` bold, a caption `c`,
///   `q` bold stay three spans in the cell, not `**p<br><br>cq**`.
///
/// 이것을 실패시키는 것: MA 자식 범위 전달 삭제 · MB 칸을 Body 문맥으로 검사 ·
/// MC 합류 앞 닫기 삭제.
#[test]
fn fix6_devices_are_pinned() {
    let cell = |ps: Vec<hwpforge_core::Paragraph>| {
        let mut table = one_cell(ps[0].clone());
        table.rows[0].cells[0].paragraphs = ps;
        doc(vec![body(vec![cases::table(table)])])
    };
    let caption = vec![body(vec![t("a"), styled_t("b", emphasis(true, false, true)), t("c")])];
    let ma = doc(vec![body(vec![
        captioned(one_cell(body(vec![t("v")])), caption),
        styled_t("p", emphasis(false, false, true)),
        t("q"),
        t("r"),
    ])]);
    assert_eq!(styled(&ma), ("v abcpqr".to_string(), 1));
    let italic = emphasis(false, true, false);
    let mb = cell(vec![
        body(vec![styled_t("a", BOLD)]),
        body(vec![t("")]),
        body(vec![styled_t("    x", italic)]),
    ]);
    assert_eq!(styled(&mb), ("| **a**<br><br>    *x* |\n| --- |".to_string(), 0));
    let mc = cell(vec![body(vec![
        styled_t("p", BOLD),
        image("", Some(vec![body(vec![t("c")])])),
        styled_t("q", BOLD),
    ])]);
    assert_eq!(styled(&mc), ("| **p**<br><br>c**q** |\n| --- |".to_string(), 0));
}

/// Text controls that read as no text or as math: a link with no text
/// (`[](url)`) and an equation (`$a*b*c$`, its `*` the math's) are written
/// as they are next to emphasis, as they are alone — no warning. `x+y`, a
/// fraction and a subscript too. The pairs alone and with bold `x`.
/// 이것을 실패시키는 것: 빈 라벨 링크에 글 읽기 요구 · 수식을 평문으로 검사.
#[test]
fn text_controls_with_nothing_or_math() {
    use hwpforge_core::control::Control;
    let eq = |s: &str| ctrl(Control::equation(s));
    let link = || ctrl(Control::hyperlink("", "https://example.org"));
    for (runs, want) in [
        (vec![eq("a*b*c")], "$a*b*c$"),
        (vec![styled_t("x", BOLD), eq("a*b*c")], "**x**$a*b*c$"),
        (vec![styled_t("x", BOLD), eq("a*b*c"), styled_t("y", BOLD)], "**x**$a*b*c$**y**"),
        (vec![styled_t("x", BOLD), eq("x+y")], "**x**$x+y$"),
        (vec![styled_t("x", BOLD), eq("{a} over {b}")], "**x**$\\frac{a}{b}$"),
        (vec![styled_t("x", BOLD), eq("x_2")], "**x**$x_{2}$"),
        (vec![link()], "[](https://example.org)"),
        (vec![styled_t("x", BOLD), link()], "**x**[](https://example.org)"),
    ] {
        assert_eq!(styled(&doc(vec![body(runs)])), (want.to_string(), 0));
    }
}

/// An equation full of its own emphasis next to bold text: each text
/// event inside it asks only whether the outermost open span began before
/// it, not every open span.
/// 이것을 실패시키는 것: 열린 강조를 전부 훑기.
#[test]
fn math_check_is_linear() {
    use hwpforge_core::control::Control;
    let n = 128_000;
    let script = format!("{}a{}b{}", "*".repeat(n), "\\!".repeat(n), "*".repeat(n));
    let doc = doc(vec![body(vec![styled_t("x", BOLD), ctrl(Control::equation(&script))])]);
    let started = std::time::Instant::now();
    let (out, fallbacks) = styled(&doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(fallbacks, 0);
    assert!(out.starts_with("**x**$"), "{}", &out[..20]);
}

/// A paragraph written as plain text keeps its content: pictures, note
/// references and allowed links stay as they were drawn, only the text is
/// escaped. Every fallback path — a guarded paragraph's emphasis, a GFM
/// cell's, the Body check, the whole-document check — and the edges: a note
/// reference at a line start before `:`, a memo in the paragraph (its body is
/// text, as before), a link not allowed (its text, as before).
/// 이것을 실패시키는 것: 그림·각주 참조·링크를 남기지 않기 · 둘째 조각부터 앞 공백 지우기.
#[test]
fn fallback_keeps_pictures_links_and_notes() {
    use hwpforge_core::control::Control;
    let cross = || {
        vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ]
    };
    let picture = || image("BinData/p.png", None);
    let note = || footnote(vec![body(vec![t("n")])]);
    let with = |tail: Vec<hwpforge_core::Run>| {
        let mut runs = cross();
        runs.extend(tail);
        runs
    };
    let cases: Vec<(hwpforge_core::Document<hwpforge_core::Validated>, &str, usize)> = vec![
        // A guarded paragraph's emphasis.
        (
            doc(vec![body(with(vec![t(" "), picture(), t(" x"), note()]))]),
            "abc ![p](images/p.png) x[^1]\n\n[^1]: n",
            1,
        ),
        // The Body check (`1` + `. x` makes a list).
        (
            doc(vec![body(vec![t("1"), compose(". x"), t(" "), picture(), note()])]),
            "1\\. x ![p](images/p.png)[^1]\n\n[^1]: n",
            1,
        ),
        // A GFM cell's emphasis: the cell alone.
        (
            doc(vec![body(vec![
                t("z"),
                table(one_cell(body(with(vec![t(" "), picture(), t(" x"), note()])))),
            ])]),
            "z\n| abc ![p](images/p.png) x[^1] |\n| --- |\n\n[^1]: n",
            1,
        ),
        // An allowed link kept, its text escaped; one not allowed is text.
        (
            doc(vec![body(with(vec![
                ctrl(Control::hyperlink("t*", "https://e.invalid/")),
                ctrl(Control::hyperlink("u", "javascript:x")),
            ]))]),
            "abc[t\\*](https://e.invalid/)u",
            1,
        ),
        // A note reference at a line start, `:` after it escaped.
        (doc(vec![body([vec![note(), t(": x")], cross()].concat())]), "[^1]\\: xabc\n\n[^1]: n", 1),
        // The whole-document check (a definition list across blocks).
        (
            doc(vec![
                cases::p(HEADING, vec![t("h")]),
                body(vec![t("a "), picture()]),
                body(vec![image("", Some(vec![body(vec![t(":b")])]))]),
            ]),
            "h\n\na ![p](images/p.png)\n\n\\:b",
            3,
        ),
        // A memo in the paragraph (it is drawn from a copy): the reference
        // still kept; the memo's body is text, as before.
        (
            doc(vec![body([vec![memo(vec![body(vec![t("m")])])], cross(), vec![note()]].concat())]),
            "mabc[^1]\n\n[^1]: n",
            1,
        ),
        // A memo's body was a comment: its picture and link are its text
        // (none kept), even where the same picture is drawn outside it.
        (
            doc(vec![body(
                [
                    vec![memo(vec![body(vec![
                        picture(),
                        ctrl(Control::hyperlink("L", "https://e.invalid/")),
                    ])])],
                    cross(),
                    vec![picture()],
                ]
                .concat(),
            )]),
            "Labc![p](images/p.png)",
            1,
        ),
    ];
    for (doc, want, warnings) in cases {
        assert_eq!(styled(&doc), (want.to_string(), warnings));
    }
}

/// A fallback keeps what was drawn for each occurrence: a note in a cell a
/// GFM table cut (not drawn, not registered) is its text, the note after
/// it keeps its reference; a picture in the cut cell was never drawn and
/// gets no asset. Two pictures with one file name and other bytes keep
/// their own keys, the assets as without the fallback.
/// 이것을 실패시키는 것: 각주를 개수로 맞추기(어긋나면 모두 글) · 그리지 않은 그림 등록 ·
/// 키 다시 매기기.
#[test]
fn fallback_keeps_only_what_was_drawn() {
    use hwpforge_core::{TableCell, TableRow};
    let cross = || {
        vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ]
    };
    let note = |n: &str| footnote(vec![body(vec![t(n)])]);
    let mut cut = one_cell(body(vec![t("h")]));
    cut.rows.push(TableRow::new(vec![
        TableCell::new(vec![body(vec![t("v")])], u(1000)),
        TableCell::new(vec![body(vec![note("cut"), image("A/one.png", None)])], u(1000)),
    ]));
    let doc_cut = doc(vec![body([cross(), vec![table(cut), note("kept")]].concat())]);
    let out = encode_styled(&doc_cut, &Styles);
    assert_eq!(out.markdown, "abc h v cut[^1]\n\n[^1]: kept");
    assert!(out.images.is_empty(), "{:?}", out.images.keys());
    let pictures = || vec![image("A/one.png", None), t(" "), image("B/one.png", None)];
    let fell = encode_styled(&doc(vec![body([cross(), pictures()].concat())]), &Styles);
    assert_eq!(fell.markdown, "abc![one](images/one.png) ![one](images/one-2.png)");
    let drawn = encode_styled(&doc(vec![body(pictures())]), &Styles);
    let keys = |o: &crate::MdOutput| {
        let mut k: Vec<_> = o.images.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        k.sort();
        k
    };
    assert_eq!(keys(&fell), keys(&drawn));
}

/// A note in a paragraph with a memo is drawn from a copy (`expand_memos`):
/// the fallback still keeps its reference, at the top level, in a GFM
/// cell, and in a cell of a paragraph that is itself a copy (the copy's
/// alias resolved to the document's note). A memo in another memo's
/// anchor draws the anchor from a copy too.
/// 이것을 실패시키는 것: 별칭 삭제 · 별칭을 문서 주소로 풀지 않기 · 별칭 조회 삭제.
#[test]
fn fallback_keeps_notes_by_occurrence() {
    let cross = || {
        vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ]
    };
    let note = |n: &str| footnote(vec![body(vec![t(n)])]);
    let m = || memo(vec![body(vec![t("m")])]);
    let cell = || one_cell(body([vec![m()], cross(), vec![note("k")]].concat()));
    let mut cases = vec![
        (doc(vec![body([vec![m()], cross(), vec![note("k")]].concat())]), "mabc[^1]\n\n[^1]: k"),
        (doc(vec![body(vec![t("z"), table(cell())])]), "z\n| mabc[^1] |\n| --- |\n\n[^1]: k"),
        (
            doc(vec![body(vec![t("z"), m(), table(cell())])]),
            "z<!-- memo: m -->\n| mabc[^1] |\n| --- |\n\n[^1]: k",
        ),
        (
            doc(vec![body(
                [cross(), vec![anchored_memo(vec![anchored_memo(vec![note("k")])])]].concat(),
            )]),
            "abc [^1]\n\n[^1]: k",
        ),
    ];
    // A note in a memo's body (inline in the comment) is not paired: the
    // anchor's note keeps its own reference.
    let both = ctrl(hwpforge_core::control::Control::memo_with_anchor(
        vec![body(vec![note("x")])],
        vec![note("a")],
    ));
    cases.push((doc(vec![body([vec![both], cross()].concat())]), "[^1] xabc\n\n[^1]: a"));
    for (doc, want) in cases {
        assert_eq!(styled(&doc), (want.to_string(), 1));
    }
}

/// A link label in a fallback keeps every character: leading spaces, a
/// tab, a label of spaces only.
/// 이것을 실패시키는 것: 라벨 앞 공백 지우기.
#[test]
fn fallback_link_labels_keep_their_spaces() {
    use hwpforge_core::control::Control;
    let cross = || {
        vec![
            styled_t("a", emphasis(true, false, false)),
            styled_t("b", emphasis(true, true, false)),
            styled_t("c", emphasis(false, true, false)),
        ]
    };
    let doc = doc(vec![body(
        [
            cross(),
            vec![
                ctrl(Control::hyperlink(" L", "https://e.invalid/")),
                ctrl(Control::hyperlink("\tT", "https://e.invalid/")),
                ctrl(Control::hyperlink("  ", "https://e.invalid/")),
            ],
        ]
        .concat(),
    )]);
    assert_eq!(
        styled(&doc),
        (
            "abc[ L](https://e.invalid/)[\tT](https://e.invalid/)[  ](https://e.invalid/)"
                .to_string(),
            1
        )
    );
}

/// Pictures whose keys collide (`a.png`, `a-2.png` …) cost what their
/// number does, drawn and in a fallback (each walked the keys before it:
/// 4,096 pictures took seconds).
/// 이것을 실패시키는 것: 키를 매번 처음부터 걷기 · 접두 기록 조건 삭제.
#[test]
fn colliding_pictures_take_linear_time() {
    let n = 4096;
    let mut runs = vec![
        styled_t("a", emphasis(true, false, false)),
        styled_t("b", emphasis(true, true, false)),
        styled_t("c", emphasis(false, true, false)),
    ];
    runs.extend((0..n).map(|i| image(&format!("N/{i}/a.png"), None)));
    let doc = doc(vec![body(runs)]);
    let started = std::time::Instant::now();
    let out = encode_styled(&doc, &Styles);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(out.images.len(), n);
    assert!(
        out.markdown.ends_with(&format!("![a](images/a-{n}.png)")),
        "{}",
        &out.markdown[out.markdown.len() - 40..]
    );
    assert_eq!(out.markdown.matches("![a]").count(), n);
}

/// The text a reader gets back: every paragraph's (a list item's with the
/// decoder's marker), a note's body after `|`, paragraphs joined by `/`.
fn decoded_text(markdown: &str) -> String {
    fn text(paragraphs: &[hwpforge_core::Paragraph], out: &mut Vec<String>) {
        for p in paragraphs {
            let mut line = String::new();
            for run in &p.runs {
                if let Some(t) = run.content.plain_text() {
                    line.push_str(&t);
                }
                if let Some(hwpforge_core::control::Control::Footnote { paragraphs, .. }) =
                    run.content.as_control()
                {
                    let mut body = Vec::new();
                    text(paragraphs, &mut body);
                    line.push('|');
                    line.push_str(&body.join("/"));
                }
            }
            out.push(line);
        }
    }
    let doc = crate::MdDecoder::decode_with_default(markdown).expect("decodes").document;
    let mut out = Vec::new();
    for section in doc.sections() {
        text(&section.paragraphs, &mut out);
    }
    out.join("/")
}

/// A tab after a list marker makes a block like a space in a note body
/// too: the marker is escaped, the `\` before the punctuation of an ordered
/// marker (1–9 digits); read back, the body is the document's.
/// 이것을 실패시키는 것: 목록 표지 뒤 탭 무시(각주 본문 · 순서 목록) · 탭 뒤 숫자 앞에 `\` ·
/// 10자리 숫자를 목록으로.
#[test]
fn tab_after_a_marker_makes_no_block() {
    for (raw, want) in [
        ("-\tx", "\\-\tx"),
        ("+\tx", "\\+\tx"),
        ("1.\tx", "1\\.\tx"),
        ("23)\tx", "23\\)\tx"),
        ("1234567890.\tx", "1234567890.\tx"),
    ] {
        let (md, warnings) =
            styled(&doc(vec![body(vec![t("a"), footnote(vec![body(vec![t(raw)])])])]));
        assert_eq!((md.as_str(), warnings), (format!("a[^1]\n\n[^1]: {want}").as_str(), 0));
        assert_eq!(decoded_text(&md), format!("a|{raw}"), "{md:?}");
    }
}

/// Definition-shaped document text in a body paragraph, every combination
/// of a list marker or quote (or none), a tab and 0–4 spaces or 0–6
/// spaces, a definition, note definition, split definition, bracket or
/// lone `[`, with and without a list item before it. Escaping such text is
/// left as it was in bd3f6e67 (its CLI's output in
/// `tests/fixtures/definition_shapes_base.txt`); a definition it lets
/// through is the final check's, which falls the paragraph back with a
/// warning. So for each text and view: lossy without a fallback writes the
/// earlier bytes; read back, the text is the earlier output's, unless the
/// output carries the warning or the earlier output did not read back (a
/// definition that reached it); and a new fallback only where the earlier
/// output did not read back. The same texts in a note body (styled): no
/// warning, and read back the earlier output's text or the document's own;
/// only six named texts do not read back, as before.
/// 이것을 실패시키는 것: 줄 첫 표지 뒤 탭을 구분자로 받기(fix4 의 예외, 코드 안 `[` 에 `\`) ·
/// 최종 검사의 각주 정의 갈래 삭제 · 각주 본문 탭 뒤 숫자 앞에 `\` · 각주 본문 탭 표지
/// 이스케이프 삭제(디코드 실패) · 각주 본문 글자 바꾸기(`[r]` → `[s]`).
#[test]
fn lossy_definition_shapes() {
    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }
    let read =
        |md: &str| crate::MdDecoder::decode_with_default(md).is_ok().then(|| decoded_text(md));
    let base: Vec<Vec<String>> = include_str!("../../tests/fixtures/definition_shapes_base.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').map(unescape).collect())
        .collect();
    let mut texts = Vec::new();
    for before in ["", "- x\n\n", "- x\n"] {
        for marker in ["-", "+", "1.", "1)", ">", ""] {
            let indents = (0..5).map(|k| format!("\t{}", " ".repeat(k)));
            for indent in indents.chain((0..7).map(|k| " ".repeat(k))) {
                for shape in ["[r]: /u", "[^r]: y", "[r]\n]: /u", "[^r]\n]: y", "[r]", "["] {
                    texts.push(format!("{before}{marker}{indent}{shape}"));
                }
            }
        }
    }
    assert_eq!(texts.len(), base.len());
    for (raw, base) in texts.iter().zip(&base) {
        assert_eq!(raw, &base[0]);
        let d = doc(vec![body(vec![t(raw)])]);
        let (lossy, warned) = crate::MdEncoder::encode_lossy_with_report(&d).unwrap();
        let lossy = (lossy, warned.len());
        let styled = styled(&d);
        if lossy.1 == 0 {
            assert_eq!(lossy.0, base[1], "{raw:?}");
        }
        for ((md, warnings), (old, old_warnings)) in
            [(lossy, (&base[1], &base[2])), (styled, (&base[3], &base[4]))]
        {
            let earlier = read(old);
            if warnings == 0 && earlier.is_some() {
                assert_eq!(read(&md), earlier, "{raw:?}: {md:?}");
            }
            if warnings > 0 && old_warnings == "0" {
                assert_eq!(earlier, None, "{raw:?}: a new fallback for {old:?}");
            }
        }
    }
    // In a note body (styled only): read back, the text is the earlier
    // output's, or the document's own where the earlier one read wrong or
    // not at all (a tab after a list marker, now escaped). Six texts did
    // not read back before and do not now: a second paragraph indented a
    // tab and four spaces.
    let known_failures = [
        "- x\n\n\t    [r]: /u",
        "- x\n\n\t    [^r]: y",
        "- x\n\n\t    [r]\n]: /u",
        "- x\n\n\t    [^r]\n]: y",
        "- x\n\n\t    [r]",
        "- x\n\n\t    [",
    ];
    let mut failures = Vec::new();
    for (raw, base) in texts.iter().zip(&base) {
        let (md, warnings) =
            styled(&doc(vec![body(vec![t("a"), footnote(vec![body(vec![t(raw)])])])]));
        assert_eq!((warnings, base[6].as_str()), (0, "0"), "{raw:?}");
        let own = format!("a|{}", raw.replace("\n\n", "/").replace('\n', " "));
        match (read(&md), read(&base[5])) {
            (None, earlier) => {
                assert_eq!((md.as_str(), earlier), (base[5].as_str(), None), "{raw:?}");
                failures.push(raw.as_str());
            }
            (Some(now), earlier) => {
                assert!(earlier.as_ref() == Some(&now) || now == own, "{raw:?}: {md:?} {now:?}");
            }
        }
    }
    assert_eq!(failures, known_failures);
    let dests = std::collections::HashSet::new();
    let lossy =
        Allowed { tags: super::guard::LOSSY_TAGS, destinations: &dests, decoder_structure: false };
    let block = Block {
        markdown: "- x\n\t[^r]: y".into(),
        source: Some(Source {
            prefix: String::new(),
            texts: vec![super::guard::plain_text("- x\n\t[^r]: y")],
            at: vec![(0, 0)],
        }),
        body: false,
    };
    let mut warnings = Vec::new();
    assert_eq!(checked(vec![block], &lossy, &mut warnings), "\\- x \t\\[\\^r\\]\\: y");
    assert_eq!(warnings.len(), 1);
}

/// The regression rows of the G4 reviews, lossy: the document text, the
/// output and warnings of 0.17.1 (`a5e33451`; bd3f6e67 wrote the same),
/// this view's, and the text this view's output reads back as. Each row
/// writes the earlier bytes, or falls back with a warning where the earlier
/// output did not read back (an orphan note definition).
/// 이것을 실패시키는 것: 줄 첫 표지 뒤 탭 예외 · 최종 검사의 각주 정의 갈래 삭제 · 들여쓰기·
/// 컨테이너 짐작(코드에 `\`).
#[test]
fn lossy_definition_rows() {
    let rows: &[(&str, &str, usize, &str, usize, &str)] = &[
        ("- x\n\n      [r]", "- x\n\n      [r]", 0, "- x\n\n      [r]", 0, "- x/[r]"),
        ("- x\n\n      [", "- x\n\n      [", 0, "- x\n\n      [", 0, "- x/["),
        ("- x\n\n\t\t[r]: /u", "- x\n\n\t\t[r]: /u", 0, "- x\n\n\t\t[r]: /u", 0, "- x/  [r]: /u"),
        ("- x\n    [^r]: y", "- x\n    \\[^r]: y", 0, "- x\n    \\[^r]: y", 0, "- x [^r]: y"),
        ("- x\n\n    [^r]: y", "- x\n\n    \\[^r]: y", 0, "- x\n\n    \\[^r]: y", 0, "- x/[^r]: y"),
        ("- x\n\t[^r]: y", "- x\n\t[^r]: y", 0, "\\- x \t\\[\\^r\\]\\: y", 1, "- x \t[^r]: y"),
        (
            "- x\n  \t[^r]: y",
            "- x\n  \t[^r]: y",
            0,
            "\\- x   \t\\[\\^r\\]\\: y",
            1,
            "- x   \t[^r]: y",
        ),
        ("> - x\n>   [^r]: y", "> - x\n>   \\[^r]: y", 0, "> - x\n>   \\[^r]: y", 0, "- x [^r]: y"),
        (
            "> - x\n>     [^r]: y",
            "> - x\n>     \\[^r]: y",
            0,
            "> - x\n>     \\[^r]: y",
            0,
            "- x [^r]: y",
        ),
        ("1. x\n    [^r]: y", "1. x\n    \\[^r]: y", 0, "1. x\n    \\[^r]: y", 0, "1. x [^r]: y"),
        (
            "- x\n    [^r]: y\n\n    z[^r]",
            "- x\n    \\[^r]: y\n\n    z[^r]",
            0,
            "- x\n    \\[^r]: y\n\n    z[^r]",
            0,
            "- x [^r]: y/z[^r]",
        ),
        ("\t[r]: /u", "\t[r]: /u", 0, "\t[r]: /u", 0, "[r]: /u"),
        ("  \t[r]: /u", "  \t[r]: /u", 0, "  \t[r]: /u", 0, "[r]: /u"),
        ("-\t\t[r]: /u", "-\t\t[r]: /u", 0, "-\t\t[r]: /u", 0, "-   [r]: /u"),
        ("-\t[r]: /u", "\\-\t\\[r\\]\\: \\/u", 1, "\\-\t\\[r\\]\\: \\/u", 1, "-\t[r]: /u"),
        ("-  \t[r]: /u", "\\-  \t\\[r\\]\\: \\/u", 1, "\\-  \t\\[r\\]\\: \\/u", 1, "-  \t[r]: /u"),
        (">\t[r]: /u", "\\>\t\\[r\\]\\: \\/u", 1, "\\>\t\\[r\\]\\: \\/u", 1, ">\t[r]: /u"),
        ("1)\t[r]: /u", "1\\)\t\\[r\\]\\: \\/u", 1, "1\\)\t\\[r\\]\\: \\/u", 1, "1)\t[r]: /u"),
        ("-\t\t- [r]: /u", "-\t\t- [r]: /u", 0, "-\t\t- [r]: /u", 0, "-   - [r]: /u"),
        ("x\n    [r]: /u", "x\n    \\[r]: /u", 0, "x\n    \\[r]: /u", 0, "x [r]: /u"),
        ("\t- x\n\n\t[r]: /u", "\t- x\n\n\t[r]: /u", 0, "\t- x\n\n\t[r]: /u", 0, "- x\n\n[r]: /u"),
        ("-x\n    [r]: /u", "-x\n    \\[r]: /u", 0, "-x\n    \\[r]: /u", 0, "-x [r]: /u"),
        (
            "> -\t[r]: /u",
            "\\> \\-\t\\[r\\]\\: \\/u",
            1,
            "\\> \\-\t\\[r\\]\\: \\/u",
            1,
            "> -\t[r]: /u",
        ),
        (
            "- -\t[r]: /u",
            "\\- \\-\t\\[r\\]\\: \\/u",
            1,
            "\\- \\-\t\\[r\\]\\: \\/u",
            1,
            "- -\t[r]: /u",
        ),
    ];
    for &(raw, old, old_warnings, new, warnings, read) in rows {
        let (md, got) =
            crate::MdEncoder::encode_lossy_with_report(&doc(vec![body(vec![t(raw)])])).unwrap();
        assert_eq!((md.as_str(), got.len()), (new, warnings), "{raw:?}");
        assert_eq!(decoded_text(&md), read, "{raw:?}");
        if (new, warnings) != (old, old_warnings) {
            assert_eq!((old_warnings, warnings), (0, 1), "{raw:?}");
            assert!(crate::MdDecoder::decode_with_default(old).is_err(), "{raw:?}");
        }
    }
}

/// Line starts are found in one pass: a long run of line endings costs
/// what its length does (the same run scanned again at each line ending
/// took billions of steps).
/// 이것을 실패시키는 것: 후보마다 공백을 다시 훑기.
#[test]
fn guarded_text_is_linear() {
    let raw = format!("{}x", "\n".repeat(100_000));
    let started = std::time::Instant::now();
    let md = Md::guarded_text(&raw);
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "{:?}", started.elapsed());
    assert_eq!(md, Md::text(&raw));
}

// ---------------------------------------------------------------------------
// G-9: a fallback shows what styled shows
// ---------------------------------------------------------------------------

/// A Body paragraph that falls back keeps a cross reference's target, an
/// empty field's `____` and an unknown control's `[tag]`; lossy is as it
/// was.
/// 이것을 실패시키는 것: `fallback_markdown` 의 styled 표시값을 공용 함수 없이 되돌리기 · lossy 에도 styled
/// 표시값.
#[test]
fn fallback_text_shows_what_styled_shows() {
    let doc = case("g9_display");
    let (out, fallbacks) = styled(&doc);
    assert_eq!(fallbacks, 1);
    for shown in ["\\[ref\\]", "\\_\\_\\_\\_", "\\[x\\]"] {
        assert!(out.contains(shown), "{shown}: {out}");
    }
    let lossy = crate::MdEncoder::encode_lossy(&doc).unwrap();
    assert_eq!(lossy, base("g9_display").1);
}

// ---------------------------------------------------------------------------
// G-7: postconditions over generated Body paragraphs
// ---------------------------------------------------------------------------

mod generated {
    use super::*;
    use proptest::prelude::*;

    const ALPHABET: &[char] = &[
        'a', '가', ' ', '\t', '\u{A0}', '\u{3000}', '-', '+', '=', ':', '|', '1', '.', ')', '&',
        ';', '\n', '<', '#', '*',
    ];

    fn piece() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(ALPHABET), 0..6)
            .prop_map(|cs| cs.into_iter().collect())
    }

    /// A carrier and the text a reader must see for it.
    fn carrier() -> impl Strategy<Value = (hwpforge_core::Run, String)> {
        (0..12usize, piece()).prop_map(|(kind, s)| match kind {
            // A picture: its alt text shows, and a fallback keeps it.
            11 => (image("BinData/pic.png", None), "pic".into()),
            // A link (its text may be empty), an equation, an unknown control.
            8 => (ctrl(hwpforge_core::control::Control::hyperlink(&s, "https://e.invalid/")), s),
            9 if s.len() % 2 == 0 => {
                (ctrl(hwpforge_core::control::Control::equation("x+y")), "x+y".into())
            }
            // An equation with its own `*` and `\!`.
            // Its inside is the math's, read as `a!b` (or `*a\!b*` as plain
            // text): each reading counts as one `§` on both sides (no `!`,
            // `\`, `§` in the alphabet), so a lost or doubled equation shows.
            9 => (ctrl(hwpforge_core::control::Control::equation("*a\\!b*")), "§".into()),
            // After `u`: two code spans next to each other read as one (a gap
            // of the code span writer, base 61d3631d the same, not text made
            // into markup: followups).
            10 => {
                let tag = s.replace('\n', " ");
                (text_box(vec![body(vec![t("u"), unknown(&tag)])]), format!("u[{tag}]"))
            }
            // A comment: at a line start it ends its line.
            6 => (chart(), String::new()),
            0 => (t(&s), s),
            // Bold text across a blank line shows its `**` (a known gap of
            // inline formats, not text made into markup: followups).
            // Emphasis of any of the seven kinds (picked by the text's length).
            1 => (styled_t(&s, emphasis(s.len() % 2 == 0, s.len() % 3 == 0, s.len() % 5 < 2)), s),
            2 => (compose(&s), s),
            3 => (field(&s), if s.is_empty() { "____".into() } else { s }),
            4 => (cross_ref(&s), if s.is_empty() { "[ref]".into() } else { s }),
            5 => (text_box(vec![body(vec![t(&s)])]), s),
            // A text box's emphasized text: it meets the text around it.
            _ => (
                text_box(vec![body(vec![styled_t(&s, emphasis(true, s.len() % 2 == 0, false))])]),
                s,
            ),
        })
    }

    /// A Body paragraph: maybe a text box holding a GFM table first, 1–4
    /// carriers, maybe a table (GFM or HTML), and a caption whose Unicode
    /// punctuation meets outer text. Two known gaps, not text made into markup,
    /// are left out (followups): a text box's GFM table after other text on
    /// its line, and a tab or Unicode space right after a GFM table (it
    /// ends the delimiter row, so the table's bars show as text — the base
    /// does the same).
    fn paragraph() -> impl Strategy<Value = (hwpforge_core::Paragraph, String)> {
        (
            prop::option::of(piece()),
            prop::collection::vec(carrier(), 1..5),
            prop::option::of((any::<bool>(), piece(), any::<bool>(), 0..5usize)),
            prop::option::of((
                any::<bool>(),
                prop::sample::select(vec!['。', '！', '？', '，', '；']),
                any::<bool>(),
                any::<bool>(),
            )),
        )
            .prop_map(|(boxed, mut carriers, table_run, caption)| {
                if let Some((html, cell, space, at)) = table_run {
                    let shape = emphasis(cell.len() % 2 == 0, cell.len() % 3 == 0, false);
                    let cell_p = body(vec![styled_t(&cell, shape)]);
                    let at = at.min(carriers.len());
                    let after = if space { vec![(t(" "), " ".into())] } else { vec![] };
                    let run = table(if html { merged(cell_p) } else { one_cell(cell_p) });
                    // After the table: a comment by the cell's length, then
                    // text that needs escapes.
                    let comment = (cell.len() % 2 == 0).then(|| (chart(), String::new()));
                    let tail = [(run, cell)]
                        .into_iter()
                        .chain(after)
                        .chain(comment)
                        .chain([(t("*x*"), "*x*".into())]);
                    carriers.splice(at..at, tail);
                }
                if let Some(s) = boxed {
                    let cell = body(vec![t(&s)]);
                    let run = text_box(vec![body(vec![table(one_cell(cell))]), body(vec![])]);
                    let comment = (s.len() % 2 == 0).then(|| (chart(), String::new()));
                    let after =
                        [(run, s)].into_iter().chain(comment).chain([(t("*x*"), "*x*".into())]);
                    carriers.splice(0..0, after);
                }
                if let Some((bold, punct, picture, suffix)) = caption {
                    let text = format!("a{punct}");
                    let cap = vec![body(vec![styled_t(&text, emphasis(bold, !bold, false))])];
                    let run = if picture {
                        image("", Some(cap))
                    } else {
                        captioned(one_cell(body(vec![t("h")])), cap)
                    };
                    let text = format!("{}{text}", if picture { "" } else { "h" });
                    carriers.extend([(run, text), (t("b"), "b".into())]);
                    if suffix {
                        carriers.extend([(t(" "), " ".into()), (styled_t("z", BOLD), "z".into())]);
                    }
                }
                let (runs, text): (Vec<_>, String) = carriers.into_iter().unzip();
                (body(runs), text)
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 512,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        /// The final output, parsed again: no forbidden block, no
        /// definition list, the paragraphs' characters, at most one
        /// warning per paragraph; read back when it holds no HTML.
        /// 이것을 실패시키는 것: 규칙·Body 단계·`ending_block`·전체 검사 중 하나 삭제 ·
        /// 캡션 강조 범위를 부모 검사에서 누락.
        #[test]
        fn guarded_output_postconditions(paragraphs in prop::collection::vec(paragraph(), 1..4)) {
            let text: String = paragraphs.iter().map(|(_, s)| s.as_str()).collect();
            let doc = doc(paragraphs.into_iter().map(|(p, _)| p).collect());
            let out = encode_styled(&doc, &Styles);
            let md = &out.markdown;
            prop_assert!(forbidden(md).is_empty(), "{:?}", md);
            // An equation's `$` delimiters are not document text.
            // 이것을 실패시키는 것: 출력에서 수식 지우기 · 수식 두 번 쓰기.
            let seen = squeeze(&visible(md)).replace("*a\\!b*", "§").replace("a!b", "§").replace('$', "");
            prop_assert_eq!(seen, squeeze(&text), "{:?}", md);
            let mut per = std::collections::HashMap::new();
            for w in &out.warnings {
                if let MdWarning::MarkupWrittenAsText { section, paragraph } = w {
                    *per.entry((*section, *paragraph)).or_insert(0) += 1;
                }
            }
            prop_assert!(per.values().all(|&n| n <= 1), "{:?}", out.warnings);
            prop_assert_ne!(decodes(md), Some(false), "{:?}", md);
        }
    }
}
