//! Roundtrip a document: decode → encode → save to disk.
//!
//! Usage: cargo run -p hwpforge-smithy-hwpx --example roundtrip_save -- input.hwpx output.hwpx

use std::fs;

fn main() {
    let mut args = std::env::args().skip(1);
    let input_path = args.next().expect("input path");
    let output_path = args.next().expect("output path");

    let input_bytes = fs::read(input_path).expect("read fixture");

    // Decode
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&input_bytes).expect("decode");

    // Validate (Draft → Validated typestate)
    let validated = decoded.document.validate().expect("validate");

    // Re-encode
    let images = hwpforge_core::image::ImageStore::new();
    let encoded =
        hwpforge_smithy_hwpx::HwpxEncoder::encode(&validated, &decoded.style_store, &images)
            .expect("encode");

    fs::write(&output_path, &encoded).expect("write");
    println!("Roundtripped: {output_path} ({} bytes)", encoded.len());
}
