//! The `*_with_diagnostics` twins: warnings survive, payloads do not change.
//!
//! Every facade in this crate that decodes internally used to drop the
//! decoder's warning list, which made a `warnings` channel above it
//! permanently empty (W1b review R2, findings 1–3). The twins added for that
//! review carry the diagnostics; the original entry points stay as thin
//! wrappers over them.
//!
//! Two things are worth proving, and each test does one of them:
//!
//! 1. the twin really reports the warnings the codec produced, and
//! 2. the plain entry point still returns exactly what it used to.
//!
//! Point 2 is what makes this an additive change to a published crate, so it
//! is asserted against the twin's own payload rather than against a snapshot.

use hwpforge_smithy_hwpx::stamp::HwpxStamper;
use hwpforge_smithy_hwpx::{
    DecodeWarning, HwpxCellEditor, HwpxDiffer, HwpxFiller, HwpxPatcher, HwpxReader,
};

/// A committed fixture whose decode raises `LayoutCacheDropped`.
///
/// The same fixture backs `ops_to_json`'s decode-warning test, so the two
/// layers cannot disagree about what "a document that warns" means.
fn warning_fixture() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/user_samples/sample-text-char-runs-basic.hwpx"
    );
    std::fs::read(path).unwrap_or_else(|e| panic!("warning fixture: {e}"))
}

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn is_cache_dropped(warning: &DecodeWarning) -> bool {
    matches!(warning, DecodeWarning::LayoutCacheDropped { .. })
}

/// Compares two payloads by their serialised form.
///
/// Several of these projections are `Serialize`-only upstream, and adding
/// `PartialEq` to a published type just to write a test would be a change to
/// the very surface this work is trying to leave alone. The wire shape is
/// also the thing consumers actually depend on.
fn same_payload<T: serde::Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).expect("serialise")
        == serde_json::to_value(right).expect("serialise")
}

/// A 2×2 label/value table package this codec wrote itself.
///
/// The cell editor admits an input only when a no-op decode→encode→decode
/// reproduces it, so a freshly encoded package is the reliable way to reach
/// the edit path in a test.
fn admissible_table_package() -> Vec<u8> {
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{Document, PageSettings, Paragraph};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    use hwpforge_smithy_hwpx::HwpxEncoder;

    let para = |text: &str| {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    };
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::new(vec![para("성명")], width),
            TableCell::new(vec![para("")], width),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![para("소속")], width),
            TableCell::new(vec![para("")], width),
        ]),
    ]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));

    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// Guards the premise of every other test here: the fixture still warns.
///
/// If a decoder improvement ever stops dropping this cache, the tests below
/// would keep passing vacuously. This one fails instead, and says why.
#[test]
fn the_warning_fixture_still_produces_a_decode_warning() {
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&warning_fixture()).expect("decode");

    assert!(
        decoded.warnings.iter().any(is_cache_dropped),
        "the twins' premise is gone: {:?}",
        decoded.warnings
    );
}

#[test]
fn outline_reports_decode_warnings_and_keeps_its_map() {
    let bytes = warning_fixture();

    let diagnosed = HwpxReader::outline_with_diagnostics(&bytes).expect("outline");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    // The field walk re-decodes the same bytes; its warnings are the same
    // ones and must not be appended a second time.
    assert_eq!(
        diagnosed.warnings.len(),
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode").warnings.len(),
        "each decode warning must appear exactly once"
    );
    assert!(same_payload(&HwpxReader::outline(&bytes).expect("outline"), &diagnosed.value));
}

#[test]
fn read_paragraphs_reports_decode_warnings_and_keeps_its_view() {
    let bytes = warning_fixture();

    let diagnosed =
        HwpxReader::read_paragraphs_with_diagnostics(&bytes, 0, None).expect("read_paragraphs");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    assert!(same_payload(
        &HwpxReader::read_paragraphs(&bytes, 0, None).expect("read_paragraphs"),
        &diagnosed.value
    ));
}

#[test]
fn list_fields_reports_decode_warnings_and_keeps_its_list() {
    let bytes = warning_fixture();

    let diagnosed = HwpxFiller::list_fields_with_diagnostics(&bytes).expect("list_fields");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    assert!(same_payload(&HwpxFiller::list_fields(&bytes).expect("list_fields"), &diagnosed.value));
}

#[test]
fn export_section_reports_decode_warnings_and_keeps_its_outcome() {
    let bytes = warning_fixture();

    let diagnosed = HwpxPatcher::export_section_for_edit_with_diagnostics(&bytes, 0, true)
        .expect("export_section");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    let plain = HwpxPatcher::export_section_for_edit(&bytes, 0, true).expect("export_section");
    assert_eq!(plain.exported.section_index, diagnosed.value.exported.section_index);
    assert_eq!(plain.warning, diagnosed.value.warning);
}

#[test]
fn the_stamp_planner_reports_decode_warnings_and_keeps_its_plan() {
    let bytes = warning_fixture();

    let diagnosed = HwpxStamper::plan_bytes_v2_with_diagnostics(&bytes).expect("plan");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    let plain = HwpxStamper::plan_bytes_v2(&bytes).expect("plan");
    assert_eq!(plain.source_sha256, diagnosed.value.source_sha256);
    assert_eq!(plain.text.len(), diagnosed.value.text.len());
    assert_eq!(plain.cells.len(), diagnosed.value.cells.len());
}

/// A diff decodes two packages, so the warnings are kept apart by input.
#[test]
fn diff_attributes_each_decode_warning_to_the_input_it_came_from() {
    let warns = warning_fixture();
    let quiet = fixture("SimpleTable.hwpx");

    let both = HwpxDiffer::diff_with_diagnostics(&warns, &warns).expect("diff");
    assert!(both.base_warnings.iter().any(is_cache_dropped), "{:?}", both.base_warnings);
    assert!(both.revised_warnings.iter().any(is_cache_dropped), "{:?}", both.revised_warnings);

    // Only the base warns here, which is what proves the two lists are not
    // the same list reported twice.
    let base_only = HwpxDiffer::diff_with_diagnostics(&warns, &quiet).expect("diff");
    assert!(base_only.base_warnings.iter().any(is_cache_dropped));
    assert!(
        base_only.revised_warnings.is_empty(),
        "the quiet side must stay quiet: {:?}",
        base_only.revised_warnings
    );

    let revised_only = HwpxDiffer::diff_with_diagnostics(&quiet, &warns).expect("diff");
    assert!(revised_only.base_warnings.is_empty());
    assert!(revised_only.revised_warnings.iter().any(is_cache_dropped));

    assert!(same_payload(&HwpxDiffer::diff(&warns, &warns).expect("diff"), &both.diff));
}

/// The regenerating editors' twin returns the same package as before.
///
/// The warning list is empty here and that is not an oversight: the only
/// non-semantic `EncodeWarning` is `LayoutCacheDropped`, which the encoder
/// raises only when `EncodeOptions::emit_layout_cache` is set — and a
/// preserve-first editor must never set it. The pass-through itself is
/// proven in `encoder::tests::split_successful_encode`, where the outcome
/// can be constructed directly.
#[test]
fn set_cells_twin_returns_the_same_package_as_the_plain_entry_point() {
    // Built and encoded here rather than read from disk: a package this
    // codec produced is round-trip safe by construction, which is what the
    // editor's admission gate requires. `SimpleTable.hwpx` is a Hancom
    // package and is rejected before any encode happens.
    let bytes = admissible_table_package();
    let specs = vec![hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            0, 1,
        )),
        text: "diagnostics".to_string(),
    }];

    let diagnosed = HwpxCellEditor::set_cells_with_diagnostics(&bytes, &specs).expect("set_cells");
    let plain = HwpxCellEditor::set_cells(&bytes, &specs).expect("set_cells");

    assert_eq!(plain.outcome, diagnosed.value.outcome, "the twin must not change what was edited");
    // Compared through the decoded grid rather than byte for byte: this
    // codec's packages are not byte-reproducible across two runs (a ZIP
    // header field differs), which predates this change.
    assert!(
        same_payload(
            &HwpxReader::read_table(&plain.bytes, 0).expect("read_table"),
            &HwpxReader::read_table(&diagnosed.value.bytes, 0).expect("read_table")
        ),
        "the twin must not change the output document"
    );
    assert!(
        diagnosed.encode_warnings.is_empty(),
        "no committed fixture reaches a non-semantic encode warning: {:?}",
        diagnosed.encode_warnings
    );
    assert!(
        diagnosed.decode_warnings.is_empty(),
        "this package is this codec's own clean output: {:?}",
        diagnosed.decode_warnings
    );
}

// ── the editing twins (W1b review R3) ───────────────────────────
//
// `fill`, `insert_paragraphs`, `delete_paragraphs` and the stamper's
// admission decode were the entry points still dropping the decoder's
// warnings after the R2 pass. They all need one fixture that both warns and
// survives the admission gate, which the two native warning documents do not:
// each fails the gate on uncarried ZIP entries.

/// A package that warns on decode **and** is admissible.
///
/// This codec's own output with the last paragraph's `<hp:linesegarray>`
/// pointing past the end of that paragraph's text — the shape a third-party
/// edit leaves behind. The decoder refuses to promote a guessed coordinate
/// and reports the drop instead. It also carries a named, fillable
/// click-here field, an index mark and one `(   )` stamp candidate, so the
/// same document reaches all four editing paths.
fn editable_warning_fixture() -> Vec<u8> {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/stale-line-cache.hwpx");
    std::fs::read(path).unwrap_or_else(|e| panic!("editable warning fixture: {e}"))
}

#[test]
fn the_editable_warning_fixture_still_produces_a_decode_warning() {
    let decoded =
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&editable_warning_fixture()).expect("decode");

    assert_eq!(
        decoded.warnings.iter().filter(|w| is_cache_dropped(w)).count(),
        1,
        "the fixture is the premise of the tests below: {:?}",
        decoded.warnings
    );
}

#[test]
fn fill_reports_decode_warnings_and_keeps_its_outcome() {
    let bytes = editable_warning_fixture();
    let values: std::collections::BTreeMap<String, String> =
        [("user_email".to_string(), "hanyul@example.com".to_string())].into_iter().collect();

    let diagnosed = HwpxFiller::fill_with_diagnostics(&bytes, &values).expect("fill twin");
    let plain = HwpxFiller::fill(&bytes, &values).expect("fill");

    assert!(diagnosed.warnings.iter().any(is_cache_dropped), "{:?}", diagnosed.warnings);
    assert_eq!(
        diagnosed.warnings.len(),
        1,
        "the preserving patcher re-decodes per touched section; that must not multiply the list"
    );
    assert!(same_payload(&plain.filled, &diagnosed.value.filled), "the twin fills the same fields");
    assert!(
        same_payload(
            &HwpxFiller::list_fields(&plain.bytes).expect("fields"),
            &HwpxFiller::list_fields(&diagnosed.value.bytes).expect("fields")
        ),
        "the twin must not change the output document"
    );
}

#[test]
fn the_structural_twins_report_decode_warnings_and_keep_their_bytes() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};

    let bytes = editable_warning_fixture();
    let anchor = ParagraphLocator { section: 0, index: 2 };
    let texts = vec!["끼움".to_string()];

    let inserted = HwpxStructuralEditor::insert_paragraphs_with_diagnostics(
        &bytes,
        anchor,
        InsertPosition::After,
        &texts,
    )
    .expect("insert twin");
    let inserted_plain =
        HwpxStructuralEditor::insert_paragraphs(&bytes, anchor, InsertPosition::After, &texts)
            .expect("insert");

    assert!(inserted.warnings.iter().any(is_cache_dropped), "{:?}", inserted.warnings);
    assert_eq!(inserted.warnings.len(), 1, "only the input decode is reported");
    assert_eq!(inserted_plain, inserted.value, "the twin must not change the bytes");

    let targets = [ParagraphLocator { section: 0, index: 3 }];
    let deleted = HwpxStructuralEditor::delete_paragraphs_with_diagnostics(&bytes, &targets)
        .expect("delete twin");
    let deleted_plain = HwpxStructuralEditor::delete_paragraphs(&bytes, &targets).expect("delete");

    assert!(deleted.warnings.iter().any(is_cache_dropped), "{:?}", deleted.warnings);
    assert_eq!(deleted.warnings.len(), 1, "only the input decode is reported");
    assert_eq!(deleted_plain, deleted.value, "the twin must not change the bytes");
}

/// Editing nothing decodes nothing, so the no-op returns an empty list rather
/// than the warnings of a decode it never ran.
#[test]
fn the_structural_no_ops_report_nothing_because_they_never_decode() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};

    let bytes = editable_warning_fixture();
    let anchor = ParagraphLocator { section: 0, index: 0 };

    let inserted = HwpxStructuralEditor::insert_paragraphs_with_diagnostics(
        &bytes,
        anchor,
        InsertPosition::After,
        &[],
    )
    .expect("insert no-op");
    let deleted = HwpxStructuralEditor::delete_paragraphs_with_diagnostics(&bytes, &[])
        .expect("delete no-op");

    assert!(inserted.warnings.is_empty(), "{:?}", inserted.warnings);
    assert!(deleted.warnings.is_empty(), "{:?}", deleted.warnings);
    assert_eq!(inserted.value, bytes, "a no-op is byte identical");
    assert_eq!(deleted.value, bytes, "a no-op is byte identical");
}

/// The stamper's twin carries the **input** decode, not the output re-read
/// that builds the manifest, and keeps the encode channel separate.
#[test]
fn the_stamp_twin_reports_the_input_decode_beside_the_encode_channel() {
    use hwpforge_smithy_hwpx::stamp::{StampAction, StampSpec};

    let bytes = editable_warning_fixture();
    let specs: Vec<StampSpec> = HwpxStamper::plan_bytes_v2(&bytes)
        .expect("plan")
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker,
            action: StampAction::Field { name: "성명".to_string(), hint: None },
        })
        .collect();
    assert_eq!(specs.len(), 1, "the fixture offers exactly one candidate");

    let diagnosed = HwpxStamper::stamp_with_diagnostics(&bytes, &specs).expect("stamp twin");
    let plain = HwpxStamper::stamp(&bytes, &specs).expect("stamp");

    assert!(
        diagnosed.decode_warnings.iter().any(is_cache_dropped),
        "{:?}",
        diagnosed.decode_warnings
    );
    assert_eq!(
        diagnosed.decode_warnings.len(),
        1,
        "the gate, the fixed point and the manifest decode too; none of those is reported"
    );
    assert!(
        diagnosed.encode_warnings.is_empty(),
        "no committed fixture reaches a non-semantic encode warning: {:?}",
        diagnosed.encode_warnings
    );
    // Compared through the decoded fields rather than the manifest: the
    // manifest embeds a hash of the output, and this codec's
    // packages are not byte-reproducible across two runs (a ZIP header field
    // differs), which predates this change — the same reason the cell
    // editor's twin test compares a decoded projection.
    assert!(
        same_payload(
            &HwpxFiller::list_fields(&plain.bytes).expect("fields"),
            &HwpxFiller::list_fields(&diagnosed.value.bytes).expect("fields")
        ),
        "the twin must not change the output document"
    );
}

/// The cell editor's twin carries the input decode beside the encode
/// channel, exactly as the stamper's does.
#[test]
fn the_set_cells_twin_reports_the_input_decode_beside_the_encode_channel() {
    let bytes = editable_warning_fixture();
    let specs = vec![hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            0, 1,
        )),
        text: "류한율".to_string(),
    }];

    let diagnosed = HwpxCellEditor::set_cells_with_diagnostics(&bytes, &specs).expect("twin");
    let plain = HwpxCellEditor::set_cells(&bytes, &specs).expect("set_cells");

    assert!(
        diagnosed.decode_warnings.iter().any(is_cache_dropped),
        "{:?}",
        diagnosed.decode_warnings
    );
    assert_eq!(
        diagnosed.decode_warnings.len(),
        1,
        "the gate re-decodes its own no-op encode; that must not be reported too"
    );
    assert!(
        diagnosed.encode_warnings.is_empty(),
        "no committed fixture reaches a non-semantic encode warning: {:?}",
        diagnosed.encode_warnings
    );
    assert_eq!(plain.outcome, diagnosed.value.outcome, "the twin must not change what was edited");
}
