//! 필드 본문과 범위 경계의 공개 디코드·인코드 왕복.
use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, EncodeOptions, HwpxDecoder, HwpxEncoder};

fn begin(id: u32, kind: &str) -> String {
    format!(
        r#"<hp:ctrl><hp:fieldBegin id="{id}" type="{kind}" name="본문" editable="1" dirty="0" zorder="-1" fieldid="627469685" metaTag=""><hp:parameters cnt="5" name=""><hp:integerParam name="Prop">8</hp:integerParam><hp:stringParam name="Command">=SUM(A1:A2)??%g,;;3</hp:stringParam><hp:stringParam name="Formula">=SUM(A1:A2)</hp:stringParam><hp:stringParam name="ResultFormat">%g,</hp:stringParam><hp:stringParam name="LastResult">3</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl>"#
    )
}
fn end(id: u32) -> String {
    format!(r#"<hp:ctrl><hp:fieldEnd beginIDRef="{id}" fieldid="627469685"/></hp:ctrl>"#)
}
fn run(body: &str) -> String {
    format!(r#"<hp:run charPrIDRef="0">{body}</hp:run>"#)
}
fn para(body: &str, lines: &[u32]) -> String {
    let cache = if lines.is_empty() {
        String::new()
    } else {
        format!("<hp:linesegarray>{}</hp:linesegarray>", lines.iter().map(|n| format!(r#"<hp:lineseg textpos="{n}" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/>"#)).collect::<String>())
    };
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">{body}{cache}</hp:p>"#
    )
}
fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}
fn package(body: &str) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("앞", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let styles = style_store_for_preset("default").unwrap();
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name() == "Contents/section0.xml" {
            let xml = String::from_utf8(bytes).unwrap();
            bytes = xml.replace("</hs:sec>", &format!("{body}</hs:sec>")).into_bytes();
        }
        out.start_file(
            entry.name(),
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}
fn containers(body: &str) -> [String; 4] {
    let rect = format!(
        r#"<hp:rect id="1"><hp:orgSz width="10000" height="5000"/><hp:curSz width="10000" height="5000"/><hc:pt0 x="0" y="0"/><hc:pt1 x="10000" y="0"/><hc:pt2 x="10000" y="5000"/><hc:pt3 x="0" y="5000"/><hp:sz width="10000" height="5000"/><hp:pos treatAsChar="1"/><hp:drawText><hp:subList>{body}</hp:subList></hp:drawText></hp:rect>"#
    );
    let cell = format!(
        r#"<hp:tbl rowCnt="1" colCnt="1"><hp:sz width="10000" height="5000"/><hp:tr><hp:tc><hp:subList>{body}</hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="10000" height="5000"/></hp:tc></hp:tr></hp:tbl>"#
    );
    let group = format!(
        r#"<hp:container id="2"><hp:sz width="10000" height="5000"/>{rect}</hp:container>"#
    );
    [body.to_string(), para(&run(&cell), &[]), para(&run(&rect), &[]), para(&run(&group), &[])]
}

fn visible(p: &Paragraph) -> String {
    p.runs
        .iter()
        .map(|r| {
            if let Some(t) = r.content.plain_text() {
                return t.into_owned();
            }
            if let Some(Control::Memo { anchor_runs, .. }) = r.content.as_control() {
                return anchor_runs.iter().filter_map(|r| r.content.plain_text()).collect();
            }
            if let Some(Control::Field { display_text, .. }) = r.content.as_control() {
                return display_text.clone();
            }
            String::new()
        })
        .collect()
}
fn roundtrip(body: &str, expected: &[&str], cached: bool) {
    let source = package(body);
    let d = HwpxDecoder::decode(&source).unwrap();
    assert!(!d.warnings.iter().any(|w| w.is_content_loss()), "{:?}", d.warnings);
    let mut before = Vec::new();
    d.document.for_each_paragraph(|p| {
        let text = visible(p);
        if expected.contains(&text.as_str()) {
            assert_eq!(p.layout_cache.is_some(), cached, "{text}: {:?}", d.warnings);
            before.push((text, p.runs.clone(), p.layout_cache.clone()));
        }
    });
    assert_eq!(before.len(), expected.len(), "{before:?}");
    let encode = |d: hwpforge_smithy_hwpx::HwpxDocument| {
        HwpxEncoder::encode_with_diagnostics(
            &d.document.validate().unwrap(),
            &d.style_store,
            &d.image_store,
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap()
    };
    let first = encode(d);
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let d = HwpxDecoder::decode(&first.bytes).unwrap();
    let mut after = Vec::new();
    d.document.for_each_paragraph(|p| {
        let text = visible(p);
        if expected.contains(&text.as_str()) {
            after.push((text, p.runs.clone(), p.layout_cache.clone()));
        }
    });
    assert_eq!(before, after, "Core body and S22 boundary");
    let second = encode(d);
    assert_eq!(section_xml(&first.bytes), section_xml(&second.bytes), "normalized XML fixed point");
    assert_eq!(first.bytes, second.bytes, "normalized package byte fixed point");
}

/// 이것을 실패시키는 것: 열린 시작을 접어 run 끝에서 본문을 버리기.
#[test]
fn open_start_body_roundtrips() {
    for cached in [false, true] {
        let lines: &[u32] = if cached { &[0, 8, 10] } else { &[] };
        let body = para(&run(&(begin(7, "CLICK_HERE") + "<hp:t>본문값</hp:t>")), lines)
            + &para(
                &run(&("<hp:t>다음</hp:t>".to_string() + &end(7))),
                if cached { &[0, 1] } else { &[] },
            );
        for wrapped in containers(&body) {
            roundtrip(&wrapped, &["본문값", "다음"], cached);
        }
    }
}

/// 이것을 실패시키는 것: 두 번째 begin 에서 앞 본문을 clear 하기.
#[test]
fn nested_start_body_roundtrips() {
    for cached in [false, true] {
        let body = para(
            &run(&(begin(7, "CLICK_HERE")
                + "<hp:t>바깥</hp:t>"
                + &begin(8, "CLICK_HERE")
                + "<hp:t>안쪽</hp:t>")),
            if cached { &[0, 8, 18, 19] } else { &[] },
        ) + &para(
            &run(&(end(8) + &end(7) + "<hp:t>끝</hp:t>")),
            if cached { &[0, 8, 16] } else { &[] },
        );
        for wrapped in containers(&body) {
            roundtrip(&wrapped, &["바깥안쪽", "끝"], cached);
        }
    }
}

/// 이것을 실패시키는 것: FORMULA·BOOKMARK 를 본문 없는 control 로 접기,
/// MEMO anchor_runs 또는 본문 보존 목록의 Memo 를 삭제하기.
#[test]
fn closed_field_bodies_roundtrip_in_all_containers() {
    for kind in ["FORMULA", "BOOKMARK", "MEMO"] {
        for cached in [false, true] {
            let lines: &[u32] = if !cached {
                &[]
            } else if kind == "MEMO" {
                &[0, 20]
            } else {
                &[0, 8, 10, 11, 19, 20]
            };
            let body = para(
                &(run(&(begin(7, kind) + "<hp:t>본문값</hp:t>" + &end(7)))
                    + &run("<hp:t>끝글</hp:t>")),
                lines,
            );
            for wrapped in containers(&body) {
                roundtrip(&wrapped, &["본문값끝글"], cached);
            }
        }
    }
}

/// 이것을 실패시키는 것: 본문과 경계를 보존하면서도 잘못된 줄 좌표를 승인하기.
#[test]
fn unplaceable_cache_is_dropped_locally() {
    let broken = para(&run(&(begin(7, "FORMULA") + "<hp:t>본문값</hp:t>" + &end(7))), &[0, 999]);
    let intact = para(&run("<hp:t>옆문단</hp:t>"), &[0, 1]);
    let d = HwpxDecoder::decode(&package(&(broken + &intact))).unwrap();
    assert!(d.document.sections()[0].paragraphs[1].layout_cache.is_none());
    assert!(d.document.sections()[0].paragraphs[2].layout_cache.is_some());
    assert!(!d.warnings.iter().any(|w| w.is_content_loss()));
    assert!(format!("{:?}", d.warnings).contains("LayoutCacheDropped"));
}

/// 이것을 실패시키는 것: beginIDRef 를 확인하지 않고 아무 끝과 접기,
/// 일반 본문을 String 으로 평탄화해 탭의 원문 속성을 없애기.
#[test]
fn mismatched_end_and_inline_body_stay_in_wire_order() {
    let source = para(
        &run(&(begin(7, "CLICK_HERE")
            + r#"<hp:t>본문<hp:tab width="1200" leader="2" type="1"/>값</hp:t>"#
            + &end(8))),
        &[0, 8, 18],
    );
    roundtrip(&source, &["본문\t값"], true);
}

/// 이것을 실패시키는 것: 파라미터 종류·값을 합성하거나 인코더에서 생략하기.
#[test]
fn formula_parameters_are_carried_verbatim() {
    use hwpforge_core::control::{FieldBoundary, FieldParameter};
    let source = para(&run(&(begin(7, "FORMULA") + "<hp:t>표시값</hp:t>" + &end(7))), &[]);
    let d = HwpxDecoder::decode(&package(&source)).unwrap();
    let Some(Control::FieldBoundary { boundary: FieldBoundary::Start(start) }) =
        d.document.sections()[0].paragraphs[1].runs[0].content.as_control()
    else {
        panic!("range start")
    };
    assert_eq!(start.parameters.len(), 5);
    assert!(start.parameters.iter().any(|p| matches!(p, FieldParameter::String { name, value, .. } if name == "Formula" && value == "=SUM(A1:A2)")));
    assert!(start.parameters.iter().any(|p| matches!(p, FieldParameter::String { name, value, .. } if name == "LastResult" && value == "3")));
    assert!(start.parameters.iter().any(
        |p| matches!(p, FieldParameter::Integer { name, value } if name == "Prop" && value == "8")
    ));
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let xml = section_xml(&bytes);
    assert!(xml.contains(r#"<hp:stringParam name="Formula">=SUM(A1:A2)</hp:stringParam>"#));
    assert!(xml.contains(r#"<hp:stringParam name="LastResult">3</hp:stringParam>"#));
    assert!(xml.contains("<hp:t>표시값</hp:t>"));
    assert!(xml.find("type=\"FORMULA\"").unwrap() < xml.find("<hp:t>표시값").unwrap());
    assert!(xml.find("<hp:t>표시값").unwrap() < xml.find("<hp:fieldEnd").unwrap());
}

/// 이것을 실패시키는 것: 열린 MEMO 를 지원 목록에서 빼기, subList 운반 삭제.
#[test]
fn open_memo_carries_annotation_and_body() {
    for cached in [false, true] {
        let note = para(&run("<hp:t>메모내용</hp:t>"), if cached { &[0, 2] } else { &[] });
        let start = begin(7, "MEMO").replace(
            "</hp:fieldBegin>",
            &format!("<hp:subList>{note}</hp:subList></hp:fieldBegin>"),
        );
        let body =
            para(&run(&(start + "<hp:t>앵커</hp:t>")), if cached { &[0, 8, 9] } else { &[] })
                + &para(
                    &run(&("<hp:t>계속</hp:t>".to_string() + &end(7))),
                    if cached { &[0, 1] } else { &[] },
                );
        for wrapped in containers(&body) {
            roundtrip(&wrapped, &["메모내용", "앵커", "계속"], cached);
        }
    }
}

/// 이것을 실패시키는 것: 원문 시작 id 와 새 필드 id 의 충돌 검사를 삭제하기.
#[test]
fn carried_id_does_not_collide_with_a_folded_field() {
    let body = para(
        &(run(&(begin(1_000_000_001, "CLICK_HERE") + "<hp:t>범위</hp:t>"))
            + &run(&(begin(8, "CLICK_HERE") + "<hp:t>값</hp:t>" + &end(8)))),
        &[],
    ) + &para(&run(&end(1_000_000_001)), &[]);
    let d = HwpxDecoder::decode(&package(&body)).unwrap();
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let xml = section_xml(&bytes);
    let ids: Vec<_> =
        xml.split("<hp:fieldBegin id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    for id in ids {
        assert!(xml.contains(&format!(r#"beginIDRef="{id}""#)));
    }
    let d = HwpxDecoder::decode(&bytes).unwrap();
    let again =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    assert_eq!(xml, section_xml(&again));
}

/// 이것을 실패시키는 것: 가변 문단 순회에서 범위 메모 문단을 제외하기.
#[test]
fn annotation_edit_reaches_nested_text() {
    let note = para(&run("<hp:t>메모내용</hp:t>"), &[]);
    let start = begin(7, "MEMO")
        .replace("</hp:fieldBegin>", &format!("<hp:subList>{note}</hp:subList></hp:fieldBegin>"));
    let body = para(&run(&(start + "<hp:t>앵커</hp:t>")), &[]) + &para(&run(&end(7)), &[]);
    let mut d = HwpxDecoder::decode(&package(&body)).unwrap();
    let mut edited = 0;
    d.document.for_each_paragraph_mut(|p| {
        if p.text_content() == "메모내용" {
            p.runs = vec![Run::text("수정한메모", CharShapeIndex::new(0))];
            edited += 1;
        }
    });
    assert_eq!(edited, 1);
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let xml = section_xml(&bytes);
    assert!(xml.contains("수정한메모"));
    assert!(!xml.contains("메모내용"));
    assert!(xml.contains("앵커"));
}

/// 이것을 실패시키는 것: 파라미터의 스칼라 종류·공백 속성·XML escape 를 버리기.
#[test]
fn range_metadata_keeps_typed_parameters_and_escaping() {
    use hwpforge_core::control::{FieldBoundary, FieldParameter};
    let start = begin(7, "FORMULA")
        .replace("cnt=\"5\"", "cnt=\"7\"")
        .replace("</hp:parameters>", r#"<hp:booleanParam name="flag">0</hp:booleanParam><hp:stringParam name="extra" xml:space="preserve"> x &amp; &lt;y&gt; </hp:stringParam></hp:parameters>"#);
    let body = para(&run(&(start + "<hp:t>표시</hp:t>" + &end(7))), &[]);
    roundtrip(&body, &["표시"], false);
    let d = HwpxDecoder::decode(&package(&body)).unwrap();
    let Some(Control::FieldBoundary { boundary: FieldBoundary::Start(start) }) =
        d.document.sections()[0].paragraphs[1].runs[0].content.as_control()
    else {
        panic!("start")
    };
    assert!(start.parameters.iter().any(
        |p| matches!(p, FieldParameter::Boolean { name, value } if name == "flag" && value == "0")
    ));
    assert!(start.parameters.iter().any(|p| matches!(p, FieldParameter::String { name, value, xml_space } if name == "extra" && value == " x & <y> " && xml_space == "preserve")));
}
