//! HWPX ↔ JSON round-trip 예제.
//!
//! 1) examples/01_text.hwpx → JSON 변환 → examples/hwpx2json/01_text.json
//! 2) JSON → HWPX 복원 → examples/json2hwpx/01_text.hwpx
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

fn main() {
    let input_path = "examples/01_text.hwpx";

    // ── 1. HWPX → JSON (hwpx2json) ─────────────────────────────
    println!("=== HWPX → JSON ===");

    let hwpx_bytes = fs::read(input_path).expect("read input hwpx");
    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("decode hwpx");

    let exported =
        ExportedDocument { document: decoded.document, styles: Some(decoded.style_store) };

    let json_string = serde_json::to_string_pretty(&exported).expect("serialize to json");

    fs::create_dir_all("examples/hwpx2json").expect("create hwpx2json dir");
    fs::write("examples/hwpx2json/01_text.json", &json_string).expect("write json");

    // 원본 HWPX도 hwpx2json 폴더에 복사 (입력 파일 명시)
    fs::copy(input_path, "examples/hwpx2json/01_text.hwpx").expect("copy input hwpx");

    println!(
        "  {} → examples/hwpx2json/01_text.json ({} bytes, {} lines)",
        input_path,
        json_string.len(),
        json_string.lines().count(),
    );

    // ── 2. JSON → HWPX (json2hwpx) ─────────────────────────────
    println!("\n=== JSON → HWPX ===");

    let json_input = fs::read_to_string("examples/hwpx2json/01_text.json").expect("read json");
    let imported: ExportedDocument = serde_json::from_str(&json_input).expect("deserialize json");

    let style_store =
        imported.styles.unwrap_or_else(|| HwpxStyleStore::with_default_fonts("함초롬돋움"));
    let validated = imported.document.validate().expect("validate document");

    // 이미지 바이너리는 JSON에 포함되지 않으므로 원본 HWPX에서 상속 (CLI --base 옵션과 동일)
    let base = HwpxDecoder::decode(&hwpx_bytes).expect("decode base for images");
    let image_store = base.image_store;
    let hwpx_output =
        HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode hwpx");

    fs::create_dir_all("examples/json2hwpx").expect("create json2hwpx dir");
    fs::write("examples/json2hwpx/01_text.hwpx", &hwpx_output).expect("write hwpx");

    // JSON 입력도 json2hwpx 폴더에 복사 (입력 파일 명시)
    fs::copy("examples/hwpx2json/01_text.json", "examples/json2hwpx/01_text.json")
        .expect("copy json input");

    println!(
        "  examples/json2hwpx/01_text.json → examples/json2hwpx/01_text.hwpx ({} bytes)",
        hwpx_output.len(),
    );

    // ── 3. 요약 ─────────────────────────────────────────────────
    println!("\n=== 결과 요약 ===");
    println!("  examples/hwpx2json/01_text.hwpx   (input)  {} bytes", hwpx_bytes.len());
    println!("  examples/hwpx2json/01_text.json   (output) {} bytes", json_string.len());
    println!("  examples/json2hwpx/01_text.json   (input)  {} bytes", json_input.len());
    println!("  examples/json2hwpx/01_text.hwpx   (output) {} bytes", hwpx_output.len());
}
