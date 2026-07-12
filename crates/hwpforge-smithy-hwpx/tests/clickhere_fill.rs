//! 채워진 누름틀(ClickHere)이 HWPX round-trip 과 preserve-first patch 에서
//! 살아남는지 검증한다.
//!
//! 배경: 누름틀의 wire 는 `fieldBegin`~`fieldEnd` 사이의 평범한 `<hp:t>` 가
//! 본문(채워지는 자리)이다. 이 본문을 `Control::Field::display_text` 로
//! carry 하지 않으면 사용자가 채운 값이 round-trip 에서 힌트로 되돌아가는
//! 무음 유실이 발생한다.

use std::path::PathBuf;

use hwpforge_core::control::Control;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxPatcher};

const FILLED: &str = "hanyul@example.com";

fn fixture_bytes() -> Vec<u8> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests/fixtures/fields/clickhere_named.hwpx");
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

/// 섹션 안의 첫 `Control::Field` 를 `(name, display_text)` 로 뽑는다.
fn first_field(section: &Section) -> (Option<String>, String) {
    section
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::Field { name, display_text, .. } => {
                    Some((name.clone(), display_text.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .expect("fixture 에 누름틀이 있어야 한다")
}

/// 섹션 안의 모든 `Control::Field` 본문을 `value` 로 채운다.
fn fill_fields(section: &mut Section, value: &str) {
    for para in section.paragraphs.iter_mut() {
        for run in para.runs.iter_mut() {
            if let RunContent::Control(c) = &mut run.content {
                if let Control::Field { display_text, .. } = c.as_mut() {
                    *display_text = value.to_string();
                }
            }
        }
    }
}

/// 한컴이 **직접** 채워 저장한 누름틀의 ground-truth 게이트 (Step 0).
///
/// `clickhere_filled.hwpx` 는 macOS 한컴에서 누름틀에 값을 타이핑해 저장한
/// 파일이다 (2026-07-12). wire 확인 결과: 한컴의 채우기 표현 = 본문
/// `<hp:t>` 교체 + 필드 마커 보존 + `Direction` 힌트 불변 — Epic 1 구현과
/// 동일한 모델이다. **단, 한컴은 재저장 시 라벨 run 을 필드 run 에
/// 병합한다** (`<hp:t>이메일: </hp:t>`·fieldBegin·`<hp:t>값</hp:t>`·
/// fieldEnd·`<hp:t/>` 가 한 run). `HxRun` 은 자식 순서를 보존하지 않아
/// 이 경우 본문 귀속이 모호하다 → 라벨을 값으로 오귀속하는 대신
/// **미채움("")으로 다운그레이드**하고, patch 슬롯도 만들지 않는다
/// (decoder `unambiguous_body` · patch.rs 거울 게이트). 이 테스트는 그
/// 다운그레이드 계약과 "모호 필드가 있어도 export 는 성공한다"를 잠근다.
#[test]
fn hancom_resaved_merged_run_downgrades_to_unfilled() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests/fixtures/fields/clickhere_filled.hwpx");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()));

    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let section = decoded.document.sections()[0].clone();
    let (name, body) = first_field(&section);
    assert_eq!(name.as_deref(), Some("user_email"), "필드 이름 앵커는 살아야 한다");
    assert_eq!(body, "", "모호한 본문(병합 run)은 라벨 오귀속 대신 미채움으로 다운그레이드");

    // 모호한 필드가 있어도 preservation export 는 성공해야 한다 — 슬롯이
    // 없을 뿐 export/patch 워크플로 전체가 죽으면 main 대비 회귀다.
    let preservation = HwpxPatcher::export_section_preservation(&bytes, 0, &section)
        .expect("모호 필드가 있어도 export 는 성공해야 한다");
    assert!(
        preservation.text_slots.iter().all(|s| !s.path.ends_with(".control.field")),
        "모호한 필드는 patch 슬롯을 만들지 않는다"
    );
}

#[test]
fn filled_clickhere_survives_hwpx_round_trip() {
    let bytes = fixture_bytes();
    let original = HwpxDecoder::decode(&bytes).expect("decode");
    assert_eq!(
        first_field(&original.document.sections()[0]).0.as_deref(),
        Some("user_email"),
        "fixture 의 필드 이름 앵커"
    );

    let mut doc = original.document;
    fill_fields(&mut doc.sections_mut()[0], FILLED);

    let validated = doc.validate().expect("validate");
    let encoded = HwpxEncoder::encode(&validated, &original.style_store, &original.image_store)
        .expect("encode");
    let again = HwpxDecoder::decode(&encoded).expect("re-decode");

    assert_eq!(
        first_field(&again.document.sections()[0]).1,
        FILLED,
        "채워진 값이 round-trip 에서 유실되면 안 된다"
    );
}

#[test]
fn patch_fills_clickhere_body_preserving_everything_else() {
    let base = fixture_bytes();
    let decoded = HwpxDecoder::decode(&base).expect("decode");
    let original_section = decoded.document.sections()[0].clone();

    // 값을 채운 교체 섹션.
    let mut replacement = original_section.clone();
    fill_fields(&mut replacement, FILLED);

    let preservation = HwpxPatcher::export_section_preservation(&base, 0, &original_section)
        .expect("preservation metadata");
    let patched =
        HwpxPatcher::patch_section_preserving(&base, 0, &replacement, None, Some(&preservation))
            .expect("텍스트-전용 패치로 누름틀을 채울 수 있어야 한다");

    let again = HwpxDecoder::decode(&patched).expect("re-decode");
    let (name, body) = first_field(&again.document.sections()[0]);
    assert_eq!(body, FILLED);
    assert_eq!(name.as_deref(), Some("user_email"), "필드 이름 앵커는 보존되어야 한다");
}
