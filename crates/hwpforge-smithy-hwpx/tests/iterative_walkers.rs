//! Public Draft boundaries and error-free planning at excessive depth.
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{grid_addr, stamp, HwpxError, HwpxRegistryBridge};

fn document(depth: usize, text: &str) -> Document {
    let mut p =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    for _ in 0..depth {
        p = Paragraph::with_runs(
            vec![Run::control(
                Control::text_box(vec![p], HwpUnit::ZERO, HwpUnit::ZERO),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
    }
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![p], PageSettings::a4()));
    d
}

// 이것을 실패시키는 것: stamp::apply 관문 삭제 또는 32에서 거부.
#[test]
fn apply_depth_gate() {
    assert!(stamp::apply(&mut document(32, "text"), &[]).is_ok());
    assert!(
        matches!(stamp::apply(&mut document(33, "text"), &[]), Err(stamp::StampError::UnknownSpec { path, .. }) if path.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: stamp::apply_cells 관문 삭제 또는 32에서 거부.
#[test]
fn apply_cells_depth_gate() {
    assert!(stamp::apply_cells(&mut document(32, "text"), &[]).is_ok());
    assert!(
        matches!(stamp::apply_cells(&mut document(33, "text"), &[]), Err(stamp::CellStampError::TableGridInvalid { detail, .. }) if detail.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: annotate_document_addresses 관문 삭제.
#[test]
fn annotate_document_depth_gate() {
    assert!(grid_addr::annotate_document_addresses(
        &mut serde_json::Value::Null,
        &document(32, "text")
    )
    .is_ok());
    assert!(
        matches!(grid_addr::annotate_document_addresses(&mut serde_json::Value::Null, &document(33, "text")), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: annotate_section_addresses 관문 삭제.
#[test]
fn annotate_section_depth_gate() {
    assert!(grid_addr::annotate_section_addresses(
        &mut serde_json::Value::Null,
        &document(32, "text").sections()[0],
        4
    )
    .is_ok());
    assert!(
        matches!(grid_addr::annotate_section_addresses(&mut serde_json::Value::Null, &document(33, "text").sections()[0], 4), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: verify_document_addresses 관문 삭제.
#[test]
fn verify_document_depth_gate() {
    assert!(grid_addr::verify_document_addresses(&serde_json::Value::Null, &document(32, "text"))
        .is_ok());
    assert!(
        matches!(grid_addr::verify_document_addresses(&serde_json::Value::Null, &document(33, "text")), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: verify_section_addresses 관문 삭제.
#[test]
fn verify_section_depth_gate() {
    assert!(grid_addr::verify_section_addresses(
        &serde_json::Value::Null,
        &document(32, "text").sections()[0],
        4
    )
    .is_ok());
    assert!(
        matches!(grid_addr::verify_section_addresses(&serde_json::Value::Null, &document(33, "text").sections()[0], 4), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("nesting_depth"))
    );
}
// 이것을 실패시키는 것: rebind_draft_document 관문 삭제.
#[test]
fn rebind_depth_gate() {
    let template = hwpforge_blueprint::builtins::builtin_default().unwrap();
    let registry = hwpforge_blueprint::registry::StyleRegistry::from_template(&template).unwrap();
    let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
    assert!(bridge.rebind_draft_document(document(32, "text")).is_ok());
    assert!(
        matches!(bridge.rebind_draft_document(document(33, "text")), Err(HwpxError::Core(hwpforge_core::CoreError::InvalidStructure { context, .. })) if context == "nesting_depth")
    );
}

#[path = "support/small_stack.rs"]
mod small_stack;
use small_stack::small_stack_child;

// 이것을 실패시키는 것: flow/text slot 재귀 복원 또는 초과 깊이의 빈 계획 반환.
// 깊이 2,000: 512 KiB 자식에서 6e6cc1be의 재귀 flow 복원 변이를 실측.
// 1,000 통과, 2,000에서 abort(3,000도 abort); 1,000 → 2,000 → 3,000 후보 중 첫 실패 깊이.
#[test]
fn deep_plan_small_stack() {
    small_stack_child("deep_plan_small_stack", || {
        let d = document(2_000, "@ ");
        let candidates = stamp::plan(&d);
        assert!(candidates.len() == 1);
        assert!(candidates[0].marker == "@");
        assert!(candidates[0].path.ends_with(".text"));
    });
}

// 이것을 실패시키는 것: flow의 Group 재귀 복원 또는 Group 하강 누락.
// 깊이 3,000: 512 KiB 자식에서 6e6cc1be의 재귀 flow 복원 변이를 실측.
// 1,000·2,000 통과, 3,000에서 abort; 1,000 → 2,000 → 3,000 후보 중 첫 실패 깊이.
#[test]
fn deep_group_plan_small_stack() {
    small_stack_child("deep_group_plan_small_stack", || {
        use hwpforge_core::control::GroupChild;
        let leaf = Paragraph::with_runs(
            vec![Run::text("@ ", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let mut control = Control::text_box(vec![leaf], HwpUnit::ZERO, HwpUnit::ZERO);
        for _ in 0..3_000 {
            control = Control::Group {
                children: vec![GroupChild::Shape(control)],
                width: HwpUnit::ZERO,
                height: HwpUnit::ZERO,
                frame: hwpforge_core::ObjectFrame::new(),
                placement: None,
                inst_id: None,
            };
        }
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(control, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let candidates = stamp::plan(&d);
        assert!(candidates.len() == 1 && candidates[0].marker == "@");
    });
}

// 이것을 실패시키는 것: flow 재귀 복원 또는 깊은 plan_cells 결과 누락.
// 깊이 1,000: 512 KiB 자식에서 6e6cc1be의 재귀 flow 복원 변이를 실측.
// 1,000에서 abort(2,000·3,000도 abort); 1,000 → 2,000 → 3,000 후보 중 첫 실패 깊이.
#[test]
fn deep_plan_cells_small_stack() {
    small_stack_child("deep_plan_cells_small_stack", || {
        let mut d = document(1_000, "@ ");
        d.for_each_paragraph_mut(|p| {
            if p.text_content() == "@ " {
                use hwpforge_core::table::{Table, TableCell, TableRow};
                let cell = |text| {
                    TableCell::new(
                        vec![Paragraph::with_runs(
                            vec![Run::text(text, CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        )],
                        HwpUnit::ZERO,
                    )
                };
                p.runs = vec![Run::table(
                    Table::new(vec![TableRow::new(vec![cell("Name"), cell("")])]),
                    CharShapeIndex::new(0),
                )];
            }
        });
        let cells = stamp::plan_cells(&d);
        assert!(cells.candidates.len() == 1 && cells.skipped_tables.is_empty());
        assert!(cells.candidates[0].table == 0);
    });
}

// 이것을 실패시키는 것: apply_cells/grid_addr 관문을 재귀 inventory 뒤로 이동.
#[test]
fn deep_apply_cells_gate_small_stack() {
    small_stack_child("deep_apply_cells_gate_small_stack", || {
        assert!(
            matches!(stamp::apply_cells(&mut document(10_000, "x"), &[]), Err(stamp::CellStampError::TableGridInvalid { detail, .. }) if detail.contains("nesting depth 10000 exceeds 32"))
        );
    });
}
#[test]
fn deep_grid_gate_small_stack() {
    small_stack_child("deep_grid_gate_small_stack", || {
        assert!(
            matches!(grid_addr::verify_document_addresses(&serde_json::Value::Null, &document(10_000, "x")), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("nesting depth 10000 exceeds 32"))
        );
    });
}

// 이것을 실패시키는 것: 단일 구역 측정의 section=0을 실제 구역 번호로 보정하지 않음.
#[test]
fn depth_gate_section_indices() {
    let mut d = Document::new();
    d.add_section(Section::new(PageSettings::a4()));
    d.add_section(std::mem::replace(
        &mut document(33, "x").sections_mut()[0],
        Section::new(PageSettings::a4()),
    ));
    assert!(
        matches!(stamp::apply(&mut d, &[]), Err(stamp::StampError::UnknownSpec { section: 1, path, .. }) if path.contains("section=1") && !path.contains("section=0"))
    );
    assert!(
        matches!(stamp::apply_cells(&mut d, &[]), Err(stamp::CellStampError::TableGridInvalid { detail, .. }) if detail.contains("section=1") && !detail.contains("section=0"))
    );
    assert!(
        matches!(grid_addr::verify_document_addresses(&serde_json::Value::Null, &d), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("section=1") && !path.contains("section=0"))
    );
    assert!(
        matches!(grid_addr::annotate_section_addresses(&mut serde_json::Value::Null, &d.sections()[1], 7), Err(grid_addr::GridAddrError::ShapeMismatch { path }) if path.contains("section=7") && !path.contains("section=0"))
    );
    let template = hwpforge_blueprint::builtins::builtin_default().unwrap();
    let registry = hwpforge_blueprint::registry::StyleRegistry::from_template(&template).unwrap();
    let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
    assert!(
        matches!(bridge.rebind_draft_document(d), Err(HwpxError::Core(hwpforge_core::CoreError::InvalidStructure { context, reason })) if context == "nesting_depth" && reason.contains("section=1") && !reason.contains("section=0"))
    );
}
