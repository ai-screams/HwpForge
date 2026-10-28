//! `ops::patch` — the preserving section replacement.
//!
//! Preservation is the whole point, so it is asserted directly: patching a
//! section back unchanged must leave the document semantically identical,
//! which is what makes the operation safe on a document whose layout matters.
#![cfg(feature = "ops-hwpx")]

#[path = "support/fixtures.rs"]
mod fixtures;
#[path = "support/style_references.rs"]
mod style_references;

use fixtures::hwpx_fixture as fixture;
#[path = "support/json.rs"]
mod json;
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::diff::diff;
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
use json::{corrupt_first_address, key_set as keys};

/// The annotated section export of `name`, as JSON text.
fn exported_section_text(name: &str, section: usize) -> String {
    let out =
        export_section(&fixture(name), &ExportSectionOptions::default().with_section(section))
            .expect("export_section");
    serde_json::to_string(&out.section).expect("serialise")
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn an_unedited_patch_round_trips() {
    let bytes = fixture("garden_notes.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("garden_notes.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    assert_eq!(out.section, 0);
    assert_eq!(out.sections, 1);
    assert!(!out.bytes.is_empty());
}

/// A preserving patch that changes nothing must leave the document
/// semantically identical — this is what makes it safe to use for a narrow
/// edit on a document whose layout matters.
#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn an_unedited_patch_preserves_the_document() {
    let bytes = fixture("garden_notes.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("garden_notes.hwpx", 0));

    let patched = patch(&bytes, &opts).expect("patch").bytes;

    let report = diff(&bytes, &patched).expect("diff").diff;
    assert!(report.semantic.is_empty(), "nothing changed semantically: {:?}", report.semantic);
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn meta_keys_are_section_and_warnings() {
    let bytes = fixture("garden_notes.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("garden_notes.hwpx", 0));

    let value =
        serde_json::to_value(patch(&bytes, &opts).expect("patch").meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["section", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(value["section"], serde_json::json!(0));
}

/// A preserving patch has no encoder, but it does decode the base.
///
/// The `warnings` channel must carry whatever the library produced on the
/// path; for `patch` that is the base decode, which used to be dropped and
/// the wrapper hard-coded to empty. `garden_notes.hwpx` is a committed fixture
/// whose decode drops a layout cache, so the loss is observable rather than
/// theoretical.
#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn the_base_decode_warnings_reach_the_caller() {
    let bytes = fixture("garden_notes.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("garden_notes.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(!codes.is_empty(), "a patch that decodes must report what the decode found");
    assert!(codes.contains(&"LAYOUT_CACHE_DROPPED".to_string()), "{codes:?}");
}

/// The other half: a clean base means a genuinely empty list.
#[test]
fn a_clean_base_reports_nothing() {
    let bytes = fixture("SimpleTable.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleTable.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn a_patch_that_is_not_json_is_a_parse_failure() {
    let opts = PatchOptions::default().with_patch("{ not json");

    let err = patch(&fixture("garden_notes.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
    assert_eq!(err.code().as_str(), "JSON_PARSE_FAILED");
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn a_patch_that_is_not_an_exported_section_is_a_parse_failure() {
    let opts = PatchOptions::default().with_patch(r#"{"unexpected": true}"#);

    let err = patch(&fixture("garden_notes.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
}

/// Writing section 0's content over section 1 would silently destroy a
/// section, so the index carried by the JSON has to agree with the request.
#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn a_section_index_that_disagrees_with_the_json_is_refused() {
    let opts = PatchOptions::default()
        .with_section(1)
        .with_patch(exported_section_text("garden_notes.hwpx", 0));

    let err = patch(&fixture("garden_notes.hwpx"), &opts).expect_err("must reject");

    assert!(
        matches!(err.code(), OpsCode::SectionIndexMismatch | OpsCode::SectionOutOfRange),
        "{err} reported {}",
        err.code().as_str()
    );
}

#[test]
fn a_stale_grid_address_in_the_patch_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(&exported_section_text("SimpleTable.hwpx", 0)).expect("parse");
    let mut patched = 0usize;
    corrupt_first_address(&mut value, &mut patched);
    assert_eq!(patched, 1, "the fixture must have an address to corrupt");

    let opts = PatchOptions::default().with_patch(value.to_string());
    let err = patch(&fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::GridAddrInvalid, "{err}");
}

#[test]
// 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
fn an_undecodable_base_is_a_decode_failure() {
    let opts = PatchOptions::default().with_patch(exported_section_text("garden_notes.hwpx", 0));

    let err = patch(b"not a zip at all", &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

/// An edited text slot holding a character XML cannot carry is written
/// without it, and the removal is reported with the slot path.
#[test]
fn a_forbidden_character_in_an_edited_slot_is_removed_and_reported() {
    // 이것을 실패시키는 것: `patch` 가 `encode_warnings` 를 `warnings` 에 합치지 않는 것.
    use hwpforge::core::image::ImageStore;
    use hwpforge::core::run::Run;
    use hwpforge::core::{Document, PageSettings, Paragraph, Section};
    use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    use hwpforge::hwpx::HwpxEncoder;

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("before", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode");
    let exported = export_section(&bytes, &ExportSectionOptions::default()).expect("export");
    let mut value = serde_json::to_value(&exported.section).expect("json");
    let slot =
        value.pointer_mut("/section/paragraphs/0/runs/0/content/Text").expect("first run is text");
    *slot = serde_json::Value::String("a\u{1}b".to_string());

    let out = patch(&bytes, &PatchOptions::default().with_patch(value.to_string())).expect("patch");

    let codes: Vec<(String, String)> =
        out.meta().warnings.into_iter().map(|w| (w.code, w.message)).collect();
    assert_eq!(
        codes,
        [(
            "XML_FORBIDDEN_CHARS_REMOVED".to_string(),
            "removed 1 XML-forbidden character(s) from paragraphs[0].runs[0].text in Contents/section0.xml"
                .to_string()
        )]
    );
}

// 이것을 실패시키는 것: patch 비교에서 ref 정수 직접 비교 또는 대응표 무시.
#[test]
fn permuted_reference_space_is_compared_by_effective_wire() {
    use serde_json::{json, Value};
    let bytes = fixture("garden_notes.hwpx");
    let mut exported: Value =
        serde_json::from_str(&exported_section_text("garden_notes.hwpx", 0)).unwrap();
    let count = exported["styles"]["styles"].as_array().unwrap().len();
    assert!(count > 1);
    exported["styles"]["style_positions"] = json!((0..count).rev().collect::<Vec<_>>());

    style_references::transform(&mut exported["section"], &|map| {
        if let Some(r) = map.get_mut("style_ref") {
            if let Some(n) = r.as_u64() {
                *r = json!(count - 1 - n as usize);
            }
        }
    });
    let out = patch(&bytes, &PatchOptions::default().with_patch(exported.to_string())).unwrap();
    assert!(diff(&bytes, &out.bytes).unwrap().diff.semantic.is_empty());
    exported["styles"]["style_positions"] = json!((0..count).collect::<Vec<_>>());
    let error =
        patch(&bytes, &PatchOptions::default().with_patch(exported.to_string())).unwrap_err();
    assert_eq!(error.code(), OpsCode::PatchFailed);
    assert_eq!(error.to_string(), "Preserving patch failed: Invalid HWPX structure: preserving patch currently supports text-only section edits; structural change detected");
}

// 이것을 실패시키는 것: styles 생략 구역에 원본 header 정체성을 추정하거나 styles 비교를 생략.
#[test]
fn omitted_styles_use_submitted_base_even_with_a_different_header() {
    use serde_json::{json, Value};
    use std::io::{Cursor, Read, Write};
    let a = fixture("garden_notes.hwpx");
    let mut input = zip::ZipArchive::new(Cursor::new(&a)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut changed = false;
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "Contents/header.xml" {
            let text = String::from_utf8(bytes).unwrap();
            let edited = text.replacen("name=\"바탕글\"", "name=\"개요 2\"", 1);
            changed = edited != text;
            bytes = edited.into_bytes();
        }
        output.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(&bytes).unwrap();
    }
    assert!(changed);
    let b = output.finish().unwrap().into_inner();
    let mut exported: Value =
        serde_json::from_str(&exported_section_text("garden_notes.hwpx", 0)).unwrap();
    let error = patch(&b, &PatchOptions::default().with_patch(exported.to_string())).unwrap_err();
    assert_eq!(error.code(), OpsCode::PatchFailed);
    assert_eq!(error.to_string(), "Preserving patch failed: Invalid HWPX structure: preserving patch does not support style-store changes yet");
    exported.as_object_mut().unwrap().remove("styles");
    for version in [3, 2] {
        let mut payload = exported.clone();
        payload["format_version"] = json!(version);
        if version == 2 {
            style_references::transform(&mut payload["section"], &|map| {
                if let Some(r) = map.remove("style_ref") {
                    map.insert("style_id".into(), r);
                }
            });
        }
        let result = patch(&b, &PatchOptions::default().with_patch(payload.to_string())).unwrap();
        assert!(diff(&b, &result.bytes).unwrap().diff.semantic.is_empty());
    }
}
