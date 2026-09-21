//! Convert Markdown to HWPX.

use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use hwpforge::ops::{convert_md, ConvertMdOptions, OpsError, OpsWarning};
use hwpforge_smithy_hwpx::{builtin_presets, EncodeWarning};

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_bounded_string, CliError, MAX_STDIN_SIZE};

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
///
/// # W3 migration note (preset behaviour change)
///
/// The old hand-rolled `if preset != "default"` guard is gone — every
/// [`hwpforge::ops::convert_md`] call now names a preset from
/// `hwpforge_smithy_hwpx::builtin_presets()` (`default`, `modern`, `classic`,
/// `latest`), all of which succeed, `default` included. Before this
/// migration only `"default"` was accepted and every other catalogued preset
/// name (`modern`/`classic`/`latest`) returned `UNKNOWN_PRESET` — that was a
/// CLI limitation, not a real gap in the preset table. `UNKNOWN_PRESET`
/// still fires, with its legacy hint, for a name no built-in preset has at
/// all (`compat::TABLE`'s `(Convert, PresetNotFound)` row).
///
/// `convert_md` also swaps `"default"`'s own font to `builtin_presets()`'s
/// declared value (함초롬돋움) for every preset, `"default"` included —
/// decision I17 in the W3 report: the Markdown decoder's own `default`
/// template used 한컴바탕, which no longer matches what `templates show
/// default`/`ops::convert_md` advertise, so this makes `convert`'s output
/// agree with the preset table it already publishes.
pub fn run(input: &str, output: &PathBuf, preset: &str, json_mode: bool) {
    // Legacy validated `--preset` before any input I/O (5ff81af `run`'s
    // `if preset != "default"` guard ran first thing) — a bad preset name
    // must fail the same way whether the input file is missing or stdin is
    // still open, not surface as `FILE_READ_FAILED`/a stdin block (W3
    // remediation finding 6). `convert_md`'s own `check_preset` validates
    // against this same `builtin_presets()` table (`ops/convert.rs`), so
    // this preflight and the one inside `convert_md` can never disagree.
    if !builtin_presets().iter().any(|p| p.name == preset) {
        let cli_err =
            compat::cli_error(Command::Convert, OpsError::PresetNotFound { name: preset.into() });
        let exit = compat::exit_code(Command::Convert, &cli_err);
        cli_err.exit(json_mode, exit);
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
        match read_bounded_string(input) {
            Ok(s) => s,
            Err(e) => {
                CliError::new("FILE_READ_FAILED", format!("Cannot read '{input}': {e}"))
                    .with_hint("Check that the file exists and is valid UTF-8")
                    .exit(json_mode, 1);
            }
        }
    };

    // 이미지 참조 해석 base_dir (W6 §12b — stdin 입력은 base_dir 없음:
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

    let opts = ConvertMdOptions::default().with_preset(preset);
    let output_result = convert_md(&markdown, base_dir, &opts).unwrap_or_else(|err| {
        let cli_err = compat::cli_error(Command::Convert, err);
        let exit = compat::exit_code(Command::Convert, &cli_err);
        cli_err.exit(json_mode, exit)
    });

    // 경고 순서는 ops 가 이미 보장: asset(embed) 경고 → encode 경고
    // (`hwpforge::ops::convert::convert_md`). 사람이 읽는 줄과 JSON 배열 둘
    // 다 이 순서를 그대로 쓴다.
    //
    // `--json` 의 `warnings` 배열과 stderr 줄은 레거시 CLI 에서
    // 기계 파싱 가능했다 — encode 경고는 `LAYOUT_CACHE_DROPPED at …`
    // 처럼 코드 접두 형태였다(`encode_warning_line`). `OpsWarning::info()`
    // 의 자연어 Display 로 바꾸면 그 접두가 사라지므로, 여기서는 변형별로
    // 레거시 줄을 재구성한다: `Encode` 는 `encode_warning_line`(레거시
    // 그대로), `Md` 는 레거시와 동일하게 `Display` 그대로(코드 접두 없음
    // — 임베드 제외 경고가 원래 코드 없이 나갔다). `OpsWarning` 은
    // `#[non_exhaustive]` 이고 `convert_md` 는 오늘 이 둘만 내지만, 새
    // variant(Decode/Structural/GridAddr/SectionWorkflow/Asset 등)가
    // 나오면 `"{code} {message}"` 로 폴백한다 — `convert_md` 가 실제로
    // 낸 적 없는 신규-전용 형태.
    let all_warnings: Vec<String> = output_result
        .warnings
        .iter()
        .map(|w| {
            let line = match w {
                OpsWarning::Encode(inner) => encode_warning_line(inner),
                OpsWarning::Md(inner) => inner.to_string(),
                other => {
                    let info = other.info();
                    format!("{} {}", info.code, info.message)
                }
            };
            eprintln!("[convert] {line}");
            line
        })
        .collect();

    // Write output
    if let Err(e) = std::fs::write(output, &output_result.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    // Report
    let result = ConvertResult {
        status: "ok",
        output: output.display().to_string(),
        sections: output_result.sections,
        paragraphs: output_result.paragraphs,
        size_bytes: output_result.bytes.len(),
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

/// [`EncodeWarning`] 을 레거시 CLI 의 한 줄 진단 문자열로 만든다 — W3
/// 마이그레이션 전 `commands/convert.rs`(pre-`ops`)의 `encode_warning_line`
/// 을 바이트 동일하게 복원한 것. `#[non_exhaustive]` — 새 variant 는
/// Debug 표기로 폴백해 무음 유실을 막는다 (경고 채널의 목적이 곧 무음
/// 방지다).
fn encode_warning_line(w: &EncodeWarning) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_smithy_hwpx::{ParagraphPath, PathSeg};

    /// Legacy fidelity lock (team-lead follow-up, W3): the pre-`ops` CLI's
    /// `[convert]` stderr lines and `--json` `warnings` array entries for an
    /// encode warning were machine-parsable code-prefixed strings
    /// (`LAYOUT_CACHE_DROPPED at …: …`), not `OpsWarning::info()`'s
    /// natural-language `Display` (`layout cache dropped at …: …`). This
    /// locks the byte-identical legacy shape for the `OpsWarning::Encode`
    /// arm in `run`.
    #[test]
    fn encode_warning_line_matches_the_legacy_format() {
        let path = ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)]);
        let warning = OpsWarning::Encode(EncodeWarning::LayoutCacheDropped {
            path: path.clone(),
            reason: "x".into(),
        });

        let line = match &warning {
            OpsWarning::Encode(inner) => encode_warning_line(inner),
            _ => unreachable!("constructed as Encode above"),
        };

        assert_eq!(line, format!("LAYOUT_CACHE_DROPPED at {path}: x"));
        // `ParagraphPath`'s own `Display` (`decoder::mod.rs`) renders
        // segments as `section[N].para[N]`, not a `sN/pN` shorthand.
        assert_eq!(line, "LAYOUT_CACHE_DROPPED at section[0].para[1]: x");
    }
}
