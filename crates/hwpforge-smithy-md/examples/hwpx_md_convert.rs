//! HWPX → Markdown 변환 예제.
//!
//! 각 HWPX 파일을 Lossy(읽기용 GFM)와 Lossless(라운드트립용 HTML) 두 가지
//! 마크다운으로 변환합니다.
//!
//! Usage: cargo run -p hwpforge-smithy-md --example hwpx_md_convert

use std::fs;

use hwpforge_smithy_hwpx::HwpxDecoder;
use hwpforge_smithy_md::MdEncoder;

fn convert(name: &str) {
    let input_path = format!("examples/{name}.hwpx");

    // ── 1. HWPX 디코딩 ──────────────────────────────────────────
    let hwpx_bytes = fs::read(&input_path).unwrap_or_else(|e| panic!("read {input_path}: {e}"));
    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("decode hwpx");
    let validated = decoded.document.validate().expect("validate document");

    // 입력 HWPX를 출력 폴더에 복사 (hwpx2json과 동일 패턴)
    fs::copy(&input_path, format!("examples/hwpx2md/{name}.hwpx")).expect("copy input hwpx");

    // ── 2. Lossy MD (사람이 읽기 좋은 GFM) ──────────────────────
    let lossy_md = MdEncoder::encode_lossy(&validated).expect("encode lossy markdown");

    let lossy_path = format!("examples/hwpx2md/{name}.md");
    fs::write(&lossy_path, &lossy_md).expect("write lossy md");

    println!(
        "  {input_path} → hwpx2md/{name}.md (lossy, {} bytes, {} lines)",
        lossy_md.len(),
        lossy_md.lines().count()
    );

    // ── 3. Lossless MD (라운드트립용 HTML-like) ──────────────────
    // 차트/수식/호/곡선 등 미지원 Control이 포함된 문서는 lossless 변환을 건너뜁니다.
    match MdEncoder::encode_lossless(&validated) {
        Ok(lossless_md) => {
            let lossless_path = format!("examples/hwpx2md/{name}.lossless.md");
            fs::write(&lossless_path, &lossless_md).expect("write lossless md");

            println!(
                "  {input_path} → hwpx2md/{name}.lossless.md (lossless, {} bytes, {} lines)",
                lossless_md.len(),
                lossless_md.lines().count()
            );
        }
        Err(e) => {
            println!("  {input_path} → lossless 건너뜀 ({e})");
        }
    }
}

fn main() {
    let targets = ["01_text", "hwpx_complete_guide"];

    fs::create_dir_all("examples/hwpx2md").expect("create hwpx2md dir");

    println!("=== HwpForge HWPX → Markdown Convert ===\n");

    for name in &targets {
        println!("[{name}]");
        convert(name);
        println!();
    }

    println!("=== {} files processed ({} outputs) ===", targets.len(), targets.len() * 2);
}
