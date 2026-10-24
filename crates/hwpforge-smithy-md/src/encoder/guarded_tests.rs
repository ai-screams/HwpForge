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
            Some(id.get() == BOLD)
        }
        fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
            (id.get() == UNDERLINE).then_some(UnderlineType::Bottom)
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
        source: Some(Source { prefix: String::new(), texts: vec![md.into()], at: vec![at] }),
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
/// 이것을 실패시키는 것: `deep_text` 를 공용 함수 없이 되돌리기 · lossy 에도 styled
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
        (0..6usize, piece()).prop_map(|(kind, s)| match kind {
            0 => (t(&s), s),
            // Bold text across a blank line shows its `**` (a known gap of
            // inline formats, not text made into markup: followups).
            1 => {
                let s = s.replace('\n', " ");
                (styled_t(&s, BOLD), s)
            }
            2 => (compose(&s), s),
            3 => (field(&s), if s.is_empty() { "____".into() } else { s }),
            4 => (cross_ref(&s), if s.is_empty() { "[ref]".into() } else { s }),
            _ => (text_box(vec![body(vec![t(&s)])]), s),
        })
    }

    /// A Body paragraph: maybe a text box holding a GFM table first, 1–4
    /// carriers, maybe a table (GFM or HTML) and a space after it. Each
    /// table is followed by `a`. Two known gaps, not text made into markup,
    /// are left out (followups): a text box's GFM table after other text on
    /// its line, and a tab or Unicode space right after a GFM table (it
    /// ends the delimiter row, so the table's bars show as text — the base
    /// does the same).
    fn paragraph() -> impl Strategy<Value = (hwpforge_core::Paragraph, String)> {
        (
            prop::option::of(piece()),
            prop::collection::vec(carrier(), 1..5),
            prop::option::of((any::<bool>(), piece(), any::<bool>(), 0..5usize)),
        )
            .prop_map(|(boxed, mut carriers, table_run)| {
                if let Some((html, cell, space, at)) = table_run {
                    let cell_p = body(vec![t(&cell)]);
                    let at = at.min(carriers.len());
                    let after = if space { vec![(t(" "), " ".into())] } else { vec![] };
                    let run = table(if html { merged(cell_p) } else { one_cell(cell_p) });
                    let tail = [(run, cell)].into_iter().chain(after).chain([(t("a"), "a".into())]);
                    carriers.splice(at..at, tail);
                }
                if let Some(s) = boxed {
                    let cell = body(vec![t(&s)]);
                    let run = text_box(vec![body(vec![table(one_cell(cell))]), body(vec![])]);
                    carriers.splice(0..0, [(run, s), (t("a"), "a".into())]);
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
        /// 이것을 실패시키는 것: 규칙·Body 단계·`ending_block`·전체 검사 중 하나 삭제.
        #[test]
        fn guarded_output_postconditions(paragraphs in prop::collection::vec(paragraph(), 1..4)) {
            let text: String = paragraphs.iter().map(|(_, s)| s.as_str()).collect();
            let doc = doc(paragraphs.into_iter().map(|(p, _)| p).collect());
            let out = encode_styled(&doc, &Styles);
            let md = &out.markdown;
            prop_assert!(forbidden(md).is_empty(), "{:?}", md);
            prop_assert_eq!(squeeze(&visible(md)), squeeze(&text), "{:?}", md);
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
