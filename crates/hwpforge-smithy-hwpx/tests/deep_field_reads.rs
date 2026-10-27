//! Read-only field consumers retain the 6e6cc1be result at depth 33.
#[path = "support/document.rs"]
mod document;
use document::{control_run as c, end_field as end, runs_para as p, text_run as text};
use hwpforge_core::control::{FieldBoundary, FieldStart};
use hwpforge_core::field_inventory::FieldInventory;
use hwpforge_core::table::{TableCell, TableRow};
use hwpforge_core::{Control, Document, PageSettings, Run, Section, Table};
use hwpforge_foundation::{CharShapeIndex, HwpUnit};
use hwpforge_smithy_hwpx::stamp;
fn start(id: &str, kind: &str) -> Run {
    let mut start = FieldStart::default();
    start.id = id.into();
    start.kind = kind.into();
    start.name = "deep-name".into();
    c(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) })
}
fn fixture() -> Document {
    let table = || {
        Run::table(
            Table::new(vec![TableRow::new(vec![
                TableCell::new(vec![p(vec![text("Name")])], HwpUnit::ZERO),
                TableCell::new(vec![p(vec![text("")])], HwpUnit::ZERO),
            ])]),
            CharShapeIndex::new(0),
        )
    };
    let mut scalar = Control::field("scalar");
    if let Control::Field { display_text, name, .. } = &mut scalar {
        *display_text = "scalar-value".into();
        *name = Some("scalar-name".into());
    }
    let mut leaf = p(vec![
        c(scalar),
        start("memo-view-1", "CLICK_HERE"),
        text("@ protected"),
        end("memo-view-1"),
        start("formula", "FORMULA"),
        text("@ computed"),
        table(),
        end("formula"),
        text("@ visible"),
        table(),
    ]);
    for _ in 0..32 {
        leaf = p(vec![c(Control::text_box(vec![leaf], HwpUnit::ZERO, HwpUnit::ZERO))]);
    }
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(
        vec![leaf, p(vec![c(Control::memo_with_anchor(vec![], vec![text("@ anchor")]))])],
        PageSettings::a4(),
    ));
    d
}
fn read_log(d: &Document) -> String {
    let fields = FieldInventory::from_sections(d.sections());
    let names: Vec<_> = fields
        .fields()
        .iter()
        .filter(|f| f.kind() == "CLICK_HERE")
        .filter_map(|f| f.name().clone())
        .collect();
    let protection: Vec<_> = fields
        .fields()
        .iter()
        .flat_map(|f| f.body().iter().map(|b| fields.protects(f.section(), b.path())))
        .collect();
    let expanded = hwpforge_core::field_view::expand_sections(d.sections());
    let expanded_fields = FieldInventory::from_sections(&expanded);
    let ids: Vec<_> = expanded_fields.fields().iter().map(|f| f.id().clone()).collect();
    let current: Vec<_> = fields.fields().iter().map(|f| f.current()).collect();
    format!("names={names:?}\nprotects={protection:?}\ncurrent={current:?}\nexpanded_ids={ids:?}\nplan={:?}\ncells={:?}\n", stamp::plan(d), stamp::plan_cells(d))
}
// 이것을 실패시키는 것: 깊이 진단 후 inventory 조기 반환 (읽기 소비처의 필드/보호 누락).
#[test]
fn deep_field_reads_baseline() {
    let d = fixture();
    let log = read_log(&d);
    println!("BASELINE_BEGIN\n{log}BASELINE_END");
    assert_eq!(log, include_str!("deep_field_reads_baseline.txt"));
}
// 이것을 실패시키는 것: diagnostics를 유지한 변경 거부 삭제.
#[test]
fn deep_field_edits_remain_rejected() {
    let source = fixture();
    let mut d = fixture();
    let inventory = FieldInventory::from_sections(source.sections());
    assert_eq!(inventory.diagnostics().len(), 1);
    assert!(source.sections()[0]
        .check_nesting_depth()
        .unwrap_err()
        .to_string()
        .contains("nesting depth 33 exceeds 32"));
    assert!(!inventory.fields().is_empty());
    assert!(!FieldInventory::replace_values(
        d.sections_mut(),
        &[(&inventory.fields()[0], "changed")]
    ));
    assert!(!FieldInventory::insert_paragraphs(d.sections_mut(), 0, 0, vec![p(vec![text("new")])]));
    d.sections_mut()[0].paragraphs.push(p(vec![text("unprotected")]));
    assert!(!FieldInventory::delete_paragraphs(d.sections_mut(), 0, &[2]));
    d.sections_mut()[0].paragraphs.pop();
    FieldInventory::clear_editable_values(d.sections_mut(), 0);
    assert_eq!(read_log(&d), read_log(&source));
}
