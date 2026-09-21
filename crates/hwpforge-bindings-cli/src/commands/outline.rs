//! Document navigation map (E5): headings, tables, fields, bookmarks.

use std::path::PathBuf;

use hwpforge::ops::{self, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the outline command.
pub fn run(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    // Decoder warnings (`OutlineOutput::warnings`) were not surfaced pre-W5
    // (the pre-migration CLI never captured them; W3 kept that byte-for-byte).
    // W5 follow-up: additive — a new, omit-if-empty `warnings` key in
    // `--json`, and one `[outline]`-prefixed stderr line each in text mode.
    let out = match ops::outline(&bytes) {
        Ok(out) => out,
        Err(e) => {
            let err = compat::cli_error(Command::Outline, e);
            let exit = compat::exit_code(Command::Outline, &err);
            err.exit(json_mode, exit);
        }
    };
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    let outline = out.outline;

    if json_mode {
        let mut result = serde_json::json!({ "status": "ok", "outline": outline });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    for w in &warnings {
        eprintln!("[outline] {}: {}", w.code, w.message);
    }

    match &outline.title {
        Some(title) => println!("{title} — {}", file.display()),
        None => println!("{}", file.display()),
    }
    for s in &outline.sections {
        println!(
            "  section {}: {} paragraph(s), {} table(s), {} image(s), {} chart(s)",
            s.section, s.paragraphs, s.tables, s.images, s.charts
        );
    }

    if !outline.headings.is_empty() {
        println!("Headings:");
        for h in &outline.headings {
            println!(
                "  {}{} {}  [s{} p{}]",
                "  ".repeat((h.level.saturating_sub(1)) as usize),
                "#".repeat(h.level as usize),
                h.text,
                h.at.section,
                h.at.para,
            );
        }
    }

    if !outline.tables.is_empty() {
        println!("Tables:");
        for t in &outline.tables {
            let dims = match (t.rows, t.cols) {
                (Some(r), Some(c)) => format!("{r}x{c}"),
                _ => "grid unaddressable".to_string(),
            };
            let caption = t.caption.as_deref().map(|c| format!(" \"{c}\"")).unwrap_or_default();
            println!("  [{}] {dims}{caption}  [s{} p{}]", t.ordinal, t.at.section, t.at.para);
        }
    }

    if !outline.fields.is_empty() {
        println!("Fields:");
        for f in &outline.fields {
            let name = f.name.as_deref().unwrap_or("(이름 없음)");
            let fillable = if f.fillable { "fillable" } else { "NOT fillable" };
            println!("  {name} ({fillable})  [s{}]", f.section);
        }
    }

    if !outline.bookmarks.is_empty() {
        println!("Bookmarks:");
        for b in &outline.bookmarks {
            println!("  {}  [s{} p{}]", b.name, b.at.section, b.at.para);
        }
    }
}
