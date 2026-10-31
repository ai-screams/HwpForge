//! Emphasis relative to the paragraph style (#297): a run's format that its
//! paragraph style already gives is not written again, compared property by
//! property; the format is written as it is when the default is unknown or
//! the paragraph turns a default off.

use hwpforge_core::{Document, Paragraph, Run, Section, StyleLookup, Validated};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, StyleRef, UnderlineType};

use super::guarded_tests::cases::{merged, table};
use super::styled::encode_styled;

const BOLD: usize = 1;
const ITALIC: usize = 2;
const STRIKE: usize = 4;
const UNDERLINE: usize = 8;
const SUP: usize = 16;
const SUB: usize = 32;
/// A character shape of the same format under another number.
const COPY: usize = 64;
/// A paragraph style the lookup gives no default for.
const UNKNOWN_STYLE: usize = 999;
/// The paragraph shape of headings (level 1).
const HEADING: usize = 1;

/// Whether character shape `id` has the property `bit`.
fn has(id: CharShapeIndex, bit: usize) -> bool {
    (id.get() % COPY) & bit != 0
}

/// Character shape `n` has the format of the bits of `n % 64`; the
/// paragraph style `n` defaults to character shape `n`.
struct Styles;

impl StyleLookup for Styles {
    fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
        Some(has(id, BOLD))
    }
    fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
        Some(has(id, ITALIC))
    }
    fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
        Some(has(id, STRIKE))
    }
    fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
        Some(if has(id, UNDERLINE) { UnderlineType::Bottom } else { UnderlineType::None })
    }
    fn char_superscript(&self, id: CharShapeIndex) -> Option<bool> {
        Some(has(id, SUP))
    }
    fn char_subscript(&self, id: CharShapeIndex) -> Option<bool> {
        Some(has(id, SUB))
    }
    // The colour and size differ with the number: two shapes of one format
    // under different numbers also differ in what Markdown does not write.
    fn char_text_color(&self, id: CharShapeIndex) -> Option<hwpforge_foundation::Color> {
        Some(hwpforge_foundation::Color::from_rgb(0, 0, (id.get() / COPY) as u8 * 200))
    }
    fn char_font_size(&self, id: CharShapeIndex) -> Option<hwpforge_foundation::HwpUnit> {
        hwpforge_foundation::HwpUnit::new(1000 + 500 * (id.get() / COPY) as i32).ok()
    }
    fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
        (id.get() == HEADING).then_some(1)
    }
    fn style_char_shape(&self, id: StyleRef) -> Option<CharShapeIndex> {
        (id.get() != UNKNOWN_STYLE).then(|| CharShapeIndex::new(id.get()))
    }
}

/// Like [`Styles`], with every property of character shape [`UNKNOWN`]
/// unknown but bold.
struct Partial;
const UNKNOWN: usize = 500;

impl StyleLookup for Partial {
    fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
        if id.get() == UNKNOWN {
            Some(true)
        } else {
            Styles.char_bold(id)
        }
    }
    fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
        (id.get() != UNKNOWN).then(|| Styles.char_italic(id)).flatten()
    }
    fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
        (id.get() != UNKNOWN).then(|| Styles.char_strikeout(id)).flatten()
    }
    fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
        (id.get() != UNKNOWN).then(|| Styles.char_underline(id)).flatten()
    }
    fn char_superscript(&self, id: CharShapeIndex) -> Option<bool> {
        (id.get() != UNKNOWN).then(|| Styles.char_superscript(id)).flatten()
    }
    fn char_subscript(&self, id: CharShapeIndex) -> Option<bool> {
        (id.get() != UNKNOWN).then(|| Styles.char_subscript(id)).flatten()
    }
    fn style_char_shape(&self, id: StyleRef) -> Option<CharShapeIndex> {
        Styles.style_char_shape(id)
    }
}

fn run(text: &str, shape: usize) -> Run {
    Run::text(text, CharShapeIndex::new(shape))
}

fn paragraph(para: usize, style: Option<usize>, runs: Vec<Run>) -> Paragraph {
    let p = Paragraph::with_runs(runs, ParaShapeIndex::new(para));
    match style {
        Some(style) => p.with_style(StyleRef::new(style)),
        None => p,
    }
}

fn markdown(paragraphs: Vec<Paragraph>) -> String {
    markdown_with(paragraphs, &Styles)
}

fn markdown_with(paragraphs: Vec<Paragraph>, styles: &dyn StyleLookup) -> String {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, hwpforge_core::PageSettings::a4()));
    let doc: Document<Validated> = doc.validate().unwrap();
    encode_styled(&doc, styles).markdown
}

/// One paragraph's Markdown on each path that writes emphasis: a heading
/// (plain Markdown), a body paragraph (guarded) and an HTML table cell.
fn paths(style: Option<usize>, runs: Vec<Run>) -> [String; 3] {
    let heading = markdown(vec![paragraph(HEADING, style, runs.clone())]);
    let body = markdown(vec![paragraph(0, style, runs.clone())]);
    let cell = markdown(vec![paragraph(0, None, vec![table(merged(paragraph(0, style, runs)))])]);
    let cell = cell.split("<td colspan=\"2\">").nth(1).unwrap().split("</td>").next().unwrap();
    [heading.trim_start_matches("# ").to_string(), body, cell.to_string()]
}

/// A format the paragraph style gives (same number or another number of
/// the same format) is not written; one the style does not give is; with a
/// default unknown (no style, a style with no default) or a default turned
/// off in the paragraph, every run's format is written as it is. Each of
/// the six properties, on all three paths.
/// 이것을 실패시키는 것: 기준 무시 · 속성 하나 빼먹기 · 해제 run 이 있어도 기준 쓰기 ·
/// 번호로 비교 · `style_ref` 없이 추정 · 한 경로만 적용.
#[test]
fn emphasis_the_style_gives_is_not_written() {
    let marked = [
        (BOLD, "**a**", "<strong>a</strong>"),
        (ITALIC, "*a*", "<em>a</em>"),
        (STRIKE, "~~a~~", "<del>a</del>"),
        (UNDERLINE, "<u>a</u>", "<u>a</u>"),
        (SUP, "<sup>a</sup>", "<sup>a</sup>"),
        (SUB, "<sub>a</sub>", "<sub>a</sub>"),
    ];
    for (bit, md, html) in marked {
        let written = [md.to_string(), md.to_string(), html.to_string()];
        let plain = ["a".to_string(), "a".to_string(), "a".to_string()];
        for shape in [bit, bit + COPY] {
            // The style gives it.
            assert_eq!(paths(Some(bit), vec![run("a", shape)]), plain, "{bit} {shape}");
            // The style does not give it.
            assert_eq!(paths(Some(0), vec![run("a", shape)]), written, "{bit} {shape}");
            // No default to compare with.
            assert_eq!(paths(None, vec![run("a", shape)]), written, "{bit} {shape}");
            assert_eq!(paths(Some(UNKNOWN_STYLE), vec![run("a", shape)]), written, "{bit}");
        }
    }
    // What a format adds beyond the style is written alone.
    assert_eq!(
        paths(Some(BOLD), vec![run("a", BOLD | ITALIC)]),
        ["*a*".to_string(), "*a*".to_string(), "<em>a</em>".to_string()]
    );
    // A paragraph turning the default off is written as it is: the bold
    // run stays marked against the plain one.
    assert_eq!(
        paths(Some(BOLD), vec![run("a", BOLD), run("b", 0)]),
        ["**a**b".to_string(), "**a**b".to_string(), "<strong>a</strong>b".to_string()]
    );
    // A superscript style's subscript run turns the superscript off.
    assert_eq!(
        paths(Some(SUP), vec![run("a", SUB)]),
        ["<sub>a</sub>".to_string(), "<sub>a</sub>".to_string(), "<sub>a</sub>".to_string()]
    );
}

/// Each paragraph is compared with its own style, two paragraphs of one
/// table cell too.
/// 이것을 실패시키는 것: 기준을 첫 문단에서 한 번만 계산.
#[test]
fn each_paragraph_has_its_own_base() {
    let cell = vec![
        paragraph(0, Some(BOLD), vec![run("a", BOLD)]),
        paragraph(0, Some(0), vec![run("b", BOLD)]),
    ];
    let mut cell_table = merged(paragraph(0, None, vec![]));
    cell_table.rows[0].cells[0].paragraphs = cell;
    let md = markdown(vec![paragraph(0, None, vec![table(cell_table)])]);
    assert!(md.contains("a<br><strong>b</strong>"), "{md}");
}

/// A run of nothing or of whitespace alone shows no emphasis on any path,
/// so it turns no default off: the paragraph is still written against its
/// style. A run with characters that turns the default off still does.
/// 이것을 실패시키는 것: 공백·빈 run 을 해제로 보기 · 공백 run 을 판정에서 빼면서 글자 run 까지
/// 빼기 · HTML 에서 공백만 태그로 감싸기.
#[test]
fn whitespace_runs_turn_no_emphasis_off() {
    let three = |s: &str| [s.to_string(), s.to_string(), s.to_string()];
    assert_eq!(paths(Some(BOLD), vec![run("a", BOLD), run(" ", 0), run("b", BOLD)]), three("a b"));
    assert_eq!(paths(Some(BOLD), vec![run("a", BOLD), run("", 0), run("b", BOLD)]), three("ab"));
    // Whitespace alone with an emphasis of its own is written plain on
    // every path.
    assert_eq!(paths(Some(0), vec![run("a", 0), run(" ", BOLD), run("b", 0)]), three("a b"));
    assert_eq!(
        paths(Some(BOLD), vec![run("a", BOLD), run(" ", 0), run("c", 0)]),
        ["**a** c".to_string(), "**a** c".to_string(), "<strong>a</strong> c".to_string()]
    );
}

/// A property the styles do not know for the default counts as off: a run
/// with it on is written with it. The bold the default has is still not.
/// 이것을 실패시키는 것: 모르는 속성을 켜짐으로 보기.
#[test]
fn unknown_default_properties_are_off() {
    let md =
        markdown_with(vec![paragraph(0, Some(UNKNOWN), vec![run("a", BOLD | ITALIC)])], &Partial);
    assert_eq!(md, "*a*");
}
