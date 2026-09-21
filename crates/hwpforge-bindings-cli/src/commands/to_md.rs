//! `to-md` subcommand: convert HWPX to Markdown.

use std::path::PathBuf;

use hwpforge::ops::{self, MdExportOptions, MdMode as OpsMdMode, OpsWarning};

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_bounded, CliError};
use crate::MdMode;

/// Run the to-md command.
pub fn run(input: &PathBuf, output: &Option<PathBuf>, mode: &MdMode, json_mode: bool) {
    check_file_size(input, json_mode);

    // Pre-migration `to-md` used `HwpxDecoder::decode_file`, which bundles
    // file I/O into the decode stage (`HwpxError::Io`) — so a missing or
    // unreadable input reports `DECODE_FAILED`/exit 2 here, unlike every
    // other command's `FILE_READ_FAILED`/exit 1. `ops::to_md` takes bytes,
    // not a path, so the read has to happen here; reproduced with the
    // legacy code/exit/message shape rather than introducing a
    // `FILE_READ_FAILED` this command's frozen contract never had.
    let bytes = match read_bounded(input) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}")).exit(json_mode, 2);
        }
    };

    let ops_mode = match mode {
        MdMode::Styled => OpsMdMode::Styled,
        MdMode::Lossy => OpsMdMode::Lossy,
        MdMode::Lossless => OpsMdMode::Lossless,
    };

    // `ops::to_md` reproduces decode → validate → encode in one call. Its
    // `DECODE_FAILED`/`VALIDATION_FAILED`/`ENCODE_FAILED` route through the
    // shared compat table; only the legacy `VALIDATE_FAILED` spelling (this
    // command's own code, not `ops`'s `VALIDATION_FAILED`) differs, and
    // `compat::cli_error`'s `TABLE` already carries that remap. Decode
    // warnings (`MdExportOutput::warnings`'s `Decode` entries, for example
    // `LAYOUT_CACHE_DROPPED`) were not surfaced pre-W5 — only `Md` (lossy
    // `TABLE_MERGE_FLATTENED`) warnings were printed pre-migration. W5
    // follow-up: `Decode` now prints through the same shape, additively.
    let out = match ops::to_md(&bytes, &MdExportOptions::default().with_mode(ops_mode)) {
        Ok(o) => o,
        Err(e) => compat::exit_ops_error(Command::ToMd, e, json_mode),
    };
    for warning in &out.warnings {
        if matches!(warning, OpsWarning::Md(_) | OpsWarning::Decode(_)) {
            let info = warning.info();
            if json_mode {
                let warn = serde_json::json!({
                    "status": "warning",
                    "code": info.code,
                    "message": info.message,
                });
                eprintln!("{}", serde_json::to_string(&warn).unwrap());
            } else {
                eprintln!("Warning: {}", info.message);
            }
        }
    }
    let markdown = out.markdown;
    let images: std::collections::HashMap<String, Vec<u8>> = out.images.into_iter().collect();

    // 4. Determine output paths
    let (out_dir, md_path) = match output {
        Some(p) if p.extension().and_then(|e| e.to_str()) == Some("md") => {
            let dir = p.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
            (dir, p.clone())
        }
        Some(dir) => {
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            (dir.clone(), dir.join(format!("{stem}.md")))
        }
        None => {
            let dir = input.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            (dir.clone(), dir.join(format!("{stem}.md")))
        }
    };

    // 5. Create output directory if needed
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        CliError::new("DIR_CREATE_FAILED", format!("Cannot create '{}': {e}", out_dir.display()))
            .exit(json_mode, 1);
    }

    // 6. Write markdown
    if let Err(e) = std::fs::write(&md_path, &markdown) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", md_path.display()))
            .exit(json_mode, 1);
    }

    // 7. Write images (styled mode only)
    let image_count = images.len();
    if image_count > 0 {
        let images_dir = out_dir.join("images");
        if let Err(e) = std::fs::create_dir_all(&images_dir) {
            CliError::new(
                "DIR_CREATE_FAILED",
                format!("Cannot create '{}': {e}", images_dir.display()),
            )
            .exit(json_mode, 1);
        }
        for (rel_path, data) in &images {
            let img_filename = std::path::Path::new(rel_path.as_str())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image");
            let img_path = images_dir.join(img_filename);
            if let Err(e) = std::fs::write(&img_path, data) {
                CliError::new(
                    "FILE_WRITE_FAILED",
                    format!("Cannot write '{}': {e}", img_path.display()),
                )
                .exit(json_mode, 1);
            }
        }
    }

    // 8. Print result
    let result = serde_json::json!({
        "status": "ok",
        "output": md_path.display().to_string(),
        "images": image_count,
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Converted {} → {} ({} image{})",
            input.display(),
            md_path.display(),
            image_count,
            if image_count == 1 { "" } else { "s" }
        );
    }
}
