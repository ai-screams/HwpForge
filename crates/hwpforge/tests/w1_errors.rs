#![cfg(feature = "ops-hwpx")]
use hwpforge::hwpx::{
    CodecFailure, CodecStage, FillError, HwpxDecoder, HwpxError, HwpxErrorCode, HwpxPatcher,
    PatchError, SectionWorkflowError,
};
use hwpforge::ops::exchange::PatchOptions;
use hwpforge::ops::{self, OpsError, ReadOptions};

fn failure(stage: CodecStage) -> CodecFailure {
    CodecFailure { stage, code: HwpxErrorCode::Zip, detail: "injected".into() }
}

/// A patch error as the patch operation reports it.
fn patch_err(error: PatchError) -> OpsError {
    OpsError::SectionWorkflow(SectionWorkflowError::PreservingPatch(error))
}

/// Every `rust` surface of the W2 cases (patch, fill, section workflow).
fn w2_rust_cases() -> Vec<(&'static str, OpsError)> {
    let inner = include_bytes!("data/patch-inner-decode.hwpx");
    let section = include_str!("data/patch-inner-decode.section.json");
    let no_metadata = include_str!("data/patch-no-metadata.section.json");
    let corrupt = include_bytes!("data/corrupt-input.hwpx");
    let fixture = include_bytes!("../../hwpforge-smithy-hwpx/tests/fixtures/SimpleTable.hwpx");
    let decoded = HwpxDecoder::decode(fixture).unwrap();
    let rejected = || PatchError::Rejected(HwpxError::InvalidStructure { detail: "x".into() });
    vec![
        // Entry checks pass (package, section, metadata); the base XML is broken.
        (
            "patch.input_decode",
            ops::exchange::patch(inner, &PatchOptions::default().with_patch(section)).unwrap_err(),
        ),
        // Missing metadata and broken XML at once: the metadata check runs first.
        (
            "patch.missing_metadata_first",
            ops::exchange::patch(inner, &PatchOptions::default().with_patch(no_metadata))
                .unwrap_err(),
        ),
        (
            "patch.section_out_of_range",
            patch_err(
                HwpxPatcher::export_section_preservation(
                    fixture,
                    5,
                    &decoded.document.sections()[0],
                )
                .unwrap_err(),
            ),
        ),
        ("patch.validation", patch_err(PatchError::Codec(failure(CodecStage::Validation)))),
        ("patch.encode", patch_err(PatchError::Codec(failure(CodecStage::Encode)))),
        ("patch.codec.admission", patch_err(PatchError::Codec(failure(CodecStage::Admission)))),
        (
            "patch.codec.verification",
            patch_err(PatchError::Codec(failure(CodecStage::Verification))),
        ),
        (
            "patch.protected_field",
            patch_err(PatchError::ProtectedFieldRange { detail: "range".into() }),
        ),
        ("patch.invariant", patch_err(PatchError::Invariant { detail: "source map".into() })),
        (
            "fill.input_decode",
            ops::edit::fill(corrupt, &[("a".into(), "b".into())], &Default::default()).unwrap_err(),
        ),
        ("fill.invariant", OpsError::Fill(FillError::Invariant { detail: "section".into() })),
        ("fill.codec.admission", OpsError::Fill(FillError::Codec(failure(CodecStage::Admission)))),
        (
            "fill.codec.validation",
            OpsError::Fill(FillError::Codec(failure(CodecStage::Validation))),
        ),
        ("fill.codec.encode", OpsError::Fill(FillError::Codec(failure(CodecStage::Encode)))),
        (
            "fill.codec.verification",
            OpsError::Fill(FillError::Codec(failure(CodecStage::Verification))),
        ),
        (
            "fill.workflow",
            OpsError::Fill(FillError::Workflow(HwpxError::InvalidStructure {
                detail: "field boundary".into(),
            })),
        ),
        ("fill.patch.rejected", OpsError::Fill(FillError::Patch(rejected()))),
        (
            "fill.patch.validation",
            OpsError::Fill(FillError::Patch(PatchError::Codec(failure(CodecStage::Validation)))),
        ),
        (
            "fill.patch.encode",
            OpsError::Fill(FillError::Patch(PatchError::Codec(failure(CodecStage::Encode)))),
        ),
        (
            "fill.patch.protected_field",
            OpsError::Fill(FillError::Patch(PatchError::ProtectedFieldRange {
                detail: "range".into(),
            })),
        ),
        (
            "fill.patch.section_out_of_range",
            OpsError::Fill(FillError::Patch(PatchError::SectionOutOfRange {
                requested: 1,
                sections: 1,
            })),
        ),
        (
            "fill.patch.invariant",
            OpsError::Fill(FillError::Patch(PatchError::Invariant { detail: "x".into() })),
        ),
    ]
}

fn w3_rust_cases() -> Vec<(&'static str, OpsError)> {
    use hwpforge::hwpx::stamp::StamperError;
    use hwpforge::hwpx::{CellEditError, StructuralEditError};
    let mut cases = Vec::new();
    for (stage, cell, structural, stamp) in [
        (CodecStage::Decode, "cell.codec.decode", "structural.codec.decode", "stamp.codec.decode"),
        (
            CodecStage::Admission,
            "cell.codec.admission",
            "structural.codec.admission",
            "stamp.codec.admission",
        ),
        (
            CodecStage::Validation,
            "cell.codec.validation",
            "structural.codec.validation",
            "stamp.codec.validation",
        ),
        (CodecStage::Encode, "cell.codec.encode", "structural.codec.encode", "stamp.codec.encode"),
        (
            CodecStage::Verification,
            "cell.codec.verification",
            "structural.codec.verification",
            "stamp.codec.verification",
        ),
    ] {
        cases.push((cell, OpsError::CellEdit(CellEditError::Codec(failure(stage)))));
        cases.push((
            structural,
            OpsError::StructuralEdit(StructuralEditError::Codec(failure(stage))),
        ));
        cases.push((stamp, OpsError::Stamper(StamperError::Codec(failure(stage)))));
    }
    cases.extend([
        (
            "cell.invariant",
            OpsError::CellEdit(CellEditError::Invariant { detail: "source".into() }),
        ),
        (
            "structural.invariant",
            OpsError::StructuralEdit(StructuralEditError::Invariant { detail: "source".into() }),
        ),
        ("stamp.invariant", OpsError::Stamper(StamperError::Invariant { detail: "source".into() })),
        (
            "structural.protected_field",
            OpsError::StructuralEdit(StructuralEditError::ProtectedFieldRange {
                section: 1,
                index: 2,
                detail: "protected".into(),
            }),
        ),
    ]);
    for (bytes, ids) in [
        (
            include_bytes!("data/corrupt-input.hwpx").as_slice(),
            [
                "cell.input_decode",
                "structural.input_decode",
                "structural_delete.input_decode",
                "stamp.input_decode",
            ],
        ),
        (
            include_bytes!("data/edit-empty-section.hwpx").as_slice(),
            [
                "cell.admission",
                "structural.admission",
                "structural_delete.admission",
                "stamp.admission",
            ],
        ),
    ] {
        cases.extend([
            (
                ids[0],
                ops::edit::set_cell(
                    bytes,
                    &ops::edit::SetCellOptions::default()
                        .with_table(0)
                        .with_at("0,0")
                        .with_text("text"),
                )
                .unwrap_err(),
            ),
            (
                ids[1],
                ops::edit::insert_para(
                    bytes,
                    &ops::edit::InsertParaOptions::default().with_text(vec!["text".into()]),
                )
                .unwrap_err(),
            ),
            (
                ids[2],
                ops::edit::delete_para(
                    bytes,
                    &ops::edit::DeleteParaOptions::default().with_indexes(vec![0]),
                )
                .unwrap_err(),
            ),
            (
                ids[3],
                ops::stamp::stamp(
                    bytes,
                    &hwpforge::hwpx::stamp::StampMap::Legacy(vec![]),
                    &Default::default(),
                )
                .unwrap_err(),
            ),
        ]);
    }
    cases.push((
        "stamp_plan.input_decode",
        ops::stamp::stamp_plan(include_bytes!("data/corrupt-input.hwpx")).unwrap_err(),
    ));
    cases
}

#[test]
// 이것을 실패시키는 것: OpsError::Io(NotFound)를 FileReadFailed로 변경, 기타 OpsError::Io를 FileNotFound로 변경, read 구역 범위를 InvalidInput으로 변경, read 표 부재를 InvalidInput으로 변경, read 필드 부재를 InvalidInput으로 변경, manifest에 미소비 Rust surface 추가.
// 이것을 실패시키는 것(W2): stage→코드 매핑 한 갈래 교체, PreservingPatch 위임을 PATCH_FAILED 로 고정,
// FillError::Patch 위임을 FILL_FAILED 로 고정, patch 원본 decode 를 Rejected 로 감싸기,
// metadata 검사를 원본 decode 뒤로 옮기기, 구역 범위 검사를 Rejected 로, fill 입력 decode 를 Workflow 로.
fn w1_read_and_io_causes_are_canonical() {
    // 이것을 실패시키는 것: O1–O3/I1–I3 W3 Codec·Invariant 매핑을 InvalidInput으로 변경, B4 Draft 전용 Rust case 제외 제거.
    // 이것을 실패시키는 것: read 매핑을 InvalidInput으로 치환하거나 NotFound/기타 I/O 매핑을 뒤바꿈.
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/error_contract_018.json")).unwrap();
    let fixture = include_bytes!("../../hwpforge-smithy-hwpx/tests/fixtures/SimpleTable.hwpx");
    let mut consumed = std::collections::BTreeSet::new();
    for (id, error) in [
        (
            "read.section",
            ops::read(fixture, &ReadOptions::default().with_section(999)).unwrap_err(),
        ),
        ("read.table", ops::read(fixture, &ReadOptions::default().with_table(999)).unwrap_err()),
        (
            "read.field",
            ops::read(fixture, &ReadOptions::default().with_field("missing")).unwrap_err(),
        ),
        ("io.missing", OpsError::Io(std::io::Error::from(std::io::ErrorKind::NotFound))),
        ("io.read", OpsError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied))),
    ]
    .into_iter()
    .chain(w2_rust_cases())
    .chain(w3_rust_cases())
    {
        consumed.insert(id);
        let case = manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
        assert_eq!(error.code().as_str(), case["expected_code"].as_str().unwrap(), "{id}");
    }
    let declared: std::collections::BTreeSet<_> = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["surfaces"].get("rust").is_some())
        // `convert.*` operations live in hwpforge-convert, which this crate
        // cannot depend on; `hwpforge-convert/tests/w4_errors.rs` consumes them.
        // `w3.*` operations are consumed by `w3_errors.rs`.
        // 이것을 실패시키는 것: 이 거르기 삭제(convert·w3 몫 case 를 이 harness 가 소비하지 못해 집합 불일치).
        .filter(|case| {
            let operation = case["surfaces"]["rust"]["operation"].as_str().unwrap();
            !operation.starts_with("convert.") && !operation.starts_with("w3.")
        })
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert_eq!(consumed, declared);
}

#[test]
// 이것을 실패시키는 것: 퇴역 variant 7개와 lookup을 복원.
fn w1_retired_codes_are_not_lookup_aliases() {
    // 이것을 실패시키는 것: R1 SetCellCodecFailed variant와 lookup 복원.
    // 이것을 실패시키는 것: 퇴역 OpsCode variant와 lookup을 복원.
    for code in [
        "SET_CELL_CODEC_FAILED",
        "STAMP_CODEC_FAILED",
        "STRUCTURAL_CODEC",
        "STAMP_DELTA_MISMATCH",
        "STAMP_CELL_TARGET_DUPLICATE",
        "READ_SECTION_OUT_OF_RANGE",
        "READ_TABLE_OUT_OF_RANGE",
        "READ_FIELD_NOT_FOUND",
        "SECTION_WORKFLOW_FAILED",
        "SET_CELL_FAILED",
        "STAMP_FAILED",
        "STRUCTURAL_EDIT_FAILED",
    ] {
        assert!(hwpforge_foundation::diagnostics::OpsCode::lookup(code).is_none(), "{code}");
    }
}
