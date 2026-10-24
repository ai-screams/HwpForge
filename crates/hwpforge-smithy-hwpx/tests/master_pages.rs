//! 바탕쪽(master page) 디코드·인코드 — 일반 문단 디코더·인코더 재사용 (P6).
//!
//! 한컴 정답 fixture = `tests/fixtures/master_pages/f5*.hwpx` (한컴독스에서 같은
//! 4쪽 본문에 바탕쪽만 만들어 저장): f5a 양쪽 + 마지막 쪽, f5b 임의 쪽 3,
//! f5c 짝수(masterpage0)·홀수(masterpage1), f5d 마지막 쪽 + 앞 바탕쪽과 겹침,
//! f5e 바탕쪽 앞으로, f5f 사각형·그림.
//!
//! 합성 입력은 이 코덱이 만든 패키지의 바탕쪽 part 를 바꿔 만든다 — 이 코덱의
//! 출력이라 편집기 admission 을 통과하므로, 거부가 바탕쪽 때문인지 가릴 수 있다.

use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{MasterPage, MasterPageSpecial, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{
    CellEditError, CellSpec, CellTarget, DecodeWarning, HwpxCellEditor, HwpxDecoder, HwpxDocument,
    HwpxEncoder, HwpxError,
};

// ── helpers ──────────────────────────────────────────────────────

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/master_pages/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}")).read_to_string(&mut out).unwrap();
    out
}

/// `bytes` with entry `name` replaced by `xml`; every other entry copied as is.
fn with_part(bytes: &[u8], name: &str, xml: &str) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let entry_name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        out.start_file(&entry_name, options).unwrap();
        if entry_name == name {
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            std::io::copy(&mut entry, &mut out).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

fn para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// A section with a 2×2 table (so set-cell has a target) and one master page.
fn section_with_master(master_text: &str) -> Section {
    let width = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::new(vec![para("성명")], width),
            TableCell::new(vec![para("")], width),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![para("소속")], width),
            TableCell::new(vec![para("")], width),
        ]),
    ]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let mut section = Section::with_paragraphs(vec![host], PageSettings::a4());
    section.master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![para(master_text)])]);
    section
}

/// This codec's own package: `sections` sections, each with one master page.
fn package(master_text: &str, sections: usize) -> Vec<u8> {
    let mut doc = Document::new();
    for _ in 0..sections {
        doc.add_section(section_with_master(master_text));
    }
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles(), &ImageStore::new())
        .expect("encode")
}

/// `bytes` with master page part `n` given a new root `type` and subList body.
fn with_master(bytes: &[u8], n: usize, root_attrs: &str, body: &str) -> Vec<u8> {
    let name = format!("Contents/masterpage{n}.xml");
    let xml = part(bytes, &name);
    let root_at = xml.find("<masterPage").unwrap();
    let id_at = root_at + xml[root_at..].find(" id=").unwrap();
    let root_end = root_at + xml[root_at..].find('>').unwrap();
    let sub_open = xml.find("<hp:subList").unwrap();
    let sub_body = sub_open + xml[sub_open..].find('>').unwrap() + 1;
    let sub_close = xml.find("</hp:subList>").unwrap();
    let new = format!(
        "{} id=\"masterpage{n}\" {root_attrs}>{}{body}{}",
        &xml[..id_at],
        &xml[root_end + 1..sub_body],
        &xml[sub_close..]
    );
    with_part(bytes, &name, &new)
}

const BOTH: &str = r#"type="BOTH" pageNumber="0" pageDuplicate="0" pageFront="0""#;

fn p(runs: &str) -> String {
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">{runs}</hp:p>"#
    )
}

fn decode(bytes: &[u8]) -> HwpxDocument {
    HwpxDecoder::decode(bytes).expect("decode")
}

fn masters(doc: &HwpxDocument, section: usize) -> &[MasterPage] {
    doc.document.sections()[section].master_pages.as_deref().unwrap_or_default()
}

fn content_lost(doc: &HwpxDocument) -> Vec<(String, String, String)> {
    doc.warnings
        .iter()
        .filter_map(|w| match w {
            DecodeWarning::ContentLost { part, path, kind, .. } => {
                Some((part.clone(), path.to_string(), kind.clone()))
            }
            _ => None,
        })
        .collect()
}

fn reencode(doc: HwpxDocument) -> Vec<u8> {
    let validated = doc.document.validate().expect("validate");
    HwpxEncoder::encode(&validated, &doc.style_store, &doc.image_store).expect("encode")
}

fn cell_spec() -> Vec<CellSpec> {
    vec![CellSpec {
        table: 0,
        target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 1)),
        text: "홍길동".to_string(),
    }]
}

// ── Hancom answer key ────────────────────────────────────────────

// 이것을 실패시키는 것: 범위 표에서 LAST_PAGE·OPTIONAL_PAGE 를 빼기(Both 로
// 떨어짐), `pageNumber`·`pageDuplicate`·`pageFront` 를 읽지 않기, subList 크기를
// 읽지 않기, 바탕쪽을 파일 번호 순서가 아닌 다른 순서로 배정하기.
#[test]
fn hancom_master_pages_decode_with_their_scope_and_area() {
    let a = decode(&fixture("f5a-last-page.hwpx"));
    let [both, last] = masters(&a, 0) else { panic!("f5a has two master pages") };
    assert_eq!((both.apply_page_type, both.special), (ApplyPageType::Both, None));
    assert_eq!(last.special, Some(MasterPageSpecial::LastPage));
    assert_eq!(last.paragraphs[0].text_content(), "마지막");
    assert_eq!((both.text_width.as_i32(), both.text_height.as_i32()), (48190, 67181));

    let b = decode(&fixture("f5b-optional-page3.hwpx"));
    assert_eq!(masters(&b, 0)[0].special, Some(MasterPageSpecial::OptionalPage { page_number: 3 }));

    let c = decode(&fixture("f5c-odd-even.hwpx"));
    let kinds: Vec<_> = masters(&c, 0).iter().map(|m| m.apply_page_type).collect();
    assert_eq!(kinds, [ApplyPageType::Even, ApplyPageType::Odd], "masterpage0 is EVEN");

    let d = decode(&fixture("f5d-overlap.hwpx"));
    assert_eq!(
        masters(&d, 0).iter().map(|m| (m.special, m.page_duplicate)).collect::<Vec<_>>(),
        [(None, false), (Some(MasterPageSpecial::LastPage), true)]
    );

    let e = decode(&fixture("f5e-front.hwpx"));
    assert!(masters(&e, 0)[0].page_front);

    for doc in [&a, &b, &c, &d, &e] {
        assert!(content_lost(doc).is_empty(), "{:?}", doc.warnings);
    }
}

// 이것을 실패시키는 것: 바탕쪽 문단을 문자열 스캐너로 되돌리기(도형·그림 소실),
// 또는 인코더가 글자 아닌 run 을 버리기.
#[test]
fn hancom_master_page_shapes_and_picture_reach_core() {
    let f = decode(&fixture("f5f-shapes.hwpx"));
    let runs = &masters(&f, 0)[0].paragraphs[0].runs;
    assert!(matches!(runs[0].content, RunContent::Control(_)), "{runs:?}");
    assert!(matches!(runs[1].content, RunContent::Image(_)), "{runs:?}");
    assert!(content_lost(&f).is_empty(), "{:?}", f.warnings);

    let a = decode(&fixture("f5a-last-page.hwpx"));
    let both = &masters(&a, 0)[0].paragraphs[0];
    assert!(matches!(both.runs[0].content, RunContent::Control(_)), "rect: {:?}", both.runs);
    assert_eq!(both.text_content(), "양쪽");
}

/// A group picture on a master page is judged for a crop against the
/// package's image sizes like one in the body: Hancom's F3a logo group
/// (three uncropped pictures, `imgClip` = the whole image) placed on a
/// master page with its `imgDim` removed decodes without a loss.
///
/// 이것을 실패시키는 것: 바탕쪽 디코드 문맥에 패키지 그림 픽셀 크기를 넘기지 않기
/// (세 그림이 "크기 모름" 자르기로 `ContentLost` 보고된다).
#[test]
fn a_master_page_group_picture_without_img_dim_uses_the_image_size() {
    let path =
        format!("{}/../../tests/fixtures/shapes/group-f3a-logo.hwpx", env!("CARGO_MANIFEST_DIR"));
    let hancom = std::fs::read(&path).unwrap();
    let body = part(&hancom, "Contents/section0.xml");
    let start = body.find("<hp:container").expect("F3a has a group");
    let end = body.find("</hp:container>").unwrap() + "</hp:container>".len();
    let mut group = body[start..end].to_string();
    assert_eq!(group.matches("<hp:pic ").count(), 3, "premise: three pictures");
    while let Some(at) = group.find("<hp:imgDim ") {
        let close = at + group[at..].find("/>").unwrap() + 2;
        group.replace_range(at..close, "");
    }

    // This codec's package of F3a (BinData kept) with one master page, whose
    // body is then swapped for the Hancom group.
    let mut doc = decode(&hancom);
    doc.document.sections_mut()[0].master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![para("바탕")])]);
    let bytes = reencode(doc);
    let run = format!(r#"<hp:run charPrIDRef="0">{group}</hp:run>"#);
    let back = decode(&with_master(&bytes, 0, BOTH, &p(&run)));

    assert!(content_lost(&back).is_empty(), "{:?}", back.warnings);
    let master = &masters(&back, 0)[0];
    assert!(
        master.paragraphs[0].runs.iter().any(|r| matches!(&r.content, RunContent::Control(c)
            if matches!(c.as_ref(), hwpforge_core::control::Control::Group { .. }))),
        "the group reached the master page: {master:?}"
    );
}

// 이것을 실패시키는 것: 인코더가 루트·subList 값을 하드코딩하기(범위·쪽 번호·
// 겹침·앞으로·크기 중 하나), 또는 글자 아닌 run 을 버리기.
#[test]
fn hancom_master_pages_keep_their_wire_attributes_through_reencode() {
    let root_and_area = |xml: &str| {
        let root = &xml[xml.find("<masterPage").unwrap()..];
        let root = &root[..root.find('>').unwrap()];
        let attrs: Vec<&str> = root
            .split(' ')
            .filter(|a| {
                ["type=", "pageNumber=", "pageDuplicate=", "pageFront="]
                    .iter()
                    .any(|k| a.starts_with(k))
            })
            .collect();
        let sub = &xml[xml.find("<hp:subList").unwrap()..];
        let sub = &sub[..sub.find('>').unwrap()];
        let area: Vec<&str> = sub
            .split(' ')
            .filter(|a| {
                ["vertAlign=", "textWidth=", "textHeight="].iter().any(|k| a.starts_with(k))
            })
            .collect();
        (attrs.join(" "), area.join(" "))
    };
    for name in [
        "f5a-last-page.hwpx",
        "f5b-optional-page3.hwpx",
        "f5c-odd-even.hwpx",
        "f5d-overlap.hwpx",
        "f5e-front.hwpx",
        "f5f-shapes.hwpx",
    ] {
        let original = fixture(name);
        let count = masters(&decode(&original), 0).len();
        let again = reencode(decode(&original));
        for n in 0..count {
            let part_name = format!("Contents/masterpage{n}.xml");
            let (before, after) = (part(&original, &part_name), part(&again, &part_name));
            assert_eq!(root_and_area(&after), root_and_area(&before), "{name} {part_name}");
            for tag in ["<hp:rect", "<hp:pic"] {
                assert_eq!(
                    after.matches(tag).count(),
                    before.matches(tag).count(),
                    "{name} {part_name} {tag}"
                );
            }
        }
    }
    let c = reencode(decode(&fixture("f5c-odd-even.hwpx")));
    assert!(part(&c, "Contents/masterpage0.xml").contains(r#"type="EVEN""#));
}

// ── wire text through the paragraph decoder (G2 §3.1) ────────────

// 이것을 실패시키는 것: 바탕쪽을 문자열 스캐너로 되돌리기 — 엔티티가 풀리지
// 않고(`A&amp;B`), 탭·줄바꿈 마크업이 글자로 들어가고, 두 번째 `<hp:t>` 가
// 사라지고, 글상자 안 문단이 바깥으로 올라온다.
#[test]
fn master_page_text_is_read_like_body_text() {
    let base = package("x", 1);
    let text_of = |body: &str| {
        let doc = decode(&with_master(&base, 0, BOTH, body));
        masters(&doc, 0)[0].paragraphs.clone()
    };

    let paras = text_of(&p(r#"<hp:run charPrIDRef="0"><hp:t>A&amp;B &lt;x&gt;</hp:t></hp:run>"#));
    assert_eq!(paras[0].text_content(), "A&B <x>");

    let paras = text_of(&p(
        r#"<hp:run charPrIDRef="0"><hp:t>L<hp:tab width="4000" leader="0" type="1"/>R<hp:lineBreak/>2줄</hp:t></hp:run>"#,
    ));
    let text = paras[0].text_content();
    assert!(!text.contains("<hp:"), "{text:?}");
    assert!(text.contains('\t') && text.contains('\n'), "{text:?}");

    let paras = text_of(&p(r#"<hp:run charPrIDRef="0"><hp:t>첫</hp:t><hp:t>둘</hp:t></hp:run>"#));
    assert_eq!(paras[0].text_content(), "첫둘");

    // 글상자(사각형 + drawText) 안 문단 뒤에 바깥 글자.
    let rect = part(
        &HwpxEncoder::encode(
            &{
                let rect = hwpforge_core::control::Control::text_box(
                    vec![para("상자 안")],
                    HwpUnit::new(8000).unwrap(),
                    HwpUnit::new(4000).unwrap(),
                );
                let mut doc = Document::new();
                doc.add_section(Section::with_paragraphs(
                    vec![Paragraph::with_runs(
                        vec![Run::control(rect, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    PageSettings::a4(),
                ));
                doc.validate().unwrap()
            },
            &styles(),
            &ImageStore::new(),
        )
        .unwrap(),
        "Contents/section0.xml",
    );
    let rect_xml = &rect[rect.find("<hp:rect").unwrap()..rect.find("</hp:rect>").unwrap() + 10];
    let paras =
        text_of(&p(&format!(r#"<hp:run charPrIDRef="0">{rect_xml}<hp:t>뒤 글자</hp:t></hp:run>"#)));
    assert_eq!(paras.len(), 1, "the text box paragraph stays inside the box");
    assert!(matches!(paras[0].runs[0].content, RunContent::Control(_)), "{:?}", paras[0].runs);
    assert_eq!(paras[0].text_content(), "뒤 글자");
}

// 이것을 실패시키는 것: 빈 `<hp:t/>` 하나뿐인 문단에서 run 을 버리기 — 한컴
// 바탕쪽은 대부분 이런 빈 문단이고, run 0개 문단은 XSD 위반이다.
#[test]
fn an_empty_master_page_paragraph_keeps_its_run_through_reencode() {
    let base = package("x", 1);
    let input = with_master(&base, 0, BOTH, &p(r#"<hp:run charPrIDRef="0"><hp:t/></hp:run>"#));
    let doc = decode(&input);
    assert_eq!(masters(&doc, 0)[0].paragraphs[0].runs.len(), 1);
    let again = reencode(doc);
    assert!(part(&again, "Contents/masterpage0.xml").contains(r#"<hp:run charPrIDRef="0""#));
}

// ── what Core cannot carry is reported, and editors refuse it ────

// 이것을 실패시키는 것: 미지 `type` 의 ContentLost 방출을 지우기 — 경고만으로는
// set-cell 이 입력을 받아들이고 그 바탕쪽을 모든 쪽에 적용되게 다시 쓴다.
#[test]
fn an_unknown_master_page_type_is_content_lost_and_set_cell_refuses_it() {
    let input = with_master(
        &package("x", 1),
        0,
        r#"type="FOO" pageNumber="0" pageDuplicate="0" pageFront="0""#,
        &p(r#"<hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run>"#),
    );
    let doc = decode(&input);
    assert!(doc.warnings.iter().any(|w| matches!(
        w,
        DecodeWarning::UnknownEnumValue { attribute: "masterPage@type", raw, .. } if raw == "FOO"
    )));
    assert_eq!(
        content_lost(&doc),
        [(
            "Contents/masterpage0.xml".to_string(),
            "section[0].masterpage[0]".to_string(),
            "master_page_apply_type".to_string()
        )]
    );

    let err = HwpxCellEditor::set_cells(&input, &cell_spec()).expect_err("must refuse");
    assert!(
        matches!(&err, CellEditError::NotRoundTripSafe { component, .. } if component.contains("CONTENT_LOST")),
        "{err:?}"
    );
}

// 이것을 실패시키는 것: 바탕쪽 문맥 플래그를 무시하기(차트·OLE·미지 요소가
// 경고 없이 사라짐), 또는 본문 보고 갈래를 지우기.
#[test]
fn master_page_chart_ole_and_unknown_elements_are_content_lost() {
    let chart = r#"<hp:switch><hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/ooxmlchart"><hp:chart chartIDRef="Chart/chart1.xml"/></hp:case></hp:switch>"#;
    let body = p(&format!(
        r#"<hp:run charPrIDRef="0">{chart}<hp:ole id="1"/><hp:foo/><hp:t>x</hp:t></hp:run>"#
    ));
    let doc = decode(&with_master(&package("x", 1), 0, BOTH, &body));
    let kinds = content_lost(&doc);
    let at = |kind: &str| {
        (
            "Contents/masterpage0.xml".to_string(),
            "section[0].masterpage[0].npara[0].run[0]".to_string(),
            kind.to_string(),
        )
    };
    assert_eq!(kinds, [at("chart"), at("ole"), at("unknown_element")]);

    // 본문의 같은 요소도 같은 kind 로 보고한다(018 D).
    let base = package("x", 1);
    let section = part(&base, "Contents/section0.xml");
    let section = section.replacen("<hp:tbl", "<hp:ole id=\"1\"/><hp:foo/><hp:tbl", 1);
    let body_doc = decode(&with_part(&base, "Contents/section0.xml", &section));
    let body_at = |kind: &str| {
        (
            "Contents/section0.xml".to_string(),
            "section[0].para[0].run[0]".to_string(),
            kind.to_string(),
        )
    };
    assert_eq!(content_lost(&body_doc), [body_at("ole"), body_at("unknown_element")]);
}

// 이것을 실패시키는 것: 바탕쪽 경고의 part 를 구역 part 로 고정하기, 또는
// 바탕쪽 번호를 구역 안 순번으로 매기기.
#[test]
fn a_second_section_master_page_warning_names_its_own_part() {
    let input = with_master(
        &package("x", 2),
        1,
        BOTH,
        &p(r#"<hp:run charPrIDRef="0"><hp:ole id="1"/><hp:t>x</hp:t></hp:run>"#),
    );
    let doc = decode(&input);
    assert_eq!(masters(&doc, 1).len(), 1);
    assert_eq!(
        content_lost(&doc),
        [(
            "Contents/masterpage1.xml".to_string(),
            "section[1].masterpage[1].npara[0].run[0]".to_string(),
            "ole".to_string()
        )]
    );
}

// 이것을 실패시키는 것: OPTIONAL_PAGE 밖의 0 아닌 `pageNumber` 를 조용히 버리기,
// 해석 불가 `pageNumber` 를 0 으로 받기, `xs:boolean` 의 `true` 를 거부하거나
// 미지 값을 조용히 false 로 받기, subList `vertAlign` 을 읽지 않기.
#[test]
fn master_page_root_attributes_core_cannot_hold_are_reported() {
    let base = package("x", 1);
    let body = p(r#"<hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run>"#);
    let kinds = |attrs: &str| {
        let doc = decode(&with_master(&base, 0, attrs, &body));
        let lost: Vec<String> = content_lost(&doc).into_iter().map(|(_, _, k)| k).collect();
        (masters(&doc, 0)[0].clone(), lost)
    };

    let (_, lost) = kinds(r#"type="BOTH" pageNumber="4" pageDuplicate="0" pageFront="0""#);
    assert_eq!(lost, ["master_page_page_number"]);
    let (_, lost) = kinds(r#"type="OPTIONAL_PAGE" pageNumber="x" pageDuplicate="0" pageFront="0""#);
    assert_eq!(lost, ["master_page_page_number"]);
    let (mp, lost) = kinds(r#"type="BOTH" pageNumber="0" pageDuplicate="true" pageFront="yes""#);
    assert!(mp.page_duplicate && !mp.page_front);
    assert_eq!(lost, ["master_page_flag"]);

    // 한컴 fixture 는 모두 TOP 이라 세로 정렬은 여기서 따로 고정한다.
    let input = with_master(&base, 0, BOTH, &body);
    let xml = part(&input, "Contents/masterpage0.xml").replacen(
        r#"vertAlign="TOP""#,
        r#"vertAlign="BOTTOM""#,
        1,
    );
    let doc = decode(&with_part(&input, "Contents/masterpage0.xml", &xml));
    assert_eq!(masters(&doc, 0)[0].vert_align, hwpforge_foundation::VerticalAlign::Bottom);
}

// 이것을 실패시키는 것: 바탕쪽 글자의 엔티티를 풀지 않기 — 인코더가 `&` 를 다시
// escape 해 admission 의 두 디코드가 달라지고(`A&amp;B` 대 `A&amp;amp;B`),
// 이 코덱이 만든 문서의 set-cell 이 거짓으로 거부된다.
#[test]
fn set_cell_accepts_this_codecs_own_master_page_with_an_ampersand() {
    let input = package("A&B <x>", 1);
    assert!(part(&input, "Contents/masterpage0.xml").contains("A&amp;B &lt;x&gt;"));
    let edited = HwpxCellEditor::set_cells(&input, &cell_spec()).expect("set-cell");
    let doc = decode(&edited.bytes);
    assert_eq!(masters(&doc, 0)[0].paragraphs[0].text_content(), "A&B <x>");
}

// 이것을 실패시키는 것: 바탕쪽 part 의 serde 실패를 삼키고 빈 바탕쪽으로 진행하기.
#[test]
fn a_malformed_master_page_fails_the_decode_naming_the_part() {
    let base = package("x", 1);
    let broken = with_part(&base, "Contents/masterpage0.xml", "<masterPage><hp:subList>");
    match HwpxDecoder::decode(&broken) {
        Err(HwpxError::XmlParse { file, .. }) => assert_eq!(file, "Contents/masterpage0.xml"),
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: subList 미보존 속성의 보고를 지우기, 또는 기본값까지
// 보고하기(한컴 fixture 가 경고를 내게 됨 — 위 디코드 테스트가 잡음).
#[test]
fn master_page_sublist_attributes_core_cannot_hold_are_reported() {
    let input = with_master(
        &package("x", 1),
        0,
        BOTH,
        &p(r#"<hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run>"#),
    );
    let xml = part(&input, "Contents/masterpage0.xml")
        .replacen(r#"textDirection="HORIZONTAL""#, r#"textDirection="VERTICAL""#, 1)
        .replacen(r#"hasNumRef="0""#, r#"hasNumRef="1""#, 1);
    let doc = decode(&with_part(&input, "Contents/masterpage0.xml", &xml));
    let lost: Vec<_> = doc
        .warnings
        .iter()
        .filter_map(|w| match w {
            DecodeWarning::ContentLost { kind, reason, .. } => Some((kind.clone(), reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].0, "master_page_sublist");
    assert!(lost[0].1.contains(r#"textDirection="VERTICAL""#) && lost[0].1.contains("hasNumRef=1"));
}

// 이것을 실패시키는 것: 인코더의 `Fill::None` 갈래를 지우기 — 바탕쪽 사각형이
// 흰 채우기로 써지고, 재디코드 값이 달라져 set-cell admission 이 거부한다.
#[test]
fn a_master_page_shape_without_fill_stays_without_fill() {
    use hwpforge_core::control::{Control, Fill, ShapeStyle};
    let mut rect = Control::rect(HwpUnit::new(5000).unwrap(), HwpUnit::new(3000).unwrap()).unwrap();
    if let Control::Rect { style, .. } = &mut rect {
        *style = Some(ShapeStyle { fill: Some(Fill::None), ..Default::default() });
    }
    let mut section = section_with_master("x");
    section.master_pages.as_mut().unwrap()[0].paragraphs = vec![Paragraph::with_runs(
        vec![Run::control(rect, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )];
    let mut doc = Document::new();
    doc.add_section(section);
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles(), &ImageStore::new())
        .expect("encode");
    let xml = part(&bytes, "Contents/masterpage0.xml");
    assert!(xml.contains("<hp:rect") && !xml.contains("<hc:fillBrush"), "{xml}");

    let edited = HwpxCellEditor::set_cells(&bytes, &cell_spec()).expect("admission passes");
    let again = decode(&edited.bytes);
    let RunContent::Control(c) = &masters(&again, 0)[0].paragraphs[0].runs[0].content else {
        panic!("rect run")
    };
    let Control::Rect { style, .. } = c.as_ref() else { panic!("{c:?}") };
    assert_eq!(style.as_ref().and_then(|s| s.fill.clone()), Some(Fill::None));
}
