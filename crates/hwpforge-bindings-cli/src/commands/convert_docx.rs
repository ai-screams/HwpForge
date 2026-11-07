//! Convert DOCX bytes through the shared import operation.
use std::path::Path;

use hwpforge::ops::convert_docx;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};

use crate::compat::{self, Command};
use crate::error::{read_bounded, CliCode, CliError};

/// Run DOCX import without interpreting the filename extension.
/// JSON success output is ImportMeta itself, with no additional envelope.
pub fn run(input: &Path, output: &Path, json_mode: bool) {
    let bytes = read_bounded(input)
        .unwrap_or_else(|err| compat::exit_ops_error(Command::ConvertDocx, err, json_mode));
    match crate::input::same_existing_file(input, output) {
        Ok(true) => CliError::new(
            CliCode::Ops(OpsCode::InvalidOutputPath),
            format!("Output '{}' would overwrite the DOCX input", output.display()),
        )
        .exit(json_mode, Command::ConvertDocx),
        Ok(false) => {}
        Err(err) => CliError::new(
            CliCode::Ops(OpsCode::FileWriteFailed),
            format!("Cannot verify output '{}': {err}", output.display()),
        )
        .exit(json_mode, Command::ConvertDocx),
    }
    let imported = convert_docx(&bytes)
        .unwrap_or_else(|err| compat::exit_ops_error(Command::ConvertDocx, err, json_mode));
    std::fs::write(output, &imported.bytes).unwrap_or_else(|err| {
        CliError::new(
            CliCode::Ops(OpsCode::FileWriteFailed),
            format!("Cannot write '{}': {err}", output.display()),
        )
        .exit(json_mode, Command::ConvertDocx)
    });
    let meta = imported.meta();
    if json_mode {
        println!("{}", serde_json::to_string(&meta).expect("serializable import metadata"));
    } else {
        for warning in &meta.warnings {
            eprintln!("[convert-docx] {}", warning_line(warning));
        }
        println!(
            "Generated {} ({} sections, {} paragraphs)",
            output.display(),
            meta.sections,
            meta.paragraphs
        );
    }
}

fn warning_line(warning: &WarningInfo) -> String {
    let (count, part, location) = warning.context.as_ref().map_or((1, None, None), |context| {
        (context.count, context.part.as_deref(), context.location.as_deref())
    });
    format!(
        "{}: total {count}; first {}:{}; subsequent locations omitted: {}; {}",
        warning.code,
        part.unwrap_or("unknown"),
        location.unwrap_or("unknown"),
        if count > 1 { "yes" } else { "no" },
        warning.message
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::diagnostics::{WarningContext, WarningStage};

    // 이것을 실패시키는 것: 없는 위치를 추측하거나 context 없는 기존 경고를 집계 경고로 출력.
    #[test]
    fn warning_text_keeps_unknown_positions_explicit() {
        let mut warning = WarningInfo::new("TEST", "message");
        assert_eq!(
            warning_line(&warning),
            "TEST: total 1; first unknown:unknown; subsequent locations omitted: no; message"
        );
        let mut context =
            WarningContext::new("hwpx", WarningStage::Project, "font", "use_first_font");
        context.location = Some("char_shapes[7].latin".into());
        context.count = 20_000;
        warning.context = Some(context);
        assert_eq!(warning_line(&warning), "TEST: total 20000; first unknown:char_shapes[7].latin; subsequent locations omitted: yes; message");
        let context = warning.context.as_mut().unwrap();
        context.part = Some("part.xml".into());
        context.location = None;
        assert_eq!(
            warning_line(&warning),
            "TEST: total 20000; first part.xml:unknown; subsequent locations omitted: yes; message"
        );
    }
}
