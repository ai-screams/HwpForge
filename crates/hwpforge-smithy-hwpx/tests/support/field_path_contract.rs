//! Byte contract captured from 71dd373c before path-storage changes.
use super::*;
use hwpforge_core::field_inventory::FieldInventory;
use hwpforge_core::node_path::CorePath;
#[path = "field_path_fixtures.rs"]
mod fixtures;

fn snapshot() -> String {
    let mut lines = Vec::new();
    for (name, document) in fixtures::fixtures() {
        for offset in [0, 7] {
            let inventory = FieldInventory::from_sections_at(document.sections(), offset);
            lines.push(format!(
                "fixture {name} offset {offset} stored {} diagnostics {:?} warnings {:?}",
                inventory.stored_run_count(),
                inventory.diagnostics(),
                inventory.warnings()
            ));
            for f in inventory.fields() {
                lines.push(format!(
                    "field {:?}",
                    (
                        f.section(),
                        f.kind(),
                        f.name(),
                        f.id(),
                        f.flow(),
                        f.start(),
                        f.end(),
                        f.hint(),
                        f.editability(),
                        f.current(),
                        f.body().iter().map(|b| b.path()).collect::<Vec<_>>()
                    )
                ));
            }
            let ids = |fs: Vec<&hwpforge_core::field_inventory::FieldEntry<'_>>| {
                fs.iter()
                    .map(|f| {
                        inventory.fields().iter().position(|entry| std::ptr::eq(entry, *f)).unwrap()
                    })
                    .collect::<Vec<_>>()
            };
            let mut probe = |si, ps: &[Paragraph], flow: &str| {
                lines.push(format!("flow {si} {flow} {}", ps.len()));
                for pi in 0..=ps.len() {
                    lines.push(format!(
                        "paragraph {pi} {} insert {}",
                        inventory.protects_paragraph(si, flow, pi),
                        inventory.protects_insertion(si, flow, pi, None)
                    ));
                    for ri in 0..=ps.get(pi).map_or(0, |p| p.runs.len()) {
                        let path = format!("{flow}[{pi}].runs[{ri}]");
                        lines.push(format!(
                            "subtree {pi}/{ri} {}",
                            inventory.protects_paragraph(si, &format!("{flow}[{pi}].runs"), ri)
                        ));
                        lines.push(format!(
                            "insert {ri} {}",
                            inventory.protects_insertion(si, flow, pi, Some(ri))
                        ));
                        for suffix in ["", ".text", ".control.field", ".imaginary.child"] {
                            let path = format!("{path}{suffix}");
                            lines.push(format!(
                                "query {pi}/{ri}{suffix} {:?} {:?} {} {}",
                                ids(inventory.fields_at(si, &path).collect()),
                                ids(inventory.at_path(&CorePath::new(si, &path)).collect()),
                                inventory.contains_body(si, &path),
                                inventory.protects(si, &path)
                            ));
                        }
                    }
                }
            };
            FieldInventory::visit_flows(document.sections(), |si, ps, flow| {
                probe(si + offset, ps, flow)
            });
            // Memo anchors are run flows, not visit_flows paragraph flows.
            for f in inventory.fields().iter().filter(|f| f.kind() == "MEMO") {
                if let Some(Control::Memo { anchor_runs, .. }) = f.body()[0].content().as_control()
                {
                    let anchor = Paragraph::with_runs(
                        anchor_runs.clone(),
                        hwpforge_foundation::ParaShapeIndex::new(0),
                    );
                    probe(f.section(), &[anchor], &format!("{}.control.memo.anchor", f.start()));
                }
            }
        }
        lines.push(format!(
            "plan {}",
            serde_json::to_string(&crate::stamp::plan(&document)).unwrap()
        ));
        lines.push(format!(
            "cells {}",
            serde_json::to_string(&crate::stamp::plan_cells(&document)).unwrap()
        ));
        for (si, section) in document.sections().iter().enumerate() {
            for patchable in [false, true] {
                let slots = collect_text_slots(section, si, patchable);
                lines.push(format!(
                    "slots {si} {patchable} {:?}",
                    slots.iter().map(|s| (&s.path, &s.text, s.inline)).collect::<Vec<_>>()
                ));
            }
        }
    }
    lines.join("\n") + "\n"
}
// 이것을 실패시키는 것: 상위 보호 prefix 생략, 빈 문단까지 보호, cursor 복구 길이 오류,
// Memo anchor 생략, 필드 변형/flow 순서 누락, slot suffix 또는 순서 변경.
#[test]
fn field_path_contract() {
    let actual = snapshot();
    assert_eq!(actual, include_str!("../data/field_path_contract.txt"));
}
