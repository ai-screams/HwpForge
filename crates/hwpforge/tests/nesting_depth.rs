//! Synthetic admission tests: no corpus, GUI or PDF writer.
#![cfg(feature = "ops-md")]
use hwpforge::ops::exchange::{from_json, patch, FromJsonOptions, PatchOptions};
use hwpforge_core::control::GroupChild;
use hwpforge_core::image::ImageStore;
use hwpforge_core::{
    Control, Document, ObjectFrame, PageSettings, Paragraph, Run, Section, StyleLookup, Table,
    TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    apply_set_cells, CellEditError, CodecFailure, CodecStage, ExportedDocument, ExportedSection,
    HwpxDecoder, HwpxEncoder, HwpxError, HwpxErrorCode, HwpxPatcher, PatchError,
};
use hwpforge_smithy_md::MdEncoder;

fn u() -> HwpUnit {
    HwpUnit::new(100).unwrap()
}
fn text() -> Run {
    Run::text("leaf", CharShapeIndex::new(0))
}
fn para(r: Run) -> Paragraph {
    Paragraph::with_runs(vec![r], ParaShapeIndex::new(0))
}
fn doc(r: Run) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(r)], PageSettings::a4()));
    d
}
fn group(n: usize) -> Run {
    let mut c = Control::Group {
        children: vec![],
        width: u(),
        height: u(),
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    for _ in 1..n {
        c = Control::Group {
            children: vec![GroupChild::Shape(c)],
            width: u(),
            height: u(),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
    }
    Run::control(c, CharShapeIndex::new(0))
}
fn boxes(n: usize) -> Run {
    let mut r = text();
    for _ in 0..n {
        r = Run::control(Control::text_box(vec![para(r)], u(), u()), CharShapeIndex::new(0));
    }
    r
}
fn tables(n: usize) -> Run {
    let mut r = text();
    for _ in 0..n {
        r = Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(vec![para(r)], u())])]),
            CharShapeIndex::new(0),
        );
    }
    r
}
fn depth_error<T>(result: Result<T, HwpxError>) {
    assert!(
        matches!(result, Err(HwpxError::InvalidStructure { detail }) if detail.contains("nesting_depth"))
    );
}
/// A patch entry's depth refusal is a validation-stage codec failure whose
/// leaf is Core (the replacement is in memory, not decoded bytes).
/// 이것을 실패시키는 것: patch·export 입구 깊이 검사의 stage 를 Decode 로 바꾸기.
fn patch_depth_error<T>(result: Result<T, PatchError>) {
    assert!(matches!(
        result,
        Err(PatchError::Codec(CodecFailure {
            stage: CodecStage::Validation,
            code: HwpxErrorCode::Core,
            detail,
        })) if detail.contains("nesting_depth")
    ));
}
struct NoStyles;
impl StyleLookup for NoStyles {}
fn child(name: &str, stack: usize, f: fn()) {
    let key = "HWPFORGE_DEPTH_ENTRY_CHILD";
    if std::env::var(key).ok().as_deref() == Some(name) {
        std::thread::Builder::new().stack_size(stack).spawn(f).unwrap().join().unwrap();
    } else {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env(key, name)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let Some(status) = process.try_wait().unwrap() {
                let output = process.wait_with_output().unwrap();
                assert!(
                    status.success(),
                    "child {name}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                    "child {name} must execute exactly one test: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
                break;
            }
            if std::time::Instant::now() > deadline {
                process.kill().unwrap();
                let _ = process.wait();
                panic!("child timeout");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

// Each entry test fails when its own admission loop is removed. Deep borrowed
// inputs are consumed via validate afterwards; recursive Drop is not under test.
#[test]
fn preservation_export_rejects_unvalidated_section() {
    child("preservation_export_rejects_unvalidated_section", 256 * 1024, || {
        let d = doc(group(100_000));
        let result = HwpxPatcher::export_section_preservation(b"invalid", 0, &d.sections()[0]);
        assert!(d.validate().is_err());
        patch_depth_error(result);
    });
}

#[test]
fn preserving_patch_rejects_unvalidated_section() {
    child("preserving_patch_rejects_unvalidated_section", 256 * 1024, || {
        let d = doc(group(100_000));
        let result = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
            b"invalid",
            0,
            &d.sections()[0],
            None,
            None,
        );
        assert!(d.validate().is_err());
        patch_depth_error(result);
    });
}

#[test]
fn cell_edits_reject_deep_draft() {
    child("cell_edits_reject_deep_draft", 2 * 1024 * 1024, || {
        let mut d = doc(group(100_000));
        let result = apply_set_cells(&mut d, &[]);
        assert!(d.validate().is_err());
        assert!(
            matches!(result, Err(CellEditError::Codec(detail)) if detail.contains("nesting_depth"))
        );
    });
}

fn provenance_base() -> hwpforge_smithy_hwpx::HwpxDocument {
    let styles = hwpforge_smithy_hwpx::presets::style_store_for_preset("default").unwrap();
    let bytes =
        HwpxEncoder::encode(&doc(text()).validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    HwpxDecoder::decode(&bytes).unwrap()
}

#[test]
fn verify_provenance_rejects_deep_sections() {
    child("verify_provenance_rejects_deep_sections", 2 * 1024 * 1024, || {
        let base = provenance_base();
        // A later section must also be checked before any recursive traversal.
        let mut d = doc(text());
        d.add_section(Section::with_paragraphs(vec![para(group(100_000))], PageSettings::a4()));
        let result = HwpxDecoder::verify_source_provenance(d.sections(), &base);
        assert!(d.validate().is_err());
        depth_error(result);
    });
}

#[test]
fn restore_provenance_rejects_deep_draft() {
    child("restore_provenance_rejects_deep_draft", 2 * 1024 * 1024, || {
        let base = provenance_base();
        let mut d = doc(group(100_000));
        let result = HwpxDecoder::restore_source_provenance(&mut d, &base);
        assert!(d.validate().is_err());
        depth_error(result);
    });
}

#[test]
fn restore_provenance_rejects_deep_base() {
    child("restore_provenance_rejects_deep_base", 2 * 1024 * 1024, || {
        let mut base = provenance_base();
        base.document = doc(group(100_000));
        let mut d = doc(text());
        let result = HwpxDecoder::restore_source_provenance(&mut d, &base);
        assert!(base.document.validate().is_err());
        depth_error(result);
    });
}

// 이것을 실패시키는 것: 상한을 31로 내리기, 공통 관문 뒤 표/주석 카운터의
// 잘못된 경계(>=31), 허용 깊이에서 인코드 재귀 프레임 확대.
#[test]
fn accepted_depths_encode_on_small_stack() {
    child("accepted_depths_encode_on_small_stack", 2 * 1024 * 1024, || {
        let styles = hwpforge_smithy_hwpx::presets::style_store_for_preset("default").unwrap();
        for depth in [31, 32] {
            for r in [group(depth), boxes(depth), tables(depth)] {
                let d = doc(r).validate().unwrap();
                assert!(HwpxEncoder::encode(&d, &styles, &ImageStore::new()).is_ok());
                assert!(MdEncoder::encode_lossless(&d).is_ok());
                assert!(MdEncoder::encode_lossy(&d).is_ok());
                assert!(!MdEncoder::encode_styled(&d, &NoStyles)
                    .markdown
                    .contains("중첩 깊이 초과"));
            }
        }
    });
}

// 이것을 실패시키는 것: typed 직후 검사 삭제/주소 검증 뒤로 이동,
// JSON 문자열 parser 재귀 제한 해제, Core 오류를 다른 오류로 매핑.
#[test]
fn json_gates_before_base_and_addresses() {
    child("json_gates_before_base_and_addresses", 2 * 1024 * 1024, || {
        let document = doc(group(33));
        let exported = ExportedDocument { format_version: 2, document, styles: None };
        let json = serde_json::to_string(&exported).unwrap();
        let error = from_json(&json, &FromJsonOptions::default().with_base(b"invalid".to_vec()))
            .unwrap_err();
        assert!(
            matches!(error,hwpforge::ops::OpsError::Core(hwpforge_core::CoreError::InvalidStructure { context, .. }) if context=="nesting_depth")
        );
        let section = Section::with_paragraphs(vec![para(group(33))], PageSettings::a4());
        let exported = ExportedSection {
            format_version: 2,
            section_index: 0,
            section,
            styles: None,
            preservation: None,
        };
        let json = serde_json::to_string(&exported).unwrap();
        let error = patch(b"invalid", &PatchOptions::default().with_patch(json)).unwrap_err();
        assert!(
            matches!(error,hwpforge::ops::OpsError::Core(hwpforge_core::CoreError::InvalidStructure { context, .. }) if context=="nesting_depth")
        );
        let json = format!("{}0{}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(from_json(&json, &FromJsonOptions::default()).is_err());
    });
}
