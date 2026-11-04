use super::*;
use crate::line_keep::width;
use crate::style_store::HwpxCharShape;
use crate::test_support::xml;
use hwpforge_core::layout::{LineBoundary, LineSeg, WireTokenKind};
use hwpforge_core::{ImageStore, PageSettings, Section, TableRow};
use hwpforge_foundation::HwpUnit;

fn section_allows(xml: &str) -> Option<()> {
    crate::line_keep::source_allows(&crate::line_keep::SectionEvidence::read(xml)?, true)
        .then_some(())
}

fn paragraph(text: &str) -> Paragraph {
    let mut para =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    let mut line = LineSeg::default();
    line.horzsize = 40000;
    line.vertsize = 1000;
    line.textheight = 1000;
    line.baseline = 850;
    para.layout_cache = Some(LayoutCache::new(vec![line]));
    para
}
fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(crate::style_store::HwpxParaShape::default());
    styles
}
fn fixture() -> Vec<u8> {
    let mut document = Document::new();
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![paragraph("가나")],
        HwpUnit::new(8000).unwrap(),
    )])]);
    let mut host = paragraph("앞");
    host.runs.push(Run::table(table, CharShapeIndex::new(0)));
    document.add_section(Section::with_paragraphs(vec![host, paragraph("뒤")], PageSettings::a4()));
    let bytes = HwpxEncoder::encode_with_options(
        &document.validate().unwrap(),
        &styles(),
        &ImageStore::new(),
        crate::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    fn count(node: &xml::Element) -> usize {
        usize::from(node.name == "hp:lineseg") + node.children.iter().map(count).sum::<usize>()
    }
    let section =
        RawPackage::read(&bytes).unwrap().read_text_entry("Contents/section0.xml").unwrap();
    assert_eq!(count(&xml::Element::parse(&section)), 3);
    bytes
}
fn specs() -> Vec<CellSpec> {
    vec![CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: "다".into() }]
}

/// 이것을 실패시키는 것: 단일 문단/일반 Text run/세그먼트, 끝 표시, textpos,
/// boundary, 빈 원본·허용 밖 원본 문자 거부를 각각 삭제.
#[test]
fn core_candidate_rejects_each_unsupported_shape() {
    let cell = TableCell::new(vec![paragraph("가나")], HwpUnit::new(8000).unwrap());
    let styles = styles();
    assert!(eligible(&cell, "다", &styles).is_some());
    let reject = |c: TableCell| assert!(eligible(&c, "다", &styles).is_none());
    let mut c = cell.clone();
    c.paragraphs.push(paragraph("라"));
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].runs.push(Run::text("라", CharShapeIndex::new(0)));
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].runs.clear();
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].runs[0].content = RunContent::Table(Box::new(Table::new(vec![])));
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].end_char_shape = Some(CharShapeIndex::new(0));
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].layout_cache = None;
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].layout_cache.as_mut().unwrap().lines.push(LineSeg::default());
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].layout_cache.as_mut().unwrap().lines[0].textpos = 1;
    reject(c);
    let mut c = cell.clone();
    c.paragraphs[0].layout_cache.as_mut().unwrap().lines[0].boundary =
        Some(LineBoundary::new(vec![WireTokenKind::Table], None));
    reject(c);
    for text in ["", "A", "%", "１", "\u{2003}"] {
        let mut c = cell.clone();
        c.paragraphs[0].runs[0].content = RunContent::Text(text.into());
        reject(c);
    }
    for size in [0, -1] {
        let mut c = cell.clone();
        c.paragraphs[0].layout_cache.as_mut().unwrap().lines[0].horzsize = size;
        reject(c);
    }
}

/// 이것을 실패시키는 것: 올림 삭제, 음수 자간을 폭에서 빼기, height/relSz/ratio 양수 검사 삭제, checked 곱셈을 wrapping으로 바꾸기.
#[test]
fn width_uses_checked_ceil_and_positive_metrics() {
    let mut shape =
        HwpxCharShape { height: HwpUnit::new(1001).unwrap(), ..HwpxCharShape::default() };
    assert_eq!(width("가", &shape), Some(1052));
    shape.spacing = -10;
    assert_eq!(width("가", &shape), Some(1052));
    shape.spacing = 1;
    assert_eq!(width("가", &shape), Some(1062));
    shape.spacing = 0;
    for invalid in [0, -1] {
        let mut s = shape.clone();
        s.height = HwpUnit::new(invalid).unwrap();
        assert_eq!(width("가", &s), None);
        let mut s = shape.clone();
        s.rel_sz = invalid;
        assert_eq!(width("가", &s), None);
        let mut s = shape.clone();
        s.ratio = invalid;
        assert_eq!(width("가", &s), None);
    }
    shape.height = HwpUnit::new(100_000_000).unwrap();
    shape.rel_sz = i32::MAX;
    shape.ratio = i32::MAX;
    assert_eq!(width("가나", &shape), None);
    shape.rel_sz = 100;
    assert_eq!(width("가나", &shape), None);
    shape.rel_sz = i32::MAX;
    shape.ratio = 1;
    shape.spacing = 0;
    assert_eq!(width("가나", &shape), None);
}

/// 이것을 실패시키는 것: 원본 위치 누락에서 panic, header 속성 누락/heading
/// 검사를 생략, paraPr 이외 요소를 정의 순서에 포함.
#[test]
fn missing_source_and_ambiguous_attributes_are_not_evidence() {
    let bytes = fixture();
    let package = RawPackage::read(&bytes).unwrap();
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    let edits = resolve_edits(&decoded.document, &specs()).unwrap();
    assert_eq!(candidates(&decoded, &package, &edits).len(), 1);
    decoded.source_maps.clear();
    assert!(candidates(&decoded, &package, &edits).is_empty());
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let mut extension = RawPackage::read(&bytes).unwrap();
    let header = extension
        .read_text_entry("Contents/header.xml")
        .unwrap()
        .replace("<hh:paraPr id=", "<hh:unused/><hh:paraPr id=");
    extension.replace_text_entry("Contents/header.xml", header);
    assert_eq!(candidates(&decoded, &extension, &edits).len(), 1);
    let mut missing = RawPackage::read(&bytes).unwrap();
    missing.replace_text_entry("Contents/header.xml", "<head/>".into());
    assert!(candidates(&decoded, &missing, &edits).is_empty());
    for (from, to) in [
        ("fontLineHeight=\"0\"", ""),
        ("condense=\"0\"", ""),
        ("eAsianEng=\"0\"", ""),
        ("eAsianNum=\"0\"", ""),
        ("lineWrap=\"BREAK\"", ""),
        ("type=\"NONE\"", "type=\"BULLET\""),
    ] {
        let mut package = RawPackage::read(&bytes).unwrap();
        let header = package.read_text_entry("Contents/header.xml").unwrap().replace(from, to);
        package.replace_text_entry("Contents/header.xml", header);
        assert!(candidates(&decoded, &package, &edits).is_empty(), "{from}");
    }
}

/// 이것을 실패시키는 것: marker 후보 취소 삭제, 빈 후보에도 source 디코드 또는 probe 인코드.
#[test]
fn dropped_candidate_leaves_its_change_in_the_probe() {
    let bytes = fixture();
    let package = RawPackage::read(&bytes).unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let baseline = HwpxEncoder::encode(
        &decoded.document.clone().validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let edits = resolve_edits(&decoded.document, &specs()).unwrap();
    let mut candidates = candidates(&decoded, &package, &edits);
    assert_eq!(candidates.len(), 1);
    let mut document = decoded.document;
    apply_resolved(&mut document, edits);
    // A source encoder refusal must revoke the candidate even if the filter passed.
    candidates[0].cache.lines[0].textpos = 100;
    candidates[0].cache.lines[0].boundary =
        Some(LineBoundary::new(vec![WireTokenKind::Table], None));
    restore_caches(&mut document, &candidates);
    let validated = document.clone().validate().unwrap();
    let output =
        HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).unwrap();
    let source =
        crate::layout_carry::cache_source(&validated, &decoded.style_store, &decoded.image_store)
            .unwrap();
    cancel_dropped(&source, &mut candidates);
    assert!(candidates.is_empty());
    let work = hwpforge_core::source_xml::source_work();
    cancel_dropped(&source, &mut candidates);
    assert_eq!(hwpforge_core::source_xml::source_work(), work);
    assert!(probe(document, &decoded.style_store, &decoded.image_store, &candidates)
        .unwrap()
        .is_none());
    let (output, _) =
        crate::layout_carry::carry_line_segs(&bytes, &baseline, &output, &output, &source).unwrap();
    let xml = RawPackage::read(&output).unwrap().read_text_entry("Contents/section0.xml").unwrap();
    let tree = xml::Element::parse(&xml);
    assert!(!xml.contains("<hp:linesegarray"));
    assert!(tree.children.iter().all(|p| p.children.iter().all(|c| c.name != "hp:linesegarray")));
}

/// 이것을 실패시키는 것: 후보마다 paraPr 전체를 다시 필터링하거나 header/구역 재조회,
/// probe를 두 번 인코드, 후보 없어도 probe 인코드.
#[test]
fn source_queries_and_probe_encodes_have_fixed_batch_cost() {
    for definitions in [1, 32, 256] {
        let mut decoded = HwpxDecoder::decode(&fixture()).unwrap();
        for _ in 1..definitions {
            decoded.style_store.push_para_shape(crate::style_store::HwpxParaShape::default());
        }
        // Reference the last definition, so lookup cost cannot hide an early exit.
        for_each_table_mut(&mut decoded.document, &mut |_, table| {
            table.rows[0].cells[0].paragraphs[0].para_shape_id =
                ParaShapeIndex::new(definitions - 1);
        });
        // Distinct table hosts within one section share the same source part.
        let host = decoded.document.sections()[0].paragraphs[0].clone();
        for _ in 1..8 {
            decoded.document.sections_mut()[0].paragraphs.push(host.clone());
        }
        let bytes = HwpxEncoder::encode_with_options(
            &decoded.document.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
            crate::EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        for count in [0, 1, 8] {
            let specs: Vec<_> =
                (0..count).map(|table| CellSpec { table, ..specs()[0].clone() }).collect();
            COST.set((0, 0, 0, 0, 0));
            let edited = HwpxCellEditor::set_cells(&bytes, &specs).unwrap();
            let output = HwpxDecoder::decode(&edited.bytes).unwrap();
            // 이것을 실패시키는 것: 승인한 후보의 자기 캐시 복원 생략.
            assert!(tables_in_document(&output.document).iter().all(|entry| entry.table.rows[0]
                .cells[0]
                .paragraphs[0]
                .layout_cache
                .is_some()));
            let active = usize::from(count > 0);
            assert_eq!(
                COST.get().4,
                active * definitions,
                "definitions={definitions}, candidates={count}"
            );
            assert_eq!(COST.get(), (active, active, 3 * count, active, active * definitions));
        }
    }
}

// 이것을 실패시키는 것: 구역 파싱 실패를 캐시에 넣기 전에 반환하거나 실패한 증거를 허용.
#[test]
fn failed_section_evidence_is_read_once_per_part() {
    let mut decoded = HwpxDecoder::decode(&fixture()).unwrap();
    let host = decoded.document.sections()[0].paragraphs[0].clone();
    for _ in 1..8 {
        decoded.document.sections_mut()[0].paragraphs.push(host.clone());
    }
    let bytes = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        crate::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let specs: Vec<_> = (0..8).map(|table| CellSpec { table, ..specs()[0].clone() }).collect();
    let edits = resolve_edits(&decoded.document, &specs).unwrap();
    let mut package = RawPackage::read(&bytes).unwrap();
    assert_eq!(candidates(&decoded, &package, &edits).len(), 8);
    // Keep the recorded cell ranges valid; only the section evidence parser fails.
    let xml = package
        .read_text_entry("Contents/section0.xml")
        .unwrap()
        .replace("</hs:sec>", r#"<hp:unused duplicate="1" duplicate="2"/></hs:sec>"#);
    assert!(crate::line_keep::SectionEvidence::read(&xml).is_none());
    package.replace_text_entry("Contents/section0.xml", xml);
    COST.set((0, 0, 0, 0, 0));
    assert!(candidates(&decoded, &package, &edits).is_empty());
    assert_eq!(COST.get().1, 1, "failed section evidence is cached for all eight cells");
}

/// 이것을 실패시키는 것: 후보가 없어도 캐시를 다시 붙이거나 probe에서 텍스트를 되돌리기.
#[test]
fn no_candidates_match_the_existing_bytes_and_diagnostics() {
    let bytes = fixture();
    let mut specs = specs();
    specs[0].text = "A\u{1}".into();
    let actual = HwpxCellEditor::set_cells_with_diagnostics(&bytes, &specs).unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let baseline =
        crate::stamp::encode_hwpx(&decoded, crate::stamp::EncodePurpose::Admission).unwrap();
    let mut document = decoded.document;
    specs[0].text = "A".into();
    apply_set_cells(&mut document, &specs).unwrap();
    let validated = document.validate().unwrap();
    let encoded = HwpxEncoder::encode_with_diagnostics(
        &validated,
        &decoded.style_store,
        &decoded.image_store,
        crate::EncodeOptions::default(),
    )
    .unwrap();
    let source =
        crate::layout_carry::cache_source(&validated, &decoded.style_store, &decoded.image_store)
            .unwrap();
    let (expected, dropped) = crate::layout_carry::carry_line_segs(
        &bytes,
        &baseline,
        &encoded.bytes,
        &encoded.bytes,
        &source,
    )
    .unwrap();
    assert_eq!(actual.value.bytes, expected);
    let mut warnings = vec![EncodeWarning::XmlForbiddenCharsRemoved {
        part: "Contents/section0.xml".into(),
        location: "table 0 cell (0, 0)".into(),
        count: 1,
    }];
    warnings.extend(encoded.warnings);
    warnings.extend(dropped);
    assert_eq!(actual.encode_warnings, warnings);
    assert_eq!(actual.decode_warnings, decoded.warnings);
}

/// 이것을 실패시키는 것: 복수 root/미완결 XML/직접 자식 중복 허용, 빈 요소 처리 반전.
#[test]
fn source_tags_require_a_unique_well_formed_element() {
    assert!(Tag::read("<p/><p/>").is_none());
    assert!(Tag::read("<p a='0' a='1'/>").is_none());
    assert!(Tag::read("<p><a></p>").is_none());
    assert!(Tag::read("<p><a/>").is_none());
    assert!(Tag::read("<p><subList/><subList/></p>").unwrap().child("subList").is_none());
    assert!(Tag::read("<p><subList textDirection='HORIZONTAL'/></p>")
        .unwrap()
        .child("subList")
        .unwrap()
        .is("textDirection", "HORIZONTAL"));
}

/// 이것을 실패시키는 것: 원본 구역의 파일명 대응을 spine 대응으로 변경(공유 carry의 기존 바이트 계약).
#[test]
fn reversed_spine_keeps_the_existing_missing_cache_result() {
    let base = fixture();
    let mut decoded = HwpxDecoder::decode(&base).unwrap();
    let cached = decoded.document.sections()[0].clone();
    decoded.document.strip_layout_caches();
    decoded.document.add_section(cached);
    let bytes = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        crate::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let mut package = RawPackage::read(&bytes).unwrap();
    let content = package.read_text_entry("Contents/content.hpf").unwrap();
    let content = content
        .replace("idref=\"section0\"", "idref=\"SWAP\"")
        .replace("idref=\"section1\"", "idref=\"section0\"")
        .replace("idref=\"SWAP\"", "idref=\"section1\"");
    package.replace_text_entry("Contents/content.hpf", content);
    let input = package.write().unwrap();
    let decoded = HwpxDecoder::decode(&input).unwrap();
    assert!(decoded.document.sections()[0].paragraphs[1].layout_cache.is_some());
    let edited = HwpxCellEditor::set_cells(&input, &specs()).unwrap();
    let output = RawPackage::read(&edited.bytes).unwrap();
    let tree = xml::Element::parse(&output.read_text_entry("Contents/section0.xml").unwrap());
    assert!(tree.children.iter().all(|p| p.children.iter().all(|c| c.name != "hp:linesegarray")));
}

/// 이것을 실패시키는 것: marker가 하나라도 있으면 모든 후보 취소, 같은 host의 경고 경로로 후보 취소, 구역 번호 무시,
/// 취소 전 후보를 probe에 사용하여 취소된 편집까지 되돌리기.
#[test]
fn markers_cancel_only_their_cells_across_sibling_tables_and_sections() {
    let bytes = fixture();
    let mut decoded = HwpxDecoder::decode(&bytes).unwrap();
    let run = decoded.document.sections()[0].paragraphs[0].runs[1].clone();
    decoded.document.sections_mut()[0].paragraphs[0].runs.push(run);
    // Rebind the host cache to both sibling tables in the encoded fixture.
    decoded.document.sections_mut()[0].paragraphs[0].layout_cache.as_mut().unwrap().lines[0]
        .boundary = None;
    let section = decoded.document.sections()[0].clone();
    decoded.document.add_section(section);
    let bytes = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        crate::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    for section in decoded.document.sections() {
        assert!(section.paragraphs[0].layout_cache.is_some());
        assert!(section.paragraphs[1].layout_cache.is_some());
    }
    let baseline = HwpxEncoder::encode(
        &decoded.document.clone().validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let specs: Vec<_> = (0..4).map(|table| CellSpec { table, ..specs()[0].clone() }).collect();
    let edits = resolve_edits(&decoded.document, &specs).unwrap();
    let mut candidates = candidates(&decoded, &RawPackage::read(&bytes).unwrap(), &edits);
    assert_eq!(candidates.len(), 4);
    for i in [0, 2] {
        candidates[i].cache.lines[0].textpos = 100;
    }
    let mut document = decoded.document;
    apply_resolved(&mut document, edits);
    restore_caches(&mut document, &candidates);
    let validated = document.clone().validate().unwrap();
    let output =
        HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).unwrap();
    let source =
        crate::layout_carry::cache_source(&validated, &decoded.style_store, &decoded.image_store)
            .unwrap();
    cancel_dropped(&source, &mut candidates);
    assert_eq!(candidates.iter().map(|c| c.table).collect::<Vec<_>>(), [1, 3]);
    let probe =
        probe(document, &decoded.style_store, &decoded.image_store, &candidates).unwrap().unwrap();
    let reverted = HwpxDecoder::decode(&probe).unwrap();
    let texts: Vec<_> = tables_in_document(&reverted.document)
        .iter()
        .map(|entry| entry.table.rows[0].cells[0].paragraphs[0].text_content())
        .collect();
    assert_eq!(texts, ["다", "가나", "다", "가나"]);
    let (carried, _) =
        crate::layout_carry::carry_line_segs(&bytes, &baseline, &output, &probe, &source).unwrap();
    let carried = HwpxDecoder::decode(&carried).unwrap();
    for (ordinal, entry) in tables_in_document(&carried.document).iter().enumerate() {
        let paragraph = &entry.table.rows[0].cells[0].paragraphs[0];
        assert_eq!(paragraph.text_content(), "다");
        if let Some(candidate) = candidates.iter().find(|c| c.table == ordinal) {
            assert_eq!(paragraph.layout_cache.as_ref(), Some(&candidate.cache));
        } else {
            assert!(paragraph.layout_cache.is_none(), "cancelled table {ordinal}");
        }
    }
    for (section, body) in carried.document.sections().iter().enumerate() {
        assert!(body.paragraphs[0].layout_cache.is_none(), "host in section {section}");
        assert!(body.paragraphs[1].layout_cache.is_none(), "following flow in section {section}");
        assert_eq!(body.paragraphs[1].text_content(), "뒤");
    }
}

/// 이것을 실패시키는 것: 축소 상대크기+양수 자간 거부 삭제, 폭의 공백 가중치 삭제.
#[test]
fn width_rejects_unmeasured_spacing_and_matches_spaced_hangul() {
    let mut shape =
        HwpxCharShape { height: HwpUnit::new(1300).unwrap(), ..HwpxCharShape::default() };
    assert_eq!(width("편집 확인 칸", &shape), Some(8125));
    shape.rel_sz = 99;
    shape.spacing = 1;
    assert_eq!(width("가", &shape), None);
    shape.spacing = 0;
    assert_eq!(width("가", &shape), Some(1352));
    shape.spacing = -1;
    assert_eq!(width("가", &shape), Some(1352));
}

/// 이것을 실패시키는 것: marker 디코드 실패를 무시하거나 후보 색인을 무검사로 읽기.
#[test]
fn unreadable_or_missing_marked_evidence_cancels_candidates() {
    let bytes = fixture();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let package = RawPackage::read(&bytes).unwrap();
    let edits = resolve_edits(&decoded.document, &specs()).unwrap();
    let fresh = || candidates(&decoded, &package, &edits);
    let mut invalid = decoded.document.clone();
    invalid.sections_mut()[0].paragraphs[1].layout_cache.as_mut().unwrap().lines[0].textpos = 100;
    let mut source = crate::layout_carry::cache_source(
        &invalid.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    assert!(source.marked_document().unwrap().is_some());
    // Each missing level must cancel only that candidate without indexing a changed shape.
    for level in 0..4 {
        let mut selected = fresh();
        match level {
            0 => selected[0].table = usize::MAX,
            1 => selected[0].row = usize::MAX,
            2 => selected[0].cell = usize::MAX,
            _ => {
                let mut empty = RawPackage::read(&bytes).unwrap();
                let mut section = empty.read_text_entry("Contents/section0.xml").unwrap();
                let text = section.find(">가나</hp:t>").unwrap();
                let start = section[..text].rfind("<hp:p ").unwrap();
                let end = text + section[text..].find("</hp:p>").unwrap() + "</hp:p>".len();
                section.replace_range(start..end, "");
                empty.replace_text_entry("Contents/section0.xml", section);
                let no_paragraph = empty.write().unwrap();
                let check = HwpxDecoder::decode(&no_paragraph).unwrap();
                assert!(tables_in_document(&check.document)[0].table.rows[0].cells[0]
                    .paragraphs
                    .is_empty());
                source.replace_bytes_for_test(no_paragraph);
            }
        }
        cancel_dropped(&source, &mut selected);
        assert!(selected.is_empty(), "level {level}");
    }
    let mut selected = fresh();
    source.replace_bytes_for_test(b"invalid package".to_vec());
    cancel_dropped(&source, &mut selected);
    assert!(selected.is_empty());
    COST.set((0, 0, 0, 0, 0));
    assert!(probe(decoded.document, &decoded.style_store, &decoded.image_store, &selected)
        .unwrap()
        .is_none());
    assert_eq!(COST.get().3, 0);
}

/// 이것을 실패시키는 것: eligible 이전에 header를 읽기.
#[test]
fn rejected_candidates_do_not_read_the_header() {
    let bytes = fixture();
    let mut rejected = specs();
    rejected[0].text = "A".into();
    COST.set((0, 0, 0, 0, 0));
    HwpxCellEditor::set_cells(&bytes, &rejected).unwrap();
    assert_eq!(COST.get(), (0, 0, 0, 0, 0));
}

/// 이것을 실패시키는 것: secPr 위치/유일성/존재 확인 삭제 또는 누락된 grid를 허용.
#[test]
fn section_grid_evidence_must_be_unique_and_explicit() {
    let grid = r#"<secPr><grid charGrid="0" wonggojiFormat="0"/></secPr>"#;
    assert!(section_allows(&format!("<sec><p><run>{grid}</run></p></sec>")).is_some());
    for xml in [
        "<sec/>",
        "<sec><p><run><secPr/></run></p></sec>",
        "<sec><p><run><secPr><grid/></secPr></run></p></sec>",
        &format!("<sec><p><run>{grid}{grid}</run></p></sec>"),
        &format!("<sec><p><run>{grid}<secPr/></run></p></sec>"),
        &format!("<sec><p><run><unknown>{grid}</unknown></run></p></sec>"),
    ] {
        assert!(section_allows(xml).is_none(), "{xml}");
    }
}

#[path = "../../tests/support/small_stack.rs"]
mod small_stack;
/// 이것을 실패시키는 것: 깊이 가드 삭제; 제한 이내에서 완성된 자식 트리의 Drop도 작은 스택에서 안전해야 한다.
#[test]
fn bounded_source_tree_drop_on_depth_rejection() {
    small_stack::small_stack_child(
        "cell_edit::line_keep::tests::bounded_source_tree_drop_on_depth_rejection",
        || {
            let depth = crate::decoder::XML_RECURSION_LIMIT - 1;
            let closed = format!("{}{}", "<x>".repeat(depth), "</x>".repeat(depth));
            assert!(Tag::read(&format!("<root>{closed}</root>")).is_some());
            let rejected = format!(
                "<root>{closed}{}{}</root>",
                "<x>".repeat(depth + 2),
                "</x>".repeat(depth + 2)
            );
            assert!(Tag::read(&rejected).is_none());
        },
    );
}

// 이것을 실패시키는 것: 칸 어댑터의 공통 charPr 검사 생략, decoder가 생략된 언어 요소에도 손실 경고.
#[test]
fn cell_candidates_reject_unmeasured_character_properties() {
    let bytes = fixture();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let edits = resolve_edits(&decoded.document, &specs()).unwrap();
    for (from, to) in [
        ("useFontSpace=\"0\"", "useFontSpace=\"1\""),
        ("</hh:charPr>", "<hh:bold/></hh:charPr>"),
        ("latin=\"100\"", "latin=\"99\""),
    ] {
        let mut package = RawPackage::read(&bytes).unwrap();
        let header = package.read_text_entry("Contents/header.xml").unwrap();
        assert!(header.contains(from));
        package.replace_text_entry("Contents/header.xml", header.replace(from, to));
        assert!(candidates(&decoded, &package, &edits).is_empty(), "{from}");
    }
    for name in ["ratio", "spacing", "relSz", "offset"] {
        let mut package = RawPackage::read(&bytes).unwrap();
        let mut header = package.read_text_entry("Contents/header.xml").unwrap();
        let start = header.find(&format!("<hh:{name} ")).unwrap();
        let end = start + header[start..].find("/>").unwrap() + 2;
        header.replace_range(start..end, "");
        package.replace_text_entry("Contents/header.xml", header);
        let decoded = HwpxDecoder::decode(&package.write().unwrap()).unwrap();
        assert!(!decoded.warnings.iter().any(|w| matches!(w, crate::decoder::DecodeWarning::ContentLost {kind,..} if kind=="char_shape_per_language")));
        assert!(candidates(&decoded, &package, &edits).is_empty(), "missing {name}");
    }
}

// 이것을 실패시키는 것: charPr 원본 authored id로 찾기, 정의 순서 대신 0 고정.
#[test]
fn character_evidence_uses_definition_positions_even_with_duplicate_ids() {
    for duplicate in [false, true] {
        let bytes = fixture();
        let mut package = RawPackage::read(&bytes).unwrap();
        let header = package.read_text_entry("Contents/header.xml").unwrap();
        let start = header.find("<hh:charPr ").unwrap();
        let end = start + header[start..].find("</hh:charPr>").unwrap() + "</hh:charPr>".len();
        let first = &header[start..end];
        let id = if duplicate { "0" } else { "7" };
        let bold = first
            .replace("id=\"0\"", &format!("id=\"{id}\""))
            .replace("</hh:charPr>", "<hh:bold/></hh:charPr>");
        package.replace_text_entry(
            "Contents/header.xml",
            header.replace(first, &format!("{bold}{}", first.replace("=\"100\"", "=\"150\""))),
        );
        let bytes = package.write().unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let header = crate::line_keep::Header::read(&package, &decoded.style_store, || {}).unwrap();
        let evidence = crate::line_keep::SectionEvidence::read(
            &package.read_text_entry("Contents/section0.xml").unwrap(),
        )
        .unwrap();
        assert!(!header.allows(&evidence, 0, 0));
        assert!(header.allows(&evidence, 0, 1));
    }
}
