//! Every decoder field representation × every public consumer.
//! 이것을 실패시키는 것: Memo 슬롯 이동 누락, PDF 앵커 미전개, annotation
//! 깊은 순회 생략, 출처 없는 unclosed 허용, 구조 preflight 누락.
use hwpforge::ops::{self, OpsError};
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxPatcher};
use std::io::{Cursor, Read, Write};

fn run(s: &str) -> String {
    format!("<hp:run charPrIDRef=\"0\">{s}</hp:run>")
}
fn para(s: &str) -> String {
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0">{s}<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/></hp:linesegarray></hp:p>"#
    )
}
fn begin(id: usize, kind: &str) -> String {
    let parameters = if kind == "CLICK_HERE" {
        r#"<hp:parameters cnt="1" name=""><hp:stringParam name="Direction">hint</hp:stringParam></hp:parameters>"#
    } else {
        ""
    };
    format!(
        r#"<hp:ctrl><hp:fieldBegin id="{id}" type="{kind}" name="A" editable="1" dirty="0" zorder="-1" fieldid="8" metaTag="">{parameters}</hp:fieldBegin></hp:ctrl>"#
    )
}
fn end(id: usize) -> String {
    format!(r#"<hp:ctrl><hp:fieldEnd beginIDRef="{id}" fieldid="8"/></hp:ctrl>"#)
}
fn memo() -> String {
    r#"<hp:ctrl><hp:fieldBegin id="7" type="MEMO" name="" editable="1" dirty="1" zorder="1" fieldid="8" metaTag=""><hp:parameters cnt="7" name=""><hp:integerParam name="Prop">0</hp:integerParam><hp:integerParam name="Number">0</hp:integerParam><hp:stringParam name="Command"></hp:stringParam><hp:stringParam name="ID"></hp:stringParam><hp:stringParam name="Author"></hp:stringParam><hp:stringParam name="MemoShapeIDRef">65535</hp:stringParam><hp:stringParam name="CreateDateTime"></hp:stringParam></hp:parameters><hp:subList></hp:subList></hp:fieldBegin></hp:ctrl>"#.into()
}
fn cell(body: &str) -> String {
    format!(
        r#"<hp:tbl rowCnt="1" colCnt="1"><hp:sz width="10000" height="5000"/><hp:tr><hp:tc><hp:subList>{body}</hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="10000" height="5000"/></hp:tc></hp:tr></hp:tbl>"#
    )
}
fn textbox(body: &str) -> String {
    format!(
        r#"<hp:rect id="1"><hp:orgSz width="10000" height="5000"/><hp:curSz width="10000" height="5000"/><hc:pt0 x="0" y="0"/><hc:pt1 x="10000" y="0"/><hc:pt2 x="10000" y="5000"/><hc:pt3 x="0" y="5000"/><hp:sz width="10000" height="5000"/><hp:pos treatAsChar="1"/><hp:drawText><hp:subList>{body}</hp:subList></hp:drawText></hp:rect>"#
    )
}
fn package(body: &str, with_b: bool) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("prefix", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let mut styles = hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap();
    let fonts: Vec<_> = styles.iter_fonts().map(|f| f.face_name.clone()).collect();
    for face in fonts {
        styles.replace_font(&face, "HwpForge Test");
    }
    let bytes =
        HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &Default::default()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name() == "Contents/section0.xml" {
            let mut xml = String::from_utf8(bytes).unwrap();
            // Give the seed paragraph the same native cache as the generated rows.
            let cache = para("");
            let cache =
                &cache[cache.find("<hp:linesegarray>").unwrap()..cache.find("</hp:p>").unwrap()];
            xml = xml.replace("</hp:p>", &format!("{cache}</hp:p>"));
            let b = if with_b {
                para(&run(&(begin(9, "CLICK_HERE").replace("name=\"A\"", "name=\"B\"")
                    + "<hp:t>old</hp:t>"
                    + &end(9))))
            } else {
                String::new()
            };
            let normal = if with_b {
                para(&run("<hp:t>( )</hp:t>"))
                    + &para(&run(&cell(&para(&run("<hp:t>normal</hp:t>")))))
            } else {
                String::new()
            };
            bytes = xml.replace("</hs:sec>", &format!("{b}{normal}{body}</hs:sec>")).into_bytes();
        }
        out.start_file(
            entry.name(),
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&bytes).unwrap();
    }
    let bytes = out.finish().unwrap().into_inner();
    if !with_b {
        return bytes;
    }
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap()
}
fn code<T>(result: Result<T, OpsError>) -> String {
    result.map(|_| "OK".into()).unwrap_or_else(|e| e.code().as_str().into())
}
fn rows() -> Vec<(&'static str, String)> {
    let closed = para(&(run(&begin(7, "FORMULA")) + &run("<hp:t>anchor</hp:t>") + &run(&end(7))));
    let note = para(&run(&cell(&para(&run("<hp:t>note</hp:t>")))));
    vec![
        ("point", para(&run(&(begin(7, "CLICK_HERE") + "<hp:t>anchor</hp:t>" + &end(7))))),
        ("closed", closed.clone()),
        ("unclosed", para(&(run(&begin(7, "FORMULA")) + &run("<hp:t>anchor</hp:t>")))),
        ("memo_one", para(&run(&(memo() + "<hp:t>anchor</hp:t>" + &end(7))))),
        (
            "memo_multi",
            para(
                &(run(&memo())
                    + &run("<hp:t>anc</hp:t>")
                    + &run("<hp:t>hor</hp:t>")
                    + &run(&end(7))),
            ),
        ),
        (
            "memo_boundary",
            para(&run(&(memo().replace("metaTag=\"\"", "metaTag=\"kept\"")
                + "<hp:t>anchor</hp:t>"
                + &end(7)))),
        ),
        (
            "nested",
            para(
                &(run(&begin(7, "FORMULA"))
                    + &run(&begin(8, "FORMULA"))
                    + &run("<hp:t>anchor</hp:t>")
                    + &run(&end(8))
                    + &run(&end(7))),
            ),
        ),
        ("cell", para(&run(&cell(&closed)))),
        ("textbox", para(&run(&textbox(&closed)))),
        (
            "memo_annotation",
            para(&run(&(memo().replace(
                "<hp:subList></hp:subList>",
                &format!("<hp:subList>{note}</hp:subList>"),
            ) + "<hp:t>anchor</hp:t>"
                + &end(7)))),
        ),
        (
            "annotation",
            para(&run(&(begin(7, "MEMO").replace(
                "metaTag=\"\"></hp:fieldBegin>",
                &format!("metaTag=\"\"><hp:subList>{note}</hp:subList></hp:fieldBegin>"),
            ) + "<hp:t>anchor</hp:t>"))),
        ),
    ]
}

#[test]
fn decoder_representations_by_consumer_matrix() {
    use ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
    use ops::markdown::{to_md, MdExportOptions, MdMode};
    let mut result_rows = Vec::new();
    for (name, body) in rows() {
        let bytes = package(&body, true);
        if let Ok(dir) = std::env::var("FBODY_MATRIX_FIXTURES") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.hwpx")), &bytes)
                .unwrap();
        }
        let fields = ops::read::fields(&bytes).unwrap();
        assert!(
            fields
                .fields
                .iter()
                .any(|f| f.name.as_deref() == Some("B") && f.fillable && f.current == "old"),
            "{name}"
        );
        let filled =
            ops::edit::fill(&bytes, &[("B".into(), "new".into())], &Default::default()).unwrap();
        assert!(ops::read::fields(&filled.bytes)
            .unwrap()
            .fields
            .iter()
            .any(|f| f.name.as_deref() == Some("B") && f.current == "new"));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let no_edit = HwpxEncoder::encode(
            &decoded.document.clone().validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        assert!(
            ops::diff::diff(&bytes, &no_edit).unwrap().diff.semantic.is_empty(),
            "{name}: no-edit roundtrip"
        );
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            decoded.document.sections(),
        );
        assert!(
            inventory.fields().iter().any(|f| f.current() == "anchor"),
            "{name}: field body inventory"
        );
        let original = &decoded.document.sections()[0];
        let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, original).unwrap();
        let mut edited = original.clone();
        let hwpforge_core::RunContent::Control(c) = &mut edited.paragraphs[1].runs[0].content
        else {
            panic!("B")
        };
        let Control::Field { display_text, .. } = c.as_mut() else { panic!("B") };
        *display_text = "new".into();
        let patched =
            HwpxPatcher::patch_section_preserving(&bytes, 0, &edited, None, Some(&preservation))
                .unwrap();
        assert_eq!(ops::read::fields(&patched).unwrap().fields[0].current, "new");
        // 이것을 실패시키는 것: 보호 대상을 admission 실패로 가리거나 정상 대상을 과잉 거부.
        let structural_error = ops::edit::delete_para(
            &bytes,
            &ops::edit::DeleteParaOptions::default().with_indexes(vec![4]),
        )
        .unwrap_err();
        let structural = structural_error.code().as_str().to_owned();
        assert_eq!(structural, "STRUCTURAL_CODEC", "{name}: {structural_error}");
        assert!(
            structural_error.to_string().contains("protected field range"),
            "{name}: {structural_error}"
        );
        ops::edit::delete_para(
            &bytes,
            &ops::edit::DeleteParaOptions::default().with_indexes(vec![2]),
        )
        .unwrap();
        let stamp_body =
            body.replace("anchor", "( )").replace(">anc<", ">( )<").replace(">hor<", "><");
        let stamp_bytes = package(&stamp_body, true);
        let stamp_doc = HwpxDecoder::decode(&stamp_bytes).unwrap();
        let spec = |path: &str| {
            StampMap::Legacy(vec![StampSpec {
                section: 0,
                path: path.into(),
                span: 0..3,
                marker: "( )".into(),
                action: StampAction::Field { name: "X".into(), hint: None },
            }])
        };
        assert_stamp_policy(name, &stamp_bytes, &stamp_doc);
        let stamp = "STAMP_SPEC_STALE".to_owned();
        ops::stamp::stamp(&stamp_bytes, &spec("paragraphs[2].runs[0].text"), &Default::default())
            .unwrap();
        let cell_bytes = package(&para(&run(&cell(&body))), true);
        let set_cell = code(ops::edit::set_cell(
            &cell_bytes,
            &ops::edit::SetCellOptions::default().with_table(1).with_at("0,0").with_text("changed"),
        ));
        assert_eq!(set_cell, "CELL_HAS_NON_TEXT_CONTENT", "{name}");
        ops::edit::set_cell(
            &cell_bytes,
            &ops::edit::SetCellOptions::default().with_table(0).with_at("0,0").with_text("changed"),
        )
        .unwrap();
        let diff = ops::diff::diff(&bytes, &filled.bytes).unwrap();
        assert_eq!(diff.diff.semantic.field_values.len(), 1, "{name}");
        let report = ops::inspect::inspect(&bytes, &Default::default()).unwrap().report;
        if ["annotation", "memo_annotation"].contains(&name) {
            assert_eq!(report.section_details[0].all_tables, 2);
            assert_eq!(report.section_details[0].deep_paragraphs, report.paragraphs);
        }
        let mut md = Vec::new();
        for mode in [MdMode::Lossy, MdMode::Styled, MdMode::Lossless] {
            let rendered = package(&body, false);
            let out = match to_md(&rendered, &MdExportOptions::default().with_mode(mode)) {
                Ok(out) => out,
                Err(error) if name == "point" && mode == MdMode::Lossless => {
                    assert_eq!(error.code().as_str(), "ENCODE_FAILED");
                    md.push("ENCODE_FAILED");
                    continue;
                }
                Err(error) => panic!("{name}/{mode:?}: {error}"),
            };
            assert!(
                out.markdown.contains("anchor")
                    || (name == "memo_multi"
                        && out.markdown.contains("anc")
                        && out.markdown.contains("hor")),
                "{name}/{mode:?}: {}",
                out.markdown
            );
            md.push("OK");
        }
        let exported = to_json(&bytes, &ToJsonOptions::default()).unwrap();
        let json = serde_json::to_string(&exported.document).unwrap();
        let roundtrip =
            from_json(&json, &FromJsonOptions::default().with_base(bytes.clone())).unwrap();
        assert_eq!(ops::read::fields(&roundtrip.bytes).unwrap().fields[0].current, "old");
        if ["unclosed", "annotation"].contains(&name) {
            assert_eq!(
                from_json(&json, &FromJsonOptions::default()).unwrap_err().code().as_str(),
                "VALIDATION_FAILED"
            );
            assert!(roundtrip
                .warnings
                .iter()
                .any(|w| w.info().code.as_str() == "UNCLOSED_FIELD_BOUNDARY"));
        }
        let pdf_bytes = package(&body, false);
        let opts = hwpforge_convert::ops::ToPdfOptions::default()
            .with_degraded(true)
            .with_font_dirs(vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../hwpforge-smithy-pdf/tests/fonts")]);
        let pdf = hwpforge_convert::ops::to_pdf(&pdf_bytes, &opts)
            .map(|_| "OK".to_string())
            .unwrap_or_else(|e| e.code().as_str().into());
        let expected_pdf =
            if ["point", "cell", "textbox"].contains(&name) { "PDF_RENDER_FAILED" } else { "OK" };
        assert_eq!(pdf, expected_pdf, "{name}");
        let row = vec![
            "OK".into(),
            "OK".into(),
            "OK".into(),
            structural,
            stamp,
            set_cell,
            "OK".into(),
            "OK".into(),
            "OK".into(),
            md.join("/"),
            pdf,
            "OK".into(),
        ];
        println!("{name}: {row:?}");
        result_rows.push((name, row));
    }
    assert_eq!(result_rows.len(), 11);
}

// 이것을 실패시키는 것: stamp plan의 contains_body 정책 필터 제거.
// Each row checks the actual marker bytes at a decoder-projected text address
// before checking rejection; no candidate-key mismatch can mask the policy.
fn assert_stamp_policy(name: &str, bytes: &[u8], decoded: &hwpforge_smithy_hwpx::HwpxDocument) {
    let fields =
        hwpforge_core::field_inventory::FieldInventory::from_sections(decoded.document.sections());
    let targets: Vec<_> = decoded
        .source_map(0)
        .unwrap()
        .text_slots()
        .iter()
        .filter(|slot| slot.original_text.contains("( )") && fields.contains_body(0, &slot.path))
        .collect();
    assert_eq!(targets.len(), 1, "{name}: actual protected marker text slot");
    let target = targets[0];
    let start = target.original_text.find("( )").unwrap();
    let span = start..start + 3;
    assert_eq!(&target.original_text[span.clone()], "( )", "{name}");
    assert!(
        target.path.ends_with(".text") || target.path.ends_with(".control.field"),
        "{name}: {}",
        target.path
    );
    assert!(
        !hwpforge_smithy_hwpx::stamp::plan(&decoded.document)
            .iter()
            .any(|c| c.path == target.path && c.span == span),
        "{name}: policy must exclude the real marker"
    );
    let spec = StampMap::Legacy(vec![StampSpec {
        section: 0,
        path: target.path.clone(),
        span,
        marker: "( )".into(),
        action: StampAction::Field { name: "X".into(), hint: None },
    }]);
    assert_eq!(
        code(ops::stamp::stamp(bytes, &spec, &Default::default())),
        "STAMP_SPEC_STALE",
        "{name}"
    );
}
macro_rules! stamp_row {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            let (_, body) = rows().into_iter().find(|(name, _)| *name == $name).unwrap();
            let body =
                body.replace("anchor", "( )").replace(">anc<", ">( )<").replace(">hor<", "><");
            let bytes = package(&body, true);
            assert_stamp_policy($name, &bytes, &HwpxDecoder::decode(&bytes).unwrap());
        }
    };
}
stamp_row!(stamp_policy_memo_one, "memo_one");
stamp_row!(stamp_policy_memo_multi, "memo_multi");
stamp_row!(stamp_policy_nested, "nested");

/// 이것을 실패시키는 것: origin별 패키지 복사/재파싱 또는 SourceMap 원점 선형 검색.
#[test]
fn provenance_acceptance_work_is_linear() {
    use hwpforge_core::{field_inventory::FieldInventory, source_xml::source_work};
    use ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
    let mut previous = None;
    for count in [10, 20, 40, 80] {
        let body: String = (0..count).map(|id| para(&run(&begin(100 + id, "FORMULA")))).collect();
        let bytes = package(&body, false);
        let base = HwpxDecoder::decode(&bytes).unwrap();
        let mut submitted: Document =
            serde_json::from_value(serde_json::to_value(&base.document).unwrap()).unwrap();
        FieldInventory::restore_source_provenance(
            submitted.sections_mut(),
            base.document.sections(),
        );
        let claims = FieldInventory::source_claims(submitted.sections());
        assert_eq!(claims.len(), count);
        let original = FieldInventory::source_claims(base.document.sections());
        let ptr = original[0].1.source_claim().unwrap().package().as_ptr();
        assert!(claims
            .iter()
            .all(|(_, start)| start.source_claim().unwrap().package().as_ptr() == ptr));
        let json =
            serde_json::to_string(&to_json(&bytes, &ToJsonOptions::default()).unwrap().document)
                .unwrap();
        let before = source_work();
        from_json(&json, &FromJsonOptions::default().with_base(bytes)).unwrap();
        let after = source_work();
        let work = (after.0 - before.0, after.1 - before.1, after.2 - before.2);
        eprintln!("unclosed={count} parses={} bytes={} evidence_work={}", work.0, work.1, work.2);
        assert_eq!(work.0, 4, "decode, initial evidence, acceptance and output each parse once");
        assert!(work.2 <= count * 8, "linear evidence work: {work:?}");
        if let Some((parses, bytes, units)) = previous {
            assert_eq!(work.0, parses);
            assert!(work.1 <= bytes * 2);
            assert!(work.2 <= units * 2);
        }
        previous = Some(work);
    }
}

stamp_row!(stamp_policy_point, "point");

stamp_row!(stamp_policy_closed, "closed");

stamp_row!(stamp_policy_unclosed, "unclosed");

stamp_row!(stamp_policy_memo_boundary, "memo_boundary");

stamp_row!(stamp_policy_cell, "cell");

stamp_row!(stamp_policy_textbox, "textbox");

stamp_row!(stamp_policy_memo_annotation, "memo_annotation");

stamp_row!(stamp_policy_annotation, "annotation");
