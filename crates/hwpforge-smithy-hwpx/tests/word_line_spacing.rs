use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
};
use hwpforge_core::{
    Document, Draft, ImageStore, LayoutProfile, PageSettings, Paragraph, Run, Section, Validated,
};
use hwpforge_foundation::{
    CharShapeIndex, HwpUnit, LineSpacingType, ParaShapeIndex, VerticalPosition,
};
use hwpforge_smithy_hwpx::{
    EncodeOptions, EncodeOutcome, HwpxEncoder, HwpxRegistryBridge, HwpxStyleStore,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
fn fixture(family: &str, size: i32, percent: f64, text: &str) -> (Document<Draft>, HwpxStyleStore) {
    let mut registry: StyleRegistry = serde_json::from_value(serde_json::json!({"fonts":["맑은 고딕",family],"char_shapes":[],"para_shapes":[],"style_entries":{}})).unwrap();
    let mut c = PartialCharShape::default();
    c.font = Some("맑은 고딕".into());
    c.font_latin = Some(family.into());
    c.size = Some(HwpUnit::new(size).unwrap());
    registry.char_shapes.push(c.resolve("body").unwrap());
    let mut p = ParaShape::default();
    p.line_spacing_value = percent;
    registry.para_shapes.push(p);
    let mut doc = Document::new();
    doc.set_layout_profile(LayoutProfile::WordCompatible);
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bridge =
        HwpxRegistryBridge::from_registry_for_import(&registry, Default::default()).unwrap().value;
    (bridge.rebind_draft_document(doc).unwrap(), bridge.into_style_store())
}
fn encode(doc: Document<Draft>, store: &HwpxStyleStore) -> EncodeOutcome {
    encode_valid(&doc.validate().unwrap(), store, EncodeOptions::default())
}
fn encode_valid(
    doc: &Document<Validated>,
    store: &HwpxStyleStore,
    options: EncodeOptions,
) -> EncodeOutcome {
    HwpxEncoder::encode_with_diagnostics(doc, store, &ImageStore::new(), options).unwrap()
}
fn values(bytes: &[u8]) -> Vec<i32> {
    let h = xml::Element::parse(&package::part(bytes, "Contents/header.xml"));
    let s = xml::Element::parse(&package::part(bytes, "Contents/section0.xml"));
    s.descendants("hp:p")
        .iter()
        .map(|p| {
            let id = p.attr("paraPrIDRef").unwrap();
            let def =
                h.descendants("hh:paraPr").into_iter().find(|d| d.attr("id") == Some(id)).unwrap();
            let lines = def.descendants("hh:lineSpacing");
            assert_eq!(lines.len(), 2);
            for line in &lines {
                assert_eq!(line.attr("type"), Some("PERCENT"));
            }
            assert_eq!(lines[0].attr("value"), lines[1].attr("value"));
            lines[0].attr("value").unwrap().parse().unwrap()
        })
        .collect()
}
// 이것을 실패시키는 것: 원래 배수 선반올림, family/크기/face 화이트리스트, ASCII/LF 경계 축소.
#[test]
fn word_line_spacing_ascii_sizes_faces_and_fractional_percentages() {
    for (family, percent, expected) in [
        ("Cambria", 100., 87),
        ("Cambria", 115., 100),
        ("Cambria", 150., 131),
        ("Calibri", 100., 85),
        ("Calibri", 115., 97),
        ("Calibri", 150., 127),
        ("Cambria", 241. / 240. * 100., 88),
        ("Cambria", 108., 94),
        ("Calibri", 108., 91),
    ] {
        for size in [900, 1000, 1050, 1100, 1200, 1400, 2000] {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let (mut d, mut s) =
                    fixture(family, size, percent, "A A~Z 'quote' \"double\"\n2009-2013\nlast");
                let p = &mut d.sections_mut()[0].paragraphs[0];
                let id = p.runs[0].char_shape_id;
                let c = s.char_shape_mut(id).unwrap();
                c.bold = bold;
                c.italic = italic;
                p.end_char_shape = Some(id);
                assert_eq!(
                    values(&encode(d, &s).bytes),
                    [expected],
                    "{family}/{percent}/{size}/{bold}/{italic}"
                );
            }
        }
    }
}
// 이것을 실패시키는 것: 허용 문자 상하한 제거, 비ASCII 예외 추가, 빈 줄 또는 제어문자 허용.
#[test]
fn word_line_spacing_rejects_non_ascii_and_empty_lines() {
    for family in ["Cambria", "Calibri"] {
        for text in [
            "2009–2013",
            "don’t",
            "‘single’",
            "“double”",
            "a\u{201a}b",
            "a\u{201b}b",
            "a\tB",
            "a\rB",
            "a\u{a0}B",
            "a\u{7f}B",
            "한글",
            "é",
            "a\u{301}",
            "Ａ",
            "漢",
            "😀",
            "",
            " ",
            "\na",
            "a\n",
            "a\n\nb",
            "a\n  \nb",
        ] {
            let (d, s) = fixture(family, 1100, 100., text);
            assert_eq!(values(&encode(d, &s).bytes), [100], "{family}: {text:?}");
        }
    }
}
// 이것을 실패시키는 것: 크기/family/높이 효과/끝 face 검사 제거, run별 개행 판정 또는 face 혼합 제외.
#[test]
fn word_line_spacing_run_and_end_mark_boundaries() {
    for case in 0..12 {
        let (mut d, mut s) = fixture("Cambria", 1100, 100., "A");
        let p = &mut d.sections_mut()[0].paragraphs[0];
        let id = p.runs[0].char_shape_id;
        let mut c = s.char_shape(id).unwrap().clone();
        match case {
            0 => c.height = HwpUnit::new(1200).unwrap(),
            1 => c.vertical_position = VerticalPosition::Superscript,
            2 => c.vertical_position = VerticalPosition::Subscript,
            3 => c.rel_sz = 99,
            4 => c.char_offset = 1,
            5 => c.bold = true,
            6 => c.italic = true,
            _ => {}
        }
        let other = s.push_char_shape(c);
        if case < 5 {
            p.runs.push(Run::text("B", other));
        } else if case < 7 {
            p.end_char_shape = Some(other);
        } else if case == 7 {
            p.runs.push(Run::text("\n", id));
            p.runs.push(Run::text("B", other));
        } else if case == 8 {
            p.runs.push(Run::text("B", other));
            s.char_shape_mut(other).unwrap().bold = true;
            p.end_char_shape = Some(other);
        } else if case == 9 {
            p.end_char_shape = Some(other);
            s.char_shape_mut(other).unwrap().rel_sz = 99;
        } else if case == 10 {
            p.end_char_shape = Some(other);
            s.char_shape_mut(other).unwrap().height = HwpUnit::new(1200).unwrap();
        } else {
            s.char_shape_mut(other).unwrap().height = HwpUnit::new(1200).unwrap();
            p.runs.push(Run::text("", other));
        }
        assert_eq!(
            values(&encode(d, &s).bytes),
            [if [7, 8, 11].contains(&case) { 87 } else { 100 }],
            "case {case}"
        );
    }
}
// 이것을 실패시키는 것: 상태를 JSON으로 복원하거나 프로필로 추정, 낡은 배수를 현재 값에 덮어쓰기.
#[test]
fn word_line_spacing_json_profile_and_mutation_lifetime() {
    let (d, s) = fixture("Cambria", 1100, 100., "ASCII");
    let json = serde_json::to_value(&s).unwrap();
    assert!(json.get("imported_percentages").is_none());
    let restored: HwpxStyleStore = serde_json::from_value(json.clone()).unwrap();
    assert_ne!(s, restored);
    assert_eq!(serde_json::to_value(&restored).unwrap(), json);
    assert_eq!(values(&encode(d.clone(), &restored).bytes), [100]);
    let mut fake = json;
    fake["imported_percentages"] = serde_json::json!({"20":100});
    assert_eq!(restored, serde_json::from_value::<HwpxStyleStore>(fake).unwrap());
    let mut hancom = d.clone();
    hancom.set_layout_profile(LayoutProfile::Hancom);
    assert_eq!(values(&encode(hancom, &s).bytes), [100]);
    let mut changed = s.clone();
    changed.para_shape_mut(d.sections()[0].paragraphs[0].para_shape_id).unwrap().line_spacing = 160;
    assert_eq!(values(&encode(d.clone(), &changed).bytes), [160]);
    for kind in [LineSpacingType::Fixed, LineSpacingType::AtLeast] {
        let mut changed = s.clone();
        changed
            .para_shape_mut(d.sections()[0].paragraphs[0].para_shape_id)
            .unwrap()
            .line_spacing_type = kind;
        let out = encode(d.clone(), &changed);
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&out.bytes).unwrap();
        let para = &decoded.document.sections()[0].paragraphs[0];
        let shape = decoded.style_store.para_shape(para.para_shape_id).unwrap();
        assert_eq!(shape.line_spacing_type, kind);
        assert_eq!(shape.line_spacing, 100);
    }
    let original = s.clone();
    let doc = d.validate().unwrap();
    let first = encode_valid(&doc, &s, EncodeOptions::default());
    let second = encode_valid(&doc, &s, EncodeOptions::default());
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(s, original);
    assert_eq!(values(&first.bytes), [87]);
}
// 이것을 실패시키는 것: 공유 paraPr 직접 변경, 키 재사용 누락, 본문/중첩 칸 참조 교체 누락, LATIN 혼합 무시.
#[test]
fn word_line_spacing_shared_definitions_and_table_cells() {
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::FontIndex;
    let (mut d, mut s) = fixture("Cambria", 1100, 100., "ASCII");
    let original = s.clone();
    let p = d.sections()[0].paragraphs[0].clone();
    let id = p.runs[0].char_shape_id;
    let mut calibri = s.char_shape(id).unwrap().clone();
    s.push_font(hwpforge_smithy_hwpx::HwpxFont::new(2, "Calibri", "LATIN"));
    calibri.font_ref.latin = FontIndex::new(2);
    let calibri = s.push_char_shape(calibri);
    let mut korean = p.clone();
    korean.runs[0] = Run::text("한글", id);
    let mut latin = p.clone();
    latin.runs[0] = Run::text("Latin", calibri);
    let mut mixed = p.clone();
    mixed.runs.push(Run::text("different", calibri));
    let mut other = p.clone();
    let mut c = s.char_shape(id).unwrap().clone();
    c.font_ref.latin = FontIndex::new(0);
    c.font_ref.hangul = FontIndex::new(1);
    let other_id = s.push_char_shape(c);
    other.runs[0] = Run::text("wrong slot", other_id);
    let inner = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![p.clone(), latin.clone()],
        HwpUnit::new(10000).unwrap(),
    )])]);
    let host =
        Paragraph::with_runs(vec![Run::table(inner, id), Run::text("host", id)], p.para_shape_id);
    d.sections_mut()[0].paragraphs = vec![korean, p.clone(), p.clone(), latin, mixed, other, host];
    let mut only_cells = d.clone();
    only_cells.sections_mut()[0].paragraphs =
        vec![only_cells.sections()[0].paragraphs.last().unwrap().clone()];
    assert_eq!(values(&encode(only_cells, &s).bytes), [100, 87, 85]);
    let out = encode(d, &s);
    assert_eq!(values(&out.bytes), [100, 87, 87, 85, 100, 100, 100, 87, 85]);
    let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    assert_eq!(h.descendants("hh:paraPr").len(), s.para_shape_count() + 2);
    assert_eq!(
        s.para_shape(p.para_shape_id).unwrap(),
        original.para_shape(p.para_shape_id).unwrap()
    );
    let section = xml::Element::parse(&package::part(&out.bytes, "Contents/section0.xml"));
    let ps = section.descendants("hp:p");
    assert_eq!(ps[1].attr("paraPrIDRef"), ps[2].attr("paraPrIDRef"));
}
// 이것을 실패시키는 것: 캐시 요청 검사/전체 억제/문단별 경고 누락, 잘못된 경로/중복 경고/폐기 표지 방출.
#[test]
fn word_line_spacing_cache_suppression_uses_emission_paths() {
    use hwpforge_core::{
        layout::{LayoutCache, LineSeg},
        Table, TableCell, TableRow,
    };
    use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};
    for case in [0, 1, 2, 3, 4, 5, 7] {
        let (mut d, s) = fixture("Cambria", 1100, 100., "ASCII");
        let mut p = d.sections()[0].paragraphs[0].clone();
        let mut seg = LineSeg::default();
        seg.vertsize = 1000;
        seg.textheight = 1000;
        seg.baseline = 850;
        seg.horzsize = 10000;
        p.layout_cache = Some(LayoutCache::new(vec![seg]));
        let mut korean = p.clone();
        korean.runs[0].content = hwpforge_core::RunContent::Text("한글".into());
        let mut no_cache = p.clone();
        no_cache.layout_cache = None;
        if case == 1 {
            p.layout_cache = None;
        }
        if case == 2 {
            p.layout_cache = None;
            korean.layout_cache = None;
        }
        if case == 4 {
            p.runs[0].content = hwpforge_core::RunContent::Text("한글".into());
            no_cache.runs[0].content = hwpforge_core::RunContent::Text("한글".into());
        }
        if case == 5 {
            p.layout_cache.as_mut().unwrap().lines[0].textpos = 999;
        }
        d.sections_mut()[0].paragraphs = vec![p.clone(), korean, no_cache];
        if case == 7 {
            let t = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![p.clone()],
                HwpUnit::new(10000).unwrap(),
            )])]);
            d.sections_mut()[0].paragraphs.push(Paragraph::with_runs(
                vec![Run::table(t, p.runs[0].char_shape_id)],
                p.para_shape_id,
            ));
        }
        let options = EncodeOptions::default().with_emit_layout_cache(case != 3);
        let out = encode_valid(&d.validate().unwrap(), &s, options);
        let tree = xml::Element::parse(&package::part(&out.bytes, "Contents/section0.xml"));
        let warnings: Vec<_> = out
            .warnings
            .iter()
            .filter_map(|w| {
                if let EncodeWarning::LayoutCacheDropped { path, reason } = w {
                    Some((path, reason))
                } else {
                    None
                }
            })
            .collect();
        if case == 4 {
            assert_eq!(tree.descendants("hp:linesegarray").len(), 2);
            assert!(warnings.is_empty());
            continue;
        }
        assert!(tree.descendants("hp:linesegarray").is_empty());
        let mut expected = vec![
            ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(0)]),
            ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)]),
        ];
        if case == 1 {
            expected.remove(0);
        }
        if case == 2 || case == 3 {
            expected.clear();
        }
        if case == 7 {
            expected.push(ParagraphPath(vec![
                PathSeg::Section(0),
                PathSeg::BodyParagraph(3),
                PathSeg::Run(0),
                PathSeg::TableCell { row: 0, cell: 0 },
                PathSeg::NestedParagraph(0),
            ]));
        }
        assert_eq!(
            warnings.iter().map(|(p, _)| (*p).clone()).collect::<Vec<_>>(),
            expected,
            "{case}"
        );
        for (_, reason) in warnings {
            assert_eq!(reason, "Layout cache omitted because imported line spacing was projected");
        }
    }
}
// 이것을 실패시키는 것: 머리말/꼬리말/각주/미주/글상자/캡션/바탕쪽에서 본문 키를 사용하거나 그 안 표를 보정.
#[test]
fn word_line_spacing_excluded_containers_keep_original_references() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        section::{HeaderFooter, MasterPage},
        Control, Table, TableCell, TableRow,
    };
    use hwpforge_foundation::{ApplyPageType, VerticalAlign};
    let (mut d, s) = fixture("Cambria", 1100, 100., "ASCII");
    let p = d.sections()[0].paragraphs[0].clone();
    let id = p.runs[0].char_shape_id;
    let t = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![p.clone()],
        HwpUnit::new(10000).unwrap(),
    )])]);
    let host = Paragraph::with_runs(vec![Run::table(t.clone(), id)], p.para_shape_id);
    let section = &mut d.sections_mut()[0];
    section.headers.push(HeaderFooter::new(vec![p.clone(), host.clone()], ApplyPageType::Both));
    section.footers.push(HeaderFooter::new(vec![p.clone()], ApplyPageType::Both));
    section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![p.clone()])]);
    for control in [
        Control::Footnote { inst_id: None, paragraphs: vec![p.clone()] },
        Control::Endnote { inst_id: None, paragraphs: vec![p.clone()] },
        Control::TextBox {
            frame: hwpforge_core::ObjectFrame::new(),
            text_margin: None,
            paragraphs: vec![p.clone()],
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(5000).unwrap(),
            placement: None,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        },
    ] {
        section
            .paragraphs
            .push(Paragraph::with_runs(vec![Run::control(control, id)], p.para_shape_id));
    }
    let mut table = t;
    table.caption = Some(Caption::new(vec![p.clone()], CaptionSide::Bottom));
    section.paragraphs.push(Paragraph::with_runs(vec![Run::table(table, id)], p.para_shape_id));
    let out = encode(d, &s);
    let tree = xml::Element::parse(&package::part(&out.bytes, "Contents/section0.xml"));
    let original = p.para_shape_id.get().to_string();
    for container in
        ["hp:header", "hp:footer", "hp:footNote", "hp:endNote", "hp:drawText", "hp:caption"]
    {
        let containers = tree.descendants(container);
        assert!(!containers.is_empty(), "{container}");
        for c in containers {
            for p in c.descendants("hp:p") {
                assert_eq!(p.attr("paraPrIDRef"), Some(original.as_str()), "{container}");
            }
        }
    }
    let master = xml::Element::parse(&package::part(&out.bytes, "Contents/masterpage0.xml"));
    for p in master.descendants("hp:p") {
        assert_eq!(p.attr("paraPrIDRef"), Some(original.as_str()));
    }
}
// 이것을 실패시키는 것: 목록/별칭 family 제외 제거, 끝 family 검사 누락, 색 차이를 높이 차이로 취급.
#[test]
fn word_line_spacing_lists_families_and_end_colors() {
    use hwpforge_foundation::{Color, FontIndex, HeadingType};
    for family in ["Arial", "Cambria Math", "cambria", "Calibri Light"] {
        let (d, s) = fixture(family, 1100, 100., "ASCII");
        assert_eq!(values(&encode(d, &s).bytes), [100]);
    }
    for case in 0..5 {
        let (mut d, mut s) = fixture("Cambria", 1100, 100., "ASCII");
        let p = &mut d.sections_mut()[0].paragraphs[0];
        let mut c = s.char_shape(p.runs[0].char_shape_id).unwrap().clone();
        if case == 0 {
            c.font_ref.latin = FontIndex::new(0);
        } else {
            c.text_color = Color::from_rgb(255, 0, 0);
        }
        p.end_char_shape = Some(s.push_char_shape(c));
        if case >= 2 {
            s.para_shape_mut(p.para_shape_id).unwrap().heading_id_ref = 1;
            s.para_shape_mut(p.para_shape_id).unwrap().heading_type =
                if case == 2 { HeadingType::Outline } else { HeadingType::Number };
        }
        if case == 4 {
            s.para_shape_mut(p.para_shape_id).unwrap().heading_id_ref = 1;
        }
        assert_eq!(values(&encode(d, &s).bytes), [if case == 1 { 87 } else { 100 }], "{case}");
    }
}

// 이것을 실패시키는 것: 비텍스트 필드를 건너뛰고 옆 ASCII만으로 후보 판정.
#[test]
fn word_line_spacing_mixed_field_is_not_text_only() {
    let (mut d, s) = fixture("Cambria", 1100, 100., "ASCII");
    let p = &mut d.sections_mut()[0].paragraphs[0];
    p.runs.push(Run::control(hwpforge_core::Control::field("value"), p.runs[0].char_shape_id));
    assert_eq!(values(&encode(d, &s).bytes), [100]);
}
