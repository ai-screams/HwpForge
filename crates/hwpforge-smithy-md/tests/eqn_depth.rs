//! HancomEQN → LaTeX nesting-depth guard: stack safety and output contract.
//!
//! The equation parser recurses once per nesting level. A crafted script
//! (`a over a over …`, `{{{{…`, nested `matrix`/`cases`, nested subscripts)
//! used to overflow the stack and abort the whole process. The parser now
//! stops at depth 32 (counted as active `parse_one` calls) and the encoder
//! writes the original script, whitespace collapsed, after a fixed notice
//! instead of LaTeX.
//!
//! Every test that touches recursion runs on a thread with a 256 KiB stack
//! (the minimum supported stack target) and a wall-clock limit, so a stack
//! overflow aborts the test binary and an infinite loop reports a timeout —
//! both are failures, neither hangs the run.
//!
//! These tests live in `tests/` rather than as unit tests because stack use
//! depends on inlining, and the unit-test binary is compiled together with
//! all `#[cfg(test)]` code (different inlining than the shipped library).
//!
//! Run in all three profiles when touching the parser:
//! `cargo nextest run -p hwpforge-smithy-md --cargo-profile dev -E 'test(eqn_depth)'`,
//! the default test profile, and `--cargo-profile release`.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hwpforge_core::control::Control;
use hwpforge_core::{
    Document, PageSettings, Paragraph, Run, Section, StyleLookup, Table, TableCell, TableRow,
    Validated,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_md::MdEncoder;
use pulldown_cmark::{Event, Parser, Tag};

/// Minimum supported caller stack for to-md (design target).
const SMALL_STACK: usize = 256 * 1024;

/// Wall-clock limit per rendering; an infinite loop shows up as a timeout.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Parser depth limit under test (active `parse_one` calls).
const MAX_DEPTH: usize = 32;

/// The fixed notice, written literally so that dropping or changing it in
/// the encoder turns these tests red.
const NOTICE: &str = "[수식 변환 생략: 중첩 깊이 초과] ";

/// The notice as ordinary Markdown text renders it (`[`/`]` escaped).
const NOTICE_MD: &str = "\\[수식 변환 생략: 중첩 깊이 초과\\] ";

/// Style lookup with every property unset (plain text, no headings).
struct PlainStyles;

impl StyleLookup for PlainStyles {}

fn cs() -> CharShapeIndex {
    CharShapeIndex::new(0)
}

fn ps() -> ParaShapeIndex {
    ParaShapeIndex::new(0)
}

fn equation(script: &str) -> Run {
    Run::control(Control::equation(script), cs())
}

fn document(paragraphs: Vec<Paragraph>) -> Document<Validated> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    doc.validate().expect("test document validates")
}

/// Encodes a document holding one equation paragraph on a 256 KiB stack.
///
/// Panics (fails the test) when rendering does not finish within
/// [`TIMEOUT`]; a stack overflow aborts the test process.
fn render_on_small_stack(script: String) -> String {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("eqn-depth-small-stack".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let doc = document(vec![Paragraph::with_runs(vec![equation(&script)], ps())]);
            let out = MdEncoder::encode_styled(&doc, &PlainStyles);
            // The receiver may already have timed out; ignore send failure.
            let _ = tx.send(out.markdown);
        })
        .expect("spawn small-stack thread");
    rx.recv_timeout(TIMEOUT).expect("equation rendering did not finish (infinite loop?)")
}

fn assert_latex(markdown: &str) {
    assert!(
        markdown.starts_with('$') && markdown.ends_with('$'),
        "expected LaTeX: {markdown:.200}"
    );
    assert!(!markdown.contains("수식 변환 생략"), "unexpected fallback: {markdown:.200}");
}

fn assert_fallback(markdown: &str) {
    assert!(markdown.starts_with(NOTICE_MD), "expected fallback notice: {markdown:.200}");
}

// ── input builders: each returns a script whose deepest atom sits at
//    exactly `depth` active `parse_one` calls ─────────────────────────────

/// `a over a over … a` — `depth - 1` right-associative `over`s.
fn over_chain(depth: usize) -> String {
    format!("{}a", "a over ".repeat(depth - 1))
}

/// `{{…a…}}` — `depth - 1` nested brace groups.
fn nested_group(depth: usize) -> String {
    format!("{}a{}", "{".repeat(depth - 1), "}".repeat(depth - 1))
}

/// `matrix {matrix {… a …}}` — `depth - 1` nested matrices.
fn nested_matrix(depth: usize) -> String {
    format!("{}a{}", "matrix {".repeat(depth - 1), "}".repeat(depth - 1))
}

/// `cases {cases {… a …}}` — `depth - 1` nested cases.
fn nested_cases(depth: usize) -> String {
    format!("{}a{}", "cases {".repeat(depth - 1), "}".repeat(depth - 1))
}

/// `a_{a_{… a …}}` — `depth - 1` nested subscripts.
fn nested_subscript(depth: usize) -> String {
    format!("{}a{}", "a_{".repeat(depth - 1), "}".repeat(depth - 1))
}

/// `sum from {sum from {… a …}}` — `depth - 1` nested `from` limits
/// (the path with the most stack frames per counted level).
fn nested_from(depth: usize) -> String {
    format!("{}a{}", "sum from {".repeat(depth - 1), "}".repeat(depth - 1))
}

// ── deep inputs: no abort, no hang, fallback ─────────────────────────────

// 이것을 실패시키는 것: 깊이 검사 제거 (스택 오버플로로 테스트 프로세스 abort).
// (`over` 사슬에는 루프가 없어 플래그-후-계속 변이는 이 테스트를 못 죽인다 —
// 그 변이는 그룹·matrix·cases 테스트가 시간 초과로 잡는다.)
#[test]
fn eqn_depth_deep_over_chain_falls_back() {
    let script = format!("{}a", "a over ".repeat(10_000));
    assert_fallback(&render_on_small_stack(script));
}

// 이것을 실패시키는 것: 깊이 검사 제거 (스택 오버플로 abort),
// 그룹 루프의 `?` 를 플래그-후-계속으로 바꿈 (시간 초과).
#[test]
fn eqn_depth_deep_open_braces_falls_back() {
    let script = "{".repeat(100_000);
    assert_fallback(&render_on_small_stack(script));
}

// 이것을 실패시키는 것: 깊이 검사 제거 (스택 오버플로 abort),
// matrix/cases 행 루프의 `?` 를 플래그-후-계속으로 바꿈 (시간 초과).
#[test]
fn eqn_depth_deep_matrix_cases_subscript_mix_falls_back() {
    let unit = "matrix {a # cases {x ## y_{";
    let script = format!("{}z{}", unit.repeat(5_000), "}}}".repeat(5_000));
    assert_fallback(&render_on_small_stack(script));
}

// ── boundary: depth 32 → LaTeX, 33 → fallback, per recursion path ────────
// 이것을 실패시키는 것 (각 경로 공통): 깊이 비교 `>=` → `>` (33 이 LaTeX 로 바뀜).

#[test]
fn eqn_depth_boundary_over() {
    assert_latex(&render_on_small_stack(over_chain(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(over_chain(MAX_DEPTH + 1)));
}

#[test]
fn eqn_depth_boundary_group() {
    assert_latex(&render_on_small_stack(nested_group(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(nested_group(MAX_DEPTH + 1)));
}

#[test]
fn eqn_depth_boundary_matrix() {
    assert_latex(&render_on_small_stack(nested_matrix(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(nested_matrix(MAX_DEPTH + 1)));
}

#[test]
fn eqn_depth_boundary_cases() {
    assert_latex(&render_on_small_stack(nested_cases(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(nested_cases(MAX_DEPTH + 1)));
}

#[test]
fn eqn_depth_boundary_subscript() {
    assert_latex(&render_on_small_stack(nested_subscript(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(nested_subscript(MAX_DEPTH + 1)));
}

#[test]
fn eqn_depth_boundary_from_limit() {
    assert_latex(&render_on_small_stack(nested_from(MAX_DEPTH)));
    assert_fallback(&render_on_small_stack(nested_from(MAX_DEPTH + 1)));
}

/// Right-associative `over` is kept at the limit: the innermost fraction
/// holds the last two atoms.
#[test]
fn eqn_depth_over_chain_at_limit_stays_right_associative() {
    let markdown = render_on_small_stack(over_chain(MAX_DEPTH));
    // 31 `over`s → 31 fractions; the outer 30 each close one brace after
    // the innermost `\frac{a}{a}`.
    assert_eq!(markdown.matches("\\frac").count(), MAX_DEPTH - 1);
    assert!(markdown.starts_with("$\\frac{a}{\\frac{a}{"), "{markdown:.80}");
    let tail = format!("\\frac{{a}}{{a}}{}$", "}".repeat(MAX_DEPTH - 2));
    assert!(markdown.ends_with(&tail), "{markdown}");
}

// ── output contexts: the fallback is ordinary text in each context ───────

/// A script deeper than the limit whose text carries everything that needs
/// escaping: backtick runs, a pipe, an HTML tag, a newline, edge spaces.
fn hostile_deep_script() -> String {
    format!(
        "  ``code`` | <tag> *x* \nline2 {}a{}  ",
        "{".repeat(MAX_DEPTH + 5),
        "}".repeat(MAX_DEPTH + 5)
    )
}

/// The fallback's script part: every run of space, tab, CR, LF becomes one
/// space (written independently of the encoder as the expected value).
fn collapsed(script: &str) -> String {
    let parts: Vec<&str> = script.split([' ', '\t', '\n', '\r']).collect();
    let last = parts.len() - 1;
    // Keep the first and last (possibly empty) parts so edge whitespace
    // survives as one space; drop the empties between separators.
    parts
        .iter()
        .enumerate()
        .filter(|(i, part)| *i == 0 || *i == last || !part.is_empty())
        .map(|(_, part)| *part)
        .collect::<Vec<_>>()
        .join(" ")
}

/// One paragraph: `before `, then `middle`, then ` after`.
fn paragraph_with(middle: Run) -> Paragraph {
    Paragraph::with_runs(vec![Run::text("before ", cs()), middle, Run::text(" after", cs())], ps())
}

fn cell(paragraph: Paragraph, col_span: u16) -> TableCell {
    let width = HwpUnit::from_mm(30.0).expect("valid width");
    if col_span > 1 {
        TableCell::with_span(vec![paragraph], width, col_span, 1)
    } else {
        TableCell::new(vec![paragraph], width)
    }
}

fn plain_cell(text: &str, col_span: u16) -> TableCell {
    cell(Paragraph::with_runs(vec![Run::text(text, cs())], ps()), col_span)
}

/// A 2×2 GFM table whose bottom-left cell holds `middle`.
fn gfm_table_with(middle: Run) -> Paragraph {
    let table = Table::new(vec![
        TableRow::new(vec![plain_cell("H1", 1), plain_cell("H2", 1)]),
        TableRow::new(vec![cell(paragraph_with(middle), 1), plain_cell("x", 1)]),
    ]);
    Paragraph::with_runs(vec![Run::table(table, cs())], ps())
}

/// A table forced to HTML by a column span; the second row's first cell
/// holds `middle`.
fn html_table_with(middle: Run) -> Paragraph {
    let table = Table::new(vec![
        TableRow::new(vec![plain_cell("merged", 2)]),
        TableRow::new(vec![cell(paragraph_with(middle), 1), plain_cell("x", 1)]),
    ]);
    Paragraph::with_runs(vec![Run::table(table, cs())], ps())
}

/// Renders the paragraph built by `build` twice — once with the deep
/// equation, once with the expected fallback written as an ordinary text
/// run — on the small stack, and returns both outputs.
fn render_equation_and_text(build: fn(Run) -> Paragraph) -> (String, String) {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let script = hostile_deep_script();
            let with_eqn = document(vec![build(equation(&script))]);
            let expected = format!("{NOTICE}{}", collapsed(&script));
            let with_text = document(vec![build(Run::text(expected, cs()))]);
            let a = MdEncoder::encode_styled(&with_eqn, &PlainStyles).markdown;
            let b = MdEncoder::encode_styled(&with_text, &PlainStyles).markdown;
            let _ = tx.send((a, b));
        })
        .expect("spawn small-stack thread");
    rx.recv_timeout(TIMEOUT).expect("rendering did not finish")
}

// 이것을 실패시키는 것: 접두문구 제거, 폴백의 Markdown 이스케이프 생략,
// 공백 접기 제거.
#[test]
fn eqn_depth_fallback_is_plain_markdown_text() {
    let (eqn, text) = render_equation_and_text(paragraph_with);
    assert_eq!(eqn, text);
    assert!(eqn.contains(NOTICE_MD), "{eqn}");
    assert!(eqn.contains("\\`\\`code\\`\\`") && eqn.contains("&lt;tag&gt;"), "{eqn}");
}

// 이것을 실패시키는 것: 접두문구 제거, 폴백의 Markdown 이스케이프 생략,
// 공백 접기 제거.
#[test]
fn eqn_depth_fallback_is_gfm_cell_text() {
    let (eqn, text) = render_equation_and_text(gfm_table_with);
    assert_eq!(eqn, text);
    assert!(eqn.contains(NOTICE_MD), "{eqn}");
    assert!(eqn.contains(" \\| ") && eqn.contains("*x\\* line2"), "{eqn}");
}

// 이것을 실패시키는 것: 접두문구 제거, HTML 셀에서 폴백을 이스케이프 예외로 둠,
// 공백 접기 제거.
#[test]
fn eqn_depth_fallback_is_html_cell_text() {
    let (eqn, text) = render_equation_and_text(html_table_with);
    assert_eq!(eqn, text);
    assert!(eqn.contains(NOTICE.trim_end()), "{eqn}");
    assert!(eqn.contains("&lt;tag&gt;") && !eqn.contains("<tag>"), "{eqn}");
}

// ── block-syntax injection through the fallback text ─────────────────────

/// A script deeper than the limit followed by `payload`.
fn deep_script_with(payload: &str) -> String {
    format!("{}a{}{payload}", "{".repeat(MAX_DEPTH + 5), "}".repeat(MAX_DEPTH + 5))
}

fn render_doc_on_small_stack(
    build: impl FnOnce() -> Document<Validated> + Send + 'static,
) -> String {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let _ = tx.send(MdEncoder::encode_styled(&build(), &PlainStyles).markdown);
        })
        .expect("spawn small-stack thread");
    rx.recv_timeout(TIMEOUT).expect("rendering did not finish")
}

/// Block or link structure that a fallback payload must never produce.
fn injected_structure(markdown: &str) -> Vec<String> {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Rule => Some("rule".to_string()),
            Event::Start(Tag::List(_)) => Some("list".to_string()),
            Event::Start(Tag::Item) => Some("item".to_string()),
            Event::Start(Tag::Image { dest_url, .. }) => Some(format!("image {dest_url}")),
            Event::Start(Tag::Heading { .. }) => Some("heading".to_string()),
            _ => None,
        })
        .collect()
}

// 이것을 실패시키는 것: 폴백의 공백 접기 제거 (빈 줄 뒤 `---`·`- ` 가 구분선·목록이 됨).
#[test]
fn eqn_depth_fallback_cannot_inject_blocks_in_markdown() {
    let script = deep_script_with("\n\n---\n\n- injected");
    let markdown = render_doc_on_small_stack(move || {
        document(vec![Paragraph::with_runs(vec![equation(&script)], ps())])
    });
    assert!(markdown.starts_with(NOTICE_MD), "{markdown}");
    assert!(!markdown.contains('\n'), "fallback must be one line: {markdown:?}");
    assert_eq!(injected_structure(&markdown), Vec::<String>::new(), "{markdown}");
}

// 이것을 실패시키는 것: 폴백의 공백 접기 제거 (빈 줄이 HTML 표 블록을 끝내 이미지가 됨).
#[test]
fn eqn_depth_fallback_cannot_inject_blocks_in_html_cell() {
    let script = deep_script_with("\n\n![x](http://e/x.png)\n\nz");
    let markdown =
        render_doc_on_small_stack(move || document(vec![html_table_with(equation(&script))]));
    let line = markdown
        .lines()
        .find(|line| line.contains(NOTICE.trim_end()))
        .unwrap_or_else(|| panic!("fallback notice missing: {markdown}"));
    assert!(line.contains("![x](http://e/x.png) z"), "fallback must stay on one line: {markdown}");
    assert_eq!(injected_structure(&markdown), Vec::<String>::new(), "{markdown}");
}
