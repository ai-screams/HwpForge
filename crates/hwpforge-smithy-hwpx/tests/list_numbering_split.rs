use hwpforge_blueprint::{ListMarkerAlign, ListMarkerLayout, ListTextStart};
use hwpforge_core::{
    Document, ImageStore, NumberingDef, PageSettings, ParaHead, Paragraph, ParagraphMeasure, Run,
    Section, Validated,
};
use hwpforge_foundation::{CharShapeIndex, HeadingType, HwpUnit, NumberFormatType, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    style_store::{HwpxCharShape, HwpxParaShape},
    EncodeOptions, EncodeOutcome, EncodeWarning, HwpxEncoder, HwpxStyleStore,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
fn unit(n: i32) -> HwpUnit {
    HwpUnit::new(n).unwrap()
}
fn fixture(
    items: &[(usize, u32, Option<usize>)],
    texts: &[&str],
    starts: &[u32],
) -> (Document<Validated>, HwpxStyleStore) {
    let mut store = HwpxStyleStore::with_default_fonts("맑은 고딕");
    for bold in [false, true] {
        let mut c = HwpxCharShape::default();
        c.height = unit(1100);
        c.bold = bold;
        store.push_char_shape(c);
    }
    store.push_numbering(NumberingDef {
        id: 1,
        start: starts[0],
        levels: texts
            .iter()
            .enumerate()
            .map(|(i, t)| ParaHead {
                level: i as u32 + 1,
                start: starts[i],
                num_format: NumberFormatType::Digit,
                text: (*t).into(),
                checkable: false,
            })
            .collect(),
    });
    let mut layouts = serde_json::Map::new();
    let mut ids = std::collections::BTreeSet::new();
    for &(id, level, character) in items {
        if ids.insert(id) {
            assert_eq!(store.para_shape_count(), id);
            let mut p = HwpxParaShape::default();
            p.heading_type = HeadingType::Number;
            p.heading_id_ref = 1;
            p.heading_level = level;
            p.margin_left = ParagraphMeasure::from_hwp_unit(unit(3600));
            p.indent = ParagraphMeasure::from_hwp_unit(unit(-1800));
            store.push_para_shape(p);
            if let Some(c) = character {
                let mut m = ListMarkerLayout::new(
                    ListMarkerAlign::Left,
                    ListTextStart::FromAnchor(unit(1800)),
                )
                .unwrap();
                m.char_shape = Some(CharShapeIndex::new(c));
                layouts.insert(id.to_string(), serde_json::to_value(m).unwrap());
            }
        }
    }
    let mut value = serde_json::to_value(store).unwrap();
    value["list_marker_layouts"] = layouts.into();
    let store = serde_json::from_value(value).unwrap();
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        items
            .iter()
            .map(|&(id, _, _)| {
                Paragraph::with_runs(
                    vec![Run::text("body", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(id),
                )
            })
            .collect(),
        PageSettings::a4(),
    ));
    (doc.validate().unwrap(), store)
}
fn encode(d: &Document<Validated>, s: &HwpxStyleStore) -> EncodeOutcome {
    HwpxEncoder::encode_with_diagnostics(d, s, &ImageStore::new(), EncodeOptions::default())
        .unwrap()
}
fn header(o: &EncodeOutcome) -> xml::Element {
    xml::Element::parse(&package::part(&o.bytes, "Contents/header.xml"))
}
fn assignments(o: &EncodeOutcome) -> (Vec<u32>, Vec<u32>) {
    let h = header(o);
    let section = xml::Element::parse(&package::part(&o.bytes, "Contents/section0.xml"));
    let ids: Vec<_> = section
        .descendants("hp:p")
        .iter()
        .map(|p| p.attr("paraPrIDRef").unwrap().parse::<u32>().unwrap())
        .collect();
    let definitions = ids
        .iter()
        .map(|id| {
            h.descendants("hh:paraPr")
                .into_iter()
                .find(|p| p.attr("id") == Some(id.to_string().as_str()))
                .unwrap()
                .child("hh:heading")
                .attr("idRef")
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    (definitions, ids)
}
fn starts(o: &EncodeOutcome) -> Vec<u32> {
    header(o)
        .descendants("hh:numbering")
        .iter()
        .map(|d| d.descendants("hh:paraHead")[0].attr("start").unwrap().parse().unwrap())
        .collect()
}
fn diagnostics(o: &EncodeOutcome, split: usize, conflict: usize) {
    assert_eq!(
        o.warnings.iter().filter(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })).count(),
        split
    );
    assert_eq!(
        o.warnings
            .iter()
            .filter(
                |w| matches!(w,EncodeWarning::ContentLost{kind,..} if kind=="list_marker_conflict")
            )
            .count(),
        conflict
    );
}
// 이것을 실패시키는 것: charPrIDRef를 head 비교에서 제외.
#[test]
fn marker_character_changes_split_numbering() {
    let (d, s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1))], &["^1."], &[1]);
    let o = encode(&d, &s);
    assert_eq!(assignments(&o).0, [1, 2]);
    assert_eq!(starts(&o), [1, 2]);
    diagnostics(&o, 1, 0);
}
// 이것을 실패시키는 것: 과거 정의로 복귀하거나 원래 문단 모양만으로 사본을 공유.
#[test]
fn returning_marker_format_opens_another_segment() {
    let (d, s) = fixture(
        &[(0, 0, Some(0)), (0, 0, Some(0)), (1, 0, Some(1)), (0, 0, Some(0))],
        &["^1."],
        &[1],
    );
    let o = encode(&d, &s);
    let (defs, paras) = assignments(&o);
    assert_eq!(defs, [1, 1, 2, 3]);
    assert_eq!(starts(&o), [1, 3, 4]);
    assert_ne!(paras[0], paras[3]);
    diagnostics(&o, 1, 0);
}
// 이것을 실패시키는 것: 요청 문단만 장부에 기록.
#[test]
fn unrequested_items_advance_the_logical_counter() {
    for items in [
        &[(0, 0, None), (1, 0, Some(0)), (2, 0, Some(1))][..],
        &[(0, 0, Some(0)), (1, 0, None), (2, 0, Some(1))][..],
    ] {
        let (d, mut s) = fixture(items, &["^1."], &[1]);
        s.set_paragraph_head_metrics(
            HeadingType::Number,
            1,
            1,
            hwpforge_foundation::Alignment::Left,
            false,
            true,
            700,
            0,
            false,
            0,
        )
        .unwrap();
        let o = encode(&d, &s);
        assert_eq!(assignments(&o).0, [1, 1, 2]);
        assert_eq!(starts(&o), [1, 3]);
        diagnostics(&o, 1, 0);
    }
}
fn draft(d: &Document<Validated>) -> Document<hwpforge_core::Draft> {
    serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap()
}
fn edit_store(s: &HwpxStyleStore, f: impl FnOnce(&mut serde_json::Value)) -> HwpxStyleStore {
    let mut v = serde_json::to_value(s).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
#[path = "support/list_counter_output.rs"]
mod list_counter_output;
fn rendered(o: &EncodeOutcome) -> Vec<String> {
    list_counter_output::rendered(&o.bytes)
}
// 이것을 실패시키는 것: 후보 카운터의 다음 참조 값 비교를 제거.
#[test]
fn split_segments_compare_values_after_parent_items() {
    let (d, s) = fixture(
        &[(0, 0, Some(0)), (1, 1, Some(0)), (2, 1, Some(1)), (0, 0, Some(0)), (2, 1, Some(1))],
        &["^1.", "^1.^2"],
        &[1, 1],
    );
    let o = encode(&d, &s);
    assert_eq!(assignments(&o).0, [1, 1, 2, 2, 3]);
    assert_eq!(rendered(&o), ["1.", "1.1", "1.2", "2.", "2.1"]);
    diagnostics(&o, 1, 0);
}
// 이것을 실패시키는 것: 새 하위 구간의 얕은 start를 다음 값으로 증가.
#[test]
fn split_child_keeps_current_roman_parent() {
    let (d, s) =
        fixture(&[(0, 0, Some(0)), (1, 1, Some(0)), (2, 1, Some(1))], &["^1.", "^1.^2"], &[1, 1]);
    let s =
        edit_store(&s, |v| v["numberings"][0]["levels"][0]["num_format"] = "RomanCapital".into());
    let o = encode(&d, &s);
    assert_eq!(rendered(&o), ["I.", "I.1", "I.2"]);
    let h = header(&o);
    let second = h.descendants("hh:numbering")[1];
    assert_eq!(
        second
            .descendants("hh:paraHead")
            .iter()
            .map(|h| h.attr("start").unwrap())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
}
// 이것을 실패시키는 것: 상위 사용 뒤 하위 used 상태를 남기거나 과거 정의로 복귀.
#[test]
fn split_segments_preserve_parent_resets() {
    let (d, s) = fixture(
        &[
            (0, 0, Some(0)),
            (1, 1, Some(0)),
            (1, 1, Some(0)),
            (2, 0, Some(1)),
            (1, 1, Some(0)),
            (0, 0, Some(0)),
        ],
        &["^1.", "^2."],
        &[1, 1],
    );
    let s =
        edit_store(&s, |v| v["numberings"][0]["levels"][1]["num_format"] = "HangulSyllable".into());
    let o = encode(&d, &s);
    assert_eq!(rendered(&o), ["1.", "가.", "나.", "2.", "가.", "3."]);
    assert_eq!(starts(&o), [1, 2, 3]);
}
// 이것을 실패시키는 것: 생성 순서나 Core 방문 순서로 정의를 배정.
#[test]
fn nested_items_follow_final_paragraph_start_order() {
    for header in [false, true] {
        let (d, s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1)), (0, 0, Some(0))], &["^1."], &[1]);
        let mut d = draft(&d);
        let nested = d.sections_mut()[0].paragraphs.remove(1);
        if header {
            d.sections_mut()[0].headers.push(hwpforge_core::section::HeaderFooter::new(
                vec![nested],
                hwpforge_foundation::ApplyPageType::Both,
            ));
        } else {
            d.sections_mut()[0].paragraphs[0].add_run(Run::control(
                hwpforge_core::Control::footnote(vec![nested]),
                CharShapeIndex::new(0),
            ));
        }
        let o = encode(&d.validate().unwrap(), &s);
        assert_eq!(assignments(&o).0, [1, 2, 3]);
        assert_eq!(starts(&o), [1, 2, 3]);
        assert_eq!(rendered(&o), ["1.", "2.", "3."]);
    }
}
// 이것을 실패시키는 것: 대상 정의의 무요청 미지원 사용처를 누락.
#[test]
fn unrequested_unsupported_uses_prevent_splitting() {
    for footer in [false, true] {
        let (d, s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1)), (2, 0, None)], &["^1."], &[1]);
        let mut d = draft(&d);
        let nested = d.sections_mut()[0].paragraphs.pop().unwrap();
        if footer {
            d.sections_mut()[0].footers.push(hwpforge_core::section::HeaderFooter::new(
                vec![nested],
                hwpforge_foundation::ApplyPageType::Both,
            ));
        } else {
            d.sections_mut()[0].paragraphs[0].add_run(Run::control(
                hwpforge_core::Control::endnote(vec![nested]),
                CharShapeIndex::new(0),
            ));
        }
        let o = encode(&d.validate().unwrap(), &s);
        assert_eq!(header(&o).descendants("hh:numbering").len(), 1);
        diagnostics(&o, 0, 1);
    }
}
// 이것을 실패시키는 것: 건너뜀·깊은 시작 차단을 복원하거나 후보 값 비교를 제거.
#[test]
fn skipped_levels_and_deep_starts_preserve_values() {
    for (levels, chars, definitions) in [
        (&[0, 1, 0, 1][..], &[0, 0, 1, 1][..], 2),
        (&[0, 1, 0, 2, 1][..], &[0, 0, 0, 1, 1][..], 2),
        (&[0, 1, 2, 2][..], &[0, 0, 0, 1][..], 2),
        (&[1, 1, 0, 1][..], &[0, 1, 0, 1][..], 3),
    ] {
        let items = levels
            .iter()
            .zip(chars)
            .enumerate()
            .map(|(i, (&l, &c))| (i, l, Some(c)))
            .collect::<Vec<_>>();
        let (d, s) = fixture(&items, &["^1.", "^1.^2", "^3."], &[1, 1, 1]);
        let o = encode(&d, &s);
        diagnostics(&o, 1, 0);
        assert_eq!(header(&o).descendants("hh:numbering").len(), definitions);
        assert!(matches!(
            o.warnings.iter().find(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })),
            Some(EncodeWarning::ListNumberingSplit { additional_definitions, .. })
                if *additional_definitions == definitions - 1
        ));
    }
}
fn coordinates(o: &EncodeOutcome) -> Vec<(i32, i32)> {
    let h = header(o);
    assignments(o)
        .1
        .iter()
        .map(|id| {
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|p| p.attr("id") == Some(id.to_string().as_str()))
                .unwrap();
            let margin = shape.child("hp:switch").child("hp:case").child("hh:margin");
            (
                margin.child("hc:left").attr("value").unwrap().parse().unwrap(),
                margin.child("hc:intent").attr("value").unwrap().parse().unwrap(),
            )
        })
        .collect()
}
// 이것을 실패시키는 것: 문단 모양 사본의 키에서 최종 여백을 제외.
#[test]
fn inherited_sizes_share_heads_but_not_final_margins() {
    let (d, s) = fixture(&[(0, 0, Some(0)), (0, 0, Some(0))], &["^1."], &[1]);
    let s = edit_store(&s, |v| {
        v["list_marker_layouts"]["0"]["align"] = "right".into();
        v["list_marker_layouts"]["0"]["char_shape"] = serde_json::Value::Null;
        v["list_marker_layouts"]["0"]["text_start"]["value"] = 0.into();
        v["para_shapes"][0]["margin_left"] =
            serde_json::to_value(ParagraphMeasure::from_hwp_unit(unit(9000))).unwrap();
        v["para_shapes"][0]["indent"] =
            serde_json::to_value(ParagraphMeasure::from_hwp_unit(unit(0))).unwrap();
        v["char_shapes"][1]["height"] = 1400.into();
        v["char_shapes"][1]["bold"] = false.into();
    });
    let mut d = draft(&d);
    d.sections_mut()[0].paragraphs[1].runs[0].char_shape_id = CharShapeIndex::new(1);
    let o = encode(&d.validate().unwrap(), &s);
    assert_eq!(coordinates(&o), [(7900, 0), (7600, 0)]);
    assert_eq!(header(&o).descendants("hh:numbering").len(), 1);
    assert_ne!(assignments(&o).1[0], assignments(&o).1[1]);
    diagnostics(&o, 0, 0);
}
// 이것을 실패시키는 것: 넓힌 RIGHT head로 최종 여백을 다시 계산하지 않음.
#[test]
fn right_slots_recompute_margins_after_widening() {
    for (anchor, margin) in [(9000, 6940), (1800, -260)] {
        let (d, s) = fixture(&[(0, 0, Some(0)), (0, 0, Some(0)), (0, 0, Some(0))], &["^1."], &[99]);
        let s = edit_store(&s, |v| {
            v["list_marker_layouts"]["0"]["align"] = "right".into();
            v["para_shapes"][0]["margin_left"] =
                serde_json::to_value(ParagraphMeasure::from_hwp_unit(unit(anchor + 1800))).unwrap();
        });
        let o = encode(&d, &s);
        assert_eq!(coordinates(&o), [(margin, 0); 3]);
        let h = header(&o);
        assert_eq!(h.descendants("hh:paraHead")[0].attr("widthAdjust"), Some("960"));
        assert_eq!(h.descendants("hh:numbering").len(), 1);
        assert_eq!(rendered(&o), ["99.", "100.", "101."]);
        diagnostics(&o, 0, 0);
    }
}
// 이것을 실패시키는 것: start 0을 저장값 1로 정규화하거나 분리.
#[test]
fn zero_starts_keep_the_original_definition() {
    let (d, s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1))], &["^1."], &[0]);
    let o = encode(&d, &s);
    assert_eq!(starts(&o), [0]);
    assert_eq!(rendered(&o), ["1.", "2."]);
    diagnostics(&o, 0, 1);
}
// 이것을 실패시키는 것: 최종 XML보다 Core 순서에서 첫 head를 고정.
#[test]
fn first_head_selection_uses_emitted_order() {
    let (d, mut s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1))], &["^1."], &[1]);
    let mut plain = s.para_shape(ParaShapeIndex::new(0)).unwrap().clone();
    plain.heading_type = HeadingType::None;
    let id = s.push_para_shape(plain);
    let mut d = draft(&d);
    let nested_header = d.sections_mut()[0].paragraphs.pop().unwrap();
    d.sections_mut()[0]
        .paragraphs
        .insert(0, Paragraph::with_runs(vec![Run::text("anchor", CharShapeIndex::new(0))], id));
    d.sections_mut()[0].headers.push(hwpforge_core::section::HeaderFooter::new(
        vec![nested_header],
        hwpforge_foundation::ApplyPageType::Both,
    ));
    let o = encode(&d.validate().unwrap(), &s);
    assert_eq!(rendered(&o), ["1.", "2."]);
    let h = header(&o);
    let defs = h.descendants("hh:numbering");
    assert_eq!(defs[0].child("hh:paraHead").attr("charPrIDRef"), Some("1"));
    assert_eq!(defs[1].child("hh:paraHead").attr("charPrIDRef"), Some("0"));
    assert_eq!(starts(&o), [1, 2]);
}
// 이것을 실패시키는 것: 글머리표 배정을 생략하거나 동일한 head도 매번 복제.
#[test]
fn bullet_formats_split_without_changing_glyphs() {
    for (items, expected) in [
        (
            &[(0, 0, Some(0)), (0, 0, Some(0)), (1, 0, Some(1)), (0, 0, Some(0))][..],
            &[1, 1, 2, 3][..],
        ),
        (&[(0, 0, Some(0)), (0, 0, Some(0))][..], &[1, 1][..]),
    ] {
        let (d, s) = fixture(items, &["^1."], &[1]);
        let s = edit_store(&s, |v| {
            let mut head = v["numberings"][0]["levels"][0].clone();
            head["text"] = "".into();
            v["bullets"] = serde_json::json!([{"id":1,"bullet_char":"•","checked_char":null,"use_image":false,"para_head":head}]);
            for p in v["para_shapes"].as_array_mut().unwrap() {
                p["heading_type"] = "Bullet".into();
            }
        });
        let o = encode(&d, &s);
        assert_eq!(assignments(&o).0, expected);
        let h = header(&o);
        assert!(h.descendants("hh:bullet").iter().all(|b| b.attr("char") == Some("•")));
        assert_eq!(h.descendants("hh:bullet").len(), *expected.last().unwrap() as usize);
        diagnostics(&o, usize::from(expected.len() == 4), 0);
    }
}
// 이것을 실패시키는 것: 정의 복제에서 미사용 수준 fidelity나 문단 자동 간격의 세 상태를 합침.
#[test]
fn cloned_shapes_and_heads_preserve_fidelity() {
    for spacing in [None, Some(serde_json::Value::Null), Some(serde_json::json!([0, 0]))] {
        let (d, s) =
            fixture(&[(0, 0, Some(0)), (1, 0, Some(1)), (0, 0, Some(0))], &["^1.", "^2."], &[1, 1]);
        let s = edit_store(&s, |v| {
            if let Some(value) = spacing.clone() {
                v["header_fidelity"]["auto_spacing"]["0"] = value;
            }
            v["header_fidelity"]["numbering"]["1"]["2"] = serde_json::json!({"align":"RIGHT","use_inst_width":1,"auto_indent":0,"width_adjust":17,"text_offset_type":"HWPUNIT","text_offset":123,"char_pr_id_ref":0});
            v["para_shapes"][0]["keep_with_next"] = true.into();
        });
        let mut d = draft(&d);
        d.set_layout_profile(hwpforge_core::LayoutProfile::WordCompatible);
        let o = encode(&d.validate().unwrap(), &s);
        let h = header(&o);
        let paras = assignments(&o).1;
        let shapes = h.descendants("hh:paraPr");
        let a =
            shapes.iter().find(|p| p.attr("id") == Some(paras[0].to_string().as_str())).unwrap();
        let b =
            shapes.iter().find(|p| p.attr("id") == Some(paras[2].to_string().as_str())).unwrap();
        assert_eq!(
            a.children.iter().find(|c| c.name == "hh:autoSpacing"),
            b.children.iter().find(|c| c.name == "hh:autoSpacing")
        );
        let da = h.descendants("hh:numbering")[0].descendants("hh:paraHead")[1];
        let db = h.descendants("hh:numbering")[2].descendants("hh:paraHead")[1];
        assert_eq!(da, db);
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&o.bytes).unwrap();
        let mut copy =
            decoded.style_store.para_shape(ParaShapeIndex::new(paras[2] as usize)).unwrap().clone();
        let original =
            decoded.style_store.para_shape(ParaShapeIndex::new(paras[0] as usize)).unwrap();
        copy.heading_id_ref = original.heading_id_ref;
        assert_eq!(&copy, original);
    }
}

fn holder(s: &mut HwpxStyleStore) -> Paragraph {
    let id = s.push_para_shape(HwpxParaShape::default());
    Paragraph::with_runs(vec![Run::text("anchor", CharShapeIndex::new(0))], id)
}
fn table(paragraphs: Vec<Paragraph>) -> hwpforge_core::Table {
    hwpforge_core::Table::new(vec![hwpforge_core::TableRow::new(vec![
        hwpforge_core::TableCell::new(paragraphs, unit(12000)),
    ])])
}
// 이것을 실패시키는 것: 미지원 조상·그림 캡션·바탕쪽을 지원 자리로 취급하거나 직접 칸만 검사.
#[test]
fn unsupported_ancestors_and_shared_table_captions_remain_approximate() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        control::GroupChild,
        Control,
    };
    for place in [
        "footer cell",
        "group footnote",
        "picture caption",
        "memo",
        "master",
        "floating",
        "caption cell",
        "caption nested cell",
        "independent caption",
        "inline",
    ] {
        let (d, mut s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1)), (2, 0, None)], &["^1."], &[1]);
        let mut d = draft(&d);
        let nested = d.sections_mut()[0].paragraphs.pop().unwrap();
        let mut anchor = holder(&mut s);
        let run = match place {
            "footer cell" => {
                anchor.add_run(Run::table(table(vec![nested]), CharShapeIndex::new(0)));
                d.sections_mut()[0].footers.push(hwpforge_core::section::HeaderFooter::new(
                    vec![anchor],
                    hwpforge_foundation::ApplyPageType::Both,
                ));
                None
            }
            "master" => {
                d.sections_mut()[0].master_pages.get_or_insert_with(Vec::new).push(
                    hwpforge_core::section::MasterPage::new(
                        hwpforge_foundation::ApplyPageType::Both,
                        vec![nested],
                    ),
                );
                None
            }
            "picture caption" => {
                let mut image = hwpforge_core::Image::new(
                    "",
                    unit(1000),
                    unit(1000),
                    hwpforge_core::ImageFormat::Png,
                );
                image.caption = Some(Caption::new(vec![nested], CaptionSide::Bottom));
                Some(Run::image(image, CharShapeIndex::new(0)))
            }
            "memo" => Some(Run::control(Control::memo(vec![nested]), CharShapeIndex::new(0))),
            "group footnote" => {
                let mut inside = holder(&mut s);
                inside
                    .add_run(Run::control(Control::footnote(vec![nested]), CharShapeIndex::new(0)));
                Some(Run::control(
                    Control::Group {
                        children: vec![GroupChild::Shape(Control::text_box(
                            vec![inside],
                            unit(12000),
                            unit(4000),
                        ))],
                        width: unit(12000),
                        height: unit(4000),
                        frame: hwpforge_core::ObjectFrame::new(),
                        placement: None,
                        inst_id: None,
                    },
                    CharShapeIndex::new(0),
                ))
            }
            "floating" | "inline" => {
                let mut c = Control::text_box(vec![nested], unit(12000), unit(4000));
                if place == "floating" {
                    let mut p = hwpforge_core::ObjectPlacement::legacy_inline_defaults();
                    p.treat_as_char = false;
                    if let Control::TextBox { placement, .. } = &mut c {
                        *placement = Some(p);
                    }
                }
                Some(Run::control(c, CharShapeIndex::new(0)))
            }
            _ => {
                let caption = nested.clone();
                if place == "independent caption" {
                    let mut def = s.iter_numberings().next().unwrap().clone();
                    def.id = 2;
                    s.push_numbering(def);
                    s.para_shape_mut(nested.para_shape_id).unwrap().heading_id_ref = 2;
                }
                let mut t = if place == "caption nested cell" {
                    let mut inner = holder(&mut s);
                    inner.add_run(Run::table(table(vec![nested]), CharShapeIndex::new(0)));
                    table(vec![inner])
                } else {
                    table(vec![nested])
                };
                let mut caption = caption;
                if place == "independent caption" {
                    caption.para_shape_id = ParaShapeIndex::new(0);
                }
                t.caption = Some(Caption::new(vec![caption], CaptionSide::Bottom));
                Some(Run::table(t, CharShapeIndex::new(0)))
            }
        };
        if let Some(run) = run {
            let mut anchor = holder(&mut s);
            anchor.add_run(run);
            d.sections_mut()[0].paragraphs.push(anchor);
        }
        let o = encode(&d.validate().unwrap(), &s);
        let supported = matches!(place, "independent caption" | "inline");
        diagnostics(&o, usize::from(supported), usize::from(!supported));
        if !supported {
            assert_eq!(header(&o).descendants("hh:numbering").len(), 1, "{place}");
        }
        for name in ["Contents/section0.xml", "Contents/header.xml"] {
            let tree = xml::Element::parse(&package::part(&o.bytes, name));
            assert!(
                tree.descendants("hp:p").iter().all(|p| p.attr("hwpforgeList").is_none()),
                "{place}"
            );
        }
    }
}

// 이것을 실패시키는 것: 리터럴 정의를 숫자 토큰으로 바꾸거나 정의 사이에 카운터를 공유.
#[test]
fn literal_heads_do_not_interrupt_other_definitions() {
    let (d, mut s) = fixture(
        &[(0, 0, Some(0)), (1, 0, Some(0)), (1, 0, Some(0)), (0, 0, Some(0))],
        &["^1."],
        &[1],
    );
    let mut def = s.iter_numberings().next().unwrap().clone();
    def.id = 2;
    def.levels[0].text = "0.".into();
    s.push_numbering(def);
    s.para_shape_mut(ParaShapeIndex::new(1)).unwrap().heading_id_ref = 2;
    assert_eq!(rendered(&encode(&d, &s)), ["1.", "0.", "0.", "2."]);
}

// 이것을 실패시키는 것: 장부만 다른 글꼴 대체 순서나 RIGHT 이동 산식을 사용.
#[test]
fn new_and_base_encoding_share_marker_requirements() {
    for (different, inherited) in [(false, false), (true, false), (false, true)] {
        let (d, s) =
            fixture(&[(0, 0, Some(0)), (1, 0, Some(usize::from(different)))], &["^1."], &[1]);
        let mut no_request = edit_store(&s, |v| {
            v["list_marker_layouts"] = serde_json::json!({});
        });
        // The base is built without deferred requests, so both entry points receive the same definitions.
        let base_bytes = encode(&d, &no_request).bytes;
        let base = hwpforge_smithy_hwpx::decoder::HwpxEditBase::read(&base_bytes).unwrap();
        let mut doc = draft(&d);
        if inherited {
            no_request = edit_store(&s, |v| {
                for id in ["0", "1"] {
                    v["list_marker_layouts"][id]["align"] = "right".into();
                    v["list_marker_layouts"][id]["char_shape"] = serde_json::Value::Null;
                    v["list_marker_layouts"][id]["text_start"]["value"] = 0.into();
                }
                for p in v["para_shapes"].as_array_mut().unwrap() {
                    p["margin_left"] =
                        serde_json::to_value(ParagraphMeasure::from_hwp_unit(unit(9000))).unwrap();
                    p["indent"] =
                        serde_json::to_value(ParagraphMeasure::from_hwp_unit(unit(0))).unwrap();
                }
                v["char_shapes"][1]["height"] = 1400.into();
                v["char_shapes"][1]["bold"] = false.into();
            });
            for (i, p) in doc.sections_mut()[0].paragraphs.iter_mut().enumerate() {
                p.runs[0].char_shape_id = CharShapeIndex::new(i);
                p.end_char_shape = Some(CharShapeIndex::new(1 - i));
            }
        } else {
            no_request = s;
        }
        let d = doc.validate().unwrap();
        let new = encode(&d, &no_request);
        let old = HwpxEncoder::encode_with_edit_base(
            &base,
            &d,
            &no_request,
            &ImageStore::new(),
            EncodeOptions::default(),
        )
        .unwrap();
        diagnostics(&new, usize::from(different), 0);
        diagnostics(&old, 0, usize::from(different || inherited));
        if inherited {
            assert_eq!(coordinates(&new), [(7900, 0), (7600, 0)]);
        }
        if !inherited {
            assert_eq!(starts(&new).len() > 1, old.warnings.iter().any(|w| matches!(w, EncodeWarning::ContentLost { kind, .. } if kind == "list_marker_conflict")));
        }
        assert!(!new.warnings.iter().any(EncodeWarning::is_semantic_loss));
        assert_eq!(
            old.warnings.iter().any(EncodeWarning::is_semantic_loss),
            different || inherited
        );
    }
    // 이것을 실패시키는 것: 미지원 사용처가 있는 새 인코드도 정의를 분리.
    let (d, s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1))], &["^1."], &[1]);
    let mut d = draft(&d);
    let nested = d.sections_mut()[0].paragraphs.pop().unwrap();
    d.sections_mut()[0].paragraphs[0].add_run(Run::control(
        hwpforge_core::Control::endnote(vec![nested]),
        CharShapeIndex::new(0),
    ));
    let d = d.validate().unwrap();
    let plain = edit_store(&s, |v| v["list_marker_layouts"] = serde_json::json!({}));
    let base_bytes = encode(&d, &plain).bytes;
    let base = hwpforge_smithy_hwpx::decoder::HwpxEditBase::read(&base_bytes).unwrap();
    for out in [
        encode(&d, &s),
        HwpxEncoder::encode_with_edit_base(
            &base,
            &d,
            &s,
            &ImageStore::new(),
            EncodeOptions::default(),
        )
        .unwrap(),
    ] {
        diagnostics(&out, 0, 1);
        assert!(out.warnings.iter().any(EncodeWarning::is_semantic_loss));
    }
}

#[path = "support/list_counter_cases.rs"]
mod list_counter_cases;
// 이것을 실패시키는 것: 사본 start를 1로 고정하거나 XML의 정의 참조를 원래 정의로 되돌림.
#[test]
fn emitted_counter_values_match_literal_observations() {
    for case in list_counter_cases::CASES {
        let Some(expected) = case.expected else {
            continue;
        };
        let items: Vec<_> = case
            .uses
            .iter()
            .enumerate()
            .map(|(i, &(_, level))| (i, level as u32, Some(i % 2)))
            .collect();
        let first = &case.definitions[0];
        let (d, mut s) = fixture(&items, first.texts, first.starts);
        s = edit_store(&s, |v| {
            v["numberings"] = case.definitions.iter().enumerate().map(|(i, def)| {
                serde_json::json!({"id": i+1, "start": def.starts[0], "levels": def.texts.iter().enumerate().map(|(j, text)| serde_json::json!({"level": j+1,"start":def.starts[j],"text":text,"num_format":def.formats[j],"checkable":false})).collect::<Vec<_>>()})
            }).collect::<Vec<_>>().into();
            let mut ordinary = 0;
            for (i, &(definition, _)) in case.uses.iter().enumerate() {
                v["para_shapes"][i]["heading_id_ref"] = definition.into();
                if definition == 0 {
                    let kind = ["None", "Bullet", "Outline"][ordinary % 3];
                    ordinary += 1;
                    v["para_shapes"][i]["heading_type"] = kind.into();
                    v["para_shapes"][i]["heading_id_ref"] = 1.into();
                    if kind == "Bullet" {
                        v["bullets"] = serde_json::json!([{"id":1,"bullet_char":"•","checked_char":null,"use_image":false,"para_head":{"level":0,"start":1,"num_format":"Digit","text":"","checkable":false}}]);
                    }
                    v["list_marker_layouts"].as_object_mut().unwrap().remove(&i.to_string());
                }
            }
        });
        let o = encode(&d, &s);
        assert_eq!(rendered(&o), expected, "{}", case.name);
    }
}

// 이것을 실패시키는 것: 최종 XML 순서 대신 생성 순서를 쓰거나 표·구역에서 목록 진행을 끊음.
#[test]
fn supported_stories_keep_literal_number_sequences() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        Control, TableCell, TableRow,
    };
    for place in ["table", "inline", "caption", "sections"] {
        let count = if place == "table" {
            6
        } else if place == "sections" {
            4
        } else {
            3
        };
        let items: Vec<_> = (0..count).map(|i| (i, 0, Some(i % 2))).collect();
        let (d, mut s) = fixture(&items, &["^1."], &[1]);
        let mut d = draft(&d);
        let mut ps = std::mem::take(&mut d.sections_mut()[0].paragraphs);
        let last = ps.pop().unwrap();
        let first = ps.remove(0);
        let mut anchor = holder(&mut s);
        match place {
            "sections" => {
                let rest = ps.split_off(1);
                d.sections_mut()[0].paragraphs = vec![first, ps.remove(0)];
                d.add_section(Section::with_paragraphs(
                    vec![rest[0].clone(), last],
                    PageSettings::a4(),
                ));
            }
            "table" => {
                let rows = ps
                    .chunks(2)
                    .map(|pair| {
                        TableRow::new(
                            pair.iter()
                                .map(|p| TableCell::new(vec![p.clone()], unit(12000)))
                                .collect(),
                        )
                    })
                    .collect();
                anchor.add_run(Run::table(hwpforge_core::Table::new(rows), CharShapeIndex::new(0)));
                d.sections_mut()[0].paragraphs = vec![first, anchor, last];
            }
            "inline" => {
                anchor.add_run(Run::control(
                    Control::text_box(ps, unit(12000), unit(4000)),
                    CharShapeIndex::new(0),
                ));
                d.sections_mut()[0].paragraphs = vec![first, anchor, last];
            }
            _ => {
                let mut t = table(vec![holder(&mut s)]);
                t.caption = Some(Caption::new(ps, CaptionSide::Bottom));
                anchor.add_run(Run::table(t, CharShapeIndex::new(0)));
                d.sections_mut()[0].paragraphs = vec![first, anchor, last];
            }
        }
        let o = encode(&d.validate().unwrap(), &s);
        let name = match place {
            "table" => "body and table cells",
            "sections" => "section boundary",
            _ => "body and nested paragraphs",
        };
        let expected =
            list_counter_cases::CASES.iter().find(|c| c.name == name).unwrap().expected.unwrap();
        assert_eq!(rendered(&o), expected, "{place}");
        diagnostics(&o, 1, 0);
    }
}

// 이것을 실패시키는 것: 동일한 미지원 사용처에 새 충돌을 추가하거나 기존 투영 바이트를 변경.
#[test]
fn identical_unsupported_presentations_keep_baseline_parts_and_warnings() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        Control,
    };
    for place in ["footer", "endnote", "floating", "picture"] {
        let (d, mut s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(0))], &["^1."], &[1]);
        for id in 0..2 {
            s.para_shape_mut(ParaShapeIndex::new(id)).unwrap().indent =
                ParagraphMeasure::from_hwp_unit(unit(0));
        }
        s.set_paragraph_head_metrics(
            HeadingType::Number,
            1,
            1,
            hwpforge_foundation::Alignment::Left,
            false,
            false,
            700,
            0,
            false,
            0,
        )
        .unwrap();
        let mut d = draft(&d);
        let nested = std::mem::take(&mut d.sections_mut()[0].paragraphs);
        let mut anchor = holder(&mut s);
        match place {
            "footer" => {
                d.sections_mut()[0].footers.push(hwpforge_core::section::HeaderFooter::new(
                    nested,
                    hwpforge_foundation::ApplyPageType::Both,
                ))
            }
            "endnote" => {
                anchor.add_run(Run::control(Control::endnote(nested), CharShapeIndex::new(0)))
            }
            "floating" => {
                let mut c = Control::text_box(nested, unit(12000), unit(4000));
                let mut placement = hwpforge_core::ObjectPlacement::legacy_inline_defaults();
                placement.treat_as_char = false;
                if let Control::TextBox { placement: target, .. } = &mut c {
                    *target = Some(placement);
                }
                anchor.add_run(Run::control(c, CharShapeIndex::new(0)));
            }
            _ => {
                let mut image = hwpforge_core::Image::new(
                    "",
                    unit(1000),
                    unit(1000),
                    hwpforge_core::ImageFormat::Png,
                );
                image.caption = Some(Caption::new(nested, CaptionSide::Bottom));
                anchor.add_run(Run::image(image, CharShapeIndex::new(0)));
            }
        }
        d.sections_mut()[0].paragraphs.push(anchor);
        let d = d.validate().unwrap();
        let mut plain = s.clone();
        plain = edit_store(&plain, |v| v["list_marker_layouts"] = serde_json::json!({}));
        let base =
            hwpforge_smithy_hwpx::decoder::HwpxEditBase::read(&encode(&d, &plain).bytes).unwrap();
        let baseline = HwpxEncoder::encode_with_edit_base(
            &base,
            &d,
            &s,
            &ImageStore::new(),
            EncodeOptions::default(),
        )
        .unwrap();
        let actual = encode(&d, &s);
        diagnostics(&actual, 0, 0);
        assert_eq!(actual.warnings, baseline.warnings, "{place}");
        for part in ["Contents/header.xml", "Contents/section0.xml"] {
            assert_eq!(
                package::part(&actual.bytes, part),
                package::part(&baseline.bytes, part),
                "{place}/{part}"
            );
        }
    }
}
// 이것을 실패시키는 것: RIGHT 넓히기를 무요청 99와 공유해 번호 끝 좌표를 이동.
#[test]
fn right_widening_preserves_unrequested_marker_and_body_coordinates() {
    let (d, mut s) = fixture(&[(0, 0, None), (1, 0, Some(0))], &["^1."], &[99]);
    s.set_paragraph_head_metrics(
        HeadingType::Number,
        1,
        1,
        hwpforge_foundation::Alignment::Right,
        false,
        true,
        700,
        1800,
        true,
        0,
    )
    .unwrap();
    s = edit_store(&s, |v| v["list_marker_layouts"]["1"]["align"] = "right".into());
    let original_margin = s.para_shape(ParaShapeIndex::new(0)).unwrap().margin_left;
    let actual = encode(&d, &s);
    assert_eq!(rendered(&actual), ["99.", "100."]);
    assert_eq!(assignments(&actual).0, [1, 2]);
    let h = header(&actual);
    let heads = h.descendants("hh:paraHead");
    assert_eq!(heads[0].attr("widthAdjust"), Some("700"));
    assert_eq!(heads[1].attr("widthAdjust"), Some("960"));
    assert_eq!(coordinates(&actual)[0], (original_margin.as_hwp_unit().unwrap().as_i32(), -1800));
    // Original number end = M + W; body = M - I (unchanged head and measures).
    let (margin, indent) = coordinates(&actual)[0];
    assert_eq!((margin + 1100 + 700, margin - indent), (5400, 5400));
    diagnostics(&actual, 1, 0);
}
// 이것을 실패시키는 것: 새 분리 경고를 Outline 또는 None 종류로 방출.
#[test]
fn split_warning_kinds_are_number_or_bullet() {
    let (d, mut s) = fixture(&[(0, 0, Some(0)), (1, 0, Some(1))], &["^1."], &[1]);
    for bullet in [false, true] {
        if bullet {
            s = edit_store(&s, |v| {
                v["bullets"] = serde_json::json!([{"id":1,"bullet_char":"•","checked_char":null,"use_image":false,"para_head":{"level":1,"start":1,"num_format":"Digit","text":"","checkable":false}}]);
                for p in v["para_shapes"].as_array_mut().unwrap() {
                    p["heading_type"] = "Bullet".into();
                }
            });
        }
        let actual = encode(&d, &s);
        let kinds: Vec<_> = actual
            .warnings
            .iter()
            .filter_map(|w| match w {
                EncodeWarning::ListNumberingSplit { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, [if bullet { HeadingType::Bullet } else { HeadingType::Number }]);
    }
}

// 이것을 실패시키는 것: 다른 수준의 무요청 head까지 RIGHT 확장 경계로 취급.
#[test]
fn right_widening_only_separates_unrequested_uses_of_the_same_head() {
    let (d, mut s) = fixture(&[(0, 1, None), (1, 0, Some(0))], &["^1.", "^2."], &[100, 1]);
    s.set_paragraph_head_metrics(
        HeadingType::Number,
        1,
        1,
        hwpforge_foundation::Alignment::Right,
        false,
        true,
        700,
        1800,
        true,
        0,
    )
    .unwrap();
    s = edit_store(&s, |v| v["list_marker_layouts"]["1"]["align"] = "right".into());
    let actual = encode(&d, &s);
    assert_eq!(assignments(&actual).0, [1, 1]);
    assert_eq!(coordinates(&actual)[0], (3600, -1800));
    assert_eq!(header(&actual).descendants("hh:paraHead")[0].attr("widthAdjust"), Some("960"));
    diagnostics(&actual, 0, 0);
}

// 이것을 실패시키는 것: 미사용 start 0 또는 미방출 요청에 의미 손실 경고·새 바이트를 추가.
#[test]
fn unused_approximation_reasons_keep_baseline_parts_and_warnings() {
    for emitted_request in [false, true] {
        let (d, mut s) = if emitted_request {
            fixture(&[(0, 0, Some(0)), (1, 0, Some(0))], &["^1.", "^2."], &[1, 0])
        } else {
            fixture(&[(0, 0, None), (1, 0, Some(0))], &["^1."], &[1])
        };
        let mut d = draft(&d);
        if !emitted_request {
            d.sections_mut()[0].paragraphs.pop();
            let nested = d.sections_mut()[0].paragraphs.remove(0);
            d.sections_mut()[0].footers.push(hwpforge_core::section::HeaderFooter::new(
                vec![nested],
                hwpforge_foundation::ApplyPageType::Both,
            ));
            d.sections_mut()[0].paragraphs.push(holder(&mut s));
        }
        let d = d.validate().unwrap();
        let plain = edit_store(&s, |v| v["list_marker_layouts"] = serde_json::json!({}));
        let base =
            hwpforge_smithy_hwpx::decoder::HwpxEditBase::read(&encode(&d, &plain).bytes).unwrap();
        let baseline = HwpxEncoder::encode_with_edit_base(
            &base,
            &d,
            &s,
            &ImageStore::new(),
            EncodeOptions::default(),
        )
        .unwrap();
        let actual = encode(&d, &s);
        diagnostics(&actual, 0, 0);
        assert_eq!(actual.warnings, baseline.warnings);
        for part in ["Contents/header.xml", "Contents/section0.xml"] {
            assert_eq!(package::part(&actual.bytes, part), package::part(&baseline.bytes, part));
        }
    }
}

// 이것을 실패시키는 것: 깊은 사용의 상위를 다음 값으로 씨앗 설정하거나 중간 used를 누락.
#[test]
fn deep_counter_outputs_match_literal_observations() {
    for name in [
        "skipped_middle_level_is_consumed",
        "compound_deep_level_uses_implicit_ancestors",
        "deep_first_item_consumes_all_ancestors",
    ] {
        let case = list_counter_cases::CASES.iter().find(|c| c.name == name).unwrap();
        let def = &case.definitions[0];
        let items: Vec<_> = case
            .uses
            .iter()
            .enumerate()
            .map(|(i, &(_, level))| (i, level as u32, Some(i % 2)))
            .collect();
        let (d, s) = fixture(&items, def.texts, def.starts);
        let s = edit_store(&s, |v| {
            for (i, format) in def.formats.iter().enumerate() {
                v["numberings"][0]["levels"][i]["num_format"] = (*format).into();
            }
        });
        let out = encode(&d, &s);
        assert_eq!(rendered(&out), case.expected.unwrap(), "{name}");
        assert_eq!(
            out.warnings
                .iter()
                .filter(|w| matches!(w,
                    EncodeWarning::ContentLost { kind, .. } if kind == "list_marker_conflict"
                ))
                .count(),
            0
        );
    }
}

// 이것을 실패시키는 것: 깊은 사본의 상위 현재 값 또는 현재 수준 씨앗을 1로 초기화.
// 이 단언은 정의의 구조 계약이며 한컴 표시 문자열 실측을 대신하지 않는다.
#[test]
fn deep_style_splits_keep_nonunit_counter_seeds() {
    let (doc, store) = fixture(
        &[(0, 0, Some(0)), (1, 1, Some(0)), (2, 2, Some(0)), (3, 2, Some(1))],
        &["^1.", "^2.", "^1.^2.^3"],
        &[2, 3, 4],
    );
    let out = encode(&doc, &store);
    let h = header(&out);
    let definitions = h.descendants("hh:numbering");
    assert_eq!(definitions.len(), 2);
    assert_eq!(
        definitions[1]
            .descendants("hh:paraHead")
            .iter()
            .map(|head| head.attr("start").unwrap())
            .collect::<Vec<_>>(),
        ["2", "3", "5"]
    );
    assert_eq!(assignments(&out).0, [1, 1, 1, 2]);
    diagnostics(&out, 1, 0);
}
