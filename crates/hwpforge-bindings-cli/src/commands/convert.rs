//! Convert Markdown to HWPX.

use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use hwpforge_core::image::ImageStore;
use hwpforge_smithy_hwpx::HwpxEncoder;
use hwpforge_smithy_md::MdDecoder;

use crate::error::{check_file_size, CliError, MAX_STDIN_SIZE};

#[derive(Serialize)]
struct ConvertResult {
    status: &'static str,
    output: String,
    sections: usize,
    paragraphs: usize,
    size_bytes: usize,
}

/// Run the convert command: MD → HWPX.
pub fn run(input: &str, output: &PathBuf, preset: &str, json_mode: bool) {
    if preset != "default" {
        CliError::new("UNKNOWN_PRESET", format!("Preset '{preset}' not found"))
            .with_hint("Available presets: default")
            .exit(json_mode, 1);
    }

    // Read input (file or stdin)
    let markdown = if input == "-" {
        let mut buf = String::new();
        // Use take() to limit reads BEFORE buffering, preventing OOM on infinite streams.
        if let Err(e) = std::io::stdin().take((MAX_STDIN_SIZE + 1) as u64).read_to_string(&mut buf)
        {
            CliError::new("STDIN_READ_FAILED", format!("Failed to read stdin: {e}"))
                .exit(json_mode, 1);
        }
        if buf.len() > MAX_STDIN_SIZE {
            CliError::new(
                "INPUT_TOO_LARGE",
                format!("Stdin input exceeds {} MB limit", MAX_STDIN_SIZE / 1024 / 1024),
            )
            .exit(json_mode, 1);
        }
        buf
    } else {
        check_file_size(std::path::Path::new(input), json_mode);
        match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                CliError::new("FILE_READ_FAILED", format!("Cannot read '{input}': {e}"))
                    .with_hint("Check that the file exists and is valid UTF-8")
                    .exit(json_mode, 1);
            }
        }
    };

    // Decode MD → Core
    let md_doc = match MdDecoder::decode_with_default(&markdown) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("MD_DECODE_FAILED", format!("Markdown decode error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // Build style store from registry
    let style_store = hwpforge_smithy_hwpx::HwpxStyleStore::from_registry(&md_doc.style_registry);

    // Validate
    let validated = match md_doc.document.validate() {
        Ok(v) => v,
        Err(e) => {
            CliError::new("VALIDATION_FAILED", format!("Document validation error: {e}"))
                .exit(json_mode, 2);
        }
    };

    let total_paragraphs: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();

    // Encode Core → HWPX
    let image_store = ImageStore::new();
    let bytes = match HwpxEncoder::encode(&validated, &style_store, &image_store) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("ENCODE_FAILED", format!("HWPX encode error: {e}")).exit(json_mode, 2);
        }
    };

    // Write output
    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    // Report
    let result = ConvertResult {
        status: "ok",
        output: output.display().to_string(),
        sections: validated.section_count(),
        paragraphs: total_paragraphs,
        size_bytes: bytes.len(),
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Generated {} ({} sections, {} paragraphs, {} bytes)",
            result.output, result.sections, result.paragraphs, result.size_bytes
        );
    }
}
