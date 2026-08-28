//! Convert Markdown to HWPX.

use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxRegistryBridge};
use hwpforge_smithy_md::{load_referenced_images, MdDecoder};

use crate::error::{check_file_size, CliError, MAX_STDIN_SIZE};

#[derive(Serialize)]
struct ConvertResult {
    status: &'static str,
    output: String,
    sections: usize,
    paragraphs: usize,
    size_bytes: usize,
    /// 이미지 임베드에서 제외된 참조들 (W6 §12b — typed 경고의 표시 문자열).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
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
    let mut md_doc = match MdDecoder::decode_with_default(&markdown) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("MD_DECODE_FAILED", format!("Markdown decode error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // 이미지 참조를 BinData 로 적재 (W6 §12b — stdin 입력은 base_dir 없음:
    // 상대 경로는 typed 경고로 제외되고 data: URI 만 임베드된다).
    // bare 파일명(`convert a.md`)의 parent() 는 None 이 아니라 빈 경로
    // `Some("")` — canonicalize 불가라 base 를 통째로 잃는다 (독립 리뷰
    // B2). 빈 경로 = 현재 디렉터리로 정규화.
    let base_dir = if input == "-" {
        None
    } else {
        match std::path::Path::new(input).parent() {
            Some(p) if p.as_os_str().is_empty() => Some(std::path::Path::new(".")),
            other => other,
        }
    };
    let embedded = load_referenced_images(&mut md_doc.document, base_dir);
    let embed_warnings: Vec<String> =
        embedded.warnings.iter().map(std::string::ToString::to_string).collect();
    for w in &embed_warnings {
        eprintln!("[convert] {w}");
    }

    let bridge = match HwpxRegistryBridge::from_registry(&md_doc.style_registry) {
        Ok(bridge) => bridge,
        Err(e) => {
            CliError::new("STYLE_STORE_FAILED", format!("Style store error: {e}"))
                .exit(json_mode, 2);
        }
    };

    let rebound = match bridge.rebind_draft_document(md_doc.document) {
        Ok(document) => document,
        Err(e) => {
            CliError::new("STYLE_REBIND_FAILED", format!("Style rebind error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // Validate
    let validated = match rebound.validate() {
        Ok(v) => v,
        Err(e) => {
            CliError::new("VALIDATION_FAILED", format!("Document validation error: {e}"))
                .exit(json_mode, 2);
        }
    };

    let total_paragraphs: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();

    // Encode Core → HWPX — 진단 동반 경로: 인코드 경고(각주 번호 머리
    // 생략·titleMark 보류 등)를 무음 폐기하지 않고 warnings 채널로 합류.
    let outcome = match HwpxEncoder::encode_with_diagnostics(
        &validated,
        bridge.style_store(),
        &embedded.store,
        hwpforge_smithy_hwpx::EncodeOptions::default(),
    ) {
        Ok(o) => o,
        Err(e) => {
            CliError::new("ENCODE_FAILED", format!("HWPX encode error: {e}")).exit(json_mode, 2);
        }
    };
    let bytes = outcome.bytes;
    let mut all_warnings = embed_warnings;
    for w in &outcome.warnings {
        let line = encode_warning_line(w);
        eprintln!("[convert] {line}");
        all_warnings.push(line);
    }

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
        warnings: all_warnings,
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

/// [`hwpforge_smithy_hwpx::EncodeWarning`] 을 한 줄 진단 문자열로 만든다.
///
/// `#[non_exhaustive]` — 새 variant 는 Debug 표기로 폴백해 무음 유실을
/// 막는다 (경고 채널의 목적이 곧 무음 방지다).
fn encode_warning_line(w: &hwpforge_smithy_hwpx::EncodeWarning) -> String {
    use hwpforge_smithy_hwpx::EncodeWarning;
    match w {
        EncodeWarning::LayoutCacheDropped { path, reason } => {
            format!("LAYOUT_CACHE_DROPPED at {path}: {reason}")
        }
        EncodeWarning::NoteHeadSkipped { path, reason } => {
            format!("NOTE_HEAD_SKIPPED at {path}: {reason}")
        }
        EncodeWarning::NoteRestartIgnored { path, reason } => {
            format!("NOTE_RESTART_IGNORED at {path}: {reason}")
        }
        EncodeWarning::TitleMarkSkipped { path, reason } => {
            format!("TITLE_MARK_SKIPPED at {path}: {reason}")
        }
        other => format!("ENCODE_WARNING {other:?}"),
    }
}
