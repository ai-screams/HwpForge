use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use serde_json::json;

#[test]
fn output_defaults_fill_intervening_positions_after_validation() {
    // 이것을 실패시키는 것: char 기본값 2만 추가하여 첫 위치 id=2인 희소 헤더 출력.
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_smithy_hwpx::HwpxStyleStore;
    let mut section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("x", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    section.page_border_fills = Some(vec![]); // page 1 does not request a default.
    let mut doc = Document::new();
    doc.add_section(section);
    let out = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &HwpxStyleStore::with_default_fonts("바탕"),
        &ImageStore::new(),
    )
    .unwrap();
    let d = HwpxDecoder::decode(&out).unwrap();
    assert_eq!(d.style_store.iter_border_fills().map(|f| f.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(d.style_store.char_shape(CharShapeIndex::new(0)).unwrap().border_fill_id, Some(2));
}

#[test]
fn registry_authoring_keys_lower_to_wire_positions() {
    // 이것을 실패시키는 것: registry 목록/탭의 원본 id를 HWPX 참조로 내보내기.
    use hwpforge_blueprint::{registry::StyleRegistry, style::ParaShape};
    use hwpforge_core::{NumberingDef, ParagraphListRef, TabDef};
    use hwpforge_foundation::NumberingIndex;
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut numbering = NumberingDef::default_outline();
    numbering.id = 42;
    registry.numberings.push(numbering);
    registry.tabs.push(TabDef {
        id: 99,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![],
    });
    let mut para = ParaShape::default();
    para.list = Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 0 });
    para.tab_def_id = 99;
    registry.para_shapes.push(para);
    let store = hwpforge_smithy_hwpx::HwpxStyleStore::from_registry(&registry).unwrap();
    let shape = store.iter_para_shapes().last().unwrap();
    assert_eq!(shape.heading_id_ref, 1);
    assert_eq!(shape.tab_pr_id_ref, 3);
}

#[test]
fn fix8b_border_push_returns_body_reference_position() {
    // 이것을 실패시키는 것: push/get이 label을 사용하거나 출력이 label로 참조 재결합.
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_smithy_hwpx::{style_store::HwpxBorderFill, HwpxStyleStore};
    for labels in [vec![2, 1], vec![99]] {
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        for (index, id) in labels.into_iter().enumerate() {
            let mut border = HwpxBorderFill::default_page_border();
            border.id = id;
            border.left.line_type = "SOLID".into();
            border.left.color = if index == 0 { "#FF0000" } else { "#0000FF" }.into();
            let reference = store.push_border_fill(border.clone());
            assert_eq!(reference, index as u32 + 1);
            assert_eq!(store.border_fill(reference).unwrap(), &border);
        }
        assert!(store.border_fill(0).is_err());
        assert!(store.border_fill(store.border_fill_count() as u32 + 1).is_err());
        for reference in 1..=store.border_fill_count() as u32 {
            let mut section = Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::text("x", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            );
            section.page_border_fills = Some(vec![serde_json::from_value(json!({
                "apply_type": "BOTH", "border_fill_id": reference
            }))
            .unwrap()]);
            let mut doc = Document::new();
            doc.add_section(section);
            let out =
                HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
            let decoded = HwpxDecoder::decode(&out).unwrap();
            let selected = decoded.document.sections()[0].page_border_fills.as_ref().unwrap()[0]
                .border_fill_id;
            assert_eq!(
                decoded.style_store.border_fill(selected).unwrap().left,
                store.border_fill(reference).unwrap().left
            );
            assert_eq!(decoded.style_store.border_fill(selected).unwrap().id, reference);
        }
    }
}
