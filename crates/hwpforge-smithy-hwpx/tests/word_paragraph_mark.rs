use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
};
use hwpforge_core::{
    Document, Draft, ImageStore, LayoutProfile, PageSettings, Paragraph, Run, Section,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    EncodeOptions, EncodeOutcome, HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleStore,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
fn unit(n: i32) -> HwpUnit {
    HwpUnit::new(n).unwrap()
}
fn fixture(text: &str, mark: i32) -> (Document<Draft>, HwpxStyleStore) {
    let mut r:StyleRegistry=serde_json::from_value(serde_json::json!({"fonts":["Cambria"],"char_shapes":[],"para_shapes":[],"style_entries":{}})).unwrap();
    for size in [1100, mark] {
        let mut c = PartialCharShape::default();
        c.font = Some("Cambria".into());
        c.size = Some(unit(size));
        r.char_shapes.push(c.resolve("body").unwrap());
    }
    r.para_shapes.push(ParaShape::default());
    let mut d = Document::new();
    d.set_layout_profile(LayoutProfile::WordCompatible);
    d.add_section(Section::with_paragraphs(vec![paragraph(text)], PageSettings::a4()));
    let b = HwpxRegistryBridge::from_registry_for_import(&r, Default::default()).unwrap().value;
    {
        let rebound = b.rebind_draft_document(d.clone()).unwrap();
        let p = &rebound.sections()[0].paragraphs[0];
        let mut s = b.into_style_store();
        let body = s.char_shape(p.runs[0].char_shape_id).unwrap().clone();
        let mark = s.char_shape(p.end_char_shape.unwrap()).unwrap().clone();
        *s.char_shape_mut(CharShapeIndex::new(0)).unwrap() = body;
        *s.char_shape_mut(CharShapeIndex::new(1)).unwrap() = mark;
        (d, s)
    }
}
fn paragraph(text: &str) -> Paragraph {
    let mut p =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    p.end_char_shape = Some(CharShapeIndex::new(1));
    p
}
fn encode(d: &Document<Draft>, s: &HwpxStyleStore) -> EncodeOutcome {
    HwpxEncoder::encode_with_diagnostics(
        &d.clone().validate().unwrap(),
        s,
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .unwrap()
}
fn tree(out: &EncodeOutcome) -> xml::Element {
    xml::Element::parse(&package::part(&out.bytes, "Contents/section0.xml"))
}
fn end(p: &xml::Element) -> &str {
    p.children
        .iter()
        .rev()
        .find(|c| c.name == "hp:run")
        .and_then(|r| r.attr("charPrIDRef"))
        .unwrap_or("")
}
fn count(out: &EncodeOutcome) -> u64 {
    out.warnings
        .iter()
        .filter(|w| w.code() == "PARAGRAPH_END_MARK_REPLACED")
        .map(|w| match w {
            hwpforge_smithy_hwpx::EncodeWarning::ParagraphEndMarkReplaced { count, .. } => *count,
            _ => unreachable!(),
        })
        .sum()
}
// 이것을 실패시키는 것: 크기 비교 반전, 끝 run 생략, 본문 대신 빈 마지막 run 선택, 전체 문단의 개행 제외.
#[test]
fn oversized_text_tails_reuse_existing_shape_and_preserve_source() {
    for mark in [1400, 2000] {
        for text in ["A", ("long text ".repeat(40) + "last").as_str(), "A\nB\nC"] {
            let (mut d, mut s) = fixture(text, mark);
            let mut empty = s.char_shape(CharShapeIndex::new(0)).unwrap().clone();
            empty.height = unit(3000);
            let empty = s.push_char_shape(empty);
            d.sections_mut()[0].paragraphs[0].runs.push(Run::text("", empty));
            let original = d.clone();
            let d: Document<Draft> =
                serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
            let store = s.clone();
            for _ in 0..3 {
                let o = encode(&d, &s);
                let t = tree(&o);
                let p = t.descendants("hp:p")[0];
                assert_eq!(end(p), "0");
                assert_eq!(count(&o), 1);
                assert_eq!(
                    p.children
                        .iter()
                        .filter(|c| c.name == "hp:run"
                            && c.attr("charPrIDRef") == Some(empty.get().to_string().as_str()))
                        .count(),
                    1
                );
                let h = xml::Element::parse(&package::part(&o.bytes, "Contents/header.xml"));
                assert_eq!(h.descendants("hh:charPr").len(), store.char_shape_count());
                let decoded = HwpxDecoder::decode(&o.bytes).unwrap();
                assert_eq!(
                    decoded.document.sections()[0].paragraphs[0].end_char_shape,
                    Some(CharShapeIndex::new(0))
                );
                assert_eq!(
                    count(
                        &HwpxEncoder::encode_with_diagnostics(
                            &decoded.document.validate().unwrap(),
                            &decoded.style_store,
                            &decoded.image_store,
                            EncodeOptions::default()
                        )
                        .unwrap()
                    ),
                    0
                );
            }
            assert_eq!(d, original);
            assert_eq!(s, store);
        }
    }
}
// 이것을 실패시키는 것: trim 후 판정, 문단 전체 nonempty만 검사, 작은 크기도 치환, 앞선 큰 본문 무시.
#[test]
fn empty_ambiguous_and_non_enlarging_marks_are_retained() {
    for text in [
        "", "\n", "A\n", "A\r", "A\r\n", "A\n\n", "A\n\n\n", "A ", "A\t", "A\u{a0}", " ", "A\n\t",
        "A\n ",
    ] {
        let (d, s) = fixture(text, 2000);
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "1", "{text:?}");
        assert_eq!(count(&o), 0);
    }
    for mark in [900, 1100] {
        let (d, s) = fixture("A", mark);
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "1");
        assert_eq!(count(&o), 0);
    }
    for first in ["A", "A\n"] {
        let (mut d, mut s) = fixture("B", 1400);
        let mut big = s.char_shape(CharShapeIndex::new(0)).unwrap().clone();
        big.height = unit(2000);
        let id = s.push_char_shape(big);
        d.sections_mut()[0].paragraphs[0].runs.insert(0, Run::text(first, id));
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "1");
        assert_eq!(count(&o), 0);
    }
}

fn host(content: hwpforge_core::RunContent) -> Paragraph {
    let mut p = paragraph("A");
    p.runs.push(Run { content, char_shape_id: CharShapeIndex::new(0) });
    p
}
fn table(ps: Vec<Paragraph>) -> hwpforge_core::Table {
    use hwpforge_core::{Table, TableCell, TableRow};
    Table::new(vec![TableRow::new(vec![TableCell::new(ps, unit(10000))])])
}
// 이것을 실패시키는 것: 컨트롤을 건너 이전 Text를 끝으로 선택하거나 자식 칸 텍스트를 부모 끝으로 사용.
#[test]
fn object_and_inline_tails_are_conservative_but_leading_objects_allow_text() {
    use hwpforge_core::{
        control::Control,
        inline::{InlineSegment, InlineText},
        RunContent,
    };
    for content in [
        RunContent::Control(Box::new(Control::footnote(vec![paragraph("A")]))),
        RunContent::Control(Box::new(Control::endnote(vec![paragraph("A")]))),
        RunContent::Control(Box::new(Control::memo(vec![paragraph("A")]))),
        RunContent::Table(Box::new(table(vec![paragraph("A")]))),
        RunContent::InlineText(InlineText { segments: vec![InlineSegment::Plain("A".into())] }),
    ] {
        let (mut d, s) = fixture("A", 2000);
        d.sections_mut()[0].paragraphs = vec![host(content.clone())];
        let mut without_lead = d.clone();
        without_lead.sections_mut()[0].paragraphs[0].runs.remove(0);
        let output = encode(&without_lead, &s);
        assert_eq!(end(tree(&output).descendants("hp:p")[0]), "1");
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "1");
        if !matches!(content, RunContent::InlineText(_)) {
            assert_eq!(count(&o), 1);
        }
        let p = &mut d.sections_mut()[0].paragraphs[0];
        p.runs.push(Run::text("last", CharShapeIndex::new(0)));
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "0");
    }
}
// 이것을 실패시키는 것: 본문 경로 gate로 보조 컨테이너를 제외하거나 캡션·묶음·바탕쪽 준비 방문 누락.
#[test]
fn every_emitted_container_uses_its_own_mark_and_canonical_path() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        control::{Control, FieldBoundary, FieldStart, GroupChild},
        image::{Image, ImageFormat},
        section::{HeaderFooter, MasterPage},
        ObjectFrame, RunContent,
    };
    use hwpforge_foundation::ApplyPageType;
    for container in [
        "body",
        "header",
        "footer",
        "master",
        "cell",
        "nested_cell",
        "footnote",
        "endnote",
        "textbox",
        "ellipse",
        "polygon",
        "memo",
        "annotation",
        "table_caption",
        "image_caption",
        "shape_caption",
        "group_shape",
        "group_picture",
    ] {
        let (mut d, s) = fixture("A", 2000);
        let ps = vec![paragraph("target")];
        let section = &mut d.sections_mut()[0];
        section.paragraphs = vec![Paragraph::with_runs(
            vec![Run::text("body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )];
        let suffix = match container {
            "body" => {
                section.paragraphs = ps;
                "para[0]"
            }
            "header" => {
                section.headers.push(HeaderFooter::new(ps, ApplyPageType::Both));
                "header[0].npara[0]"
            }
            "footer" => {
                section.footers.push(HeaderFooter::new(ps, ApplyPageType::Both));
                "footer[0].npara[0]"
            }
            "master" => {
                section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, ps)]);
                "masterpage[0].npara[0]"
            }
            _ => {
                let (content, suffix) = match container {
                    "cell" => (RunContent::Table(Box::new(table(ps))), "cell[0][0].npara[0]"),
                    "nested_cell" => {
                        let nested = table(ps);
                        let parent = host(RunContent::Table(Box::new(nested)));
                        (
                            RunContent::Table(Box::new(table(vec![parent]))),
                            "cell[0][0].npara[0].run[1].cell[0][0].npara[0]",
                        )
                    }
                    "table_caption" => {
                        let mut t = table(vec![Paragraph::new(ParaShapeIndex::new(0))]);
                        t.caption = Some(Caption::new(ps, CaptionSide::Bottom));
                        (RunContent::Table(Box::new(t)), "caption.npara[0]")
                    }
                    "image_caption" => {
                        let mut i = Image::new("", unit(10000), unit(10000), ImageFormat::Png);
                        i.caption = Some(Caption::new(ps, CaptionSide::Bottom));
                        (RunContent::Image(i), "caption.npara[0]")
                    }
                    _ => {
                        let (c, suffix) = match container {
                            "footnote" => (Control::footnote(ps), "footnote.npara[0]"),
                            "endnote" => (Control::endnote(ps), "endnote.npara[0]"),
                            "textbox" => (
                                Control::text_box(ps, unit(10000), unit(10000)),
                                "textbox.npara[0]",
                            ),
                            "ellipse" => (
                                Control::ellipse_with_text(unit(10000), unit(10000), ps),
                                "textbox.npara[0]",
                            ),
                            "polygon" => {
                                let mut c = Control::polygon(vec![
                                    hwpforge_core::control::ShapePoint::new(0, 0),
                                    hwpforge_core::control::ShapePoint::new(10000, 0),
                                    hwpforge_core::control::ShapePoint::new(0, 10000),
                                ])
                                .unwrap();
                                if let Control::Polygon { paragraphs, .. } = &mut c {
                                    *paragraphs = ps;
                                }
                                (c, "textbox.npara[0]")
                            }
                            "memo" => (Control::memo(ps), "memo.npara[0]"),
                            "annotation" => {
                                let mut start = FieldStart::default();
                                start.id = "memo".into();
                                start.kind = "MEMO".into();
                                start.annotation = Some(ps);
                                (
                                    Control::FieldBoundary {
                                        boundary: FieldBoundary::Start(Box::new(start)),
                                    },
                                    "memo.npara[0]",
                                )
                            }
                            "shape_caption" => {
                                let mut c = Control::text_box(
                                    vec![Paragraph::with_runs(
                                        vec![Run::text("shape", CharShapeIndex::new(0))],
                                        ParaShapeIndex::new(0),
                                    )],
                                    unit(10000),
                                    unit(10000),
                                );
                                if let Control::TextBox { caption, .. } = &mut c {
                                    *caption = Some(Caption::new(ps, CaptionSide::Bottom));
                                }
                                (c, "caption.npara[0]")
                            }
                            _ => {
                                let (child, path) = if container == "group_shape" {
                                    (
                                        GroupChild::Shape(Control::text_box(
                                            ps,
                                            unit(10000),
                                            unit(10000),
                                        )),
                                        "group[0].textbox.npara[0]",
                                    )
                                } else {
                                    let mut i =
                                        Image::new("", unit(10000), unit(10000), ImageFormat::Png);
                                    i.caption = Some(Caption::new(ps, CaptionSide::Bottom));
                                    (GroupChild::Picture(i), "group[0].caption.npara[0]")
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
                                    path,
                                )
                            }
                        };
                        (RunContent::Control(Box::new(c)), suffix)
                    }
                };
                section.paragraphs = vec![host(content)];
                section.paragraphs[0].end_char_shape = None;
                if container == "annotation" {
                    section.paragraphs[0].end_char_shape = Some(CharShapeIndex::new(1));
                    section.paragraphs[0].runs.push(Run::control(
                        Control::FieldBoundary {
                            boundary: FieldBoundary::End {
                                begin_id: "memo".into(),
                                field_id: String::new(),
                            },
                        },
                        CharShapeIndex::new(0),
                    ));
                }
                suffix
            }
        };
        let o = encode(&d, &s);
        assert_eq!(count(&o), 1, "{container}");
        let expected = if matches!(container, "body" | "header" | "footer" | "master") {
            format!("section[0].{suffix}")
        } else {
            format!("section[0].para[0].run[1].{suffix}")
        };
        let w = o.warnings.iter().find(|w| w.code() == "PARAGRAPH_END_MARK_REPLACED").unwrap();
        assert_eq!(w.location(), Some(expected), "{container}");
        let part = if container == "master" {
            "Contents/masterpage0.xml"
        } else {
            "Contents/section0.xml"
        };
        let t = xml::Element::parse(&package::part(&o.bytes, part));
        let projected: Vec<_> = t
            .descendants("hp:p")
            .into_iter()
            .filter(|p| {
                p.children
                    .iter()
                    .filter(|c| c.name == "hp:run")
                    .any(|r| r.children.iter().any(|c| c.name == "hp:t" && c.text == "target"))
            })
            .collect();
        assert_eq!(projected.len(), 1, "{container}");
        assert_eq!(end(projected[0]), "0", "{container}");
    }
}
// 이것을 실패시키는 것: 필드 방출 가능 판정을 annotation 뒤로 미루어 버려지는 자식 치환을 집계.
#[test]
fn unencodable_field_annotation_does_not_replace_marks_or_suppress_caches() {
    use hwpforge_core::{
        control::{Control, FieldBoundary, FieldParameter, FieldStart},
        layout::{LayoutCache, LineSeg},
    };
    let (mut d, s) = fixture("A", 2000);
    let mut start = FieldStart::default();
    start.id = "field".into();
    start.kind = "MEMO".into();
    start.parameter_name = None;
    start.parameters.push(FieldParameter::String {
        name: "item".into(),
        value: "value".into(),
        xml_space: String::new(),
    });
    start.annotation = Some(vec![paragraph("A")]);
    let mut p = Paragraph::with_runs(
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
    );
    let mut line = LineSeg::default();
    line.vertsize = 1100;
    line.textheight = 1100;
    line.horzsize = 10000;
    p.layout_cache = Some(LayoutCache::new(vec![line]));
    d.sections_mut()[0].paragraphs = vec![p];
    let o = HwpxEncoder::encode_with_diagnostics(
        &d.validate().unwrap(),
        &s,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert_eq!(count(&o), 0);
    assert!(o.warnings.iter().any(|w| w.code() == "CONTENT_LOST"));
    assert!(o
        .warnings
        .iter()
        .filter(|w| w.code() == "LAYOUT_CACHE_DROPPED")
        .all(|w| !w.message().contains("paragraph-end")));
    assert_eq!(tree(&o).descendants("hp:subList").len(), 0);
}
// 이것을 실패시키는 것: 끝 치환만 있는 머리말은 캐시 억제에서 제외하거나 원인마다 캐시 경고 중복.
#[test]
fn header_mark_changes_drop_all_requested_caches_exactly_once() {
    use hwpforge_core::{
        layout::{LayoutCache, LineSeg},
        section::HeaderFooter,
    };
    use hwpforge_foundation::ApplyPageType;
    for emit in [false, true] {
        let (mut d, s) = fixture("body", 2000);
        let sec = &mut d.sections_mut()[0];
        sec.paragraphs[0].end_char_shape = None;
        sec.paragraphs.push(sec.paragraphs[0].clone());
        sec.headers.push(HeaderFooter::new(vec![paragraph("A")], ApplyPageType::Both));
        for p in &mut sec.paragraphs {
            let mut l = LineSeg::default();
            l.vertsize = 1100;
            l.textheight = 1100;
            l.horzsize = 10000;
            p.layout_cache = Some(LayoutCache::new(vec![l]));
        }
        let original = d.clone();
        let doc = d.validate().unwrap();
        let o = HwpxEncoder::encode_with_diagnostics(
            &doc,
            &s,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(emit),
        )
        .unwrap();
        assert_eq!(count(&o), 1);
        let drops: Vec<_> =
            o.warnings.iter().filter(|w| w.code() == "LAYOUT_CACHE_DROPPED").collect();
        assert_eq!(drops.len(), if emit { 2 } else { 0 });
        if emit {
            assert_eq!(
                drops.iter().map(|w| w.location().unwrap()).collect::<Vec<_>>(),
                ["section[0].para[0]", "section[0].para[1]"]
            );
            for w in drops {
                assert_eq!(w.message(),"layout cache dropped: Layout cache omitted because imported paragraph-end formatting was replaced");
            }
        }
        assert_eq!(tree(&o).descendants("hp:linesegarray").len(), 0);
        assert_eq!(tree(&o).descendants("hp:lineSegArray").len(), 0);
        assert!(HwpxEncoder::encode(&doc, &s, &ImageStore::new()).is_ok());
        assert!(HwpxEncoder::encode_with_options(
            &doc,
            &s,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true)
        )
        .is_err());
        let restored: HwpxStyleStore =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        let o = encode(&original, &restored);
        assert_eq!(count(&o), 0);
        assert_eq!(end(tree(&o).descendants("hp:p")[1]), "1");
    }
}

// 이것을 실패시키는 것: 필드/그림/탭 InlineText 끝을 일반 문자열로 평탄화하거나 이전 Text까지 건너뜀.
#[test]
fn field_picture_and_tab_boundaries_keep_the_source_mark() {
    use hwpforge_core::{
        control::Control,
        image::{Image, ImageFormat},
        inline::{InlineSegment, InlineTabAttr, InlineText},
        RunContent,
    };
    for content in [
        RunContent::Control(Box::new(Control::Hyperlink {
            text: "link".into(),
            url: "https://example.com".into(),
        })),
        RunContent::Control(Box::new(Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            hint_text: Some("hint".into()),
            help_text: None,
            name: Some("field".into()),
            display_text: String::new(),
        })),
        RunContent::Control(Box::new(Control::Unknown { tag: "unknown".into(), data: None })),
        RunContent::Image(Image::new("", unit(10000), unit(10000), ImageFormat::Png)),
        RunContent::InlineText(InlineText {
            segments: vec![
                InlineSegment::Plain("A".into()),
                InlineSegment::Tab(InlineTabAttr { width: unit(800), leader: 0, tab_type: 0 }),
            ],
        }),
    ] {
        let (mut d, s) = fixture("A", 2000);
        d.sections_mut()[0].paragraphs = vec![host(content)];
        let o = encode(&d, &s);
        assert_eq!(end(tree(&o).descendants("hp:p")[0]), "1");
        assert_eq!(count(&o), 0);
    }
}
// 이것을 실패시키는 것: 재디코드·stamp·fill·patch 경로에서 import 권한을 다시 만들거나 현재 끝 표시를 삭제.
#[test]
fn output_marks_survive_stamp_fill_and_preserving_patch() {
    use hwpforge_smithy_hwpx::{
        stamp::{HwpxStamper, StampAction, StampSpec},
        HwpxFiller, HwpxPatcher,
    };
    let (d, s) = fixture("Name: (          )", 2000);
    let projected = encode(&d, &s);
    assert_eq!(count(&projected), 1);
    let base = projected.bytes;
    let specs: Vec<StampSpec> = HwpxStamper::plan_bytes_v2(&base)
        .unwrap()
        .text
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "name".into(), hint: None },
        })
        .collect();
    assert_eq!(specs.len(), 1);
    let stamped = HwpxStamper::stamp_with_diagnostics(&base, &specs).unwrap();
    assert!(stamped.encode_warnings.iter().all(|w| w.code() != "PARAGRAPH_END_MARK_REPLACED"));
    let filled = HwpxFiller::fill_with_codec_diagnostics(
        &stamped.value.bytes,
        &std::collections::BTreeMap::from([("name".into(), "Alice".into())]),
    )
    .unwrap();
    assert!(filled.encode_warnings.iter().all(|w| w.code() != "PARAGRAPH_END_MARK_REPLACED"));
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let original = decoded.document.sections()[0].clone();
    let mut edit = original.clone();
    edit.paragraphs[0].runs[0].content = hwpforge_core::RunContent::Text("Updated".into());
    let preservation = HwpxPatcher::export_section_preservation(&base, 0, &original).unwrap();
    let patched =
        HwpxPatcher::patch_section_preserving(&base, 0, &edit, None, Some(&preservation)).unwrap();
    for bytes in [&stamped.value.bytes, &filled.value.bytes, &patched] {
        let decoded = HwpxDecoder::decode(bytes).unwrap();
        let p = &decoded.document.sections()[0].paragraphs[0];
        assert_eq!(p.end_char_shape, Some(CharShapeIndex::new(0)));
        assert_eq!(
            decoded.style_store.char_shape(p.end_char_shape.unwrap()).unwrap().height.as_i32(),
            1100
        );
    }
    assert_eq!(HwpxFiller::list_fields(&filled.value.bytes).unwrap()[0].current, "Alice");
    assert_eq!(
        HwpxDecoder::decode(&patched).unwrap().document.sections()[0].paragraphs[0].text_content(),
        "Updated"
    );
}

// 이것을 실패시키는 것: 자식 방출 뒤 부모 치환 위치를 기록하여 첫 위치를 뒤집음.
#[test]
fn first_replacement_location_follows_parent_entry_before_children() {
    use hwpforge_core::{control::Control, RunContent};
    let (mut d, s) = fixture("A", 2000);
    let mut p = host(RunContent::Control(Box::new(Control::footnote(vec![paragraph("child")]))));
    p.runs.push(Run::text("last", CharShapeIndex::new(0)));
    d.sections_mut()[0].paragraphs = vec![p];
    let o = encode(&d, &s);
    assert_eq!(count(&o), 2);
    let w = o.warnings.iter().find(|w| w.code() == "PARAGRAPH_END_MARK_REPLACED").unwrap();
    assert_eq!(w.location().as_deref(), Some("section[0].para[0]"));
}
