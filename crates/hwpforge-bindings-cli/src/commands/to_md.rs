//! `to-md` subcommand: convert HWPX to Markdown.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::{hancom_eqn_to_latex, MdEncoder};

use crate::error::{check_file_size, CliError};
use crate::MdMode;

static SIDECAR_TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Run the to-md command.
pub fn run(input: &PathBuf, output: &Option<PathBuf>, mode: &MdMode, json_mode: bool) {
    check_file_size(input, json_mode);

    // 1. Decode HWPX
    let (hwpx_doc, mut visual_equations) = match HwpxDecoder::decode_file_with_report(input) {
        Ok(result) => result,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}")).exit(json_mode, 2);
        }
    };

    for equation in &mut visual_equations.equations {
        equation.latex = Some(hancom_eqn_to_latex(&equation.script));
    }

    // 2. Validate document (Draft → Validated)
    let document = match hwpx_doc.document.validate() {
        Ok(d) => d,
        Err(e) => {
            CliError::new("VALIDATE_FAILED", format!("Document validation error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // 3. Encode to Markdown based on mode
    let (markdown, images) = match mode {
        MdMode::Styled => {
            let lookup = HwpxStyleLookup::new(&hwpx_doc.style_store, &hwpx_doc.image_store);
            let md_output = MdEncoder::encode_styled(&document, &lookup);
            (md_output.markdown, md_output.images)
        }
        MdMode::Lossy => match MdEncoder::encode_lossy_with_report(&document) {
            Ok((md, warnings)) => {
                // Warning-first: lossy 렌더가 표현 못 하는 것(병합셀 평탄화)을
                // 무음으로 버리지 않고 노출한다. styled 모드는 HTML 로 보존.
                for warning in warnings {
                    if json_mode {
                        let warn = serde_json::json!({
                            "status": "warning",
                            "code": "TABLE_MERGE_FLATTENED",
                            "message": warning.to_string(),
                        });
                        eprintln!("{}", serde_json::to_string(&warn).unwrap());
                    } else {
                        eprintln!("Warning: {warning}");
                    }
                }
                (md, HashMap::new())
            }
            Err(e) => {
                CliError::new("ENCODE_FAILED", format!("Markdown encode error: {e}"))
                    .exit(json_mode, 2);
            }
        },
        MdMode::Lossless => match MdEncoder::encode_lossless(&document) {
            Ok(md) => (md, HashMap::new()),
            Err(e) => {
                CliError::new("ENCODE_FAILED", format!("Markdown encode error: {e}"))
                    .exit(json_mode, 2);
            }
        },
    };

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

    // 8. Write the styled-mode structural sidecar before reporting success.
    let visual_equations_result = if matches!(mode, MdMode::Styled) {
        let sidecar_path = md_path.with_extension("visual-equations.json");
        let sidecar_json = match serde_json::to_vec_pretty(&visual_equations) {
            Ok(json) => json,
            Err(e) => {
                CliError::new(
                    "ENCODE_FAILED",
                    format!("Visual-equations sidecar encode error: {e}"),
                )
                .exit(json_mode, 2);
            }
        };
        if let Err(e) = write_atomic(&sidecar_path, &sidecar_json) {
            CliError::new(
                "FILE_WRITE_FAILED",
                format!("Cannot write '{}': {e}", sidecar_path.display()),
            )
            .exit(json_mode, 1);
        }
        Some(serde_json::json!({
            "output": sidecar_path.display().to_string(),
            "count": visual_equations.equations.len(),
        }))
    } else {
        None
    };

    // 9. Print result
    let mut result = serde_json::json!({
        "status": "ok",
        "output": md_path.display().to_string(),
        "images": image_count,
    });
    if let Some(visual_equations_result) = visual_equations_result {
        result
            .as_object_mut()
            .expect("JSON object literal must remain an object")
            .insert("visual_equations".to_string(), visual_equations_result);
    }

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

fn write_atomic(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("sidecar.json");
    let attempt = SIDECAR_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path =
        path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), attempt));

    std::fs::write(&temp_path, contents)?;
    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(())
}
