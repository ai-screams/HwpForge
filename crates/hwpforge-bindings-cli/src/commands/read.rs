//! Targeted text reads (E5): paragraph range, table grid, or field by name.

use std::path::PathBuf;

use hwpforge::ops::{self, OpsWarning, ReadOptions};
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{EmbeddedContent, ParaKindView};

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_input, CliError};

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
    // Legacy guards (5ff81af `run`): checked before the file is even read,
    // so a bad target/`--paras` combination fails fast without touching the
    // filesystem. `ops::read` keeps its own copies of both rules for the
    // in-document path (module docs, `hwpforge/src/ops/read.rs`) — these
    // reproduce the identical code/message/exit for the pre-read path.
    let targets = usize::from(section.is_some())
        + usize::from(table.is_some())
        + usize::from(field.is_some());
    if targets != 1 {
        CliError::new("READ_TARGET_REQUIRED", "Pass exactly one of --section, --table, --field")
            .exit(json_mode, 1);
    }
    if paras.is_some() && section.is_none() {
        CliError::new("READ_PARAS_WITHOUT_SECTION", "--paras requires --section")
            .exit(json_mode, 1);
    }

    check_file_size(file, json_mode);
    let bytes = read_input(file, json_mode);

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

    // Target-count / paras-without-section are pre-checked above, matching
    // the legacy pre-read guards byte-for-byte; a paras-parse rejection
    // (`READ_PARAS_INVALID`/`READ_PARA_RANGE_INVALID`) still comes from
    // `ops::read` itself (its rules and messages are the CLI's own,
    // reproduced verbatim — `hwpforge/src/ops/read.rs` module docs).
    // Decoder warnings (`ReadOutput::warnings`) were not surfaced pre-W5. W5
    // follow-up: additive — a new, omit-if-empty `warnings` key in `--json`,
    // and one `[read]`-prefixed stderr line each in text mode, for whichever
    // of the three payloads below is the requested target.
    let out = match ops::read(&bytes, &opts) {
        Ok(o) => o,
        Err(e) => compat::exit_ops_error(Command::Read, e, json_mode),
    };
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();

    if let Some(view) = out.paragraphs {
        if json_mode {
            let mut result = serde_json::json!({ "status": "ok", "paragraphs": view });
            if !warnings.is_empty() {
                result["warnings"] = serde_json::to_value(&warnings).unwrap();
            }
            println!("{}", serde_json::to_string(&result).unwrap());
            return;
        }
        for w in &warnings {
            eprintln!("[read] {}: {}", w.code, w.message);
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
            let mut result = serde_json::json!({ "status": "ok", "table": view });
            if !warnings.is_empty() {
                result["warnings"] = serde_json::to_value(&warnings).unwrap();
            }
            println!("{}", serde_json::to_string(&result).unwrap());
            return;
        }
        for w in &warnings {
            eprintln!("[read] {}: {}", w.code, w.message);
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
        let mut result = serde_json::json!({ "status": "ok", "fields": fields });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    for w in &warnings {
        eprintln!("[read] {}: {}", w.code, w.message);
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
