use std::path::PathBuf;

use hwpforge_smithy_pdf::source::replay_layout;
use hwpforge_smithy_pdf::{PdfInput, PdfOptions};

/// blank-HPC 실전 프로브 (수동 — 리뷰 영역 미추적 파일이라 fixture-optional).
#[test]
#[ignore = "manual probe against untracked blank-HPC review artifact"]
fn probe_blank_hpc_replay() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/blank-hpc-application-2026.hwpx");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("blank-HPC not present — skip");
        return;
    };
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let input = PdfInput { document: &validated, styles: &decoded.style_store };
    match replay_layout(&input, &PdfOptions::default()) {
        Ok(layout) => {
            let mut warn_kinds = std::collections::BTreeMap::new();
            for w in &layout.warnings {
                *warn_kinds
                    .entry(format!("{w:?}").split('{').next().unwrap().trim().to_string())
                    .or_insert(0usize) += 1;
            }
            eprintln!("PAGES = {} (한컴 실측 9)", layout.pages.len());
            eprintln!("warnings = {warn_kinds:?}");
        }
        Err(e) => eprintln!("REJECTED: {e}"),
    }
}
