use super::*;
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

fn para(controls: Vec<Control>) -> Paragraph {
    Paragraph::with_runs(
        controls.into_iter().map(|c| Run::control(c, CharShapeIndex::new(0))).collect(),
        ParaShapeIndex::new(0),
    )
}

fn chart(title: &str) -> Control {
    use hwpforge_core::chart::*;
    Control::Chart {
        chart_type: ChartType::Line,
        data: ChartData::category(&["X"], &[("S", [1.0].as_slice())]),
        width: HwpUnit::new(5000).unwrap(),
        height: HwpUnit::new(4000).unwrap(),
        title: Some(title.into()),
        legend: LegendPosition::default(),
        grouping: ChartGrouping::Clustered,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }
}

/// 이것을 실패시키는 것: 구역 경계 차트/OLE 번호 초기화, 일반 Chart 중첩 수집 누락,
/// 이미지 id와 OLE 할당 충돌, 중첩 경고 폐기/part 경로 오염.
#[test]
fn r3_document_context_spans_sections_and_reports_nested_losses() {
    let mut document = Document::new();
    for n in 0..2 {
        let nested = para(vec![
            chart(&format!("nested{n}")),
            Control::EmbeddedChart {
                chart_xml: format!("<chart>embedded{n}</chart>"),
                ole_bytes: vec![n as u8, 42],
                width: HwpUnit::new(5000).unwrap(),
                height: HwpUnit::new(4000).unwrap(),
                placement: None,
            },
            Control::Unknown { tag: format!("r3-loss-{n}"), data: None },
        ]);
        document.add_section(Section::with_paragraphs(
            vec![para(vec![chart(&format!("body{n}")), Control::footnote(vec![nested])])],
            PageSettings::a4(),
        ));
    }
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(crate::style_store::HwpxCharShape::default());
    styles.push_para_shape(crate::style_store::HwpxParaShape::default());
    let mut images = ImageStore::new();
    images.insert("ole1.png", vec![89, 1]);
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &document.validate().unwrap(),
        &styles,
        &images,
        Default::default(),
    )
    .unwrap();
    let losses: Vec<_> = outcome.warnings.iter().filter(|w| w.is_semantic_loss()).collect();
    assert_eq!(losses.len(), 2, "{:?}", outcome.warnings);
    for (n, loss) in losses.iter().enumerate() {
        let EncodeWarning::ContentLost { part, path, reason, .. } = loss else {
            panic!("{loss:?}")
        };
        assert_eq!(part, &format!("Contents/section{n}.xml"));
        assert!(path.to_string().contains("footnote"), "{path}");
        assert!(reason.contains(&format!("r3-loss-{n}")));
    }
    let mut zip = crate::PackageReader::new(&outcome.bytes).unwrap();
    let charts = zip.read_chart_xmls().unwrap();
    assert_eq!(charts.len(), 6);
    let bins = zip.read_all_bindata().unwrap();
    assert_eq!(bins.get("ole1.png").unwrap(), [89, 1]);
    for n in 0..2 {
        let xml = zip.read_section_xml(n).unwrap();
        let mut actual = Vec::new();
        for rest in xml.split("chartIDRef=\"").skip(1) {
            let name = rest.split('"').next().unwrap();
            actual.push(charts.get(name).unwrap().as_str());
        }
        assert_eq!(actual.len(), 3);
        for name in [format!("body{n}"), format!("nested{n}"), format!("embedded{n}")] {
            assert_eq!(actual.iter().filter(|c| c.contains(&name)).count(), 1);
        }
        let ole = format!("ole{}", n + 2);
        assert!(xml.contains(&format!("binaryItemIDRef=\"{ole}\"")));
        assert_eq!(bins.get(&format!("{ole}.ole")).unwrap(), [n as u8, 42]);
    }
}

fn r4_text(s: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(s, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn r4_encode(document: Document, images: &ImageStore) -> EncodeOutcome {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(crate::style_store::HwpxCharShape::default());
    styles.push_para_shape(crate::style_store::HwpxParaShape::default());
    HwpxEncoder::encode_with_diagnostics(
        &document.validate().unwrap(),
        &styles,
        images,
        Default::default(),
    )
    .unwrap()
}

fn r4_attribute(head: &str, name: &str) -> String {
    head.split_once(&format!("{name}=\"")).unwrap().1.split('"').next().unwrap().into()
}

/// 이것을 실패시키는 것: 컨텍스트에서 필드 예약 누락, 중첩/구역 경계에서
/// IdScope 재생성, Core ID와 새 필드/그림 ID를 별개 할당기로 발급하기.
#[test]
fn document_context_reserves_later_fields_before_nested_objects() {
    use hwpforge_core::control::{FieldBoundary, FieldStart};
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::ObjectId;

    let cs = CharShapeIndex::new(0);
    let mut imported = Image::new(
        "ole1.png",
        HwpUnit::new(1000).unwrap(),
        HwpUnit::new(1000).unwrap(),
        ImageFormat::Png,
    );
    imported.inst_id = Some(ObjectId::new(2));
    let mut drawn = imported.clone();
    drawn.inst_id = None;
    let nested = Paragraph::with_runs(
        vec![
            Run::control(
                Control::Hyperlink { text: "link".into(), url: "https://example.org".into() },
                cs,
            ),
            Run::image(imported, cs),
            Run::image(drawn, cs),
        ],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![para(vec![Control::footnote(vec![nested])])],
        PageSettings::a4(),
    ));
    let mut boundaries = Vec::new();
    for id in ["1", "1100000000"] {
        let mut start = FieldStart::default();
        start.id = id.into();
        start.kind = "CLICK_HERE".into();
        if id == "1100000000" {
            start.kind = "MEMO".into();
            start.annotation = Some(vec![para(vec![Control::footnote(vec![r4_text("note")])])]);
        }
        boundaries.push(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) });
        boundaries.push(Control::FieldBoundary {
            boundary: FieldBoundary::End { begin_id: id.into(), field_id: String::new() },
        });
    }
    doc.add_section(Section::with_paragraphs(vec![para(boundaries)], PageSettings::a4()));
    let mut images = ImageStore::new();
    images.insert("ole1.png", vec![89, 1]);
    let out = r4_encode(doc.clone(), &images);
    assert_eq!(r4_encode(doc, &images).bytes, out.bytes);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let mut zip = crate::PackageReader::new(&out.bytes).unwrap();
    let first = zip.read_section_xml(0).unwrap();
    let second = zip.read_section_xml(1).unwrap();
    assert_eq!(r4_numbers(&first, "footNote"), [1]);
    assert_eq!(r4_numbers(&second, "footNote"), [2]);
    let mut ids = std::collections::HashSet::new();
    for xml in [&first, &second] {
        for tag in ["fieldBegin", "pic"] {
            for head in xml.split(&format!("<hp:{tag} ")).skip(1) {
                let id = r4_attribute(head.split('>').next().unwrap(), "id");
                assert!(ids.insert(id.clone()), "duplicate {tag} id {id}");
                if tag == "fieldBegin" {
                    assert!(xml.contains(&format!("beginIDRef=\"{id}\"")));
                }
            }
        }
    }
    assert_eq!(ids.len(), 5);
    for id in ["1", "2", "1100000000"] {
        assert!(ids.contains(id));
    }
}

fn r4_numbers(xml: &str, tag: &str) -> Vec<u32> {
    xml.split(&format!("<hp:{tag} "))
        .skip(1)
        .map(|s| {
            let head = s.split('>').next().unwrap();
            assert_eq!(r4_attribute(head, "suffixChar"), "41");
            r4_attribute(head, "number").parse().unwrap()
        })
        .collect()
}

/// 이것을 실패시키는 것: run 순서 대신 문단 전체 우선 방문, caption/cell 순서 뒤집기,
/// 그림 캡션/바탕쪽 방문 누락, 그룹 fallback 복제 재연결 누락, 구역 경계 번호 리셋.
#[test]
fn r4_note_plan_covers_emission_order_captions_fallback_and_parts() {
    use hwpforge_core::{
        caption::Caption,
        control::{GroupChild, ShapePoint},
        image::{Image, ImageFormat},
        section::{HeaderFooter, MasterPage},
        table::{Table, TableCell, TableRow},
    };
    let hu = |n| HwpUnit::new(n).unwrap();
    for endnote in [false, true] {
        for fallback in [false, true] {
            let note = |name: &str| {
                if endnote {
                    Control::endnote(vec![r4_text(name)])
                } else {
                    Control::footnote(vec![r4_text(name)])
                }
            };
            let cap = |name: &str| {
                let mut c = Caption::default();
                c.paragraphs = vec![para(vec![note(name)])];
                c
            };
            let mut textbox =
                Control::text_box(vec![para(vec![note("box-text")])], hu(5000), hu(5000));
            if let Control::TextBox { caption, .. } = &mut textbox {
                *caption = Some(cap("box-caption"));
            }
            let mut ellipse = Control::ellipse_with_text(
                hu(5000),
                hu(5000),
                vec![para(vec![note("ellipse-text")])],
            );
            if let Control::Ellipse { caption, .. } = &mut ellipse {
                *caption = Some(cap("ellipse-caption"));
            }
            let mut children = vec![GroupChild::Shape(textbox), GroupChild::Shape(ellipse)];
            if fallback {
                children.push(GroupChild::Shape(Control::Line {
                    start: ShapePoint { x: 0, y: 0 },
                    end: ShapePoint { x: 100_000_000, y: 0 },
                    width: hu(49),
                    height: hu(0),
                    placement: None,
                    caption: None,
                    style: None,
                    frame: hwpforge_core::ObjectFrame::new()
                        .with_original_size(hwpforge_foundation::Size::new(hu(100_000_000), hu(0))),
                }));
            }
            children.push(GroupChild::Picture(
                Image::new("BinData/r4.png", hu(2000), hu(2000), ImageFormat::Png)
                    .with_caption(cap("group-picture-caption")),
            ));
            let mut rect = Control::rect(hu(2000), hu(2000)).unwrap();
            if let Control::Rect { caption, .. } = &mut rect {
                *caption = Some(cap("rect-caption"));
            }
            children.push(GroupChild::Shape(rect));
            let group = Control::Group {
                children,
                width: hu(10000),
                height: hu(10000),
                frame: Default::default(),
                placement: None,
                inst_id: None,
            };
            assert_eq!(
                hwpforge_core::control::geometry::group_plan(&group, &super::shapes::HWPX_POLICY)
                    .unwrap()
                    .is_fallback(),
                fallback
            );
            let mut table = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![para(vec![note("cell")])],
                hu(5000),
            )])]);
            table.caption = Some(cap("table-caption"));
            let image = Image::new("BinData/r4.png", hu(2000), hu(2000), ImageFormat::Png)
                .with_caption(cap("picture-caption"));
            let mut p = para(vec![note("A"), group, note("C")]);
            p.runs.insert(1, Run::table(table, CharShapeIndex::new(0)));
            p.runs.insert(2, Run::image(image, CharShapeIndex::new(0)));
            let mut section = Section::with_paragraphs(
                vec![p, para(vec![note("after-header")])],
                PageSettings::a4(),
            );
            let mut header =
                HeaderFooter::new(vec![para(vec![note("header")])], Default::default());
            header.anchor_paragraph = Some(1);
            section.headers.push(header);
            section.master_pages =
                Some(vec![MasterPage::new(Default::default(), vec![para(vec![note("master")])])]);
            let mut doc = Document::new();
            doc.add_section(section);
            doc.add_section(Section::with_paragraphs(
                vec![para(vec![note("next-section")])],
                PageSettings::a4(),
            ));
            let mut images = ImageStore::new();
            images.insert("r4.png", vec![1, 2, 3]);
            let out = r4_encode(doc, &images);
            let losses: Vec<_> = out.warnings.iter().filter(|w| w.is_semantic_loss()).collect();
            assert!(losses.iter().all(|w| matches!(w, EncodeWarning::ContentLost { kind, .. } if fallback && kind == "shape_emission")), "{:?}", out.warnings);
            let mut zip = crate::PackageReader::new(&out.bytes).unwrap();
            let tag = if endnote { "endNote" } else { "footNote" };
            let mut all = Vec::new();
            for xml in [
                zip.read_section_xml(0).unwrap(),
                zip.read_text_entry("Contents/masterpage0.xml").unwrap(),
                zip.read_section_xml(1).unwrap(),
            ] {
                let numbers = r4_numbers(&xml, tag);
                let autos: Vec<u32> = xml
                    .split("<hp:autoNum ")
                    .skip(1)
                    .map(|s| r4_attribute(s.split('>').next().unwrap(), "num").parse().unwrap())
                    .collect();
                assert_eq!(numbers, autos, "note heads: {xml}");
                all.extend(numbers);
            }
            assert_eq!(all, (1..=15).collect::<Vec<_>>());
        }
    }
}

/// 이것을 실패시키는 것: 중첩 restart 방문 생략, 비텍스트 메모 앵커 무경고 폐기,
/// 버리는 앵커의 각주가 실제 본문 번호를 소비하게 하기.
#[test]
fn r4_nested_restart_and_unsupported_anchor_have_explicit_semantics() {
    use hwpforge_core::control::NewNumberKind;
    let note = || Control::footnote(vec![r4_text("note")]);
    let memo = Control::memo_with_anchor(
        vec![para(vec![Control::NewNumber { kind: NewNumberKind::Footnote, number: 7 }, note()])],
        vec![Run::control(note(), CharShapeIndex::new(0))],
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![para(vec![note(), memo, note()])],
        PageSettings::a4(),
    ));
    let out = r4_encode(doc, &ImageStore::new());
    let losses: Vec<_> = out.warnings.iter().filter(|w| w.is_semantic_loss()).collect();
    assert_eq!(losses.len(), 1, "{:?}", out.warnings);
    assert!(matches!(losses[0], EncodeWarning::ContentLost { kind, .. } if kind == "memo_anchor"));
    let xml = crate::PackageReader::new(&out.bytes).unwrap().read_section_xml(0).unwrap();
    assert_eq!(r4_numbers(&xml, "footNote"), [1, 7, 8]);
}

/// 이것을 실패시키는 것: 문단마다 span 상태 초기화, 자식 호스트 상태 누출,
/// 치환 목록 길이로 memo와 내부 field에 같은 ID 배정, 문서/part 경계 ID 리셋.
#[test]
fn r4_field_ids_and_bookmark_scopes_survive_nested_builds() {
    use hwpforge_foundation::BookmarkType;
    let mark = |kind| Control::Bookmark { name: "span".into(), bookmark_type: kind };
    let link = || Control::Hyperlink { text: "link".into(), url: "https://example.org".into() };
    let mut doc = Document::new();
    for _ in 0..2 {
        let inner = Control::text_box(
            vec![para(vec![mark(BookmarkType::SpanEnd)])],
            HwpUnit::new(5000).unwrap(),
            HwpUnit::new(5000).unwrap(),
        );
        let memo = Control::memo(vec![para(vec![link()])]);
        doc.add_section(Section::with_paragraphs(
            vec![
                para(vec![mark(BookmarkType::SpanStart), inner, memo]),
                para(vec![mark(BookmarkType::SpanEnd), link()]),
            ],
            PageSettings::a4(),
        ));
    }
    let out = r4_encode(doc, &ImageStore::new());
    let losses: Vec<_> = out.warnings.iter().filter(|w| w.is_semantic_loss()).collect();
    assert_eq!(losses.len(), 2, "{:?}", out.warnings);
    assert!(losses.iter().all(|w| matches!(w, EncodeWarning::ContentLost { kind, reason, .. } if kind == "bookmark_span" && reason.contains("unmatched"))));
    let mut zip = crate::PackageReader::new(&out.bytes).unwrap();
    let mut ids = std::collections::HashSet::new();
    for n in 0..2 {
        let xml = zip.read_section_xml(n).unwrap();
        let begins: Vec<_> = xml
            .split("<hp:fieldBegin ")
            .skip(1)
            .map(|s| r4_attribute(s.split('>').next().unwrap(), "id"))
            .collect();
        let ends: Vec<_> = xml
            .split("<hp:fieldEnd ")
            .skip(1)
            .map(|s| r4_attribute(s.split('>').next().unwrap(), "beginIDRef"))
            .collect();
        assert_eq!(begins.len(), 4, "{xml}");
        assert_eq!(ends.len(), 4, "{xml}");
        for id in begins {
            assert!(ends.contains(&id));
            assert!(ids.insert(id), "field ID collision: {xml}");
        }
    }
}

/// 이것을 실패시키는 것: 호스트 종료 시 unclosed span 손실 경고 생략,
/// 출력하지 않는 group child 각주에 번호 배정.
#[test]
fn r4_unclosed_span_and_unencodable_group_note_are_reported() {
    let note = || Control::footnote(vec![r4_text("note")]);
    let group = Control::Group {
        children: vec![hwpforge_core::control::GroupChild::Shape(note())],
        width: HwpUnit::new(5000).unwrap(),
        height: HwpUnit::new(5000).unwrap(),
        frame: Default::default(),
        placement: None,
        inst_id: None,
    };
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![para(vec![
            note(),
            group,
            note(),
            Control::Bookmark {
                name: "unclosed".into(),
                bookmark_type: hwpforge_foundation::BookmarkType::SpanStart,
            },
        ])],
        PageSettings::a4(),
    ));
    // The public validator rejects this group child. Exercise the internal
    // unvalidated section writer's warning path as well.
    assert!(doc.clone().validate().is_err());
    let out =
        super::section::encode_section(&doc.sections()[0], 0, 0, 0, 0, Default::default()).unwrap();
    let losses: Vec<_> = out.warnings.iter().filter(|w| w.is_semantic_loss()).collect();
    assert_eq!(losses.len(), 2, "{:?}", out.warnings);
    assert!(losses.iter().any(|w| matches!(w, EncodeWarning::ContentLost { kind, reason, .. } if kind == "bookmark_span" && reason.contains("unclosed"))));
    assert!(losses.iter().any(|w| matches!(w, EncodeWarning::ContentLost { kind, reason, .. } if kind == "footnote" && reason.contains("group child"))));
    let xml = out.xml;
    assert_eq!(r4_numbers(&xml, "footNote"), [1, 2]);
}
