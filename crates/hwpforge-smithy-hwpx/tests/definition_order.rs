//! W3 Hancom probes: id labels are decorative; references select list positions.
use hwpforge_core::StyleLookup;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxDocument, HwpxEncoder, PackageReader};
use serde_json::{json, Value};

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/definition_order/{name}.hwpx",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn decode(name: &str) -> HwpxDocument {
    HwpxDecoder::decode(&bytes(name)).unwrap()
}
fn selected(name: &str, kind: &str) -> Value {
    let d = decode(name);
    let paras = &d.document.sections()[0].paragraphs;
    let shape = |p: usize| d.style_store.para_shape(paras[p].para_shape_id).unwrap();
    match kind {
        "borderFill" => {
            let id = d.document.sections()[0].page_border_fills.as_ref().unwrap()[0].border_fill_id;
            let mut value = serde_json::to_value(d.style_store.border_fill(id).unwrap()).unwrap();
            value.as_object_mut().unwrap().remove("id");
            value
        }
        "tabPr" => {
            json!(d.style_store.iter_tabs().nth(shape(0).tab_pr_id_ref as usize).unwrap().stops)
        }
        "numbering" => json!(
            d.style_store
                .iter_numberings()
                .nth(shape(3).heading_id_ref as usize - 1)
                .unwrap()
                .levels
        ),
        "bullet" => json!(
            d.style_store
                .iter_bullets()
                .nth(shape(1).heading_id_ref as usize - 1)
                .unwrap()
                .bullet_char
        ),
        "font" => json!(d.style_store.char_font_axis_names(paras[0].runs[0].char_shape_id)),
        "style" => {
            let style = d.style_store.style(paras[0].style_id.map_or(0, |i| i.get())).unwrap();
            let para = d
                .style_store
                .para_shape(ParaShapeIndex::new(style.para_pr_id_ref as usize))
                .unwrap();
            let ch = d
                .style_store
                .char_shape(CharShapeIndex::new(style.char_pr_id_ref as usize))
                .unwrap();
            json!([para, ch])
        }
        _ => unreachable!(),
    }
}

#[test]
fn hancom_resaves_select_the_same_definitions() {
    // 이것을 실패시키는 것: 각 종류를 id로 조회, 0/1 시작 혼동, 언어별 font 순서 무시.
    for (name, kind) in [
        ("p1-borderfill", "borderFill"),
        ("p2-tab", "tabPr"),
        ("p3-numbering", "numbering"),
        ("p4-bullet", "bullet"),
        ("p5-font", "font"),
        ("p6-style", "style"),
    ] {
        assert_eq!(selected(name, kind), selected(&format!("{name}-hancom"), kind), "{kind}");
        let d = decode(name);
        assert!(d.warnings.iter().any(|w| matches!(w, DecodeWarning::InactiveHeaderReference { kind: k, reason, .. } if k.starts_with(&format!("definition_order:{kind}")) && reason.contains("id ") && reason.contains("position "))));
        let output =
            HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
                .unwrap();
        let reread = HwpxDecoder::decode(&output).unwrap();
        assert!(!reread.warnings.iter().any(|w| matches!(w, DecodeWarning::InactiveHeaderReference { kind, .. } if kind.starts_with("definition_order:"))));
    }
}

#[test]
fn para_and_char_order_follow_hancom_pdf_observations() {
    // 이것을 실패시키는 것: W1의 원본 id 보존 조회. 이 두 탐침의 정답은 사용자 PDF 실측.
    let original = decode("e23-orig-nocache");
    let para = decode("e23-order-swap");
    assert_eq!(
        para.style_store.para_shape(ParaShapeIndex::new(20)).unwrap(),
        original.style_store.para_shape(ParaShapeIndex::new(22)).unwrap()
    );
    assert_eq!(para.document.sections()[0].paragraphs[1].para_shape_id.get(), 20);
    let ch = decode("e23-charpr-swap");
    assert_eq!(ch.style_store.char_shape(CharShapeIndex::new(0)).unwrap().height.as_i32(), 900);
    assert_eq!(ch.document.sections()[0].paragraphs[1].runs[0].char_shape_id.get(), 0);
}

#[test]
fn synthetic_stale_next_style_uses_self_with_explicit_loss() {
    // 이것을 실패시키는 것: nextStyle을 자기 자신 대신 0으로 복구 또는 ContentLost kind 변조.
    let original = bytes("e23-orig-nocache");
    let mut header = PackageReader::new(&original).unwrap().read_header_xml().unwrap();
    // Pick a nonzero style so a hard-coded fallback to zero cannot pass.
    let style = header.match_indices("<hh:style ").nth(1).unwrap().0;
    let start =
        style + header[style..].find("nextStyleIDRef=\"").unwrap() + "nextStyleIDRef=\"".len();
    let end = start + header[start..].find('"').unwrap();
    header.replace_range(start..end, "65535");
    let d = HwpxDecoder::decode(&replace_header(&original, &header)).unwrap();
    let losses: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| {
            matches!(w,
        DecodeWarning::ContentLost { kind, reason, .. }
        if kind == "style.nextStyleIDRef" && reason.contains("replaced with self"))
        })
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(d.style_store.iter_styles().nth(1).unwrap().next_style_id_ref, 1);
    let out = HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
        .unwrap();
    assert!(!HwpxDecoder::decode(&out).unwrap().warnings.iter().any(|w| matches!(w,
        DecodeWarning::ContentLost { kind, .. } if kind == "style.nextStyleIDRef")));
    assert!(HwpxDecoder::decode(&bytes("u5-id-gap")).is_err());
}

#[test]
fn u5_delete_definition_rebind_positions_and_emit_dense_ids() {
    // 이것을 실패시키는 것: 삭제 후 원본 id 10..22 출력하여 한컴 선택이 한 칸 밀리는 것.
    let d = decode("e23-orig-nocache");
    let expected: Vec<_> = d.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| d.style_store.para_shape(p.para_shape_id).unwrap().clone())
        .collect();
    let mut store = serde_json::to_value(&d.style_store).unwrap();
    store["para_shapes"].as_array_mut().unwrap().remove(9);
    // The unused style owning para 9 is removed with that definition, as in U5.
    let styles = store["styles"].as_array_mut().unwrap();
    let removed_style =
        styles.iter().position(|s| s["style_type"] == "PARA" && s["para_pr_id_ref"] == 9).unwrap();
    styles.remove(removed_style);
    for (index, style) in styles.iter_mut().enumerate() {
        let next = style["next_style_id_ref"].as_u64().unwrap() as usize;
        style["next_style_id_ref"] = json!(if next == removed_style {
            index
        } else if next > removed_style {
            next - 1
        } else {
            next
        });
        if style["style_type"] == "PARA" {
            let id = style["para_pr_id_ref"].as_u64().unwrap();
            assert_ne!(id, 9);
            if id > 9 {
                style["para_pr_id_ref"] = json!(id - 1);
            }
        }
    }
    let mut doc = d.document;
    doc.for_each_paragraph_mut(|p| {
        assert_ne!(p.para_shape_id.get(), 9);
        if p.para_shape_id.get() > 9 {
            p.para_shape_id = ParaShapeIndex::new(p.para_shape_id.get() - 1);
        }
    });
    let store = serde_json::from_value(store).unwrap();
    let out = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &d.image_store).unwrap();
    let decoded = HwpxDecoder::decode(&out).unwrap();
    for (p, expected) in decoded.document.sections()[0].paragraphs.iter().zip(expected) {
        assert_eq!(decoded.style_store.para_shape(p.para_shape_id).unwrap(), &expected);
    }
    let header = PackageReader::new(&out).unwrap().read_header_xml().unwrap();
    for id in 0..22 {
        assert!(header.contains(&format!("<hh:paraPr id=\"{id}\"")));
    }
    assert!(!header.contains("<hh:paraPr id=\"22\""));
    assert!(!serde_json::to_string(&decoded.style_store).unwrap().contains("source_id"));
}

#[test]
fn public_definition_labels_cannot_leak_into_output_references() {
    // 이것을 실패시키는 것: 출력 경계의 종류 하나라도 재번호에서 빼거나 ID로 참조 검사.
    let original = decode("p0-control");
    let mut value = serde_json::to_value(&original.style_store).unwrap();
    for kind in ["fonts", "border_fills", "numberings", "bullets", "styles", "tabs"] {
        for def in value[kind].as_array_mut().unwrap() {
            def["id"] = json!(99);
        }
    }
    let edited = serde_json::from_value(value).unwrap();
    let output =
        HwpxEncoder::encode(&original.document.validate().unwrap(), &edited, &original.image_store)
            .unwrap();
    let d = HwpxDecoder::decode(&output).unwrap();
    assert!(!d.warnings.iter().any(|w| matches!(w, DecodeWarning::InactiveHeaderReference { kind, .. } if kind.starts_with("definition_order:"))));
    assert_eq!(
        d.style_store.iter_border_fills().cloned().collect::<Vec<_>>(),
        original.style_store.iter_border_fills().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        d.style_store.iter_styles().cloned().collect::<Vec<_>>(),
        original.style_store.iter_styles().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        d.style_store.iter_fonts().map(|f| (f.id, &f.lang, &f.face_name)).collect::<Vec<_>>(),
        original
            .style_store
            .iter_fonts()
            .map(|f| (f.id, &f.lang, &f.face_name))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        d.style_store.iter_numberings().cloned().collect::<Vec<_>>(),
        original.style_store.iter_numberings().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        d.style_store.iter_tabs().cloned().collect::<Vec<_>>(),
        original.style_store.iter_tabs().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        d.style_store.iter_bullets().cloned().collect::<Vec<_>>(),
        original.style_store.iter_bullets().cloned().collect::<Vec<_>>()
    );
}

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
fn structural_edits_number_the_preserved_header() {
    // 이것을 실패시키는 것: 본문 byte splice가 원본 희소/재배열 header를 그대로 출력.
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let d = decode("e23-charpr-swap");
    let canonical =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let header = PackageReader::new(&canonical).unwrap().read_header_xml().unwrap();
    let header = header.replace("<hh:charPr id=\"0\"", "<hh:charPr id=\"7\"");
    let base = replace_header(&canonical, &header);
    let locator = ParagraphLocator { section: 0, index: 1 };
    let deleted = HwpxStructuralEditor::delete_paragraphs(&base, &[locator]).unwrap();
    let inserted = HwpxStructuralEditor::insert_paragraphs(
        &base,
        locator,
        InsertPosition::After,
        &["new".into()],
    )
    .unwrap();
    for out in [deleted, inserted] {
        let decoded = HwpxDecoder::decode(&out).unwrap();
        assert_eq!(
            decoded.style_store.char_shape(CharShapeIndex::new(0)).unwrap().height.as_i32(),
            900
        );
        assert!(!decoded.warnings.iter().any(|w| matches!(w, DecodeWarning::InactiveHeaderReference { kind, .. } if kind.starts_with("definition_order:"))));
    }
}

fn replace_header(bytes: &[u8], xml: &str) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let part = "Contents/header.xml";
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.start_file(&name, options).unwrap();
        out.write_all(if name == part { xml.as_bytes() } else { &data }).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn fix8b_head_metrics_survive_public_id_only_json_edits() {
    // 이것을 실패시키는 것: 부가정보 capture/출력 재연결/encoder 조회에 공개 id 사용.
    let original = decode("p0-control");
    let mut value = serde_json::to_value(&original.style_store).unwrap();
    for (defs, metrics) in [("numberings", "numbering"), ("bullets", "bullet")] {
        let level = if defs == "numberings" {
            value[defs][0]["levels"][0]["level"].as_u64().unwrap()
        } else {
            0
        };
        value["header_fidelity"][metrics]["1"][level.to_string()] = json!({
            "align": "RIGHT", "use_inst_width": 0, "auto_indent": 0,
            "width_adjust": 137, "text_offset_type": "HWPUNIT", "text_offset": 321,
            "char_pr_id_ref": 0
        });
    }
    let store = serde_json::from_value(value).unwrap();
    let doc = original.document.validate().unwrap();
    let baseline = HwpxEncoder::encode(&doc, &store, &original.image_store).unwrap();
    // Decode once so the capture path, including explicit charPr 0, is tested too.
    let captured = HwpxDecoder::decode(&baseline).unwrap();
    let before = serde_json::to_value(&captured.style_store).unwrap();
    assert_eq!(before["header_fidelity"]["numbering"]["1"]["1"]["char_pr_id_ref"], 0);
    assert_eq!(before["header_fidelity"]["bullet"]["1"]["0"]["text_offset"], 321);
    for kind in ["numberings", "bullets"] {
        let mut edited = before.clone();
        edited[kind][0]["id"] = json!(99);
        let edited = serde_json::from_value(edited).unwrap();
        let output = HwpxEncoder::encode(&doc, &edited, &original.image_store).unwrap();
        assert_eq!(
            PackageReader::new(&output).unwrap().read_header_xml().unwrap(),
            PackageReader::new(&baseline).unwrap().read_header_xml().unwrap(),
            "{kind}"
        );
        let after = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(
            serde_json::to_value(after.style_store).unwrap()["header_fidelity"],
            before["header_fidelity"]
        );
    }
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
