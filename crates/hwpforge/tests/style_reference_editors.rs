#![cfg(all(feature = "ops-hwpx", feature = "ops-md"))]
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge};
use hwpforge::ops::{edit, stamp, style};

fn source() -> Vec<u8> {
    let template = hwpforge::blueprint::builtins::builtin_default().unwrap();
    let decoded = hwpforge::md::MdDecoder::decode(
        "# Heading\n\nName: (   )\n\n| Label | Value |\n| --- | --- |\n| A | B |",
        &template,
    )
    .unwrap();
    let bridge = HwpxRegistryBridge::from_registry(&decoded.style_registry).unwrap();
    let body = decoded.style_registry.get_style("body").unwrap().style_ref();
    assert_ne!(bridge.style_store().style_position(body).unwrap(), body.get());
    let document = bridge.rebind_draft_document(decoded.document).unwrap().validate().unwrap();
    HwpxEncoder::encode(&document, bridge.style_store(), &Default::default()).unwrap()
}

fn references(bytes: &[u8]) -> Vec<usize> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let mut refs = Vec::new();
    for section in decoded.document.sections() {
        let mut section = section.clone();
        section.for_each_paragraph_mut(|p| {
            refs.push(decoded.style_store.effective_style_position(p.style_ref).unwrap());
        });
    }
    refs
}

// 이것을 실패시키는 것: cell 첫 참조/structural anchor 상속 제거 또는 편집 시 ref0을 wire0으로 오인.
#[test]
fn editors_preserve_connections_from_nonidentity_authoring_store() {
    let bytes = source();
    let before = references(&bytes);
    let cell = edit::set_cell(
        &bytes,
        &edit::SetCellOptions::default().with_table(0).with_at("1,1").with_text("Changed"),
    )
    .unwrap();
    assert_eq!(references(&cell.bytes), before);
    let styled =
        style::restyle(&bytes, &style::RestyleOptions::default().with_preset("modern")).unwrap();
    assert_eq!(references(&styled.bytes), before);
    let inserted = edit::insert_para(
        &bytes,
        &edit::InsertParaOptions::default().with_anchor(0).with_text(vec!["Inserted".into()]),
    )
    .unwrap();
    let mut expected = before.clone();
    expected.insert(1, before[0]);
    assert_eq!(references(&inserted.bytes), expected);

    use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
    let candidates = stamp::stamp_plan(&bytes).unwrap().plan.text;
    assert!(!candidates.is_empty());
    let specs = candidates
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "name".into(), hint: None },
        })
        .collect();
    let stamped = stamp::stamp(&bytes, &StampMap::Legacy(specs), &Default::default()).unwrap();
    assert_eq!(references(&stamped.bytes), before);
    let filled =
        edit::fill(&stamped.bytes, &[("name".into(), "Filled".into())], &Default::default())
            .unwrap();
    assert_eq!(references(&filled.bytes), before);
}
