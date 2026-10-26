#[path = "support/fill_api.rs"]
mod shared;
use shared::*;

use std::collections::BTreeMap;
use std::path::PathBuf;

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::{FillError, HwpxDecoder, HwpxEncoder, HwpxFiller};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests/fixtures/fields");
    path.push(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn named_field(name: &str, hint: &str) -> Control {
    Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: Some(hint.to_string()),
        help_text: None,
        name: Some(name.to_string()),
        display_text: String::new(),
    }
}

fn field_paragraph(control: Control) -> Paragraph {
    let mut para = Paragraph::new(ParaShapeIndex::new(0));
    para.runs.push(Run::control(control, CharShapeIndex::new(0)));
    para
}

/// 이름 붙은 누름틀 N개를 섹션별로 배치한 HWPX 바이트를 만든다.
fn build_hwpx(sections: &[Vec<Control>]) -> Vec<u8> {
    let mut doc = Document::new();
    for controls in sections {
        let mut section = Section::new(PageSettings::default());
        for control in controls {
            section.paragraphs.push(field_paragraph(control.clone()));
        }
        doc.add_section(section);
    }
    let validated = doc.validate().expect("validate");
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    HwpxEncoder::encode(&validated, &styles, &ImageStore::default()).expect("encode")
}

// ── list_fields ─────────────────────────────────────────────────

#[test]
fn list_fields_reports_name_hint_current_and_fillable() {
    let bytes = fixture_bytes("clickhere_named.hwpx");
    let fields = HwpxFiller::list_fields(&bytes).expect("list");
    assert_eq!(fields.len(), 1);
    let f = &fields[0];
    assert_eq!(f.name.as_deref(), Some("user_email"));
    assert_eq!(f.hint.as_deref(), Some("회사 이메일을 입력하세요"));
    assert_eq!(f.current, "회사 이메일을 입력하세요");
    assert_eq!(f.section, 0);
    assert!(f.fillable, "네이티브 미채움 필드(본문=힌트)는 채움 가능");
}

// ── fill 성공 경로 ───────────────────────────────────────────────

#[test]
fn fill_replaces_named_field_body() {
    let bytes = fixture_bytes("clickhere_named.hwpx");
    let outcome =
        HwpxFiller::fill(&bytes, &values(&[("user_email", "soeburi@example.com")])).expect("fill");
    assert_eq!(outcome.filled.len(), 1);
    assert_eq!(outcome.filled[0].name, "user_email");
    assert_eq!(outcome.filled[0].previous, "회사 이메일을 입력하세요");
    assert_eq!(field_display(&outcome.bytes, 0, "user_email"), "soeburi@example.com");
}

#[test]
fn fill_spans_multiple_sections_atomically() {
    let bytes = build_hwpx(&[
        vec![named_field("과제명", "과제명을 입력하세요")],
        vec![named_field("총연구비", "금액을 입력하세요")],
    ]);
    let outcome = HwpxFiller::fill(
        &bytes,
        &values(&[("과제명", "AI 문서 자동화"), ("총연구비", "300,000천원")]),
    )
    .expect("fill");
    assert_eq!(outcome.filled.len(), 2);
    assert_eq!(field_display(&outcome.bytes, 0, "과제명"), "AI 문서 자동화");
    assert_eq!(field_display(&outcome.bytes, 1, "총연구비"), "300,000천원");
}

// ── preflight 거부 정책 ─────────────────────────────────────────

#[test]
fn fill_rejects_unknown_name_and_lists_available() {
    let bytes = fixture_bytes("clickhere_named.hwpx");
    let err = HwpxFiller::fill(&bytes, &values(&[("없는이름", "x")])).unwrap_err();
    match err {
        FillError::UnknownField { name, available } => {
            assert_eq!(name, "없는이름");
            assert_eq!(available, vec!["user_email".to_string()]);
        }
        other => panic!("expected UnknownField, got {other:?}"),
    }
}

#[test]
fn fill_rejects_duplicate_field_name() {
    let bytes =
        build_hwpx(&[vec![named_field("이름", "성명 입력"), named_field("이름", "성명 입력")]]);
    let err = HwpxFiller::fill(&bytes, &values(&[("이름", "류한율")])).unwrap_err();
    match err {
        FillError::DuplicateFieldName { name, count } => {
            assert_eq!(name, "이름");
            assert_eq!(count, 2);
        }
        other => panic!("expected DuplicateFieldName, got {other:?}"),
    }
}

#[test]
fn fill_rejects_empty_value() {
    let bytes = fixture_bytes("clickhere_named.hwpx");
    let err = HwpxFiller::fill(&bytes, &values(&[("user_email", "")])).unwrap_err();
    assert!(matches!(err, FillError::EmptyValue { name } if name == "user_email"));
}

#[test]
fn fill_is_all_or_nothing_when_one_target_fails_preflight() {
    // 두 값 중 하나(없는 이름)가 preflight 에서 실패하면 나머지도 적용되지
    // 않아야 한다 — 원본 바이트가 그대로임을 확인할 방법: fill 이 Err 를
    // 반환하므로 산출물 자체가 없다.
    let bytes = build_hwpx(&[vec![named_field("과제명", "입력")]]);
    let err =
        HwpxFiller::fill(&bytes, &values(&[("과제명", "값"), ("없는필드", "값2")])).unwrap_err();
    assert!(matches!(err, FillError::UnknownField { .. }));
}
