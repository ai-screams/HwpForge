//! `ops::stamp::stamp` — applying an approved map, phase two of stamping.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{
    parse_stamp_map, StampAction, StampMap, StampRequestV2, StampSpec, STAMP_MANIFEST_V2_VERSION,
    STAMP_MANIFEST_VERSION, STAMP_MAP_VERSION,
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
    assert_eq!(keys(&serde_json::to_value(out.meta()).expect("serialise")), ["warnings"]);
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
    assert_eq!(keys(&value), ["manifest", "warnings"]);
    assert_eq!(value["manifest"]["schema_version"], STAMP_MANIFEST_VERSION);
    assert_eq!(value["manifest"]["fields"].as_array().expect("fields").len(), 2);
    assert_eq!(value["warnings"], serde_json::json!([]));
}
