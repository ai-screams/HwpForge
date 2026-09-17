//! The committed synthetic-face HWPX fixture: its generator and its proof.
//!
//! # What the fixture is for
//!
//! Every other `to_pdf` success test in this repo either needs the Hancom font
//! bundle or retypes a Hancom-authored fixture at test time. Neither helps a
//! *consumer* crate that just wants a document it can render: the Python
//! bindings need one file, committed, that renders on any checkout with
//! nothing installed.
//!
//! `crates/hwpforge-bindings-py/tests/fixtures/synthetic_face.hwpx` is that
//! file. Every face it names is `"HwpForge Test"`, the committed synthetic
//! family in `crates/hwpforge-smithy-pdf/tests/fonts/`, and every character it
//! contains is one that family actually draws — so it renders under the
//! **default** (fatal) failure mode, with no `degraded` opt-in and no
//! `MISSING_GLYPHS`.
//!
//! # How it is generated
//!
//! ```text
//! UPDATE_FIXTURE=1 cargo nextest run -p hwpforge-convert --no-capture \
//!     -E 'test(the_fixture_is_what_the_generator_produces)'
//! ```
//!
//! The generator takes a real Hancom-saved package's *style store* (so the
//! char shapes, para shapes and border fills are genuine), renames every face
//! to the synthetic family, and re-encodes it around a document this file
//! builds: paragraphs and table cells that carry explicit layout caches,
//! because the renderer replays a cache rather than laying text out itself.
//!
//! Without `UPDATE_FIXTURE` the same test asserts the committed bytes still
//! match what the generator produces, so the recipe cannot rot unnoticed.

use std::path::{Path, PathBuf};

use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::layout::{LayoutCache, LineSeg};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder};

/// The committed synthetic family, as its `name` table declares it.
const TEST_FACE: &str = "HwpForge Test";

/// The fixture's path, relative to the workspace root.
const FIXTURE: &str = "crates/hwpforge-bindings-py/tests/fixtures/synthetic_face.hwpx";

/// The style donor: a real Hancom-saved package, used for its style store
/// only. Nothing of its text or layout survives into the fixture.
const STYLE_DONOR: &str = "tests/fixtures/pdf-rules/rules-justify.hwpx";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

/// The committed synthetic faces smithy-pdf renders its own e2e with.
fn test_font_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hwpforge-smithy-pdf/tests/fonts")
}

// ── the document ────────────────────────────────────────────────

/// One cached line. The metrics are smithy-pdf's own test values: the
/// synthetic family is 1.0em per Hangul syllable, 0.6em per Latin glyph and
/// 0.3em per space at 10pt, so a 1000-unit line with 600 of spacing is a
/// faithful single line of text.
fn seg(textpos: u32, vertpos: i32) -> LineSeg {
    LineSeg {
        textpos,
        vertpos,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 42520,
        flags: 0,
    }
}

/// A paragraph with one run and an explicit layout cache.
fn cached(text: &str, lines: Vec<LineSeg>) -> Paragraph {
    let mut paragraph =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    paragraph.layout_cache = Some(LayoutCache::new(lines));
    paragraph
}

/// One table cell: a single cached paragraph, an explicit width and height.
fn cell(text: &str, width: i32, height: i32) -> TableCell {
    TableCell::new(vec![cached(text, vec![seg(0, 0)])], HwpUnit::new(width).expect("cell width"))
        .with_height(HwpUnit::new(height).expect("cell height"))
}

/// The document the fixture carries: three paragraphs around a 2×2 table.
///
/// Every character is one the synthetic family draws — Latin letters, digits,
/// `.,:-`, the space, and the fourteen Hangul syllables 가나다라마바사아자차카타파하.
/// A character outside that set would render as tofu and, under the default
/// fatal mode, fail the render outright.
fn paragraphs() -> Vec<Paragraph> {
    const ROW_HEIGHT: i32 = 1200;
    const COL_WIDTH: i32 = 20000;
    /// Where the flow resumes after the table.
    ///
    /// Not `host_v + 2 * ROW_HEIGHT`: the replay adds the cells' own margins
    /// and border widths, and it refuses a following paragraph whose cached
    /// `v` disagrees with what it computed (`InvalidCache`, which names the
    /// number it wanted). 6330 is that number, measured rather than derived —
    /// a cache that lies about where the table ended is exactly the kind of
    /// drift this fixture must not carry.
    const TABLE_FLOW_END: i32 = 6330;

    let table = Table::new(vec![
        TableRow::with_height(
            vec![cell("가나", COL_WIDTH, ROW_HEIGHT), cell("다라", COL_WIDTH, ROW_HEIGHT)],
            HwpUnit::new(ROW_HEIGHT).expect("row height"),
        ),
        TableRow::with_height(
            vec![cell("마바", COL_WIDTH, ROW_HEIGHT), cell("사아", COL_WIDTH, ROW_HEIGHT)],
            HwpUnit::new(ROW_HEIGHT).expect("row height"),
        ),
    ])
    .with_width(HwpUnit::new(COL_WIDTH * 2).expect("table width"));

    // The host line of an inline table is as tall as the table it hosts: that
    // is how the replay knows the table consumes the flow rather than being
    // anchored beside it.
    let mut host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut host_line = seg(0, 3200);
    host_line.vertsize = ROW_HEIGHT * 2;
    host_line.textheight = ROW_HEIGHT * 2;
    host.layout_cache = Some(LayoutCache::new(vec![host_line]));

    vec![
        cached("HwpForge Test 2026", vec![seg(0, 0)]),
        cached("가나다라 마바사 아자차카", vec![seg(0, 1600)]),
        host,
        cached("ABCdef 0123456789", vec![seg(0, TABLE_FLOW_END)]),
    ]
}

/// Builds the fixture bytes.
fn generate() -> Vec<u8> {
    let donor = std::fs::read(workspace_root().join(STYLE_DONOR)).expect("style donor");
    let decoded = HwpxDecoder::decode(&donor).expect("decode the style donor");

    // The store is genuine Hancom material; only the face names change.
    let mut store = decoded.style_store;
    let faces: Vec<String> = store.iter_fonts().map(|font| font.face_name.clone()).collect();
    for face in faces {
        if face != TEST_FACE {
            store.replace_font(&face, TEST_FACE);
        }
    }

    let page_settings = decoded.document.sections()[0].page_settings;
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(paragraphs(), page_settings));
    let validated = document.validate().expect("validate the synthetic document");

    HwpxEncoder::encode_with_diagnostics(
        &validated,
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect("encode the synthetic document")
    .bytes
}

// ── the tests ───────────────────────────────────────────────────

#[test]
fn the_fixture_is_what_the_generator_produces() {
    let path = workspace_root().join(FIXTURE);
    let generated = generate();

    if std::env::var_os("UPDATE_FIXTURE").is_some() {
        std::fs::create_dir_all(path.parent().expect("fixture directory"))
            .expect("create the fixture directory");
        std::fs::write(&path, &generated).expect("write the fixture");
        eprintln!("UPDATED {} ({} bytes)", path.display(), generated.len());
        return;
    }

    let committed = std::fs::read(&path).unwrap_or_else(|err| {
        panic!("{}: {err} — regenerate with UPDATE_FIXTURE=1", path.display())
    });
    assert_eq!(
        committed, generated,
        "the committed fixture no longer matches the generator — \
         regenerate with UPDATE_FIXTURE=1 if the change is intended"
    );
}

#[test]
fn the_fixture_renders_with_the_committed_fonts_and_nothing_else() {
    let bytes = std::fs::read(workspace_root().join(FIXTURE)).expect("the committed fixture");

    // Default options: fatal failure mode, `ExplicitOnly` discovery. The only
    // thing supplied is the committed font directory, so a machine with no
    // Hancom install renders exactly the same bytes.
    let output = to_pdf(&bytes, &ToPdfOptions::default().with_font_dirs(vec![test_font_dir()]))
        .expect("render");

    assert!(output.bytes.starts_with(b"%PDF-"), "PDF header");
    assert!(output.pages >= 1, "pages = {}", output.pages);
    assert!(output.bytes.len() > 1_000, "real content ({} bytes)", output.bytes.len());

    // Nothing was degraded, skipped or left unclassified. Fatal mode already
    // refuses a missing glyph; this says the render was clean as well as
    // successful, which is what makes the fixture usable as a *success* input.
    let codes: Vec<String> = output.warnings.iter().map(|w| w.info().code).collect();
    assert!(codes.is_empty(), "the fixture renders without a single warning: {codes:?}");
}

#[test]
fn the_fixture_carries_the_table_that_makes_it_worth_committing() {
    // The table is the part a hand-written Core document usually gets wrong:
    // it only replays when the decoded package gives it back a layout cache
    // and a default-flow position. If a future regeneration dropped it, the
    // render test above would still pass, so the structure is asserted here.
    let bytes = std::fs::read(workspace_root().join(FIXTURE)).expect("the committed fixture");
    let decoded = HwpxDecoder::decode(&bytes).expect("decode the fixture");

    let mut tables = 0;
    let mut cells = 0;
    for section in decoded.document.sections() {
        section.for_each_paragraph(|paragraph| {
            for run in &paragraph.runs {
                if let hwpforge_core::run::RunContent::Table(table) = &run.content {
                    tables += 1;
                    cells += table.rows.iter().map(|row| row.cells.len()).sum::<usize>();
                    assert!(
                        table.layout_cache.is_some_and(|cache| cache.default_flow_pos),
                        "the decoder must give the table back a default-flow layout cache"
                    );
                }
            }
        });
    }
    assert_eq!(tables, 1, "one table");
    assert_eq!(cells, 4, "a 2×2 grid");
}

#[test]
fn the_fixture_names_only_the_synthetic_face() {
    let bytes = std::fs::read(workspace_root().join(FIXTURE)).expect("the committed fixture");
    let decoded = HwpxDecoder::decode(&bytes).expect("decode the fixture");

    let faces: Vec<String> =
        decoded.style_store.iter_fonts().map(|f| f.face_name.clone()).collect();
    assert!(!faces.is_empty(), "the fixture declares fonts");
    for face in &faces {
        assert_eq!(face, TEST_FACE, "an installed font would be needed for {face:?}");
    }
}
