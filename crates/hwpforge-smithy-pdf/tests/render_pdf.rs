//! W2d 통합 게이트 — 실물 fixture 를 PDF 바이트로 렌더.
//!
//! fixture(한컴 재저장 hwpx)와 한컴 폰트 번들이 모두 있는 머신에서만
//! 실행된다 (CI 는 둘 다 없음 — fixture-optional 관례).

use std::path::PathBuf;

use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::{render_document, FontFallbackMode, PdfInput, PdfOptions};

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

/// blank-HPC 실전 렌더 프로브 (수동 — W4c 폰트 파이프라인 통과 확인).
///
/// 실측 (2026-08-09): 렌더 run 의 hangul 축 폰트 8종 중 7종(휴먼명조·
/// 맑은 고딕·HY헤드라인M=H2HDRM·굴림·휴먼고딕·함초롬바탕·HY견고딕)은
/// 한컴 번들 name table 로 해석되고, "한양중고딕"(1 run)은 한컴 내부 별칭
/// DB 없이는 미해결 — regular face 미해결은 모드 무관 fatal 이 정직한
/// 경계다 (no-fallback). 축 불일치 run 30% 는 Degraded 로 경고 표면화.
#[test]
#[ignore = "manual probe against untracked blank-HPC review artifact"]
fn probe_blank_hpc_full_render_degraded() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/blank-hpc-application-2026.hwpx");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("blank-HPC not present — skip");
        return;
    };
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    let mut options = PdfOptions::default();
    options.discovery = FontDiscovery::HancomBundle;
    options.font_fallback = FontFallbackMode::Degraded;
    match render_document(&input, &options) {
        Ok(out) => {
            let mut kinds = std::collections::BTreeMap::new();
            for w in &out.warnings {
                *kinds
                    .entry(format!("{w:?}").split('{').next().unwrap().trim().to_string())
                    .or_insert(0usize) += 1;
            }
            let hay = String::from_utf8_lossy(&out.bytes);
            let pages = hay.matches("/Type/Page").count() - hay.matches("/Type/Pages").count();
            eprintln!("PAGES = {pages} (한컴 실측 9)");
            eprintln!("warnings = {kinds:?}");
        }
        Err(e) => eprintln!("REJECTED: {e}"),
    }
}
