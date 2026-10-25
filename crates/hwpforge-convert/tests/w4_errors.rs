//! W4: HWP5 conversion failures report the code of the stage that failed.
//!
//! Consumes the shared error contract's Rust cases whose operation is
//! `convert.*` (the umbrella harness, `hwpforge/tests/w1_errors.rs`, skips
//! them because the umbrella cannot depend on this crate). Fixture cases run
//! the shared input through the public operation; injected cases build the
//! staged error by hand, because validation, HWPX encoding and the layout
//! replay are not reachable from an input this suite can supply (see
//! `src/ops/tests.rs`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use hwpforge_convert::ops::{
    convert_hwp5, to_pdf, ConvertHwp5Options, ConvertOpsError, ToPdfOptions,
};
use hwpforge_convert::{ConvertError, LayoutPatchError};
use hwpforge_core::CoreError;
use hwpforge_smithy_hwpx::HwpxError;

fn shared(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// `(case id, operation run, mode, error)`: the operation and mode name what
/// this function actually did, so the manifest cannot claim a different one.
fn cases() -> Vec<(&'static str, &'static str, &'static str, ConvertOpsError)> {
    // A table whose borderFill id names no DocInfo definition: the decoder's
    // own reference check refuses it with a Core error. `inspect_hwp5` does
    // not run that check, so the CLI's inspect pre-pass lets it through.
    let dangling = shared("hwp5-dangling-table-border-fill.hwp");
    vec![
        (
            "hwp5.decode_core",
            "convert.convert_hwp5",
            "fixture",
            convert_hwp5(&dangling, &ConvertHwp5Options::default()).expect_err("broken reference"),
        ),
        (
            "hwp5.to_pdf.decode_core",
            "convert.to_pdf",
            "fixture",
            to_pdf(&dangling, &ToPdfOptions::default()).expect_err("broken reference"),
        ),
        (
            "hwp5.validate",
            "convert.convert_hwp5",
            "injected",
            ConvertOpsError::Convert(ConvertError::Validate(CoreError::InvalidStructure {
                context: "document".into(),
                reason: "no sections".into(),
            })),
        ),
        (
            "hwp5.encode",
            "convert.convert_hwp5",
            "injected",
            ConvertOpsError::Convert(ConvertError::Encode(HwpxError::XmlSerialize {
                detail: "section0.xml".into(),
            })),
        ),
        (
            "hwp5.layout_patch",
            "convert.convert_hwp5",
            "injected",
            ConvertOpsError::Convert(ConvertError::LayoutPatch(LayoutPatchError::MissingEntry {
                name: "Contents/section0.xml".into(),
            })),
        ),
    ]
}

#[test]
// 이것을 실패시키는 것: `hwp5_code` 의 `Core` 를 `Hwp5ConvertFailed` 로, `convert_code` 의 Validate·Encode 를
// `Hwp5ConvertFailed` 로, LayoutPatch 를 다른 코드로 바꾸기, manifest 에 소비하지 않는 `convert.*` case 추가.
// 이것을 실패시키는 것(fix1): manifest 의 rust operation 을 없는 이름(`convert.does_not_exist`)으로,
// mode 를 injected → fixture 로 바꾸기.
fn hwp5_stages_report_the_manifest_codes() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
            .expect("manifest");
    let all = manifest["cases"].as_array().expect("cases");
    let mut consumed = BTreeSet::new();
    for (id, operation, mode, error) in cases() {
        let case = all.iter().find(|c| c["id"] == id).unwrap_or_else(|| panic!("case {id}"));
        let rust = &case["surfaces"]["rust"];
        assert_eq!(rust["operation"], operation, "{id}: operation run");
        assert_eq!(rust["mode"], mode, "{id}: mode run");
        assert_eq!(error.code().as_str(), case["expected_code"].as_str().unwrap(), "{id}: {error}");
        consumed.insert(id);
    }
    let declared: BTreeSet<&str> = all
        .iter()
        .filter(|c| {
            c["surfaces"]["rust"]["operation"].as_str().is_some_and(|op| op.starts_with("convert."))
        })
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(consumed, declared);
}
