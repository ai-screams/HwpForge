//! HWPX ↔ JSON round-trip 예제.
//!
//! 각 HWPX 파일을 JSON으로 변환하고, 다시 HWPX로 복원합니다.
//!
//! Usage: cargo run -p hwpforge-smithy-hwpx --example hwpx_json_roundtrip

use std::fs;

use hwpforge_core::document::Document;
use hwpforge_core::Draft;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxStyleStore};
use serde::{Deserialize, Serialize};

/// CLI의 ExportedDocument와 동일한 구조.
/// smithy-hwpx 예제에서 bindings-cli에 의존하지 않기 위해 재정의.
#[derive(Serialize, Deserialize)]
struct ExportedDocument {
    document: Document<Draft>,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<HwpxStyleStore>,
}

fn roundtrip(name: &str) {
    let input_path = format!("examples/{name}.hwpx");

    // ── 1. HWPX → JSON ─────────────────────────────────────────
    let hwpx_bytes = fs::read(&input_path).unwrap_or_else(|e| panic!("read {input_path}: {e}"));
    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("decode hwpx");

    let exported =
        ExportedDocument { document: decoded.document, styles: Some(decoded.style_store) };
    let json_string = serde_json::to_string_pretty(&exported).expect("serialize to json");

    fs::create_dir_all("examples/hwpx2json").expect("create hwpx2json dir");
    fs::write(format!("examples/hwpx2json/{name}.json"), &json_string).expect("write json");
    fs::copy(&input_path, format!("examples/hwpx2json/{name}.hwpx")).expect("copy input hwpx");

    println!(
        "  {input_path} → hwpx2json/{name}.json ({} bytes, {} lines)",
        json_string.len(),
        json_string.lines().count()
    );

    // ── 2. JSON → HWPX ─────────────────────────────────────────
    let json_input =
        fs::read_to_string(format!("examples/hwpx2json/{name}.json")).expect("read json");
    let imported: ExportedDocument = serde_json::from_str(&json_input).expect("deserialize json");

    let style_store =
        imported.styles.unwrap_or_else(|| HwpxStyleStore::with_default_fonts("함초롬돋움"));
    let validated = imported.document.validate().expect("validate document");

    // 이미지 바이너리는 JSON에 포함되지 않으므로 원본 HWPX에서 상속 (CLI --base 옵션과 동일)
    let base = HwpxDecoder::decode(&hwpx_bytes).expect("decode base for images");
    let hwpx_output =
        HwpxEncoder::encode(&validated, &style_store, &base.image_store).expect("encode hwpx");

    fs::create_dir_all("examples/json2hwpx").expect("create json2hwpx dir");
    fs::write(format!("examples/json2hwpx/{name}.hwpx"), &hwpx_output).expect("write hwpx");
    fs::copy(format!("examples/hwpx2json/{name}.json"), format!("examples/json2hwpx/{name}.json"))
        .expect("copy json input");

    println!("  json2hwpx/{name}.json → json2hwpx/{name}.hwpx ({} bytes)", hwpx_output.len());
}

fn main() {
    let targets = ["01_text", "hwpx_complete_guide"];

    println!("=== HwpForge HWPX ↔ JSON Round-trip ===\n");

    for name in &targets {
        println!("[{name}]");
        roundtrip(name);
        println!();
    }

    println!("=== {} files processed ===", targets.len());
}
