//! HWPX → Markdown 변환 예제.
//!
//! 각 HWPX 파일을 Lossy(읽기용 GFM)와 Lossless(라운드트립용 HTML) 두 가지
//! 마크다운으로 변환합니다.
//!
//! Usage: cargo run -p hwpforge-smithy-md --example hwpx_md_convert

use std::fs;

use std::path::Path;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::MdEncoder;

fn write_extracted_images(output_root: &Path, images: &std::collections::HashMap<String, Vec<u8>>) {
    for (rel_path, data) in images {
        let output_path = output_root.join(rel_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("create image parent dir");
        }
        fs::write(&output_path, data).expect("write extracted image");
    }
}

fn convert(name: &str) {
    let input_path = format!("examples/{name}.hwpx");
    let output_root = Path::new("examples/hwpx2md");

    // ── 1. HWPX 디코딩 ──────────────────────────────────────────
    let hwpx_bytes = fs::read(&input_path).unwrap_or_else(|e| panic!("read {input_path}: {e}"));
    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("decode hwpx");
    let validated = decoded.document.validate().expect("validate document");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);

    // 입력 HWPX를 출력 폴더에 복사 (hwpx2json과 동일 패턴)
    fs::copy(&input_path, output_root.join(format!("{name}.hwpx"))).expect("copy input hwpx");

    // ── 2. Styled MD (이미지/스타일 반영 GFM) ───────────────────
    let styled = MdEncoder::encode_styled(&validated, &lookup);

    let styled_path = output_root.join(format!("{name}.md"));
    fs::write(&styled_path, &styled.markdown).expect("write styled md");
    write_extracted_images(output_root, &styled.images);

    println!(
        "  {input_path} → hwpx2md/{name}.md (styled, {} bytes, {} lines, {} images)",
        styled.markdown.len(),
        styled.markdown.lines().count(),
        styled.images.len()
    );

    // ── 3. Lossless MD (라운드트립용 HTML-like) ──────────────────
    // 차트/수식/호/곡선 등 미지원 Control이 포함된 문서는 lossless 변환을 건너뜁니다.
    match MdEncoder::encode_lossless(&validated) {
        Ok(lossless_md) => {
            let lossless_path = output_root.join(format!("{name}.lossless.md"));
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
