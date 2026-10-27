//! Generated examples must stay byte-identical to their committed inputs.

#[allow(dead_code)]
#[path = "../examples/merged_cells.rs"]
mod merged_cells;
#[allow(dead_code)]
#[path = "../examples/page_and_picture.rs"]
mod page_and_picture;

// 이것을 실패시키는 것: SimpleTable.hwpx 바이트 복사본의 한 바이트 변경.
#[test]
fn merged_cells_matches_committed_fixture() {
    let generated = merged_cells::build_merged_cells();
    let committed = include_bytes!("fixtures/SimpleTable.hwpx").as_slice();
    assert!(generated == committed, "SimpleTable.hwpx differs from its generator");
}

// 이것을 실패시키는 것: garden_notes.hwpx 바이트 복사본의 한 바이트 변경.
#[test]
fn garden_notes_matches_committed_fixture() {
    let generated = page_and_picture::build_garden_notes();
    let committed = include_bytes!("fixtures/garden_notes.hwpx").as_slice();
    assert!(generated == committed, "garden_notes.hwpx differs from its generator");
}

// 이것을 실패시키는 것: green_swatch.hwpx 바이트 복사본의 한 바이트 변경.
#[test]
fn green_swatch_matches_committed_fixture() {
    let generated = page_and_picture::build_green_swatch();
    let committed = include_bytes!("fixtures/green_swatch.hwpx").as_slice();
    assert!(generated == committed, "green_swatch.hwpx differs from its generator");
}

// 이것을 실패시키는 것: small_page.hwpx 바이트 복사본의 한 바이트 변경.
#[test]
fn small_page_matches_committed_fixture() {
    let generated = page_and_picture::build_small_page();
    let committed = include_bytes!("fixtures/small_page.hwpx").as_slice();
    assert!(generated == committed, "small_page.hwpx differs from its generator");
}

// 이것을 실패시키는 것: garden_columns.hwpx 바이트 복사본의 한 바이트 변경.
#[test]
fn garden_columns_matches_committed_fixture() {
    let generated = page_and_picture::build_garden_columns();
    let committed = include_bytes!("fixtures/garden_columns.hwpx").as_slice();
    assert!(generated == committed, "garden_columns.hwpx differs from its generator");
}
