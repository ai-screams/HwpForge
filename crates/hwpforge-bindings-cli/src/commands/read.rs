//! Targeted text reads (E5): paragraph range, table grid, or field by name.

use std::path::PathBuf;

use hwpforge::ops::{self, ReadOptions};
use hwpforge_smithy_hwpx::{EmbeddedContent, ParaKindView};

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the read command.
#[allow(clippy::too_many_arguments)]
pub fn run(
    file: &PathBuf,
    section: Option<usize>,
    paras: Option<&str>,
    table: Option<usize>,
    field: Option<&str>,
    json_mode: bool,
) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let mut opts = ReadOptions::default();
    if let Some(section) = section {
        opts = opts.with_section(section);
    }
    if let Some(paras) = paras {
        opts = opts.with_paras(paras);
    }
    if let Some(table) = table {
        opts = opts.with_table(table);
    }
    if let Some(field) = field {
        opts = opts.with_field(field);
    }

    // Target-count / paras-without-section / paras-parse rejections all
    // come from `ops::read` itself now (its rules and messages are the
    // CLI's own, reproduced verbatim — `hwpforge/src/ops/read.rs` module
    // docs), rather than being pre-checked locally as before. Decoder
    // warnings (`ReadOutput::warnings`) are not surfaced (W3 report).
    let out = match ops::read(&bytes, &opts) {
        Ok(o) => o,
        Err(e) => {
            let err = compat::cli_error(Command::Read, e);
            let exit = compat::exit_code(Command::Read, &err);
            err.exit(json_mode, exit);
        }
    };

    if let Some(view) = out.paragraphs {
        if json_mode {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({ "status": "ok", "paragraphs": view }))
                    .unwrap()
            );
            return;
        }
        println!("section {}, paragraphs {}..={}:", view.section, view.from, view.to);
        for p in &view.paragraphs {
            let prefix = match p.kind {
                ParaKindView::Heading { level } => format!("{} ", "#".repeat(level as usize)),
                ParaKindView::List { numbered: true, .. } => "1. ".to_string(),
                ParaKindView::List { checked: Some(true), .. } => "- [x] ".to_string(),
                ParaKindView::List { checked: Some(false), .. } => "- [ ] ".to_string(),
                ParaKindView::List { .. } => "- ".to_string(),
                ParaKindView::Body => String::new(),
            };
            let contains = render_contains(&p.contains);
            println!("  [p{}] {prefix}{}{contains}", p.at.para, p.text.replace('\n', " ⏎ "));
        }
        return;
    }

    if let Some(view) = out.table {
        if json_mode {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({ "status": "ok", "table": view }))
                    .unwrap()
            );
            return;
        }
        println!(
            "table {} ({}x{}) at [s{} p{}]:",
            view.ordinal, view.rows, view.cols, view.at.section, view.at.para
        );
        for c in &view.cells {
            let span = if c.row_span > 1 || c.col_span > 1 {
                format!(" +{}x{}", c.row_span, c.col_span)
            } else {
                String::new()
            };
            let contains = render_contains(&c.contains);
            println!("  [{},{}{span}] {}{contains}", c.row, c.col, c.text.replace('\n', " / "));
        }
        return;
    }

    let fields = out.fields.expect("ops::read guarantees exactly one payload is set");
    let name = field.expect("target validation guarantees field");
    if json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({ "status": "ok", "fields": fields }))
                .unwrap()
        );
        return;
    }
    for f in &fields {
        let fillable = if f.fillable { "fillable" } else { "NOT fillable" };
        println!(
            "  {} = {:?} ({fillable}; hint: {})  [s{}]",
            name,
            f.current,
            f.hint.as_deref().unwrap_or("-"),
            f.section,
        );
    }
}

fn render_contains(contains: &[EmbeddedContent]) -> String {
    if contains.is_empty() {
        return String::new();
    }
    let markers: Vec<String> = contains
        .iter()
        .map(|c| match c {
            EmbeddedContent::Table { ordinal: Some(o) } => format!("table:{o}"),
            EmbeddedContent::Table { ordinal: None } => "table".to_string(),
            EmbeddedContent::Image => "image".to_string(),
            EmbeddedContent::Control { control } => format!("control:{control}"),
            EmbeddedContent::Other => "other".to_string(),
        })
        .collect();
    format!("  ({})", markers.join(", "))
}
