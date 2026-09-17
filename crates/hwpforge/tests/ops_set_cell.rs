//! `ops::edit::set_cell` — the regenerating, grid-addressed cell edit.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::grid::GridCoord;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{CellResolution, CellSpec, CellTarget, HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::OpsError;

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn width() -> HwpUnit {
    HwpUnit::new(8000).expect("width")
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

fn section_of(paragraphs: Vec<Paragraph>) -> Section {
    Section::with_paragraphs(paragraphs, PageSettings::a4())
}

fn host_of(table: Table) -> Paragraph {
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    host
}

/// A plain 2×2 label/value table.
fn labelled_table() -> Vec<u8> {
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::new(vec![text_para("성명")], width()),
            TableCell::new(vec![text_para("")], width()),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![text_para("소속")], width()),
            TableCell::new(vec![text_para("")], width()),
        ]),
    ]);
    let mut doc = Document::new();
    doc.add_section(section_of(vec![host_of(table)]));
    encode(doc)
}

/// A 2×2 table whose first column is one cell spanning both rows, so (1,0)
/// is a covered position.
fn merged_table() -> Vec<u8> {
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::with_span(vec![text_para("병합")], width(), 1, 2),
            TableCell::new(vec![text_para("B")], width()),
        ]),
        TableRow::new(vec![TableCell::new(vec![text_para("C")], width())]),
    ]);
    let mut doc = Document::new();
    doc.add_section(section_of(vec![host_of(table)]));
    encode(doc)
}

/// A table document that also carries a footnote whose body is a heading.
///
/// Re-encoding such a document drops the note's number head, which
/// `EncodeWarning::is_semantic_loss` classifies as a semantic loss — the
/// exact case a regenerating edit must fail closed on.
fn table_with_title_mark_note() -> Vec<u8> {
    let mut heading_body = text_para("제목 각주");
    heading_body.heading_level = Some(1);
    let note = Paragraph::with_runs(
        vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let table =
        Table::new(vec![TableRow::new(vec![TableCell::new(vec![text_para("셀")], width())])]);
    let mut doc = Document::new();
    doc.add_section(section_of(vec![note, host_of(table)]));
    encode(doc)
}

/// Every cell text of the first table, row-major.
fn cell_texts(bytes: &[u8]) -> Vec<String> {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    let mut out = Vec::new();
    decoded.document.sections()[0].for_each_paragraph(|paragraph| {
        for run in &paragraph.runs {
            if let hwpforge::core::run::RunContent::Table(table) = &run.content {
                for row in &table.rows {
                    for cell in &row.cells {
                        out.push(
                            cell.paragraphs.iter().map(Paragraph::text_content).collect::<String>(),
                        );
                    }
                }
            }
        }
    });
    out
}

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn replaces_the_text_of_a_coordinate_addressed_cell() {
    let bytes = labelled_table();

    let out = set_cell(
        &bytes,
        &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"),
    )
    .expect("set_cell");

    assert_eq!(out.results.len(), 1);
    assert_eq!(out.results[0].table, 0);
    assert_eq!(out.results[0].requested, GridCoord::new(0, 1));
    assert_eq!(out.results[0].anchor, GridCoord::new(0, 1));
    assert_eq!(out.results[0].resolution, CellResolution::Exact);
    assert!(!out.results[0].cleared);
    assert_eq!(cell_texts(&out.bytes), ["성명", "류한율", "소속", ""]);
    // A successful regenerating edit now reports the encode's non-semantic
    // warnings instead of a hard-coded empty list. This document produces
    // none — see `a_successful_edit_reports_the_encoders_nonsemantic_warnings`
    // for why an empty list here is the right answer and not a dropped one.
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn addresses_a_cell_by_the_label_to_its_left() {
    let out = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_right_of("소속").with_text("제노스큐브"),
    )
    .expect("set_cell");

    assert_eq!(out.results[0].anchor, GridCoord::new(1, 1));
    assert_eq!(cell_texts(&out.bytes), ["성명", "", "소속", "제노스큐브"]);
}

#[test]
fn addresses_a_cell_by_the_label_above_it() {
    let out = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_below("성명").with_text("부서"),
    )
    .expect("set_cell");

    assert_eq!(out.results[0].anchor, GridCoord::new(1, 0));
    assert_eq!(cell_texts(&out.bytes), ["성명", "", "부서", ""]);
}

#[test]
fn a_covered_coordinate_resolves_to_its_merge_anchor() {
    let out = set_cell(
        &merged_table(),
        &SetCellOptions::default().with_table(0).with_at("1,0").with_text("새 값"),
    )
    .expect("set_cell");

    assert_eq!(out.results[0].requested, GridCoord::new(1, 0), "the request is reported verbatim");
    assert_eq!(out.results[0].anchor, GridCoord::new(0, 0), "the anchor is what was edited");
    assert_eq!(out.results[0].resolution, CellResolution::CoveredToAnchor);
    assert_eq!(cell_texts(&out.bytes), ["새 값", "B", "C"]);
}

#[test]
fn an_empty_text_clears_the_cell_and_says_so() {
    let out = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_at("1,0").with_text(""),
    )
    .expect("set_cell");

    assert!(out.results[0].cleared);
    assert_eq!(cell_texts(&out.bytes), ["성명", "", "", ""]);
}

#[test]
fn a_batch_applies_every_spec_in_order() {
    let specs = vec![
        CellSpec {
            table: 0, target: CellTarget::At(GridCoord::new(0, 1)), text: "류한율".into()
        },
        CellSpec { table: 0, target: CellTarget::Below("성명".into()), text: "아래".into() },
    ];

    let out = set_cell(&labelled_table(), &SetCellOptions::default().with_specs(specs))
        .expect("set_cell");

    assert_eq!(out.results.len(), 2);
    assert_eq!(out.results[0].anchor, GridCoord::new(0, 1));
    assert_eq!(out.results[1].anchor, GridCoord::new(1, 0));
    assert_eq!(cell_texts(&out.bytes), ["성명", "류한율", "아래", ""]);
}

#[test]
fn an_unusable_option_combination_is_rejected_before_decoding() {
    let cases = [
        (
            SetCellOptions::default().with_at("0,0").with_text("v"),
            OpsCode::InvalidSetCellArgs,
            "table is required",
        ),
        (
            SetCellOptions::default().with_table(0).with_at("0,0"),
            OpsCode::InvalidSetCellArgs,
            "text is required",
        ),
        (
            SetCellOptions::default().with_table(0).with_text("v"),
            OpsCode::InvalidSetCellArgs,
            "exactly one",
        ),
        (
            SetCellOptions::default().with_table(0).with_at("nope").with_text("v"),
            OpsCode::InvalidSetCellArgs,
            "row,col",
        ),
        // A batch that parses but edits nothing is a map problem, not an
        // argument problem, and the frontends keep the two codes apart.
        (
            SetCellOptions::default().with_specs(Vec::new()),
            OpsCode::InvalidSetCellMap,
            "spec list is empty",
        ),
    ];

    for (opts, code, fragment) in cases {
        let err = set_cell(b"not a document", &opts).expect_err("must reject");
        assert_eq!(err.code(), code, "{err}");
        assert!(err.to_string().contains(fragment), "{fragment} not in {err}");
    }

    let mixed = set_cell(
        b"not a document",
        &SetCellOptions::default()
            .with_specs(vec![CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "v".into(),
            }])
            .with_table(0),
    )
    .expect_err("mixing is ambiguous");
    assert_eq!(mixed.code(), OpsCode::InvalidSetCellArgs, "{mixed}");
}

#[test]
fn an_out_of_range_table_reports_how_many_there_are() {
    let err = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(7).with_at("0,0").with_text("v"),
    )
    .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::TableNotFound, "{err}");
    assert!(err.hint().is_none(), "the count is dynamic, so the hint stays with the frontend");
}

#[test]
fn an_unmatched_label_is_a_cell_lookup_failure() {
    let err = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_right_of("없는라벨").with_text("v"),
    )
    .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::CellNotFound, "{err}");
}

#[test]
fn a_non_document_input_is_a_codec_failure() {
    let err = set_cell(
        b"not a zip at all",
        &SetCellOptions::default().with_table(0).with_at("0,0").with_text("v"),
    )
    .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::SetCellCodecFailed, "{err}");
}

/// The regenerating-edit policy row: an encode that loses meaning produces
/// no bytes, because the admission gate compares Core documents and cannot
/// see damage that happens on the wire.
#[test]
fn a_semantic_loss_on_re_encode_produces_no_bytes() {
    let err = set_cell(
        &table_with_title_mark_note(),
        &SetCellOptions::default().with_table(0).with_at("0,0").with_text("새 값"),
    )
    .expect_err("a silent success here would ship a damaged document");

    assert_eq!(err.code(), OpsCode::EncodeSemanticLoss, "{err}");
    assert_eq!(err.code().as_str(), "ENCODE_SEMANTIC_LOSS");
    assert!(err.hint().is_some(), "the caller is told to read the warning list");
    let OpsError::EncodeSemanticLoss { warnings, others } = &err else {
        panic!("a regenerating edit must report the shared envelope, got {err:?}");
    };
    assert!(!warnings.is_empty(), "the envelope names what was lost");
    assert!(
        warnings.iter().all(|w| w.code == "NOTE_HEAD_SKIPPED"),
        "the note number head is what this fixture loses: {warnings:?}"
    );
    assert!(
        others.iter().all(|w| w.code != "NOTE_HEAD_SKIPPED"),
        "no semantic loss leaks into the other list: {others:?}"
    );
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"),
    )
    .expect("set_cell");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(keys(&value), ["results", "warnings"]);
    assert_eq!(value["results"][0]["resolution"], "exact");
    assert_eq!(value["warnings"], serde_json::json!([]));
}

/// The success half of `set_cell`'s warning contract.
///
/// The failure half (semantic loss ⇒ no bytes) is covered above. This pins
/// the other half: the operation forwards whatever **non-semantic** warnings
/// the successful encode raised, rather than hard-coding an empty vector as
/// it used to.
///
/// The list is empty for every document reachable here, and that is a
/// property of the encoder rather than of this wiring: the only non-semantic
/// `EncodeWarning` is `LayoutCacheDropped`, which the encoder raises only
/// when `EncodeOptions::emit_layout_cache` is set — an opt-in a
/// preserve-first editor must never set. The forwarding itself is proven
/// where the outcome can be built directly, in `smithy-hwpx`'s
/// `encoder::tests::split_successful_encode`.
#[test]
fn a_successful_edit_reports_the_encoders_nonsemantic_warnings() {
    let out = set_cell(
        &labelled_table(),
        &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"),
    )
    .expect("set_cell");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(
        codes.iter().all(|code| code != "NOTE_HEAD_SKIPPED" && code != "TITLE_MARK_SKIPPED"),
        "a semantic-loss warning must fail the edit, never ride along: {codes:?}"
    );
    assert_eq!(codes, Vec::<String>::new(), "this encode raises nothing: {codes:?}");
}

/// A package that warns on decode, is admissible, and holds one 2x2 table.
///
/// This codec's own output with one paragraph's `<hp:linesegarray>` pointing
/// past the end of that paragraph's text — the shape a third-party edit
/// leaves behind — so the decoder refuses to promote a guessed coordinate and
/// reports the drop. The same fixture backs the decode-warning tests of
/// `fill`, `insert_para`, `delete_para` and `stamp`, so the five editing
/// surfaces cannot disagree about what "a document that warns" means.
fn warning_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/");
    std::fs::read(format!("{path}stale-line-cache.hwpx")).expect("stale-line-cache.hwpx")
}

/// The edit admits its input by decoding it, so the caller hears what that
/// decode reported — in front of the encode channel it already carried.
///
/// One warning, not two: the admission gate also decodes its own no-op
/// re-encode, but that package is a discarded verification artefact.
#[test]
fn decode_warnings_reach_the_set_cell_output() {
    let out = set_cell(
        &warning_fixture(),
        &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"),
    )
    .expect("set_cell");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert_eq!(codes, ["LAYOUT_CACHE_DROPPED"], "one input decode, reported once: {codes:?}");
    assert_eq!(out.results.len(), 1, "the edit itself still happened");
}
