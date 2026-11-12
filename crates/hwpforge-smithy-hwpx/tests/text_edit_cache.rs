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
    // Keep source-layout tests eligible under the measured-font requirement.
    let bytes = package::splice_part(&bytes, "Contents/header.xml", "함초롬돋움", "맑은 고딕");
    let xml = package::section_xml(&bytes);
    let cache_start = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos=""#;
    let cache_end = r#"" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="42520" flags="393216"/></hp:linesegarray></hp:p>"#;
    let mut k = 0;
    let xml = xml
        .split_inclusive("</hp:p>")
        .map(|part| {
            let cache = format!("{cache_start}{}{cache_end}", 9000 + k);
            let result = part.replace("</hp:p>", &cache);
            k += 1;
            result
        })
        .collect::<String>();
    package::with_section(&bytes, &xml)
}

/// The `vertpos` of each body paragraph's first cached line, `None` without
/// a `hp:linesegarray`.
// 이것을 실패시키는 것: 다줄 캐시에서 첫 줄 대신 마지막 줄을 관찰.
fn vertpos(xml: &str) -> Vec<Option<String>> {
    Element::parse(xml)
        .children
        .iter()
        .filter(|p| p.name == "hp:p")
        .map(|p| {
            let cache = p.children.iter().find(|c| c.name == "hp:linesegarray")?;
            let first = cache.children.iter().find(|c| c.name == "hp:lineseg")?;
            Some(first.attr("vertpos").unwrap().to_owned())
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
    // 이것을 실패시키는 것: 측정 허용표에서 맑은 고딕 제거.
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

// 이것을 실패시키는 것: grid/paraPr 조건 우회, lineGrid 비영 승인, 명시 snapToGrid의 Core 일치 무시.
#[test]
fn body_properties_share_the_cell_admission_rules() {
    let base = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    for (part, from, to, keep) in [
        ("Contents/section0.xml", "charGrid=\"0\"", "charGrid=\"1\"", false),
        ("Contents/section0.xml", "wonggojiFormat=\"0\"", "wonggojiFormat=\"1\"", false),
        ("Contents/section0.xml", "lineGrid=\"0\"", "lineGrid=\"7\"", false),
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
        let bytes = package::splice_part(
            &bytes,
            "Contents/header.xml",
            "charPrIDRef=\"0\"",
            &format!("charPrIDRef=\"{index}\""),
        );
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
    let boundary_runs = "</hp:run><hp:run charPrIDRef=\"0\"><hp:t>원래 값</hp:t></hp:run><hp:run charPrIDRef=\"0\">";
    let bytes = package::splice(&base, "<hp:t>원래 값</hp:t>", boundary_runs);
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
    let field_start = r#"<hp:t>가</hp:t><hp:ctrl><hp:fieldBegin id="77" type="CLICK_HERE" name="range" editable="1" dirty="0" zorder="0" fieldid="1"/></hp:ctrl></hp:run><hp:run charPrIDRef="0"><hp:t>나나</hp:t>"#;
    let field_end = r#"<hp:t>다다</hp:t></hp:run><hp:run charPrIDRef="0"><hp:ctrl><hp:fieldEnd beginIDRef="77" fieldid="1"/></hp:ctrl><hp:t>라</hp:t>"#;
    let xml = package::section_xml(&base)
        .replace("<hp:t>가</hp:t>", field_start)
        .replace("<hp:t>라</hp:t>", field_end);
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

fn table_host(cells: Vec<Vec<Paragraph>>) -> Paragraph {
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    Paragraph::with_runs(
        vec![Run::table(
            Table::new(vec![TableRow::new(
                cells
                    .into_iter()
                    .map(|ps| TableCell::new(ps, HwpUnit::new(8000).unwrap()))
                    .collect(),
            )]),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    )
}

fn cell_paragraph(
    section: &mut Section,
    host: usize,
    cell: usize,
    paragraph: usize,
) -> &mut Paragraph {
    let RunContent::Table(table) = &mut section.paragraphs[host].runs[0].content else {
        panic!("table")
    };
    &mut table.rows[0].cells[cell].paragraphs[paragraph]
}

fn patch_section(bytes: &[u8], edit: impl FnOnce(&mut Section)) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let mut section = decoded.document.sections()[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(bytes, 0, &section).unwrap();
    edit(&mut section);
    HwpxPatcher::patch_section_preserving(bytes, 0, &section, None, Some(&preservation)).unwrap()
}

fn cache_marks(xml: &str) -> Vec<usize> {
    fn visit(node: &Element, marks: &mut Vec<usize>) {
        if node.name == "hp:linesegarray" {
            marks.push(node.children[0].attr("vertpos").unwrap().parse::<usize>().unwrap() - 9000);
        }
        for child in &node.children {
            visit(child, marks);
        }
    }
    let mut marks = Vec::new();
    visit(&Element::parse(xml), &mut marks);
    marks.sort_unstable();
    marks
}

// 이것을 실패시키는 것: 성공 칸도 내부 전파 시작점에 포함, 최종 제거에서 성공 문단 차감,
// 첫 변경만 처리, 모든 칸을 한 목록으로 취급, 호출 전체를 일괄 실패로 처리.
#[test]
fn direct_cell_edits_have_independent_seeds_and_removal_precedence() {
    let bytes = input(vec![
        para("본문"),
        table_host(vec![vec![para("첫째"), para("둘째"), para("셋째")], vec![para("옆칸")]]),
        para("뒤"),
        table_host(vec![vec![para("다른 표")]]),
        para("끝"),
    ]);
    // Cache markers: body 0, first cell 1/2/3, sibling 4, host 5, tail 6,
    // second table cell 7, second host 8, last body 9.
    // 한 칸이 커져도 같은 행의 옆 CENTER·BOTTOM 칸은 저장된 lineseg가 변하지 않았다.
    let cases: &[(Option<&str>, &[(usize, usize, usize, &str)], &[usize])] = &[
        (None, &[(1, 0, 1, "가")], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
        (None, &[(1, 0, 0, "가"), (1, 0, 1, "나")], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
        (None, &[(1, 0, 0, "가\n나"), (1, 0, 1, "나")], &[0, 4, 7]),
        (None, &[(1, 0, 0, "가"), (1, 0, 1, "나\n다")], &[0, 1, 4, 7]),
        (None, &[(1, 0, 1, "가"), (1, 1, 0, "나\n다"), (3, 0, 0, "라")], &[0, 1, 2, 3, 7]),
        (Some("가"), &[(1, 0, 1, "나")], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
        (Some("가\n나"), &[(1, 0, 1, "나")], &[1, 2, 3, 4, 7]),
        (Some("가"), &[(1, 0, 1, "나\n다")], &[0, 1, 4, 7]),
        (Some("가\n나"), &[(1, 0, 1, "나\n다")], &[1, 4, 7]),
    ];
    for &(body, edits, expected) in cases {
        for reverse in [false, true] {
            let out = patch_section(&bytes, |section| {
                if let Some(text) = body {
                    section.paragraphs[0].runs[0].content = RunContent::Text(text.into());
                }
                let mut ordered = edits.to_vec();
                if reverse {
                    ordered.reverse();
                }
                // Repeated assignments produce one final normalized change.
                for (host, cell, p, text) in ordered.into_iter().chain(edits.iter().copied()) {
                    cell_paragraph(section, host, cell, p).runs[0].content =
                        RunContent::Text(text.into());
                }
            });
            assert_eq!(cache_marks(&package::section_xml(&out)), expected, "{body:?} {edits:?}");
        }
    }
}

// 이것을 실패시키는 것: 첫 문단의 캐시/스타일로 판정, 변경 슬롯만 합산, 폭 경계의 <=를 <로,
// 정규화 전 글자로 판정, 캐시 없는 변경을 전파에서 제외.
#[test]
fn direct_cell_width_uses_its_own_paragraph_and_all_slots() {
    let mut middle = para("가");
    middle.runs.push(Run::text("나다", CharShapeIndex::new(0)));
    let mut styles = style_store_for_preset("default").unwrap();
    let mut character = styles.char_shape(CharShapeIndex::new(0)).unwrap().clone();
    character.bold = true;
    let first_character = styles.push_char_shape(character);
    let mut definition = styles.para_shape(ParaShapeIndex::new(0)).unwrap().clone();
    definition.condense = 1;
    let first_definition = styles.push_para_shape(definition);
    let mut first = para("첫째");
    first.para_shape_id = first_definition;
    first.runs[0].char_shape_id = first_character;
    let bytes = input_styled(
        vec![para("앞"), table_host(vec![vec![first, middle, para("뒤")]]), para("끝")],
        &styles,
    );
    let xml = package::section_xml(&bytes);
    // The first cell paragraph has disallowed definitions and a different width.
    for (width, text, expected) in [
        (3150, "라", vec![0, 1, 2, 3, 4, 5]),
        (3149, "라", vec![0, 1]),
        (3150, "라\u{0001}", vec![0, 1, 2, 3, 4, 5]),
    ] {
        let original_line = "vertpos=\"9002\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" horzsize=\"42520\"";
        let narrow_line = format!("vertpos=\"9002\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" horzsize=\"{width}\"");
        let narrow = xml.replace(original_line, &narrow_line);
        let base = package::with_section(&bytes, &narrow);
        let decoded = HwpxDecoder::decode(&base).unwrap();
        assert!(cell_paragraph(&mut decoded.document.sections()[0].clone(), 1, 0, 1)
            .layout_cache
            .is_some());
        let out = patch_section(&base, |s| {
            cell_paragraph(s, 1, 0, 1).runs[0].content = RunContent::Text(text.into())
        });
        assert_eq!(
            cache_marks(&package::section_xml(&out)),
            expected,
            "width {width}, text {text:?}"
        );
    }
    let (_, source) = XmlSource::read(&xml).unwrap();
    let cache = source
        .elements()
        .iter()
        .find(|e| e.name() == "linesegarray" && xml[e.outer()].contains("vertpos=\"9002\""))
        .unwrap();
    let mut uncached = xml.clone();
    uncached.replace_range(cache.outer(), "");
    let base = package::with_section(&bytes, &uncached);
    let out = patch_section(&base, |s| {
        cell_paragraph(s, 1, 0, 1).runs[0].content = RunContent::Text("라".into())
    });
    assert_eq!(cache_marks(&package::section_xml(&out)), [0, 1]);
}

// 이것을 실패시키는 것: 빈 필드 끝점을 본문 경로로만 조회, 직접 슬롯 누락/표시값 이중 합산.
#[test]
fn direct_cell_fields_share_patch_and_fill_evidence() {
    let mut middle = field("칸값");
    middle.runs.insert(0, Run::text("앞", CharShapeIndex::new(0)));
    middle.runs.push(Run::text("뒤", CharShapeIndex::new(0)));
    let bytes = input(vec![
        para("앞"),
        table_host(vec![vec![para("첫째"), middle, para("셋째")]]),
        para("끝"),
    ]);
    let bytes = with_width(&bytes, 3150);
    let out = HwpxFiller::fill(
        &bytes,
        &std::collections::BTreeMap::from([("칸값".to_string(), "값".to_string())]),
    )
    .unwrap()
    .bytes;
    let patched = patch_section(&bytes, |s| {
        let RunContent::Control(control) = &mut cell_paragraph(s, 1, 0, 1).runs[1].content else {
            panic!("field")
        };
        let Control::Field { display_text, .. } = control.as_mut() else { panic!("field") };
        *display_text = "값".into();
    });
    assert_eq!(out, patched);
    assert_eq!(cache_marks(&package::section_xml(&out)), [0, 1, 2, 3, 4, 5]);
    let boundary_runs = "</hp:run><hp:run charPrIDRef=\"0\"><hp:t>원래 값</hp:t></hp:run><hp:run charPrIDRef=\"0\">";
    let boundary = package::splice(&bytes, "<hp:t>원래 값</hp:t>", boundary_runs);
    let filled_boundary = HwpxFiller::fill(
        &boundary,
        &std::collections::BTreeMap::from([("칸값".into(), "값".into())]),
    )
    .unwrap()
    .bytes;
    let patched_boundary = patch_section(&boundary, |s| {
        let p = cell_paragraph(s, 1, 0, 1);
        p.runs.iter_mut().find(|r| r.content.as_text() == Some("원래 값")).unwrap().content =
            RunContent::Text("값".into());
    });
    assert_eq!(filled_boundary, patched_boundary);
    assert_eq!(cache_marks(&package::section_xml(&filled_boundary)), [0, 1, 2, 3, 4, 5]);
    let empty = patch_section(&bytes, |s| {
        let RunContent::Control(control) = &mut cell_paragraph(s, 1, 0, 1).runs[1].content else {
            panic!("field")
        };
        let Control::Field { display_text, .. } = control.as_mut() else { panic!("field") };
        *display_text = "\u{1}".into();
    });
    assert_eq!(cache_marks(&package::section_xml(&empty)), [0, 1]);
}

// 이것을 실패시키는 것: 병합 칸 제외, TOP 또는 CENTER 정렬만 허용, 글자처럼 배치한 표만 허용.
#[test]
fn direct_cell_merges_alignment_and_nonwrapping_placement_are_not_extra_gates() {
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    for vertical in [false, true] {
        let merged = TableCell::with_span(
            vec![para("대상")],
            HwpUnit::new(8000).unwrap(),
            if vertical { 1 } else { 2 },
            if vertical { 2 } else { 1 },
        );
        let side = || TableCell::new(vec![para("옆칸")], HwpUnit::new(8000).unwrap());
        let rows = if vertical {
            vec![TableRow::new(vec![merged, side()]), TableRow::new(vec![side()])]
        } else {
            vec![TableRow::new(vec![merged, side()])]
        };
        let host = Paragraph::with_runs(
            vec![Run::table(Table::new(rows), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let base = input(vec![para("앞"), host, para("뒤")]);
        let original = package::section_xml(&base);
        let (_, source_map) = XmlSource::read(&original).unwrap();
        let sublist = source_map.elements().iter().find(|e| e.name() == "subList").unwrap();
        let start_tag = sublist.outer().start..sublist.inner().unwrap().start;
        let original_tag = &original[start_tag.clone()];
        assert!(original_tag.contains("vertAlign=\"CENTER\""));
        for align in ["TOP", "CENTER", "BOTTOM"] {
            for inline in [true, false] {
                let replacement = format!("vertAlign=\"{align}\"");
                let tag = original_tag.replace("vertAlign=\"CENTER\"", &replacement);
                assert!(tag.contains(&replacement));
                assert_eq!(tag == original_tag, align == "CENTER");
                let mut source = original.clone();
                source.replace_range(start_tag.clone(), &tag);
                if !inline {
                    source = source
                        .replace("treatAsChar=\"1\"", "treatAsChar=\"0\"")
                        .replace("textWrap=\"SQUARE\"", "textWrap=\"TOP_AND_BOTTOM\"");
                }
                assert_eq!(
                    Element::parse(&source).find("hp:subList").attr("vertAlign"),
                    Some(align)
                );
                let bytes = package::with_section(&base, &source);
                let out = patch_section(&bytes, |s| {
                    cell_paragraph(s, 1, 0, 0).runs[0].content = RunContent::Text("값".into())
                });
                assert_eq!(
                    cache_marks(&package::section_xml(&out)),
                    if vertical { vec![0, 1, 2, 3, 4, 5] } else { vec![0, 1, 2, 3, 4] },
                    "{vertical} {align} {inline}"
                );
                assert_eq!(
                    package::section_xml(&out),
                    source.replace("<hp:t>대상</hp:t>", "<hp:t>값</hp:t>")
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 공용 원문 검사 무조건 true, patch 또는 fill 한쪽에서만 증거 검사,
// 원문 누락을 Core 기본값으로 승인, lineGrid 검사 생략, raw splice에서 측정하지 않은 이름 통과.
#[test]
fn preserving_patch_and_fill_require_the_same_explicit_source_properties() {
    // 이것을 실패시키는 것: 측정 허용표에서 맑은 고딕 제거.
    for (part, from, to, keep) in [
        ("Contents/section0.xml", "lineGrid=\"0\"", "lineGrid=\"1\"", false),
        ("Contents/section0.xml", "lineGrid=\"0\"", "", false),
        ("Contents/section0.xml", "styleIDRef=\"0\"", "", false),
        ("Contents/section0.xml", "paraPrIDRef=\"0\"", "", false),
        ("Contents/section0.xml", "charPrIDRef=\"0\"", "", false),
        ("Contents/header.xml", "snapToGrid=\"1\"", "", false),
        // Direct paragraph/run references supply the proof values; style type does not.
        // 이것을 실패시키는 것: 스타일 PARA 타입 요구 복원.
        ("Contents/header.xml", "type=\"PARA\"", "type=\"CHAR\"", true),
        ("Contents/header.xml", "<hh:offset hangul=\"0\"", "<hh:offset hangul=\"1\"", false),
        ("Contents/header.xml", "face=\"맑은 고딕\"", "face=\" 맑은 고딕 \"", false),
    ] {
        for is_field in [false, true] {
            let body = if is_field { field("edit") } else { para("원래 값") };
            let base = input(vec![para("앞"), body, para("뒤")]);
            let bytes = package::splice_part(&base, part, from, to);
            let out = if is_field {
                HwpxFiller::fill(
                    &bytes,
                    &std::collections::BTreeMap::from([("edit".into(), "1".into())]),
                )
                .unwrap()
                .bytes
            } else {
                patch(&bytes, "1")
            };
            let xml = package::section_xml(&out);
            assert_eq!(
                vertpos(&xml),
                [Some("9000".into()), keep.then(|| "9001".into()), Some("9002".into())],
                "{from} {to} field={is_field}"
            );
            let tree = Element::parse(&xml);
            assert_eq!(tree.children[1].find("hp:t").text, "1");
            assert_eq!(
                package::part(&out, "Contents/header.xml"),
                package::part(&bytes, "Contents/header.xml")
            );
        }
    }
}

// 이것을 실패시키는 것: 요소 local name만 비교, 루트 별칭을 후보 증거에 전달하지 않기.
#[test]
fn preserving_patch_resolves_element_aliases_from_the_section_root() {
    let base = input(vec![para("앞"), para("가나"), para("뒤")]);
    for (uri, keep) in
        [("http://www.hancom.co.kr/hwpml/2011/paragraph", true), ("urn:foreign", false)]
    {
        let xml = package::section_xml(&base)
            .replace("<hs:sec ", &format!("<hs:sec xmlns:alias=\"{uri}\" "))
            .replace("<hp:p ", "<alias:p ")
            .replace("</hp:p>", "</alias:p>")
            .replace("<hp:run ", "<alias:run ")
            .replace("</hp:run>", "</alias:run>");
        let out = patch(&package::with_section(&base, &xml), "1");
        let tree = Element::parse(&package::section_xml(&out));
        let paragraph = &tree.children[1];
        assert_eq!(paragraph.children.iter().any(|c| c.name == "hp:linesegarray"), keep);
        assert_eq!(paragraph.find("hp:t").text, "1");
    }
}

// 이것을 실패시키는 것: nested_cell_sublist를 항상 Some(false)로 반환.
#[test]
fn nested_cell_patch_and_fill_keep_caches() {
    for click_here in [false, true] {
        let middle = if click_here { field("안쪽 값") } else { para("원래 값") };
        let bytes = input(vec![
            para("본문 앞"),
            table_host(vec![vec![
                table_host(vec![vec![para("안 앞"), middle, para("안 뒤")], vec![para("형제")]]),
                para("밖 뒤"),
            ]]),
            para("본문 뒤"),
        ]);
        let original = package::section_xml(&bytes);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let mut section = decoded.document.sections()[0].clone();
        let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
        let host = cell_paragraph(&mut section, 1, 0, 0);
        let RunContent::Table(inner) = &mut host.runs[0].content else { panic!("inner table") };
        let middle = &mut inner.rows[0].cells[0].paragraphs[1];
        if click_here {
            let RunContent::Control(control) = &mut middle.runs[0].content else { panic!("field") };
            let Control::Field { display_text, .. } = control.as_mut() else { panic!("field") };
            *display_text = "나".into();
        } else {
            middle.runs[0].content = RunContent::Text("나".into());
        }
        let patched = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
            &bytes,
            0,
            &section,
            None,
            Some(&preservation),
        )
        .unwrap();
        assert!(patched.decode_warnings.is_empty(), "{:?}", patched.decode_warnings);
        assert!(patched.encode_warnings.is_empty(), "{:?}", patched.encode_warnings);
        // Full source equality fixes all direct caches, both table sizes and field boundaries.
        assert_eq!(
            package::section_xml(&patched.value),
            original.replace("<hp:t>원래 값</hp:t>", "<hp:t>나</hp:t>")
        );
        assert_eq!(cache_marks(&package::section_xml(&patched.value)), (0..9).collect::<Vec<_>>());
        if click_here {
            let filled = HwpxFiller::fill(
                &bytes,
                &std::collections::BTreeMap::from([("안쪽 값".into(), "나".into())]),
            )
            .unwrap();
            assert_eq!(filled.bytes, patched.value);
        } else {
            let RunContent::Table(inner) =
                &mut cell_paragraph(&mut section, 1, 0, 0).runs[0].content
            else {
                panic!("inner")
            };
            inner.rows[0].cells[0].paragraphs[1].runs[0].content =
                RunContent::Text("가\n나".into());
            let negative = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
                &bytes,
                0,
                &section,
                None,
                Some(&preservation),
            )
            .unwrap();
            assert!(negative.decode_warnings.is_empty());
            assert!(negative.encode_warnings.is_empty());
            assert_eq!(cache_marks(&package::section_xml(&negative.value)), [0, 1, 4]);
            // 이것을 실패시키는 것: 중첩 칸 증명에서 LATIN 글꼴 측정 게이트 생략.
            let header = package::part(&bytes, "Contents/header.xml");
            let start = header.find("lang=\"LATIN\"").unwrap();
            let end = start + header[start..].find("</hh:fontface>").unwrap();
            let latin = &header[start..end];
            let unsupported = package::splice_part(
                &bytes,
                "Contents/header.xml",
                latin,
                &latin.replace("맑은 고딕", "Times New Roman"),
            );
            let out = patch_section(&unsupported, |s| {
                let RunContent::Table(inner) = &mut cell_paragraph(s, 1, 0, 0).runs[0].content
                else {
                    panic!("inner")
                };
                inner.rows[0].cells[0].paragraphs[1].runs[0].content =
                    RunContent::Text("나".into());
            });
            assert_eq!(cache_marks(&package::section_xml(&out)), [0, 1, 4]);
        }
    }
}

// 이것을 실패시키는 것: nested_cell_sublist를 항상 false로 반환, 성공 문단을 최종 제거 집합에서 차감.
#[test]
fn nested_cell_edits_have_independent_seeds() {
    let bytes = input(vec![
        para("본문 앞"),
        table_host(vec![vec![
            table_host(vec![vec![para("안 앞"), para("편집"), para("안 뒤")], vec![para("형제")]]),
            para("밖 뒤"),
        ]]),
        para("본문 뒤"),
    ]);
    // End-tag cache markers: B0=0, I0/I1/I2=1/2/3, J0=4, O0/O1=5/6, B1/B2=7/8.
    let cases: &[(&[(usize, usize, &str)], &[usize])] = &[
        (&[(0, 1, "나")], &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
        (&[(0, 2, "나")], &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
        (&[(1, 0, "나")], &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
        (&[(0, 1, "가\n나")], &[0, 1, 4]),
        (&[(0, 1, "가\n나"), (0, 2, "나")], &[0, 1, 4]),
        (&[(0, 1, "가\n나"), (1, 0, "나")], &[0, 1, 4]),
    ];
    for &(edits, expected) in cases {
        for reverse in [false, true] {
            let out = patch_section(&bytes, |s| {
                let RunContent::Table(inner) = &mut cell_paragraph(s, 1, 0, 0).runs[0].content
                else {
                    panic!("inner")
                };
                let mut ordered = edits.to_vec();
                if reverse {
                    ordered.reverse();
                }
                for (cell, p, text) in ordered {
                    inner.rows[0].cells[cell].paragraphs[p].runs[0].content =
                        RunContent::Text(text.into());
                }
            });
            let xml = package::section_xml(&out);
            assert_eq!(cache_marks(&xml), expected, "{edits:?}, reverse={reverse}");
            let original = Element::parse(&package::section_xml(&bytes));
            let result = Element::parse(&xml);
            // Compare every surviving full array, including all lines and attributes.
            let before = original.descendants("hp:p");
            for (old, new) in before.iter().zip(result.descendants("hp:p")) {
                let old_cache = old.children.iter().find(|c| c.name == "hp:linesegarray").unwrap();
                if let Some(cache) = new.children.iter().find(|c| c.name == "hp:linesegarray") {
                    assert_eq!(cache.attributes, old_cache.attributes);
                    assert_eq!(cache.child_names(), old_cache.child_names());
                    for (line, old_line) in cache.children.iter().zip(&old_cache.children) {
                        assert_eq!(line.attributes, old_line.attributes);
                    }
                }
            }
        }
    }
}

// 이것을 실패시키는 것: nested_cell_sublist에서 바깥 direct_cell_list 검사 생략.
#[test]
fn nested_tables_inside_notes_stay_unproven() {
    let nested = table_host(vec![vec![
        table_host(vec![vec![para("편집"), para("안 꼬리")]]),
        para("밖 꼬리"),
    ]]);
    let note = Paragraph::with_runs(
        vec![Run {
            content: RunContent::Control(Box::new(Control::footnote(vec![
                nested,
                para("각주 꼬리"),
            ]))),
            char_shape_id: CharShapeIndex::new(0),
        }],
        ParaShapeIndex::new(0),
    );
    let bytes = input(vec![para("본문 앞"), note, para("본문 뒤")]);
    let out = patch_section(&bytes, |s| {
        let RunContent::Control(control) = &mut s.paragraphs[1].runs[0].content else {
            panic!("note")
        };
        let Control::Footnote { paragraphs, .. } = control.as_mut() else { panic!("footnote") };
        let RunContent::Table(outer) = &mut paragraphs[0].runs[0].content else { panic!("outer") };
        let RunContent::Table(inner) = &mut outer.rows[0].cells[0].paragraphs[0].runs[0].content
        else {
            panic!("inner")
        };
        inner.rows[0].cells[0].paragraphs[0].runs[0].content = RunContent::Text("나".into());
    });
    let xml = package::section_xml(&out);
    assert_eq!(cache_marks(&xml), [0]);
    let tree = Element::parse(&xml);
    let note = tree.find("hp:footNote");
    let inner = note.find("hp:tbl").find("hp:tc").find("hp:tbl").find("hp:tc").find("hp:subList");
    assert!(inner.children[0].children.iter().all(|c| c.name != "hp:linesegarray"));
    assert_eq!(inner.children[0].find("hp:t").text, "나");
    assert_eq!(
        tree.children[0].child("hp:linesegarray").child("hp:lineseg").attr("vertpos"),
        Some("9000")
    );
}
