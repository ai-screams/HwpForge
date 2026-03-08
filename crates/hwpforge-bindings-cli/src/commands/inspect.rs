//! Inspect HWPX document structure.

use std::path::PathBuf;

use serde::Serialize;

use hwpforge_core::control::Control;
use hwpforge_core::RunContent;
use hwpforge_smithy_hwpx::HwpxDecoder;

use crate::error::{check_file_size, CliError};

#[derive(Serialize)]
struct InspectResult {
    status: &'static str,
    metadata: MetadataInfo,
    sections: Vec<SectionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<StylesInfo>,
}

#[derive(Serialize)]
struct MetadataInfo {
    title: String,
    author: String,
}

#[derive(Serialize)]
struct SectionInfo {
    index: usize,
    paragraphs: usize,
    tables: usize,
    images: usize,
    charts: usize,
    has_header: bool,
    has_footer: bool,
    has_page_number: bool,
}

#[derive(Serialize)]
struct StylesInfo {
    fonts: Vec<FontInfo>,
    char_shapes: Vec<CharShapeInfo>,
    para_shapes: Vec<ParaShapeInfo>,
}

#[derive(Serialize)]
struct FontInfo {
    id: usize,
    face_name: String,
    lang: String,
}

#[derive(Serialize)]
struct CharShapeInfo {
    id: usize,
    font_id: usize,
    size_pt: f64,
    bold: bool,
    italic: bool,
    color: String,
}

#[derive(Serialize)]
struct ParaShapeInfo {
    id: usize,
    alignment: String,
    line_spacing: i32,
}

/// Run the inspect command.
pub fn run(file: &PathBuf, show_styles: bool, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let hwpx_doc = match HwpxDecoder::decode(&bytes) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}"))
                .with_hint("Check that the file is a valid HWPX document")
                .exit(json_mode, 2);
        }
    };

    let doc = &hwpx_doc.document;
    let store = &hwpx_doc.style_store;
    let meta = doc.metadata();

    let sections: Vec<SectionInfo> = doc
        .sections()
        .iter()
        .enumerate()
        .map(|(i, sec)| {
            let mut tables = 0usize;
            let mut images = 0usize;
            let mut charts = 0usize;

            for para in &sec.paragraphs {
                for run in &para.runs {
                    match &run.content {
                        RunContent::Table(_) => tables += 1,
                        RunContent::Image(_) => images += 1,
                        RunContent::Control(c) => {
                            if matches!(**c, Control::Chart { .. }) {
                                charts += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }

            SectionInfo {
                index: i,
                paragraphs: sec.paragraphs.len(),
                tables,
                images,
                charts,
                has_header: sec.header.is_some(),
                has_footer: sec.footer.is_some(),
                has_page_number: sec.page_number.is_some(),
            }
        })
        .collect();

    let styles = if show_styles {
        let mut seen_fonts = std::collections::HashSet::new();
        let mut fonts = Vec::new();
        for i in 0..store.font_count() {
            if let Ok(f) = store.font(hwpforge_foundation::FontIndex::new(i)) {
                if seen_fonts.insert((f.face_name.clone(), f.lang.clone())) {
                    fonts.push(FontInfo {
                        id: i,
                        face_name: f.face_name.clone(),
                        lang: f.lang.clone(),
                    });
                }
            }
        }

        let char_shapes: Vec<CharShapeInfo> = (0..store.char_shape_count())
            .filter_map(|i| {
                store.char_shape(hwpforge_foundation::CharShapeIndex::new(i)).ok().map(|cs| {
                    CharShapeInfo {
                        id: i,
                        font_id: cs.font_ref.hangul.get(),
                        size_pt: cs.height.as_i32() as f64 / 100.0,
                        bold: cs.bold,
                        italic: cs.italic,
                        color: cs.text_color.to_hex_rgb(),
                    }
                })
            })
            .collect();

        let para_shapes: Vec<ParaShapeInfo> = (0..store.para_shape_count())
            .filter_map(|i| {
                store.para_shape(hwpforge_foundation::ParaShapeIndex::new(i)).ok().map(|ps| {
                    ParaShapeInfo {
                        id: i,
                        alignment: serde_json::to_value(ps.alignment)
                            .ok()
                            .and_then(|v| v.as_str().map(String::from))
                            .unwrap_or_else(|| format!("{:?}", ps.alignment)),
                        line_spacing: ps.line_spacing,
                    }
                })
            })
            .collect();

        Some(StylesInfo { fonts, char_shapes, para_shapes })
    } else {
        None
    };

    let result = InspectResult {
        status: "ok",
        metadata: MetadataInfo {
            title: meta.title.clone().unwrap_or_default(),
            author: meta.author.clone().unwrap_or_default(),
        },
        sections,
        styles,
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Document: {}", file.display());
        println!("  Title:  {}", result.metadata.title);
        println!("  Author: {}", result.metadata.author);
        println!("  Sections: {}", result.sections.len());
        for sec in &result.sections {
            println!(
                "    [{}] {} paras, {} tables, {} images, {} charts | header={} footer={} pagenum={}",
                sec.index, sec.paragraphs, sec.tables, sec.images, sec.charts,
                sec.has_header, sec.has_footer, sec.has_page_number
            );
        }
        if let Some(styles) = &result.styles {
            println!("  Fonts: {} unique", styles.fonts.len());
            println!("  CharShapes: {}", styles.char_shapes.len());
            println!("  ParaShapes: {}", styles.para_shapes.len());
        }
    }
}
