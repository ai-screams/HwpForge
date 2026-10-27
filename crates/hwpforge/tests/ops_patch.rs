//! `ops::patch` — the preserving section replacement.
//!
//! Preservation is the whole point, so it is asserted directly: patching a
//! section back unchanged must leave the document semantically identical,
//! which is what makes the operation safe on a document whose layout matters.
#![cfg(feature = "ops-hwpx")]

#[path = "support/fixtures.rs"]
mod fixtures;
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
