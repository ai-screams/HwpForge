//! Targeted text reads (E5): paragraph range, table grid, or field by name.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{EmbeddedContent, HwpxReader, ParaKindView, ReadError};

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
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    if let Some(section) = section {
        let range = paras.map(|s| parse_paras(s, json_mode));
        let view = match HwpxReader::read_paragraphs(&bytes, section, range) {
            Ok(v) => v,
            Err(e) => exit_read_error(e, json_mode),
        };
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

    if let Some(ordinal) = table {
        let view = match HwpxReader::read_table(&bytes, ordinal) {
            Ok(v) => v,
            Err(e) => exit_read_error(e, json_mode),
        };
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

    let name = field.expect("target validation guarantees field");
    let fields = match HwpxReader::read_field(&bytes, name) {
        Ok(v) => v,
        Err(e) => exit_read_error(e, json_mode),
    };
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

/// Parses `"A..B"` (inclusive) or a single `"N"` into an inclusive pair.
fn parse_paras(spec: &str, json_mode: bool) -> (usize, usize) {
    let parsed = match spec.split_once("..") {
        Some((a, b)) => a
            .trim()
            .parse::<usize>()
            .and_then(|from| b.trim().parse::<usize>().map(|to| (from, to)))
            .ok(),
        None => spec.trim().parse::<usize>().map(|n| (n, n)).ok(),
    };
    match parsed {
        Some(range) => range,
        None => {
            CliError::new(
                "READ_PARAS_INVALID",
                format!(
                    "Cannot parse --paras {spec:?}: use \"A..B\" (inclusive) or a single \"N\""
                ),
            )
            .exit(json_mode, 1);
        }
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

fn exit_read_error(err: ReadError, json_mode: bool) -> ! {
    let (code, exit) = match &err {
        ReadError::Codec(_) => ("DECODE_FAILED", 2),
        ReadError::SectionOutOfRange { .. } => ("READ_SECTION_OUT_OF_RANGE", 1),
        ReadError::ParaRangeInvalid { .. } => ("READ_PARA_RANGE_INVALID", 1),
        ReadError::TableOutOfRange { .. } => ("READ_TABLE_OUT_OF_RANGE", 1),
        ReadError::TableUnaddressable { .. } => ("TABLE_GRID_INVALID", 1),
        ReadError::FieldNotFound { .. } => ("READ_FIELD_NOT_FOUND", 1),
    };
    CliError::new(code, err.to_string()).exit(json_mode, exit);
}
