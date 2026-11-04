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
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let bytes = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
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
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let mut section = decoded.document.sections()[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(bytes, 0, &section).unwrap();
    section.paragraphs[1].runs[0].content = RunContent::Text(text.into());
    HwpxPatcher::patch_section_preserving(bytes, 0, &section, None, Some(&preservation)).unwrap()
}

/// 이것을 실패시키는 것: patch에서 기존 stale_caches 호출, 편집 캐시까지 보존,
/// patch에서 필드 범위 투영 호출 삭제.
#[test]
fn body_edit_keeps_plain_tail_verbatim() {
    let bytes = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    let original = caches(&package::section_xml(&bytes));
    for text in ["가".to_string(), format!("원래 값 {}", "가".repeat(70)), "원래 값\n가".into()]
    {
        let out = patch(&bytes, &text);
        let xml = package::section_xml(&out);
        let expected = [Some("9000".to_owned()), None, Some("9002".to_owned())];
        assert_eq!(vertpos(&xml), expected);
        let kept = caches(&xml);
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
    let values = std::collections::BTreeMap::from([("edit".to_string(), "길어진 값".to_string())]);
    let out = HwpxFiller::fill(&bytes, &values).unwrap();
    let out_xml = package::section_xml(&out.bytes);
    assert_eq!(vertpos(&out_xml), [None, Some("9001".to_owned())]);
    assert_eq!(
        body_spans(&out_xml)[1],
        body_spans(&xml)[1],
        "the tail paragraph is kept byte for byte"
    );
}

/// 이것을 실패시키는 것: 정규화 전 요청 슬롯을 변경으로 간주.
#[test]
fn normalized_noop_keeps_package_bytes() {
    let bytes = input(vec![para("앞"), para("원래 값"), para("뒤")]);
    assert_eq!(patch(&bytes, "원래 값"), bytes);
    assert_eq!(patch(&bytes, "원래\u{0} 값"), bytes);
}

/// The rebuild path (`encode_with_base`: re-encode, then carry unchanged
/// caches) keeps its conservative rule: the edited paragraph and its tail
/// lose their caches, while the preserving patch keeps the tail.
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
    assert_eq!(vertpos(&preserved), [Some("9000".to_owned()), None, Some("9002".to_owned())]);
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
