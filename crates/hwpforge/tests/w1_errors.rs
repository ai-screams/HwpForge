#![cfg(feature = "ops-hwpx")]
use hwpforge::ops::{self, OpsError, ReadOptions};

#[test]
// 이것을 실패시키는 것: OpsError::Io(NotFound)를 FileReadFailed로 변경, 기타 OpsError::Io를 FileNotFound로 변경, read 구역 범위를 InvalidInput으로 변경, read 표 부재를 InvalidInput으로 변경, read 필드 부재를 InvalidInput으로 변경, manifest에 미소비 Rust surface 추가.
fn w1_read_and_io_causes_are_canonical() {
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
    ] {
        consumed.insert(id);
        let case = manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
        assert_eq!(error.code().as_str(), case["expected_code"].as_str().unwrap(), "{id}");
    }
    let declared: std::collections::BTreeSet<_> = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["surfaces"].get("rust").is_some())
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert_eq!(consumed, declared);
}

#[test]
// 이것을 실패시키는 것: 퇴역 variant 7개와 lookup을 복원.
fn w1_retired_codes_are_not_lookup_aliases() {
    // 이것을 실패시키는 것: 퇴역 OpsCode variant와 lookup을 복원.
    for code in [
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
