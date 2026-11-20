use hwpforge_blueprint::{
    border_fill::{Border, BorderFill, Fill},
    registry::StyleRegistry,
};
use hwpforge_core::{
    table::TableMargin, Document, Draft, ImageStore, LayoutProfile, PageSettings, Paragraph, Run,
    Section, Table, TableCell, TableRow, Validated,
};
use hwpforge_foundation::{BorderLineType, CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    EncodeOptions, EncodeOutcome, HancomStyleSet, HwpxDecoder, HwpxEncoder, HwpxRegistryBridge,
    HwpxStyleStore,
};
#[path = "support/xml.rs"]
mod xml;
fn unit(n: i32) -> HwpUnit {
    HwpUnit::new(n).unwrap()
}
fn margin(l: i32, r: i32, t: i32, b: i32) -> TableMargin {
    TableMargin { left: unit(l), right: unit(r), top: unit(t), bottom: unit(b) }
}
fn paragraph() -> Paragraph {
    Paragraph::with_runs(vec![Run::text("ASCII", CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn containing(t: Table) -> Paragraph {
    Paragraph::with_runs(vec![Run::table(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn definition(widths: [Option<i32>; 4]) -> BorderFill {
    let mut border = Border::default();
    for (side, width) in [&mut border.left, &mut border.right, &mut border.top, &mut border.bottom]
        .into_iter()
        .zip(widths)
    {
        side.line_type = BorderLineType::Solid;
        side.width = width.map(unit);
    }
    BorderFill { border, fill: Fill::default() }
}
fn registry() -> StyleRegistry {
    let mut r = StyleRegistry::with_fonts(vec![]);
    let mut cs = hwpforge_blueprint::style::PartialCharShape::default();
    cs.font = Some("Batang".into());
    cs.size = Some(unit(1000));
    r.char_shapes.push(cs.resolve("body").unwrap());
    r.para_shapes.push(hwpforge_blueprint::style::ParaShape::default());
    r
}
fn draft(t: Table) -> Document<Draft> {
    let mut d = Document::new();
    d.set_layout_profile(LayoutProfile::WordCompatible);
    d.add_section(Section::with_paragraphs(vec![containing(t)], PageSettings::a4()));
    d
}
fn rebound(t: Table, registry: &StyleRegistry) -> (Document<Draft>, HwpxStyleStore) {
    let bridge = HwpxRegistryBridge::from_registry_for_import(registry, HancomStyleSet::Modern)
        .unwrap()
        .value;
    (bridge.rebind_draft_document(draft(t)).unwrap(), bridge.into_style_store())
}
fn table(rows: usize, cols: usize, id: u32, m: TableMargin) -> Table {
    Table::new(
        (0..rows)
            .map(|_| {
                TableRow::new(
                    (0..cols).map(|_| TableCell::new(vec![paragraph()], unit(10000))).collect(),
                )
            })
            .collect(),
    )
    .with_border_fill_id(id)
    .with_in_margin(m)
}
fn encode(d: &Document<Validated>, s: &HwpxStyleStore) -> EncodeOutcome {
    HwpxEncoder::encode_with_diagnostics(d, s, &ImageStore::new(), EncodeOptions::default())
        .unwrap()
}
fn section(bytes: &[u8]) -> xml::Element {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("Contents/section0.xml").unwrap().read_to_string(&mut s).unwrap();
    xml::Element::parse(&s)
}
fn margins(bytes: &[u8]) -> Vec<[i32; 4]> {
    section(bytes)
        .descendants("hp:tc")
        .iter()
        .map(|c| {
            let m = c.child("hp:cellMargin");
            ["left", "right", "top", "bottom"].map(|k| m.attr(k).unwrap().parse().unwrap())
        })
        .collect()
}
// 이것을 실패시키는 것: 오른쪽에 이동량 가산, 내부 아래 선 중복 가산, 반폭 조기 버림, ties-away 반올림.
#[test]
fn uniform_widths_project_output_margins_without_mutating_core() {
    for (width, m, expected) in [
        (50, margin(540, 540, 0, 0), [565, 515, 50, 50]),
        (150, margin(540, 540, 200, 200), [615, 465, 350, 350]),
        (50, margin(0, 0, 0, 0), [50, 0, 50, 50]),
        (150, margin(0, 0, 200, 200), [150, 0, 350, 350]),
        (37, margin(540, 540, 0, 0), [558, 522, 37, 37]),
        (38, margin(540, 540, 0, 0), [559, 521, 38, 38]),
        (75, margin(540, 541, 0, 0), [578, 504, 75, 75]),
    ] {
        let mut r = registry();
        let id = r.register_border_fill(definition([Some(width); 4])).unwrap();
        let (d, s) = rebound(table(1, 1, id, m), &r);
        let d = d.validate().unwrap();
        let before = d.clone();
        let store = s.clone();
        let out = encode(&d, &s);
        assert_eq!(margins(&out.bytes), [expected]);
        assert_eq!(d, before);
        assert_eq!(s, store);
        assert_eq!(encode(&d, &s).bytes, out.bytes);
    }
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let (d, s) = rebound(table(3, 1, id, margin(540, 540, 0, 0)), &r);
    assert_eq!(
        margins(&encode(&d.validate().unwrap(), &s).bytes),
        [[565, 515, 50, 0], [565, 515, 50, 0], [565, 515, 50, 50]]
    );
}
// 이것을 실패시키는 것: store JSON에서 import 권한 복원 또는 일반 디코드 뒤 재가산.
#[test]
fn store_json_and_output_decode_discard_import_authority() {
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let (d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
    let d = d.validate().unwrap();
    let json = serde_json::to_value(&s).unwrap();
    let restored: HwpxStyleStore = serde_json::from_value(json).unwrap();
    assert_ne!(s, restored);
    assert_eq!(margins(&encode(&d, &restored).bytes), [[540, 540, 0, 0]]);
    let out = encode(&d, &s);
    let decoded = HwpxDecoder::decode(&out.bytes).unwrap();
    assert_eq!(
        margins(&encode(&decoded.document.validate().unwrap(), &decoded.style_store).bytes),
        [[565, 515, 50, 50]]
    );
}
// 이것을 실패시키는 것: 마지막 행 최초값 고정, 행별 이동량 사용, row_span 대신 앵커 행으로 아래 판단.
#[test]
fn edited_rows_and_spans_use_current_structure() {
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let (mut d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
    d.for_each_table_mut(|t| t.rows.push(t.rows[0].clone()));
    assert_eq!(
        margins(&encode(&d.validate().unwrap(), &s).bytes),
        [[565, 515, 50, 0], [565, 515, 50, 50]]
    );
    let mut t = table(2, 2, id, margin(540, 540, 0, 0));
    t.rows[0].cells[0].row_span = 2;
    t.rows[1].cells.remove(0);
    let (d, s) = rebound(t, &r);
    assert_eq!(
        margins(&encode(&d.validate().unwrap(), &s).bytes),
        [[565, 515, 50, 50], [565, 515, 50, 0], [565, 515, 50, 50]]
    );
}
// 이것을 실패시키는 것: 칸별 네 면 NONE이면 투영에서 빼기, 안쪽 또는 오른쪽 최대 폭을 표 이동량으로 사용.
#[test]
fn mixed_faces_use_first_left_border_for_the_whole_table() {
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50), Some(150), Some(50), Some(50)])).unwrap();
    let (d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
    assert_eq!(margins(&encode(&d.validate().unwrap(), &s).bytes), [[565, 515, 50, 50]]);
}
fn reasons(out: &EncodeOutcome) -> Vec<(&'static str, String, u64)> {
    out.warnings
        .iter()
        .filter_map(|w| match w {
            hwpforge_smithy_hwpx::EncodeWarning::TableBorderSpaceApproximated {
                path,
                reason,
                count,
            } => Some((*reason, path.to_string(), *count)),
            _ => None,
        })
        .collect()
}
fn face_definition(widths: [i32; 4]) -> BorderFill {
    let mut d = definition(widths.map(Some));
    for side in [&mut d.border.left, &mut d.border.right, &mut d.border.top, &mut d.border.bottom] {
        if side.width == Some(unit(0)) {
            side.line_type = BorderLineType::None;
            side.width = None;
        }
    }
    d
}
fn patterned(rows: usize, cols: usize, outer: i32, inner: i32, r: &mut StyleRegistry) -> Table {
    let mut t = table(rows, cols, 1, margin(540, 540, 0, 0));
    for (ri, row) in t.rows.iter_mut().enumerate() {
        for (ci, c) in row.cells.iter_mut().enumerate() {
            c.border_fill_id = Some(
                r.register_border_fill(face_definition([
                    if ci == 0 { outer } else { inner },
                    if ci + 1 == cols { outer } else { inner },
                    if ri == 0 { outer } else { inner },
                    if ri + 1 == rows { outer } else { inner },
                ]))
                .unwrap(),
            );
        }
    }
    t.border_fill_id = Some(r.register_border_fill(face_definition([outer; 4])).unwrap());
    t
}
// 이것을 실패시키는 것: NONE 칸 제외, 행별 이동량, 최대 안쪽 폭 이동량, 병합 외곽 판단 오류.
#[test]
fn outer_only_and_mixed_faces_project_every_physical_slot() {
    for (rows, cols, outer, inner, expected) in [
        (
            2,
            2,
            50,
            0,
            vec![[565, 515, 50, 0], [565, 515, 50, 0], [565, 515, 0, 50], [565, 515, 0, 50]],
        ),
        (
            2,
            2,
            150,
            50,
            vec![[615, 465, 150, 0], [615, 465, 150, 0], [615, 465, 50, 150], [615, 465, 50, 150]],
        ),
        (
            2,
            2,
            50,
            150,
            vec![[565, 515, 50, 0], [565, 515, 50, 0], [565, 515, 150, 50], [565, 515, 150, 50]],
        ),
    ] {
        let mut r = registry();
        let t = patterned(rows, cols, outer, inner, &mut r);
        let (d, s) = rebound(t, &r);
        let out = encode(&d.validate().unwrap(), &s);
        assert_eq!(margins(&out.bytes), expected);
        assert!(reasons(&out).is_empty());
    }
    let mut r = registry();
    let t = patterned(3, 3, 50, 0, &mut r);
    let (d, s) = rebound(t, &r);
    let out = encode(&d.validate().unwrap(), &s);
    assert_eq!(margins(&out.bytes).len(), 9);
    assert_eq!(margins(&out.bytes)[4], [565, 515, 0, 0]);
    let mut t = patterned(1, 2, 50, 0, &mut r);
    t.rows[0].cells.truncate(1);
    t.rows[0].cells[0].col_span = 2;
    t.rows[0].cells[0].border_fill_id = t.border_fill_id;
    let (d, s) = rebound(t, &r);
    let out = encode(&d.validate().unwrap(), &s);
    assert_eq!(margins(&out.bytes), [[565, 515, 50, 50]]);
    assert_eq!(section(&out.bytes).descendants("hp:cellSpan")[0].attr("colSpan"), Some("2"));
}
// 이것을 실패시키는 것: 현재 보류 가드 제거, Solid 미기재를 0으로 처리, 우선순위 뒤집기.
#[test]
fn current_candidate_failures_retain_margins_with_one_reason() {
    use hwpforge_core::table::CellZone;
    for case in 0..9 {
        let mut r = registry();
        let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
        let mut t = table(1, 1, id, margin(540, 540, 0, 0));
        let reason = match case {
            0 => {
                t.frame.size_protect = true;
                "size_protected"
            }
            1 => {
                t.cell_spacing = Some(unit(100));
                "nonzero_cell_spacing"
            }
            2 => "cell_zones",
            3 => {
                t.border_fill_id = Some(
                    r.register_border_fill(definition([Some(50), None, Some(50), Some(50)]))
                        .unwrap(),
                );
                "incomplete_widths"
            }
            4 => {
                t.in_margin = None;
                t.rows[0].cells[0].dormant_margin = Some(margin(540, 540, 0, 0));
                "missing_margins"
            }
            5 => {
                t.border_fill_id = Some(
                    r.register_border_fill(definition([Some(75), Some(150), Some(75), Some(75)]))
                        .unwrap(),
                );
                "unverified_mixed_widths"
            }
            6 => {
                t.rows.push(t.rows[0].clone());
                t.rows[1].cells[0].border_fill_id = Some(
                    r.register_border_fill(definition([Some(150), Some(50), Some(50), Some(50)]))
                        .unwrap(),
                );
                "later_left_wider"
            }
            7 => {
                t.border_fill_id = Some(
                    r.register_border_fill(definition([Some(150), Some(50), Some(50), Some(50)]))
                        .unwrap(),
                );
                t.in_margin = Some(margin(0, 0, 0, 0));
                "negative_right_margin"
            }
            _ => {
                let copied = t.rows[0].cells[0].clone();
                t.rows[0].cells.push(copied);
                t.rows[0].cells[1].border_fill_id = Some(
                    r.register_border_fill(definition([Some(150), Some(50), Some(50), Some(50)]))
                        .unwrap(),
                );
                "inconsistent_edges"
            }
        };
        let (mut d, s) = rebound(t, &r);
        if case == 2 {
            d.for_each_table_mut(|t| {
                t.cell_zones.push(CellZone::new(0, 0, 0, 0, t.border_fill_id.unwrap()))
            });
        }
        let out = encode(&d.validate().unwrap(), &s);
        assert_eq!(reasons(&out), [(reason, "section[0].para[0].run[0]".into(), 1)], "case {case}");
        assert_eq!(margins(&out.bytes)[0], if case == 7 { [0, 0, 0, 0] } else { [540, 540, 0, 0] });
    }
}
// 이것을 실패시키는 것: run 키 생략, 바깥 보류로 안쪽 생략, 보조 컨테이너 ancestry 무시.
#[test]
fn nested_and_auxiliary_tables_share_emission_keys_without_changing_old_paths() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        control::Control,
        section::{HeaderFooter, MasterPage},
    };
    use hwpforge_foundation::ApplyPageType;
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let child = table(1, 1, id, margin(540, 540, 0, 0));
    let mut outer = child.clone();
    outer.frame.size_protect = true;
    outer.rows[0].cells[0].paragraphs = vec![containing(child.clone())];
    let mut host = containing(outer);
    host.runs.insert(0, Run::text("lead", CharShapeIndex::new(0)));
    host.runs.push(Run::control(
        Control::Footnote { inst_id: None, paragraphs: vec![containing(child.clone())] },
        CharShapeIndex::new(0),
    ));
    let mut captable = child.clone();
    captable.caption = Some(Caption::new(vec![containing(child.clone())], CaptionSide::Bottom));
    host.runs.push(Run::table(captable, CharShapeIndex::new(0)));
    let mut d = draft(child.clone());
    d.sections_mut()[0].paragraphs = vec![host];
    let mut auxiliary = child.clone();
    auxiliary.rows[0].cells[0].paragraphs = vec![containing(child.clone())];
    d.sections_mut()[0]
        .headers
        .push(HeaderFooter::new(vec![containing(auxiliary)], ApplyPageType::Both));
    d.sections_mut()[0]
        .footers
        .push(HeaderFooter::new(vec![containing(child.clone())], ApplyPageType::Both));
    d.sections_mut()[0].master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![containing(child)])]);
    let b = HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).unwrap().value;
    let d = b.rebind_draft_document(d).unwrap().validate().unwrap();
    let s = b.into_style_store();
    let out = encode(&d, &s);
    let rs = reasons(&out);
    assert_eq!(
        rs,
        [
            ("size_protected", "section[0].para[0].run[1]".into(), 1),
            ("outside_scope", "section[0].para[0].run[2].footnote.npara[0].run[0]".into(), 6)
        ]
    );
    assert!(margins(&out.bytes).contains(&[565, 515, 50, 50]));
    let mut legacy: Document<Draft> =
        serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    legacy.set_layout_profile(LayoutProfile::Hancom);
    assert_eq!(reasons(&encode(&legacy.validate().unwrap(), &s))[0].0, "outside_scope");
}
// 이것을 실패시키는 것: 현재 필드 대신 과거 구조 사용, 셀 override 무시, 무관 배치/폭/맞춤을 산식에 섞기.
#[test]
fn current_fields_and_explicit_zero_spacing_preserve_the_local_formula() {
    use hwpforge_core::{
        placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap},
        table::TableVerticalAlign,
    };
    for spacing in [None, Some(unit(0))] {
        for alignment in
            [TableVerticalAlign::Top, TableVerticalAlign::Center, TableVerticalAlign::Bottom]
        {
            let mut r = registry();
            let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
            let mut t = table(3, 1, id, margin(540, 540, 0, 0));
            t.cell_spacing = spacing;
            t.width = Some(unit(25000));
            t.placement = Some(
                ObjectPlacement::new(
                    ObjectTextWrap::TopAndBottom,
                    ObjectRelativeTo::Para,
                    ObjectRelativeTo::Column,
                )
                .with_horz_offset(unit(123)),
            );
            for row in &mut t.rows {
                row.height = Some(unit(3000));
                row.cells[0].vertical_align = Some(alignment);
            }
            t.rows[1].cells[0].margin = Some(margin(540, 540, 200, 300));
            let (d, s) = rebound(t, &r);
            let out = encode(&d.validate().unwrap(), &s);
            assert_eq!(
                margins(&out.bytes),
                [[565, 515, 50, 0], [565, 515, 250, 300], [565, 515, 50, 50]]
            );
            assert!(reasons(&out).is_empty());
            assert_eq!(
                section(&out.bytes).descendants("hp:cellSz")[1].attr("height"),
                Some("3000")
            );
        }
    }
}
// 이것을 실패시키는 것: JSON에서 import 상태 복구, base 경로에서 상태를 남김, 반복 재가산.
#[test]
fn base_encoding_and_forged_store_json_ignore_import_state() {
    for profile in [LayoutProfile::WordCompatible, LayoutProfile::Hancom] {
        let mut r = registry();
        let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
        let (mut d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
        d.set_layout_profile(profile);
        let mut json = serde_json::to_value(&s).unwrap();
        json["imported_border_widths"] = serde_json::json!({"4":[50,50,50,50]});
        let plain: HwpxStyleStore = serde_json::from_value(json).unwrap();
        let valid = d.clone().validate().unwrap();
        let base = encode(&valid, &plain);
        for edited in [false, true] {
            let mut d = d.clone();
            if edited {
                d.for_each_table_mut(|t| {
                    t.rows[0].cells[0].paragraphs[0].runs[0].content =
                        hwpforge_core::RunContent::Text("edited".into())
                });
            }
            let d = d.validate().unwrap();
            let before = s.clone();
            let a = HwpxEncoder::encode_with_base(
                &base.bytes,
                &d,
                &s,
                &ImageStore::new(),
                EncodeOptions::default(),
            )
            .unwrap();
            let b = HwpxEncoder::encode_with_base(
                &base.bytes,
                &d,
                &plain,
                &ImageStore::new(),
                EncodeOptions::default(),
            )
            .unwrap();
            assert_eq!(a.bytes, b.bytes);
            assert_eq!(a.warnings, b.warnings);
            assert_eq!(s, before);
            assert_eq!(margins(&a.bytes), [[540, 540, 0, 0]]);
        }
    }
}
// 이것을 실패시키는 것: 빈/미요청 캐시에 경고, 두 보정에 중복 경고, 대체 lineseg 생성.
#[test]
fn cache_suppression_reports_only_requested_nonempty_caches_once() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_smithy_hwpx::EncodeWarning;
    for latin in [false, true] {
        for cache in [0, 1, 2] {
            for requested in [false, true] {
                let mut r = registry();
                if latin {
                    r.char_shapes[0].font_latin = Some("Cambria".into());
                }
                let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
                let mut t = table(1, 1, id, margin(540, 540, 0, 0));
                t.rows[0].cells[0].paragraphs[0].layout_cache = match cache {
                    0 => None,
                    1 => Some(LayoutCache::new(vec![])),
                    _ => Some(LayoutCache::new(vec![LineSeg::default()])),
                };
                let (d, s) = rebound(t, &r);
                let out = HwpxEncoder::encode_with_diagnostics(
                    &d.validate().unwrap(),
                    &s,
                    &ImageStore::new(),
                    EncodeOptions::default().with_emit_layout_cache(requested),
                )
                .unwrap();
                assert!(section(&out.bytes).descendants("hp:linesegarray").is_empty());
                let paths: Vec<_> = out
                    .warnings
                    .iter()
                    .filter_map(|w| match w {
                        EncodeWarning::LayoutCacheDropped { path, .. } => Some(path.to_string()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    paths,
                    if requested && cache == 2 {
                        vec!["section[0].para[0].run[0].cell[0][0].npara[0]".to_string()]
                    } else {
                        vec![]
                    }
                );
            }
        }
    }
}
// 이것을 실패시키는 것: 아래 행 최대 왼쪽 폭으로 표 이동량 대체, 음수 여백 clamp, 가드 순서 역전.
#[test]
fn mixed_left_arrangements_use_the_first_owner_and_recompute_after_reordering() {
    let mut r = registry();
    let a = r.register_border_fill(face_definition([150, 50, 50, 50])).unwrap();
    let b = r.register_border_fill(face_definition([50; 4])).unwrap();
    let mut t = table(2, 2, b, margin(540, 540, 0, 0));
    t.rows[0].cells[0].border_fill_id = Some(a);
    let (mut d, s) = rebound(t, &r);
    assert_eq!(
        margins(&encode(&d.clone().validate().unwrap(), &s).bytes),
        [[615, 465, 50, 0], [615, 465, 50, 0], [615, 465, 50, 50], [615, 465, 50, 50]]
    );
    d.for_each_table_mut(|t| t.rows.swap(0, 1));
    let out = encode(&d.clone().validate().unwrap(), &s);
    assert_eq!(reasons(&out)[0].0, "later_left_wider");
    d.for_each_table_mut(|t| {
        t.frame.size_protect = true;
        t.cell_spacing = Some(unit(100));
    });
    assert_eq!(reasons(&encode(&d.validate().unwrap(), &s))[0].0, "size_protected");
}
// 이것을 실패시키는 것: 유효 zones를 적용하거나 재대응 제한/음수/불법 참조를 보류로 낮춤.
#[test]
fn zones_spacing_and_reference_errors_keep_validation_precedence() {
    use hwpforge_core::table::CellZone;
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let mut t = table(1, 1, id, margin(540, 540, 0, 0));
    t.cell_zones.push(CellZone::new(0, 0, 0, 0, id));
    let bridge =
        HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).unwrap().value;
    assert!(bridge.rebind_draft_document(draft(t)).is_err());
    for spacing in [None, Some(unit(0)), Some(unit(100)), Some(unit(-1))] {
        let (mut d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
        d.for_each_table_mut(|t| t.cell_spacing = spacing);
        let valid = d.validate();
        if spacing == Some(unit(-1)) {
            assert!(HwpxEncoder::encode(&valid.unwrap(), &s, &ImageStore::new()).is_err());
            continue;
        }
        let out = encode(&valid.unwrap(), &s);
        assert_eq!(
            margins(&out.bytes),
            if spacing == Some(unit(100)) {
                vec![[540, 540, 0, 0]]
            } else {
                vec![[565, 515, 50, 50]]
            }
        );
    }
    let (mut d, s) = rebound(table(1, 1, id, margin(540, 540, 0, 0)), &r);
    d.for_each_table_mut(|t| {
        t.cell_zones.push(CellZone::new(0, 0, 0, 0, t.border_fill_id.unwrap()))
    });
    let out = encode(&d.clone().validate().unwrap(), &s);
    assert_eq!(reasons(&out)[0].0, "cell_zones");
    d.for_each_table_mut(|t| t.cell_zones.clear());
    assert_eq!(margins(&encode(&d.clone().validate().unwrap(), &s).bytes), [[565, 515, 50, 50]]);
    d.for_each_table_mut(|t| t.border_fill_id = Some(9999));
    let valid = d.validate().unwrap();
    let before = s.clone();
    assert!(HwpxEncoder::encode(&valid, &s, &ImageStore::new()).is_err());
    assert_eq!(s, before);
}
// 이것을 실패시키는 것: 사유별 문장을 식별자로 대체, advisory에 retain 접미사, 미지 사유 panic.
#[test]
fn warning_messages_use_complete_sentences_and_stable_suffixes() {
    use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};
    for (reason,first) in [
        ("outside_scope","Table border space was not projected because the table is outside WordCompatible body or body-cell scope."),
        ("size_protected","Table border space was not projected because the table size is protected."),
        ("nonzero_cell_spacing","Table border space was not projected because the table has nonzero cell spacing."),
        ("cell_zones","Table border space was not projected because the table has cell-zone border definitions."),
        ("incomplete_widths","Table border space was not projected because some effective border widths are unknown."),
        ("inconsistent_edges","Table border space was not projected because adjoining border widths are inconsistent."),
        ("missing_margins","Table border space was not projected because effective source cell margins are unavailable."),
        ("unverified_mixed_widths","Table border space was not projected because the mixed border widths are outside the measured set."),
        ("later_left_wider","Table border space was not projected because a later left border is wider than the first cell's left border."),
        ("negative_right_margin","Table border space was not projected because it would make a right cell margin negative."),
        ("unverified_uniform_width","Table border space was projected using a uniform width outside the measured set."),
        ("unknown","Table border space could not be projected for an unspecified reason."),
    ] {for count in [1,2] {
        let w=EncodeWarning::TableBorderSpaceApproximated {path:ParagraphPath(vec![PathSeg::Section(0),PathSeg::BodyParagraph(2),PathSeg::Run(1)]),reason,count};
        let retained=if reason=="unverified_uniform_width" {""}else{" Existing cell margins were retained."};
        let expected=format!("{first}{retained} Affected tables: {count}. Only the first location is shown.");
        assert_eq!(w.message(),expected);assert_eq!(w.to_string(),format!("section[0].para[2].run[1]: {expected}"));assert_eq!(w.code(),"TABLE_BORDER_SPACE_APPROXIMATED");assert!(!w.is_semantic_loss());
    }}
}

// 이것을 실패시키는 것: 실제 여백 변화 대신 후보 존재만으로 cache 억제, 비import 경로에 권한 생성.
#[test]
fn unchanged_candidate_and_general_bridge_keep_existing_caches_and_margins() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    let mut r = registry();
    let id = r.register_border_fill(face_definition([0, 50, 0, 0])).unwrap();
    let mut t = table(1, 1, id, margin(540, 540, 0, 0));
    let mut line = LineSeg::default();
    line.vertsize = 1000;
    line.textheight = 1000;
    line.baseline = 850;
    line.horzsize = 10000;
    t.rows[0].cells[0].paragraphs[0].layout_cache = Some(LayoutCache::new(vec![line]));
    let (d, s) = rebound(t, &r);
    let out = HwpxEncoder::encode_with_diagnostics(
        &d.validate().unwrap(),
        &s,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert_eq!(margins(&out.bytes), [[540, 540, 0, 0]]);
    assert_eq!(section(&out.bytes).descendants("hp:linesegarray").len(), 1);
    assert!(out.warnings.iter().all(|w| w.code() != "LAYOUT_CACHE_DROPPED"));
    let mut t = table(1, 1, 3, margin(540, 540, 0, 0));
    t.rows[0].cells[0].margin = Some(margin(540, 540, 0, 0));
    let bridge = HwpxRegistryBridge::from_registry_with_diagnostics(
        &registry(),
        HancomStyleSet::Modern,
        hwpforge_smithy_hwpx::ListHeadPolicy::Template,
    )
    .unwrap()
    .value;
    let d = bridge.rebind_draft_document(draft(t)).unwrap().validate().unwrap();
    let out = encode(&d, bridge.style_store());
    assert_eq!(margins(&out.bytes), [[540, 540, 0, 0]]);
    assert!(reasons(&out).is_empty());
}

// 이것을 실패시키는 것: 표 선언/안쪽/오른쪽 최대 폭이나 행별 폭으로 이동량을 고름, 음수 clamp.
#[test]
fn measured_mixed_arrangements_use_one_shift_for_all_cells() {
    for (left, later, inside, right, horizontal, m, expected, reason) in [
        (50, 50, 150, 50, 150, 540, [565, 515], None),
        (150, 150, 50, 50, 50, 540, [615, 465], None),
        (50, 50, 50, 150, 50, 540, [565, 515], None),
        (50, 50, 150, 50, 50, 540, [565, 515], None),
        (150, 50, 50, 50, 50, 540, [615, 465], None),
        (150, 150, 50, 50, 50, 0, [0, 0], Some("negative_right_margin")),
    ] {
        let mut r = registry();
        let mut t = table(2, 2, 1, margin(m, m, 0, 0));
        for (ri, row) in t.rows.iter_mut().enumerate() {
            for (ci, c) in row.cells.iter_mut().enumerate() {
                let l = if ci == 0 {
                    if ri == 0 {
                        left
                    } else {
                        later
                    }
                } else {
                    inside
                };
                let rr = if ci == 1 { right } else { inside };
                c.border_fill_id = Some(
                    r.register_border_fill(face_definition([
                        l,
                        rr,
                        if ri == 0 { 50 } else { horizontal },
                        if ri == 1 { 50 } else { horizontal },
                    ]))
                    .unwrap(),
                );
            }
        }
        t.border_fill_id = Some(r.register_border_fill(face_definition([50; 4])).unwrap());
        let (d, s) = rebound(t, &r);
        let out = encode(&d.validate().unwrap(), &s);
        assert!(margins(&out.bytes).iter().all(|m| m[..2] == expected));
        assert_eq!(
            reasons(&out).iter().map(|r| r.0).collect::<Vec<_>>(),
            reason.into_iter().collect::<Vec<_>>()
        );
    }
}

// 이것을 실패시키는 것: 테두리 공간 cache 억제가 기존 ContentLost 경고에 Run 경로를 추가함.
#[test]
fn table_projection_cache_requests_preserve_existing_warning_paths() {
    use hwpforge_core::{
        control::Control,
        layout::{LayoutCache, LineSeg},
    };
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let mut t = table(1, 1, id, margin(540, 540, 0, 0));
    let p = &mut t.rows[0].cells[0].paragraphs[0];
    p.runs.push(Run::control(
        Control::Unknown { tag: "unsupported".into(), data: None },
        CharShapeIndex::new(0),
    ));
    p.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
    let (d, s) = rebound(t, &r);
    let d = d.validate().unwrap();
    let old = encode(&d, &s);
    let requested = HwpxEncoder::encode_with_diagnostics(
        &d,
        &s,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let paths = |out: &EncodeOutcome| {
        out.warnings
            .iter()
            .filter_map(|w| match w {
                hwpforge_smithy_hwpx::EncodeWarning::ContentLost { path, .. } => {
                    Some(path.to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(paths(&old), ["section[0].para[0].cell[0][0].npara[0]"]);
    assert_eq!(paths(&requested), paths(&old));
    let dropped: Vec<_> = requested
        .warnings
        .iter()
        .filter_map(|w| match w {
            hwpforge_smithy_hwpx::EncodeWarning::LayoutCacheDropped { path, .. } => {
                Some(path.to_string())
            }
            _ => None,
        })
        .collect();
    assert_eq!(dropped, ["section[0].para[0].run[0].cell[0][0].npara[0]"]);
}

// 이것을 실패시키는 것: 음수 검사보다 먼저 -0.5를 0으로 반올림해 표를 수용.
#[test]
fn negative_half_unit_right_margin_defers_the_entire_table() {
    let mut r = registry();
    let id = r.register_border_fill(face_definition([1, 0, 0, 0])).unwrap();
    let (d, s) = rebound(table(2, 1, id, margin(0, 0, 0, 0)), &r);
    let out = encode(&d.validate().unwrap(), &s);
    assert_eq!(margins(&out.bytes), [[0, 0, 0, 0]; 2]);
    assert_eq!(reasons(&out), [("negative_right_margin", "section[0].para[0].run[0]".into(), 1)]);
}

// 이것을 실패시키는 것: 묶음 자식/메모/필드 주석/미주 준비 또는 방출 키 누락, 보조 표에 본문 투영 적용.
#[test]
fn auxiliary_container_tables_are_prepared_and_emitted_with_identical_keys() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        control::{Control, FieldBoundary, FieldStart, GroupChild},
        image::{Image, ImageFormat},
        ObjectFrame,
    };
    for container in ["group_shape", "group_picture", "memo", "annotation", "endnote"] {
        let mut r = registry();
        let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
        let t = table(1, 1, id, margin(540, 540, 0, 0));
        let ps = vec![containing(t.clone())];
        let (control, path) = match container {
            "group_shape" | "group_picture" => {
                let (child, suffix) = if container == "group_shape" {
                    (
                        GroupChild::Shape(Control::text_box(ps, unit(10000), unit(10000))),
                        "textbox.npara[0].run[0]",
                    )
                } else {
                    let mut image = Image::new("", unit(10000), unit(10000), ImageFormat::Png);
                    image.caption = Some(Caption::new(ps, CaptionSide::Bottom));
                    (GroupChild::Picture(image), "caption.npara[0].run[0]")
                };
                (
                    Control::Group {
                        children: vec![child],
                        width: unit(10000),
                        height: unit(10000),
                        frame: ObjectFrame::new(),
                        placement: None,
                        inst_id: None,
                    },
                    format!("group[0].{suffix}"),
                )
            }
            "memo" => (Control::memo(ps), "memo.npara[0].run[0]".into()),
            "annotation" => {
                let mut start = FieldStart::default();
                start.id = "note".into();
                start.kind = "MEMO".into();
                start.annotation = Some(ps);
                (
                    Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                    "memo.npara[0].run[0]".into(),
                )
            }
            _ => (Control::endnote(ps), "endnote.npara[0].run[0]".into()),
        };
        let mut host = Paragraph::with_runs(
            vec![Run::control(control, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        if container == "annotation" {
            host.runs.push(Run::control(
                Control::FieldBoundary {
                    boundary: FieldBoundary::End {
                        begin_id: "note".into(),
                        field_id: String::new(),
                    },
                },
                CharShapeIndex::new(0),
            ));
        }
        let mut d = draft(t);
        d.sections_mut()[0].paragraphs = vec![host];
        let b =
            HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).unwrap().value;
        let d = b.rebind_draft_document(d).unwrap().validate().unwrap();
        let s = b.into_style_store();
        let out = encode(&d, &s);
        assert_eq!(margins(&out.bytes), [[540, 540, 0, 0]], "{container}");
        assert_eq!(
            reasons(&out),
            [("outside_scope", format!("section[0].para[0].run[0].{path}"), 1)],
            "{container}"
        );
        assert_eq!(encode(&d, &s).bytes, out.bytes);
    }
}

// 이것을 실패시키는 것: 문단 끝 치환을 표 테두리 투영 성공에 종속시킴.
#[test]
fn paragraph_marks_project_even_when_border_space_is_deferred() {
    for exact in [false, true] {
        let mut r = registry();
        let mut mark = r.char_shapes[0].clone();
        mark.size = unit(2000);
        r.char_shapes.push(mark);
        let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
        let mut t = table(1, 1, id, margin(540, 540, 0, 0));
        t.frame.size_protect = exact;
        t.rows[0].cells[0].paragraphs[0].end_char_shape = Some(CharShapeIndex::new(1));
        let (d, s) = rebound(t, &r);
        let o = encode(&d.validate().unwrap(), &s);
        let w: Vec<_> = o
            .warnings
            .iter()
            .filter_map(|w| {
                if let hwpforge_smithy_hwpx::EncodeWarning::ParagraphEndMarkReplaced {
                    count, ..
                } = w
                {
                    Some(*count)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(w, [1]);
        assert_eq!(
            margins(&o.bytes),
            if exact { vec![[540, 540, 0, 0]] } else { vec![[565, 515, 50, 50]] }
        );
        let d = HwpxDecoder::decode(&o.bytes).unwrap();
        let p = &d.document.sections()[0].paragraphs[0];
        let hwpforge_core::RunContent::Table(t) = &p.runs[0].content else { panic!("table") };
        let p = &t.rows[0].cells[0].paragraphs[0];
        assert_eq!(p.end_char_shape, Some(p.runs[0].char_shape_id));
    }
}

// 이것을 실패시키는 것: 투영 원인마다 캐시 경고를 중복하거나 빈 캐시/요청 안 한 캐시도 폐기 경고.
#[test]
fn combined_projections_drop_each_requested_nonempty_cache_once() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    for emit in [false, true] {
        for nonempty in [false, true] {
            let mut r=serde_json::from_value::<StyleRegistry>(serde_json::json!({"fonts":["Cambria"],"char_shapes":[],"para_shapes":[],"style_entries":{}})).unwrap();
            let mut body = hwpforge_blueprint::style::PartialCharShape::default();
            body.font = Some("Cambria".into());
            body.size = Some(unit(1100));
            r.char_shapes.push(body.resolve("body").unwrap());
            let mut mark = r.char_shapes[0].clone();
            mark.size = unit(2000);
            r.char_shapes.push(mark);
            let mut p = hwpforge_blueprint::style::ParaShape::default();
            p.line_spacing_value = 100.;
            r.para_shapes.push(p);
            let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
            let mut t = table(1, 1, id, margin(540, 540, 0, 0));
            let p = &mut t.rows[0].cells[0].paragraphs[0];
            p.end_char_shape = Some(CharShapeIndex::new(1));
            let mut line = LineSeg::default();
            line.vertsize = 1100;
            line.textheight = 1100;
            line.horzsize = 10000;
            let cache = LayoutCache::new(if nonempty { vec![line] } else { vec![] });
            p.layout_cache = Some(cache.clone());
            let (mut d, s) = rebound(t, &r);
            d.sections_mut()[0].paragraphs[0].layout_cache = Some(cache);
            let o = HwpxEncoder::encode_with_diagnostics(
                &d.validate().unwrap(),
                &s,
                &ImageStore::new(),
                EncodeOptions::default().with_emit_layout_cache(emit),
            )
            .unwrap();
            let drops: Vec<_> =
                o.warnings.iter().filter(|w| w.code() == "LAYOUT_CACHE_DROPPED").collect();
            assert_eq!(drops.len(), if emit && nonempty { 2 } else { 0 });
            let unique: std::collections::HashSet<_> = drops.iter().map(|w| w.location()).collect();
            assert_eq!(unique.len(), drops.len());
            let t = section(&o.bytes);
            assert_eq!(t.descendants("hp:linesegarray").len(), 0);
            assert_eq!(t.descendants("hp:lineSegArray").len(), 0);
            assert_eq!(t.descendants("hp:layoutCacheDropped").len(), 0);
        }
    }
}

// 이것을 실패시키는 것: 필드 builder에서 버리는 annotation 표를 테두리 준비 순회에 포함.
#[test]
fn omitted_field_annotations_do_not_prepare_border_candidates() {
    use hwpforge_core::control::{Control, FieldBoundary, FieldParameter, FieldStart};
    let mut r = registry();
    let id = r.register_border_fill(definition([Some(50); 4])).unwrap();
    let t = table(1, 1, id, margin(540, 540, 0, 0));
    let mut start = FieldStart::default();
    start.id = "field".into();
    start.kind = "MEMO".into();
    start.parameter_name = None;
    start.parameters.push(FieldParameter::String {
        name: "item".into(),
        value: "value".into(),
        xml_space: String::new(),
    });
    start.annotation = Some(vec![containing(t.clone())]);
    let mut d = draft(t);
    d.sections_mut()[0].paragraphs = vec![Paragraph::with_runs(
        vec![
            Run::control(
                Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                CharShapeIndex::new(0),
            ),
            Run::control(
                Control::FieldBoundary {
                    boundary: FieldBoundary::End {
                        begin_id: "field".into(),
                        field_id: String::new(),
                    },
                },
                CharShapeIndex::new(0),
            ),
        ],
        ParaShapeIndex::new(0),
    )];
    let b = HwpxRegistryBridge::from_registry_for_import(&r, Default::default()).unwrap().value;
    let d = b.rebind_draft_document(d).unwrap().validate().unwrap();
    let s = b.into_style_store();
    let o =
        HwpxEncoder::encode_with_diagnostics(&d, &s, &ImageStore::new(), EncodeOptions::default());
    assert!(o.is_ok());
    let o = o.unwrap();
    assert!(o.warnings.iter().any(|w| w.code() == "CONTENT_LOST"));
    assert_eq!(
        o.warnings
            .iter()
            .filter(|w| matches!(
                w.code(),
                "TABLE_BORDER_SPACE_APPROXIMATED" | "PARAGRAPH_END_MARK_REPLACED"
            ))
            .count(),
        0
    );
}
