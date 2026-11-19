//! HWPX ↔ JSON round-trip 예제.
//!
//! 레포에 있는 쇼케이스 HWPX 파일을 JSON으로 변환하고, 다시 HWPX로 복원합니다.
//! 입력은 실행한 폴더와 상관없이 레포에서 읽고, 출력은 실행한 폴더의
//! `temp/hwpx_json_roundtrip/` 아래 `hwpx2json/`·`json2hwpx/`에 씁니다.
//!
//! Usage: cargo run -p hwpforge-smithy-hwpx --example hwpx_json_roundtrip

use std::fs;
use std::path::Path;

use hwpforge_smithy_hwpx::{ExportedDocument, HwpxDecoder, HwpxEncoder, HwpxStyleStore};

/// 변환할 문서: 이름과 레포 루트 기준 경로.
const TARGETS: [(&str, &str); 2] = [
    ("01_text", "examples/showcase/features/feature_isolation/01_text.hwpx"),
    (
        "hwpx_complete_guide",
        "examples/showcase/guides/hwpx_complete_guide/hwpx_complete_guide.hwpx",
    ),
];

const OUT_DIR: &str = "temp/hwpx_json_roundtrip";

fn roundtrip(name: &str, input: &str) {
    let input_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(input);
    let hwpx2json = Path::new(OUT_DIR).join("hwpx2json");
    let json2hwpx = Path::new(OUT_DIR).join("json2hwpx");

    // ── 1. HWPX → JSON ─────────────────────────────────────────
    let hwpx_bytes = fs::read(&input_path).unwrap_or_else(|e| panic!("read {input}: {e}"));
    let decoded = HwpxDecoder::decode(&hwpx_bytes).expect("decode hwpx");

    let exported = ExportedDocument {
        format_version: hwpforge_smithy_hwpx::exchange::FORMAT_VERSION,
        document: decoded.document,
        styles: Some(decoded.style_store),
    };
    let json_string = serde_json::to_string_pretty(&exported).expect("serialize to json");

    fs::create_dir_all(&hwpx2json).expect("create hwpx2json dir");
    let json_path = hwpx2json.join(format!("{name}.json"));
    fs::write(&json_path, &json_string).expect("write json");
    fs::copy(&input_path, hwpx2json.join(format!("{name}.hwpx"))).expect("copy input hwpx");

    println!(
        "  {input} → {} ({} bytes, {} lines)",
        json_path.display(),
        json_string.len(),
        json_string.lines().count()
    );

    // ── 2. JSON → HWPX ─────────────────────────────────────────
    let json_input = fs::read_to_string(&json_path).expect("read json");
    let imported: ExportedDocument = serde_json::from_str(&json_input).expect("deserialize json");

    let style_store =
        imported.styles.unwrap_or_else(|| HwpxStyleStore::with_default_fonts("함초롬돋움"));
    let validated = imported.document.validate().expect("validate document");

    // 이미지 바이너리는 JSON에 포함되지 않으므로 원본 HWPX에서 상속 (CLI --base 옵션과 동일)
    let base = HwpxDecoder::decode(&hwpx_bytes).expect("decode base for images");
    let hwpx_output =
        HwpxEncoder::encode(&validated, &style_store, &base.image_store).expect("encode hwpx");

    fs::create_dir_all(&json2hwpx).expect("create json2hwpx dir");
    let hwpx_path = json2hwpx.join(format!("{name}.hwpx"));
    fs::write(&hwpx_path, &hwpx_output).expect("write hwpx");
    fs::copy(&json_path, json2hwpx.join(format!("{name}.json"))).expect("copy json input");

    println!("  {} → {} ({} bytes)", json_path.display(), hwpx_path.display(), hwpx_output.len());
}

fn main() {
    println!("=== HwpForge HWPX ↔ JSON Round-trip ===\n");

    for (name, input) in TARGETS {
        println!("[{name}]");
        roundtrip(name, input);
        println!();
    }

    println!("=== {} files processed ===", TARGETS.len());
}
