use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
    ListMarkerAlign, ListMarkerLayout, ListTextStart,
};
use hwpforge_core::{
    BulletDef, Document, ImageStore, NumberingDef, PageSettings, ParaHead, Paragraph,
    ParagraphListRef, Run, Section, Table, TableCell, TableRow, Validated,
};
use hwpforge_foundation::{
    BulletIndex, CharShapeIndex, HwpUnit, NumberFormatType, NumberingIndex, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::{
    style_store::HwpxParaShape, EncodeOptions, EncodeOutcome, EncodeWarning, HwpxEncoder,
    HwpxRegistryBridge, HwpxStyleStore,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
fn u(n: i32) -> HwpUnit {
    HwpUnit::new(n).unwrap()
}
fn layout(align: ListMarkerAlign, start: ListTextStart, char: Option<usize>) -> ListMarkerLayout {
    let mut m = ListMarkerLayout::new(align, start).unwrap();
    m.char_shape = char.map(CharShapeIndex::new);
    m
}
fn fixed(g: i32) -> ListMarkerLayout {
    layout(ListMarkerAlign::Left, ListTextStart::FromAnchor(u(g)), Some(0))
}
fn fixture(
    requests: &[(i32, Option<ListMarkerLayout>)],
    bullet: Option<&str>,
    cell: bool,
) -> (Document<Validated>, HwpxStyleStore) {
    let mut registry:StyleRegistry=serde_json::from_value(serde_json::json!({"fonts":["맑은 고딕","Arial"],"char_shapes":[],"para_shapes":[],"style_entries":{}})).unwrap();
    for (font, size) in [("맑은 고딕", 1100), ("Arial", 1400)] {
        let mut c = PartialCharShape::default();
        c.font = Some(font.into());
        c.size = Some(u(size));
        registry.char_shapes.push(c.resolve("body").unwrap());
    }
    let head = ParaHead {
        level: 1,
        start: 1,
        num_format: NumberFormatType::Digit,
        text: "^1.".into(),
        checkable: false,
    };
    registry.numberings.push(NumberingDef { id: 99, start: 1, levels: vec![head.clone()] });
    if let Some(glyph) = bullet {
        registry.bullets.push(BulletDef {
            id: 99,
            bullet_char: glyph.into(),
            checked_char: None,
            use_image: false,
            para_head: head,
        });
    }
    let mut paragraphs = vec![];
    for (i, (anchor, request)) in requests.iter().enumerate() {
        let mut shape = ParaShape::default();
        shape.line_spacing_value = 160.;
        shape.indent_left = u(*anchor);
        shape.list_marker = *request;
        shape.list = Some(if bullet.is_some() {
            ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 }
        } else {
            ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 0 }
        });
        registry.para_shapes.push(shape);
        paragraphs.push(Paragraph::with_runs(
            vec![Run::text("body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(i),
        ));
    }
    if cell {
        let nested = Table::new(vec![TableRow::new(vec![TableCell::new(paragraphs, u(12000))])]);
        paragraphs = vec![Paragraph::with_runs(
            vec![Run::table(nested, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )];
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let bridge =
        HwpxRegistryBridge::from_registry_for_import(&registry, Default::default()).unwrap().value;
    let doc = bridge.rebind_draft_document(doc).unwrap().validate().unwrap();
    (doc, bridge.into_style_store())
}
fn encode(doc: &Document<Validated>, store: &HwpxStyleStore) -> EncodeOutcome {
    HwpxEncoder::encode_with_diagnostics(doc, store, &ImageStore::new(), EncodeOptions::default())
        .unwrap()
}
fn header(out: &EncodeOutcome) -> xml::Element {
    xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"))
}
fn head(h: &xml::Element, bullet: bool) -> &xml::Element {
    let (tag, id) = if bullet { ("hh:bullet", "1") } else { ("hh:numbering", "2") };
    let def = h.descendants(tag).into_iter().find(|d| d.attr("id") == Some(id)).unwrap();
    let level = if bullet { "0" } else { "1" };
    def.descendants("hh:paraHead").into_iter().find(|d| d.attr("level") == Some(level)).unwrap()
}
fn metrics(h: &xml::Element, bullet: bool, expected: [&str; 7]) {
    let node = head(h, bullet);
    for (attr, value) in [
        "align",
        "useInstWidth",
        "autoIndent",
        "widthAdjust",
        "textOffsetType",
        "textOffset",
        "charPrIDRef",
    ]
    .into_iter()
    .zip(expected)
    {
        assert_eq!(node.attr(attr), Some(value), "{attr}");
    }
}
fn margins(h: &xml::Element, left: &str, default: &str) {
    let p = h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("20")).unwrap();
    let switch = p.child("hp:switch");
    assert_eq!(
        switch.child("hp:case").child("hh:margin").child("hc:left").attr("value"),
        Some(left)
    );
    assert_eq!(
        switch.child("hp:default").child("hh:margin").child("hc:left").attr("value"),
        Some(default)
    );
    assert_eq!(
        switch.child("hp:case").child("hh:margin").child("hc:intent").attr("value"),
        Some("0")
    );
}
fn losses(out: &EncodeOutcome, kind: &str) -> usize {
    out.warnings
        .iter()
        .filter(|w| matches!(w,EncodeWarning::ContentLost{kind:k,..} if k==kind))
        .count()
}
// 이것을 실패시키는 것: useInstWidth=1 또는 기본폭 차감 누락.
#[test]
fn list_marker_left_fixed_wire() {
    for cell in [false, true] {
        let (d, s) = fixture(&[(1800, Some(fixed(1800)))], None, cell);
        let h = header(&encode(&d, &s));
        metrics(&h, false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
        margins(&h, "1800", "3600");
    }
}
// 이것을 실패시키는 것: A−g 이동 누락 또는 g ≥ B에서 칸 폭을 g+B로 변경,
// 또는 근사 경고의 g < B를 g ≤ B로 변경.
#[test]
fn list_marker_right_fixed_wire() {
    for cell in [false, true] {
        for (g, width, offset, left, default) in
            [(1100, "0", "1100", "700", "1400"), (1800, "700", "1800", "0", "0")]
        {
            let (d, s) = fixture(
                &[(
                    1800,
                    Some(layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(g)), Some(0))),
                )],
                None,
                cell,
            );
            let out = encode(&d, &s);
            let h = header(&out);
            metrics(&h, false, ["RIGHT", "0", "1", width, "HWPUNIT", offset, "7"]);
            margins(&h, left, default);
            assert_eq!(losses(&out, "list_marker_alignment"), 0);
        }
    }
}
// 이것을 실패시키는 것: max 제거 → widthAdjust −600 · margin 3100/6200,
// margin에서 g 차감 → 3100/6200, textOffset을 0이나 B로 대체 → 본문 위치 변경,
// 또는 미측정 구간의 기존 정렬 근사 경고 누락/사유를 Left로 표기.
#[test]
fn list_marker_right_narrow_gap_keeps_anchor_and_text_offset() {
    let (d, s) = fixture(
        &[(3600, Some(layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(500)), Some(0))))],
        None,
        false,
    );
    let out = encode(&d, &s);
    let h = header(&out);
    metrics(&h, false, ["RIGHT", "0", "1", "0", "HWPUNIT", "500", "7"]);
    margins(&h, "2500", "5000");
    assert_eq!(losses(&out, "list_marker_alignment"), 1);
    assert!(out.warnings.iter().any(|w| matches!(
        w,
        EncodeWarning::ContentLost { kind, reason, .. }
            if kind == "list_marker_alignment"
                && reason
                    == concat!(
                        "Unmeasured narrow right-aligned marker slot ",
                        "is widened to the nominal base width"
                    )
    )));
}
// 이것을 실패시키는 것: 중앙 슬롯에 2g 대신 g 사용.
#[test]
fn list_marker_center_fixed_wire() {
    for cell in [false, true] {
        let (d, s) = fixture(
            &[(
                1800,
                Some(layout(ListMarkerAlign::Center, ListTextStart::FromAnchor(u(1800)), Some(0))),
            )],
            None,
            cell,
        );
        let h = header(&encode(&d, &s));
        metrics(&h, false, ["CENTER", "0", "1", "2500", "HWPUNIT", "0", "7"]);
        margins(&h, "0", "0");
    }
}
// 이것을 실패시키는 것: 명시적 d=0을 legacy offset50으로 대체.
#[test]
fn list_marker_after_marker_wire() {
    for (d, wire) in [(306, "306"), (0, "0")] {
        let (doc, s) = fixture(
            &[(
                1800,
                Some(layout(ListMarkerAlign::Left, ListTextStart::AfterMarker(u(d)), Some(0))),
            )],
            None,
            false,
        );
        let h = header(&encode(&doc, &s));
        metrics(&h, false, ["LEFT", "1", "1", "0", "HWPUNIT", wire, "7"]);
        margins(&h, "1800", "3600");
    }
}
// 이것을 실패시키는 것: 미측정 정렬의 손실 경고 누락.
#[test]
fn list_marker_unmeasured_alignment_warns() {
    for align in [ListMarkerAlign::Right, ListMarkerAlign::Center] {
        let (d, s) = fixture(
            &[(1800, Some(layout(align, ListTextStart::AfterMarker(u(306)), Some(0))))],
            None,
            false,
        );
        let out = encode(&d, &s);
        metrics(&header(&out), false, ["LEFT", "1", "1", "0", "HWPUNIT", "306", "7"]);
        assert_eq!(losses(&out, "list_marker_alignment"), 1);
    }
}
// 이것을 실패시키는 것: 음수 widthAdjust를 0으로 절삭하거나 g=0을 미지정 취급.
#[test]
fn list_marker_small_and_zero_slots() {
    for (g, width) in [(900, "-200"), (0, "-1100")] {
        let (d, s) = fixture(&[(1800, Some(fixed(g)))], None, false);
        metrics(&header(&encode(&d, &s)), false, ["LEFT", "0", "1", width, "PERCENT", "0", "7"]);
    }
    let (d, s) = fixture(&[(1800, None)], None, false);
    metrics(
        &header(&encode(&d, &s)),
        false,
        ["LEFT", "1", "1", "0", "PERCENT", "50", "4294967295"],
    );
}
// 이것을 실패시키는 것: registry 문자 참조를 출력 참조로 직접 기록.
#[test]
fn list_marker_font_reference_is_rebound() {
    let mut m = fixed(1800);
    m.char_shape = Some(CharShapeIndex::new(1));
    let (d, s) = fixture(&[(1800, Some(m))], None, false);
    let out = encode(&d, &s);
    let h = header(&out);
    metrics(&h, false, ["LEFT", "0", "1", "400", "PERCENT", "0", "8"]);
    let c = h.descendants("hh:charPr").into_iter().find(|c| c.attr("id") == Some("8")).unwrap();
    assert_eq!(c.attr("height"), Some("1400"));
    let section = xml::Element::parse(&package::part(&out.bytes, "Contents/section0.xml"));
    assert!(section.descendants("hp:run").iter().any(|r| r.attr("charPrIDRef") == Some("7")));
}
// 이것을 실패시키는 것: 상속 문자 모양 대신 store 기본 문자 모양 사용.
#[test]
fn list_marker_inherited_font_is_not_shape_zero() {
    let mut m = fixed(1800);
    m.char_shape = None;
    let (d, s) = fixture(&[(1800, Some(m))], None, false);
    metrics(
        &header(&encode(&d, &s)),
        false,
        ["LEFT", "0", "1", "700", "PERCENT", "0", "4294967295"],
    );
}
// 이것을 실패시키는 것: 번호 경로에만 배치 변환 적용.
#[test]
fn list_marker_bullet_uses_shared_projection() {
    for glyph in ["●", "■"] {
        let (d, s) = fixture(&[(1800, Some(fixed(1800)))], Some(glyph), false);
        metrics(&header(&encode(&d, &s)), true, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    }
}
// 이것을 실패시키는 것: 충돌을 마지막 요청으로 덮어쓰기.
#[test]
fn list_marker_conflict_keeps_first_use() {
    let (d, s) = fixture(
        &[(1800, Some(fixed(1800))), (2400, Some(fixed(2400))), (3000, Some(fixed(3000)))],
        None,
        false,
    );
    let out = encode(&d, &s);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    margins(&header(&out), "1800", "3600");
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    let reason = out
        .warnings
        .iter()
        .find_map(|w| match w {
            EncodeWarning::ContentLost { kind, reason, .. } if kind == "list_marker_conflict" => {
                Some(reason)
            }
            _ => None,
        })
        .unwrap();
    assert!(reason.contains("align=LEFT useInstWidth=0 autoIndent=1 widthAdjust=1300 textOffsetType=PERCENT textOffset=0 charPrIDRef=7"));
    assert!(reason.contains("margin shift 0"));
    assert!(reason.contains("first use head and relative margin shift applied"));
    assert!(reason.contains("3 uses"));
    margin_at(&header(&out), "21", "2400", "4800");
    let mut reversed: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    reversed.sections_mut()[0].paragraphs.swap(0, 1);
    metrics(
        &header(&encode(&reversed.validate().unwrap(), &s)),
        false,
        ["LEFT", "0", "1", "1300", "PERCENT", "0", "7"],
    );
}
// 이것을 실패시키는 것: char/align 비교 누락, anchor까지 충돌 키에 포함 또는 첫 head를 기존값으로 복귀.
#[test]
fn list_marker_conflict_compares_complete_metrics() {
    let m = fixed(1800);
    for other in [
        layout(ListMarkerAlign::Left, m.text_start, Some(1)),
        layout(ListMarkerAlign::Right, m.text_start, Some(0)),
    ] {
        let (d, s) = fixture(&[(1800, Some(m)), (1800, Some(other))], None, false);
        let out = encode(&d, &s);
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
        metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
        margin_at(&header(&out), "20", "1800", "3600");
    }
    use hwpforge_foundation::Alignment::{Left, Right};
    for (align, instant, auto, width, offset, hwpunit, char_ref) in [
        (Left, false, true, 700, 1800, true, 7),
        (Right, true, true, 700, 1800, true, 7),
        (Right, false, false, 700, 1800, true, 7),
        (Right, false, true, 701, 1800, true, 7),
        (Right, false, true, 700, 1801, true, 7),
        (Right, false, true, 700, 1800, false, 7),
        (Right, false, true, 700, 1800, true, 8),
    ] {
        let (d, mut s) = fixture(
            &[
                (
                    1800,
                    Some(layout(
                        ListMarkerAlign::Right,
                        ListTextStart::FromAnchor(u(1800)),
                        Some(0),
                    )),
                ),
                (1800, None),
            ],
            None,
            false,
        );
        s.set_paragraph_head_metrics(
            hwpforge_foundation::HeadingType::Number,
            2,
            1,
            align,
            instant,
            auto,
            width,
            offset,
            hwpunit,
            char_ref,
        )
        .unwrap();
        let out = encode(&d, &s);
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
        metrics(&header(&out), false, ["RIGHT", "0", "1", "700", "HWPUNIT", "1800", "7"]);
        margin_at(&header(&out), "20", "0", "0");
    }
    let (d, s) = fixture(&[(1800, Some(m)), (2400, Some(m))], None, false);
    assert_eq!(losses(&encode(&d, &s), "list_marker_conflict"), 0);
}
// 이것을 실패시키는 것: None 사용처를 집계에서 제외하거나 선택한 첫 head를 기존값으로 복귀.
#[test]
fn list_marker_unspecified_use_participates() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800))), (1800, None)], None, false);
    let out = encode(&d, &s);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    margin_at(&header(&out), "20", "1800", "3600");
    margin_at(&header(&out), "21", "1800", "3600");
}
// 이것을 실패시키는 것: 무요청 첫 head를 setter에 다시 넣어 원본 scalar를 정규화하거나 margin을 이동.
#[test]
fn list_marker_unspecified_first_preserves_original_scalars() {
    for bullet in [None, Some("●")] {
        let (d, s) = fixture(&[(1800, None), (2400, Some(fixed(2400)))], bullet, false);
        let mut value = serde_json::to_value(&s).unwrap();
        let metrics = if bullet.is_some() {
            &mut value["header_fidelity"]["bullet"]["1"]["0"]
        } else {
            &mut value["header_fidelity"]["numbering"]["2"]["1"]
        };
        *metrics = serde_json::json!({
            "align":"LEFT", "use_inst_width":2, "auto_indent":1, "width_adjust":17,
            "text_offset_type":"PERCENT", "text_offset":37, "char_pr_id_ref":7
        });
        let store = serde_json::from_value(value.clone()).unwrap();
        let out = encode(&d, &store);
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
        let h = header(&out);
        self::metrics(&h, bullet.is_some(), ["LEFT", "2", "1", "17", "PERCENT", "37", "7"]);
        margin_at(&h, "20", "1800", "3600");
        margin_at(&h, "21", "2400", "4800");
        assert_eq!(serde_json::to_value(&store).unwrap(), value);
    }
}
// 이것을 실패시키는 것: 글머리 문단 수준별 가상 head 생성 또는 충돌에서 기존 head로 복귀.
#[test]
fn list_marker_shared_bullet_head_conflicts() {
    let (d, mut s) =
        fixture(&[(1800, Some(fixed(1800))), (1800, Some(fixed(2400)))], Some("●"), false);
    s.para_shape_mut(ParaShapeIndex::new(21)).unwrap().heading_level = 1;
    let out = encode(&d, &s);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    metrics(&header(&out), true, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    margin_at(&header(&out), "20", "1800", "3600");
    margin_at(&header(&out), "21", "1800", "3600");
}
// 이것을 실패시키는 것: 포화 산술로 범위 밖 좌표 출력.
#[test]
fn list_marker_checked_arithmetic_rejects_overflow() {
    for (a, align, g) in [
        (-100000000, ListMarkerAlign::Right, 1),
        (100000000, ListMarkerAlign::Left, 1),
        (0, ListMarkerAlign::Center, 50000001),
    ] {
        let (d, s) = fixture(
            &[(a, Some(layout(align, ListTextStart::FromAnchor(u(g)), Some(0))))],
            None,
            false,
        );
        assert!(HwpxEncoder::encode(&d, &s, &ImageStore::new()).is_err());
    }
}
// 이것을 실패시키는 것: 출력 중 원본 store 변경 또는 margin 두 번 이동.
#[test]
fn list_marker_projection_is_repeatable() {
    let (d, s) = fixture(
        &[(
            1800,
            Some(layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(1800)), Some(0))),
        )],
        None,
        false,
    );
    let before = serde_json::to_value(&s).unwrap();
    let a = encode(&d, &s);
    let b = encode(&d, &s);
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(a.warnings, b.warnings);
    assert_eq!(serde_json::to_value(&s).unwrap(), before);
    let (plain, plain_store) = fixture(&[(1800, None)], None, false);
    let base = encode(&plain, &plain_store);
    let edited = HwpxEncoder::encode_with_base(
        &base.bytes,
        &d,
        &s,
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .unwrap();
    margins(&header(&edited), "0", "0");
}
// 이것을 실패시키는 것: 요청 map의 JSON 왕복 손실 또는 충돌에서 선택한 첫 head로 투영하지 않음.
#[test]
fn list_marker_request_survives_store_json() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800))), (1800, Some(fixed(2400)))], None, false);
    let s: HwpxStyleStore = serde_json::from_value(serde_json::to_value(s).unwrap()).unwrap();
    let out = encode(&d, &s);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    margin_at(&header(&out), "20", "1800", "3600");
    margin_at(&header(&out), "21", "1800", "3600");
}

// 이것을 실패시키는 것: 생성자만 검증하고 사후 참조 변경 허용.
#[test]
fn list_marker_reference_is_validated_after_mutation() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800)))], None, false);
    let mut json = serde_json::to_value(&s).unwrap();
    json["list_marker_layouts"]["20"]["char_shape"] = 999.into();
    assert!(serde_json::from_value::<HwpxStyleStore>(json).is_err());
    let mut s = s;
    s.para_shape_mut(ParaShapeIndex::new(20)).unwrap().heading_type =
        hwpforge_foundation::HeadingType::None;
    assert!(HwpxEncoder::encode(&d, &s, &ImageStore::new()).is_err());
}
// 이것을 실패시키는 것: 새 요청 뒤에 원본 fidelity를 다시 적용.
#[test]
fn list_marker_fidelity_has_explicit_precedence() {
    for request in [None, Some(fixed(1800))] {
        let (d, mut s) = fixture(&[(1800, request)], None, false);
        s.set_paragraph_head_metrics(
            hwpforge_foundation::HeadingType::Number,
            2,
            1,
            hwpforge_foundation::Alignment::Center,
            false,
            false,
            -17,
            23,
            true,
            0,
        )
        .unwrap();
        let h = header(&encode(&d, &s));
        metrics(
            &h,
            false,
            if request.is_some() {
                ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]
            } else {
                ["CENTER", "0", "0", "-17", "HWPUNIT", "23", "0"]
            },
        );
    }
}
// 이것을 실패시키는 것: Word 호환 모드에서도 자동 간격 0 고정.
#[test]
fn word_auto_spacing_defaults_to_enabled() {
    for (profile, value) in [
        (hwpforge_core::LayoutProfile::Hancom, "0"),
        (hwpforge_core::LayoutProfile::WordCompatible, "1"),
    ] {
        let (d, s) = fixture(&[(1800, None)], None, false);
        let mut d: Document<hwpforge_core::Draft> =
            serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
        d.set_layout_profile(profile);
        let out = encode(&d.validate().unwrap(), &s);
        let h = header(&out);
        let p =
            h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("20")).unwrap();
        let a = p.child("hh:autoSpacing");
        assert_eq!(a.attr("eAsianEng"), Some(value));
        assert_eq!(a.attr("eAsianNum"), Some(value));
    }
}
// 이것을 실패시키는 것: 원본 자동 간격 값 또는 부재를 profile 기본값으로 덮기.
#[test]
fn hwpx_auto_spacing_fidelity_roundtrips() {
    let (d, s) = fixture(&[(1800, None)], None, false);
    let mut d: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
    d.set_layout_profile(hwpforge_core::LayoutProfile::WordCompatible);
    let out = encode(&d.validate().unwrap(), &s);
    // 이것을 실패시키는 것: 원본 수치를 bool로 정규화하거나 요소 부재와 키 없음을 합침.
    for (replacement, de, dn) in [
        // 이것을 실패시키는 것: 원본 꺼짐을 프로필 기본 켬으로 덮음.
        (r#"<hh:autoSpacing eAsianEng="0" eAsianNum="0"/>"#, "0", "0"),
        (r#"<hh:autoSpacing eAsianEng="0" eAsianNum="1"/>"#, "0", "1"),
        // 이것을 실패시키는 것: 영문 켬과 숫자 꺼짐을 같은 축으로 복사.
        (r#"<hh:autoSpacing eAsianEng="1" eAsianNum="0"/>"#, "1", "0"),
        // 이것을 실패시키는 것: 원본 켬을 한컴 프로필 기본 꺼짐으로 덮음.
        (r#"<hh:autoSpacing eAsianEng="1" eAsianNum="1"/>"#, "1", "1"),
        // 이것을 실패시키는 것: 원본 비정규 수치를 bool로 정규화.
        (r#"<hh:autoSpacing eAsianEng="2" eAsianNum="7"/>"#, "2", "7"),
        ("", "", ""),
    ] {
        let bytes = package::splice_part(
            &out.bytes,
            "Contents/header.xml",
            r#"<hh:autoSpacing eAsianEng="1" eAsianNum="1"/>"#,
            replacement,
        );
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
        let mut store: HwpxStyleStore =
            serde_json::from_value(serde_json::to_value(decoded.style_store).unwrap()).unwrap();
        let mut p = store.para_shape(ParaShapeIndex::new(20)).unwrap().clone();
        p.heading_type = hwpforge_foundation::HeadingType::None;
        store.push_para_shape(p);
        let out = encode(&decoded.document.validate().unwrap(), &store);
        let h = header(&out);
        let a =
            h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("20")).unwrap();
        if replacement.is_empty() {
            assert!(a.children.iter().all(|c| c.name != "hh:autoSpacing"));
        } else {
            assert_eq!(a.child("hh:autoSpacing").attr("eAsianEng"), Some(de));
            assert_eq!(a.child("hh:autoSpacing").attr("eAsianNum"), Some(dn));
        }
        let a =
            h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("21")).unwrap();
        assert_eq!(a.child("hh:autoSpacing").attr("eAsianEng"), Some("1"));
    }
}
// 이것을 실패시키는 것: 새 marker 요청이 있을 때 기존 조판 캐시 재사용.
#[test]
fn marker_header_changes_invalidate_layout_cache() {
    let (d, s) = fixture(&[(1800, None)], None, false);
    let base = encode(&d, &s);
    let base = package::splice(
        &base.bytes,
        "</hp:p>",
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1100" textheight="1100" baseline="850" spacing="600" horzpos="0" horzsize="10000" flags="393216"/></hp:linesegarray></hp:p>"#,
    );
    for (gap, character) in [(1800, 7), (2400, 7), (1800, 8)] {
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&base).unwrap();
        assert!(decoded.document.sections()[0].paragraphs[0].layout_cache.is_some());
        let mut value = serde_json::to_value(&decoded.style_store).unwrap();
        value["list_marker_layouts"] = serde_json::json!({"20":{"align":"left","text_start":{"kind":"from_anchor","value":gap},"char_shape":character}});
        let store: HwpxStyleStore = serde_json::from_value(value).unwrap();
        let doc = decoded.document.validate().unwrap();
        let out = HwpxEncoder::encode_with_diagnostics(
            &doc,
            &store,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        assert!(xml::Element::parse(&package::section_xml(&out.bytes))
            .descendants("hp:linesegarray")
            .is_empty());
        let out = HwpxEncoder::encode_with_base(
            &base,
            &doc,
            &store,
            &ImageStore::new(),
            EncodeOptions::default(),
        )
        .unwrap();
        assert!(xml::Element::parse(&package::section_xml(&out.bytes))
            .descendants("hp:linesegarray")
            .is_empty());
    }
}
// 이것을 실패시키는 것: 미사용 모양도 충돌에 포함하거나 일반 문단을 목록 head로 해석.
#[test]
fn unused_marker_shapes_do_not_create_conflicts() {
    let (d, mut s) = fixture(&[(1800, Some(fixed(1800)))], None, false);
    let id = s.push_para_shape(s.para_shape(ParaShapeIndex::new(20)).unwrap().clone());
    let mut value = serde_json::to_value(&s).unwrap();
    value["list_marker_layouts"][id.get().to_string()] = serde_json::json!({"align":"right","text_start":{"kind":"from_anchor","value":2400},"char_shape":8});
    let mut s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    let plain_id = s.push_para_shape(HwpxParaShape::default());
    let mut d: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
    d.sections_mut()[0]
        .paragraphs
        .push(Paragraph::with_runs(vec![Run::text("plain", CharShapeIndex::new(7))], plain_id));
    let d = d.validate().unwrap();
    assert_eq!(losses(&encode(&d, &s), "list_marker_conflict"), 0);
}

// 이것을 실패시키는 것: 중첩 표 사용처를 빼거나 첫 head 대신 기존 head로 복귀.
#[test]
fn list_marker_conflict_includes_nested_table_uses() {
    let (d, mut s) = fixture(&[(1800, Some(fixed(1800))), (2400, Some(fixed(2400)))], None, false);
    let mut plain = s.para_shape(ParaShapeIndex::new(20)).unwrap().clone();
    plain.heading_type = hwpforge_foundation::HeadingType::None;
    let id = s.push_para_shape(plain);
    let body = d.sections()[0].paragraphs[0].clone();
    let cell = d.sections()[0].paragraphs[1].clone();
    let inner = Table::new(vec![TableRow::new(vec![TableCell::new(vec![cell], u(10000))])]);
    let inner_holder = Paragraph::with_runs(vec![Run::table(inner, CharShapeIndex::new(7))], id);
    let outer = Table::new(vec![TableRow::new(vec![TableCell::new(vec![inner_holder], u(14000))])]);
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(
        vec![body, Paragraph::with_runs(vec![Run::table(outer, CharShapeIndex::new(7))], id)],
        PageSettings::a4(),
    ));
    let out = encode(&d.validate().unwrap(), &s);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    margins(&header(&out), "1800", "3600");
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    margin_at(&header(&out), "21", "2400", "4800");
}

// 이것을 실패시키는 것: 번호 문자 상속 우선순위 반전 또는 명시적 참조에도 상속 경고.
#[test]
fn list_marker_font_context_follows_effective_references() {
    let mut m = fixed(1800);
    m.char_shape = None;
    let (d, s) = fixture(&[(1800, Some(m))], None, false);
    let mut doc: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    doc.sections_mut()[0].paragraphs[0].end_char_shape = Some(CharShapeIndex::new(8));
    metrics(
        &header(&encode(&doc.validate().unwrap(), &s)),
        false,
        ["LEFT", "0", "1", "700", "PERCENT", "0", "4294967295"],
    );
    for (character, char_ref) in [(None, "4294967295"), (Some(0), "7")] {
        let mut m = fixed(1800);
        m.char_shape = character.map(CharShapeIndex::new);
        let (d, s) = fixture(&[(1800, Some(m))], None, false);
        let mut doc: Document<hwpforge_core::Draft> =
            serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
        doc.sections_mut()[0].paragraphs[0].add_run(Run::text("mixed", CharShapeIndex::new(8)));
        let out = encode(&doc.validate().unwrap(), &s);
        assert_eq!(losses(&out, "list_marker_font_context"), usize::from(character.is_none()));
        metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", char_ref]);
    }
    for (character, width) in [(0, "800"), (8, "400")] {
        let mut s = s.clone();
        s.set_paragraph_head_metrics(
            hwpforge_foundation::HeadingType::Number,
            2,
            1,
            hwpforge_foundation::Alignment::Left,
            true,
            true,
            0,
            50,
            false,
            character,
        )
        .unwrap();
        metrics(
            &header(&encode(&d, &s)),
            false,
            ["LEFT", "0", "1", width, "PERCENT", "0", if character == 0 { "0" } else { "8" }],
        );
    }
}
// 이것을 실패시키는 것: 잘못된 좌표 단위·문자 크기·수준을 미설정 값처럼 허용.
#[test]
fn list_marker_output_boundary_revalidates_mutable_shapes() {
    for bad in ["indent", "unit", "size", "level"] {
        let (d, mut s) = fixture(&[(1800, Some(fixed(1800)))], None, false);
        match bad {
            "indent" => {
                s.para_shape_mut(ParaShapeIndex::new(20)).unwrap().indent =
                    hwpforge_core::ParagraphMeasure::from_hwp_unit(u(1))
            }
            "unit" => {
                s.para_shape_mut(ParaShapeIndex::new(20)).unwrap().margin_left =
                    hwpforge_core::ParagraphMeasure::from_char_count(
                        hwpforge_core::CharCount::from_ticks(1),
                    )
            }
            "size" => s.char_shape_mut(CharShapeIndex::new(7)).unwrap().height = u(0),
            "level" => s.para_shape_mut(ParaShapeIndex::new(20)).unwrap().heading_level = 9,
            _ => unreachable!(),
        }
        assert!(HwpxEncoder::encode(&d, &s, &ImageStore::new()).is_err(), "{bad}");
    }
}

// 이것을 실패시키는 것: 형식 기본폭을 절삭하거나 미검증 형식의 1em 대체를 무진단 처리.
#[test]
fn list_marker_base_width_rounds_half_up() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800)))], None, false);
    let mut value = serde_json::to_value(&s).unwrap();
    value["char_shapes"][7]["height"] = 1101.into();
    value["numberings"][1]["levels"][0]["text"] = "(^1)".into();
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    metrics(&header(&encode(&d, &s)), false, ["LEFT", "0", "1", "148", "PERCENT", "0", "7"]);
    let mut value = serde_json::to_value(&s).unwrap();
    value["char_shapes"][7]["height"] = 1100.into();
    value["numberings"][1]["levels"][0]["num_format"] =
        serde_json::to_value(NumberFormatType::RomanCapital).unwrap();
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    let out = encode(&d, &s);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    assert_eq!(losses(&out, "list_marker_base_width"), 1);
}

// 이것을 실패시키는 것: 요청이 없는 별도 정의까지 새 metrics writer로 정규화.
#[test]
fn list_marker_unrequested_groups_keep_raw_fidelity() {
    let (d, mut s) = fixture(&[(1800, Some(fixed(1800))), (1800, None)], None, false);
    let mut other = s.iter_numberings().nth(1).unwrap().clone();
    other.id = 100;
    s.push_numbering(other);
    s.para_shape_mut(ParaShapeIndex::new(21)).unwrap().heading_id_ref = 3;
    let mut value = serde_json::to_value(&s).unwrap();
    value["header_fidelity"]["numbering"]["3"] = serde_json::json!({"1":{"align":"UNRECOGNIZED","use_inst_width":1,"auto_indent":0,"width_adjust":-17,"text_offset_type":"HWPUNIT","text_offset":23,"char_pr_id_ref":4294967295_u32}});
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    let h = header(&encode(&d, &s));
    let n = h.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("3")).unwrap();
    let head = n.child("hh:paraHead");
    assert_eq!(head.attr("align"), Some("UNRECOGNIZED"));
    assert_eq!(head.attr("widthAdjust"), Some("-17"));
    assert_eq!(head.attr("autoIndent"), Some("0"));
}
// 이것을 실패시키는 것: head가 같아도 미설정 사용처의 이동 0을 충돌 비교에서 제외.
#[test]
fn list_marker_unspecified_matching_head_compares_shift() {
    let (d, mut s) = fixture(
        &[
            (
                1800,
                Some(layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(1800)), Some(0))),
            ),
            (1800, None),
        ],
        None,
        false,
    );
    s.set_paragraph_head_metrics(
        hwpforge_foundation::HeadingType::Number,
        2,
        1,
        hwpforge_foundation::Alignment::Right,
        false,
        true,
        700,
        1800,
        true,
        7,
    )
    .unwrap();
    let out = encode(&d, &s);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    let h = header(&out);
    margins(&h, "0", "0");
    let p = h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("21")).unwrap();
    let sw = p.child("hp:switch");
    assert_eq!(sw.child("hp:case").child("hh:margin").child("hc:left").attr("value"), Some("0"));
    assert_eq!(sw.child("hp:default").child("hh:margin").child("hc:left").attr("value"), Some("0"));
}

// 이것을 실패시키는 것: 원본 그대로 반환하는 분기 또는 재인코드 분기에서 준비 경고 폐기.
#[test]
fn marker_warnings_survive_unchanged_and_changed_edit_bases() {
    for (start, unchanged) in
        [(ListTextStart::FromAnchor(u(1800)), false), (ListTextStart::AfterMarker(u(306)), true)]
    {
        let (d, s) =
            fixture(&[(1800, Some(layout(ListMarkerAlign::Left, start, Some(0))))], None, false);
        let base = encode(&d, &s);
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&base.bytes).unwrap();
        let mut value = serde_json::to_value(&decoded.style_store).unwrap();
        value["list_marker_layouts"] = serde_json::json!({"20":{"align":"right","text_start":{"kind":"after_marker","value":306},"char_shape":7}});
        let store: HwpxStyleStore = serde_json::from_value(value).unwrap();
        let out = HwpxEncoder::encode_with_base(
            &base.bytes,
            &decoded.document.validate().unwrap(),
            &store,
            &decoded.image_store,
            EncodeOptions::default(),
        )
        .unwrap();
        assert_eq!(losses(&out, "list_marker_alignment"), 1);
        assert_eq!(out.bytes == base.bytes, unchanged);
    }
}

// 이것을 실패시키는 것: 글머리 저장 수준을 실제 writer 대신 다시 계산하여 기존 0 수준을 거부.
#[test]
fn list_marker_bullet_uses_the_existing_wire_level() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800)))], Some("●"), false);
    let mut value = serde_json::to_value(s).unwrap();
    value["bullets"][0]["para_head"]["level"] = 0.into();
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    metrics(&header(&encode(&d, &s)), true, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
}

// 이것을 실패시키는 것: 같은 문자 모양을 가리키는 서로 다른 참조도 모호한 상속으로 판정.
#[test]
fn identical_character_aliases_do_not_lose_marker_context() {
    let mut m = fixed(1800);
    m.char_shape = None;
    let (d, mut s) = fixture(&[(1800, Some(m))], None, false);
    let same = s.char_shape(CharShapeIndex::new(7)).unwrap().clone();
    *s.char_shape_mut(CharShapeIndex::new(8)).unwrap() = same;
    let mut doc: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
    doc.sections_mut()[0].paragraphs[0].add_run(Run::text("same", CharShapeIndex::new(8)));
    assert_eq!(losses(&encode(&doc.validate().unwrap(), &s), "list_marker_font_context"), 0);
}

// 이것을 실패시키는 것: 미설정 목록의 보존된 autoIndent=0·charPr=0을 기본값으로 덮기.
#[test]
fn legacy_carried_list_metrics_are_byte_stable() {
    use sha2::Digest;
    let (d, s) = fixture(&[(1800, None), (1800, None)], None, false);
    let mut value = serde_json::to_value(s).unwrap();
    value["numberings"][1]["levels"].as_array_mut().unwrap().push(
        serde_json::json!({"start":1,"level":8,"num_format":"Digit","text":"","checkable":false}),
    );
    value["para_shapes"][21]["heading_level"] = 7.into();
    let mut s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    s.set_paragraph_head_metrics(
        hwpforge_foundation::HeadingType::Number,
        2,
        8,
        hwpforge_foundation::Alignment::Left,
        false,
        false,
        0,
        0,
        false,
        0,
    )
    .unwrap();
    let out = encode(&d, &s);
    let h = header(&out);
    let n = h.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("2")).unwrap();
    let head =
        n.descendants("hh:paraHead").into_iter().find(|p| p.attr("level") == Some("8")).unwrap();
    assert_eq!(head.attr("useInstWidth"), Some("0"));
    assert_eq!(head.attr("autoIndent"), Some("0"));
    assert_eq!(head.attr("textOffsetType"), Some("PERCENT"));
    assert_eq!(head.attr("textOffset"), Some("0"));
    assert_eq!(head.attr("charPrIDRef"), Some("0"));
    assert_eq!(
        sha2::Sha256::digest(&out.bytes).iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "b6cf07fe279255c8be767c3fee2f4010f27cd73af16fc3763d183bf9224c9719",
        "XML part differences: {}",
        package::xml_part_differences(
            &out.bytes,
            &[
                ("Contents/header.xml", 4334184830282841434),
                ("Contents/section0.xml", 2268140883848037865),
                ("META-INF/container.xml", 4559238386486593401),
                ("META-INF/manifest.xml", 15859762740291687696),
                ("settings.xml", 14377881348694768885),
                ("version.xml", 14611401900661807598),
            ]
        ),
    );
}

// 이것을 실패시키는 것: 문단이 참조한 수준 대신 번호 정의의 첫 head에 metrics를 기록.
#[test]
fn numbered_marker_requests_select_the_referenced_head() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800)))], None, false);
    let mut value = serde_json::to_value(s).unwrap();
    value["numberings"][1]["levels"].as_array_mut().unwrap().push(serde_json::json!({"start":1,"level":2,"num_format":"Digit","text":"^2.","checkable":false}));
    value["para_shapes"][20]["heading_level"] = 1.into();
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    let h = header(&encode(&d, &s));
    let n = h.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("2")).unwrap();
    let second =
        n.descendants("hh:paraHead").into_iter().find(|h| h.attr("level") == Some("2")).unwrap();
    assert_eq!(second.attr("useInstWidth"), Some("0"));
    assert_eq!(second.attr("widthAdjust"), Some("700"));
    assert_eq!(second.attr("charPrIDRef"), Some("7"));
    let first =
        n.descendants("hh:paraHead").into_iter().find(|h| h.attr("level") == Some("1")).unwrap();
    assert_eq!(first.attr("useInstWidth"), Some("1"));
    assert_eq!(first.attr("textOffset"), Some("50"));
    assert_eq!(first.attr("charPrIDRef"), Some("4294967295"));
}

// 이것을 실패시키는 것: 같은 head의 문단마다 근사 경고를 내거나 충돌에서 선택 후보 경고 제거.
#[test]
fn marker_approximations_survive_selected_conflict() {
    let m = layout(ListMarkerAlign::Right, ListTextStart::AfterMarker(u(306)), None);
    let (d, mut s) = fixture(&[(1800, Some(m)); 3], None, false);
    // Roman format and mixed body runs exercise all three approximation kinds.
    let mut value = serde_json::to_value(&s).unwrap();
    value["numberings"][1]["levels"][0]["num_format"] = "RomanCapital".into();
    s = serde_json::from_value(value).unwrap();
    let mut draft: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    for p in &mut draft.sections_mut()[0].paragraphs {
        p.add_run(Run::text("mixed", CharShapeIndex::new(8)));
    }
    let d = draft.validate().unwrap();
    let out = encode(&d, &s);
    for kind in ["list_marker_alignment", "list_marker_base_width", "list_marker_font_context"] {
        assert_eq!(losses(&out, kind), 1, "{kind}");
    }
    let mut value = serde_json::to_value(&s).unwrap();
    value["list_marker_layouts"]["21"]["text_start"]["value"] = 400.into();
    let s = serde_json::from_value(value).unwrap();
    let out = encode(&d, &s);
    for kind in ["list_marker_alignment", "list_marker_base_width", "list_marker_font_context"] {
        assert_eq!(losses(&out, kind), 1, "{kind}");
    }
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    metrics(&header(&out), false, ["LEFT", "1", "1", "0", "HWPUNIT", "306", "4294967295"]);
    margin_at(&header(&out), "20", "1800", "3600");
    margin_at(&header(&out), "21", "1800", "3600");
}

// 이것을 실패시키는 것: 요청 없는 문단의 깨진 번호 수준도 새 요청과 함께 거부.
#[test]
fn unrequested_broken_level_does_not_block_other_marker_requests() {
    let (d, s) = fixture(&[(1800, Some(fixed(1800))), (1800, None)], None, false);
    let mut value = serde_json::to_value(&s).unwrap();
    value["para_shapes"][21]["heading_level"] = 8.into();
    let s = serde_json::from_value(value).unwrap();
    let out = encode(&d, &s);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    assert_eq!(losses(&out, "list_marker_conflict"), 0);
    let h = header(&out);
    let para = h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("21")).unwrap();
    assert_eq!(para.child("hh:heading").attr("level"), Some("8"));
}

fn margin_at(h: &xml::Element, id: &str, left: &str, default: &str) {
    let p = h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some(id)).unwrap();
    let switch = p.child("hp:switch");
    for (branch, value) in [("hp:case", left), ("hp:default", default)] {
        let margin = switch.child(branch).child("hh:margin");
        assert_eq!(margin.child("hc:left").attr("value"), Some(value));
        assert_eq!(margin.child("hc:intent").attr("value"), Some("0"));
    }
}
// 이것을 실패시키는 것: 뒤 항목에 자신의 −g 적용 또는 첫 절대 margin 복사.
#[test]
fn list_marker_conflict_reuses_first_margin_shift() {
    let right = |g| layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(g)), Some(0));
    let (d, s) = fixture(&[(1800, Some(right(1800))), (2400, Some(right(2400)))], None, false);
    let out = encode(&d, &s);
    let h = header(&out);
    metrics(&h, false, ["RIGHT", "0", "1", "700", "HWPUNIT", "1800", "7"]);
    margin_at(&h, "20", "0", "0");
    margin_at(&h, "21", "600", "1200");
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
    // Repeated uses of one shape must not accumulate the chosen shift.
    let mut draft: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    let p = draft.sections()[0].paragraphs[1].clone();
    draft.sections_mut()[0].paragraphs.push(p);
    margin_at(&header(&encode(&draft.validate().unwrap(), &s)), "21", "600", "1200");
}
// 이것을 실패시키는 것: head 일곱 속성만 비교하고 상대 margin 이동량 제외.
#[test]
fn list_marker_conflict_compares_margin_shift() {
    let center = |g| layout(ListMarkerAlign::Center, ListTextStart::FromAnchor(u(g)), None);
    let (d, mut s) =
        fixture(&[(1800, Some(center(1000))), (2400, Some(center(1200)))], None, false);
    s.char_shape_mut(CharShapeIndex::new(8)).unwrap().height = u(1500);
    let mut draft: Document<hwpforge_core::Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    draft.sections_mut()[0].paragraphs[1].runs[0].char_shape_id = CharShapeIndex::new(8);
    let out = encode(&draft.validate().unwrap(), &s);
    let h = header(&out);
    metrics(&h, false, ["CENTER", "0", "1", "900", "HWPUNIT", "0", "4294967295"]);
    margin_at(&h, "20", "800", "1600");
    margin_at(&h, "21", "1400", "2800");
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
}
// 이것을 실패시키는 것: None 사용처 제외 또는 첫 요청만 선택.
#[test]
fn list_marker_unspecified_first_use_is_effective_candidate() {
    for unspecified_first in [true, false] {
        let right = layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(1800)), Some(0));
        let requests = if unspecified_first {
            [(1800, None), (2400, Some(right))]
        } else {
            [(1800, Some(right)), (2400, None)]
        };
        let (d, s) = fixture(&requests, None, false);
        let out = encode(&d, &s);
        let h = header(&out);
        metrics(
            &h,
            false,
            if unspecified_first {
                ["LEFT", "1", "1", "0", "PERCENT", "50", "4294967295"]
            } else {
                ["RIGHT", "0", "1", "700", "HWPUNIT", "1800", "7"]
            },
        );
        if unspecified_first {
            margin_at(&h, "20", "1800", "3600");
            margin_at(&h, "21", "2400", "4800");
        } else {
            margin_at(&h, "20", "0", "0");
            margin_at(&h, "21", "600", "1200");
        }
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
    }
}
// 이것을 실패시키는 것: 등록 ID 순 선택 또는 표 자손을 다음 본문 뒤에 방문.
#[test]
fn list_marker_first_use_follows_document_walk() {
    let (d, mut s) = fixture(
        &[(1800, Some(fixed(2400))), (1800, Some(fixed(1800))), (1800, Some(fixed(3000)))],
        None,
        false,
    );
    let mut plain = s.para_shape(ParaShapeIndex::new(20)).unwrap().clone();
    plain.heading_type = hwpforge_foundation::HeadingType::None;
    let id = s.push_para_shape(plain);
    let mut ps = d.sections()[0].paragraphs.clone();
    for (p, text) in ps.iter_mut().zip(["later-body", "first-body", "cell"]) {
        p.runs[0].content = hwpforge_core::RunContent::Text(text.into());
    }
    let nested =
        Table::new(vec![TableRow::new(vec![TableCell::new(vec![ps[2].clone()], u(12000))])]);
    let nested_holder = Paragraph::with_runs(vec![Run::table(nested, CharShapeIndex::new(7))], id);
    let table =
        Table::new(vec![TableRow::new(vec![TableCell::new(vec![nested_holder], u(12000))])]);
    ps[1].add_run(Run::table(table, CharShapeIndex::new(7)));
    let mut section =
        Section::with_paragraphs(vec![ps[1].clone(), ps[0].clone()], PageSettings::a4());
    let mut header = ps[0].clone();
    header.runs[0].content = hwpforge_core::RunContent::Text("header".into());
    section.headers.push(hwpforge_core::section::HeaderFooter::new(
        vec![header],
        hwpforge_foundation::ApplyPageType::Both,
    ));
    let mut next = ps[0].clone();
    next.runs[0].content = hwpforge_core::RunContent::Text("next-section".into());
    let mut document = Document::new();
    document.add_section(section);
    document.add_section(Section::with_paragraphs(vec![next], PageSettings::a4()));
    let mut visited = vec![];
    for section in document.sections() {
        section.for_each_paragraph(|p| {
            let text = p.text_content();
            if !text.is_empty() {
                visited.push(text);
            }
        });
    }
    assert_eq!(visited, ["first-body", "cell", "later-body", "header", "next-section"]);
    let out = encode(&document.validate().unwrap(), &s);
    metrics(&self::header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);

    // With no list on the owner, the nested cell precedes the following body request.
    let nested =
        Table::new(vec![TableRow::new(vec![TableCell::new(vec![ps[1].clone()], u(12000))])]);
    let inner = Paragraph::with_runs(vec![Run::table(nested, CharShapeIndex::new(7))], id);
    let outer = Table::new(vec![TableRow::new(vec![TableCell::new(vec![inner], u(12000))])]);
    let holder = Paragraph::with_runs(vec![Run::table(outer, CharShapeIndex::new(7))], id);
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![holder, ps[0].clone()], PageSettings::a4()));
    let out = encode(&document.validate().unwrap(), &s);
    metrics(&self::header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
}
// 이것을 실패시키는 것: 탈락 후보의 근사 경고까지 합침.
#[test]
fn marker_approximations_ignore_unselected_candidates() {
    let exact = fixed(1800);
    let approximate = layout(ListMarkerAlign::Right, ListTextStart::AfterMarker(u(306)), None);
    let (d, s) = fixture(&[(1800, Some(exact)), (2400, Some(approximate))], None, false);
    let out = encode(&d, &s);
    metrics(&header(&out), false, ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
    assert_eq!(losses(&out, "list_marker_alignment"), 0);
    assert_eq!(losses(&out, "list_marker_conflict"), 1);
}
// 이것을 실패시키는 것: 사용처마다 경고, 정의 복제, bullet 수준별 가상 그룹 생성.
#[test]
fn list_marker_conflict_preserves_group_identity() {
    for bullet in [None, Some("●")] {
        let (d, mut s) = fixture(
            &[(1800, Some(fixed(1800))), (2400, Some(fixed(2400))), (3000, Some(fixed(3000)))],
            bullet,
            false,
        );
        if bullet.is_some() {
            s.para_shape_mut(ParaShapeIndex::new(21)).unwrap().heading_level = 2;
        }
        let original = serde_json::to_value(&s).unwrap();
        let out = encode(&d, &s);
        let h = header(&out);
        metrics(&h, bullet.is_some(), ["LEFT", "0", "1", "700", "PERCENT", "0", "7"]);
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
        assert_eq!(h.descendants("hh:numbering").len(), 2);
        assert_eq!(h.descendants("hh:bullet").len(), usize::from(bullet.is_some()));
        assert_eq!(serde_json::to_value(&s).unwrap(), original);
        let section = xml::Element::parse(&package::section_xml(&out.bytes));
        assert_eq!(
            section
                .descendants("hp:p")
                .iter()
                .map(|p| p.attr("paraPrIDRef").unwrap())
                .collect::<Vec<_>>(),
            ["20", "21", "22"]
        );

        if bullet.is_none() {
            let mut def = s.iter_numberings().nth(1).unwrap().clone();
            def.id = 100;
            s.push_numbering(def);
            let mut value = serde_json::to_value(&s).unwrap();
            value["para_shapes"][22]["heading_id_ref"] = 3.into();
            let shape = value["para_shapes"][22].clone();
            value["para_shapes"].as_array_mut().unwrap().push(shape);
            value["list_marker_layouts"]["23"] = serde_json::json!({"align":"left","text_start":{"kind":"from_anchor","value":3600},"char_shape":7});
            let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
            let mut draft: Document<hwpforge_core::Draft> =
                serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
            let mut p = draft.sections()[0].paragraphs[2].clone();
            p.para_shape_id = ParaShapeIndex::new(23);
            draft.sections_mut()[0].paragraphs.push(p);
            assert_eq!(losses(&encode(&draft.validate().unwrap(), &s), "list_marker_conflict"), 2);
        }
    }
}
// 이것을 실패시키는 것: 충돌을 기존값으로 복귀, 원본 store 변경, 중복 이동 또는 JSON 왕복에서 선택/경고 손실.
#[test]
fn list_marker_first_use_projection_is_repeatable() {
    let right = |g| layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(g)), Some(0));
    let (d, s) = fixture(&[(1800, Some(right(1800))), (2400, Some(right(2400)))], None, false);
    let original = serde_json::to_value(&s).unwrap();
    let a = encode(&d, &s);
    let b = encode(&d, &s);
    let restored: HwpxStyleStore = serde_json::from_value(original.clone()).unwrap();
    let c = encode(&d, &restored);
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(a.bytes, c.bytes);
    assert_eq!(a.warnings, b.warnings);
    assert_eq!(a.warnings, c.warnings);
    assert_eq!(serde_json::to_value(&s).unwrap(), original);
    let h = header(&a);
    metrics(&h, false, ["RIGHT", "0", "1", "700", "HWPUNIT", "1800", "7"]);
    margin_at(&h, "21", "600", "1200");
    assert_eq!(losses(&a, "list_marker_conflict"), 1);
    assert!(hwpforge_smithy_hwpx::HwpxEncoder::encode_with_base(
        &a.bytes,
        &d,
        &s,
        &ImageStore::new(),
        EncodeOptions::default()
    )
    .is_ok());
}
// 이것을 실패시키는 것: 뒤의 무요청 margin에 단위 환산/포화 허용.
#[test]
fn list_marker_first_shift_checks_unrequested_margins() {
    let right = layout(ListMarkerAlign::Right, ListTextStart::FromAnchor(u(1800)), Some(0));
    for bad in ["unit", "range"] {
        let (d, mut s) = fixture(&[(1800, Some(right)), (2400, None)], None, false);
        s.para_shape_mut(ParaShapeIndex::new(21)).unwrap().margin_left = if bad == "unit" {
            hwpforge_core::ParagraphMeasure::from_char_count(hwpforge_core::CharCount::from_ticks(
                1,
            ))
        } else {
            hwpforge_core::ParagraphMeasure::from_hwp_unit(u(-100_000_000))
        };
        assert!(HwpxEncoder::encode(&d, &s, &ImageStore::new()).is_err(), "{bad}");
    }
}
// 이것을 실패시키는 것: 기본 탭 g를 고정 head 기본폭으로 사용하거나 margin을 이동.
#[test]
fn word_zero_hanging_default_tab_wire() {
    for (tab, gaps, widths) in [
        (720, [3600, 1100, 4100, 3600, 3600, 3600], ["2500", "0", "3000", "2500", "2500", "2500"]),
        (800, [4400, 1500, 900, 4000, 4400, 4400], ["3300", "400", "-200", "2900", "3300", "3300"]),
    ] {
        for (i, anchor) in [3600, 2500, 3100, 0, 3600, 3600].into_iter().enumerate() {
            let (d, s) = fixture(
                &[(anchor, Some(fixed(gaps[i])))],
                if i == 5 { Some("•") } else { None },
                false,
            );
            let out = encode(&d, &s);
            let h = header(&out);
            metrics(&h, i == 5, ["LEFT", "0", "1", widths[i], "PERCENT", "0", "7"]);
            margin_at(
                &h,
                "20",
                ["3600", "2500", "3100", "0", "3600", "3600"][i],
                ["7200", "5000", "6200", "0", "7200", "7200"][i],
            );
            assert_eq!(losses(&out, "list_marker_conflict"), 0, "{tab}");
        }
    }
}
// 이것을 실패시키는 것: max 제거 → Right g=0에서 widthAdjust −1100 · margin 3600/7200,
// 또는 Center 슬롯을 g로 계산하거나 측정된 Right g=0에 근사 경고 생성.
#[test]
fn word_zero_hanging_aligned_tab_wire() {
    for (align, g, width, offset, left, default) in [
        (ListMarkerAlign::Right, 0, "0", "0", "2500", "5000"),
        (ListMarkerAlign::Center, 3600, "6100", "0", "0", "0"),
        (ListMarkerAlign::Center, 4400, "7700", "0", "-800", "-1600"),
    ] {
        let (d, s) = fixture(
            &[(3600, Some(layout(align, ListTextStart::FromAnchor(u(g)), Some(0))))],
            None,
            false,
        );
        let out = encode(&d, &s);
        let h = header(&out);
        assert_eq!(losses(&out, "list_marker_alignment"), 0);
        metrics(
            &h,
            false,
            [
                if align == ListMarkerAlign::Right { "RIGHT" } else { "CENTER" },
                "0",
                "1",
                width,
                "HWPUNIT",
                offset,
                "7",
            ],
        );
        margin_at(&h, "20", left, default);
    }
}

// 이것을 실패시키는 것: 충돌을 원래 head/margin으로 돌려 선택값과 캐시 무효화를 누락.
#[test]
fn list_marker_first_use_changes_invalidate_cache() {
    let (d, s) = fixture(&[(1800, None), (2400, None)], None, false);
    let base = encode(&d, &s);
    let base = package::splice(
        &base.bytes,
        "</hp:p>",
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1100" textheight="1100" baseline="850" spacing="600" horzpos="0" horzsize="10000" flags="393216"/></hp:linesegarray></hp:p>"#,
    );
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&base).unwrap();
    assert!(decoded.document.sections()[0].paragraphs.iter().all(|p| p.layout_cache.is_some()));
    let mut value = serde_json::to_value(&decoded.style_store).unwrap();
    value["list_marker_layouts"] = serde_json::json!({"20":{"align":"right","text_start":{"kind":"from_anchor","value":1800},"char_shape":7},"21":{"align":"right","text_start":{"kind":"from_anchor","value":2400},"char_shape":7}});
    let store = serde_json::from_value(value).unwrap();
    let doc = decoded.document.validate().unwrap();
    for out in [
        HwpxEncoder::encode_with_diagnostics(
            &doc,
            &store,
            &decoded.image_store,
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap(),
        HwpxEncoder::encode_with_base(
            &base,
            &doc,
            &store,
            &decoded.image_store,
            EncodeOptions::default(),
        )
        .unwrap(),
    ] {
        let h = header(&out);
        metrics(&h, false, ["RIGHT", "0", "1", "700", "HWPUNIT", "1800", "7"]);
        margin_at(&h, "21", "600", "1200");
        assert_eq!(losses(&out, "list_marker_conflict"), 1);
        assert!(xml::Element::parse(&package::section_xml(&out.bytes))
            .descendants("hp:linesegarray")
            .is_empty());
    }
}
