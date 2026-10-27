use shared::fixtures::test_font_dir;

#[path = "support/synthetic_face_fixture.rs"]
mod shared;
use shared::*;

use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
use hwpforge_smithy_hwpx::HwpxDecoder;

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
