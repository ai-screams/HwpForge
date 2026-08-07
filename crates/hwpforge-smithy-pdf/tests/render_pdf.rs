//! W2d 통합 게이트 — 실물 fixture 를 PDF 바이트로 렌더.
//!
//! fixture(한컴 재저장 hwpx)와 한컴 폰트 번들이 모두 있는 머신에서만
//! 실행된다 (CI 는 둘 다 없음 — fixture-optional 관례).

use std::path::PathBuf;

use hwpforge_smithy_pdf::{render_document, PdfInput, PdfOptions};

const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

fn fixture(name: &str) -> Option<Vec<u8>> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/pdf-rules").join(name);
    std::fs::read(path).ok()
}

fn options() -> Option<PdfOptions> {
    let dir = PathBuf::from(HANCOM_TTF_DIR);
    if !dir.exists() {
        return None;
    }
    let mut options = PdfOptions::default();
    options.font_dirs = vec![dir];
    Some(options)
}

fn render_fixture(name: &str) -> Option<hwpforge_smithy_pdf::PdfOutput> {
    let bytes = fixture(name)?;
    let options = options()?;
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    Some(render_document(&input, &options).expect("render"))
}

#[test]
fn rules_justify_renders_to_pdf_without_warnings() {
    let Some(output) = render_fixture("rules-justify.hwpx") else { return };
    assert!(output.bytes.starts_with(b"%PDF-"), "PDF 헤더");
    assert!(output.bytes.len() > 10_000, "실질 콘텐츠 ({} bytes)", output.bytes.len());
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn rules_headerfooter_renders_two_pages() {
    let Some(output) = render_fixture("rules-headerfooter.hwpx") else { return };
    // 쪽 수는 PDF 카탈로그 파싱 없이 /Type/Page 오브젝트 카운트로 확인
    // (krilla 는 공백 없는 사전 문법으로 쓴다 — 실물 출력 확인).
    let hay = String::from_utf8_lossy(&output.bytes);
    let pages = hay.matches("/Type/Page").count() - hay.matches("/Type/Pages").count();
    assert_eq!(pages, 2, "W0 실측 42+18줄 = 2쪽");
}

/// 시각 게이트 산출물 생성 (사용자 판정용) — `--ignored` 로 수동 실행.
#[test]
#[ignore = "visual gate artifact generation (writes to examples/hwp5_review/_verify)"]
fn generate_visual_gate_artifacts() {
    let out_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review/_verify/pdf-w2");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    for name in ["rules-justify", "rules-headerfooter", "rules-fonts-hbatang"] {
        let Some(output) = render_fixture(&format!("{name}.hwpx")) else {
            panic!("fixture/폰트 번들 필요: {name}");
        };
        let path = out_dir.join(format!("{name}-w2.pdf"));
        std::fs::write(&path, &output.bytes).expect("write");
        println!(
            "wrote {path:?} ({} bytes, warnings={})",
            output.bytes.len(),
            output.warnings.len()
        );
    }
}

/// 시각 게이트 산출물 확장 — 전체 텍스트 fixture (bbox 게이트용).
#[test]
#[ignore = "extended artifact generation"]
fn generate_all_text_fixture_artifacts() {
    let out_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review/_verify/pdf-w2");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    for name in ["rules-fonts-hcrbatang", "rules-fonts-hcrdotum"] {
        let Some(output) = render_fixture(&format!("{name}.hwpx")) else {
            panic!("fixture/폰트 번들 필요: {name}");
        };
        std::fs::write(out_dir.join(format!("{name}-w2.pdf")), &output.bytes).expect("write");
    }
}

/// W3 표 시각 게이트 산출물 — `--ignored` 수동 실행 (한컴 폰트 필요).
#[test]
#[ignore = "W3 table visual gate artifact generation"]
fn generate_w3_table_artifacts() {
    let out_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review/_verify/pdf-w3");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    for name in
        ["rules-table", "rules-pagespan3", "rules-pagespan3-repeat", "rules-rowspan-deficit"]
    {
        let Some(output) = render_fixture(&format!("{name}.hwpx")) else {
            panic!("fixture/폰트 번들 필요: {name}");
        };
        let path = out_dir.join(format!("{name}-w3.pdf"));
        std::fs::write(&path, &output.bytes).expect("write");
        println!(
            "wrote {path:?} ({} bytes, warnings={})",
            output.bytes.len(),
            output.warnings.len()
        );
    }
}
