//! Preserve edits keep raw untouched body tails, including ClickHere metadata.
#[path = "support/document.rs"]
mod document;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
use document::text_para as para;
use hwpforge_core::control::Control;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::source_xml::XmlSource;
use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    style_store_for_preset, EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxFiller, HwpxPatcher,
};
use xml::Element;

fn field(name: &str) -> Paragraph {
    Paragraph::with_runs(
        vec![Run {
            content: RunContent::Control(Box::new(Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("입력".into()),
                help_text: Some("도움".into()),
                name: Some(name.into()),
                display_text: "원래 값".into(),
            })),
            char_shape_id: CharShapeIndex::new(0),
        }],
        ParaShapeIndex::new(0),
    )
}

fn input(paragraphs: Vec<Paragraph>) -> Vec<u8> {
    input_styled(paragraphs, &style_store_for_preset("default").unwrap())
}

fn input_styled(
    paragraphs: Vec<Paragraph>,
    styles: &hwpforge_smithy_hwpx::HwpxStyleStore,
) -> Vec<u8> {
    input_section(Section::with_paragraphs(paragraphs, PageSettings::a4()), styles)
}

fn input_section(section: Section, styles: &hwpforge_smithy_hwpx::HwpxStyleStore) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(section);
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), styles, &ImageStore::new()).unwrap();
    let xml = package::section_xml(&bytes);
    let mut k = 0;
    let xml = xml.split_inclusive("</hp:p>").map(|part| {
        let result = part.replace("</hp:p>", &format!("<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray></hp:p>", 9000 + k));
        k += 1;
        result
    }).collect::<String>();
    package::with_section(&bytes, &xml)
}

/// The `vertpos` of each body paragraph's first cached line, `None` without
/// a `hp:linesegarray`.
fn vertpos(xml: &str) -> Vec<Option<String>> {
    Element::parse(xml)
        .children
        .iter()
        .filter(|p| p.name == "hp:p")
        .map(|p| {
            let cache = p.children.iter().find(|c| c.name == "hp:linesegarray")?;
            Some(cache.child("hp:lineseg").attr("vertpos").unwrap().to_owned())
        })
        .collect()
}

/// Each body paragraph's source bytes and its `hp:linesegarray` source bytes.
fn body_spans(xml: &str) -> Vec<(String, Option<String>)> {
    let (_, source) = XmlSource::read(xml).unwrap();
    let elements = source.elements();
    elements
        .iter()
        .filter(|e| e.name() == "p" && elements[e.parent()].name() == "sec")
        .map(|p| {
            let cache = p
                .children()
                .iter()
                .map(|&c| &elements[c])
                .find(|c| c.name() == "linesegarray")
                .map(|c| xml[c.outer()].to_owned());
            (xml[p.outer()].to_owned(), cache)
        })
        .collect()
}

fn caches(xml: &str) -> Vec<Option<String>> {
    body_spans(xml).into_iter().map(|(_, cache)| cache).collect()
}

fn patch(bytes: &[u8], text: &str) -> Vec<u8> {
    patch_at(bytes, 0, 1, text)
}

fn patch_at(bytes: &[u8], section_index: usize, paragraph: usize, text: &str) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let mut section = decoded.document.sections()[section_index].clone();
    let preservation =
        HwpxPatcher::export_section_preservation(bytes, section_index, &section).unwrap();
    section.paragraphs[paragraph].runs[0].content = RunContent::Text(text.into());
    HwpxPatcher::patch_section_preserving(bytes, section_index, &section, None, Some(&preservation))
        .unwrap()
}

/// 이것을 실패시키는 것: patch에서 기존 stale_caches 호출, 폭 초과 편집 캐시까지 보존,
/// patch에서 필드 범위 투영 호출 삭제.
#[test]
fn body_edit_keeps_plain_tail_verbatim() {
    let bytes = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let original = caches(&package::section_xml(&bytes));
    for text in ["가".to_string(), format!("원래 값 {}", "가".repeat(70)), "원래 값\n가".into()]
    {
        let out = patch(&bytes, &text);
        let xml = package::section_xml(&out);
        let own = (text == "가").then(|| "9001".to_owned());
        let expected = [Some("9000".to_owned()), own, Some("9002".to_owned())];
        assert_eq!(vertpos(&xml), expected);
        let kept = caches(&xml);
        if text == "가" {
            assert_eq!(kept[1], original[1]);
        }
        assert_eq!(kept[0], original[0]);
        assert_eq!(kept[2], original[2]);
        let decoded = HwpxDecoder::decode(&out).unwrap();
        assert_eq!(
            decoded.document.sections()[0].paragraphs[1].runs[0].content,
            RunContent::Text(text)
        );
        assert_eq!(
            package::part(&out, "Contents/header.xml"),
            package::part(&bytes, "Contents/header.xml")
        );
    }
}

// 이것을 실패시키는 것: 위치 없는 BOOKMARK/편집 불가 CLICK_HERE가 구역 전체를 unproven으로 만들기,
// 위치 없는 필드의 자기 차단 생략, 비누름틀·비책갈피 필드의 위치 실패도 자기 차단으로만 처리.
#[test]
fn unlocated_master_page_fields_keep_tail_policy_and_drop_edited_cache() {
    use hwpforge_core::field_inventory::{FieldEditability, FieldInventory};
    use hwpforge_core::section::MasterPage;
    use hwpforge_foundation::ApplyPageType;

    let mut section = Section::with_paragraphs(
        vec![para("앞"), para("원래 값"), para("뒤"), para("끝")],
        PageSettings::a4(),
    );
    section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![field("master")])]);
    let base = input_section(section, &style_store_for_preset("default").unwrap());
    for (kind, keeps_tail) in [("BOOKMARK", true), ("CLICK_HERE", true), ("CROSSREF", false)] {
        let bytes = package::splice_part(
            &base,
            "Contents/masterpage0.xml",
            "type=\"CLICK_HERE\"",
            &format!("type=\"{kind}\""),
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let inventory = FieldInventory::from_sections(decoded.document.sections());
        assert!(inventory.diagnostics().is_empty());
        assert!(inventory.warnings().is_empty());
        assert_eq!(inventory.fields().len(), 1);
        let field = &inventory.fields()[0];
        assert_eq!(field.kind(), kind);
        assert!(field.start().starts_with("master_pages[0]."));
        assert_ne!(field.editability(), FieldEditability::Editable);

        for edited in [1, 2] {
            let out = patch_at(&bytes, 0, edited, "가");
            let xml = package::section_xml(&out);
            let expected = match (edited, keeps_tail) {
                (1, true) => [Some("9000"), None, Some("9002"), Some("9003")],
                (2, true) => [Some("9000"), Some("9001"), None, Some("9003")],
                (1, false) => [Some("9000"), None, None, None],
                (2, false) => [Some("9000"), Some("9001"), None, None],
                _ => unreachable!(),
            };
            assert_eq!(vertpos(&xml), expected.map(|v| v.map(str::to_owned)), "{kind}, {edited}");
            let original = caches(&package::section_xml(&bytes));
            for (p, cache) in caches(&xml).iter().enumerate() {
                if expected[p].is_some() {
                    assert_eq!(*cache, original[p]);
                }
            }
            assert_eq!(
                HwpxDecoder::decode(&out).unwrap().document.sections()[0].paragraphs[edited]
                    .text_content(),
                "가"
            );
            assert_eq!(
                package::part(&out, "Contents/masterpage0.xml"),
                package::part(&bytes, "Contents/masterpage0.xml")
            );
        }
    }
}

/// 이것을 실패시키는 것: fieldBegin/fieldEnd/parameters/stringParam을 허용 목록에서 제외,
/// CLICK_HERE 필드를 허용 type 에서 빼기.
#[test]
fn fill_keeps_clickhere_tail_and_its_metadata_bytes() {
    let bytes = input(vec![field("edit"), field("keep")]);
    let xml = package::section_xml(&bytes);
    let tree = Element::parse(&xml);
    let tail = tree.children.iter().filter(|p| p.name == "hp:p").nth(1).unwrap();
    assert_eq!(tail.find("hp:fieldBegin").attr("type"), Some("CLICK_HERE"));
    for name in ["hp:parameters", "hp:stringParam", "hp:fieldEnd"] {
        tail.find(name);
    }
    for value in ["가나다", "가상시 가상로 12"] {
        let values = std::collections::BTreeMap::from([("edit".to_string(), value.to_string())]);
        let out = HwpxFiller::fill(&bytes, &values).unwrap();
        let out_xml = package::section_xml(&out.bytes);
        assert_eq!(vertpos(&out_xml), [Some("9000".to_owned()), Some("9001".to_owned())]);
        assert_eq!(caches(&out_xml), caches(&xml));
        assert_eq!(
            body_spans(&out_xml)[1],
            body_spans(&xml)[1],
            "the tail paragraph is kept byte for byte"
        );
    }
}

/// 이것을 실패시키는 것: 정규화 전 요청 슬롯을 변경으로 간주.
#[test]
fn normalized_noop_keeps_package_bytes() {
    let bytes = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    assert_eq!(patch(&bytes, "원래 값"), bytes);
    assert_eq!(patch(&bytes, "원래\u{0} 값"), bytes);
    let unsupported = input(vec![para("앞"), para("A"), para("뒤")]);
    assert_eq!(patch(&unsupported, "A\u{0}"), unsupported);
}

/// The rebuild path (`encode_with_base`: re-encode, then carry unchanged
/// caches) keeps its conservative rule: the edited paragraph and its tail
/// lose their caches, while the preserving patch keeps a proven one-line paragraph and its tail.
/// 이것을 실패시키는 것: 공유 stale_paragraphs를 자기 문단만 제거하도록 완화.
#[test]
fn rebuild_with_base_keeps_conservative_invalidation() {
    let bytes = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    decoded.document.sections_mut()[0].paragraphs[1].runs[0].content =
        RunContent::Text("새 값".into());
    let validated = decoded.document.validate().unwrap();
    let encoded =
        HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).unwrap();
    let out = HwpxEncoder::encode_with_base(
        &bytes,
        &validated,
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap()
    .bytes;
    let first_cache = caches(&package::section_xml(&bytes))[0].clone().unwrap();
    let expected =
        package::section_xml(&encoded).replacen("</hp:p>", &format!("{first_cache}</hp:p>"), 1);
    assert_eq!(package::section_xml(&out), expected);
    assert_eq!(vertpos(&package::section_xml(&out)), [Some("9000".to_owned()), None, None]);
    let preserved = package::section_xml(&patch(&bytes, "새 값"));
    assert_eq!(
        vertpos(&preserved),
        [Some("9000".to_owned()), Some("9001".to_owned()), Some("9002".to_owned())]
    );
    let normalized = package::splice_part(
        &bytes,
        "Contents/header.xml",
        "<hh:charPr id=\"0\"",
        "<hh:charPr id=\"7\"",
    );
    let rebuilt = patch(&normalized, "새 값");
    assert_eq!(vertpos(&package::section_xml(&rebuilt)), [Some("9000".to_owned()), None, None]);
}

/// A cross reference (id 77) that begins in the edited body paragraph after
/// the edited text and ends two paragraphs later, holding a ClickHere field
/// with the same id: inside an inline table cell, or folded inside the
/// first paragraph. Every body paragraph the cross reference covers, and the
/// tail after it, loses its cache.
/// 이것을 실패시키는 것: 인벤토리 범위 투영 삭제, 같은 id 누름틀 끝으로 필드 닫기.
#[test]
fn a_cross_reference_over_paragraphs_drops_every_paragraph_it_covers() {
    let begin = r#"<hp:ctrl><hp:fieldBegin id="77" type="CROSSREF" name="" editable="0" dirty="0" zorder="0" fieldid="1"/></hp:ctrl>"#;
    let end = r#"<hp:ctrl><hp:fieldEnd beginIDRef="77" fieldid="1"/></hp:ctrl>"#;
    let click = r#"<hp:ctrl><hp:fieldBegin id="77" type="CLICK_HERE" name="" editable="1" dirty="0" zorder="0" fieldid="2"/></hp:ctrl><hp:t>값</hp:t><hp:ctrl><hp:fieldEnd beginIDRef="77" fieldid="2"/></hp:ctrl>"#;
    let cell = format!(
        r#"<hp:tbl rowCnt="1" colCnt="1"><hp:sz width="10000" height="5000"/><hp:pos treatAsChar="1"/><hp:tr><hp:tc><hp:subList><hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">{click}</hp:run></hp:p></hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="10000" height="5000"/></hp:tc></hp:tr></hp:tbl>"#
    );
    let base = input(vec![para("편집"), para("본문"), para("끝"), para("꼬리")]);
    for inner in [cell.as_str(), click] {
        let xml = package::section_xml(&base)
            .replacen(
                "<hp:t>편집</hp:t>",
                &format!("<hp:t>편집</hp:t>{begin}{inner}<hp:t>앞</hp:t>"),
                1,
            )
            .replacen("<hp:t>끝</hp:t>", &format!("<hp:t>끝</hp:t>{end}"), 1);
        let bytes = package::with_section(&base, &xml);
        assert_eq!(
            vertpos(&xml),
            [
                Some("9000".to_owned()),
                Some("9001".to_owned()),
                Some("9002".to_owned()),
                Some("9003".to_owned())
            ]
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let mut section = decoded.document.sections()[0].clone();
        let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
        let run = section.paragraphs[0]
            .runs
            .iter_mut()
            .find(|r| r.content.as_text() == Some("편집"))
            .unwrap();
        run.content = RunContent::Text(format!("편집 {}", "가".repeat(70)));
        let out =
            HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
                .unwrap();
        assert_eq!(vertpos(&package::section_xml(&out)), [None, None, None, None], "{inner}");
    }
}

/// A section whose root element is named `pos` instead of `sec` decodes
/// without a panic, and a preserving patch on it neither panics nor keeps the
/// later caches: a `pos` with no owning object cannot prove the tail plain.
/// 이것을 실패시키는 것: 가상 루트 아래 pos 를 실패로 다루지 않기(가상 원소 범위를 시작 태그로 읽기).
#[test]
fn a_section_rooted_at_pos_decodes_and_patches_without_a_panic() {
    let rename =
        |xml: String| xml.replace("<hs:sec ", "<hs:pos ").replace("</hs:sec>", "</hs:pos>");
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/structural/plain_paragraphs.hwpx"
    ))
    .unwrap();
    let renamed = package::with_section(&fixture, &rename(package::section_xml(&fixture)));
    let decoded = HwpxDecoder::decode(&renamed).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs.len(), 4);

    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let bytes = package::with_section(&base, &rename(package::section_xml(&base)));
    let out = patch(&bytes, "가");
    let xml = package::section_xml(&out);
    assert!(xml.contains("<hs:pos "), "the root stays as it was");
    let tree = Element::parse(&xml);
    let caches: Vec<_> = tree
        .children
        .iter()
        .filter(|p| p.name == "hp:p")
        .map(|p| p.children.iter().any(|c| c.name == "hp:linesegarray"))
        .collect();
    assert_eq!(caches, [true, false, false]);
}

fn with_width(bytes: &[u8], width: i32) -> Vec<u8> {
    package::splice(bytes, "horzsize=\"42520\"", &format!("horzsize=\"{width}\""))
}
fn assert_self(bytes: &[u8], text: &str, expected: bool) {
    let before = package::section_xml(bytes);
    let out = patch(bytes, text);
    let xml = package::section_xml(&out);
    let tree = Element::parse(&xml);
    let own = tree.children.iter().filter(|p| p.name == "hp:p").nth(1).unwrap();
    assert_eq!(own.children.iter().any(|n| n.name == "hp:linesegarray"), expected, "{text:?}");
    assert_eq!(caches(&xml)[1], expected.then(|| caches(&before)[1].clone().unwrap()));
}

// 이것을 실패시키는 것: 정리 전 폭 계산, 공백 trim, 원문 폭 검사 추가, 경계 <= 를 < 로 변경.
#[test]
fn normalized_body_width_uses_the_whole_line_and_original_interval() {
    let base = with_width(&input(vec![para("앞"), para("원래 값"), para("뒤")]), 1050);
    for text in ["가", "\u{1}가", "0", ":", " "] {
        assert_self(&base, text, true);
    }
    for text in [" 가", "가 ", "가\n", "가\r", "가\r\n", "가\t", "가\u{a0}", "A"] {
        assert_self(&base, text, false);
    }
    assert_self(
        &with_width(&input(vec![para("앞"), para("원래 값"), para("뒤")]), 1049),
        "가",
        false,
    );
    for indent in [-500, 500] {
        let mut styles = style_store_for_preset("default").unwrap();
        let shape = styles.para_shape_mut(ParaShapeIndex::new(0)).unwrap();
        shape.margin_left = hwpforge_core::ParagraphMeasure::from_hwp_unit(
            hwpforge_foundation::HwpUnit::new(500).unwrap(),
        );
        shape.indent = hwpforge_core::ParagraphMeasure::from_hwp_unit(
            hwpforge_foundation::HwpUnit::new(indent).unwrap(),
        );
        let bytes =
            with_width(&input_styled(vec![para("앞"), para("원래 값"), para("뒤")], &styles), 1050);
        assert_self(&bytes, "가", true);
    }
}

// 이것을 실패시키는 것: 변경값만 합산, 미편집 조각 누락, 필드 표시값 이중 합산.
#[test]
fn fields_and_surrounding_runs_share_one_width_budget() {
    let mut first = field("first");
    let mut second = field("second");
    for p in [&mut first, &mut second] {
        if let RunContent::Control(c) = &mut p.runs[0].content {
            if let Control::Field { display_text, .. } = &mut **c {
                *display_text = "1".into();
            }
        }
    }
    let mut runs = vec![Run::text("1", CharShapeIndex::new(0))];
    runs.append(&mut first.runs);
    runs.push(Run::text("1", CharShapeIndex::new(0)));
    runs.append(&mut second.runs);
    runs.push(Run::text("1", CharShapeIndex::new(0)));
    let body = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    let bytes = with_width(&input(vec![body, para("뒤")]), 5000);
    for (value, keep) in [("2", true), ("22", false)] {
        let values = std::collections::BTreeMap::from([
            ("first".to_string(), value.to_string()),
            ("second".to_string(), value.to_string()),
        ]);
        let filled = HwpxFiller::fill(&bytes, &values).unwrap().bytes;
        let original = HwpxDecoder::decode(&bytes).unwrap();
        let mut sections = original.document.sections().to_vec();
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            original.document.sections(),
        );
        let edits: Vec<_> = inventory.fields().iter().map(|f| (f, value)).collect();
        assert!(hwpforge_core::field_inventory::FieldInventory::replace_values(
            &mut sections,
            &edits
        ));
        let preservation =
            HwpxPatcher::export_section_preservation(&bytes, 0, &original.document.sections()[0])
                .unwrap();
        let patched = HwpxPatcher::patch_section_preserving(
            &bytes,
            0,
            &sections[0],
            None,
            Some(&preservation),
        )
        .unwrap();
        for out in [filled, patched] {
            let xml = package::section_xml(&out);
            assert_eq!(vertpos(&xml), [keep.then(|| "9000".to_owned()), Some("9001".to_owned())]);
            if keep {
                assert_eq!(caches(&xml), caches(&package::section_xml(&bytes)));
            }
        }
    }
}

// 이것을 실패시키는 것: 자기 요소에 꼬리 허용 목록 재사용, ctrl 하위 무조건 허용.
#[test]
fn inline_markers_and_other_controls_prevent_self_preservation() {
    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    for element in ["<hp:tab/>", "<hp:lineBreak/>", "<hp:nbSpace/>", "<hp:fwSpace/>"] {
        let bytes = package::splice(
            &base,
            "<hp:t>원래 값</hp:t>",
            &format!("<hp:t>원래 값{element}</hp:t>"),
        );
        assert_self(&bytes, "가", false);
    }
    for control in [
        r#"<hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT"/></hp:autoNum>"#,
        r#"<hp:footNote><hp:subList><hp:p id="4" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote>"#,
    ] {
        let base = input(vec![field("edit"), para("뒤")]);
        let bytes =
            package::splice(&base, "</hp:run>", &format!("<hp:ctrl>{control}</hp:ctrl></hp:run>"));
        let values = std::collections::BTreeMap::from([("edit".into(), "가".into())]);
        let out = HwpxFiller::fill(&bytes, &values).unwrap().bytes;
        assert_eq!(vertpos(&package::section_xml(&out))[0], None, "{control}");
    }
}

// 이것을 실패시키는 것: 승격 캐시 대신 원시 캐시로 승인, 중복 배열/다줄 허용.
#[test]
fn absent_multiline_and_unpromoted_body_caches_stay_unproven() {
    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let original = package::section_xml(&base);
    let cache = caches(&original)[1].clone().unwrap();
    for replacement in [
        String::new(),
        cache.replace("textpos=\"0\"", "textpos=\"999\""),
        cache.replace(
            "</hp:linesegarray>",
            &format!(
                "{}</hp:linesegarray>",
                cache.replace("<hp:linesegarray>", "").replace("</hp:linesegarray>", "")
            ),
        ),
    ] {
        let bytes = package::with_section(&base, &original.replacen(&cache, &replacement, 1));
        assert_self(&bytes, "가", false);
    }
}

// 이것을 실패시키는 것: grid/paraPr 조건 우회, lineGrid/snapToGrid 를 새 거부 조건으로 추가.
#[test]
fn body_properties_share_the_cell_admission_rules() {
    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    for (part, from, to, keep) in [
        ("Contents/section0.xml", "charGrid=\"0\"", "charGrid=\"1\"", false),
        ("Contents/section0.xml", "wonggojiFormat=\"0\"", "wonggojiFormat=\"1\"", false),
        ("Contents/section0.xml", "lineGrid=\"0\"", "lineGrid=\"7\"", true),
        ("Contents/header.xml", "snapToGrid=\"1\"", "snapToGrid=\"0\"", true),
        ("Contents/header.xml", "fontLineHeight=\"0\"", "fontLineHeight=\"1\"", false),
        ("Contents/header.xml", "condense=\"0\"", "condense=\"1\"", false),
        ("Contents/header.xml", "eAsianEng=\"0\"", "eAsianEng=\"1\"", false),
        ("Contents/header.xml", "eAsianNum=\"0\"", "eAsianNum=\"1\"", false),
    ] {
        let bytes = package::splice_part(&base, part, from, to);
        assert_self(&bytes, "가", keep);
    }
}

// 이것을 실패시키는 것: 원본/최종 빈 누름틀을 전체 문단 문자열만으로 승인.
#[test]
fn an_empty_field_stays_unproven_even_with_surrounding_text() {
    let mut p = field("edit");
    p.runs.insert(0, Run::text("앞", CharShapeIndex::new(0)));
    let bytes = input(vec![p]);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let mut section = decoded.document.sections()[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
    for run in &mut section.paragraphs[0].runs {
        match &mut run.content {
            RunContent::Text(t) if t == "원래 값" => *t = "\u{1}".into(),
            RunContent::Control(c) => {
                if let Control::Field { display_text, .. } = &mut **c {
                    *display_text = "\u{1}".into();
                }
            }
            _ => {}
        }
    }
    let output =
        HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
            .unwrap();
    assert_eq!(vertpos(&package::section_xml(&output)), [None]);
}

// 이것을 실패시키는 것: charPr 0 고정, 본문 공통 charPr 검사 우회.
#[test]
fn body_character_definitions_follow_reference_order() {
    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let header = package::part(&base, "Contents/header.xml");
    let start = header.find("<hh:charPr ").unwrap();
    let end = start + header[start..].find("</hh:charPr>").unwrap() + "</hh:charPr>".len();
    let first = &header[start..end];
    for (effect_first, index, keep) in
        [(true, 0, false), (true, 1, true), (false, 0, true), (false, 1, false)]
    {
        let bold = first.replace("</hh:charPr>", "<hh:bold/></hh:charPr>");
        // 이것을 실패시키는 것: 모든 원본 글자 정의를 저장소의 첫 모양과 비교.
        // Distinct metrics expose a lookup that always selects the first style.
        let second = if effect_first { first.to_owned() } else { bold.clone() }
            .replace("id=\"0\"", "id=\"1\"")
            .replace("=\"100\"", "=\"90\"");
        let definitions =
            if effect_first { format!("{bold}{second}") } else { format!("{first}{second}") };
        let second_end = end + header[end..].find("</hh:charPr>").unwrap() + "</hh:charPr>".len();
        let bytes = package::splice_part(
            &base,
            "Contents/header.xml",
            &header[start..second_end],
            &definitions,
        );
        let bytes =
            package::splice(&bytes, "charPrIDRef=\"0\"", &format!("charPrIDRef=\"{index}\""));
        assert_self(&bytes, "가", keep);
    }
    for name in ["ratio", "spacing", "relSz", "offset"] {
        let start = header.find(&format!("<hh:{name} ")).unwrap();
        let end = start + header[start..].find("/>").unwrap() + 2;
        let element = &header[start..end];
        let latin = if matches!(name, "ratio" | "relSz") { "latin=\"100\"" } else { "latin=\"0\"" };
        let replaced = element.replace(latin, "latin=\"3\"");
        assert_ne!(element, replaced);
        let bytes = package::splice_part(&base, "Contents/header.xml", element, &replaced);
        assert_self(&bytes, "12", false);
    }
}

// 이것을 실패시키는 것: 경계형 필드 slot 제외 또는 field.current를 한 번 더 합산.
#[test]
fn field_boundaries_across_runs_preserve_one_line() {
    let base = with_width(&input(vec![para("앞"), field("edit"), para("뒤")]), 1050);
    let bytes = package::splice(&base,"<hp:t>원래 값</hp:t>","</hp:run><hp:run charPrIDRef=\"0\"><hp:t>원래 값</hp:t></hp:run><hp:run charPrIDRef=\"0\">");
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    assert!(decoded.document.sections()[0].paragraphs[1]
        .runs
        .iter()
        .any(|r| r.content.as_text() == Some("원래 값")));
    let out =
        HwpxFiller::fill(&bytes, &std::collections::BTreeMap::from([("edit".into(), "가".into())]))
            .unwrap()
            .bytes;
    let xml = package::section_xml(&out);
    assert_eq!(vertpos(&xml), [Some("9000".into()), Some("9001".into()), Some("9002".into())]);
    assert_eq!(caches(&xml), caches(&package::section_xml(&bytes)));
}

// 이것을 실패시키는 것: 편집 불가 CLICK_HERE도 자기 문단 범위 검사에서 제외,
// 또는 그 필드를 미편집 꼬리 캐시의 차단 요소로 처리.
#[test]
fn clickhere_with_two_body_runs_drops_edited_paragraph_cache() {
    let base = with_width(&input(vec![para("앞"), para("가"), para("뒤")]), 3150);
    let bytes = package::splice(
        &base,
        "<hp:t>가</hp:t>",
        r#"<hp:t>가</hp:t><hp:ctrl><hp:fieldBegin id="77" type="CLICK_HERE" name="range" editable="1" dirty="0" zorder="0" fieldid="1"/></hp:ctrl></hp:run><hp:run charPrIDRef="0"><hp:t>나나</hp:t></hp:run><hp:run charPrIDRef="0"><hp:t>다</hp:t></hp:run><hp:run charPrIDRef="0"><hp:ctrl><hp:fieldEnd beginIDRef="77" fieldid="1"/></hp:ctrl>"#,
    );
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let inventory =
        hwpforge_core::field_inventory::FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(inventory.fields().len(), 1);
    assert_eq!(inventory.fields()[0].current(), "나나다");
    assert_eq!(
        inventory.fields()[0].editability(),
        hwpforge_core::field_inventory::FieldEditability::UnsupportedRange
    );
    let out = patch(&bytes, "가가");
    let xml = package::section_xml(&out);
    assert_eq!(vertpos(&xml), [Some("9000".into()), None, Some("9002".into())]);
    let tree = Element::parse(&xml);
    let texts: Vec<_> = tree.children[1]
        .children
        .iter()
        .filter(|r| r.name == "hp:run")
        .flat_map(|r| &r.children)
        .filter(|t| t.name == "hp:t")
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(texts, ["가가", "나나", "다"]);
    let preceding_edit = patch_at(&bytes, 0, 0, "가");
    assert_eq!(
        vertpos(&package::section_xml(&preceding_edit)),
        [Some("9000".into()), Some("9001".into()), Some("9002".into())]
    );
}

// 이것을 실패시키는 것: 편집 불가 CLICK_HERE 제외 또는 그 끝 문단을 자기 차단 범위에서 제외.
#[test]
fn clickhere_across_paragraphs_drops_both_edited_endpoint_caches() {
    let base = with_width(&input(vec![para("앞"), para("가"), para("라"), para("뒤")]), 3150);
    let xml = package::section_xml(&base)
        .replace("<hp:t>가</hp:t>", r#"<hp:t>가</hp:t><hp:ctrl><hp:fieldBegin id="77" type="CLICK_HERE" name="range" editable="1" dirty="0" zorder="0" fieldid="1"/></hp:ctrl></hp:run><hp:run charPrIDRef="0"><hp:t>나나</hp:t>"#)
        .replace("<hp:t>라</hp:t>", r#"<hp:t>다다</hp:t></hp:run><hp:run charPrIDRef="0"><hp:ctrl><hp:fieldEnd beginIDRef="77" fieldid="1"/></hp:ctrl><hp:t>라</hp:t>"#);
    let bytes = package::with_section(&base, &xml);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let inventory =
        hwpforge_core::field_inventory::FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(inventory.fields().len(), 1);
    assert_eq!(
        inventory.fields()[0].editability(),
        hwpforge_core::field_inventory::FieldEditability::UnsupportedRange
    );
    let mut section = decoded.document.sections()[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
    for (p, before, after) in [(1, "가", "가가"), (2, "라", "라라")] {
        section.paragraphs[p]
            .runs
            .iter_mut()
            .find(|r| r.content.as_text() == Some(before))
            .unwrap()
            .content = RunContent::Text(after.into());
    }
    let out = HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
        .unwrap();
    let xml = package::section_xml(&out);
    assert_eq!(vertpos(&xml), [Some("9000".into()), None, None, Some("9003".into())]);
    let tree = Element::parse(&xml);
    for (p, expected) in [(1, vec!["가가", "나나"]), (2, vec!["다다", "라라"])] {
        let texts: Vec<_> = tree.children[p]
            .children
            .iter()
            .filter(|r| r.name == "hp:run")
            .flat_map(|r| &r.children)
            .filter(|t| t.name == "hp:t")
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(texts, expected);
    }
}

// 이것을 실패시키는 것: 직접 칸 후보 제외, 성공한 편집도 전파 시작점에 포함.
#[test]
fn body_and_direct_cell_edits_preserve_without_propagation() {
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![para("칸 앞"), para("칸 뒤")],
        HwpUnit::new(8000).unwrap(),
    )])]);
    let host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let bytes = input(vec![para("앞"), host, para("뒤")]);
    let mut source = package::section_xml(&bytes).replace("<hp:t>뒤</hp:t>", "<hp:t>뒤꼬리</hp:t>");
    let tail = source.rfind("</hp:linesegarray>").unwrap();
    let second_line = "<hp:lineseg textpos=\"1\" vertpos=\"9600\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" horzsize=\"42520\" flags=\"393216\"/>";
    source.insert_str(tail, second_line);
    let bytes = package::with_section(&bytes, &source);
    assert_eq!(
        HwpxDecoder::decode(&bytes).unwrap().document.sections()[0].paragraphs[2]
            .layout_cache
            .as_ref()
            .unwrap()
            .lines
            .len(),
        2
    );
    for body_too in [false, true] {
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let mut section = decoded.document.sections()[0].clone();
        let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
        if body_too {
            section.paragraphs[0].runs[0].content = RunContent::Text("가".into());
        }
        let RunContent::Table(table) = &mut section.paragraphs[1].runs[0].content else {
            panic!("table")
        };
        table.rows[0].cells[0].paragraphs[1].runs[0].content = RunContent::Text("나".into());
        let out =
            HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
                .unwrap();
        let xml = package::section_xml(&out);
        assert_eq!(vertpos(&xml), [Some("9000".into()), Some("9003".into()), Some("9004".into())]);
        let mut expected_xml =
            package::section_xml(&bytes).replace("<hp:t>칸 뒤</hp:t>", "<hp:t>나</hp:t>");
        if body_too {
            expected_xml = expected_xml.replace("<hp:t>앞</hp:t>", "<hp:t>가</hp:t>");
        }
        assert_eq!(xml, expected_xml);
        let tree = Element::parse(&xml);
        let cell = tree.find("hp:tc").find("hp:subList");
        let ps: Vec<_> = cell.children.iter().filter(|p| p.name == "hp:p").collect();
        assert!(ps[0].children.iter().any(|c| c.name == "hp:linesegarray"));
        assert!(ps[1].children.iter().any(|c| c.name == "hp:linesegarray"));
    }
}
