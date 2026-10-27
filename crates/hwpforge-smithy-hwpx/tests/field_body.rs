#[path = "support/package.rs"]
mod package;
use package::section_xml_unchecked as section_xml;
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
fn memo_begin(id: u32) -> String {
    format!(
        r#"<hp:ctrl><hp:fieldBegin id="{id}" type="MEMO" name="" editable="1" dirty="1" zorder="1" fieldid="627469685" metaTag=""><hp:parameters cnt="7" name=""><hp:integerParam name="Prop">0</hp:integerParam><hp:integerParam name="Number">0</hp:integerParam><hp:stringParam name="Command"></hp:stringParam><hp:stringParam name="ID"></hp:stringParam><hp:stringParam name="Author"></hp:stringParam><hp:stringParam name="MemoShapeIDRef">65535</hp:stringParam><hp:stringParam name="CreateDateTime"></hp:stringParam></hp:parameters><hp:subList></hp:subList></hp:fieldBegin></hp:ctrl>"#
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
        if entry.name().expect("ZIP entry name") == "Contents/section0.xml" {
            let xml = String::from_utf8(bytes).unwrap();
            bytes = xml.replace("</hs:sec>", &format!("{body}</hs:sec>")).into_bytes();
        }
        out.start_file(
            entry.name().expect("ZIP entry name").to_string(),
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
    assert_eq!(before, after, "Core body and field visibility boundary");
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
            // Folded memo anchors have no ordinary Core text coordinates.
            let lines: &[u32] = if !cached {
                &[]
            } else if kind == "MEMO" {
                &[0, 19, 20]
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
    let d = HwpxDecoder::decode(&package(&source)).unwrap();
    assert_eq!(visible(&d.document.sections()[0].paragraphs[1]), "본문\t값");
    assert!(d.warnings.iter().any(|w| w.code() == "INVALID_FIELD_BOUNDARY"));
    assert!(d.document.validate().is_err());
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

/// 이것을 실패시키는 것: 경계형 누름틀을 목록에서 빼거나 별도 wire 스캐너로 세기.
#[test]
fn inventory_keeps_boundary_fields_while_filling_a_folded_neighbor() {
    use hwpforge_smithy_hwpx::{FillError, HwpxFiller};
    let a = begin(7, "CLICK_HERE").replace("name=\"본문\"", "name=\"A\"");
    let b = begin(8, "CLICK_HERE").replace("name=\"본문\"", "name=\"B\"");
    let bytes = package(&para(
        &(run(&(a
            + "<hp:t>(   )</hp:t><hp:ctrl><hp:bookmark name=\"point\"/></hp:ctrl>"
            + &end(7)))
            + &run(&(b + "<hp:t>old</hp:t>" + &end(8)))),
        &[],
    ));
    let fields = HwpxFiller::list_fields(&bytes).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name.as_deref(), Some("A"));
    assert!(!fields[0].fillable);
    assert!(matches!(
        HwpxFiller::fill(&bytes, &[("A".into(), "new".into())].into()),
        Err(FillError::UnfillableField { .. })
    ));
    let filled = HwpxFiller::fill(&bytes, &[("B".into(), "new".into())].into()).unwrap();
    assert_eq!(HwpxFiller::list_fields(&filled.bytes).unwrap()[1].current, "new");
    let doc = HwpxDecoder::decode(&bytes).unwrap().document;
    assert!(hwpforge_smithy_hwpx::stamp::plan(&doc).is_empty());
}

/// 이것을 실패시키는 것: 문단 간 누름틀을 발견하지 않거나 편집 가능으로 표시하기.
#[test]
fn cross_paragraph_field_is_visible_but_explicitly_unfillable() {
    use hwpforge_smithy_hwpx::{FillError, HwpxFiller};
    let bytes = package(
        &(para(&run(&(begin(7, "CLICK_HERE") + "<hp:t>first</hp:t>")), &[])
            + &para(&run(&("<hp:t>second</hp:t>".to_string() + &end(7))), &[])),
    );
    let fields = HwpxFiller::list_fields(&bytes).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name.as_deref(), Some("본문"));
    assert_eq!(fields[0].current, "firstsecond");
    assert!(!fields[0].fillable);
    assert!(matches!(
        HwpxFiller::fill(&bytes, &[("본문".into(), "new".into())].into()),
        Err(FillError::UnfillableField { .. })
    ));
}

/// 이것을 실패시키는 것: 자동 본문을 텍스트 슬롯으로 노출하거나 PAGE_NUM 을 누름틀로 접기.
#[test]
fn automatic_boundary_values_are_not_patch_slots() {
    use hwpforge_smithy_hwpx::HwpxPatcher;
    for kind in ["FORMULA", "SUMMERY", "DATE", "TIME", "PATH", "CROSSREF", "PAGE_NUM"] {
        for split in [false, true] {
            let body = if split {
                run(&begin(7, kind)) + &run("<hp:t>3</hp:t>") + &run(&end(7))
            } else {
                run(&(begin(7, kind) + "<hp:t>3</hp:t>" + &end(7)))
            };
            let bytes = package(&para(&body, &[]));
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            assert!(!decoded.warnings.iter().any(|w| w.is_content_loss()), "{kind}");
            assert!(
                hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap().is_empty(),
                "automatic {kind} must never become ClickHere"
            );
            let mut section = decoded.document.sections()[0].clone();
            let preservation =
                HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
            assert!(!preservation.text_slots.iter().any(|s| s.original_text == "3"), "{kind}");
            let mut changed = false;
            for run in &mut section.paragraphs[1].runs {
                let text = match &mut run.content {
                    hwpforge_core::RunContent::Text(text) => Some(text),
                    hwpforge_core::RunContent::Control(c) => match c.as_mut() {
                        Control::Field { display_text, .. }
                        | Control::UnknownSummary { display_text, .. }
                        | Control::PathField { display_text, .. }
                        | Control::DateCodeField { display_text, .. }
                        | Control::CrossRef { display_text, .. } => Some(display_text),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(text) = text {
                    if text == "3" {
                        *text = "999".into();
                        changed = true;
                    }
                }
            }
            assert!(changed, "{kind} retains its displayed value");
            // 이것을 실패시키는 것: 보호 필드 값 변경 거부를 `PatchError::Rejected` 로 되돌리기.
            let error = HwpxPatcher::patch_section_preserving(
                &bytes,
                0,
                &section,
                None,
                Some(&preservation),
            )
            .unwrap_err();
            assert!(
                matches!(error, hwpforge_smithy_hwpx::PatchError::ProtectedFieldRange { .. }),
                "{kind}: {error}"
            );
        }
    }
}

/// 이것을 실패시키는 것: 단일 값 누름틀의 표현에 따라 편집 정책을 다르게 적용하기.
#[test]
fn single_value_boundary_and_folded_fields_share_fill_policy() {
    use hwpforge_smithy_hwpx::HwpxFiller;
    for boundary in [false, true] {
        let body = if boundary {
            run(&begin(7, "CLICK_HERE")) + &run("<hp:t>old</hp:t>") + &run(&end(7))
        } else {
            run(&(begin(7, "CLICK_HERE") + "<hp:t>old</hp:t>" + &end(7)))
        };
        let bytes = package(&para(&body, &[]));
        assert!(HwpxFiller::list_fields(&bytes).unwrap()[0].fillable);
        let outcome = HwpxFiller::fill(&bytes, &[("본문".into(), "new".into())].into()).unwrap();
        assert_eq!(HwpxFiller::list_fields(&outcome.bytes).unwrap()[0].current, "new");
    }
}

/// 이것을 실패시키는 것: 불완전 범위를 Validated 로 승인하거나 서로 다른 흐름끼리 짝짓기.
#[test]
fn invalid_boundaries_are_recovery_only_in_every_flow() {
    let cases = [
        begin(7, "FORMULA") + "<hp:t>keep</hp:t>" + &end(8),
        end(7),
        end(7) + &begin(7, "FORMULA"),
        begin(7, "FORMULA") + &begin(7, "FORMULA") + &end(7),
        begin(7, "FORMULA") + &end(7) + &begin(7, "FORMULA") + &end(7),
    ];
    for case in cases {
        for wrapped in containers(&para(&run(&case), &[])) {
            let bytes = package(&wrapped);
            let d = HwpxDecoder::decode(&bytes).unwrap();
            assert!(d.warnings.iter().any(|w| w.code() == "INVALID_FIELD_BOUNDARY"));
            assert!(d.document.validate().is_err());
        }
    }
    // An empty body is a valid clear, not an unmatched range.
    let bytes = package(&para(&run(&(begin(7, "FORMULA") + &end(7))), &[]));
    assert!(HwpxDecoder::decode(&bytes).unwrap().document.validate().is_ok());
}

/// 이것을 실패시키는 것: 닫힌 메모를 문자열로 접거나 메타데이터를 기본값으로 바꾸기.
#[test]
fn closed_memo_keeps_tab_attributes_and_metadata() {
    let source = memo_begin(7)
        .replace(
            "<hp:stringParam name=\"Author\"></hp:stringParam>",
            "<hp:stringParam name=\"Author\">writer&#13;&#9;&#10;&lt;&amp;</hp:stringParam>",
        )
        .replace(
            "<hp:stringParam name=\"CreateDateTime\"></hp:stringParam>",
            "<hp:stringParam name=\"CreateDateTime\">2020-01-02T03:04:05Z</hp:stringParam>",
        )
        + "<hp:t>anchor<hp:tab width=\"1200\" leader=\"2\" type=\"1\"/>end</hp:t>"
        + &end(7);
    let d = HwpxDecoder::decode(&package(&para(&run(&source), &[]))).unwrap();
    let before = d.document.sections()[0].paragraphs[1].runs.clone();
    let Some(Control::Memo { anchor_runs, metadata, .. }) = before[0].content.as_control() else {
        panic!("closed memo must remain a Memo")
    };
    assert!(matches!(anchor_runs[0].content, hwpforge_core::RunContent::InlineText(_)));
    assert_eq!(metadata.author, "writer\r\t\n<&");
    assert_eq!(metadata.create_datetime, "2020-01-02T03:04:05Z");
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let xml = section_xml(&bytes);
    assert!(xml.contains("writer"));
    assert!(xml.contains("2020-01-02T03:04:05Z"));
    assert_eq!(
        HwpxDecoder::decode(&bytes).unwrap().document.sections()[0].paragraphs[1].runs,
        before
    );
}

/// 이것을 실패시키는 것: XML 제어 공백을 문자 참조 없이 재출력하기.
#[test]
fn character_references_preserve_values_on_the_first_roundtrip() {
    let source = begin(7, "FORMULA")
        .replace("name=\"본문\"", "name=\"a&#9;b&#10;c&#13;d\"")
        .replace("=SUM(A1:A2)</hp:stringParam>", "a&#13;b&#9;c&#10;d</hp:stringParam>")
        + "<hp:t>3</hp:t>"
        + &end(7);
    let d = HwpxDecoder::decode(&package(&para(&run(&source), &[]))).unwrap();
    let before = d.document.sections()[0].paragraphs[1].runs.clone();
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    assert_eq!(
        HwpxDecoder::decode(&bytes).unwrap().document.sections()[0].paragraphs[1].runs,
        before
    );
}

/// 이것을 실패시키는 것: 편집 가능한 경계형 누름틀을 stamp 후보로 다시 노출하기.
#[test]
fn editable_boundary_placeholder_is_not_a_stamp_candidate() {
    let bytes = package(&para(
        &(run(&begin(7, "CLICK_HERE")) + &run("<hp:t>(   )</hp:t>") + &run(&end(7))),
        &[],
    ));
    let d = HwpxDecoder::decode(&bytes).unwrap();
    assert!(hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap()[0].fillable);
    assert!(hwpforge_smithy_hwpx::stamp::plan(&d.document).is_empty());
}

/// 이것을 실패시키는 것: 탭 속성이 있는 범위를 단일 일반 텍스트 값으로 승인하기.
#[test]
fn inline_boundary_body_is_explicitly_unfillable() {
    let bytes = package(&para(
        &(run(&begin(7, "CLICK_HERE"))
            + &run("<hp:t>a<hp:tab width=\"1200\" leader=\"2\" type=\"1\"/>b</hp:t>")
            + &run(&end(7))),
        &[],
    ));
    let field = hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap().remove(0);
    assert!(!field.fillable);
    assert_eq!(field.current, "a\tb");
}

/// 이것을 실패시키는 것: 접힌 필드의 이름 속성에 제어 공백을 그대로 출력하기.
#[test]
fn folded_field_name_references_survive_first_roundtrip() {
    let source = begin(7, "CLICK_HERE").replace("name=\"본문\"", "name=\"a&#9;b&#10;c&#13;d\"")
        + "<hp:t>value</hp:t>"
        + &end(7);
    let bytes = package(&para(&run(&source), &[]));
    let before = hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap()[0].name.clone();
    let d = HwpxDecoder::decode(&bytes).unwrap();
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    assert_eq!(hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap()[0].name, before);
}

/// 이것을 실패시키는 것: 자동 범위 안의 빈 셀을 stamp 후보·명시적 대상으로 승인하기.
#[test]
fn table_inside_field_is_not_a_cell_stamp_target() {
    use hwpforge_core::control::{FieldBoundary, FieldStart};
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    use hwpforge_smithy_hwpx::stamp::{apply_cells, plan_cells, CellStampAction, CellStampSpec};
    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);
    let cell = |value| {
        TableCell::new(
            vec![Paragraph::with_runs(vec![Run::text(value, cs)], ps)],
            HwpUnit::new(2000).unwrap(),
        )
    };
    let table = Table::new(vec![TableRow::new(vec![cell("성명"), cell("")])]);
    let mut start = FieldStart::default();
    start.id = "7".into();
    start.kind = "FORMULA".into();
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::control(
                    Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                    cs,
                ),
                Run::table(table, cs),
                Run::control(
                    Control::FieldBoundary {
                        boundary: FieldBoundary::End {
                            begin_id: "7".into(),
                            field_id: String::new(),
                        },
                    },
                    cs,
                ),
            ],
            ps,
        )],
        PageSettings::a4(),
    ));
    assert!(plan_cells(&doc).candidates.is_empty());
    let before = doc.sections().to_vec();
    assert!(apply_cells(
        &mut doc,
        &[CellStampSpec {
            table: 0,
            at: GridCoord::new(0, 1),
            label: None,
            action: CellStampAction::Field { name: "value".into(), hint: "hint".into() }
        }]
    )
    .is_err());
    assert_eq!(doc.sections(), before);
}

/// A memo's text can cross character-shape runs without becoming a generic
/// field range. Empty authored metadata must stay empty across repeated writes.
#[test]
fn closed_memo_across_runs_preserves_public_control_and_styles() {
    let source = run(&(memo_begin(7) + "<hp:t>first</hp:t>"))
        + &run("<hp:t>second<hp:tab width=\"1700\" leader=\"3\" type=\"2\"/>last</hp:t>")
            .replace("charPrIDRef=\"0\"", "charPrIDRef=\"1\"")
        + &run(&end(7));
    let d = HwpxDecoder::decode(&package(&para(&source, &[]))).unwrap();
    let before = d.document.sections()[0].paragraphs[1].runs.clone();
    let Some(Control::Memo { anchor_runs, metadata, .. }) = before[0].content.as_control() else {
        panic!("closed memo must use its public control")
    };
    assert_eq!(anchor_runs.len(), 2);
    assert_eq!(anchor_runs[0].char_shape_id.get(), 0);
    assert_eq!(anchor_runs[1].char_shape_id.get(), 1);
    assert!(matches!(anchor_runs[1].content, hwpforge_core::RunContent::InlineText(_)));
    assert!(metadata.preserve_empty);
    assert!(
        metadata.author.is_empty() && metadata.create_datetime.is_empty() && metadata.id.is_empty()
    );
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs[1].runs, before);
}

/// An unrelated group cannot supply a missing end. These are the two wire
/// patterns seen in press releases: one orphan, or an orphan before a closed
/// range with the same field tag but a different instance ID.
#[test]
fn missing_wire_ends_are_not_repaired_by_unrelated_groups_or_field_tags() {
    for second in [false, true] {
        for grouped in [false, true] {
            let mut body = para(&run(&(begin(7, "CLICK_HERE") + "<hp:t>kept</hp:t>")), &[]);
            if grouped {
                body += &containers(&para(&run("<hp:t>group text</hp:t>"), &[]))[3];
            }
            if second {
                body += &para(&run(&begin(8, "CLICK_HERE")), &[]);
                body += &para(&run(&("<hp:t>other</hp:t>".to_string() + &end(8))), &[]);
            }
            let d = HwpxDecoder::decode(&package(&body)).unwrap();
            let warnings: Vec<_> =
                d.warnings.iter().filter(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY").collect();
            assert_eq!(warnings.len(), 1, "{warnings:?}");
            assert!(format!("{warnings:?}").contains("7"));
            assert!(!d.warnings.iter().any(|w| matches!(w, hwpforge_smithy_hwpx::DecodeWarning::ContentLost { kind, .. } if kind == "field_body")));
            assert!(!warnings[0].is_content_loss());
            let before = range_and_text_runs(&d.document);
            let outcome = HwpxEncoder::encode_with_diagnostics(
                &d.document.validate().unwrap(),
                &d.style_store,
                &d.image_store,
                Default::default(),
            )
            .unwrap();
            assert!(outcome.warnings.iter().any(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY"));
            let bytes = outcome.bytes;
            let xml = section_xml(&bytes);
            assert!(!xml.contains("beginIDRef=\"7\""));
            assert_eq!(xml.matches("<hp:fieldEnd ").count(), usize::from(second));
            let after = HwpxDecoder::decode(&bytes).unwrap();
            assert_eq!(range_and_text_runs(&after.document), before);
            assert_eq!(
                after.warnings.iter().filter(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY").count(),
                1
            );
        }
    }
}

#[test]
fn recovered_range_blocks_touching_edits_and_allows_preceding_values() {
    use hwpforge_smithy_hwpx::stamp::{apply, plan, StampAction, StampSpec};
    use hwpforge_smithy_hwpx::{FillError, HwpxFiller, HwpxPatcher};
    let closed = begin(6, "CLICK_HERE").replace("name=\"본문\"", "name=\"before\"")
        + "<hp:t>old</hp:t>"
        + &end(6);
    let source = para(&run(&closed), &[])
        + &para(&run(&(begin(7, "CLICK_HERE") + "<hp:t>(   )</hp:t>")), &[]);
    let bytes = package(&source);
    let listed = HwpxFiller::list_fields_with_diagnostics(&bytes).unwrap();
    assert_eq!(listed.warnings.iter().filter(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY").count(), 1);
    assert!(matches!(
        HwpxFiller::fill(&bytes, &[("본문".into(), "new".into())].into()),
        Err(FillError::UnfillableField { .. })
    ));
    let filled =
        HwpxFiller::fill_with_codec_diagnostics(&bytes, &[("before".into(), "new".into())].into())
            .unwrap();
    assert_eq!(HwpxFiller::list_fields(&filled.value.bytes).unwrap()[0].current, "new");
    assert!(filled.decode_warnings.iter().any(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY"));
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    let original = decoded.document.sections().to_vec();
    assert!(plan(&decoded.document).is_empty());
    let request = StampSpec {
        section: 0,
        path: "paragraphs[2].runs[1].text".into(),
        span: 0..5,
        marker: "(   )".into(),
        action: StampAction::Field { name: "denied".into(), hint: None },
    };
    assert!(apply(&mut decoded.document, &[request]).is_err());
    assert_eq!(decoded.document.sections(), original);
    let mut section = original[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
    assert!(!preservation.text_slots.iter().any(|s| s.original_text == "(   )"));
    section.paragraphs[2].runs[1].content = hwpforge_core::RunContent::Text("changed".into());
    assert!(HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
        .is_err());
    section = original[0].clone();
    section.paragraphs[0].runs[0].content = hwpforge_core::RunContent::Text("allowed".into());
    let patched = HwpxPatcher::patch_section_preserving_with_diagnostics(
        &bytes,
        0,
        &section,
        None,
        Some(&preservation),
    )
    .unwrap();
    assert!(patched.warnings.iter().any(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY"));
    assert!(section_xml(&patched.value).contains("allowed"));
}

#[test]
fn unclosed_source_ranges_recover_in_each_nested_flow() {
    for wrapped in containers(&para(&run(&(begin(7, "FORMULA") + "<hp:t>keep</hp:t>")), &[])) {
        let decoded = HwpxDecoder::decode(&package(&wrapped)).unwrap();
        assert_eq!(
            decoded.warnings.iter().filter(|w| w.code() == "UNCLOSED_FIELD_BOUNDARY").count(),
            1
        );
        let original = range_and_text_runs(&decoded.document);
        let bytes = HwpxEncoder::encode(
            &decoded.document.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        let read = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(range_and_text_runs(&read.document), original);
        assert!(!section_xml(&bytes).contains("<hp:fieldEnd"));
    }
}

#[test]
fn unclosed_range_protects_contained_cells_but_not_preceding_tables() {
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_smithy_hwpx::stamp::{apply_cells, CellStampAction, CellStampSpec};
    use hwpforge_smithy_hwpx::{apply_set_cells, CellSpec, CellTarget};
    let table = &containers(&para(&run("<hp:t>plain</hp:t>"), &[]))[1];
    let bytes = package(&(table.clone() + &para(&run(&begin(7, "FORMULA")), &[]) + table));
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    let original = decoded.document.sections().to_vec();
    let spec = |table| CellSpec {
        table,
        target: CellTarget::At(GridCoord::new(0, 0)),
        text: "edited".into(),
    };
    assert!(apply_set_cells(&mut decoded.document, &[spec(0), spec(1)]).is_err());
    assert_eq!(decoded.document.sections(), original, "a denied batch changes no cell");
    assert!(apply_cells(
        &mut decoded.document,
        &[CellStampSpec {
            table: 1,
            at: GridCoord::new(0, 0),
            label: None,
            action: CellStampAction::Field { name: "denied".into(), hint: "hint".into() }
        }]
    )
    .is_err());
    assert_eq!(decoded.document.sections(), original);
    assert!(apply_set_cells(&mut decoded.document, &[spec(0)]).is_ok());
    assert_ne!(decoded.document.sections(), original);
}

fn range_and_text_runs(document: &Document) -> Vec<Run> {
    let mut runs = Vec::new();
    document.for_each_paragraph(|p| {
        runs.extend(
            p.runs
                .iter()
                .filter(|r| {
                    r.content.plain_text().is_some()
                        || matches!(r.content.as_control(), Some(Control::FieldBoundary { .. }))
                })
                .cloned(),
        );
    });
    runs
}

/// 이것을 실패시키는 것: Memo 접힘 때 원본 위치 기록을 함께 접지 않기.
#[test]
fn closed_memo_does_not_break_an_independent_fill_slot() {
    for multi in [false, true] {
        let memo = if multi {
            run(&memo_begin(7))
                + &run("<hp:t>anchor1</hp:t>")
                + &run("<hp:t>anchor2</hp:t>")
                + &run(&end(7))
        } else {
            run(&(memo_begin(7) + "<hp:t>anchor</hp:t>" + &end(7)))
        };
        let b = begin(8, "CLICK_HERE").replace("name=\"본문\"", "name=\"B\"")
            + "<hp:t>old</hp:t>"
            + &end(8);
        let bytes = package(&para(&(memo + &run(&b)), &[]));
        let listed = hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap();
        assert!(listed[0].fillable);
        let filled =
            hwpforge_smithy_hwpx::HwpxFiller::fill(&bytes, &[("B".into(), "new".into())].into())
                .unwrap();
        assert_eq!(
            hwpforge_smithy_hwpx::HwpxFiller::list_fields(&filled.bytes).unwrap()[0].current,
            "new"
        );
        let xml = section_xml(&filled.bytes);
        assert!(xml.contains("anchor"));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        hwpforge_smithy_hwpx::HwpxPatcher::export_section_preservation(
            &bytes,
            0,
            &decoded.document.sections()[0],
        )
        .unwrap();
    }
}

/// 이것을 실패시키는 것: Memo 모델에 없는 속성·파라미터를 접으면서 삭제.
#[test]
fn memo_folding_requires_representable_metadata() {
    let canonical = memo_begin(7);
    for (from, to) in [
        ("name=\"Prop\">0", "name=\"Prop\">8"),
        ("</hp:parameters>", "<hp:stringParam name=\"Extra\">kept</hp:stringParam></hp:parameters>"),
        ("metaTag=\"\"", "metaTag=\"authored-tag\""),
        ("type=\"MEMO\" name=\"\"", "type=\"MEMO\" name=\"named\""),
        ("editable=\"1\"", "editable=\"0\""),
        ("dirty=\"1\"", "dirty=\"0\""),
        ("zorder=\"1\"", "zorder=\"-1\""),
        ("cnt=\"7\" name=\"\"", "cnt=\"7\" name=\"group\""),
        ("name=\"Number\">0", "name=\"Number\">00"),
        ("name=\"ID\"", "name=\"ID\" xml:space=\"preserve\""),
        ("<hp:integerParam name=\"Prop\">0</hp:integerParam><hp:integerParam name=\"Number\">0</hp:integerParam>", "<hp:integerParam name=\"Number\">0</hp:integerParam><hp:integerParam name=\"Prop\">0</hp:integerParam>"),
    ] {
        let wire = canonical.replace(from, to) + "<hp:t>anchor</hp:t>" + &end(7);
        let decoded = HwpxDecoder::decode(&package(&para(&run(&wire), &[]))).unwrap();
        let before = decoded.document.sections()[0].paragraphs[1].clone();
        assert!(matches!(before.runs[0].content.as_control(), Some(Control::FieldBoundary { .. })), "{to}");
        let encoded = HwpxEncoder::encode(&decoded.document.validate().unwrap(), &decoded.style_store, &decoded.image_store).unwrap();
        let read = HwpxDecoder::decode(&encoded).unwrap();
        assert_eq!(read.document.sections()[0].paragraphs[1], before, "{to}");
    }
}

/// 이것을 실패시키는 것: 디코더에서 생성하는 경계·접힌 앵커·중첩 슬롯의 기록 누락.
#[test]
fn generated_field_representations_cover_original_text_elements() {
    use hwpforge_core::node_path::NodeKind;
    for length in 1..=24 {
        for folded in [false, true] {
            let start = if folded { memo_begin(7) } else { begin(7, "FORMULA") };
            let middle: String = (0..length).map(|i| run(&format!("<hp:t>t{i}</hp:t>"))).collect();
            let body = para(&(run(&start) + &middle + &run(&end(7))), &[]);
            let mut wrapped = containers(&body).to_vec();
            for tag in ["footNote", "endNote", "header"] {
                wrapped.push(para(&run(&format!("<hp:ctrl><hp:{tag} id=\"1\" applyPageType=\"BOTH\"><hp:subList>{body}</hp:subList></hp:{tag}></hp:ctrl>")), &[]));
            }
            wrapped.push(para(
                &run(&(memo_begin(9).replace(
                    "<hp:subList></hp:subList>",
                    &format!("<hp:subList>{body}</hp:subList>"),
                ) + &end(9))),
                &[],
            ));
            for container in &wrapped {
                let bytes = package(container);
                let xml = section_xml(&bytes);
                let decoded = HwpxDecoder::decode(&bytes).unwrap();
                let recorded: std::collections::BTreeSet<_> = decoded
                    .source_map(0)
                    .unwrap()
                    .nodes()
                    .filter(|n| n.kind() == NodeKind::Text)
                    .flat_map(|n| n.ranges().iter().map(|r| (r.start, r.end)))
                    .filter(|(start, end)| xml[*start..*end].starts_with("<hp:t"))
                    .collect();
                // Independent test oracle: every source text element must be covered.
                let (_, source) = hwpforge_core::source_xml::XmlSource::read(&xml).unwrap();
                let expected: std::collections::BTreeSet<_> = source
                    .elements()
                    .iter()
                    .filter(|e| e.name() == "t")
                    .map(|e| (e.outer().start, e.outer().end))
                    .collect();
                assert_eq!(recorded, expected, "folded={folded} length={length}");
                let preservation = hwpforge_smithy_hwpx::HwpxPatcher::export_section_preservation(
                    &bytes,
                    0,
                    &decoded.document.sections()[0],
                )
                .unwrap();
                for slot in &preservation.text_slots {
                    use hwpforge_smithy_hwpx::exchange::TextLocator;
                    let range = match slot.locator {
                        TextLocator::TextElement { element_start, element_end, .. } => {
                            element_start..element_end
                        }
                        TextLocator::EmptyRun { run_start, run_end } => run_start..run_end,
                    };
                    let address = hwpforge_core::node_path::CorePath::new(0, &slot.path);
                    assert!(
                        decoded
                            .source_map(0)
                            .unwrap()
                            .node(&address)
                            .unwrap()
                            .ranges()
                            .contains(&range),
                        "{}",
                        slot.path
                    );
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 구조 편집기가 공통 문단·삽입 위치 보호를 생략.
#[test]
fn structural_edits_refuse_closed_and_unclosed_formula_interiors() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    for closed in [false, true] {
        let body = para(&run(&begin(7, "FORMULA")), &[])
            + &para(&run("<hp:t>3</hp:t>"), &[])
            + &if closed { para(&run(&end(7)), &[]) } else { String::new() };
        let bytes = package(&body);
        let anchor = ParagraphLocator { section: 0, index: 2 };
        assert!(HwpxStructuralEditor::delete_paragraphs(&bytes, &[anchor]).is_err());
        for position in [InsertPosition::Before, InsertPosition::After] {
            let error =
                HwpxStructuralEditor::insert_paragraphs(&bytes, anchor, position, &["999".into()])
                    .unwrap_err();
            assert!(error.to_string().contains("protected field range"), "{error}");
        }
    }
}

/// 이것을 실패시키는 것: 여러 원본 t를 한 편집 슬롯으로 접거나 빈 점 필드를 슬롯으로 요구.
#[test]
fn multi_text_and_empty_fields_do_not_poison_neighbor_preservation() {
    for text in ["", "<hp:t>one</hp:t><hp:t>two</hp:t>"] {
        let a = begin(7, "CLICK_HERE") + text + &end(7);
        let b = begin(8, "CLICK_HERE").replace("name=\"본문\"", "name=\"B\"")
            + "<hp:t>old</hp:t>"
            + &end(8);
        let bytes = package(&para(&(run(&a) + &run(&b)), &[]));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let xml = section_xml(&bytes);
        let mapped: std::collections::BTreeSet<_> = decoded
            .source_map(0)
            .unwrap()
            .nodes()
            .flat_map(|n| n.ranges().iter().map(|r| (r.start, r.end)))
            .collect();
        let (_, source) = hwpforge_core::source_xml::XmlSource::read(&xml).unwrap();
        for element in
            source.elements().iter().filter(|e| matches!(e.name(), "fieldBegin" | "fieldEnd"))
        {
            assert!(
                mapped.contains(&(element.outer().start, element.outer().end)),
                "{}",
                element.name()
            );
        }
        let fields = hwpforge_smithy_hwpx::HwpxFiller::list_fields(&bytes).unwrap();
        assert!(!fields[0].fillable);
        assert!(fields[1].fillable);
        let filled =
            hwpforge_smithy_hwpx::HwpxFiller::fill(&bytes, &[("B".into(), "new".into())].into())
                .unwrap();
        assert_eq!(
            hwpforge_smithy_hwpx::HwpxFiller::list_fields(&filled.bytes).unwrap()[1].current,
            "new"
        );
    }
}

/// 이것을 실패시키는 것: 구역 편집 인벤토리가 원래 구역 번호를 0으로 다시 매기기.
#[test]
fn source_evidence_keeps_section_indices_in_preserving_editors() {
    let b = begin(8, "CLICK_HERE").replace("name=\"본문\"", "name=\"B\"")
        + "<hp:t>old</hp:t>"
        + &end(8);
    let body = para(&run(&b), &[]) + &para(&run(&(begin(7, "FORMULA") + "<hp:t>keep</hp:t>")), &[]);
    let xml = section_xml(&package(&body));
    let mut seed = HwpxDecoder::decode(&package("")).unwrap();
    seed.document.add_section(seed.document.sections()[0].clone());
    let base = HwpxEncoder::encode(
        &seed.document.validate().unwrap(),
        &seed.style_store,
        &seed.image_store,
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("ZIP entry name") == "Contents/section1.xml" {
            bytes = xml.as_bytes().to_vec();
        }
        out.start_file(
            entry.name().expect("ZIP entry name").to_string(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        out.write_all(&bytes).unwrap();
    }
    let bytes = out.finish().unwrap().into_inner();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let fields = hwpforge_core::field_inventory::FieldInventory::from_sections_at(
        &decoded.document.sections()[1..],
        1,
    );
    assert!(fields.diagnostics().is_empty());
    assert_eq!(fields.warnings().len(), 1);
    let filled =
        hwpforge_smithy_hwpx::HwpxFiller::fill(&bytes, &[("B".into(), "new".into())].into())
            .unwrap();
    assert_eq!(
        hwpforge_smithy_hwpx::HwpxFiller::list_fields(&filled.bytes).unwrap()[0].current,
        "new"
    );
}

/// 이것을 실패시키는 것: base의 현재 위치·ID를 확인하지 않고 과거 증거를 복원.
#[test]
fn stale_base_cannot_restore_provenance() {
    use hwpforge_core::field_inventory::FieldInventory;
    let bytes = package(&para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")), &[]));
    let base = HwpxDecoder::decode(&bytes).unwrap().document;
    let json = serde_json::to_string(&base).unwrap();
    for move_node in [false, true] {
        let mut stale = base.clone();
        if move_node {
            stale.sections_mut()[0].paragraphs.insert(
                1,
                Paragraph::with_runs(
                    vec![Run::text("moved", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
            );
        } else {
            let hwpforge_core::RunContent::Control(c) =
                &mut stale.sections_mut()[0].paragraphs[1].runs[0].content
            else {
                panic!()
            };
            let Control::FieldBoundary {
                boundary: hwpforge_core::control::FieldBoundary::Start(s),
            } = c.as_mut()
            else {
                panic!()
            };
            s.id = "changed".into();
        }
        let mut submitted: Document = serde_json::from_str(&json).unwrap();
        FieldInventory::restore_source_provenance(submitted.sections_mut(), stale.sections());
        assert!(submitted.validate().is_err());
    }
}

/// 이것을 실패시키는 것: 원본 hp 접두사 재검색·본문 hp: 치환 또는 승인된 문단 이동의 증거 이전 누락.
#[test]
fn structural_edit_before_unclosed_field_uses_decoder_positions() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let body = para(&run("<hp:t>first</hp:t>"), &[])
        + &para(&run("<hp:t>second</hp:t>"), &[])
        + &para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")), &[]);
    for alternate in [false, true] {
        let mut bytes = package(&body);
        if alternate {
            bytes = rewrite_section(&bytes, |s| {
                s.replace("hp:", "para:").replace("xmlns:hp=", "xmlns:para=")
            });
        }
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(decoded.document.sections()[0].paragraphs.len(), 4);
        let deleted = HwpxStructuralEditor::delete_paragraphs(
            &bytes,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        let inserted = HwpxStructuralEditor::insert_paragraphs(
            &bytes,
            ParagraphLocator { section: 0, index: 2 },
            InsertPosition::Before,
            &["hp:literal".into()],
        )
        .unwrap();
        for (output, count) in [(deleted, 3), (inserted, 5)] {
            if alternate {
                assert!(!section_xml(&output).contains("<hp:"));
            }
            let d = HwpxDecoder::decode(&output).unwrap();
            assert_eq!(d.document.sections()[0].paragraphs.len(), count);
            if count == 5 {
                assert_eq!(visible(&d.document.sections()[0].paragraphs[2]), "hp:literal");
            }
            assert_eq!(visible(d.document.sections()[0].paragraphs.last().unwrap()), "open");
            assert!(d.document.validate().is_ok());
        }
    }
}

/// 이것을 실패시키는 것: 접힌 Memo 상위 구간에서 앵커 기여분 누락 또는 겹치는 구간 중복 복사.
#[test]
fn source_copy_coverage_includes_folded_descendants_once() {
    use hwpforge_core::node_path::CorePath;
    use hwpforge_smithy_hwpx::decoder::source_map::SourceCoverage;
    for body in [
        run(&memo_begin(7))
            + &run("<hp:t>anchor1</hp:t>")
            + &run("<hp:t>anchor2</hp:t>")
            + &run(&end(7)),
        run(&(begin(7, "CLICK_HERE") + "<hp:t>anchor1</hp:t>" + &end(7))),
    ] {
        let bytes = package(&para(&body, &[]));
        let xml = section_xml(&bytes);
        let d = HwpxDecoder::decode(&bytes).unwrap();
        for path in ["paragraphs[1].runs[0]", "paragraphs[1].runs[0].control"] {
            let node = d.source_map(0).unwrap().node(&CorePath::new(0, path)).unwrap();
            let SourceCoverage::Complete(ranges) = node.copy_source() else { panic!("unrecorded") };
            assert!(ranges.windows(2).all(|r| r[0].end < r[1].start));
            let copy: String = ranges.iter().map(|r| &xml[r.clone()]).collect();
            assert_eq!(copy.matches("anchor1").count(), 1);
            assert_eq!(copy.matches("fieldBegin").count(), 2);
            assert_eq!(copy.matches("fieldEnd").count(), 1);
            if body.contains("anchor2") {
                assert_eq!(copy.matches("anchor2").count(), 1);
            }
        }
    }
}

/// 이것을 실패시키는 것: 파라미터 전체 원본 순서를 확인하지 않고 타입별 재정렬 뒤 Memo 접기.
#[test]
fn interleaved_memo_parameters_remain_ordered_boundaries() {
    let canonical = memo_begin(7);
    let folded = HwpxDecoder::decode(&package(&para(
        &run(&(canonical.clone() + "<hp:t>anchor</hp:t>" + &end(7))),
        &[],
    )))
    .unwrap();
    let encoded = HwpxEncoder::encode(
        &folded.document.clone().validate().unwrap(),
        &folded.style_store,
        &folded.image_store,
    )
    .unwrap();
    let read = HwpxDecoder::decode(&encoded).unwrap();
    assert_eq!(
        read.document.sections()[0].paragraphs[1],
        folded.document.sections()[0].paragraphs[1]
    );
    let number = "<hp:integerParam name=\"Number\">0</hp:integerParam>";
    let command = "<hp:stringParam name=\"Command\"></hp:stringParam>";
    let begin = canonical.replace(number, "").replace(command, &(command.to_owned() + number));
    let bytes = package(&para(&run(&(begin + "<hp:t>anchor</hp:t>" + &end(7))), &[]));
    let d = HwpxDecoder::decode(&bytes).unwrap();
    let original = d.document.sections()[0].paragraphs[1].clone();
    assert!(matches!(original.runs[0].content.as_control(), Some(Control::FieldBoundary { .. })));
    let bytes =
        HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
            .unwrap();
    let output = section_xml(&bytes);
    assert!(output.find(command).unwrap() < output.find(number).unwrap());
    assert_eq!(HwpxDecoder::decode(&bytes).unwrap().document.sections()[0].paragraphs[1], original);
}

fn rewrite_section(bytes: &[u8], rewrite: impl Fn(&str) -> String) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("ZIP entry name") == "Contents/section0.xml" {
            bytes = rewrite(std::str::from_utf8(&bytes).unwrap()).into_bytes();
        }
        out.start_file(
            entry.name().expect("ZIP entry name").to_string(),
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// 이것을 실패시키는 것: 한 wire run을 나눈 Core 형제들이 각각 전체 원본 run을 복사하도록 허용.
#[test]
fn split_wire_run_is_not_independently_copyable() {
    use hwpforge_core::node_path::CorePath;
    use hwpforge_smithy_hwpx::decoder::source_map::SourceCoverage;
    let bytes = package(&para(&run(&(begin(7, "FORMULA") + "<hp:t>body</hp:t>" + &end(7))), &[]));
    let d = HwpxDecoder::decode(&bytes).unwrap();
    let map = d.source_map(0).unwrap();
    for index in 0..3 {
        let node = map.node(&CorePath::new(0, format!("paragraphs[1].runs[{index}]"))).unwrap();
        assert!(matches!(node.copy_source(), SourceCoverage::Incomplete(_)));
    }
}

/// 이것을 실패시키는 것: 진짜 패키지의 fieldBegin에 임의 Core 주소를 결합해 출력 승인.
#[test]
fn original_package_cannot_authorize_a_forged_core_address() {
    use hwpforge_core::{
        field_inventory::FieldInventory, node_path::CorePath, source_xml::XmlSource,
    };
    let bytes = package(&para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")), &[]));
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    HwpxDecoder::verify_source_provenance(
        decoded.document.sections(),
        &HwpxDecoder::decode(&bytes).unwrap(),
    )
    .unwrap();
    let xml = section_xml(&bytes);
    let (_, source) = XmlSource::read(&xml).unwrap();
    let id = source.elements().iter().position(|e| e.name() == "fieldBegin").unwrap();
    decoded.document.sections_mut()[0].paragraphs[1]
        .runs
        .insert(0, Run::text("unauthorized", CharShapeIndex::new(0)));
    let forged = source
        .unclosed_origin(id, CorePath::new(0, "paragraphs[1].runs[1]"), "Contents/section0.xml")
        .unwrap();
    FieldInventory::recover_source_ranges(decoded.document.sections_mut(), &[forged], &bytes);
    assert!(HwpxDecoder::verify_source_provenance(
        decoded.document.sections(),
        &HwpxDecoder::decode(&bytes).unwrap()
    )
    .is_err());
    // Core checks local pairing. Format output must also verify correspondence
    // with the canonical source map, even when Core's local checks passed.
    let doc = decoded.document.validate().unwrap();
    assert!(HwpxEncoder::encode(&doc, &decoded.style_store, &decoded.image_store).is_err());
}

/// 이것을 실패시키는 것: 복제한 증거의 현재 ID/주소 검사 또는 패키지 재검증 생략.
#[test]
fn source_verifier_rejects_stale_claims_and_accepts_edit_owned_relocation() {
    use hwpforge_core::field_inventory::FieldInventory;
    let bytes = package(
        &(para(&run("<hp:t>editable</hp:t>"), &[])
            + &para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")), &[])),
    );
    let base = HwpxDecoder::decode(&bytes).unwrap().document;
    for change in ["id", "path", "flag", "missing"] {
        let mut doc = base.clone();
        if change == "path" {
            doc.sections_mut()[0].paragraphs[2]
                .runs
                .insert(0, Run::text("x", CharShapeIndex::new(0)));
        } else if change == "missing" {
            doc = serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
        } else {
            let hwpforge_core::RunContent::Control(c) =
                &mut doc.sections_mut()[0].paragraphs[2].runs[0].content
            else {
                panic!()
            };
            let Control::FieldBoundary {
                boundary: hwpforge_core::control::FieldBoundary::Start(s),
            } = c.as_mut()
            else {
                panic!()
            };
            if change == "id" {
                s.id = "changed".into();
            } else {
                s.unclosed = false;
            }
        }
        assert!(
            HwpxDecoder::verify_source_provenance(
                doc.sections(),
                &HwpxDecoder::decode(&bytes).unwrap()
            )
            .is_err(),
            "{change}"
        );
    }
    let mut moved = base.clone();
    assert!(FieldInventory::delete_paragraphs(moved.sections_mut(), 0, &[1]));
    HwpxDecoder::verify_source_provenance(moved.sections(), &HwpxDecoder::decode(&bytes).unwrap())
        .unwrap();
    let mut submitted: Document =
        serde_json::from_value(serde_json::to_value(&moved).unwrap()).unwrap();
    FieldInventory::restore_source_provenance(submitted.sections_mut(), moved.sections());
    HwpxDecoder::verify_source_provenance(
        submitted.sections(),
        &HwpxDecoder::decode(&bytes).unwrap(),
    )
    .unwrap();
    assert!(submitted.validate().is_ok());
}

/// 이것을 실패시키는 것: 원본 빈 t/run 위치 기록 누락 또는 패치의 접두사 hp 고정.
#[test]
fn preserving_patch_uses_decoder_text_locations_with_alternate_prefix() {
    use hwpforge_smithy_hwpx::HwpxPatcher;
    for body in ["<hp:t/>", "<hp:t>old</hp:t>", ""] {
        let bytes = rewrite_section(&package(&para(&run(body), &[])), |s| {
            s.replace("hp:", "para:").replace("xmlns:hp=", "xmlns:para=")
        });
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let mut section = decoded.document.sections()[0].clone();
        let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section).unwrap();
        section.paragraphs[1].runs[0].content =
            hwpforge_core::RunContent::Text("hp:literal\tnew".into());
        let result = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
            &bytes,
            0,
            &section,
            None,
            Some(&preservation),
        )
        .unwrap();
        let xml = section_xml(&result.value);
        assert!(!xml.contains("<hp:"));
        let read = HwpxDecoder::decode(&result.value).unwrap();
        assert_eq!(visible(&read.document.sections()[0].paragraphs[1]), "hp:literal\tnew");
    }
}

fn whitespace_only_preservation(wire: &str, text: &str) {
    use hwpforge_smithy_hwpx::{ExportedSection, HwpxPatcher, TextLocator};

    // Place a blank on both sides of a multibyte edit: offsets must address
    // the original XML, before either provenance attributes or marks exist.
    let body = run(&format!("<hp:t>{wire}</hp:t>"))
        + &run("<hp:t>가상 편집 대상</hp:t>")
        + &run(&format!("<hp:t>{wire}</hp:t>"));
    let bytes = package(&para(&body, &[]));
    let original_xml = section_xml(&bytes);
    let outcome = HwpxPatcher::export_section_for_edit(&bytes, 0, true).unwrap();
    assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
    let mut exported: ExportedSection =
        serde_json::from_str(&serde_json::to_string(&outcome.exported).unwrap()).unwrap();
    let preservation = exported.preservation.as_ref().expect("preservation metadata");
    for ri in [0, 2] {
        assert_eq!(exported.section.paragraphs[1].runs[ri].content.plain_text().unwrap(), text);
        let path = format!("paragraphs[1].runs[{ri}].text");
        let slot = preservation.text_slots.iter().find(|slot| slot.path == path).unwrap();
        assert_eq!(slot.original_text, text);
        let TextLocator::TextElement {
            element_start,
            element_end,
            content_start: Some(start),
            content_end: Some(end),
        } = slot.locator
        else {
            panic!("whitespace has an original text element");
        };
        assert_eq!(&original_xml[element_start..element_end], format!("<hp:t>{wire}</hp:t>"));
        assert_eq!(&original_xml[start..end], wire);
    }
    exported.section.paragraphs[1].runs[1].content =
        hwpforge_core::RunContent::Text("수정한 가상 문장".into());
    let patched = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap().bytes;
    assert_eq!(section_xml(&patched), original_xml.replace("가상 편집 대상", "수정한 가상 문장"));

    let mut before = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut after = zip::ZipArchive::new(Cursor::new(&patched)).unwrap();
    for i in 0..before.len() {
        let mut entry = before.by_index(i).unwrap();
        let name = entry.name().unwrap();
        if name == "Contents/section0.xml" {
            continue;
        }
        let mut other = after.by_name(&name).unwrap();
        let mut original = Vec::new();
        let mut unchanged = Vec::new();
        entry.read_to_end(&mut original).unwrap();
        other.read_to_end(&mut unchanged).unwrap();
        assert_eq!(unchanged, original);
    }
}

/// #206. 이것을 실패시키는 것: 공백 전처리 생략, 표식 제거 생략,
/// source map 원문 trim/슬롯 누락, 원본 바이트 위치 이동,
/// 패치 때 공백 재작성 또는 무관한 ZIP 엔트리 변경.
#[test]
fn preserving_patch_keeps_space_only_runs() {
    whitespace_only_preservation(" ", " ");
}

/// #206. 이것을 실패시키는 것: 공백 전처리 생략, 표식 제거 생략,
/// source map 원문 trim/슬롯 누락, 원본 바이트 위치 이동,
/// 패치 때 CR 참조 재작성 또는 무관한 ZIP 엔트리 변경.
#[test]
fn preserving_patch_keeps_cr_only_runs() {
    // 이것을 실패시키는 것: HxText::text의 CR 1:1 정규화 제거.
    // Core는 LF지만 미편집 슬롯의 원본 CR 참조 바이트는 그대로다.
    whitespace_only_preservation("&#13;", "\n");
}

/// 이것을 실패시키는 것: claim의 A 패키지로 B 검증, 또는 복원 시 기존 A claim 유지.
#[test]
fn foreign_package_claim_cannot_authorize_base_b() {
    use hwpforge_core::field_inventory::FieldInventory;
    let b = package(&para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")), &[]));
    let a = rewrite_section(&b, |xml| xml.replace("id=\"77\"", "id=\"88\""));
    let base = HwpxDecoder::decode(&b).unwrap();
    let foreign = HwpxDecoder::decode(&a).unwrap();
    let mut submitted = base.document.clone();
    let hwpforge_core::RunContent::Control(c) =
        &mut submitted.sections_mut()[0].paragraphs[1].runs[0].content
    else {
        panic!()
    };
    let Control::FieldBoundary { boundary: hwpforge_core::control::FieldBoundary::Start(s) } =
        c.as_mut()
    else {
        panic!()
    };
    s.id = "88".into();
    let origin = FieldInventory::source_claims(foreign.document.sections())[0]
        .1
        .source_claim()
        .unwrap()
        .clone();
    FieldInventory::recover_source_ranges(submitted.sections_mut(), &[origin], &a);
    HwpxDecoder::verify_source_provenance(submitted.sections(), &foreign).unwrap();
    assert!(HwpxDecoder::verify_source_provenance(submitted.sections(), &base).is_err());
    let validated = submitted.clone().validate().unwrap();
    assert!(HwpxEncoder::encode(&validated, &base.style_store, &base.image_store).is_err());
    FieldInventory::restore_source_provenance(submitted.sections_mut(), base.document.sections());
    assert!(FieldInventory::source_claims(submitted.sections())
        .iter()
        .all(|(_, s)| s.source_claim().is_none()));
    assert!(HwpxDecoder::verify_source_provenance(submitted.sections(), &base).is_err());
    assert!(submitted.validate().is_err());
}

/// 이것을 실패시키는 것: 머리말/꼬리말 제거 후 복수 인덱스 또는 단수 경로 이전 생략.
#[test]
fn structural_delete_relocates_header_footer_provenance() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, ParagraphLocator};
    for tag in ["header", "footer"] {
        for total in [2, 3] {
            let mut body = String::new();
            for i in 0..total {
                let inside = if i == total - 1 {
                    run(&begin(77, "FORMULA")) + &run("<hp:t>open</hp:t>")
                } else {
                    run("<hp:t>ordinary</hp:t>")
                };
                body += &para(&run(&format!("<hp:ctrl><hp:{tag} id=\"{}\" applyPageType=\"BOTH\"><hp:subList>{}</hp:subList></hp:{tag}></hp:ctrl><hp:t>anchor</hp:t>", i + 10, para(&inside, &[]))), &[]);
            }
            let bytes = package(&body);
            let base = HwpxDecoder::decode(&bytes).unwrap();
            let list = if tag == "header" {
                &base.document.sections()[0].headers
            } else {
                &base.document.sections()[0].footers
            };
            assert_eq!(list.len(), total);
            HwpxEncoder::encode(
                &base.document.clone().validate().unwrap(),
                &base.style_store,
                &base.image_store,
            )
            .unwrap();
            let output = HwpxStructuralEditor::delete_paragraphs(
                &bytes,
                &[ParagraphLocator { section: 0, index: 1 }],
            )
            .unwrap();
            let after = HwpxDecoder::decode(&output).unwrap();
            let claims = hwpforge_core::field_inventory::FieldInventory::source_claims(
                after.document.sections(),
            );
            assert_eq!(claims.len(), 1);
            let prefix = if total == 2 { tag.to_owned() } else { format!("{tag}s[1]") };
            assert_eq!(claims[0].0.relative(), format!("{prefix}.paragraphs[0].runs[0]"));
            HwpxDecoder::verify_source_provenance(after.document.sections(), &after).unwrap();
            assert!(after.document.validate().is_ok());
        }
    }
}

/// 이것을 실패시키는 것: 조상과 같은 원본 run을 공유하는 Memo 앵커에 Complete 부여.
#[test]
fn memo_child_sharing_ancestor_wire_run_is_not_independently_copyable() {
    use hwpforge_core::node_path::CorePath;
    use hwpforge_smithy_hwpx::decoder::source_map::SourceCoverage;
    let bytes = package(&para(&run(&(memo_begin(7) + "<hp:t>anchor</hp:t>" + &end(7))), &[]));
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let map = decoded.source_map(0).unwrap();
    let parent = map.node(&CorePath::new(0, "paragraphs[1].runs[0]")).unwrap();
    assert!(matches!(parent.copy_source(), SourceCoverage::Complete(_)));
    let child = map
        .node(&CorePath::new(0, "paragraphs[1].runs[0].control.memo.anchor[0].runs[0]"))
        .unwrap();
    let SourceCoverage::Incomplete(ranges) = child.copy_source() else {
        panic!("child cannot independently copy parent markers")
    };
    let xml = section_xml(&bytes);
    assert!(ranges.iter().any(|r| xml[r.clone()].contains("fieldBegin")));
    let text = map
        .node(&CorePath::new(0, "paragraphs[1].runs[0].control.memo.anchor[0].runs[0].text"))
        .unwrap();
    assert!(matches!(text.copy_source(), SourceCoverage::Incomplete(_)));
}

/// 이것을 실패시키는 것: base의 공개 document에 붙은 context로 고정 SourceMap을 대체.
#[test]
fn base_source_map_is_independent_of_mutable_document() {
    // Existing Document auto-traits survive the private decoder context.
    // 이것을 실패시키는 것: 컨텍스트에서 unwind-safety bound 제거 (컴파일 계약).
    fn preserves_unwind_safety<T: std::panic::RefUnwindSafe + std::panic::UnwindSafe>() {}
    preserves_unwind_safety::<Document>();
    let b = package(&para(&(run(&begin(77, "FORMULA")) + &run("<hp:t>base</hp:t>")), &[]));
    let a = rewrite_section(&b, |xml| xml.replace(">base<", ">foreign<"));
    let mut base = HwpxDecoder::decode(&b).unwrap();
    let foreign = HwpxDecoder::decode(&a).unwrap();
    base.document = foreign.document.clone();
    assert!(HwpxDecoder::verify_source_provenance(foreign.document.sections(), &base).is_err());
    assert!(HwpxDecoder::restore_source_provenance(&mut foreign.document.clone(), &base).is_err());
    // Rebind a formerly foreign document to a fresh B; no serialized context is trusted.
    let base = HwpxDecoder::decode(&b).unwrap();
    let mut submitted = foreign.document;
    HwpxDecoder::restore_source_provenance(&mut submitted, &base).unwrap();
    HwpxDecoder::verify_source_provenance(submitted.sections(), &base).unwrap();
    let json = serde_json::to_string(&submitted).unwrap();
    assert!(!json.contains("format_source"));
    assert!(!json.contains("source_basis"));
    let without_context: Document = serde_json::from_str(&json).unwrap();
    assert!(without_context.validate().is_err());
    HwpxEncoder::encode(&submitted.validate().unwrap(), &base.style_store, &base.image_store)
        .unwrap();
}

fn anchored_container_bytes(tag: &str, inside: &str, plural: bool) -> Vec<u8> {
    let container = |id, content: &str| {
        para(&run(&format!("<hp:ctrl><hp:{tag} id=\"{id}\" applyPageType=\"BOTH\"><hp:subList>{content}</hp:subList></hp:{tag}></hp:ctrl><hp:t>anchor</hp:t>")), &[])
    };
    let mut body = container(10, inside);
    if plural {
        body += &container(11, &para(&run("<hp:t>ordinary retained</hp:t>"), &[]));
    }
    package(&body)
}

fn assert_anchored_field_delete_rejected(tag: &str, closed: bool) {
    use hwpforge_core::field_inventory::FieldInventory;
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, ParagraphLocator};
    let body = para(
        &(run(&begin(77, "FORMULA"))
            + &run("<hp:t>PROTECTED</hp:t>")
            + &if closed { run(&end(77)) } else { String::new() }),
        &[],
    );
    // Singular/plural paths and descendants of detached containers must all
    // receive the same policy. 이것을 실패시키는 것: 단수 경로만 검사/중첩 필드 무시.
    for plural in [false, true] {
        for inside in containers(&body) {
            let bytes = anchored_container_bytes(tag, &inside, plural);
            let mut base = HwpxDecoder::decode(&bytes).unwrap();
            assert_eq!(FieldInventory::from_sections(base.document.sections()).fields().len(), 1);
            let before = serde_json::to_value(&base.document).unwrap();
            assert!(!FieldInventory::delete_paragraphs(base.document.sections_mut(), 0, &[1]));
            assert_eq!(serde_json::to_value(&base.document).unwrap(), before);
            HwpxDecoder::verify_source_provenance(base.document.sections(), &base).unwrap();
            // Minimal nested shape/table fixtures exercise Core directly; their
            // unrelated layout defaults are not structural admission fixtures.
            if inside == body {
                let error = HwpxStructuralEditor::delete_paragraphs(
                    &bytes,
                    &[ParagraphLocator { section: 0, index: 1 }],
                )
                .unwrap_err();
                assert!(error.to_string().contains("field"), "{error}");
            }
        }
    }
}

macro_rules! anchored_field_delete_test {
    ($test:ident, $tag:literal, $closed:literal) => {
        /// 이것을 실패시키는 것: 앵커 컨테이너 삭제 preflight 생략.
        #[test]
        fn $test() {
            assert_anchored_field_delete_rejected($tag, $closed);
        }
    };
}
anchored_field_delete_test!(delete_anchor_rejects_closed_header_field, "header", true);
anchored_field_delete_test!(delete_anchor_rejects_unclosed_header_field, "header", false);
anchored_field_delete_test!(delete_anchor_rejects_closed_footer_field, "footer", true);
anchored_field_delete_test!(delete_anchor_rejects_unclosed_footer_field, "footer", false);

/// 이것을 실패시키는 것: 필드 유무와 무관하게 앵커 컨테이너 삭제 금지.
#[test]
fn delete_anchor_allows_ordinary_header_footer() {
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, ParagraphLocator};
    for tag in ["header", "footer"] {
        let bytes = anchored_container_bytes(tag, &para(&run("<hp:t>ordinary</hp:t>"), &[]), false);
        let output = HwpxStructuralEditor::delete_paragraphs(
            &bytes,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        let after = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(after.document.sections()[0].paragraphs.len(), 1);
        assert!(after.document.sections()[0].headers.is_empty());
        assert!(after.document.sections()[0].footers.is_empty());
        assert!(!section_xml(&output).contains("ordinary"));
    }
}

/// 이것을 실패시키는 것: 수락/출력 raw 검증 생략, 작업 간 캐시, 필드마다 파트 재파싱.
#[test]
fn every_acceptance_and_output_reparses_original_parts_once() {
    use hwpforge_core::source_xml::source_work;
    for count in [1, 10, 80] {
        let body: String =
            (0..count).map(|id| para(&run(&begin(100 + id, "FORMULA")), &[])).collect();
        let bytes = package(&body);
        let base = HwpxDecoder::decode(&bytes).unwrap();
        let validated = base.document.clone().validate().unwrap();
        let size = section_xml(&bytes).len();
        for operation in ["accept", "output"] {
            let before = source_work();
            for k in 1..=7 {
                if operation == "accept" {
                    HwpxDecoder::verify_source_provenance(base.document.sections(), &base).unwrap();
                } else {
                    HwpxEncoder::encode(&validated, &base.style_store, &base.image_store).unwrap();
                }
                let after = source_work();
                assert_eq!(after.0 - before.0, k, "{operation}: fields={count}, calls={k}");
                assert_eq!(after.1 - before.1, k * size);
                assert_eq!(after.2 - before.2, k * count as usize * 2);
            }
            eprintln!("{operation}: fields={count}, calls=7, original_part_parses=7");
        }
    }
}

fn memo_cache_body(middle: &str, starts: &[u32]) -> String {
    para(&(run("<hp:t>a</hp:t>") + middle + &run("<hp:t>bcdefghij</hp:t>")), starts)
}

fn cached_wire_positions(bytes: &[u8]) -> Vec<u32> {
    let xml = section_xml(bytes);
    xml.match_indices("textpos=\"")
        .map(|(at, key)| {
            let value = &xml[at + key.len()..];
            value[..value.find('"').unwrap()].parse().unwrap()
        })
        .collect()
}

/// Independent wire positions, including both sides of a folded field, must
/// survive actual re-encoding (the unchanged-base byte shortcut is insufficient).
/// Mutation: remove decoder folded-field normalization; these assertions fail.
#[test]
fn memo_cache_normalizes_core_and_token_boundaries_before_roundtrip() {
    let memo = |id, text: &str| run(&(memo_begin(id) + text + &end(id)));
    let cases = [
        (memo(7, "<hp:t>XY</hp:t>"), vec![0, 1, 19, 20], vec![0, 1, 1, 2], 1),
        // Adjacent memos share a Core coordinate. The second anchor uses both
        // a surrogate pair and a tab: 1 + 2 + 8 + 1 = 12 wire units.
        (
            memo(7, "<hp:t>XY</hp:t>") + &memo(8, "<hp:t>Z😀<hp:tab/>Q</hp:t>"),
            vec![0, 19, 47, 48],
            vec![0, 1, 1, 2],
            2,
        ),
        // Memo spans cross runs; a run-local folded field lies between them.
        (
            run(&memo_begin(7))
                + &run("<hp:t>X</hp:t>")
                + &run("<hp:t>Y</hp:t>")
                + &run(&end(7))
                + &run(&(begin(9, "CLICK_HERE") + "<hp:t>123</hp:t>" + &end(9)))
                + &memo(8, "<hp:t>K</hp:t>"),
            vec![0, 19, 38, 55, 56],
            vec![0, 1, 1, 1, 2],
            2,
        ),
    ];
    for (middle, wire, core, memo_count) in cases {
        let original = package(
            &(memo_cache_body(&middle, &wire)
                + &para(&run("<hp:t>edit target</hp:t>"), &[])
                    .replace("pageBreak=\"0\"", "pageBreak=\"1\"")),
        );
        assert_eq!(cached_wire_positions(&original), wire);
        let decoded = HwpxDecoder::decode(&original).unwrap();
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        let p = &decoded.document.sections()[0].paragraphs[1];
        assert_eq!(
            p.runs
                .iter()
                .filter(|r| matches!(r.content.as_control(), Some(Control::Memo { .. })))
                .count(),
            memo_count
        );
        let cache = p.layout_cache.as_ref().expect("folded cache");
        assert_eq!(cache.lines.iter().map(|l| l.textpos).collect::<Vec<_>>(), core);
        // The position immediately after the first Memo must name all three
        // folded tokens, not just the field end at an expanded Core position.
        let after = wire.iter().position(|&w| w == 19).unwrap();
        assert_eq!(
            cache.lines[after].boundary.as_ref().unwrap().skip,
            [
                hwpforge_core::layout::WireTokenKind::FieldBegin,
                hwpforge_core::layout::WireTokenKind::Text,
                hwpforge_core::layout::WireTokenKind::FieldEnd
            ]
        );
        for edited in [false, true] {
            let mut doc = decoded.document.clone();
            if edited {
                // Edit a different paragraph; the memo's layout inputs stay
                // unchanged for full re-encoding of the supplied Core cache.
                doc.sections_mut()[0].paragraphs[2].runs[0] =
                    Run::text("edited target", CharShapeIndex::new(0));
            }
            let doc = doc.validate().unwrap();
            let options = EncodeOptions::default().with_emit_layout_cache(true);
            let full = HwpxEncoder::encode_with_diagnostics(
                &doc,
                &decoded.style_store,
                &decoded.image_store,
                options,
            )
            .unwrap();
            let saved = HwpxEncoder::encode_with_base(
                &original,
                &doc,
                &decoded.style_store,
                &decoded.image_store,
                options,
            )
            .unwrap();
            let outputs = if edited {
                // The base-aware editor deliberately invalidates all field
                // caches on text edits (numbering/backwards dependency policy).
                // Preserve that safety contract; never resurrect these caches.
                assert!(cached_wire_positions(&saved.bytes).is_empty());
                assert!(saved.warnings.iter().any(|w| matches!(w,
                    hwpforge_smithy_hwpx::EncodeWarning::LayoutCacheDropped { reason, .. }
                        if reason.contains("policy: numbering/page dependencies"))));
                vec![full]
            } else {
                vec![full, saved]
            };
            for output in outputs {
                assert!(output.warnings.is_empty(), "{:?}", output.warnings);
                assert_eq!(cached_wire_positions(&output.bytes), wire, "edited={edited}");
                let again = HwpxDecoder::decode(&output.bytes).unwrap();
                assert_eq!(
                    again.document.sections()[0].paragraphs[1].layout_cache.as_ref(),
                    Some(cache)
                );
            }
        }
    }
}

#[test]
fn memo_cache_rejects_lines_inside_folded_anchors() {
    let middle = run(&(memo_begin(7) + "<hp:t>XY</hp:t>" + &end(7)))
        + &run(&(memo_begin(8) + "<hp:t>Z</hp:t>" + &end(8)));
    // 10 is the boundary counterexample; also check the begin/end token
    // boundaries and the second memo, where cumulative contraction matters.
    for wire in [10, 9, 11, 2, 20, 27, 28] {
        let decoded = HwpxDecoder::decode(&package(&memo_cache_body(&middle, &[0, wire]))).unwrap();
        assert!(decoded.document.sections()[0].paragraphs[1].layout_cache.is_none(), "wire {wire}");
        assert!(
            decoded.warnings.iter().any(|w| matches!(w,
            hwpforge_smithy_hwpx::DecodeWarning::LayoutCacheDropped { path, reason }
            if path.to_string() == "section[0].para[1]"
                && reason == &format!("lineseg textpos {wire} is inside a field"))),
            "{:?}",
            decoded.warnings
        );
    }
}

#[test]
fn memo_cache_drops_unproven_folded_wire_content() {
    for anchor in [
        r#"<hp:t>X<hp:titleMark ignore="1"/>Y</hp:t>"#,
        r#"<hp:t>X</hp:t><hp:titleMark ignore="1"/><hp:t>Y</hp:t>"#,
    ] {
        let middle = run(&(memo_begin(7) + anchor + &end(7)));
        let decoded = HwpxDecoder::decode(&package(&memo_cache_body(&middle, &[0, 28]))).unwrap();
        let paragraph = &decoded.document.sections()[0].paragraphs[1];
        assert!(paragraph
            .runs
            .iter()
            .any(|r| matches!(r.content.as_control(), Some(Control::Memo { .. }))));
        assert!(paragraph.layout_cache.is_none());
        assert!(
            decoded.warnings.iter().any(|w| matches!(w,
            hwpforge_smithy_hwpx::DecodeWarning::LayoutCacheDropped { reason, .. }
                if reason.contains("inside a folded field"))),
            "{:?}",
            decoded.warnings
        );
    }
}

fn runless_paragraph_cases() -> Vec<String> {
    vec![para("", &[]), para("", &[0]), "<hp:p/>".into()]
}

/// 이것을 실패시키는 것: 가상 빈 run 출처 기록/슬롯 제외 생략,
/// 가상 빈 문단 캐시 예외 필터 복원(변이 시험으로 실패 확인),
/// 무편집 출력 바이트 추가, 무관한 ZIP 엔트리 변경.
#[test]
fn preserving_patch_keeps_runless_paragraphs() {
    use hwpforge_smithy_hwpx::{ExportedSection, HwpxPatcher};
    for empty in runless_paragraph_cases() {
        for wrapped in containers(&empty) {
            // Keep the preceding container intact; all three following body
            // paragraphs share the edited flow, including the runless one.
            let body = wrapped.clone()
                + &para(&run("<hp:t>가상 편집 대상</hp:t>"), &[0])
                + &empty
                + &para(&run("<hp:t>뒤 보통 문단</hp:t>"), &[0]);
            let bytes = package(&body);
            let original_xml = section_xml(&bytes);
            let outcome = HwpxPatcher::export_section_for_edit(&bytes, 0, true).unwrap();
            assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
            let mut exported: ExportedSection =
                serde_json::from_str(&serde_json::to_string(&outcome.exported).unwrap()).unwrap();
            let noop = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap();
            assert_eq!(noop.bytes, bytes);
            exported.section.for_each_paragraph_mut(|p| {
                for r in &mut p.runs {
                    if r.content.as_text() == Some("가상 편집 대상") {
                        r.content = hwpforge_core::RunContent::Text("수정한 가상 문장".into());
                    }
                }
            });
            let patched = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap().bytes;
            let uncached_empty = if empty == "<hp:p/>" { empty.clone() } else { para("", &[]) };
            let expected_body = wrapped
                + &para(&run("<hp:t>수정한 가상 문장</hp:t>"), &[])
                + &uncached_empty
                + &para(&run("<hp:t>뒤 보통 문단</hp:t>"), &[]);
            assert_eq!(section_xml(&patched), original_xml.replace(&body, &expected_body));
            let mut before = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let mut after = zip::ZipArchive::new(Cursor::new(&patched)).unwrap();
            assert_eq!(before.len(), after.len());
            for i in 0..before.len() {
                let mut entry = before.by_index(i).unwrap();
                let name = entry.name().unwrap().to_string();
                if name == "Contents/section0.xml" {
                    continue;
                }
                let mut original = Vec::new();
                let mut unchanged = Vec::new();
                entry.read_to_end(&mut original).unwrap();
                after.by_name(&name).unwrap().read_to_end(&mut unchanged).unwrap();
                assert_eq!(unchanged, original, "{name}");
            }
        }
    }
}

/// The #260 contract in a later section: a runless empty paragraph in
/// section 1 of a two-section package exports without a warning, an unedited
/// patch returns the package byte for byte, and editing another paragraph
/// drops the empty paragraph's line cache with the rest of its flow, while
/// section 0 stays as it was.
/// 이것을 실패시키는 것: patch 의 슬롯 투영에 `source_maps[section_idx]` 대신
/// `source_maps[0]` 을 넘기기(구역 0 에는 가상 run 이 없어 슬롯 수가 어긋난다).
#[test]
fn preserving_patch_keeps_runless_paragraphs_in_a_later_section() {
    use hwpforge_smithy_hwpx::HwpxPatcher;
    let section = |text: &str| {
        Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )
    };
    let mut doc = Document::new();
    doc.add_section(section("구역0"));
    doc.add_section(section("구역1"));
    let styles = style_store_for_preset("default").unwrap();
    let encoded =
        HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let read = |bytes: &[u8], name: &str| {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
        xml
    };
    for empty in runless_paragraph_cases() {
        let body = para(&run("<hp:t>편집 대상</hp:t>"), &[0])
            + &empty
            + &para(&run("<hp:t>뒤 문단</hp:t>"), &[0]);
        let mut source = zip::ZipArchive::new(Cursor::new(encoded.clone())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..source.len() {
            let mut entry = source.by_index(i).unwrap();
            let name = entry.name().expect("ZIP entry name").to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if name == "Contents/section1.xml" {
                let xml = String::from_utf8(data).unwrap();
                data = xml.replace("</hs:sec>", &format!("{body}</hs:sec>")).into_bytes();
            }
            let options =
                zip::write::SimpleFileOptions::default().compression_method(entry.compression());
            out.start_file(name, options).unwrap();
            out.write_all(&data).unwrap();
        }
        let bytes = out.finish().unwrap().into_inner();

        let outcome = HwpxPatcher::export_section_for_edit(&bytes, 1, true).unwrap();
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        let mut exported = outcome.exported;
        let noop = HwpxPatcher::patch_exported_section(&bytes, 1, &exported).unwrap();
        assert_eq!(noop.bytes, bytes);
        exported.section.for_each_paragraph_mut(|p| {
            for r in &mut p.runs {
                if r.content.as_text() == Some("편집 대상") {
                    r.content = hwpforge_core::RunContent::Text("고친 문장".into());
                }
            }
        });
        let patched = HwpxPatcher::patch_exported_section(&bytes, 1, &exported).unwrap().bytes;
        let uncached_empty = if empty == "<hp:p/>" { empty.clone() } else { para("", &[]) };
        let expected = para(&run("<hp:t>고친 문장</hp:t>"), &[])
            + &uncached_empty
            + &para(&run("<hp:t>뒤 문단</hp:t>"), &[]);
        let section1 = read(&bytes, "Contents/section1.xml");
        assert_eq!(read(&patched, "Contents/section1.xml"), section1.replace(&body, &expected));
        assert_eq!(read(&patched, "Contents/section0.xml"), read(&bytes, "Contents/section0.xml"));
    }
}

fn assert_runless_container_contract(body: &str, path: &str) {
    use hwpforge_smithy_hwpx::{HwpxError, HwpxPatcher, SectionWorkflowError};
    let bytes = package(body);
    let outcome = HwpxPatcher::export_section_for_edit_with_diagnostics(&bytes, 0, false).unwrap();
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    assert!(outcome.value.warning.is_none(), "{:?}", outcome.value.warning);
    let mut exported = outcome.value.exported;
    let noop = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap();
    assert_eq!(noop.bytes, bytes);
    let mut empty_count = 0;
    exported.section.for_each_paragraph_mut(|p| {
        if p.runs.len() == 1 && p.runs[0].content.as_text() == Some("") {
            empty_count += 1;
            p.runs[0].content = hwpforge_core::RunContent::Text("가상 삽입 문장".into());
        }
    });
    assert_eq!(empty_count, 1, "the nested paragraph must contain only its placeholder");
    let error = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap_err();
    assert!(
        matches!(error, SectionWorkflowError::PreservingPatch(hwpforge_smithy_hwpx::PatchError::Rejected(HwpxError::InvalidStructure { ref detail }))
            if detail.contains("no original run") && detail.contains(path)),
        "{error}"
    );
}

/// 이것을 실패시키는 것: skip 뒤 hx_runs 대신 원본 hx.runs로 가상 여부 판정
/// (변이 시험으로 실패 확인).
#[test]
fn preserving_patch_handles_note_head_only_paragraphs() {
    for (tag, kind, path) in [
        ("footNote", "FOOTNOTE", "footnote.paragraphs[0].runs[0].text"),
        ("endNote", "ENDNOTE", "endnote.paragraphs[0].runs[0].text"),
    ] {
        let head = run(&format!(
            r#"<hp:ctrl><hp:autoNum num="1" numType="{kind}"><hp:autoNumFormat type="DIGIT"/></hp:autoNum></hp:ctrl>"#
        )) + &run("<hp:t> </hp:t>");
        let body = para(&run(&format!(
            "<hp:ctrl><hp:{tag}><hp:subList>{}</hp:subList></hp:{tag}></hp:ctrl><hp:t>가상 앵커</hp:t>",
            para(&head, &[])
        )), &[]);
        assert_runless_container_contract(&body, path);
    }
}

/// 이것을 실패시키는 것: 머리말·꼬리말 안 run 없는 문단의 원본 출처가 source map 에서
/// 빠져 슬롯 누락 경고가 다시 나는 회귀(출처 기록을 생략하는 변이로 실패 확인).
#[test]
fn preserving_patch_handles_runless_headers_and_footers() {
    for tag in ["header", "footer"] {
        for empty in runless_paragraph_cases() {
            let body = para(&run(&format!(
                "<hp:ctrl><hp:{tag} id=\"1\" applyPageType=\"BOTH\"><hp:subList>{empty}</hp:subList></hp:{tag}></hp:ctrl><hp:t>가상 앵커</hp:t>"
            )), &[]);
            assert_runless_container_contract(&body, &format!("{tag}.paragraphs[0].runs[0].text"));
        }
    }
}

/// 이것을 실패시키는 것: 가상 빈 텍스트 편집 거부를 생략하고 슬롯에서 제외.
#[test]
fn preserving_patch_rejects_runless_paragraph_text_insertion() {
    use hwpforge_smithy_hwpx::{HwpxError, HwpxPatcher, SectionWorkflowError};
    for empty in runless_paragraph_cases() {
        for wrapped in containers(&empty).into_iter().take(3) {
            let bytes = package(&wrapped);
            let outcome = HwpxPatcher::export_section_for_edit(&bytes, 0, false).unwrap();
            assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
            let mut exported = outcome.exported;
            exported.section.for_each_paragraph_mut(|p| {
                for r in &mut p.runs {
                    if r.content.as_text() == Some("") {
                        r.content = hwpforge_core::RunContent::Text("가상 삽입 문장".into());
                    }
                }
            });
            let error = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap_err();
            assert!(
                matches!(error, SectionWorkflowError::PreservingPatch(hwpforge_smithy_hwpx::PatchError::Rejected(HwpxError::InvalidStructure { ref detail }))
                if detail.contains("no original run") && detail.contains(".runs[0].text")),
                "{error}"
            );
        }
    }
}

/// 이것을 실패시키는 것: 원본 run이 있는 정규화까지 가상 빈 텍스트로 분류.
#[test]
fn preserving_patch_still_edits_authored_empty_runs() {
    use hwpforge_smithy_hwpx::HwpxPatcher;
    for body in ["", "<hp:t/>", "<hp:t></hp:t>", "<hp:titleMark ignore=\"0\"/>"] {
        let bytes = package(&para(&run(body), &[]));
        let mut exported = HwpxPatcher::export_section_for_edit(&bytes, 0, false).unwrap().exported;
        exported.section.paragraphs[1].runs[0].content =
            hwpforge_core::RunContent::Text("가상 삽입 문장".into());
        let patched = HwpxPatcher::patch_exported_section(&bytes, 0, &exported).unwrap().bytes;
        assert_eq!(
            visible(&HwpxDecoder::decode(&patched).unwrap().document.sections()[0].paragraphs[1]),
            "가상 삽입 문장"
        );
    }
}
