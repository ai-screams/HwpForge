//! Audit a converted HWPX file against its original HWP5 source.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwp5::{
    build_hwp5_semantic, inspect_hwp5, Hwp5DocInfoSummary, Hwp5SemanticDocument,
};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxDocument, HwpxStyleStore};

use crate::analysis::deep_counts::{
    summarize_hwp5_semantic, summarize_hwpx_document, DeepDocumentSummary, DeepSectionSummary,
    DeepTableCellEvidence, DeepTableEvidence,
};
use crate::error::{check_file_size, CliError};

#[derive(Serialize)]
struct AuditResult {
    status: &'static str,
    scope_note: &'static str,
    source: AuditSide,
    output: AuditSide,
    comparisons: Vec<MetricComparison>,
    section_comparisons: Vec<SectionComparison>,
    checklist: Vec<String>,
}

#[derive(Serialize)]
struct AuditSide {
    format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<String>,
    totals: AuditTotals,
    table_properties: AuditTableProperties,
    styles: AuditStyles,
    sections: Vec<AuditSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
struct AuditTableProperties {
    page_break_none: usize,
    page_break_table: usize,
    page_break_cell: usize,
    repeat_header_tables: usize,
    header_rows: usize,
    nonzero_cell_spacing_tables: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    table_border_fill_ids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cell_border_fill_ids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cell_heights_hwp: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cell_widths_hwp: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    table_widths_hwp: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    row_max_cell_heights_hwp: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    table_evidence: Vec<DeepTableEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cell_evidence: Vec<DeepTableCellEvidence>,
}

#[derive(Serialize)]
struct AuditTotals {
    sections: usize,
    paragraphs: usize,
    non_empty_paragraphs: usize,
    tables: usize,
    images: usize,
    text_boxes: usize,
    ole_objects: usize,
    lines: usize,
    rectangles: usize,
    polygons: usize,
    headers: usize,
    footers: usize,
    page_numbers: usize,
    landscape_sections: usize,
}

#[derive(Serialize)]
struct AuditStyles {
    font_faces: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_buckets: Option<FontBuckets>,
    char_shapes: usize,
    para_shapes: usize,
    styles: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FontBuckets {
    hangul: usize,
    latin: usize,
    hanja: usize,
    japanese: usize,
    other: usize,
    symbol: usize,
    user: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AuditSection {
    index: usize,
    paragraphs: usize,
    non_empty_paragraphs: usize,
    deep_paragraphs: usize,
    deep_non_empty_paragraphs: usize,
    tables: usize,
    images: usize,
    text_boxes: usize,
    ole_objects: usize,
    lines: usize,
    rectangles: usize,
    polygons: usize,
    has_header: bool,
    has_footer: bool,
    has_page_number: bool,
    landscape: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_non_empty_text: Option<String>,
}

#[derive(Serialize)]
struct MetricComparison {
    field: String,
    source: String,
    output: String,
    verdict: &'static str,
}

#[derive(Serialize)]
struct SectionComparison {
    index: usize,
    verdict: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<AuditSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<AuditSection>,
}

pub fn run(source: &Path, result: &Path, json_mode: bool) {
    check_file_size(source, json_mode);
    check_file_size(result, json_mode);

    let source_bytes = read_required(source, "source HWP5", json_mode);
    let result_bytes = read_required(result, "result HWPX", json_mode);

    let source_metadata = match inspect_hwp5(&source_bytes) {
        Ok(summary) => summary,
        Err(err) => CliError::new(
            "HWP5_DECODE_FAILED",
            format!("Cannot decode '{}': {err}", source.display()),
        )
        .with_hint("Check that the file is a valid HWP5 document")
        .exit(json_mode, 2),
    };
    let source_semantic = match build_hwp5_semantic(&source_bytes) {
        Ok(document) => document,
        Err(err) => CliError::new(
            "HWP5_SEMANTIC_FAILED",
            format!("Cannot build semantic truth for '{}': {err}", source.display()),
        )
        .with_hint("Check that the file is a valid HWP5 document")
        .exit(json_mode, 2),
    };

    let hwpx_doc = match HwpxDecoder::decode(&result_bytes) {
        Ok(doc) => doc,
        Err(err) => CliError::new(
            "HWPX_DECODE_FAILED",
            format!("Cannot decode '{}': {err}", result.display()),
        )
        .with_hint("Check that the file is a valid HWPX document")
        .exit(json_mode, 2),
    };
    let output_deep = match summarize_hwpx_document(&result_bytes, &hwpx_doc) {
        Ok(summary) => summary,
        Err(err) => CliError::new(
            "HWPX_ANALYSIS_FAILED",
            format!("Cannot analyze '{}' deeply: {err}", result.display()),
        )
        .with_hint("Check that the file is a valid HWPX document")
        .exit(json_mode, 2),
    };

    let source_side = audit_side_from_hwp5_semantic(
        &source_semantic,
        &source_metadata.doc_info,
        source_metadata.warning_count,
        source_metadata.validation_error.as_deref(),
    );
    let output_side = audit_side_from_hwpx(&hwpx_doc, &output_deep);
    let (comparisons, section_comparisons, mismatch_found) =
        compare_sides(&source_side, &output_side);
    let checklist = build_checklist(source, result, &source_side, mismatch_found);

    let report = AuditResult {
        status: if mismatch_found { "mismatch" } else { "ok" },
        scope_note: "Source metrics come from parser-backed HWP5 semantic truth. DocInfo style counts and warning count still come from the current decode summary. Output metrics come from recursive HWPX XML/package analysis, not top-level body-only counts.",
        source: source_side,
        output: output_side,
        comparisons,
        section_comparisons,
        checklist,
    };

    if json_mode {
        println!("{}", serde_json::to_string(&report).unwrap());
        return;
    }

    print_human_report(source, result, &report);
}

fn read_required(path: &Path, label: &str, json_mode: bool) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|err| {
        CliError::new(
            "FILE_READ_FAILED",
            format!("Cannot read {label} '{}': {err}", path.display()),
        )
        .exit(json_mode, 1)
    })
}

fn audit_side_from_hwp5_semantic(
    semantic: &Hwp5SemanticDocument,
    doc_info: &Hwp5DocInfoSummary,
    warning_count: usize,
    validation_error: Option<&str>,
) -> AuditSide {
    let deep_summary: DeepDocumentSummary = summarize_hwp5_semantic(semantic);
    let mut side = build_audit_side(
        "HWP5",
        &deep_summary,
        AuditStyles {
            font_faces: doc_info.font_faces,
            font_buckets: doc_info.font_buckets.as_ref().map(|b| FontBuckets {
                hangul: b.hangul,
                latin: b.latin,
                hanja: b.hanja,
                japanese: b.japanese,
                other: b.other,
                symbol: b.symbol,
                user: b.user,
            }),
            char_shapes: doc_info.char_shapes,
            para_shapes: doc_info.para_shapes,
            styles: doc_info.styles,
        },
        Some(semantic.package_meta.version.clone()),
        Some(warning_count),
    );
    if let Some(validation_error) = validation_error {
        side.notes.push(format!("validation-error: {validation_error}"));
    }
    side
}

fn audit_side_from_hwpx(doc: &HwpxDocument, deep_summary: &DeepDocumentSummary) -> AuditSide {
    build_audit_side(
        "HWPX",
        deep_summary,
        AuditStyles {
            font_faces: doc.style_store.font_count(),
            font_buckets: Some(font_buckets_from_hwpx(&doc.style_store)),
            char_shapes: doc.style_store.char_shape_count(),
            para_shapes: doc.style_store.para_shape_count(),
            styles: doc.style_store.style_count(),
        },
        None,
        None,
    )
}

fn build_audit_side(
    format: &'static str,
    deep_summary: &DeepDocumentSummary,
    styles: AuditStyles,
    version: Option<String>,
    warning_count: Option<usize>,
) -> AuditSide {
    AuditSide {
        format,
        version,
        warning_count,
        notes: deep_summary.notes.clone(),
        totals: audit_totals_from_summary(deep_summary),
        table_properties: audit_table_properties_from_summary(deep_summary),
        styles,
        sections: deep_summary.sections.iter().map(AuditSection::from).collect(),
    }
}

fn audit_totals_from_summary(summary: &DeepDocumentSummary) -> AuditTotals {
    AuditTotals {
        sections: summary.total_sections(),
        paragraphs: summary.total_paragraphs(),
        non_empty_paragraphs: summary.total_non_empty_paragraphs(),
        tables: summary.total_tables(),
        images: summary.total_images(),
        text_boxes: summary.total_text_boxes(),
        ole_objects: summary.total_ole_objects(),
        lines: summary.total_lines(),
        rectangles: summary.total_rectangles(),
        polygons: summary.total_polygons(),
        headers: summary.total_headers(),
        footers: summary.total_footers(),
        page_numbers: summary.total_page_numbers(),
        landscape_sections: summary.total_landscape_sections(),
    }
}

fn audit_table_properties_from_summary(summary: &DeepDocumentSummary) -> AuditTableProperties {
    AuditTableProperties {
        page_break_none: summary.table_properties.page_break_none,
        page_break_table: summary.table_properties.page_break_table,
        page_break_cell: summary.table_properties.page_break_cell,
        repeat_header_tables: summary.table_properties.repeat_header_tables,
        header_rows: summary.table_properties.header_rows,
        nonzero_cell_spacing_tables: summary.table_properties.nonzero_cell_spacing_tables,
        table_border_fill_ids: summary
            .table_properties
            .table_border_fill_ids
            .iter()
            .copied()
            .collect(),
        cell_border_fill_ids: summary
            .table_properties
            .cell_border_fill_ids
            .iter()
            .copied()
            .collect(),
        cell_heights_hwp: summary.table_properties.cell_heights_hwp.iter().copied().collect(),
        cell_widths_hwp: summary.table_properties.cell_widths_hwp.iter().copied().collect(),
        table_widths_hwp: summary.table_properties.table_widths_hwp.iter().copied().collect(),
        row_max_cell_heights_hwp: summary
            .table_properties
            .row_max_cell_heights_hwp
            .iter()
            .copied()
            .collect(),
        table_evidence: summary.table_properties.table_evidence.iter().cloned().collect(),
        cell_evidence: summary.table_properties.cell_evidence.iter().cloned().collect(),
    }
}

fn font_buckets_from_hwpx(store: &HwpxStyleStore) -> FontBuckets {
    let mut buckets =
        FontBuckets { hangul: 0, latin: 0, hanja: 0, japanese: 0, other: 0, symbol: 0, user: 0 };

    for font in store.iter_fonts() {
        match font.lang.as_str() {
            "HANGUL" => buckets.hangul += 1,
            "LATIN" => buckets.latin += 1,
            "HANJA" => buckets.hanja += 1,
            "JAPANESE" => buckets.japanese += 1,
            "OTHER" => buckets.other += 1,
            "SYMBOL" => buckets.symbol += 1,
            "USER" => buckets.user += 1,
            _ => {}
        }
    }

    buckets
}

fn compare_sides(
    source: &AuditSide,
    output: &AuditSide,
) -> (Vec<MetricComparison>, Vec<SectionComparison>, bool) {
    let mut mismatch_found = false;
    let mut comparisons: Vec<MetricComparison> = Vec::new();

    push_total_metric_comparisons(&mut comparisons, &mut mismatch_found, source, output);
    push_table_property_metric_comparisons(&mut comparisons, &mut mismatch_found, source, output);
    push_style_metric_comparisons(&mut comparisons, &mut mismatch_found, source, output);

    let section_comparisons: Vec<SectionComparison> =
        build_section_comparisons(source, output, &mut mismatch_found);

    (comparisons, section_comparisons, mismatch_found)
}

fn push_total_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "sections",
        source.totals.sections,
        output.totals.sections,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "paragraphs",
        source.totals.paragraphs,
        output.totals.paragraphs,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "non_empty_paragraphs",
        source.totals.non_empty_paragraphs,
        output.totals.non_empty_paragraphs,
    );
    push_metric(comparisons, mismatch_found, "tables", source.totals.tables, output.totals.tables);
    push_metric(comparisons, mismatch_found, "images", source.totals.images, output.totals.images);
    push_metric(
        comparisons,
        mismatch_found,
        "text_boxes",
        source.totals.text_boxes,
        output.totals.text_boxes,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "ole_objects",
        source.totals.ole_objects,
        output.totals.ole_objects,
    );
    push_metric(comparisons, mismatch_found, "lines", source.totals.lines, output.totals.lines);
    push_metric(
        comparisons,
        mismatch_found,
        "rectangles",
        source.totals.rectangles,
        output.totals.rectangles,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "polygons",
        source.totals.polygons,
        output.totals.polygons,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "headers",
        source.totals.headers,
        output.totals.headers,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "footers",
        source.totals.footers,
        output.totals.footers,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "page_numbers",
        source.totals.page_numbers,
        output.totals.page_numbers,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "landscape_sections",
        source.totals.landscape_sections,
        output.totals.landscape_sections,
    );
}

fn push_table_property_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_table_property_count_metric_comparisons(comparisons, mismatch_found, source, output);
    push_table_property_id_metric_comparisons(comparisons, mismatch_found, source, output);
    push_table_property_sizing_metric_comparisons(comparisons, mismatch_found, source, output);
    push_table_property_evidence_metric_comparisons(comparisons, mismatch_found, source, output);
}

fn push_table_property_count_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "table_page_break_none",
        source.table_properties.page_break_none,
        output.table_properties.page_break_none,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_page_break_table",
        source.table_properties.page_break_table,
        output.table_properties.page_break_table,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_page_break_cell",
        source.table_properties.page_break_cell,
        output.table_properties.page_break_cell,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_repeat_header_tables",
        source.table_properties.repeat_header_tables,
        output.table_properties.repeat_header_tables,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_header_rows",
        source.table_properties.header_rows,
        output.table_properties.header_rows,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_nonzero_cell_spacing_tables",
        source.table_properties.nonzero_cell_spacing_tables,
        output.table_properties.nonzero_cell_spacing_tables,
    );
}

fn push_table_property_id_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "table_border_fill_ids",
        join_u32_list(&source.table_properties.table_border_fill_ids),
        join_u32_list(&output.table_properties.table_border_fill_ids),
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_cell_border_fill_ids",
        join_u32_list(&source.table_properties.cell_border_fill_ids),
        join_u32_list(&output.table_properties.cell_border_fill_ids),
    );
}

fn push_table_property_sizing_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "table_cell_heights_hwp",
        join_i32_list(&source.table_properties.cell_heights_hwp),
        join_i32_list(&output.table_properties.cell_heights_hwp),
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_cell_widths_hwp",
        join_i32_list(&source.table_properties.cell_widths_hwp),
        join_i32_list(&output.table_properties.cell_widths_hwp),
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_structural_widths_hwp",
        join_i32_list(&source.table_properties.table_widths_hwp),
        join_i32_list(&output.table_properties.table_widths_hwp),
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_row_max_cell_heights_hwp",
        join_i32_list(&source.table_properties.row_max_cell_heights_hwp),
        join_i32_list(&output.table_properties.row_max_cell_heights_hwp),
    );
}

fn push_table_property_evidence_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "table_structural_evidence",
        serde_json::to_string(&source.table_properties.table_evidence)
            .expect("table structural evidence should serialize"),
        serde_json::to_string(&output.table_properties.table_evidence)
            .expect("table structural evidence should serialize"),
    );
    push_metric(
        comparisons,
        mismatch_found,
        "table_cell_evidence",
        serde_json::to_string(&source.table_properties.cell_evidence)
            .expect("table cell evidence should serialize"),
        serde_json::to_string(&output.table_properties.cell_evidence)
            .expect("table cell evidence should serialize"),
    );
}

fn push_style_metric_comparisons(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    source: &AuditSide,
    output: &AuditSide,
) {
    push_metric(
        comparisons,
        mismatch_found,
        "font_faces_total",
        source.styles.font_faces,
        output.styles.font_faces,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "char_shapes",
        source.styles.char_shapes,
        output.styles.char_shapes,
    );
    push_metric(
        comparisons,
        mismatch_found,
        "para_shapes",
        source.styles.para_shapes,
        output.styles.para_shapes,
    );
    push_metric(comparisons, mismatch_found, "styles", source.styles.styles, output.styles.styles);

    if let (Some(source_fonts), Some(output_fonts)) =
        (&source.styles.font_buckets, &output.styles.font_buckets)
    {
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_hangul",
            source_fonts.hangul,
            output_fonts.hangul,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_latin",
            source_fonts.latin,
            output_fonts.latin,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_hanja",
            source_fonts.hanja,
            output_fonts.hanja,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_japanese",
            source_fonts.japanese,
            output_fonts.japanese,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_other",
            source_fonts.other,
            output_fonts.other,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_symbol",
            source_fonts.symbol,
            output_fonts.symbol,
        );
        push_metric(
            comparisons,
            mismatch_found,
            "fonts_user",
            source_fonts.user,
            output_fonts.user,
        );
    }
}

fn build_section_comparisons(
    source: &AuditSide,
    output: &AuditSide,
    mismatch_found: &mut bool,
) -> Vec<SectionComparison> {
    let mut section_comparisons: Vec<SectionComparison> = Vec::new();
    let max_sections: usize = source.sections.len().max(output.sections.len());
    for index in 0..max_sections {
        let source_section: Option<AuditSection> = source.sections.get(index).cloned();
        let output_section: Option<AuditSection> = output.sections.get(index).cloned();
        let verdict: &'static str = if source_section == output_section {
            "MATCH"
        } else {
            *mismatch_found = true;
            "DIFF"
        };
        section_comparisons.push(SectionComparison {
            index,
            verdict,
            source: source_section,
            output: output_section,
        });
    }
    section_comparisons
}

fn push_metric<T: std::fmt::Display + PartialEq>(
    comparisons: &mut Vec<MetricComparison>,
    mismatch_found: &mut bool,
    field: &str,
    source: T,
    output: T,
) {
    let verdict = if source == output {
        "MATCH"
    } else {
        *mismatch_found = true;
        "DIFF"
    };
    comparisons.push(MetricComparison {
        field: field.to_string(),
        source: source.to_string(),
        output: output.to_string(),
        verdict,
    });
}

fn join_u32_list(values: &[u32]) -> String {
    values.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn join_i32_list(values: &[i32]) -> String {
    values.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

fn build_checklist(
    source_path: &Path,
    result_path: &Path,
    source: &AuditSide,
    mismatch_found: bool,
) -> Vec<String> {
    let mut checklist = vec![
        format!(
            "원본 '{}' 와 결과 '{}' 를 같은 배율로 나란히 연다.",
            source_path.display(),
            result_path.display()
        ),
        "리포트의 sections / paragraphs / non-empty paragraphs 값이 실제 화면 흐름과 맞는지 본다."
            .to_string(),
        "각 섹션의 첫 비어있지 않은 문단이 같은 위치와 같은 텍스트인지 확인한다.".to_string(),
        "대표 문단 2~3개에서 글꼴, 크기, 굵게/기울임, 문단 정렬이 유지됐는지 본다.".to_string(),
    ];

    if source.totals.tables > 0 {
        checklist.push(
            "모든 표에서 행/열 수, 병합(rowSpan/colSpan), 셀 텍스트 순서가 유지됐는지 확인한다."
                .to_string(),
        );
    }
    if source.totals.images > 0 {
        checklist.push(
            "본문/표 셀/머리말/글상자 안 그림이 빠지지 않았는지와 위치가 크게 어긋나지 않았는지 본다."
                .to_string(),
        );
    }
    if source.totals.text_boxes > 0 {
        checklist.push(
            "글상자 내부 텍스트와 중첩 객체 순서(text -> object -> text)가 유지됐는지 확인한다."
                .to_string(),
        );
    }
    if source.totals.ole_objects > 0 {
        checklist.push(
            "OLE 기반 객체가 있으므로 원본과 결과를 함께 열어 보이지 않는 임베디드 객체가 없는지 우선 확인한다."
                .to_string(),
        );
    }
    if source.totals.lines > 0 || source.totals.rectangles > 0 || source.totals.polygons > 0 {
        checklist.push(
            "선/사각형/다각형 도형의 존재 여부와 화면상 위치가 유지됐는지 확인한다.".to_string(),
        );
    }
    if source.totals.landscape_sections > 0 {
        checklist.push("가로 섹션의 방향과 여백이 원본과 같은지 확인한다.".to_string());
    }
    if source.totals.headers > 0 || source.totals.footers > 0 || source.totals.page_numbers > 0 {
        checklist.push("머리말, 꼬리말, 쪽번호의 존재 여부와 위치를 확인한다.".to_string());
    }
    if source.warning_count.unwrap_or(0) > 0 {
        checklist.push("HWP5 decode warning이 있었으므로 해당 섹션을 우선 재검토한다.".to_string());
    }
    if source.notes.iter().any(|note| note.starts_with("validation-error: ")) {
        checklist.push(
            "HWP5 projection 결과가 Core validation을 통과하지 못했으므로 source notes의 validation-error를 먼저 확인한다."
                .to_string(),
        );
    }
    if mismatch_found {
        checklist.push(
            "리포트에서 DIFF가 나온 항목은 XML과 화면을 함께 열고 우선 확인한다.".to_string(),
        );
    }

    checklist
}

fn print_human_report(source: &Path, result: &Path, report: &AuditResult) {
    println!("Audit: {} vs {}", source.display(), result.display());
    println!("Status: {}", report.status.to_uppercase());
    println!("Note: {}", report.scope_note);
    if let Some(version) = &report.source.version {
        println!("Source Version: {}", version);
    }
    if let Some(warnings) = report.source.warning_count {
        println!("HWP5 Warnings: {}", warnings);
    }
    if !report.source.notes.is_empty() {
        println!("Source Notes:");
        for note in &report.source.notes {
            println!("  - {}", note);
        }
    }
    if !report.output.notes.is_empty() {
        println!("Output Notes:");
        for note in &report.output.notes {
            println!("  - {}", note);
        }
    }
    println!();
    println!("{:<24} {:>12} {:>12} {:>8}", "Metric", "HWP5", "HWPX", "Verdict");
    for row in &report.comparisons {
        println!("{:<24} {:>12} {:>12} {:>8}", row.field, row.source, row.output, row.verdict);
    }

    println!();
    println!("Sections:");
    for row in &report.section_comparisons {
        let source_desc =
            row.source.as_ref().map(section_preview).unwrap_or_else(|| "missing".to_string());
        let output_desc =
            row.output.as_ref().map(section_preview).unwrap_or_else(|| "missing".to_string());
        println!(
            "  [{}] {:<5} HWP5: {} | HWPX: {}",
            row.index, row.verdict, source_desc, output_desc
        );
    }

    println!();
    println!("Visual Checklist:");
    for item in &report.checklist {
        println!("  [ ] {}", item);
    }
}

fn section_preview(section: &AuditSection) -> String {
    format!(
        "paras={} nonempty={} deepParas={} deepNonEmpty={} tables={} images={} textboxes={} ole={} lines={} rects={} polys={} hdr={} ftr={} pnum={} landscape={} first=\"{}\"",
        section.paragraphs,
        section.non_empty_paragraphs,
        section.deep_paragraphs,
        section.deep_non_empty_paragraphs,
        section.tables,
        section.images,
        section.text_boxes,
        section.ole_objects,
        section.lines,
        section.rectangles,
        section.polygons,
        section.has_header,
        section.has_footer,
        section.has_page_number,
        section.landscape,
        preview_text(section.first_non_empty_text.as_deref().unwrap_or("")),
    )
}

fn preview_text(text: &str) -> String {
    const MAX_CHARS: usize = 32;
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

impl From<&DeepSectionSummary> for AuditSection {
    fn from(summary: &DeepSectionSummary) -> Self {
        Self {
            index: summary.index,
            paragraphs: summary.paragraphs,
            non_empty_paragraphs: summary.non_empty_paragraphs,
            deep_paragraphs: summary.deep_paragraphs,
            deep_non_empty_paragraphs: summary.deep_non_empty_paragraphs,
            tables: summary.tables,
            images: summary.images,
            text_boxes: summary.text_boxes,
            ole_objects: summary.ole_objects,
            lines: summary.lines,
            rectangles: summary.rectangles,
            polygons: summary.polygons,
            has_header: summary.has_header,
            has_footer: summary.has_footer,
            has_page_number: summary.has_page_number,
            landscape: summary.landscape,
            first_non_empty_text: summary.first_non_empty_text.clone(),
        }
    }
}
