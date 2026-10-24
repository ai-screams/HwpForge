//! Public decode admission and operation-specific error codes.
#![cfg(feature = "ops-hwpx")]
#[path = "../../hwpforge-smithy-hwpx/tests/support/depth_package.rs"]
mod support;
use hwpforge::ops::{self, OpsError};
use support::*;

fn code<T: std::fmt::Debug>(result: Result<T, OpsError>, expected: &str) -> OpsError {
    let err = result.unwrap_err();
    assert_eq!(err.code().to_string(), expected, "{err}");
    err
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (inspect 성공으로 첫 오류 단언 실패).
#[test]
fn semantic_overflow_stops_at_decode_with_existing_operation_codes() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        code(ops::inspect::inspect(&bytes, &Default::default()), "DECODE_FAILED");
        code(ops::read::outline(&bytes), "DECODE_FAILED");
        code(
            ops::read::read(&bytes, &ops::read::ReadOptions::default().with_section(0)),
            "DECODE_FAILED",
        );
        code(ops::read::fields(&bytes), "DECODE_FAILED");
        code(ops::diff::diff(&bytes, &bytes), "DECODE_FAILED");
    });
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (코드는 같아도 Core decode 오류가 사라짐).
#[test]
fn stamp_overflow_stops_at_decode() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let err = code(
            ops::stamp::stamp(
                &bytes,
                &hwpforge_smithy_hwpx::stamp::StampMap::Legacy(vec![]),
                &Default::default(),
            ),
            "STAMP_CODEC_FAILED",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (코드는 같아도 Core decode 오류가 사라짐).
#[test]
fn insert_overflow_stops_at_decode() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let err = code(
            ops::edit::insert_para(
                &bytes,
                &ops::edit::InsertParaOptions::default().with_text(vec!["text".into()]),
            ),
            "STRUCTURAL_CODEC",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (코드는 같아도 Core decode 오류가 사라짐).
#[test]
fn delete_overflow_stops_at_decode() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let err = code(
            ops::edit::delete_para(
                &bytes,
                &ops::edit::DeleteParaOptions::default().with_indexes(vec![0]),
            ),
            "STRUCTURAL_CODEC",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (코드는 같아도 Core decode 오류가 사라짐).
#[test]
fn set_cell_overflow_stops_at_decode() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let err = code(
            ops::edit::set_cell(
                &bytes,
                &ops::edit::SetCellOptions::default()
                    .with_table(0)
                    .with_at("0,0")
                    .with_text("text"),
            ),
            "SET_CELL_CODEC_FAILED",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: 공개 decode Core 관문 삭제 (코드는 같아도 Core decode 오류가 사라짐).
#[test]
fn patch_overflow_stops_at_decode() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let clean = package(nested_section("T", 1), Place::Body);
        let exported = ops::exchange::export_section(&clean, &Default::default()).unwrap();
        let err = code(
            ops::exchange::patch(
                &bytes,
                &ops::exchange::PatchOptions::default()
                    .with_patch(serde_json::to_string(&exported.section).unwrap()),
            ),
            "PATCH_FAILED",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: decode Core 관문 삭제 → GRID_ADDR_PROJECTION_FAILED.
#[test]
fn to_json_overflow_is_decode_failure() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        code(ops::exchange::to_json(&bytes, &Default::default()), "DECODE_FAILED");
    });
}

// 이것을 실패시키는 것: decode Core 관문 삭제 → 필드 inventory 진단(E4006).
#[test]
fn fill_overflow_keeps_code_but_moves_before_inventory() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        let err = code(
            ops::edit::fill(&bytes, &[("field".into(), "value".into())], &Default::default()),
            "FILL_FAILED",
        );
        assert!(err.to_string().contains("Core error:"), "{err}");
    });
}

// 이것을 실패시키는 것: decode Core 관문 삭제 → stamp-plan 성공.
#[test]
fn stamp_plan_overflow_becomes_codec_failure() {
    on_big_stack(|| {
        let bytes = package(section_with(&groups(32)), Place::Body);
        code(ops::stamp::stamp_plan(&bytes), "STAMP_CODEC_FAILED");
    });
}
