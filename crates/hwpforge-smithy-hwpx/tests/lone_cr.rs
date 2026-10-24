//! Body CR is a line break; only encoding folds CRLF within a text fragment.

use std::io::{Read, Write};

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::inline::{InlineSegment, InlineTabAttr, InlineText};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    CellSpec, CellTarget, HwpxCellEditor, HwpxDecoder, HwpxEncoder, HwpxPatcher,
};

const SECTION0: &str = "Contents/section0.xml";

fn run_para(run: Run) -> Paragraph {
    Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))
}

fn text_para(text: &str) -> Paragraph {
    run_para(Run::text(text, CharShapeIndex::new(0)))
}

fn encode(paragraphs: Vec<Paragraph>) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::default()));
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::default())
        .expect("encode")
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut entry = zip.by_name(SECTION0).expect("section0");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("utf-8");
    text
}

/// Replaces `from` with `to` in section0 (the way to hand-write a base).
fn splice(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut entry = zin.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let method = entry.compression();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == SECTION0 {
            let text = String::from_utf8(data).expect("utf-8");
            assert!(text.contains(from), "{from:?} not in {text}");
            data = text.replace(from, to).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method))
            .expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// Every run of the first section as Core reads it, flattened to text; a
/// field shows its display text.
fn texts(bytes: &[u8]) -> Vec<String> {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    decoded.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| {
            p.runs
                .iter()
                .map(|r| match &r.content {
                    RunContent::Text(t) => t.clone(),
                    RunContent::InlineText(it) => it.plain_text(),
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Field { display_text, .. } => display_text.clone(),
                        Control::Hyperlink { text, .. } => text.clone(),
                        other => format!("{other:?}"),
                    },
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn assert_wire_has_line_break(bytes: &[u8]) {
    let xml = section_xml(bytes);
    assert!(xml.contains("<hp:lineBreak/>"), "{xml}");
    assert!(!xml.contains("&#13;"), "{xml}");
    assert!(!xml.contains('\r'), "a raw CR reached the section: {xml:?}");
}

#[test]
fn a_cr_mixed_with_tab_lf_nbsp_or_fwspace_is_written_as_a_line_break() {
    // 이것을 실패시키는 것: `encode_inline_text_xml` 의 본문 정규화 호출을 빼는 것
    // (수동 경로가 CR 문자 참조를 그대로 쓴다).
    let samples = ["가\r나\t다", "가\r나\n다", "가\r나\u{A0}다", "가\r나\u{1F}다"];
    let bytes = encode(samples.iter().map(|s| text_para(s)).collect());

    assert_wire_has_line_break(&bytes);
    assert_eq!(texts(&bytes), samples.map(|s| s.replace('\r', "\n")));
}

#[test]
fn a_cr_in_an_inline_text_plain_segment_is_written_as_a_line_break() {
    // 이것을 실패시키는 것: `build_inline_text_element_xml` 의 Plain 갈래가
    // `encode_inline_text_xml` 대신 CR 을 그대로 쓰는 `escape_xml` 을 부르는 것.
    let tab = InlineTabAttr { width: HwpUnit::new(1000).unwrap(), leader: 1, tab_type: 1 };
    let inline = InlineText::from_segments([
        InlineSegment::Plain("가\r나".to_string()),
        InlineSegment::Tab(tab),
        InlineSegment::Plain("다".to_string()),
    ]);
    let bytes = encode(vec![run_para(Run {
        content: RunContent::InlineText(inline),
        char_shape_id: CharShapeIndex::new(0),
    })]);

    assert_wire_has_line_break(&bytes);
    assert_eq!(texts(&bytes), ["가\n나\t다"]);
}

#[test]
fn a_cr_in_a_field_display_body_is_written_as_a_line_break() {
    // 이것을 실패시키는 것: 필드 표시 본문의 `build_text_element_xml` 호출을
    // CR 을 그대로 쓰는 직접 이스케이프로 바꾸는 것.
    let click_here = Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: Some("힌트".to_string()),
        help_text: None,
        name: Some("f".to_string()),
        display_text: "가\r나".to_string(),
    };
    let hyperlink =
        Control::Hyperlink { text: "가\r나".to_string(), url: "https://example.com".to_string() };
    for control in [click_here, hyperlink] {
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.runs.push(Run::control(control.clone(), CharShapeIndex::new(0)));
        let bytes = encode(vec![para]);

        assert_wire_has_line_break(&bytes);
        assert_eq!(texts(&bytes), ["가\n나"], "{control:?}");
    }
}

/// Patches paragraph 1 (`edit me`) of `base` to `text`.
fn patch_para1(base: &[u8], text: &str) -> Vec<u8> {
    let mut exported =
        HwpxPatcher::export_section_for_edit(base, 0, false).expect("export").exported;
    let RunContent::Text(slot) = &mut exported.section.paragraphs[1].runs[0].content else {
        panic!("paragraph 1 is plain text");
    };
    *slot = text.to_string();
    HwpxPatcher::patch_exported_section_with_codec_diagnostics(base, 0, &exported)
        .expect("patch")
        .value
        .bytes
}

#[test]
fn a_patched_cr_becomes_a_break_and_an_untouched_reference_keeps_its_bytes() {
    // 이것을 실패시키는 것: `encode_inline_text_xml` 의 본문 정규화 호출을 빼는 것
    // (편집된 slot 이 lineBreak 대신 문자 참조로 쓰인다). 미편집 slot 은 인코더가
    // 쓰지 않는 표기 `&#x0D;` 로 두어, patch 가 그 slot 을 다시 쓰면 바이트가 바뀌어
    // 잡힌다.
    let base = splice(&encode(vec![text_para("keep"), text_para("edit me")]), "keep", "가&#x0D;나");

    let noop = {
        let exported =
            HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
        HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
            .expect("patch")
            .value
            .bytes
    };
    assert_eq!(noop, base, "a no-op patch is byte-identical");

    let patched = patch_para1(&base, "다\r라\t마");
    assert!(section_xml(&patched).contains("다<hp:lineBreak/>라<hp:tab/>마"));
    assert!(section_xml(&patched).contains("가&#x0D;나"), "the untouched slot keeps its bytes");
    assert_eq!(texts(&patched), ["가\n나", "다\n라\t마"]);
}

#[test]
fn set_cell_accepts_a_document_holding_a_cr_beside_a_tab() {
    // 이것을 실패시키는 것: 디코더 CR 정규화 제거
    // (decode → encode → decode 차이로 admission 이 거부한다).
    let width = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let base = splice(&encode(vec![text_para("ZZZ"), host]), "ZZZ", "가&#13;나<hp:tab/>다");
    assert_eq!(texts(&base)[0], "가\n나\t다");

    let spec =
        CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 1)), text: "라".into() };
    let edited = HwpxCellEditor::set_cells(&base, &[spec]).expect("admission accepts the base");

    let after = texts(&edited.bytes);
    assert_eq!(after[0], "가\n나\t다");
}

// 이것을 실패시키는 것: CR 라우팅/변환/CRLF 접기 삭제, 연속 CR 삭제,
// 또는 run/Plain 조각들을 먼저 합쳐 경계를 넘는 CRLF도 접기.
#[test]
fn cr_edges_fold_only_within_one_text_fragment() {
    for (input, want, breaks) in [
        ("가\r나", "가\n나", 1),
        ("가\r\n나", "가\n나", 1),
        ("가\r\r\n나", "가\n\n나", 2),
        ("\r", "\n", 1),
    ] {
        let bytes = encode(vec![text_para(input)]);
        let xml = section_xml(&bytes);
        assert_eq!(xml.matches("<hp:lineBreak/>").count(), breaks, "{input:?}");
        assert!(!xml.contains("&#13;"));
        assert_eq!(texts(&bytes), [want]);
    }
    let cs = CharShapeIndex::new(0);
    let bytes = encode(vec![
        Paragraph::with_runs(
            vec![Run::text("가\r", cs), Run::text("\n나", cs)],
            ParaShapeIndex::new(0),
        ),
        text_para("다\r"),
        text_para("\n라"),
        run_para(Run {
            content: RunContent::InlineText(InlineText {
                segments: vec![
                    InlineSegment::Plain("마\r".into()),
                    InlineSegment::Plain("\n바".into()),
                ],
            }),
            char_shape_id: cs,
        }),
        Paragraph::with_runs(
            vec![
                Run::text("사\r", cs),
                Run::control(
                    Control::Hyperlink { text: "\n아".into(), url: "https://example.com".into() },
                    cs,
                ),
            ],
            ParaShapeIndex::new(0),
        ),
    ]);
    assert_eq!(texts(&bytes), ["가\n\n나", "다\n", "\n라", "마\n\n바", "사\n\n아"]);
}

// 이것을 실패시키는 것: 디코더 CR 정규화 제거, 디코더 CRLF 접기, InlineText Plain 정규화 우회.
#[test]
fn referenced_cr_is_one_to_one_in_plain_inline_and_field_bodies() {
    for body in ["가&#13;&#10;나", "가&#xD;&#10;나", "가&#x0d;&#10;나"] {
        for tail in ["", "<hp:tab width=\"1000\" leader=\"1\" type=\"1\"/>"] {
            let base = splice(&encode(vec![text_para("ZZZ")]), "ZZZ", &format!("{body}{tail}"));
            assert_eq!(texts(&base), [if tail.is_empty() { "가\n\n나" } else { "가\n\n나\t" }]);
        }
        for control in [
            Control::Hyperlink { text: "ZZZ".into(), url: "https://example.com".into() },
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: None,
                help_text: None,
                name: Some("f".into()),
                display_text: "ZZZ".into(),
            },
        ] {
            let base = splice(
                &encode(vec![run_para(Run::control(control, CharShapeIndex::new(0)))]),
                "ZZZ",
                body,
            );
            assert_eq!(texts(&base), ["가\n\n나"]);
        }
    }
}

// 이것을 실패시키는 것: 메모 anchor의 텍스트 인코더 우회 또는 중첩 본문 CR 변환 제거.
#[test]
fn memo_anchor_and_body_use_body_breaks() {
    let memo = Control::memo_with_anchor(
        vec![text_para("가\r\n나")],
        vec![Run::text("다\r라", CharShapeIndex::new(0))],
    );
    let bytes = encode(vec![run_para(Run::control(memo, CharShapeIndex::new(0)))]);
    let xml = section_xml(&bytes);
    assert!(xml.contains("가<hp:lineBreak/>나"));
    assert!(xml.contains("다<hp:lineBreak/>라"));
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let RunContent::Control(c) = &decoded.document.sections()[0].paragraphs[0].runs[0].content
    else {
        panic!("memo");
    };
    let Control::Memo { content, anchor_runs, .. } = c.as_ref() else {
        panic!("memo");
    };
    assert_eq!(content[0].text_content(), "가\n나");
    assert_eq!(anchor_runs[0].content.as_text(), Some("다\n라"));
}

// 이것을 실패시키는 것: patch의 메타데이터 비교 삭제 또는 inline markup 편집 거부 삭제.
#[test]
fn old_exports_require_reexport_and_a_rewritten_slot_rejects_a_second_edit() {
    let base = splice(&encode(vec![text_para("keep"), text_para("edit me")]), "keep", "가&#13;나");
    let mut old = HwpxPatcher::export_section_for_edit(&base, 0, false).unwrap().exported;
    old.preservation.as_mut().unwrap().text_slots[0].original_text = "가\r나".into();
    let err =
        HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &old).unwrap_err();
    assert!(err.to_string().contains("re-export"), "{err}");
    let patched = patch_para1(&base, "다\r\n라");
    assert!(section_xml(&patched).contains("다<hp:lineBreak/>라"));
    let mut next = HwpxPatcher::export_section_for_edit(&patched, 0, false).unwrap().exported;
    next.section.paragraphs[1].runs[0].content = RunContent::Text("바".into());
    let err =
        HwpxPatcher::patch_exported_section_with_codec_diagnostics(&patched, 0, &next).unwrap_err();
    assert!(err.to_string().contains("inline HWPX markup"), "{err}");
}

// 이것을 실패시키는 것: mixed_text 또는 folded_field의 CRLF 캐시 거부 삭제.
#[test]
fn crlf_contraction_drops_cached_coordinates_with_a_diagnostic() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning};
    let cs = CharShapeIndex::new(0);
    for content in [
        RunContent::Text("가\r\n나".into()),
        RunContent::InlineText(InlineText {
            segments: vec![InlineSegment::Plain("가\r\n나".into())],
        }),
        RunContent::Control(Box::new(Control::Hyperlink {
            text: "가\r\n나".into(),
            url: "https://example.com".into(),
        })),
        RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: None,
            help_text: None,
            name: Some("f".into()),
            display_text: "가\r\n나".into(),
        })),
        RunContent::Control(Box::new(Control::memo_with_anchor(
            vec![text_para("메모")],
            vec![Run::text("가\r\n나", cs)],
        ))),
    ] {
        let mut p = run_para(Run { content, char_shape_id: cs });
        p.runs.push(Run::text("뒤", cs));
        p.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![p], PageSettings::default()));
        let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap();
        let outcome = HwpxEncoder::encode_with_diagnostics(
            &doc.validate().unwrap(),
            &styles,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        assert!(!section_xml(&outcome.bytes).contains("<hp:linesegarray"));
        assert!(outcome.warnings.iter().any(|w| matches!(w, EncodeWarning::LayoutCacheDropped { reason, .. } if reason.contains("CRLF"))), "{:?}", outcome.warnings);
    }
}

fn cached_base() -> Vec<u8> {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_smithy_hwpx::EncodeOptions;
    let mut para = text_para("가나다라");
    let mut second = LineSeg::default();
    second.textpos = 3;
    para.layout_cache = Some(LayoutCache::new(vec![LineSeg::default(), second]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![text_para("앞"), para], PageSettings::default()));
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}

// 이것을 실패시키는 것: 디코더에서 CRLF를 접거나 wire map 길이를 정규화된 길이로 바꾸기.
#[test]
fn referenced_crlf_keeps_original_wire_cache_positions() {
    let base = splice(&cached_base(), "가나다라", "가&#13;&#10;나");
    let decoded = HwpxDecoder::decode(&base).unwrap();
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
    let para = &decoded.document.sections()[0].paragraphs[1];
    assert_eq!(para.text_content(), "가\n\n나");
    assert_eq!(para.layout_cache.as_ref().unwrap().lines[1].textpos, 3);
}

// 이것을 실패시키는 것: CR만 있어도 캐시를 거부하거나 run들을 합쳐 CRLF를 접기.
#[test]
fn lone_cr_and_split_run_cr_lf_keep_admissible_caches() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_smithy_hwpx::EncodeOptions;
    let cs = CharShapeIndex::new(0);
    for runs in [vec![Run::text("가\r나", cs)], vec![Run::text("가\r", cs), Run::text("\n나", cs)]]
    {
        let mut p = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        p.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![p], PageSettings::default()));
        let out = HwpxEncoder::encode_with_diagnostics(
            &doc.validate().unwrap(),
            &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert!(section_xml(&out.bytes).contains("<hp:linesegarray"));
    }
}

// 이것을 실패시키는 것: from-json --base 공통 경로 또는 patch가 변경된 문단의 캐시를 복사하기.
#[test]
fn edit_base_and_patch_invalidate_cache_before_crlf_contraction() {
    use hwpforge_smithy_hwpx::EncodeOptions;
    let base = cached_base();
    let mut decoded = HwpxDecoder::decode(&base).unwrap();
    decoded.document.sections_mut()[0].paragraphs[1].runs[0].content =
        RunContent::Text("가\r\n나".into());
    let out = HwpxEncoder::encode_with_base(
        &base,
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    for bytes in [out.bytes, patch_para1(&base, "가\r\n나")] {
        assert!(section_xml(&bytes).contains("가<hp:lineBreak/>나"));
        assert!(!section_xml(&bytes).contains("<hp:linesegarray"));
    }
}

// 이것을 실패시키는 것: fill이 patch의 공통 본문 인코더를 우회하거나 CRLF 접기 제거.
#[test]
fn fill_replacement_uses_the_same_crlf_rule() {
    use hwpforge_smithy_hwpx::HwpxFiller;
    let control = Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: None,
        help_text: None,
        name: Some("f".into()),
        display_text: "가".into(),
    };
    let base = encode(vec![run_para(Run::control(control, CharShapeIndex::new(0)))]);
    let out = HwpxFiller::fill(
        &base,
        &std::collections::BTreeMap::from([("f".into(), "나\r\n다".into())]),
    )
    .unwrap();
    assert!(section_xml(&out.bytes).contains("나<hp:lineBreak/>다"));
    assert_eq!(texts(&out.bytes), ["나\n다"]);
}

// 이것을 실패시키는 것: layout_edit::prepare의 캐시 무효화 진단 제거.
#[test]
fn edit_base_reports_cache_drop_even_when_normalized_wire_is_unchanged() {
    use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning};
    let base = splice(&cached_base(), "가나다라", "가<hp:lineBreak/>나다");
    let mut decoded = HwpxDecoder::decode(&base).unwrap();
    assert!(decoded.document.sections()[0].paragraphs[1].layout_cache.is_some());
    decoded.document.sections_mut()[0].paragraphs[1].runs[0].content =
        RunContent::Text("가\r\n나다".into());
    let out = HwpxEncoder::encode_with_base(
        &base,
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(!section_xml(&out.bytes).contains("<hp:linesegarray"));
    assert!(out.warnings.iter().any(|w| matches!(w, EncodeWarning::LayoutCacheDropped { reason, .. } if reason.contains("intentionally invalidated"))), "{:?}", out.warnings);
}

fn assert_repeated_fill_is_byte_identical(value: &str) {
    use hwpforge_smithy_hwpx::HwpxFiller;
    let control = Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: None,
        help_text: None,
        name: Some("f".into()),
        display_text: "before".into(),
    };
    let base = encode(vec![run_para(Run::control(control, CharShapeIndex::new(0)))]);
    let values = std::collections::BTreeMap::from([("f".into(), value.into())]);
    let first = HwpxFiller::fill(&base, &values).expect("first fill");
    assert_eq!(first.filled.len(), 1);
    assert_eq!(texts(&first.bytes), ["a\nb"]);
    let second = HwpxFiller::fill_with_codec_diagnostics(&first.bytes, &values)
        .expect("same HWPX value must be a no-op");
    assert!(second.value.filled.is_empty());
    assert!(second.encode_warnings.is_empty());
    assert_eq!(second.value.bytes, first.bytes);
}

// 이것을 실패시키는 것: fill의 비교 전 본문 정규화 제거.
#[test]
fn repeated_crlf_fill_is_byte_identical() {
    assert_repeated_fill_is_byte_identical("a\r\nb");
}

// 이것을 실패시키는 것: fill의 비교 전 본문 정규화 제거.
#[test]
fn repeated_cr_fill_is_byte_identical() {
    assert_repeated_fill_is_byte_identical("a\rb");
}

fn assert_repeated_patch_is_byte_identical(value: &str) {
    let base = encode(vec![text_para("keep"), text_para("edit me")]);
    let first = patch_para1(&base, value);
    assert_eq!(texts(&first), ["keep", "a\nb"]);
    assert_eq!(patch_para1(&first, value), first);
}

// 이것을 실패시키는 것: patch의 비교 전 본문 정규화 제거.
#[test]
fn repeated_crlf_patch_is_byte_identical() {
    assert_repeated_patch_is_byte_identical("a\r\nb");
}

// 이것을 실패시키는 것: patch의 비교 전 본문 정규화 제거.
#[test]
fn repeated_cr_patch_is_byte_identical() {
    assert_repeated_patch_is_byte_identical("a\rb");
}

// 이것을 실패시키는 것: 메모 anchor를 합쳐 CRLF 길이 변화를 판정하기.
#[test]
fn split_memo_anchor_cr_lf_keeps_admissible_cache() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    use hwpforge_smithy_hwpx::EncodeOptions;
    let cs = CharShapeIndex::new(0);
    let mut p = run_para(Run::control(
        Control::memo_with_anchor(
            vec![text_para("memo")],
            vec![Run::text("a\r", cs), Run::text("\nb", cs)],
        ),
        cs,
    ));
    p.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![p], PageSettings::default()));
    let out = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let xml = section_xml(&out.bytes);
    assert!(xml.contains("<hp:linesegarray"));
    assert_eq!(xml.matches("<hp:lineBreak/>").count(), 2);
}
