use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::{render_document, PdfInput, PdfOptions, RenderFailureMode};
use std::path::PathBuf;

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
    options.failure_mode = RenderFailureMode::Degraded;
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
            eprintln!("PAGES = {pages} (렌더러 집계 {}, 한컴 실측 9)", out.pages);
            eprintln!("warnings = {kinds:?}");
        }
        Err(e) => eprintln!("REJECTED: {e}"),
    }
}
