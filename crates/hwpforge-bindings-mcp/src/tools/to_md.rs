//! `hwpforge_to_md` — HWPX → Markdown conversion tool.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::MdEncoder;

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful HWPX → Markdown conversion.
#[derive(Debug, Serialize)]
pub struct ToMdData {
    /// Path to the generated Markdown file.
    pub markdown_path: String,
    /// Paths to extracted image files (relative names → full output paths).
    pub image_paths: Vec<String>,
    /// Size of the generated Markdown file in bytes.
    pub size_bytes: u64,
    /// Number of images extracted.
    pub image_count: usize,
}

/// Execute HWPX → Markdown conversion.
///
/// Decodes an HWPX file and encodes it to style-aware Markdown via
/// [`HwpxStyleLookup`] + [`MdEncoder::encode_styled`].  Images embedded in
/// the document are written alongside the Markdown file.
pub fn run_to_md(file_path: &str, output_dir: Option<&str>) -> Result<ToMdData, ToolErrorInfo> {
    // 0. Validate input extension (case-insensitive, path-aware)
    let ext_ok = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("hwpx"))
        .unwrap_or(false);
    if !ext_ok {
        return Err(ToolErrorInfo::new(
            "INVALID_INPUT",
            format!("Expected a .hwpx file, got: {file_path}"),
            "Provide a path to a valid .hwpx document.",
        ));
    }

    // 1. Read HWPX bytes
    let bytes = read_file_bytes(file_path)?;

    // 2. Decode HWPX → document + style store + image store
    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    // 3. Validate document
    let validated = hwpx_doc.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Document validation failed: {e}"),
            "The HWPX document structure is invalid.",
        )
    })?;

    // 4. Build style lookup bridge
    let lookup = HwpxStyleLookup::new(&hwpx_doc.style_store, &hwpx_doc.image_store);

    // 5. Encode to Markdown (style-aware)
    let md_output = MdEncoder::encode_styled(&validated, &lookup);

    // 6. Determine output directory
    let base_stem = Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output");

    let out_dir: String = if let Some(dir) = output_dir {
        dir.to_string()
    } else {
        // Default: same directory as input file
        Path::new(file_path).parent().and_then(|p| p.to_str()).unwrap_or(".").to_string()
    };

    // 7. Write Markdown file
    let md_filename = format!("{base_stem}.md");
    let md_path = Path::new(&out_dir).join(&md_filename).to_string_lossy().into_owned();
    let md_bytes = md_output.markdown.as_bytes();
    write_output_file(&md_path, md_bytes)?;

    // 8. Write extracted images into `images/` subdirectory (matches CLI behavior
    //    and the `images/{filename}` references generated in the markdown).
    let mut image_paths: Vec<String> = Vec::new();
    if !md_output.images.is_empty() {
        let images_dir = Path::new(&out_dir).join("images");
        let images_dir_str = images_dir.to_string_lossy().into_owned();
        // Ensure the images directory exists
        std::fs::create_dir_all(&images_dir).map_err(|e| {
            ToolErrorInfo::new(
                "DIR_CREATE_ERROR",
                format!("Cannot create images directory '{}': {e}", images_dir_str),
                "Check write permissions for the output directory.",
            )
        })?;
        for (rel_name, data) in &md_output.images {
            let img_filename = Path::new(rel_name.as_str())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image");
            let img_path = images_dir.join(img_filename);
            let img_path_str = img_path.to_string_lossy().into_owned();
            write_output_file(&img_path_str, data)?;
            image_paths.push(img_path_str);
        }
    }
    image_paths.sort();

    let size_bytes = md_bytes.len() as u64;
    let image_count = image_paths.len();

    Ok(ToMdData { markdown_path: md_path, image_paths, size_bytes, image_count })
}
