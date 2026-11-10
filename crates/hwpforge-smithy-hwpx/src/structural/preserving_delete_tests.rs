use super::*;
use hwpforge_core::{ImageStore, PageSettings, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use std::io::{Cursor, Read, Write};
const SECTION: &str = "Contents/section0.xml";
const TARGET: ParagraphLocator = ParagraphLocator { section: 0, index: 1 };

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn generated() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("처음"), paragraph("가 & <나>끝"), paragraph("남음")],
        PageSettings::a4(),
    ));
    crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}
fn replace(base: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut package = RawPackage::read(base).unwrap();
    let xml = package.read_text_entry(part).unwrap();
    assert!(xml.contains(from), "missing {from}");
    package.replace_text_entry(part, xml.replacen(from, to, 1));
    package.write().unwrap()
}
fn extra(base: &[u8], part: &str, value: &[u8]) -> Vec<u8> {
    let mut input = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.start_file(
            entry.name().unwrap(),
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.start_file(part, zip::write::SimpleFileOptions::default()).unwrap();
    out.write_all(value).unwrap();
    out.finish().unwrap().into_inner()
}
fn carry_base() -> Vec<u8> {
    extra(&generated(), "Scripts/opaque.bin", b"unrelated payload")
}
fn xml(base: &[u8]) -> String {
    RawPackage::read(base).unwrap().read_text_entry(SECTION).unwrap()
}
fn original_error(base: &[u8]) -> StructuralEditError {
    match admit(base) {
        Err(e) => e,
        Ok(_) => panic!("admission unexpectedly succeeded"),
    }
}

// 이것을 실패시키는 것: 공개 삭제에서 BOM 원문 좌표 불일치 또는 삭제 범위 밖 재작성.
#[test]
fn public_delete_with_bom_preserves_bytes_outside_decoder_paragraph_range() {
    let plain = carry_base();
    let base = replace(&plain, SECTION, "<?xml", "\u{feff}<?xml");
    assert!(matches!(original_error(&base), StructuralEditError::UncarriedZipEntries { .. }));
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let span = &decoded.source_maps[0].paragraphs[TARGET.index].outer;
    let plain_decoded = HwpxDecoder::decode(&plain).unwrap();
    let plain_span = &plain_decoded.source_maps[0].paragraphs[TARGET.index].outer;
    assert_eq!(span, &(plain_span.start + 3..plain_span.end + 3));
    let original = xml(&base);
    assert!(original.starts_with('\u{feff}'));
    assert!(original[span.clone()].starts_with("<hp:p id=\"1\""));

    let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    assert_eq!(xml(&result), format!("{}{}", &original[..span.start], &original[span.end..]));
    let after = HwpxDecoder::decode(&result).unwrap();
    assert_eq!(
        after.document.sections()[0]
            .paragraphs
            .iter()
            .map(Paragraph::text_content)
            .collect::<Vec<_>>(),
        ["처음", "남음"]
    );
    let before = RawPackage::read(&base).unwrap();
    let after = RawPackage::read(&result).unwrap();
    for path in before.entry_paths().filter(|p| *p != SECTION) {
        assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
    }
}

// 이것을 실패시키는 것: BOM 좌표 보정 제거로 대상 p의 미지 속성 검증을 건너뜀.
#[test]
fn public_delete_with_bom_and_unknown_paragraph_attribute_retains_admission_error() {
    let base = replace(&carry_base(), SECTION, "<?xml", "\u{feff}<?xml");
    let base = replace(&base, SECTION, "id=\"1\"", "id=\"1\" mystery='1'");
    let original = xml(&base);
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let span = &decoded.source_maps[0].paragraphs[TARGET.index].outer;
    assert!(original.starts_with('\u{feff}'));
    assert!(original[span.clone()].starts_with("<hp:p id=\"1\" mystery='1'"));
    let error = original_error(&base);
    assert!(matches!(error, StructuralEditError::UncarriedZipEntries { .. }));
    assert_eq!(HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(), error);
}

// 이것을 실패시키는 것: carry 실패의 원문 삭제 생략, 생존 ID 재배정, header 재작성.
#[test]
fn carry_failure_deletes_plain_source_and_preserves_ids_and_parts() {
    let base = replace(
        &replace(&carry_base(), SECTION, "id=\"0\"", "id=\"004\""),
        SECTION,
        "id=\"2\"",
        "id=\"004\"",
    );
    assert!(matches!(original_error(&base), StructuralEditError::UncarriedZipEntries { .. }));
    let result =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET]).unwrap();
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let span = decoded.source_maps[0].paragraphs[1].outer.clone();
    let original = xml(&base);
    assert_eq!(xml(&result.value), format!("{}{}", &original[..span.start], &original[span.end..]));
    assert_eq!(result.decode_warnings, decoded.warnings);
    FINAL_CHANGES.with(|c| {
        assert_eq!(c.borrow().rewritten, [SECTION]);
        assert!(c.borrow().deleted.is_empty());
    });
    assert!(result.encode_warnings.is_empty());
    let before = RawPackage::read(&base).unwrap();
    let after = RawPackage::read(&result.value).unwrap();
    for path in before.entry_paths().filter(|p| *p != SECTION) {
        assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
    }
    assert_eq!(
        HwpxStructuralEditor::insert_paragraphs(
            &base,
            TARGET,
            InsertPosition::After,
            &["추가".into()]
        )
        .unwrap_err(),
        original_error(&base)
    );
}

// 이것을 실패시키는 것: 원문 보존 삭제의 실패를 공개하거나 미지 태그/속성/namespace/control 삭제 승인.
#[test]
fn unsafe_deleted_source_returns_the_original_admission_error() {
    let plain = carry_base();
    for (from, to) in [
        ("<hp:t>가", "<hp:t><hp:unknown/>가"),
        ("<hp:t>가", "<hp:t mystery='1'>가"),
        ("<hp:t>가", "<hp:t xmlns:hp='urn:foreign'>가"),
        ("<hp:t>가", "<?unknown x?><hp:t>가"),
        ("<hp:t>가", "<!--unknown--><hp:t>가"),
        ("<hp:t>가", "<hp:ctrl/><hp:t>가"),
        ("http://www.hancom.co.kr/hwpml/2011/section", "urn:foreign-section"),
        ("id=\"1\"", "id=\"1\" mystery='1'"),
        ("id=\"1\"", "id=\"1\" xmlns:odd='urn:odd' odd:pageBreak='1'"),
        ("id=\"1\"", "id=\"1\" merged='true'"),
    ] {
        let base = if from == "id=\"1\"" && to.contains("merged") {
            replace(&plain,SECTION,"id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" columnBreak=\"0\" merged=\"0\"", "id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" columnBreak=\"0\" merged=\"true\"")
        } else {
            replace(&plain, SECTION, from, to)
        };
        let error = original_error(&base);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
            error,
            "{to}"
        );
    }
    for targets in [
        vec![TARGET, TARGET],
        vec![ParagraphLocator { section: 0, index: 99 }],
        vec![ParagraphLocator { section: 99, index: 1 }],
        vec![ParagraphLocator { section: 0, index: 0 }],
    ] {
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&plain, &targets).unwrap_err(),
            original_error(&plain)
        );
    }
}

// 이것을 실패시키는 것: 빈 run/t의 디코더 기록 무시 또는 Unicode/entity/CR 텍스트 재해석.
#[test]
fn recorded_empty_and_text_sources_are_deletable() {
    let base = carry_base();
    let text = xml(&base);
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let slot = &decoded.source_maps[0].text_slots()[1];
    let crate::exchange::TextLocator::TextElement { element_start, element_end, .. } = slot.locator
    else {
        panic!()
    };
    let raw = &text[element_start..element_end];
    for replacement in ["<hp:t/>", "<hp:t></hp:t>", "", "<hp:t>한글&#13;&amp;끝</hp:t>"] {
        let input = replace(&base, SECTION, raw, replacement);
        HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap();
        if replacement.is_empty() {
            let d = HwpxDecoder::decode(&input).unwrap();
            let raw = xml(&input);
            let map = &d.source_maps[0];
            let paragraphs = &d.document.sections()[0].paragraphs;
            let mut wrong = map.clone();
            if let crate::exchange::TextLocator::EmptyRun { run_end, .. } =
                &mut wrong.text_slots_mut()[1].locator
            {
                *run_end += 1;
            } else {
                panic!("empty run must be recorded")
            }
            assert!(delete_source::check(&raw, SECTION, &wrong, 0, &[1], paragraphs).is_err());
            let mut wrong = map.clone();
            wrong.set_node_coverage(
                &hwpforge_core::node_path::CorePath::new(0, "paragraphs[1].runs[0].text"),
                crate::decoder::source_map::SourceCoverage::Complete(
                    std::iter::once(0..1).collect(),
                ),
            );
            assert!(delete_source::check(&raw, SECTION, &wrong, 0, &[1], paragraphs).is_err());
            let mut wrong = map.clone();
            wrong.text_slots_mut()[1].original_text = "unrecorded".into();
            let mut ps = paragraphs.clone();
            ps[1].runs[0] = Run::text("unrecorded", CharShapeIndex::new(0));
            assert!(delete_source::check(&raw, SECTION, &wrong, 0, &[1], &ps).is_err());
        }
    }
}

// 이것을 실패시키는 것: Preserve일 때 원본 문단 참조 선검사 생략.
#[test]
fn surviving_reference_to_deleted_id_is_still_refused() {
    let input =
        replace(&carry_base(), SECTION, "<hp:t>남음</hp:t>", "<hp:t paraIDRef='1'>남음</hp:t>");
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
        original_error(&input)
    );
}

// 이것을 실패시키는 것: 빈 요청 앞에 decode/무결성/Preview gate 배치.
#[test]
fn empty_delete_ignores_every_admission_gate() {
    failures(&[]);
    crate::derived_values::COLLECTIONS.with(|c| c.set(0));
    let signed = extra(&carry_base(), "META-INF/signatures.xml", b"<signatures/>");
    for input in
        [vec![], b"broken".to_vec(), signed, extra(&generated(), "Preview/PrvText.txt", b"preview")]
    {
        let result =
            HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&input, &[]).unwrap();
        assert_eq!(result.value, input);
        assert!(result.decode_warnings.is_empty());
        assert!(result.encode_warnings.is_empty());
        assert!(visits().is_empty());
        assert_eq!(crate::derived_values::COLLECTIONS.with(|c| c.get()), 0);
    }
}
thread_local! {
    static FAIL: std::cell::RefCell<Vec<&'static str>> = const {std::cell::RefCell::new(Vec::new())};
    static VISITS: std::cell::RefCell<Vec<&'static str>> = const {std::cell::RefCell::new(Vec::new())};
}
fn marker(stage: &str) -> StructuralEditError {
    StructuralEditError::NotRoundTripSafe {
        component: format!("injected {stage}"),
        diff_path: "unchanged ordering and fields".into(),
    }
}
pub(super) fn checkpoint(stage: &'static str) -> Result<(), StructuralEditError> {
    VISITS.with(|v| v.borrow_mut().push(stage));
    if FAIL.with(|f| f.borrow().contains(&stage)) {
        if stage.starts_with("derived-") {
            Err(verification_error(crate::HwpxError::InvalidStructure {
                detail: format!("injected {stage}"),
            }))
        } else {
            Err(marker(stage))
        }
    } else {
        Ok(())
    }
}
fn failures(stages: &[&'static str]) {
    FAIL.with(|f| *f.borrow_mut() = stages.to_vec());
    VISITS.with(|v| v.borrow_mut().clear());
}
fn visits() -> Vec<&'static str> {
    VISITS.with(|v| v.borrow().clone())
}

// 이것을 실패시키는 것: compare 뒤 carry 호출, encode 오류에서 재시도, A 후단 실패에서 재시도.
#[test]
fn admission_call_boundaries_and_error_precedence_are_exact() {
    let early_calls =
        ["input-decode", "package-read", "integrity", "admission-encode", "admission-decode"];
    for (index, &early) in early_calls.iter().enumerate() {
        failures(&[early]);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&generated(), &[TARGET]).unwrap_err(),
            marker(early)
        );
        assert_eq!(visits(), early_calls[..=index]);
    }
    failures(&["compare", "carry"]);
    crate::stamp::ZIP_CARRY_CHECKS.with(|c| c.set(0));
    let (d, p, e) = admit_delete(&generated()).unwrap();
    assert_eq!(e, Some(marker("compare")));
    assert!(!visits().contains(&"carry"));
    assert_eq!(crate::stamp::ZIP_CARRY_CHECKS.with(|c| c.get()), 0);
    drop((d, p));
    failures(&["carry"]);
    HwpxStructuralEditor::delete_paragraphs(&generated(), &[TARGET]).unwrap();
    assert!(visits().contains(&"input-loss"));
    failures(&["target-preflight"]);
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&generated(), &[TARGET]).unwrap_err(),
        marker("target-preflight")
    );
    assert!(!visits().contains(&"input-loss"));
    failures(&[]);
}

// 이것을 실패시키는 것: 원래 admission 오류 대신 내부 오류 반환 또는 어느 최종 검증이든 생략.
#[test]
fn every_preserving_failure_restores_the_original_error() {
    for gate in [
        "input-loss",
        "header",
        "preview-plan",
        "target-preflight",
        "paragraph-references",
        "plain-source",
        "derived-collect",
        "derived-effects",
        "derived-check",
        "derived-heights",
        "splice",
        "preview-apply",
        "write",
        "self-verify",
        "package-final",
        "source-final",
        "preview-final",
    ] {
        failures(&["compare", gate]);
        let error =
            HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&generated(), &[TARGET])
                .unwrap_err();
        assert_eq!(error, marker("compare"), "{gate}");
        assert!(visits().contains(&gate), "not reached {gate}");
    }
    failures(&[]);
}

// 이것을 실패시키는 것: facts 중복 수집, check/height 순서 역전, admission 성공에 새 gate 적용.
#[test]
fn collection_and_policy_are_scoped_to_preserving_delete() {
    failures(&[]);
    crate::derived_values::DECISIONS.with(|d| d.borrow_mut().clear());
    crate::derived_values::COLLECTIONS.with(|c| c.set(0));
    HwpxStructuralEditor::delete_paragraphs(&generated(), &[TARGET]).unwrap();
    assert_eq!(crate::derived_values::COLLECTIONS.with(|c| c.get()), 0);
    assert!(!visits().contains(&"plain-source"));
    failures(&[]);
    crate::derived_values::DECISIONS.with(|d| d.borrow_mut().clear());
    crate::derived_values::COLLECTIONS.with(|c| c.set(0));
    HwpxStructuralEditor::delete_paragraphs(&carry_base(), &[TARGET]).unwrap();
    assert_eq!(crate::derived_values::COLLECTIONS.with(|c| c.get()), 1);
    assert_eq!(
        crate::derived_values::DECISIONS.with(|d| d.borrow().clone()),
        ["collect", "check", "heights"]
    );
    let v = visits();
    assert!(
        v.iter().position(|s| *s == "derived-collect").unwrap()
            < v.iter().position(|s| *s == "derived-check").unwrap()
    );
    assert!(
        v.iter().position(|s| *s == "derived-check").unwrap()
            < v.iter().position(|s| *s == "derived-heights").unwrap()
    );
    failures(&[]);
    crate::derived_values::DECISIONS.with(|d| d.borrow_mut().clear());
}

// 이것을 실패시키는 것: 손실 또는 header flag 검사 제거, 두 검사를 admission 성공에도 적용.
#[test]
fn source_guards_use_loss_classification_and_header_flag() {
    let base = carry_base();
    for guard in ["loss", "header"] {
        let (mut d, p, _) = admit_delete(&base).unwrap();
        if guard == "loss" {
            d.warnings.push(crate::DecodeWarning::ContentLost {
                part: "other.xml".into(),
                path: Default::default(),
                kind: "field_body".into(),
                reason: "discarded input".into(),
            });
        } else {
            d.source_basis = None;
        }
        assert!(
            delete_admitted(&base, &[TARGET], d, p, ParagraphIds::Preserve).is_err(),
            "{guard}"
        );
    }
    let (mut d, p, _) = admit_delete(&base).unwrap();
    d.source_basis = None;
    // The existing branch still normalizes a header if needed.
    delete_admitted(&base, &[TARGET], d, p, ParagraphIds::Reindex).unwrap();
}

// 이것을 실패시키는 것: 원본 digest/part/coverage/본문 주소/run·text 기록 대응 생략.
#[test]
fn plain_source_requires_decoder_owned_identity() {
    let base = carry_base();
    let d = HwpxDecoder::decode(&base).unwrap();
    let original = xml(&base);
    let map = &d.source_maps[0];
    let paragraphs = &d.document.sections()[0].paragraphs;
    let check = |m: &crate::decoder::source_map::SourceMap, part: &str, s, ps: &[Paragraph]| {
        delete_source::check(&original, part, m, s, &[1], ps)
    };
    check(map, SECTION, 0, paragraphs).unwrap();
    assert!(check(map, "Contents/other.xml", 0, paragraphs).is_err());
    assert!(check(map, SECTION, 1, paragraphs).is_err());
    let mut changed = map.clone();
    changed.source_digest[0] ^= 1;
    assert!(check(&changed, SECTION, 0, paragraphs).is_err());
    let mut changed = map.clone();
    changed.paragraphs[1].outer.start += 1;
    assert!(check(&changed, SECTION, 0, paragraphs).is_err());
    let mut missing = crate::decoder::source_map::SourceMap::default();
    missing.source_digest = map.source_digest;
    missing.paragraphs = map.paragraphs.clone();
    assert!(check(&missing, SECTION, 0, paragraphs).is_err());
    let mut changed = paragraphs.clone();
    changed[1].runs[0] = Run::text("other", CharShapeIndex::new(0));
    assert!(check(map, SECTION, 0, &changed).is_err());
    let mut changed = paragraphs.clone();
    changed[1].runs.push(Run::text("extra", CharShapeIndex::new(0)));
    assert!(check(map, SECTION, 0, &changed).is_err());
    use crate::decoder::source_map::SourceCoverage;
    use hwpforge_core::node_path::{CorePath, NodeKind};
    let node = map.node(&CorePath::new(0, "paragraphs[1]")).unwrap();
    for coverage in [SourceCoverage::Unrecorded, SourceCoverage::Incomplete(node.ranges().to_vec())]
    {
        let corrupt = node.with_coverage(coverage);
        assert!(delete_source::complete(Some(&corrupt), SECTION, NodeKind::Paragraph).is_err());
    }
    assert!(delete_source::complete(Some(node), SECTION, NodeKind::Table).is_err());
    for path in ["paragraphs[1]", "paragraphs[1].runs[0]", "paragraphs[1].runs[0].text"] {
        let mut corrupt = map.clone();
        corrupt.set_node_coverage(
            &CorePath::new(0, path),
            SourceCoverage::Complete(std::iter::once(0..1).collect()),
        );
        assert!(check(&corrupt, SECTION, 0, paragraphs).is_err());
    }

    let mut changed = paragraphs.clone();
    changed[1].runs[0] = Run::control(
        hwpforge_core::Control::InlinePageNumber {
            kind: hwpforge_core::control::InlinePageKind::TotalPages,
        },
        CharShapeIndex::new(0),
    );
    assert!(check(map, SECTION, 0, &changed).is_err());
    for fault in 0..5 {
        let mut corrupt = map.clone();
        let slot = &mut corrupt.text_slots_mut()[1];
        match fault {
            0 => slot.has_inline_markup = true,
            1 => slot.original_text = "wrong".into(),
            2 => {
                if let crate::exchange::TextLocator::TextElement { element_start, .. } =
                    &mut slot.locator
                {
                    *element_start += 1;
                }
            }
            3 => {
                if let crate::exchange::TextLocator::TextElement {
                    content_end: Some(end), ..
                } = &mut slot.locator
                {
                    *end -= 1;
                }
            }
            _ => slot.locator = crate::exchange::TextLocator::EmptyRun { run_start: 0, run_end: 0 },
        }
        assert!(check(&corrupt, SECTION, 0, paragraphs).is_err());
    }
}

fn table_host() -> Paragraph {
    use hwpforge_core::{Table, TableCell, TableRow};
    let cell =
        TableCell::new(vec![paragraph("칸")], hwpforge_foundation::HwpUnit::new(9000).unwrap());
    Paragraph::with_runs(
        vec![Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )
}
const CACHE: &str = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="99" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="9000" flags="393216"/></hp:linesegarray>"#;
fn table_base() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![table_host(), paragraph("삭제"), table_host(), paragraph("뒤")],
        PageSettings::a4(),
    ));
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let mut pkg = RawPackage::read(&base).unwrap();
    let x = xml(&base)
        .replace("height=\"0\" heightRelTo=", "height=\"321\" heightRelTo=")
        .replace("</hp:p>", &format!("{CACHE}</hp:p>"));
    pkg.replace_text_entry(SECTION, x);
    extra(&pkg.write().unwrap(), "Scripts/opaque.bin", b"carry")
}

// 이것을 실패시키는 것: top index를 flow ordinal로 캐스팅, cache 없는 seed 제외, 셀 cache 삭제, 높이 미적용.
#[test]
fn body_indices_resolve_across_nested_flows_and_keep_cell_caches() {
    for (adjust, cell_height, caches) in
        [("0", "0", true), ("0", "0", false), ("1", "0", true), ("1", "2400", true)]
    {
        let mut base = table_base();
        if adjust == "1" {
            let mut package = RawPackage::read(&base).unwrap();
            let source = xml(&base).replace("noAdjust=\"0\"", "noAdjust=\"1\"").replace(
                "width=\"9000\" height=\"0\"/>",
                &format!("width=\"9000\" height=\"{cell_height}\"/>"),
            );
            package.replace_text_entry(SECTION, source);
            base = package.write().unwrap();
        }

        if !caches {
            let d = HwpxDecoder::decode(&base).unwrap();
            let r = d.source_maps[0].paragraphs[1].outer.clone();
            let x = xml(&base);
            let edited = x[r].replace(CACHE, "");
            base =
                replace(&base, SECTION, &x[d.source_maps[0].paragraphs[1].outer.clone()], &edited);
        }
        let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
        let output = xml(&result);
        assert_eq!(output.matches("height=\"321\" heightRelTo=").count(), 1);
        assert_eq!(output.matches("height=\"0\" heightRelTo=").count(), 1);
        assert_eq!(output.matches(CACHE).count(), 3); // preceding host and both surviving cells
        assert!(output.contains("id=\"2\""));
    }
}

// 이것을 실패시키는 것: 표 R 검사를 삭제하거나 원래 0인 미지원 표를 면제.
#[test]
fn unsupported_affected_table_returns_original_admission_error() {
    for height in ["321", "0"] {
        let base = replace(&table_base(), SECTION, "treatAsChar=\"0\"", "treatAsChar=\"1\"");
        // Change both tables, including the following one.
        let base = replace(&base, SECTION, "treatAsChar=\"0\"", "treatAsChar=\"1\"");
        let base = if height == "0" {
            let mut p = RawPackage::read(&base).unwrap();
            p.replace_text_entry(
                SECTION,
                xml(&base).replace("height=\"321\" heightRelTo=", "height=\"0\" heightRelTo="),
            );
            p.write().unwrap()
        } else {
            base
        };
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
            original_error(&base)
        );
    }
}

// 이것을 실패시키는 것: 기존 병합 프로필의 noAdjust와 셀 높이를 서로 다른 행에서 OR로 조합.
// 이것을 실패시키는 것: 원문 삭제에 두 번째 표 gate/비분할 증명 추가, inline 행 누락 또는 축 OR 승인.
#[test]
fn observed_inline_profiles_work_through_public_delete() {
    for (height, adjust, merged) in [("0", "0", false), ("4688", "0", true), ("2148", "1", true)] {
        let base = table_base();
        let original = xml(&base);
        let source = references::source(&original).unwrap();
        let table = source.elements().iter().filter(|e| e.name() == "tbl").nth(1).unwrap().outer();
        let inline = crate::derived_values::tests::inline_table(height, adjust, merged)
            .replacen("height='10000'", "height='60000'", 1)
            .replace("<hp:run>칸</hp:run>", "<hp:run charPrIDRef='0'><hp:t>칸</hp:t></hp:run>");
        let base = replace(&base, SECTION, &original[table], &inline);
        let (d, p, _) = admit_delete(&base).unwrap();
        delete_admitted(&base, &[TARGET], d, p, ParagraphIds::Preserve).unwrap();
        let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
        assert!(xml(&result).contains("height='0' heightRelTo='ABSOLUTE'"));
        assert!(xml(&result).contains(&format!("height='{height}'/><hp:cellMargin")));
        let mut variants = vec![
            inline.replacen("repeatHeader='1'", "repeatHeader='0'", 1),
            inline.replacen("rowCnt=", "unknown=", 1),
            inline.replace(
                &format!("height='{height}'/><hp:cellMargin"),
                "height='19638'/><hp:cellMargin",
            ),
        ];
        if merged {
            let opposite_adjust = if adjust == "0" { "1" } else { "0" };
            variants.push(inline.replacen(
                &format!("noAdjust='{adjust}'"),
                &format!("noAdjust='{opposite_adjust}'"),
                1,
            ));
        }
        for changed in variants {
            let bad = replace(&base, SECTION, &inline, &changed);
            assert_eq!(
                HwpxStructuralEditor::delete_paragraphs(&bad, &[TARGET]).unwrap_err(),
                original_error(&bad)
            );
        }
    }
}

// 이것을 실패시키는 것: compare 실패 시 원본 bytes 반환, 차트 재인코드 또는 두 실패 때 carry 오류 선택.
#[test]
fn semantic_compare_failure_can_delete_without_roundtrip_reencoding() {
    use hwpforge_core::{
        chart::{ChartData, ChartType},
        Control,
    };
    let mut doc = Document::new();
    let chart = Paragraph::with_runs(
        vec![Run::control(
            Control::chart(ChartType::Scatter, ChartData::xy(&[("값", &[1.0], &[2.0])])),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    doc.add_section(Section::with_paragraphs(
        vec![chart, paragraph("삭제"), paragraph("남음")],
        PageSettings::a4(),
    ));
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = replace(&base, "Chart/chart1.xml", "val=\"lineMarker\"", "val=\"marker\"");
    // The chart wire normalizes an unused style; the original chart is retained.
    let original = original_error(&base);
    assert!(matches!(original, StructuralEditError::NotRoundTripSafe { .. }));
    let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    let after = HwpxDecoder::decode(&result).unwrap();
    let paragraphs = &after.document.sections()[0].paragraphs;
    assert_eq!(paragraphs.len(), 2);
    assert!(!paragraphs.iter().any(|p| p.text_content() == "삭제"));
    assert_eq!(paragraphs[1].text_content(), "남음");
    let before_package = RawPackage::read(&base).unwrap();
    let after_package = RawPackage::read(&result).unwrap();
    assert_eq!(
        after_package.read_entry("Chart/chart1.xml").unwrap(),
        before_package.read_entry("Chart/chart1.xml").unwrap()
    );
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let encoded = encode_hwpx(&decoded, crate::stamp::EncodePurpose::Admission).unwrap();
    check_zip_carry(&base, &encoded).unwrap();
    let both = extra(&base, "Scripts/other.bin", b"carry");
    assert_eq!(admit_delete(&both).unwrap().2, Some(original));
}

// 이것을 실패시키는 것: 원문 삭제 후 Preview 유지, Preview 경고 중복 또는 descriptor 최종 검증 생략.
#[test]
fn preserving_delete_removes_preview_and_keeps_existing_warning_channel() {
    let base = extra(
        &extra(&carry_base(), "Preview/PrvText.txt", b"unrelated preview"),
        "Preview/PrvImage.png",
        b"fake bitmap",
    );
    let result =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET]).unwrap();
    assert_eq!(result.encode_warnings.len(), 1);
    assert_eq!(result.encode_warnings[0].code(), "PREVIEW_REMOVED");
    let pkg = RawPackage::read(&result.value).unwrap();
    assert!(!pkg.entry_paths().any(|p| p.starts_with("Preview/")));
}

// 이것을 실패시키는 것: 한 구역만 처리, ID 없는 생존 문단 재작성, batch마다 facts 재수집.
#[test]
fn multiple_sections_and_targets_keep_original_id_spellings() {
    let mut doc = Document::new();
    for _ in 0..2 {
        doc.add_section(Section::with_paragraphs(
            vec![paragraph("시작"), paragraph("삭제1"), paragraph("유지"), paragraph("삭제2")],
            PageSettings::a4(),
        ));
    }
    let mut base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    for path in [SECTION, "Contents/section1.xml"] {
        base = replace(&replace(&base, path, "id=\"0\"", "id=\"0009\""), path, " id=\"2\"", "");
    }
    base = extra(&base, "Scripts/opaque.bin", b"carry");
    crate::derived_values::COLLECTIONS.with(|c| c.set(0));
    let result = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[
            ParagraphLocator { section: 1, index: 3 },
            TARGET,
            ParagraphLocator { section: 0, index: 3 },
            ParagraphLocator { section: 1, index: 1 },
        ],
    )
    .unwrap();
    assert_eq!(crate::derived_values::COLLECTIONS.with(|c| c.get()), 1);
    for path in [SECTION, "Contents/section1.xml"] {
        let p = RawPackage::read(&result).unwrap();
        let x = p.read_text_entry(path).unwrap();
        assert!(x.contains("id=\"0009\""));
        assert!(!x.contains("삭제"));
        assert!(x.contains("유지"));
        let source = references::source(&x).unwrap();
        let ids: Vec<_> = source
            .elements()
            .iter()
            .filter(|e| e.name() == "p")
            .map(|e| e.id_value().map(|r| x[r].to_owned()))
            .collect();
        assert_eq!(ids, [Some("0009".into()), None]);
    }
}

thread_local! {
    static FINAL_CHANGES:std::cell::RefCell<crate::raw_package::OutputChanges>=const{std::cell::RefCell::new(crate::raw_package::OutputChanges{rewritten:Vec::new(),deleted:Vec::new()})};
    static TAMPER:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};
}
pub(super) fn record_changes(changes: &crate::raw_package::OutputChanges) {
    FINAL_CHANGES.with(|c| *c.borrow_mut() = changes.clone());
}
pub(super) fn tamper_preview(bytes: Vec<u8>) -> Vec<u8> {
    if TAMPER.with(|c| c.get()) {
        let mut package = RawPackage::read(&bytes).unwrap();
        let path = "META-INF/container.xml";
        package.replace_text_entry(
            path,
            format!("{}<!--undeclared-->", package.read_text_entry(path).unwrap()),
        );
        package.write().unwrap()
    } else {
        bytes
    }
}

// 이것을 실패시키는 것: finish_edit의 self_verify/package/source_ranges 실제 호출 생략.
#[test]
fn final_bytes_and_declared_delta_are_independently_checked() {
    let base = carry_base();
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let output = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    let mut expected = decoded.document.clone();
    expected.sections_mut()[0].paragraphs.remove(TARGET.index);
    let changes =
        crate::raw_package::OutputChanges { rewritten: vec![SECTION.into()], ..Default::default() };
    let change = SectionChange {
        ids: ParagraphIds::Preserve,
        section: 0,
        path: SECTION.into(),
        original: xml(&base),
        deleted: vec![TARGET.index],
        inserted: None,
        heights: Vec::new(),
    };
    for kind in ["expected", "package", "source"] {
        let bytes = if kind == "package" {
            extra(&output, "Scripts/undeclared.bin", b"added")
        } else {
            output.clone()
        };
        let mut package = RawPackage::read(&bytes).unwrap();
        if kind == "source" {
            package.replace_text_entry(SECTION, format!("{}<!--undeclared-->", xml(&output)));
        }
        let mut declared = expected.clone();
        if kind == "expected" {
            declared.sections_mut()[0].paragraphs[1] = paragraph("wrong declared text");
        }
        let result = finish_edit(
            &base,
            package,
            &decoded,
            declared,
            &changes,
            std::slice::from_ref(&change),
        );
        assert!(result.is_err(), "tampered {kind} was accepted");
    }
}

// 이것을 실패시키는 것: Preview final 실제 호출 또는 원래 admission 오류 복구 생략.
#[test]
fn preview_final_failure_restores_the_original_error() {
    let base = extra(&carry_base(), "Preview/PrvText.txt", b"old preview");
    let original = original_error(&base);
    TAMPER.with(|c| c.set(true));
    let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]);
    TAMPER.with(|c| c.set(false));
    assert_eq!(result.unwrap_err(), original);
}

// 이것을 실패시키는 것: footer TOTAL_PAGE를 면제하거나 본문 관측 형태를 거부.
#[test]
fn total_page_recalculation_stays_scoped_to_observed_body_shape() {
    use hwpforge_core::{control::InlinePageKind, section::HeaderFooter, Control};
    for footer in [false, true] {
        let number = Paragraph::with_runs(
            vec![Run::control(
                Control::InlinePageNumber { kind: InlinePageKind::TotalPages },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        );
        let mut section = Section::with_paragraphs(
            vec![paragraph("처음"), paragraph("삭제"), paragraph("뒤")],
            PageSettings::a4(),
        );
        if footer {
            section.footers.push(HeaderFooter::all_pages(vec![number]));
        } else {
            section.paragraphs.push(number);
        }
        let mut doc = Document::new();
        doc.add_section(section);
        let base = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let base = extra(&base, "Scripts/opaque.bin", b"carry");
        failures(&[]);
        let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]);
        if footer {
            assert_eq!(result.unwrap_err(), original_error(&base));
        } else {
            assert!(result.is_ok());
        }
        assert!(visits().contains(&"derived-check"));
    }
}

// 이것을 실패시키는 것: admission 앞 무결성 검사 제거 또는 그 오류에서 원문 삭제 재시도.
#[test]
fn signed_package_is_refused_before_comparison() {
    let base = extra(&carry_base(), "META-INF/signatures.xml", b"<signature/>");
    failures(&[]);
    let error = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err();
    assert!(matches!(
        error,
        StructuralEditError::Codec(CodecFailure { stage: CodecStage::Decode, .. })
    ));
    assert!(!visits().contains(&"compare"));
    assert!(!visits().contains(&"input-loss"));
}

// 이것을 실패시키는 것: autoNum 객체 번호를 fieldBegin 표시 필드에 연결해 수집 거부.
#[test]
fn stored_object_numbers_use_the_original_number_ledger() {
    use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind};
    for kind in [AutoNumberKind::Table, AutoNumberKind::Picture, AutoNumberKind::Equation] {
        let mut doc = Document::new();
        let number = Run::control(
            hwpforge_core::Control::AutoNumber {
                kind,
                num: 5,
                format: AutoNumberFormat::default(),
            },
            CharShapeIndex::new(0),
        );
        doc.add_section(Section::with_paragraphs(
            vec![
                paragraph("처음"),
                paragraph("삭제"),
                Paragraph::with_runs(vec![number], ParaShapeIndex::new(0)),
            ],
            PageSettings::a4(),
        ));
        let base = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let base = extra(&base, "Scripts/opaque.bin", b"carry");
        let result = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
        assert!(xml(&result).contains("num=\"5\""));
    }
}

// 이것을 실패시키는 것: BOM 좌표 보정 또는 namespace·속성·명시 false 검증 제거.
#[test]
fn deleted_markup_grammar_checks_actual_namespaces_and_explicit_false_flags() {
    const ATTRS: &str =
        "id='5' paraPrIDRef='0' styleIDRef='0' pageBreak='0' columnBreak='0' merged='0'";
    let plain=format!("<hp:p {ATTRS}><hp:run charPrIDRef='0'><hp:t>한글&amp;&#13;끝</hp:t></hp:run><hp:linesegarray><hp:lineseg textpos='0'/></hp:linesegarray></hp:p>");
    for bom in ["", "\u{feff}"] {
        let check = |fragment: &str, uri: &str| {
            let prefix = format!(
            "{bom}<hs:sec xmlns:hs='{uri}' xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph'>"
        );
            let xml = format!("{prefix}{fragment}</hs:sec>");
            delete_source::check_markup(
                &xml,
                std::slice::from_ref(&(prefix.len()..prefix.len() + fragment.len())),
            )
        };
        let section = "http://www.hancom.co.kr/hwpml/2011/section";
        check(&plain, section).unwrap();
        check(
            &plain
                .replace("<hp:t>", "<q:t xmlns:q='http://www.hancom.co.kr/hwpml/2011/paragraph'>")
                .replace("</hp:t>", "</q:t>"),
            section,
        )
        .unwrap();
        assert!(check(&plain, "urn:other").is_err());
        for bad in [
            plain.replace("id='5'", "id='5' mystery='1'"),
            plain.replace(" charPrIDRef='0'", " mystery='1'"),
            plain.replace("<hp:t>", "<hp:t mystery='1'>"),
            plain.replace("<hp:t>", "<hp:t xmlns:hp='urn:other'>"),
            plain.replace("<hp:t>", "<hp:ctrl/><hp:t>"),
            plain.replace("<hp:t>", "<hp:run/><hp:t>"),
            plain.replace("<hp:t>", "text outside t<hp:t>"),
            plain.replace("<hp:t>", "<![CDATA[text outside t]]><hp:t>"),
            plain.replace("<hp:t>", "&amp;<hp:t>"),
            plain.replace("<hp:t>", "<?unknown x?><hp:t>"),
            plain.replace("<hp:t>", "<!--unknown--><hp:t>"),
            plain.replace(" pageBreak='0'", ""),
            plain.replace(" columnBreak='0'", " columnBreak='false'"),
            plain.replace(" merged='0'", " merged='1'"),
            plain.replace("textpos='0'", "unknown='1'"),
        ] {
            assert!(check(&bad, section).is_err(), "{bad}");
        }
    }
}

// 이것을 실패시키는 것: 대상마다 전체 요소·슬롯·범위를 재검색하거나 정렬·전진 커서를 우회.
#[test]
fn batch_plain_source_work_scales_with_paragraphs_and_deletions() {
    let measure = |n: usize| {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            (0..n).map(|_| paragraph("본문")).collect(),
            PageSettings::a4(),
        ));
        let base = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let decoded = HwpxDecoder::decode(&base).unwrap();
        let original = xml(&base);
        // Reverse source order makes sorting observable; keep only the section-properties paragraph.
        let indices: Vec<_> = (1..n).rev().collect();
        crate::work::WORK.with(|w| w.set(0));
        delete_source::INDEX_LOOKUPS.with(|c| c.set((0, 0)));
        delete_source::MARKUP_PATH.with(|c| c.set((0, 0)));
        delete_source::check(
            &original,
            SECTION,
            &decoded.source_maps[0],
            0,
            &indices,
            &decoded.document.sections()[0].paragraphs,
        )
        .unwrap();
        assert_eq!(delete_source::INDEX_LOOKUPS.with(|c| c.get()), (n - 1, n - 1));
        let (sort_keys, advances) = delete_source::MARKUP_PATH.with(|c| c.get());
        assert!(sort_keys >= n - 1);
        assert_eq!(advances, n - 1);
        let work = crate::work::WORK.with(|w| w.get());
        // A bad paragraph at either end must still be checked after sorting and cursor movement.
        for index in [1, n - 1] {
            let span = &decoded.source_maps[0].paragraphs[index].outer;
            let mut bad = original.clone();
            bad.insert_str(span.start + "<hp:p".len(), " mystery='1'");
            let mapped = replace(&base, SECTION, &original, &bad);
            let mapped = HwpxDecoder::decode(&mapped).unwrap();
            assert!(delete_source::check(
                &bad,
                SECTION,
                &mapped.source_maps[0],
                0,
                &indices,
                &mapped.document.sections()[0].paragraphs,
            )
            .is_err());
        }
        work
    };
    let small = measure(160);
    let large = measure(320);
    eprintln!("plain source work: 160={small}, 320={large}");
    assert!(small > 0 && large > small && large <= small * 2 + 64);
}

#[path = "preserved_loss_tests.rs"]
mod losses;

// 이것을 실패시키는 것: 보존 delete에서 범주 안의 축을 제한하거나 전체 높이·연속 표의 마지막 치환 누락.
#[test]
fn inline_six_column_tables_zero_heights_through_public_delete() {
    let mut cases = crate::derived_values::tests::inline_single_axis_tables();
    cases.push(crate::derived_values::tests::inline_six_column_table());
    for (index, inline) in cases.into_iter().enumerate() {
        let base = table_base();
        let original = xml(&base);
        let source = references::source(&original).unwrap();
        let table = source.elements().iter().filter(|e| e.name() == "tbl").nth(1).unwrap().outer();
        let inline = inline
            .replace("<hp:run>칸</hp:run>", "<hp:run charPrIDRef='0'><hp:t>칸</hp:t></hp:run>");
        let mut base = replace(&base, SECTION, &original[table], &inline);
        if index == 5 {
            let d = HwpxDecoder::decode(&base).unwrap();
            let x = xml(&base);
            let host = &x[d.source_maps[0].paragraphs[2].outer.clone()];
            let extra_host = host.replace("id=\"2\"", "id=\"900\"").replace("id='7'", "id='8'");
            base = replace(&base, SECTION, host, &(host.to_owned() + &extra_host));
        }
        let expected_warnings = HwpxDecoder::decode(&base).unwrap().warnings;
        let result =
            HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET])
                .unwrap();
        let output = xml(&result.value);
        let count = [1, 1, 1, 1, 1, 2][index];
        assert_eq!(output.matches("height='0' heightRelTo='ABSOLUTE'").count(), count);
        assert!(!output.contains("height='3130'"));
        assert!(output.contains(&inline.replace("height='3130'", "height='0'")));
        assert_eq!(output.matches("height='1565'").count(), 12 * count);
        assert_eq!(output.matches("height=\"321\" heightRelTo=").count(), 1);
        assert_eq!(result.decode_warnings, expected_warnings);
        let before = RawPackage::read(&base).unwrap();
        let after = RawPackage::read(&result.value).unwrap();
        assert_eq!(
            before.read_text_entry("Contents/header.xml").unwrap(),
            after.read_text_entry("Contents/header.xml").unwrap()
        );
        let bad = replace(
            &base,
            SECTION,
            &inline,
            &inline.replace("zOrder='3'", "zOrder='238'").replace("zOrder='0'", "zOrder='238'"),
        );
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&bad, &[TARGET]).unwrap_err(),
            original_error(&bad)
        );
    }
}

fn inline_mixed_height_policy_base() -> (Vec<u8>, String, String) {
    use hwpforge_core::{Image, ImageFormat};
    use hwpforge_foundation::HwpUnit;
    let image = Image::new(
        "swatch.png",
        HwpUnit::new(1000).unwrap(),
        HwpUnit::new(1000).unwrap(),
        ImageFormat::Png,
    );
    let picture = Paragraph::with_runs(
        vec![Run::image(image, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![table_host(), paragraph("삭제"), table_host(), picture],
        PageSettings::a4(),
    ));
    let mut images = ImageStore::new();
    images.insert(
        "swatch.png",
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 176, 89,
            16, 0, 0, 2, 72, 1, 45, 108, 176, 0, 156, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ],
    );
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &images,
    )
    .unwrap();
    let original = xml(&base);
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let picture_xml = &original[decoded.source_maps[0].paragraphs[3].outer.clone()];
    let source = references::source(&original).unwrap();
    let table = source.elements().iter().filter(|e| e.name() == "tbl").nth(1).unwrap().outer();
    let a = crate::derived_values::tests::inline_tall_table();
    let a = a
        .replace("<hp:run>칸</hp:run>", "<hp:run charPrIDRef='0'><hp:t>칸</hp:t></hp:run>")
        .replacen("</hp:subList>", &format!("{picture_xml}</hp:subList>"), 1);
    let b = crate::derived_values::tests::inline_grid(1, 1, 1, 1)
        .replace("id='7'", "id='8'")
        .replace("height='3130'", "height='1282'")
        .replace("height='1565'", "height='1282'")
        .replace("<hp:run>칸</hp:run>", "<hp:run charPrIDRef='0'><hp:t>칸</hp:t></hp:run>");
    let host = &original[decoded.source_maps[0].paragraphs[2].outer.clone()];
    let second_host = host.replace(&original[table.clone()], &b).replace("id=\"2\"", "id=\"900\"");
    let base = replace(&base, SECTION, picture_xml, &second_host);
    let base = replace(&base, SECTION, &original[table], &a);
    (base, a, b)
}

// 이것을 실패시키는 것: zOrder 237·표 폭 16084·안 여백 283·바깥 여백 0의 결합을 거부.
// 이것을 실패시키는 것: 마지막 높이 누락·셀 XML 재인코딩·바이너리/관계 누락·둘째 표 거부 무시.
#[test]
fn inline_mixed_height_policies_preserve_picture_cells_and_atomicity() {
    let (base, a, b) = inline_mixed_height_policy_base();
    let before = RawPackage::read(&base).unwrap();
    let output =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET]).unwrap();
    assert_eq!(output.decode_warnings, HwpxDecoder::decode(&base).unwrap().warnings);
    let out = xml(&output.value);
    assert!(out.contains(&a.replace("height='52523'", "height='0'")));
    assert!(out.contains(&b.replace("height='1282' heightRelTo=", "height='0' heightRelTo=")));
    assert!(!out.contains("<hp:t>삭제</hp:t>"));
    let after = RawPackage::read(&output.value).unwrap();
    for path in before.entry_paths().filter(|p| *p != SECTION) {
        assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
    }
    for bad in [
        b.replace("height='1282' heightRelTo=", "height='52524' heightRelTo="),
        b.replace("</hp:tbl>", "<hp:caption/></hp:tbl>"),
    ] {
        let input = replace(&base, SECTION, &b, &bad);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
            original_error(&input)
        );
    }
}

// 이것을 실패시키는 것: 첫 영향 표의 NA0 범위를 둘째 NA1 표에 재사용하여 셀 높이 6261을 승인.
#[test]
fn inline_mixed_height_policies_reject_second_table_cell_height_outside_own_policy() {
    let (base, _, b) = inline_mixed_height_policy_base();
    // 표 전체 높이 1282는 유지하고 셀 높이만 NA0 안·NA1 밖인 6261로 바꾼다.
    let bad =
        b.replace("<hp:cellSz width='7969' height='1282'", "<hp:cellSz width='7969' height='6261'");
    assert_ne!(bad, b);
    let input = replace(&base, SECTION, &b, &bad);
    // Err에는 출력 bytes가 없으며 첫 표까지 포함한 요청 전체가 실패해야 한다.
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
        original_error(&input)
    );
}

// 이것을 실패시키는 것: 첫 영향 표의 NA1 범위를 둘째 NA0 표에 재사용하여 셀 높이 282를 거부.
#[test]
fn inline_mixed_height_policies_accept_reversed_order_with_minimum_cells() {
    let (base, a, b) = inline_mixed_height_policy_base();
    let minimum = a
        .replace("height='1565'", "height='282'")
        .replace("height='3130'", "height='282'")
        .replace("height='19637'", "height='282'");
    let input = replace(&base, SECTION, &b, &minimum);
    let input = replace(&input, SECTION, &a, &b);
    let output =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&input, &[TARGET]).unwrap();
    assert_eq!(output.decode_warnings, HwpxDecoder::decode(&input).unwrap().warnings);
    let out = xml(&output.value);
    let first = b.replace("height='1282' heightRelTo=", "height='0' heightRelTo=");
    let second = minimum.replace("height='52523'", "height='0'");
    assert!(out.contains(&first));
    assert!(out.contains(&second));
    assert!(out.find(&first).unwrap() < out.find(&second).unwrap());
    assert!(!out.contains("<hp:t>삭제</hp:t>"));
    let before = RawPackage::read(&input).unwrap();
    let after = RawPackage::read(&output.value).unwrap();
    for path in before.entry_paths().filter(|p| *p != SECTION) {
        assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
    }
}
