//! `ops::stamp::stamp` — applying an approved map, phase two of stamping.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::grid::GridCoord;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{
    parse_stamp_map, CellLabelClaim, CellStampAction, CellStampSpec, StampAction, StampMap,
    StampRequestV2, StampSpec, STAMP_MANIFEST_V2_VERSION, STAMP_MANIFEST_VERSION,
    STAMP_MAP_VERSION,
};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxEncoder, HwpxFiller};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions, StampedManifest};
use hwpforge::ops::OpsError;

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// A body paren blank plus a checkbox in a table cell.
fn two_candidates() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let row = TableRow::new(vec![TableCell::new(vec![text_para("□ 동의")], width)]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("성명: (   )"), host],
        PageSettings::a4(),
    ));
    encode(doc)
}

/// A document with no candidates whose re-encode loses a note number head.
fn title_mark_note() -> Vec<u8> {
    let mut heading_body = text_para("제목 각주");
    heading_body.heading_level = Some(1);
    let note = Paragraph::with_runs(
        vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![note], PageSettings::a4()));
    encode(doc)
}

/// Approves every candidate of `bytes`, naming each after its marker.
fn approve_all(bytes: &[u8]) -> Vec<StampSpec> {
    stamp_plan(bytes)
        .expect("plan")
        .plan
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker.clone(),
            action: StampAction::Field {
                name: if candidate.marker == "□" { "동의".into() } else { "성명".into() },
                hint: None,
            },
        })
        .collect()
}

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn a_legacy_map_stamps_and_reports_the_v1_manifest() {
    let bytes = two_candidates();
    let request = StampMap::Legacy(approve_all(&bytes));

    let out = stamp(&bytes, &request, &StampOptions::default()).expect("stamp");

    let manifest = out.manifest.as_ref().expect("the default returns a manifest");
    assert_eq!(manifest.schema_version(), STAMP_MANIFEST_VERSION);
    assert!(matches!(manifest, StampedManifest::V1(_)), "a legacy map keeps the v1 shape");
    let names: Vec<String> = HwpxFiller::list_fields(&out.bytes)
        .expect("list_fields")
        .into_iter()
        .filter_map(|field| field.name)
        .collect();
    assert_eq!(names, ["성명", "동의"], "the stamped fields are discoverable");
    let stamped_names: Vec<&str> = out.stamped.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(stamped_names, ["성명", "동의"], "spec order, not document order");
    assert!(out.stamped_cells.is_empty(), "a legacy request never carries cell specs");
    assert_eq!(out.ignored, 0);
    assert_eq!(out.skipped_guarded, 0);
    // A successful stamp now reports the encode's non-semantic warnings
    // instead of a hard-coded empty list; this document produces none. The
    // only non-semantic `EncodeWarning` needs `emit_layout_cache`, which a
    // preserve-first editor never sets, so an empty list here is the real
    // answer rather than a dropped one. The forwarding is proven in
    // `smithy-hwpx`'s `encoder::tests::split_successful_encode`.
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn a_v2_map_stamps_and_reports_the_v2_manifest() {
    let bytes = two_candidates();
    let request = StampMap::V2(StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: stamp_plan(&bytes).expect("plan").plan.source_sha256,
        text: approve_all(&bytes),
        cells: Vec::new(),
    });

    let out = stamp(&bytes, &request, &StampOptions::default()).expect("stamp");

    let manifest = out.manifest.as_ref().expect("manifest");
    assert_eq!(manifest.schema_version(), STAMP_MANIFEST_V2_VERSION);
    assert!(matches!(manifest, StampedManifest::V2(_)), "a v2 envelope keeps the v2 shape");
}

/// The caller parses the map, so the whole JSON path has to work end to end.
#[test]
fn a_v2_map_parsed_from_json_stamps_the_same_way() {
    let bytes = two_candidates();
    let json = serde_json::to_string(&serde_json::json!({
        "schema_version": STAMP_MAP_VERSION,
        "source_sha256": stamp_plan(&bytes).expect("plan").plan.source_sha256,
        "text": approve_all(&bytes),
        "cells": [],
    }))
    .expect("serialise map");

    let request = parse_stamp_map(&json).expect("parse");
    let out = stamp(&bytes, &request, &StampOptions::default()).expect("stamp");

    assert_eq!(out.manifest.expect("manifest").schema_version(), STAMP_MANIFEST_V2_VERSION);
}

#[test]
fn a_map_pinned_to_other_bytes_is_refused_before_anything_is_touched() {
    let bytes = two_candidates();
    let request = StampMap::V2(StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: "0".repeat(64),
        text: approve_all(&bytes),
        cells: Vec::new(),
    });

    let err = stamp(&bytes, &request, &StampOptions::default()).expect_err("drifted pin");

    assert_eq!(err.code(), OpsCode::StampSourceHashMismatch, "{err}");
    assert!(err.hint().is_some(), "the caller is told to re-plan");
}

#[test]
fn an_unclassified_candidate_blocks_the_whole_stamp() {
    let bytes = two_candidates();

    let err = stamp(&bytes, &StampMap::Legacy(Vec::new()), &StampOptions::default())
        .expect_err("every unguarded candidate must be named or ignored");

    assert_eq!(err.code(), OpsCode::StampCandidateUncovered, "{err}");
}

#[test]
fn an_explicit_ignore_covers_a_candidate_without_stamping_it() {
    let bytes = two_candidates();
    let specs: Vec<StampSpec> = approve_all(&bytes)
        .into_iter()
        .map(|spec| StampSpec { action: StampAction::Ignore, ..spec })
        .collect();

    let out = stamp(&bytes, &StampMap::Legacy(specs), &StampOptions::default()).expect("stamp");

    assert!(
        HwpxFiller::list_fields(&out.bytes).expect("list_fields").is_empty(),
        "an ignored candidate creates no field"
    );
}

#[test]
fn turning_the_manifest_off_omits_it_from_the_output_and_the_wire() {
    let bytes = two_candidates();
    let request = StampMap::Legacy(approve_all(&bytes));

    let out =
        stamp(&bytes, &request, &StampOptions::default().with_manifest(false)).expect("stamp");

    assert!(out.manifest.is_none());
    assert!(!out.bytes.is_empty(), "the document is produced either way");
    assert_eq!(out.stamped.len(), 2, "apply-phase fields stay populated without the manifest");
    assert_eq!(
        keys(&serde_json::to_value(out.meta()).expect("serialise")),
        ["ignored", "skipped_guarded", "stamped", "stamped_cells", "warnings"]
    );
}

/// The regenerating-edit policy row: an encode that loses meaning produces
/// no bytes, even when the map asks for nothing at all.
#[test]
fn a_semantic_loss_on_re_encode_produces_no_bytes() {
    let err = stamp(&title_mark_note(), &StampMap::Legacy(Vec::new()), &StampOptions::default())
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
fn a_non_document_input_is_a_stamp_codec_failure() {
    let err = stamp(b"not a zip at all", &StampMap::Legacy(Vec::new()), &StampOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::StampCodecFailed, "{err}");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let bytes = two_candidates();
    let out = stamp(&bytes, &StampMap::Legacy(approve_all(&bytes)), &StampOptions::default())
        .expect("stamp");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(
        keys(&value),
        ["ignored", "manifest", "skipped_guarded", "stamped", "stamped_cells", "warnings"]
    );
    assert_eq!(value["manifest"]["schema_version"], STAMP_MANIFEST_VERSION);
    assert_eq!(value["manifest"]["fields"].as_array().expect("fields").len(), 2);
    assert_eq!(value["stamped"].as_array().expect("stamped").len(), 2);
    assert_eq!(value["stamped_cells"], serde_json::json!([]));
    assert_eq!(value["ignored"], 0);
    assert_eq!(value["skipped_guarded"], 0);
    assert_eq!(value["warnings"], serde_json::json!([]));
}

/// A package whose decode raises `LAYOUT_CACHE_DROPPED` and which is still
/// admissible.
///
/// It is this codec's own output with the last paragraph's
/// `<hp:linesegarray>` pointing past the end of that paragraph's text — the
/// shape a third-party edit leaves behind — so the decoder refuses to
/// promote a guessed coordinate and reports the drop instead. Being our own
/// output is what keeps it admissible: the two native documents in the tree
/// that warn both fail the gate on uncarried ZIP entries.
///
/// The same fixture backs the decode-warning tests of `fill`, `insert_para`,
/// `delete_para` and `stamp`, so the four surfaces cannot disagree about
/// what "a document that warns" means.
fn warning_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/");
    std::fs::read(format!("{path}stale-line-cache.hwpx")).expect("stale-line-cache.hwpx")
}

/// A stamp decodes its input behind the admission gate, so the caller hears
/// what that decode reported.
///
/// The documented order is decode first, then the successful encode's
/// non-semantic warnings; the encode half is empty for every document
/// reachable today (its only member needs `emit_layout_cache`, which a
/// preserve-first editor never sets), so what is asserted here is the decode
/// half and the absence of anything after it.
///
/// One warning, not four: the admission gate decodes the re-encoded package,
/// the manifest decodes the output, and the v2 path decodes a fixed-point
/// re-encode. Those three describe packages that are either discarded or
/// derived from the decode already reported, so only the input decode is
/// carried.
#[test]
fn decode_warnings_reach_the_stamp_output() {
    let bytes = warning_fixture();
    let request = StampMap::Legacy(approve_all(&bytes));

    let out = stamp(&bytes, &request, &StampOptions::default()).expect("stamp");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert_eq!(codes, ["LAYOUT_CACHE_DROPPED"], "one input decode, reported once: {codes:?}");
    assert!(out.manifest.is_some(), "the stamp itself still happened");
}

/// Two unguarded paren blanks plus a checkbox inside a `※`-prefixed
/// instruction paragraph, which downgrades that checkbox to guarded (see
/// `paragraph_guard` in `smithy-hwpx`'s stamp detector).
fn two_unguarded_and_one_guarded() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("성명: (   )"),
            text_para("소속: (  )"),
            text_para("※ 해당하는 항목의 □에 표시"),
        ],
        PageSettings::a4(),
    ));
    encode(doc)
}

/// `stamped`, `stamped_cells`, `ignored` and `skipped_guarded` are the
/// apply-phase disposition of every plan candidate, not a projection of the
/// manifest — this covers all four on one request: one candidate named, one
/// explicitly ignored, one guarded candidate left uncovered.
#[test]
fn ignored_and_guarded_candidates_are_both_counted_without_being_stamped() {
    let bytes = two_unguarded_and_one_guarded();
    let plan = stamp_plan(&bytes).expect("plan").plan;
    let unguarded: Vec<_> = plan.text.iter().filter(|c| c.guard.is_none()).collect();
    assert_eq!(unguarded.len(), 2, "{:?}", plan.text);
    assert_eq!(plan.text.len(), 3, "two unguarded plus one guarded checkbox: {:?}", plan.text);

    let named = |c: &hwpforge::hwpx::stamp::StampCandidate, action: StampAction| StampSpec {
        section: c.section,
        path: c.path.clone(),
        span: c.span.clone(),
        marker: c.marker.clone(),
        action,
    };
    let specs = vec![
        named(unguarded[0], StampAction::Field { name: "성명".into(), hint: None }),
        named(unguarded[1], StampAction::Ignore),
    ];

    let out = stamp(&bytes, &StampMap::Legacy(specs), &StampOptions::default()).expect("stamp");

    assert_eq!(out.stamped.len(), 1, "{:?}", out.stamped);
    assert_eq!(out.stamped[0].name, "성명");
    assert!(out.stamped_cells.is_empty(), "no cell specs in this request");
    assert_eq!(out.ignored, 1, "the second unguarded candidate was explicitly ignored");
    assert_eq!(out.skipped_guarded, 1, "the guarded checkbox needed no spec and got none");
}

/// A 1×2 label table (label cell + stampable empty cell) so a v2 request can
/// promote the empty cell to a class-B field.
fn label_form() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let row = TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    encode(doc)
}

/// A class-B cell stamp lands in `stamped_cells`, not `stamped` — the two
/// lists are kept apart because they carry materially different shapes
/// (`StampedField` vs. `CellStampedField`).
#[test]
fn a_cell_stamp_lands_in_stamped_cells_not_the_text_list() {
    let bytes = label_form();
    let plan = stamp_plan(&bytes).expect("plan");
    assert_eq!(plan.plan.cells.len(), 1, "{:?}", plan.plan.cells);

    let request = StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: plan.plan.source_sha256.clone(),
        text: Vec::new(),
        cells: vec![CellStampSpec {
            table: 0,
            at: GridCoord::new(0, 1),
            label: Some(CellLabelClaim { at: GridCoord::new(0, 0), text: "성명".into() }),
            action: CellStampAction::Field { name: "성명".into(), hint: "성명 입력".into() },
        }],
    };

    let out = stamp(&bytes, &StampMap::V2(request), &StampOptions::default()).expect("stamp");

    assert!(out.stamped.is_empty(), "no text specs in this request");
    assert_eq!(out.stamped_cells.len(), 1);
    assert_eq!(out.stamped_cells[0].name, "성명");
    assert_eq!(out.ignored, 0);
    assert_eq!(out.skipped_guarded, 0);
}
