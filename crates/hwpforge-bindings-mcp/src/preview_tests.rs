#[path = "../../hwpforge/tests/support/preview.rs"]
mod fixture;

// 이것을 실패시키는 것: 공통 encode 경고 코드가 OTHER로 바뀌거나 endpoint가 최종 bytes/경고를 버림.
#[test]
fn preview_is_removed_by_patch_and_fill() {
    let dir = tempfile::tempdir().unwrap();
    let base = fixture::base();
    let input = dir.path().join("base.hwpx");
    std::fs::write(&input, &base).unwrap();
    let json = dir.path().join("section.json");
    std::fs::write(&json, fixture::patch_json(&base)).unwrap();
    let patched = dir.path().join("patched.hwpx");
    let result = crate::tools::patch::run_patch(
        input.to_str().unwrap(),
        0,
        json.to_str().unwrap(),
        patched.to_str().unwrap(),
    )
    .unwrap();
    fixture::assert_warning(&serde_json::to_value(result).unwrap());
    fixture::assert_removed(&std::fs::read(patched).unwrap());
    let filled = dir.path().join("filled.hwpx");
    let result = crate::tools::fill::run_fill(
        input.to_str().unwrap(),
        &[("name".into(), "new".into())].into_iter().collect(),
        filled.to_str().unwrap(),
    )
    .unwrap();
    fixture::assert_warning(&serde_json::to_value(result).unwrap());
    fixture::assert_removed(&std::fs::read(filled).unwrap());
}
