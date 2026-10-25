//! Error contract 0.18, D24: a codec failure keeps the original error's
//! stage, leaf code and message, and the wrappers that carry a patch error
//! (`FillError::Patch`, `SectionWorkflowError::PreservingPatch`) pass it on
//! unchanged. Each test compares against the original error the same input
//! produces on its own.

use hwpforge::core::{Control, RunContent};
use hwpforge::hwpx::{
    CodecFailure, CodecStage, ExportedSection, FillError, HwpxDecoder, HwpxError, HwpxFiller,
    HwpxPatcher, PatchError, SectionWorkflowError,
};

fn original(error: &HwpxError, stage: CodecStage) -> CodecFailure {
    CodecFailure { stage, code: error.code(), detail: error.to_string() }
}

#[test]
fn fill_decode_failure_keeps_stage_leaf_and_detail() {
    // 이것을 실패시키는 것: fill 입력 decode 의 leaf 를 다른 코드로 바꾸기, 또는
    // CodecFailure::hwpx 가 code() 대신 고정 코드를 쓰기.
    let bytes = include_bytes!("data/corrupt-input.hwpx");
    let decode = HwpxDecoder::decode(bytes).unwrap_err();
    let error = HwpxFiller::fill(bytes, &[("a".into(), "b".into())].into()).unwrap_err();
    let FillError::Codec(failure) = error else { panic!("{error}") };
    assert_eq!(failure, original(&decode, CodecStage::Decode));
}

#[test]
fn patch_inner_decode_failure_reaches_the_workflow_unchanged() {
    // 이것을 실패시키는 것: patch 원본 decode 의 leaf 를 다른 코드로 바꾸기, 또는
    // PreservingPatch 로 넘길 때 CodecFailure 를 고쳐 쓰기.
    let bytes = include_bytes!("data/patch-inner-decode.hwpx");
    let exported: ExportedSection =
        serde_json::from_str(include_str!("data/patch-inner-decode.section.json")).unwrap();
    let decode = HwpxDecoder::decode(bytes).unwrap_err();

    let direct = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
        bytes,
        0,
        &exported.section,
        exported.styles.as_ref(),
        exported.preservation.as_ref(),
    )
    .unwrap_err();
    let PatchError::Codec(direct) = direct else { panic!("{direct}") };
    assert_eq!(direct, original(&decode, CodecStage::Decode));

    let workflow = HwpxPatcher::patch_exported_section(bytes, 0, &exported).unwrap_err();
    let SectionWorkflowError::PreservingPatch(PatchError::Codec(carried)) = workflow else {
        panic!("{workflow}")
    };
    assert_eq!(carried, direct);
}

#[test]
fn fill_carries_the_patch_refusal_unchanged() {
    // 이것을 실패시키는 것: FillError::Patch 로 넘길 때 PatchError 를 고쳐 쓰기.
    // The field body holds a Hancom-style tab with attributes, which the
    // preserving patch refuses to recreate.
    let bytes = include_bytes!("data/fill-inline-tab.hwpx");
    let error = HwpxFiller::fill(bytes, &[("user_email".into(), "x".into())].into()).unwrap_err();
    let FillError::Patch(PatchError::Rejected(via_fill)) = error else { panic!("{error}") };

    // The same edit through the section workflow, without fill.
    let mut exported = HwpxPatcher::export_section_for_edit(bytes, 0, false).unwrap().exported;
    let mut edited = 0;
    for paragraph in &mut exported.section.paragraphs {
        for run in &mut paragraph.runs {
            if let RunContent::Control(control) = &mut run.content {
                if let Control::Field { display_text, .. } = control.as_mut() {
                    *display_text = "x".into();
                    edited += 1;
                }
            }
        }
    }
    assert_eq!(edited, 1);
    let workflow = HwpxPatcher::patch_exported_section(bytes, 0, &exported).unwrap_err();
    let SectionWorkflowError::PreservingPatch(PatchError::Rejected(direct)) = workflow else {
        panic!("{workflow}")
    };
    assert_eq!(via_fill.code(), direct.code());
    assert_eq!(via_fill.to_string(), direct.to_string());
}
