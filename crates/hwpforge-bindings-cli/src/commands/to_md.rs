//! `to-md` subcommand: convert HWPX to Markdown.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::MdEncoder;

use crate::error::{check_file_size, CliError};

/// Run the to-md command.
///
/// Decodes the HWPX file, encodes it as Markdown with style information,
/// then writes the `.md` file and any embedded images to the output directory.
pub fn run(input: &PathBuf, output: &Option<PathBuf>, json_mode: bool) {
    check_file_size(input, json_mode);

    // 1. Decode HWPX
    let hwpx_doc = match HwpxDecoder::decode_file(input) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}")).exit(json_mode, 2);
        }
    };

    // 2. Validate document (Draft → Validated)
    let document = match hwpx_doc.document.validate() {
        Ok(d) => d,
        Err(e) => {
            CliError::new("VALIDATE_FAILED", format!("Document validation error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // 3. Create style lookup bridge
    let lookup = HwpxStyleLookup::new(&hwpx_doc.style_store, &hwpx_doc.image_store);

    // 4. Encode to Markdown
    let md_output = MdEncoder::encode_styled(&document, &lookup);

    // 5. Determine output paths
    //    If -o ends with .md, treat as file path; otherwise treat as directory.
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

    // 6. Create output directory if needed
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        CliError::new("DIR_CREATE_FAILED", format!("Cannot create '{}': {e}", out_dir.display()))
            .exit(json_mode, 1);
    }

    // 7. Write markdown
    if let Err(e) = std::fs::write(&md_path, &md_output.markdown) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", md_path.display()))
            .exit(json_mode, 1);
    }

    // 8. Write images
    let image_count = md_output.images.len();
    if image_count > 0 {
        let images_dir = out_dir.join("images");
        if let Err(e) = std::fs::create_dir_all(&images_dir) {
            CliError::new(
                "DIR_CREATE_FAILED",
                format!("Cannot create '{}': {e}", images_dir.display()),
            )
            .exit(json_mode, 1);
        }
        for (rel_path, data) in &md_output.images {
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

    // 9. Print result
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
