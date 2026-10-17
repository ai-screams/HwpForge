//! Field policy and independent-flow validation through the public Core API.
use hwpforge_core::control::{FieldBoundary, FieldStart};
use hwpforge_core::field_inventory::{FieldEditability, FieldInventory};
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
fn start(id: &str, kind: &str) -> Run {
    let mut s = FieldStart::default();
    s.id = id.into();
    s.kind = kind.into();
    s.name = "value".into();
    Run::control(
        Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) },
        CharShapeIndex::new(0),
    )
}
fn end(id: &str) -> Run {
    Run::control(
        Control::FieldBoundary {
            boundary: FieldBoundary::End { begin_id: id.into(), field_id: String::new() },
        },
        CharShapeIndex::new(0),
    )
}
fn text(s: &str) -> Run {
    Run::text(s, CharShapeIndex::new(0))
}
fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}
fn document(runs: Vec<Run>) -> Document {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    doc
}

/// 이것을 실패시키는 것: 부모 흐름의 시작과 각주·메모의 끝을 짝짓기.
#[test]
fn field_ids_are_local_to_independent_flows() {
    let note = |runs| Run::control(Control::footnote(vec![para(runs)]), CharShapeIndex::new(0));
    let valid = document(vec![
        start("7", "FORMULA"),
        note(vec![start("7", "FORMULA"), end("7")]),
        end("7"),
    ]);
    assert!(valid.validate().is_ok());
    let invalid = document(vec![start("7", "FORMULA"), note(vec![end("7")])]);
    assert_eq!(FieldInventory::from_sections(invalid.sections()).diagnostics().len(), 2);
    assert!(invalid.validate().is_err());
    let memo = Run::control(
        Control::memo_with_anchor(vec![para(vec![end("7")])], vec![start("7", "FORMULA")]),
        CharShapeIndex::new(0),
    );
    assert!(document(vec![memo]).validate().is_err());
}

/// 이것을 실패시키는 것: 불완전 경계의 종류·순서·중복 ID 검사를 생략하기.
#[test]
fn malformed_ranges_fail_validation_and_empty_clear_is_valid() {
    for runs in [
        vec![start("7", "FORMULA")],
        vec![end("7")],
        vec![start("7", "FORMULA"), end("8")],
        vec![end("7"), start("7", "FORMULA")],
        vec![start("7", "FORMULA"), start("7", "FORMULA"), end("7")],
        vec![start("7", "FORMULA"), end("7"), start("7", "FORMULA"), end("7")],
        vec![start("", "FORMULA"), end("")],
    ] {
        assert!(document(runs).validate().is_err());
    }
    assert!(document(vec![start("7", "FORMULA"), end("7")]).validate().is_ok());
    // Crossing valid ranges remain representable; neither owns an editable value.
    assert!(document(vec![
        start("7", "FORMULA"),
        start("8", "FORMULA"),
        text("3"),
        end("7"),
        end("8")
    ])
    .validate()
    .is_ok());
}

/// 이것을 실패시키는 것: 자동 필드 안의 누름틀에 독립적인 편집 권한을 주기.
#[test]
fn protected_outer_range_blocks_inner_click_here() {
    let mut doc = document(vec![
        start("7", "FORMULA"),
        start("8", "CLICK_HERE"),
        text("3"),
        end("8"),
        end("7"),
    ]);
    let snapshot = doc.clone();
    let inventory = FieldInventory::from_sections(snapshot.sections());
    let field = inventory.fields().iter().find(|f| f.kind() == "CLICK_HERE").unwrap();
    assert_ne!(field.editability(), FieldEditability::Editable);
    assert!(!FieldInventory::replace_value(doc.sections_mut(), field, "999"));
}

/// 이것을 실패시키는 것: stale·중복 요청을 일부 적용하거나 빈 clear 를 거부하기.
#[test]
fn batch_edit_is_atomic_and_rejects_stale_entries() {
    let mut doc = document(vec![
        start("7", "CLICK_HERE"),
        text("old"),
        end("7"),
        start("8", "CLICK_HERE"),
        text("other"),
        end("8"),
    ]);
    let snapshot = doc.clone();
    let inventory = FieldInventory::from_sections(snapshot.sections());
    let a = &inventory.fields()[0];
    let b = &inventory.fields()[1];
    let before = doc.sections().to_vec();
    assert!(!FieldInventory::replace_values(doc.sections_mut(), &[(a, "new"), (a, "twice")]));
    assert_eq!(doc.sections(), before);
    assert!(FieldInventory::replace_values(doc.sections_mut(), &[(a, ""), (b, "new")]));
    assert!(!FieldInventory::replace_value(doc.sections_mut(), a, "stale"));
    assert!(doc.validate().is_ok());
}

/// 이것을 실패시키는 것: 본문 run 하나인 문단 간 범위를 편집 가능으로 승인하기.
#[test]
fn cross_paragraph_single_run_is_not_a_scalar_value() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![para(vec![start("7", "CLICK_HERE")]), para(vec![text("one"), end("7")])],
        PageSettings::a4(),
    ));
    let snapshot = doc.clone();
    let fields = FieldInventory::from_sections(snapshot.sections());
    assert_eq!(fields.fields()[0].editability(), FieldEditability::UnsupportedRange);
}

/// 이것을 실패시키는 것: 자동 범위가 감싸는 독립 흐름의 값을 편집 가능으로 두기.
#[test]
fn protected_range_covers_values_in_contained_notes() {
    let note = Run::control(
        Control::footnote(vec![para(vec![start("8", "CLICK_HERE"), text("3"), end("8")])]),
        CharShapeIndex::new(0),
    );
    let doc = document(vec![start("7", "FORMULA"), note, end("7")]);
    let snapshot = doc.clone();
    let fields = FieldInventory::from_sections(snapshot.sections());
    let field = fields.fields().iter().find(|f| f.kind() == "CLICK_HERE").unwrap();
    assert_eq!(field.editability(), FieldEditability::UnsupportedRange);
}

/// 이것을 실패시키는 것: 캡션 번호를 쪽 번호 종류로 잘못 분류하기.
#[test]
fn automatic_number_kinds_are_distinct_and_protected() {
    use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, InlinePageKind};
    let doc = document(vec![
        Run::control(
            Control::AutoNumber {
                kind: AutoNumberKind::Picture,
                num: 3,
                format: AutoNumberFormat::default(),
            },
            CharShapeIndex::new(0),
        ),
        Run::control(
            Control::InlinePageNumber { kind: InlinePageKind::CurrentPage },
            CharShapeIndex::new(0),
        ),
    ]);
    let snapshot = doc.clone();
    let inventory = FieldInventory::from_sections(snapshot.sections());
    assert_eq!(inventory.fields()[0].kind(), "AUTO_NUMBER");
    assert_eq!(inventory.fields()[1].kind(), "PAGE_NUM");
    assert!(inventory.fields().iter().all(|f| f.editability() == FieldEditability::Protected));
}

/// 이것을 실패시키는 것: 범위의 현재 값에서 접힌 필드·링크·메모 앵커를 빼기.
#[test]
fn range_current_includes_folded_visible_values() {
    let mut folded = Control::field("hint");
    if let Control::Field { display_text, .. } = &mut folded {
        *display_text = "inner".into();
    }
    let doc = document(vec![
        start("7", "CLICK_HERE"),
        text("before"),
        Run::control(folded, CharShapeIndex::new(0)),
        Run::control(Control::hyperlink("link", "https://example.org"), CharShapeIndex::new(0)),
        Run::control(
            Control::memo_with_anchor(
                Vec::new(),
                vec![
                    text("memo"),
                    Run::control(
                        Control::hyperlink("anchor", "https://example.org"),
                        CharShapeIndex::new(0),
                    ),
                ],
            ),
            CharShapeIndex::new(0),
        ),
        text("after"),
        end("7"),
    ]);
    let snapshot = doc.clone();
    let fields = FieldInventory::from_sections(snapshot.sections());
    let outer = fields.fields().iter().find(|f| f.id().as_deref() == Some("7")).unwrap();
    assert_eq!(outer.current(), "beforeinnerlinkmemoanchorafter");
    assert_eq!(outer.editability(), FieldEditability::UnsupportedRange);
}

#[test]
fn legacy_memo_metadata_keeps_authored_defaults() {
    let mut old = serde_json::to_value(hwpforge_core::MemoMetadata::default()).unwrap();
    old.as_object_mut().unwrap().remove("preserve_empty");
    let mut metadata: hwpforge_core::MemoMetadata = serde_json::from_value(old).unwrap();
    assert!(!metadata.preserve_empty);
    assert_eq!(metadata.hwpx_id(), "memo0");
    metadata.preserve_empty = true;
    assert_eq!(metadata.hwpx_id(), "");
    metadata.id = "authored-id".into();
    assert_eq!(metadata.hwpx_id(), "authored-id");
}

/// 이것을 실패시키는 것: 원본 패키지 없이 임의 runs[3] 주소의 XML 증거를 승인.
#[test]
fn source_claim_without_original_package_cannot_authorize_an_arbitrary_address() {
    let mut doc = document(vec![text("a"), text("b"), text("c"), start("open", "FORMULA")]);
    let (_, source) = hwpforge_core::source_xml::XmlSource::read(
        "<sec><p><run><fieldBegin id=\"open\"/></run></p></sec>",
    )
    .unwrap();
    let origin = source
        .unclosed_origin(
            4,
            hwpforge_core::node_path::CorePath::new(0, "paragraphs[0].runs[3]"),
            "Contents/section0.xml",
        )
        .unwrap();
    FieldInventory::recover_source_ranges(doc.sections_mut(), &[origin], &[]);
    assert!(doc.clone().validate().is_err());
    let mut submitted = doc.clone();
    FieldInventory::restore_source_provenance(submitted.sections_mut(), doc.sections());
    assert!(submitted.validate().is_err());
}

#[test]
fn source_recovery_does_not_hide_duplicate_empty_or_backward_boundaries() {
    for runs in [
        vec![start("", "FORMULA")],
        vec![start("7", "FORMULA"), start("7", "FORMULA")],
        vec![end("7"), start("7", "FORMULA")],
        vec![start("7", "FORMULA"), end("8")],
    ] {
        let mut doc = document(runs);
        FieldInventory::recover_source_ranges(doc.sections_mut(), &[], &[]);
        assert_eq!(FieldInventory::from_sections(doc.sections()).warnings().len(), 0);
        assert!(doc.validate().is_err());
    }
    let mut old = serde_json::to_value(FieldStart::default()).unwrap();
    old.as_object_mut().unwrap().remove("unclosed");
    assert!(!serde_json::from_value::<FieldStart>(old).unwrap().unclosed);
}

/// 이것을 실패시키는 것: 각 범위에 본문 스냅샷을 저장하거나 중첩마다 run 저장소를 만들기.
#[test]
fn overlapping_ranges_store_one_reference_per_run() {
    for n in [1, 16, 128, 1024] {
        let mut runs: Vec<_> = (0..n).map(|i| start(&i.to_string(), "FORMULA")).collect();
        runs.push(text(&"x".repeat(1024 * 1024)));
        runs.extend((0..n).map(|i| end(&i.to_string())));
        let doc = document(runs);
        let fields = FieldInventory::from_sections(doc.sections());
        assert_eq!(fields.stored_run_count(), 2 * n + 1);
        assert_eq!(fields.fields().len(), n);
        for field in fields.fields() {
            let original = field
                .body()
                .iter()
                .find(|b| matches!(b.content(), hwpforge_core::RunContent::Text(_)))
                .unwrap();
            assert!(std::ptr::eq(
                original.content(),
                &doc.sections()[0].paragraphs[0].runs[n].content
            ));
        }
    }
}
/// 이것을 실패시키는 것: 기존 run 만 보호하고 문단·run 사이 삽입 위치 무시.
#[test]
fn preflight_covers_paragraphs_and_insertion_gaps() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            para(vec![start("a", "FORMULA")]),
            para(vec![text("3")]),
            para(vec![end("a")]),
            para(vec![text("after")]),
        ],
        PageSettings::a4(),
    ));
    let fields = FieldInventory::from_sections(doc.sections());
    for p in 0..3 {
        assert!(fields.protects_paragraph(0, "paragraphs", p));
    }
    assert!(!fields.protects_paragraph(0, "paragraphs", 3));
    assert!(!fields.protects_insertion(0, "paragraphs", 0, None));
    assert!(fields.protects_insertion(0, "paragraphs", 0, Some(1)));
    assert!(fields.protects_insertion(0, "paragraphs", 1, None));
    assert!(fields.protects_insertion(0, "paragraphs", 2, None));
    assert!(!fields.protects_insertion(0, "paragraphs", 3, None));
}

/// 이것을 실패시키는 것: XML 출처 검사에서 빈 ID·중복 시작·끝의 존재 무시.
#[test]
fn xml_evidence_refuses_ambiguous_or_closed_starts() {
    for body in [
        "<fieldBegin id=\"\"/>",
        "<fieldBegin id=\"a\"/><fieldBegin id=\"a\"/>",
        "<fieldBegin id=\"a\"/><fieldEnd beginIDRef=\"a\"/>",
        "<fieldEnd beginIDRef=\"a\"/><fieldBegin id=\"a\"/>",
    ] {
        let (_, source) = hwpforge_core::source_xml::XmlSource::read(&format!(
            "<sec><p><run>{body}</run></p></sec>"
        ))
        .unwrap();
        for id in 1..source.elements().len() {
            assert!(
                source
                    .unclosed_origin(
                        id,
                        hwpforge_core::node_path::CorePath::new(0, "paragraphs[0].runs[0]"),
                        "Contents/section0.xml"
                    )
                    .is_none(),
                "{body}"
            );
        }
    }
    assert!(hwpforge_core::source_xml::XmlSource::read("<sec _hwpforge_origin=\"3\"/>").is_err());
}

/// 이것을 실패시키는 것: Editable 필드마다 전체 필드 보호 검색을 수행하기.
#[test]
fn editable_protection_work_scales_linearly() {
    let mut work = Vec::new();
    for n in [500, 4_000] {
        let runs =
            (0..n).map(|_| Run::control(Control::field("hint"), CharShapeIndex::new(0))).collect();
        let doc = document(runs);
        let inventory = FieldInventory::from_sections(doc.sections());
        assert!(inventory.fields().iter().all(|f| f.editability() == FieldEditability::Editable));
        work.push(inventory.protection_work());
    }
    assert!(work[1] <= work[0] * 8 + 8, "{work:?}");
    assert!(work[1] >= 4_000, "counter must include actual sweep work");
    println!("protection work 500 -> 4000: {work:?}");
}
