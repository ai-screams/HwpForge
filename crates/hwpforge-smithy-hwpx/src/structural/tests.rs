use super::*;
use hwpforge_core::control::{Control, RefTarget};
use hwpforge_core::{ObjectId, Paragraph, Run, Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, RefContentType, RefType};

#[test]
fn insert_fragment_encode_failure_is_validation() {
    // 이것을 실패시키는 것: 삽입 fragment 호출 목적을 Admission으로 변경.
    let doc =
        HwpxDecoder::decode(include_bytes!("../../../hwpforge/tests/data/edit-empty-section.hwpx"))
            .unwrap();
    assert!(matches!(
        encode_insert_fragment(&doc),
        Err(StructuralEditError::Codec(CodecFailure { stage: CodecStage::Validation, .. }))
    ));
}

#[test]
fn generated_fragment_and_output_decode_are_verification() {
    // 이것을 실패시키는 것: 생성 fragment·출력 decode stage를 Decode로 변경.
    let error = encoded_paragraph_spans("<hs:sec><hp:p>").unwrap_err();
    assert!(matches!(
        error,
        StructuralEditError::Codec(CodecFailure {
            stage: CodecStage::Verification,
            code: crate::HwpxErrorCode::XmlParse,
            ..
        })
    ));
    assert!(matches!(
        encoded_paragraph_spans("<other/>"),
        Err(StructuralEditError::Invariant { .. })
    ));
    // 이것을 실패시키는 것: self_verify의 decode 실패 stage를 Decode로 변경.
    let (doc, _) = admit(&fixture("plain_paragraphs.hwpx")).unwrap();
    let error = self_verify(b"bad", &doc).unwrap_err();
    assert!(matches!(
        error,
        StructuralEditError::Codec(CodecFailure {
            stage: CodecStage::Verification,
            code: crate::HwpxErrorCode::Zip,
            ..
        })
    ));
}

#[test]
fn mapper_preserves_typed_failures() {
    // 이것을 실패시키는 것: typed Codec·Invariant detail 삭제, 예상 밖 helper 오류 detail 삭제.
    for stage in [
        CodecStage::Decode,
        CodecStage::Admission,
        CodecStage::Validation,
        CodecStage::Encode,
        CodecStage::Verification,
    ] {
        let failure = CodecFailure {
            stage,
            code: crate::HwpxErrorCode::XmlParse,
            detail: "original detail".into(),
        };
        assert_eq!(
            map_admission(StamperError::Codec(failure.clone())),
            StructuralEditError::Codec(failure)
        );
    }
    assert_eq!(
        map_admission(StamperError::Invariant { detail: "source".into() }),
        StructuralEditError::Invariant { detail: "source".into() }
    );
    assert_eq!(
        map_admission(StamperError::NotRoundTripSafe {
            component: "document".into(),
            diff_path: "$.a".into()
        }),
        StructuralEditError::NotRoundTripSafe {
            component: "document".into(),
            diff_path: "$.a".into()
        }
    );
    assert_eq!(
        map_admission(StamperError::UncarriedZipEntries { entries: vec!["extra".into()] }),
        StructuralEditError::UncarriedZipEntries { entries: vec!["extra".into()] }
    );
    assert!(
        matches!(map_admission(StamperError::ManifestInvariant { detail: "unexpected helper variant".into() }), StructuralEditError::Invariant { detail } if detail.contains("unexpected helper variant"))
    );
}

fn para() -> Paragraph {
    Paragraph::new(ParaShapeIndex::new(0))
}

fn with_control(control: Control) -> Paragraph {
    let mut p = para();
    p.runs.push(Run::control(control, CharShapeIndex::new(0)));
    p
}

fn test_splice(xml: &str, from: usize) -> String {
    let parsed = crate::decoder::section::parse_section(xml, 0, &Default::default()).unwrap();
    let section = hwpforge_core::Section::with_paragraphs(
        parsed.paragraphs,
        hwpforge_core::PageSettings::a4(),
    );
    let map = parsed.source.finish(&section, 0, "Contents/section0.xml");
    splice_paragraphs(xml, &map, &[], Some((from, 0, ""))).unwrap()
}

#[test]
fn plain_text_paragraph_has_no_reference_material() {
    let mut p = para();
    p.runs.push(Run::text("본문", CharShapeIndex::new(0)));
    let scan = scan_paragraph_references(&p);
    assert!(!scan.has_reference_material());
    assert_eq!(scan, ReferenceScan::default());
}

#[test]
fn bookmark_is_detected() {
    let scan = scan_paragraph_references(&with_control(Control::bookmark("장시작")));
    assert!(scan.bookmark);
    assert!(scan.has_reference_material());
}

#[test]
fn footnote_inst_id_is_detected() {
    let ctrl = Control::footnote_with_id(4242, vec![para()]);
    let scan = scan_paragraph_references(&with_control(ctrl));
    assert!(scan.object_id);
    assert!(scan.has_reference_material());
}

#[test]
fn footnote_without_inst_id_is_not_object_id() {
    // A footnote with inst_id = None is self-contained; nothing can
    // reference it, so it is not blocking on the object-id axis.
    let ctrl = Control::footnote(vec![para()]);
    let scan = scan_paragraph_references(&with_control(ctrl));
    assert!(!scan.object_id);
}

#[test]
fn reference_nested_in_group_children_is_detected() {
    // Serde recurses into Group children — the fill.rs walker's Group gap
    // cannot cause a miss here.
    let inner = Control::footnote_with_id(7, vec![para()]);
    let group = Control::Group {
        frame: hwpforge_core::ObjectFrame::new(),
        children: vec![hwpforge_core::control::GroupChild::Shape(inner)],
        inst_id: None,
        width: HwpUnit::ZERO,
        height: HwpUnit::ZERO,
        placement: None,
    };
    let scan = scan_paragraph_references(&with_control(group));
    assert!(scan.object_id, "footnote hidden inside a group must be seen");
}

#[test]
fn reference_nested_in_table_cell_is_detected() {
    let mut cell_para = para();
    cell_para.runs.push(Run::control(Control::bookmark("셀책갈피"), CharShapeIndex::new(0)));
    let cell = TableCell::new(vec![cell_para], HwpUnit::new(1000).unwrap());
    let table = Table::new(vec![TableRow::new(vec![cell])]);
    let mut p = para();
    p.runs.push(Run::table(table, CharShapeIndex::new(0)));
    let scan = scan_paragraph_references(&p);
    assert!(scan.bookmark, "bookmark inside a table cell must be seen");
}

#[test]
fn scan_saturates_with_all_reference_kinds_present() {
    // A paragraph carrying a bookmark AND a cross-ref AND an inst_id-bearing
    // control exercises the scan's early-out once all three axes are set.
    let mut p = para();
    p.runs.push(Run::control(Control::bookmark("b"), CharShapeIndex::new(0)));
    p.runs.push(Run::control(
        Control::CrossRef {
            target: RefTarget::Object(ObjectId::new(1)),
            ref_type: RefType::Table,
            content_type: RefContentType::Number,
            as_hyperlink: false,
            display_text: String::new(),
        },
        CharShapeIndex::new(0),
    ));
    p.runs.push(Run::control(Control::footnote_with_id(9, vec![para()]), CharShapeIndex::new(0)));
    let scan = scan_paragraph_references(&p);
    assert!(scan.bookmark && scan.cross_ref && scan.object_id);
}

#[test]
fn cross_ref_is_detected() {
    let ctrl = Control::CrossRef {
        target: RefTarget::Object(ObjectId::new(9)),
        ref_type: RefType::Table,
        content_type: RefContentType::Number,
        as_hyperlink: false,
        display_text: String::new(),
    };
    let scan = scan_paragraph_references(&with_control(ctrl));
    assert!(scan.cross_ref);
    assert!(scan.has_reference_material());
}

// --: delete_paragraphs (byte-splice) ------------------------------

fn fixture(rel: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/structural")
        .join(rel);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn body_texts(bytes: &[u8]) -> Vec<String> {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    decoded.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| p.text_content().trim().to_string())
        .collect()
}

fn loc(section: usize, index: usize) -> ParagraphLocator {
    ParagraphLocator { section, index }
}

#[test]
fn delete_middle_paragraph_preserves_others() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]).expect("delete");
    assert_eq!(body_texts(&out), vec!["첫째 문단입니다.", "셋째 문단입니다.", "넷째 문단입니다."]);
}

#[test]
fn delete_batch_removes_all_targets() {
    let base = fixture("plain_paragraphs.hwpx");
    let out =
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1), loc(0, 3)]).expect("delete");
    assert_eq!(body_texts(&out), vec!["첫째 문단입니다.", "셋째 문단입니다."]);
}

#[test]
fn delete_rejects_section_properties_paragraph() {
    // Paragraph 0 carries <hp:secPr>; deleting it would reset the section
    // layout, so it is refused with a clear error (not an opaque delta
    // mismatch). Confirms the self-verify's finding is surfaced cleanly.
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 0)]),
        Err(StructuralEditError::SectionPropertiesParagraph { section: 0, index: 0 })
    ));
}

#[test]
fn delete_preserves_non_section_entries_byte_for_byte() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]).expect("delete");
    let before = RawPackage::read(&base).unwrap();
    let after = RawPackage::read(&out).unwrap();
    // Every entry except the edited section XML is byte-identical.
    for path in before.entry_paths() {
        if path == "Contents/section0.xml" {
            continue;
        }
        assert_eq!(
            before.read_text_entry(path).ok(),
            after.read_text_entry(path).ok(),
            "entry {path} must be untouched"
        );
    }
}

#[test]
fn delete_rejects_out_of_range() {
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 99)]),
        Err(StructuralEditError::ParagraphOutOfRange { .. })
    ));
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(9, 0)]),
        Err(StructuralEditError::SectionOutOfRange { .. })
    ));
}

#[test]
fn delete_rejects_duplicate_target() {
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1), loc(0, 1)]),
        Err(StructuralEditError::DuplicateTarget { .. })
    ));
}

#[test]
fn delete_rejects_emptying_the_section() {
    let base = fixture("plain_paragraphs.hwpx");
    let all = [loc(0, 0), loc(0, 1), loc(0, 2), loc(0, 3)];
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &all),
        Err(StructuralEditError::EmptySection { section: 0 })
    ));
}

#[test]
fn delete_rejects_reference_bearing_paragraph() {
    let base = fixture("crossref_para.hwpx");
    // p0 carries a footnote (inst_id), p2 a cross-ref — both blocking.
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 0)]),
        Err(StructuralEditError::ReferenceStranded { .. })
    ));
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 2)]),
        Err(StructuralEditError::ReferenceStranded { .. })
    ));
}

#[test]
fn delete_rejects_hard_break_paragraph() {
    let base = fixture("page_break.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]),
        Err(StructuralEditError::HardBreakLoss { .. })
    ));
}

#[test]
fn delete_rejects_non_hwpx_bytes() {
    assert!(matches!(
        HwpxStructuralEditor::delete_paragraphs(b"not a zip", &[loc(0, 0)]),
        Err(StructuralEditError::Codec(_))
    ));
}

#[test]
fn strip_line_segs_drops_caches_from_index_onward() {
    // The stale-cache stripping used by delete/insert: paragraphs before
    // the edit keep their cache, the rest are dropped so the renderer
    // recomputes their (shifted) cumulative positions.
    let xml = concat!(
        "<hs:sec>",
        "<hp:p id=\"0\"><hp:run><hp:t>a</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"0\"/></hp:linesegarray></hp:p>",
        "<hp:p id=\"1\"><hp:run><hp:t>b</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"1600\"/></hp:linesegarray></hp:p>",
        "<hp:p id=\"2\"><hp:run><hp:t>c</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"3200\"/></hp:linesegarray></hp:p>",
        "</hs:sec>"
    );
    assert_eq!(xml.matches("<hp:linesegarray").count(), 3);
    let out = test_splice(xml, 1);
    assert_eq!(out.matches("<hp:linesegarray").count(), 1, "only p0 keeps its cache");
    for t in ["<hp:t>a</hp:t>", "<hp:t>b</hp:t>", "<hp:t>c</hp:t>"] {
        assert!(out.contains(t), "text {t} must be preserved");
    }
}

// --: insert_paragraph (byte-splice) -------------------------------

fn insertion(section: usize, index: usize, pos: InsertPosition, text: &str) -> Insertion {
    Insertion { anchor: loc(section, index), position: pos, text: text.to_string() }
}

#[test]
fn insert_after_anchor_adds_paragraph_and_preserves_others() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 1, InsertPosition::After, "새로 삽입된 문단"),
    )
    .expect("insert");
    assert_eq!(
        body_texts(&out),
        vec![
            "첫째 문단입니다.",
            "둘째 문단입니다.",
            "새로 삽입된 문단",
            "셋째 문단입니다.",
            "넷째 문단입니다."
        ]
    );
}

#[test]
fn insert_before_anchor_adds_paragraph() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 2, InsertPosition::Before, "삽입"),
    )
    .expect("insert");
    assert_eq!(
        body_texts(&out),
        vec![
            "첫째 문단입니다.",
            "둘째 문단입니다.",
            "삽입",
            "셋째 문단입니다.",
            "넷째 문단입니다."
        ]
    );
}

#[test]
fn insert_inherits_anchor_shape() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 1, InsertPosition::After, "상속확인"),
    )
    .expect("insert");
    let decoded = HwpxDecoder::decode(&out).unwrap();
    let anchor = &decoded.document.sections()[0].paragraphs[1];
    let inserted = &decoded.document.sections()[0].paragraphs[2];
    assert_eq!(inserted.para_shape_id, anchor.para_shape_id);
    assert_eq!(inserted.runs[0].char_shape_id, anchor.runs[0].char_shape_id);
    assert!(!inserted.page_break && !inserted.column_break);
}

#[test]
fn insert_rejects_multiline_text() {
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraph(
            &base,
            &insertion(0, 1, InsertPosition::After, "줄1\n줄2")
        ),
        Err(StructuralEditError::MultiParagraphText)
    ));
}

#[test]
fn insert_rejects_before_section_properties_paragraph() {
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraph(
            &base,
            &insertion(0, 0, InsertPosition::Before, "맨앞")
        ),
        Err(StructuralEditError::InsertBeforeSectionProperties { section: 0, index: 0 })
    ));
}

#[test]
fn insert_after_section_properties_paragraph_is_allowed() {
    // Inserting AFTER the secPr carrier keeps secPr in first position.
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 0, InsertPosition::After, "둘째 앞"),
    )
    .expect("insert");
    assert_eq!(body_texts(&out)[0], "첫째 문단입니다.");
    assert_eq!(body_texts(&out)[1], "둘째 앞");
}

#[test]
fn insert_escapes_special_characters() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 1, InsertPosition::After, "a < b & c > d"),
    )
    .expect("insert");
    assert_eq!(body_texts(&out)[2], "a < b & c > d");
}

#[test]
fn insert_rejects_out_of_range_anchor() {
    let base = fixture("plain_paragraphs.hwpx");
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraph(
            &base,
            &insertion(0, 99, InsertPosition::After, "x")
        ),
        Err(StructuralEditError::ParagraphOutOfRange { .. })
    ));
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraph(&base, &insertion(9, 0, InsertPosition::After, "x")),
        Err(StructuralEditError::SectionOutOfRange { .. })
    ));
}

#[test]
fn insert_rejects_non_hwpx_bytes() {
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraph(
            b"not a zip",
            &insertion(0, 0, InsertPosition::After, "x")
        ),
        Err(StructuralEditError::Codec(_))
    ));
}

fn paragraph_ids(bytes: &[u8]) -> Vec<u32> {
    let pkg = RawPackage::read(bytes).unwrap();
    let xml = pkg.read_text_entry("Contents/section0.xml").unwrap();
    let mut ids = Vec::new();
    let mut rest = xml.as_str();
    while let Some(rel) = rest.find("<hp:p ") {
        rest = &rest[rel + 6..];
        if let Some(idrel) = rest.find("id=\"") {
            let after = &rest[idrel + 4..];
            if let Some(q) = after.find('"') {
                if let Ok(v) = after[..q].parse::<u32>() {
                    ids.push(v);
                }
            }
        }
    }
    ids
}

#[test]
fn insert_keeps_paragraph_ids_unique_and_contiguous() {
    // A transplanted paragraph must not duplicate a wire id.
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 1, InsertPosition::After, "삽입"),
    )
    .expect("insert");
    let ids = paragraph_ids(&out);
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>(), "ids must be 0..N contiguous");
}

#[test]
fn delete_keeps_paragraph_ids_contiguous() {
    // Delete must not leave a gap in the sequential ids.
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 2)]).expect("delete");
    let ids = paragraph_ids(&out);
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>(), "ids must be 0..N contiguous");
}

#[test]
fn delete_no_targets_is_byte_identical_noop() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[]).expect("noop");
    assert_eq!(out, base, "empty delete must return the input unchanged");
}

#[test]
fn strip_line_segs_targets_only_direct_child_of_paragraph() {
    // A paragraph whose <hp:p> hosts a nested table: the cell paragraph's
    // linesegarray appears earlier in byte order, but only the paragraph's
    // own direct-child cache must be stripped.
    let xml = concat!(
        "<hs:sec>",
        "<hp:p id=\"0\"><hp:run><hp:t>x</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"0\"/></hp:linesegarray></hp:p>",
        "<hp:p id=\"1\"><hp:run><hp:tbl rowCnt=\"1\" colCnt=\"1\"><hp:tr><hp:tc><hp:subList>",
        "<hp:p id=\"0\"><hp:run><hp:t>cell</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"9\"/></hp:linesegarray></hp:p>",
        "</hp:subList></hp:tc></hp:tr></hp:tbl></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"1600\"/></hp:linesegarray></hp:p>",
        "</hs:sec>"
    );
    let out = test_splice(xml, 1);
    // The paragraph's own cache (vertpos 1600) is gone; the cell cache stays.
    assert!(!out.contains("vertpos=\"1600\""), "top-level cache must be stripped");
    assert!(out.contains("vertpos=\"9\""), "nested cell cache must be preserved");
    assert!(out.contains("vertpos=\"0\""), "pre-edit cache preserved");
}

#[test]
// 이것을 실패시키는 것: 디코더의 id 값 구간을 무시하거나 소비처에서 id= 문자열 재검색.
fn renumber_paragraph_ids_rewrites_sequentially_and_skips_missing_id() {
    // Direct helper coverage: sequential rewrite + a `<hp:p>` with no id
    // attribute (the skip branch) + values preserved.
    let xml = concat!(
        "<hs:sec>",
        "<hp:p data-id=\"untouched\" id = '7'><hp:run><hp:t>a</hp:t></hp:run></hp:p>",
        "<hp:p paraPrIDRef=\"3\"><hp:run><hp:t>b</hp:t></hp:run></hp:p>",
        "<hp:p id=\"9\"><hp:run><hp:t>c</hp:t></hp:run></hp:p>",
        "</hs:sec>"
    );
    let out = test_splice(xml, usize::MAX);
    // First paragraph id 7→0; second has no id (unchanged); third 9→2.
    assert!(out.contains("<hp:p data-id=\"untouched\" id = '0'>"), "out: {out}");
    assert!(out.contains("<hp:p paraPrIDRef=\"3\">"), "no-id paragraph untouched");
    assert!(out.contains("<hp:p id=\"2\">"), "out: {out}");
    for t in ["<hp:t>a</hp:t>", "<hp:t>b</hp:t>", "<hp:t>c</hp:t>"] {
        assert!(out.contains(t));
    }
}

#[test]
fn paragraph_spans_errors_without_a_section_root() {
    // 이것을 실패시키는 것: 확정 section root 부재를 Invariant 대신 Codec으로 반환.
    assert!(matches!(
        encoded_paragraph_spans("<other/>"),
        Err(StructuralEditError::Invariant { .. })
    ));
}

#[test]
fn strip_line_segs_from_index_past_end_is_noop() {
    let xml = concat!(
        "<hs:sec><hp:p id=\"0\"><hp:run><hp:t>x</hp:t></hp:run>",
        "<hp:linesegarray><hp:lineseg vertpos=\"0\"/></hp:linesegarray></hp:p></hs:sec>"
    );
    let out = test_splice(xml, 5);
    assert_eq!(out, xml, "no paragraph at or past the index → unchanged");
}

#[test]
fn self_verify_flags_a_declared_delta_mismatch() {
    // Safety-net coverage: feed self_verify an "expected" document that
    // does not match the bytes → the reverse-delta guard must fire.
    let base = fixture("plain_paragraphs.hwpx");
    let decoded = HwpxDecoder::decode(&base).unwrap();
    // Valid bytes decode to 4 paragraphs; claim a 3-paragraph expected.
    let mut wrong = decoded.document.clone();
    wrong.sections_mut()[0].paragraphs.pop();
    let expected = HwpxDocument {
        source_maps: Vec::new(),
        source_basis: None,
        warnings: Vec::new(),
        document: wrong,
        style_store: decoded.style_store.clone(),
        image_store: decoded.image_store.clone(),
    };
    assert!(matches!(
        self_verify(&base, &expected),
        Err(StructuralEditError::DeltaMismatch { .. })
    ));
}

#[test]
fn self_verify_passes_for_matching_expected() {
    let base = fixture("plain_paragraphs.hwpx");
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let expected = HwpxDocument {
        source_maps: Vec::new(),
        source_basis: None,
        warnings: Vec::new(),
        document: decoded.document.clone(),
        style_store: decoded.style_store.clone(),
        image_store: decoded.image_store.clone(),
    };
    assert!(self_verify(&base, &expected).is_ok());
}

/// Rewrites `Contents/header.xml` of a package, keeping every other entry.
fn with_header(base: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
    let mut zout = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let name = f.name().expect("ZIP entry name").to_string();
        let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == "Contents/header.xml" {
            data = edit(std::str::from_utf8(&data).unwrap()).into_bytes();
        }
        zout.start_file(name, opts).unwrap();
        zout.write_all(&data).unwrap();
    }
    zout.finish().unwrap().into_inner()
}

// Documents converted from HWP5 carry names like `' 윤명조440'`. The
// encoder writes font names trimmed, so the decoder must read them trimmed
// too, or decode→encode→decode differs and every edit is refused.
// 이것을 실패시키는 것: 디코더(`decoder/header.rs`)에서 `clean_font_name` 을 빼는 것 —
// `NotRoundTripSafe` (style_store … face_name) 로 거부된다.
#[test]
fn admit_accepts_a_font_name_with_edge_spaces() {
    let base = fixture("plain_paragraphs.hwpx");
    let spaced = with_header(&base, |h| {
        let i = h.find(" face=\"").expect("a font face") + " face=\"".len();
        format!("{} {}", &h[..i], &h[i..])
    });
    admit(&spaced).expect("a leading space in a font name is not a round-trip hazard");
}

#[test]
fn every_error_variant_renders_a_message() {
    // 이것을 실패시키는 것: StructuralEditError::Codec Display를 빈 문자열로 변경.
    // Exercises the Display impl for the whole closed rejection set — each
    // fail-closed reason must produce a human-readable diagnostic.
    let variants = [
        StructuralEditError::Codec(crate::CodecFailure {
            stage: crate::CodecStage::Encode,
            code: crate::HwpxErrorCode::Zip,
            detail: "x".into(),
        }),
        StructuralEditError::NotRoundTripSafe {
            component: "document".into(),
            diff_path: "$".into(),
        },
        StructuralEditError::UncarriedZipEntries { entries: vec!["a".into()] },
        StructuralEditError::SectionOutOfRange { section: 1, available: 1 },
        StructuralEditError::ParagraphOutOfRange { section: 0, index: 9, available: 4 },
        StructuralEditError::DuplicateTarget { section: 0, index: 1 },
        StructuralEditError::ReferenceStranded { section: 0, index: 1 },
        StructuralEditError::HardBreakLoss { section: 0, index: 1 },
        StructuralEditError::EmptySection { section: 0 },
        StructuralEditError::SectionPropertiesParagraph { section: 0, index: 0 },
        StructuralEditError::SpanCountMismatch { section: 0, decoded: 4, wire: 5 },
        StructuralEditError::DeltaMismatch { detail: "d".into() },
        StructuralEditError::MultiParagraphText,
        StructuralEditError::InsertBeforeSectionProperties { section: 0, index: 0 },
    ];
    for v in &variants {
        assert!(!v.to_string().is_empty(), "variant {v:?} must render");
    }
}

// -- followups: batch insert + delete warnings ---------------------

#[test]
fn insert_batch_matches_sequential_single_inserts() {
    // Differential lock: batch-N must equal N sequential
    // single inserts byte-for-byte — same splice product, one admission.
    let base = fixture("plain_paragraphs.hwpx");
    let texts = vec!["추가 하나.".to_string(), "추가 둘.".to_string()];
    let batch =
        HwpxStructuralEditor::insert_paragraphs(&base, loc(0, 1), InsertPosition::After, &texts)
            .expect("batch insert");

    let s1 = HwpxStructuralEditor::insert_paragraph(
        &base,
        &insertion(0, 1, InsertPosition::After, "추가 하나."),
    )
    .expect("single 1");
    let s2 = HwpxStructuralEditor::insert_paragraph(
        &s1,
        &insertion(0, 2, InsertPosition::After, "추가 둘."),
    )
    .expect("single 2");

    assert_eq!(batch, s2, "batch must equal sequential singles byte-for-byte");
    assert_eq!(
        body_texts(&batch),
        vec![
            "첫째 문단입니다.",
            "둘째 문단입니다.",
            "추가 하나.",
            "추가 둘.",
            "셋째 문단입니다.",
            "넷째 문단입니다."
        ]
    );
}

#[test]
fn insert_batch_after_last_paragraph_appends() {
    // Boundary: anchoring After the final paragraph must land the block
    // at the section tail (off-by-one guard exercises the upper edge).
    let base = fixture("plain_paragraphs.hwpx");
    let texts = vec!["꼬리 하나.".to_string(), "꼬리 둘.".to_string()];
    let out =
        HwpxStructuralEditor::insert_paragraphs(&base, loc(0, 3), InsertPosition::After, &texts)
            .expect("append");
    assert_eq!(
        body_texts(&out),
        vec![
            "첫째 문단입니다.",
            "둘째 문단입니다.",
            "셋째 문단입니다.",
            "넷째 문단입니다.",
            "꼬리 하나.",
            "꼬리 둘."
        ]
    );
}

#[test]
fn insert_batch_empty_texts_is_byte_identical_noop() {
    let base = fixture("plain_paragraphs.hwpx");
    let out = HwpxStructuralEditor::insert_paragraphs(&base, loc(0, 1), InsertPosition::After, &[])
        .expect("noop");
    assert_eq!(out, base, "empty batch must return the input unchanged");
}

#[test]
fn insert_batch_rejects_multiline_item() {
    let base = fixture("plain_paragraphs.hwpx");
    let texts = vec!["정상.".to_string(), "줄\n바꿈".to_string()];
    assert!(matches!(
        HwpxStructuralEditor::insert_paragraphs(&base, loc(0, 1), InsertPosition::After, &texts),
        Err(StructuralEditError::MultiParagraphText)
    ));
}

#[test]
fn insert_batch_keeps_paragraph_ids_unique_and_contiguous() {
    // For a batch edit: the transplanted block
    // must renumber into one contiguous id space.
    let base = fixture("plain_paragraphs.hwpx");
    let texts = vec!["일.".to_string(), "이.".to_string(), "삼.".to_string()];
    let out =
        HwpxStructuralEditor::insert_paragraphs(&base, loc(0, 2), InsertPosition::Before, &texts)
            .expect("batch insert");
    let ids = paragraph_ids(&out);
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>(), "ids must be 0..N contiguous");
}

#[test]
fn scan_delete_warnings_flags_index_mark_paragraphs() {
    // 이것을 실패시키는 것: scan_delete_warnings가 빈 목록으로 조기 성공.
    // Build a package whose paragraph 2 carries an index mark, then scan
    // a mixed target set: only the index-mark paragraph warns; the plain
    // paragraph and the out-of-range target stay silent (best-effort).
    let base = fixture("plain_paragraphs.hwpx");
    let d0 = HwpxDecoder::decode(&base).expect("decode");
    let mut document = d0.document.clone();
    document.sections_mut()[0].paragraphs[2].add_run(Run::control(
        Control::IndexMark { primary: "색인어".to_string(), secondary: None },
        hwpforge_foundation::CharShapeIndex::new(0),
    ));
    let bytes = encode_hwpx(
        &HwpxDocument {
            source_maps: Vec::new(),
            source_basis: None,
            document,
            style_store: d0.style_store.clone(),
            image_store: d0.image_store.clone(),
            warnings: Vec::new(),
        },
        crate::stamp::EncodePurpose::Output,
    )
    .expect("encode");

    let warnings = scan_delete_warnings(&bytes, &[loc(0, 1), loc(0, 2), loc(0, 99)]);
    assert_eq!(
        warnings,
        vec![StructuralWarning::IndexMarkRemoved { section: 0, index: 2, count: 1 }]
    );

    // Undecodable input: advisory scan stays silent (editor errors later).
    assert!(scan_delete_warnings(b"not a package", &[loc(0, 0)]).is_empty());
}

#[test]
fn structural_warning_renders_a_message() {
    let one = StructuralWarning::IndexMarkRemoved { section: 0, index: 2, count: 1 };
    let many = StructuralWarning::IndexMarkRemoved { section: 1, index: 3, count: 2 };
    assert!(one.to_string().contains("index-mark entry"), "{one}");
    assert!(many.to_string().contains("index-mark entries"), "{many}");
}
