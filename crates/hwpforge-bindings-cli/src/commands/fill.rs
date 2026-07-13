//! Fill named click-here fields with values (delta edit, preserve-first).

use std::collections::BTreeMap;
use std::path::PathBuf;

use hwpforge_smithy_hwpx::{FillError, HwpxFiller};

use crate::error::{check_file_size, CliError};

/// Run the fill command.
pub fn run(file: &PathBuf, sets: &[String], output: &PathBuf, json_mode: bool) {
    let values = parse_sets(sets, json_mode);
    if values.is_empty() {
        CliError::new("NO_VALUES", "no --set name=value pairs given")
            .with_hint("예: hwpforge fill doc.hwpx --set 과제명=\"AI 문서 자동화\" -o out.hwpx")
            .exit(json_mode, 1);
    }

    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let outcome = match HwpxFiller::fill(&bytes, &values) {
        Ok(outcome) => outcome,
        Err(error) => exit_fill_error(error, json_mode),
    };

    if let Err(e) = std::fs::write(output, &outcome.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "output": output.display().to_string(),
            "filled": outcome.filled,
            "size_bytes": outcome.bytes.len(),
        });
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Filled {} field(s) -> {}", outcome.filled.len(), output.display());
        for f in &outcome.filled {
            println!("  [{}] {}: {:?} -> filled", f.section, f.name, f.previous);
        }
    }
}

/// `name=value` 인자 목록을 맵으로 파싱한다. `=` 누락·중복 이름은 거부.
fn parse_sets(sets: &[String], json_mode: bool) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for set in sets {
        let Some((name, value)) = set.split_once('=') else {
            CliError::new("INVALID_SET", format!("'--set {set}' is not name=value"))
                .with_hint("--set 이름=값 형식으로 지정하세요 (값에 공백이 있으면 따옴표)")
                .exit(json_mode, 1);
        };
        if values.insert(name.to_string(), value.to_string()).is_some() {
            CliError::new("DUPLICATE_SET", format!("'--set {name}=…' given more than once"))
                .exit(json_mode, 1);
        }
    }
    values
}

fn exit_fill_error(error: FillError, json_mode: bool) -> ! {
    match error {
        FillError::EmptyValue { name } => {
            CliError::new("EMPTY_FIELD_VALUE", format!("field '{name}': empty value"))
                .with_hint("빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요")
                .exit(json_mode, 1)
        }
        FillError::UnknownField { name, available } => CliError::new(
            "FIELD_NOT_FOUND",
            format!("field '{name}' not found in document"),
        )
        .with_hint(format!(
            "사용 가능한 필드: [{}] — `hwpforge fields <file>` 로 확인",
            available.join(", ")
        ))
        .exit(json_mode, 1),
        FillError::DuplicateFieldName { name, count } => CliError::new(
            "FIELD_NAME_AMBIGUOUS",
            format!("field '{name}' appears {count} times"),
        )
        .with_hint("같은 이름의 누름틀이 여러 개라 대상이 모호합니다 — 문서에서 이름을 유일하게 하세요")
        .exit(json_mode, 1),
        FillError::UnfillableField { name, section } => CliError::new(
            "FIELD_NOT_FILLABLE",
            format!("field '{name}' in section {section} has no patchable body"),
        )
        .with_hint(
            "병합-run 모호 필드 또는 빈 본문 — 한컴에서 재저장하거나 from-json --base 로 재생성하세요",
        )
        .exit(json_mode, 1),
        FillError::Workflow(e) => {
            CliError::new("FILL_FAILED", format!("fill workflow error: {e}")).exit(json_mode, 2)
        }
    }
}
