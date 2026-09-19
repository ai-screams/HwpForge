//! `hwpforge_to_md` — HWPX → Markdown conversion tool.

use std::path::Path;

use serde::Serialize;

use hwpforge::ops::{to_md as ops_to_md, MdExportOptions};

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo, ToolWarningInfo};

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
    /// Decode warnings from `ops::to_md` (`ops::MdExportOutput::warnings`).
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Execute HWPX → Markdown conversion.
///
/// Decodes an HWPX file and encodes it to style-aware Markdown via
/// [`hwpforge::ops::to_md`] (`MdMode::Styled` — the only mode that extracts
/// images). Images embedded in the document are written alongside the
/// Markdown file.
pub fn run_to_md(file_path: &str, output_dir: Option<&str>) -> Result<ToMdData, ToolErrorInfo> {
    // 0. Validate input extension (case-insensitive, path-aware; MCP-local
    //    — stays outside `ops`).
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

    // 2. Delegate decode → validate → styled-encode to `ops::to_md`.
    //    Decode warnings ride along in `out.warnings` and are surfaced
    //    through `ToMdData::warnings`.
    let out = ops_to_md(&bytes, &MdExportOptions::default())
        .map_err(|e| compat::tool_error(Tool::ToMd, e))?;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();

    // 3. Determine output directory
    let base_stem = Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output");

    let out_dir: String = if let Some(dir) = output_dir {
        dir.to_string()
    } else {
        // Default: same directory as input file
        Path::new(file_path).parent().and_then(|p| p.to_str()).unwrap_or(".").to_string()
    };

    // 4. Write Markdown file
    let md_filename = format!("{base_stem}.md");
    let md_path = Path::new(&out_dir).join(&md_filename).to_string_lossy().into_owned();
    let md_bytes = out.markdown.as_bytes();
    write_output_file(&md_path, md_bytes)?;

    // 5. Write extracted images into `images/` subdirectory (matches CLI behavior
    //    and the `images/{filename}` references generated in the markdown).
    let mut image_paths: Vec<String> = Vec::new();
    if !out.images.is_empty() {
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
        for (rel_name, data) in &out.images {
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

    Ok(ToMdData { markdown_path: md_path, image_paths, size_bytes, image_count, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("hwpforge_mcp_to_md_{}", std::process::id()))
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 병합 표가 든 최소 hwpx 를 만들어 경로를 돌려준다.
    fn merged_form_hwpx(dir: &std::path::Path) -> String {
        use hwpforge_core::page::PageSettings;
        use hwpforge_core::run::Run;
        use hwpforge_core::table::{Table, TableCell, TableRow};
        use hwpforge_core::{Document, Paragraph, Section};
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
        use hwpforge_smithy_hwpx::HwpxEncoder;

        let text_para = |t: &str| {
            Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
        };
        let cell = |t: &str, rs: u16| {
            TableCell::with_span(vec![text_para(t)], HwpUnit::new(8000).unwrap(), 1, rs)
        };
        let table = Table::new(vec![
            TableRow::new(vec![cell("병합", 2), cell("우측", 1)]),
            TableRow::new(vec![cell("아래", 1)]),
        ]);
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(table, CharShapeIndex::new(0)));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));

        let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(HwpxCharShape::default());
        styles.push_para_shape(HwpxParaShape::default());
        let bytes = HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &styles,
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap();
        let path = dir.join("merged.hwpx");
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn to_md_converts_merged_table_as_html_spans() {
        let dir = temp_dir("convert");
        let input = merged_form_hwpx(&dir);
        let data = run_to_md(&input, Some(dir.to_str().unwrap())).unwrap();
        assert!(std::path::Path::new(&data.markdown_path).exists());
        assert_eq!(data.image_count, 0);
        let md = std::fs::read_to_string(&data.markdown_path).unwrap();
        // styled 경로는 병합을 rowspan HTML 로 보존한다.
        assert!(md.contains("rowspan=\"2\""), "{md}");
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 to_md 하면, 디코드 경고
    /// (`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn to_md_surfaces_decode_warnings() {
        let dir = temp_dir("warnings");
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_to_md(&path, Some(dir.to_str().unwrap())).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "to_md must surface the decode warning: {:?}",
            data.warnings
        );
    }

    #[test]
    fn to_md_rejects_non_hwpx_extension() {
        let dir = temp_dir("reject");
        let bogus = dir.join("doc.txt");
        std::fs::write(&bogus, b"x").unwrap();
        let err = run_to_md(bogus.to_str().unwrap(), None).unwrap_err();
        assert_eq!(err.code, "INVALID_INPUT");
    }
}
