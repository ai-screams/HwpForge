#[path = "../../hwpforge/tests/support/preview.rs"]
mod fixture;
use pyo3::{
    prelude::*,
    types::{PyBytes, PyString},
};

// 이것을 실패시키는 것: 공통 encode 경고 코드가 OTHER로 바뀌거나 Python 반환 bytes/메타데이터가 분리됨.
#[test]
fn preview_is_removed_by_patch_and_fill() {
    Python::initialize();
    Python::attach(|py| {
        let base = fixture::base();
        let patch = fixture::patch_json(&base);
        let patched = crate::exchange::patch(
            py,
            PyBytes::new(py, &base).extract().unwrap(),
            0,
            PyString::new(py, &patch).extract().unwrap(),
        )
        .unwrap();
        let filled = crate::edit::fill(
            py,
            PyBytes::new(py, &base).extract().unwrap(),
            [("name".into(), "new".into())].into_iter().collect(),
            false,
        )
        .unwrap();
        for (bytes, metadata) in [patched, filled] {
            fixture::assert_removed(bytes.as_bytes());
            let json: String = py
                .import("json")
                .unwrap()
                .call_method1("dumps", (&metadata,))
                .unwrap()
                .extract()
                .unwrap();
            fixture::assert_warning(&serde_json::from_str(&json).unwrap());
        }
    });
}

// 이것을 실패시키는 것: Python 원문 삭제 성공을 거부하거나 bytes/Preview 경고를 다르게 반환.
#[test]
fn preserving_delete_matches_codec_and_keeps_failure_without_bytes() {
    Python::initialize();
    Python::attach(|py| {
        let base = fixture::paragraph_base();
        let codec = hwpforge::hwpx::HwpxStructuralEditor::delete_paragraphs(
            &base,
            &[hwpforge::hwpx::ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        let (bytes, metadata) = crate::edit::delete_para(
            py,
            PyBytes::new(py, &base).extract().unwrap(),
            0,
            vec![1],
            false,
        )
        .unwrap();
        assert_eq!(bytes.as_bytes(), codec);
        fixture::assert_removed(bytes.as_bytes());
        let json: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (&metadata,))
            .unwrap()
            .extract()
            .unwrap();
        fixture::assert_warning(&serde_json::from_str(&json).unwrap());
        assert!(crate::edit::delete_para(
            py,
            PyBytes::new(py, &base).extract().unwrap(),
            0,
            vec![0],
            false
        )
        .is_err());
    });
}
