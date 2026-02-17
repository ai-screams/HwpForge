//! Roundtrip a golden fixture: decode → encode → save to disk.
//!
//! Usage: cargo run -p hwpforge-smithy-hwpx --example roundtrip_save

use std::fs;

fn main() {
    let input_path = "crates/hwpforge-smithy-hwpx/tests/fixtures/SimpleEdit.hwpx";
    let output_path = "roundtrip_SimpleEdit.hwpx";

    let input_bytes = fs::read(input_path).expect("read fixture");

    // Decode
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&input_bytes).expect("decode");

    // Validate (Draft → Validated typestate)
    let validated = decoded.document.validate().expect("validate");

    // Re-encode
    let encoded = hwpforge_smithy_hwpx::HwpxEncoder::encode(&validated, &decoded.style_store)
        .expect("encode");

    fs::write(output_path, &encoded).expect("write");
    println!("Roundtripped: {output_path} ({} bytes)", encoded.len());
}
