#[path = "support/cell_line_wrap.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read, Write};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// S19: set-cell re-encodes the package and keeps another cell's dormant
/// margin.
///
/// 이것을 실패시키는 것: 디코더가 휴면 값을 읽지 않기 / 인코더가 쓰지 않기.
#[test]
fn set_cell_keeps_a_dormant_margin() {
    let padding = r#"<hp:cellMargin left="510" right="510" top="141" bottom="141"/>"#;
    let own = r#"<hp:cellMargin left="11" right="12" top="13" bottom="14"/>"#;
    let form = fixture("tables/merged_grid_form.hwpx");
    let form = with_section(&form, &section_xml(&form).replacen(padding, own, 1));
    let spec = hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            1, 1,
        )),
        text: "x".to_string(),
    };
    let edited = hwpforge_smithy_hwpx::HwpxCellEditor::set_cells_with_diagnostics(&form, &[spec])
        .expect("set-cell");
    assert_eq!(section_xml(&edited.value.bytes).matches(own).count(), 1);
}
