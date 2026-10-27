//! W3 editor boundaries, with synthetic inputs only.
#![cfg(feature = "hwpx")]
#[path = "support/document.rs"]
mod document;
use document::run_para as paragraph;

use hwpforge::hwpx::{apply_set_cells, CellSpec, CellTarget};
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::{Document, PageSettings, Run, Section, Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, HwpUnit};

fn table(run: Run) -> Run {
    Run::table(
        Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![paragraph(run)],
            HwpUnit::new(100).unwrap(),
        )])]),
        CharShapeIndex::new(0),
    )
}

#[test]
fn draft_depth_reports_the_later_section_before_any_edit() {
    // 이것을 실패시키는 것: D1 section을 0으로 고정, D2 reason 구역 보정 제거, D3 Display 접두 삭제, D4 깊이 검사를 편집 뒤로 이동, D5 ops를 InvalidInput으로 변경.
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(
        vec![paragraph(table(Run::text("before", CharShapeIndex::new(0))))],
        PageSettings::a4(),
    ));
    let mut nested = Run::text("deep", CharShapeIndex::new(0));
    for _ in 0..33 {
        nested = table(nested);
    }
    document.add_section(Section::with_paragraphs(vec![paragraph(nested)], PageSettings::a4()));
    let before = document.clone();
    let error = apply_set_cells(
        &mut document,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(GridCoord::new(0, 0)),
            text: "after".into(),
        }],
    )
    .unwrap_err();
    assert_eq!(document, before);
    let hwpforge::hwpx::CellEditError::NestingTooDeep { section, reason } = &error else {
        panic!("expected Draft depth variant: {error}");
    };
    assert_eq!(*section, 1);
    assert!(reason.contains("section=1"), "{reason}");
    assert_eq!(error.to_string(), format!("nesting depth exceeds the limit: {reason}"));
    #[cfg(feature = "ops-hwpx")]
    {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("data/error_contract_018.json")).unwrap();
        let case =
            manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == DRAFT_CASE).unwrap();
        assert_eq!(
            hwpforge::ops::OpsError::CellEdit(error).code().as_str(),
            case["expected_code"].as_str().unwrap()
        );
    }
}

#[cfg(feature = "ops-hwpx")]
#[test]
fn byte_editor_decode_boundaries_are_canonical() {
    // 이것을 실패시키는 것: P3 CodecFailure::hwpx의 원본 leaf를 XmlSerialize로 덮어쓰기.
    use hwpforge::ops;
    let bytes = include_bytes!("data/corrupt-input.hwpx");
    let errors = [
        ops::edit::set_cell(
            bytes,
            &ops::edit::SetCellOptions::default().with_table(0).with_at("0,0").with_text("text"),
        )
        .unwrap_err(),
        ops::edit::insert_para(
            bytes,
            &ops::edit::InsertParaOptions::default().with_text(vec!["text".into()]),
        )
        .unwrap_err(),
        ops::edit::delete_para(
            bytes,
            &ops::edit::DeleteParaOptions::default().with_indexes(vec![0]),
        )
        .unwrap_err(),
        ops::stamp::stamp(
            bytes,
            &hwpforge::hwpx::stamp::StampMap::Legacy(vec![]),
            &Default::default(),
        )
        .unwrap_err(),
    ];
    let original = hwpforge::hwpx::HwpxDecoder::decode(bytes).unwrap_err();
    for error in errors {
        assert_eq!(error.code().as_str(), "DECODE_FAILED");
        let failure = match error {
            ops::OpsError::CellEdit(hwpforge::hwpx::CellEditError::Codec(failure))
            | ops::OpsError::StructuralEdit(hwpforge::hwpx::StructuralEditError::Codec(failure))
            | ops::OpsError::Stamper(hwpforge::hwpx::stamp::StamperError::Codec(failure)) => {
                failure
            }
            other => panic!("expected typed decode failure: {other}"),
        };
        assert_eq!(failure.stage, hwpforge::hwpx::CodecStage::Decode);
        assert_eq!(failure.code, original.code());
        assert_eq!(failure.detail, original.to_string());
    }
}

#[test]
fn shallow_draft_keeps_table_not_found() {
    // 이것을 실패시키는 것: D6 TableNotFound의 tables를 실제 개수 + 1로 반환.
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(
        vec![paragraph(Run::text("plain", CharShapeIndex::new(0)))],
        PageSettings::a4(),
    ));
    assert!(matches!(
        apply_set_cells(
            &mut document,
            &[CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "after".into()
            }]
        ),
        Err(hwpforge::hwpx::CellEditError::TableNotFound { table: 0, tables: 0 })
    ));
}

#[cfg(feature = "ops-hwpx")]
#[test]
fn four_byte_depth_inputs_fail_at_decode() {
    // 이것을 실패시키는 것: E1 셀 입력 decode stage를 Encode로 변경.
    use hwpforge::hwpx::HwpxDecoder;
    use hwpforge::ops;
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            for &(bytes, leaf, id) in DEPTH_CASES {
                assert_eq!(HwpxDecoder::decode(bytes).unwrap_err().code(), leaf);
                let error = ops::edit::set_cell(
                    bytes,
                    &ops::edit::SetCellOptions::default()
                        .with_table(0)
                        .with_at("0,0")
                        .with_text("text"),
                )
                .unwrap_err();
                let manifest: serde_json::Value =
                    serde_json::from_str(include_str!("data/error_contract_018.json")).unwrap();
                let case =
                    manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
                assert_eq!(error.code().as_str(), case["expected_code"].as_str().unwrap());
                assert!(!error.hint().unwrap_or_default().contains("Report this as a bug."));
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn invariant_and_protected_messages_keep_the_detail() {
    // 이것을 실패시키는 것: M1–M4 세 Invariant 또는 ProtectedFieldRange Display에서 detail 삭제.
    use hwpforge::hwpx::{stamp::StamperError, CellEditError, StructuralEditError};
    assert_eq!(
        CellEditError::Invariant { detail: "cell source".into() }.to_string(),
        "internal invariant violated: cell source"
    );
    assert_eq!(
        StructuralEditError::Invariant { detail: "paragraph source".into() }.to_string(),
        "internal invariant violated: paragraph source"
    );
    assert_eq!(
        StamperError::Invariant { detail: "slot source".into() }.to_string(),
        "internal invariant violated: slot source"
    );
    assert_eq!(
        StructuralEditError::ProtectedFieldRange {
            section: 1,
            index: 2,
            detail: "protected field range: paragraph deletion".into()
        }
        .to_string(),
        "protected field range: paragraph deletion"
    );
}

#[cfg(feature = "ops-hwpx")]
const DRAFT_CASE: &str = "cell.draft_depth";
#[cfg(feature = "ops-hwpx")]
const V2_CASE: &str = "stamp.v2_admission";
#[cfg(feature = "ops-hwpx")]
const DEPTH_CASES: &[(&[u8], hwpforge::hwpx::HwpxErrorCode, &str)] = &[
    (
        include_bytes!("data/depth-table-42.hwpx").as_slice(),
        hwpforge::hwpx::HwpxErrorCode::XmlParse,
        "cell.depth-table-42",
    ),
    (
        include_bytes!("data/depth-table-33.hwpx").as_slice(),
        hwpforge::hwpx::HwpxErrorCode::InvalidStructure,
        "cell.depth-table-33",
    ),
    (
        include_bytes!("data/depth-group-32.hwpx").as_slice(),
        hwpforge::hwpx::HwpxErrorCode::Core,
        "cell.depth-group-32",
    ),
    (
        include_bytes!("data/depth-xml-broken.hwpx").as_slice(),
        hwpforge::hwpx::HwpxErrorCode::XmlParse,
        "cell.depth-xml-broken",
    ),
];

#[cfg(feature = "ops-hwpx")]
#[test]
fn stamp_v2_admission_is_canonical() {
    // 이것을 실패시키는 것: v2 admission 호출 목적을 Output으로 변경.
    let request =
        hwpforge::hwpx::stamp::parse_stamp_map(include_str!("data/stamp-v2-admission.json"))
            .unwrap();
    let error = hwpforge::ops::stamp::stamp(
        include_bytes!("data/edit-empty-section.hwpx"),
        &request,
        &Default::default(),
    )
    .unwrap_err();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/error_contract_018.json")).unwrap();
    let case = manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == V2_CASE).unwrap();
    assert_eq!(error.code().as_str(), case["expected_code"].as_str().unwrap());
}

#[cfg(feature = "ops-hwpx")]
#[test]
fn w3_manifest_cases_are_all_consumed() {
    // 이것을 실패시키는 것: manifest에 rust operation=w3.x case 추가.
    // Use the same IDs as the fixture assertions above, never a separate allowlist.
    let consumed: std::collections::BTreeSet<_> =
        [DRAFT_CASE, V2_CASE].into_iter().chain(DEPTH_CASES.iter().map(|(_, _, id)| *id)).collect();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/error_contract_018.json")).unwrap();
    let declared = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            case["surfaces"]["rust"]["operation"].as_str().is_some_and(|op| op.starts_with("w3."))
        })
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert_eq!(consumed, declared);
}
