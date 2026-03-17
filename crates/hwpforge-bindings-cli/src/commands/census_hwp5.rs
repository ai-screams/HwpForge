//! Build a raw fixture census for HWP5 documents and optional HWPX companions.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwp5::{census_hwp5, Hwp5CensusReport};
use hwpforge_smithy_hwpx::{PackageEntryInfo, PackageReader};

use crate::analysis::hwpx_paths::{collect_section_path_inventory, HwpxPathOccurrence};
use crate::error::{check_file_size, CliError};

#[derive(Debug, Serialize)]
struct CensusResult {
    status: &'static str,
    source: String,
    hwp5: Hwp5CensusReport,
    chart_evidence: ChartDiscoveryEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    companion: Option<HwpxCompanionCensus>,
}

#[derive(Debug, Serialize)]
struct ChartDiscoveryEvidence {
    source: Hwp5ChartEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    companion: Option<HwpxChartEvidence>,
    assessment: &'static str,
}

#[derive(Debug, Serialize)]
struct Hwp5ChartEvidence {
    gso_ctrl_count: usize,
    shape_component_ole_count: usize,
    chart_data_tag_count: usize,
    ole_bin_data_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HwpxChartEvidence {
    chart_xml_paths: Vec<String>,
    ole_bindata_paths: Vec<String>,
    switch_count: usize,
    case_chart_count: usize,
    default_ole_count: usize,
}

#[derive(Debug, Serialize)]
struct HwpxCompanionCensus {
    path: String,
    package_entries: Vec<HwpxPackageEntry>,
    section_count: usize,
    chart_xml_paths: Vec<String>,
    bindata_paths: Vec<String>,
    content_hpf_mentions_chart: bool,
    path_inventory: Vec<HwpxPathOccurrence>,
}

#[derive(Debug, Serialize)]
struct HwpxPackageEntry {
    path: String,
    size: u64,
    compressed_size: u64,
}

/// Run the census-hwp5 command.
pub fn run(
    input: &Path,
    companion: &Option<std::path::PathBuf>,
    output: &Option<std::path::PathBuf>,
    json_mode: bool,
) {
    check_file_size(input, json_mode);
    if let Some(companion) = companion {
        check_file_size(companion, json_mode);
    }

    let input_bytes = std::fs::read(input).unwrap_or_else(|err| {
        CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {err}", input.display()))
            .exit(json_mode, 1)
    });

    let hwp5 = census_hwp5(&input_bytes).unwrap_or_else(|err| {
        CliError::new("HWP5_CENSUS_FAILED", format!("Cannot census '{}': {err}", input.display()))
            .with_hint("Check that the file is a valid HWP5 document")
            .exit(json_mode, 2)
    });

    let companion = companion.as_ref().map(|path| census_hwpx_companion(path, json_mode));
    let chart_evidence = build_chart_evidence(&hwp5, companion.as_ref());
    let mut result = CensusResult {
        status: "ok",
        source: input.display().to_string(),
        hwp5,
        chart_evidence,
        companion,
    };
    canonicalize_external_census_result(&mut result);

    if let Some(output) = output {
        let json = serde_json::to_string_pretty(&result).unwrap();
        std::fs::write(output, json).unwrap_or_else(|err| {
            CliError::new(
                "FILE_WRITE_FAILED",
                format!("Cannot write census output '{}': {err}", output.display()),
            )
            .exit(json_mode, 1)
        });
    }

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        print_human_report(&result, output.as_deref());
    }
}

fn census_hwpx_companion(path: &Path, json_mode: bool) -> HwpxCompanionCensus {
    let bytes = std::fs::read(path).unwrap_or_else(|err| {
        CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {err}", path.display()))
            .exit(json_mode, 1)
    });

    let mut pkg = PackageReader::new(&bytes).unwrap_or_else(|err| {
        CliError::new(
            "HWPX_CENSUS_FAILED",
            format!("Cannot census companion '{}': {err}", path.display()),
        )
        .with_hint("Check that the companion file is a valid HWPX document")
        .exit(json_mode, 2)
    });

    let package_entries = pkg
        .list_entries()
        .unwrap_or_else(|err| {
            CliError::new(
                "HWPX_CENSUS_FAILED",
                format!("Cannot list HWPX entries '{}': {err}", path.display()),
            )
            .exit(json_mode, 2)
        })
        .into_iter()
        .map(hwpx_package_entry)
        .collect::<Vec<_>>();

    let section_count = pkg.section_count();
    let chart_xmls = pkg.read_chart_xmls().unwrap_or_else(|err| {
        CliError::new(
            "HWPX_CENSUS_FAILED",
            format!("Cannot read chart XMLs from '{}': {err}", path.display()),
        )
        .exit(json_mode, 2)
    });
    let mut chart_xml_paths: Vec<String> = chart_xmls.keys().cloned().collect();
    chart_xml_paths.sort();

    let bindata_paths: Vec<String> = package_entries
        .iter()
        .filter(|entry| entry.path.starts_with("BinData/"))
        .map(|entry| entry.path.clone())
        .collect();

    let content_hpf_mentions_chart = pkg
        .read_text_entry("Contents/content.hpf")
        .map(|text| text.contains("Chart/"))
        .unwrap_or(false);

    let path_inventory = collect_section_path_inventory(&mut pkg).unwrap_or_else(|err| {
        CliError::new(
            "HWPX_CENSUS_FAILED",
            format!("Cannot inventory HWPX paths from '{}': {err}", path.display()),
        )
        .exit(json_mode, 2)
    });

    HwpxCompanionCensus {
        path: path.display().to_string(),
        package_entries,
        section_count,
        chart_xml_paths,
        bindata_paths,
        content_hpf_mentions_chart,
        path_inventory,
    }
}

fn hwpx_package_entry(entry: PackageEntryInfo) -> HwpxPackageEntry {
    HwpxPackageEntry { path: entry.path, size: entry.size, compressed_size: entry.compressed_size }
}

fn build_chart_evidence(
    hwp5: &Hwp5CensusReport,
    companion: Option<&HwpxCompanionCensus>,
) -> ChartDiscoveryEvidence {
    let source: Hwp5ChartEvidence = Hwp5ChartEvidence {
        gso_ctrl_count: hwp5
            .sections
            .iter()
            .flat_map(|section| section.ctrl_ids.iter())
            .filter(|ctrl_id| ctrl_id.ctrl_id_ascii == "gso ")
            .map(|ctrl_id| ctrl_id.count)
            .sum(),
        shape_component_ole_count: hwp5
            .sections
            .iter()
            .flat_map(|section| section.tag_counts.iter())
            .filter(|tag| tag.tag_name == "ShapeComponentOle" || tag.tag_id == 0x54)
            .map(|tag| tag.count)
            .sum(),
        chart_data_tag_count: hwp5
            .sections
            .iter()
            .flat_map(|section| section.tag_counts.iter())
            .filter(|tag| tag.tag_name == "ChartData" || tag.tag_id == 0x5F)
            .map(|tag| tag.count)
            .sum(),
        ole_bin_data_paths: hwp5
            .bin_data_streams
            .iter()
            .filter(|stream| stream.name.to_ascii_lowercase().ends_with(".ole"))
            .map(|stream| format!("BinData/{}", stream.name))
            .collect(),
    };

    let companion_evidence: Option<HwpxChartEvidence> =
        companion.map(|companion| HwpxChartEvidence {
            chart_xml_paths: companion.chart_xml_paths.clone(),
            ole_bindata_paths: companion
                .bindata_paths
                .iter()
                .filter(|path| path.to_ascii_lowercase().ends_with(".ole"))
                .cloned()
                .collect(),
            switch_count: companion
                .path_inventory
                .iter()
                .filter(|entry| entry.kind == "switch")
                .count(),
            case_chart_count: companion
                .path_inventory
                .iter()
                .filter(|entry| entry.path.ends_with("/case/chart"))
                .count(),
            default_ole_count: companion
                .path_inventory
                .iter()
                .filter(|entry| entry.path.ends_with("/default/ole"))
                .count(),
        });

    let assessment: &'static str =
        if source.shape_component_ole_count > 0 && !source.ole_bin_data_paths.is_empty() {
            "ole-backed-gso-evidence"
        } else if source.chart_data_tag_count > 0 {
            "chart-data-tag-evidence"
        } else {
            "no-chart-evidence"
        };

    ChartDiscoveryEvidence { source, companion: companion_evidence, assessment }
}

fn print_human_report(result: &CensusResult, output: Option<&Path>) {
    println!("Census: {}", result.source);
    println!(
        "  HWP5 version {} | compressed={} | package entries={} | sections={} | bin streams={}",
        result.hwp5.version,
        result.hwp5.compressed,
        result.hwp5.package_entries.len(),
        result.hwp5.sections.len(),
        result.hwp5.bin_data_streams.len()
    );
    println!(
        "  DocInfo records={} | tags={} | BinData records={}",
        result.hwp5.doc_info.record_count,
        result.hwp5.doc_info.tag_counts.len(),
        result.hwp5.doc_info.bin_data_records.len()
    );
    for section in &result.hwp5.sections {
        println!(
            "    section{}: {} records | {} tags | {} ctrl ids",
            section.index,
            section.record_count,
            section.tag_counts.len(),
            section.ctrl_ids.len()
        );
    }
    println!(
        "  Chart evidence {} | HWP5 gso={} ole-tags={} chart-data-tags={} ole-bindata={}",
        result.chart_evidence.assessment,
        result.chart_evidence.source.gso_ctrl_count,
        result.chart_evidence.source.shape_component_ole_count,
        result.chart_evidence.source.chart_data_tag_count,
        result.chart_evidence.source.ole_bin_data_paths.len()
    );

    if let Some(companion) = &result.companion {
        println!(
            "  Companion {} | entries={} | charts={} | bindata={} | section paths={}",
            companion.path,
            companion.package_entries.len(),
            companion.chart_xml_paths.len(),
            companion.bindata_paths.len(),
            companion.path_inventory.len()
        );
    }
    if let Some(companion) = &result.chart_evidence.companion {
        println!(
            "  Companion chart evidence | chart-xml={} ole-bindata={} switch={} case/chart={} default/ole={}",
            companion.chart_xml_paths.len(),
            companion.ole_bindata_paths.len(),
            companion.switch_count,
            companion.case_chart_count,
            companion.default_ole_count
        );
    }

    if let Some(output) = output {
        println!("  Saved JSON: {}", output.display());
    }
}

/// Canonicalize externally visible census strings.
///
/// Policy:
/// - CLI JSON output and checked-in research datasets use escaped-string canonical form.
/// - Raw semantic values remain internal reference semantics inside smithy-hwp5.
/// - This external view is transport-safe for JSON/diff tooling, but it is not intended to
///   reconstruct raw control bytes by itself.
/// - Escaped control bytes use lowercase hex digits for a single stable textual form.
fn canonicalize_external_census_result(result: &mut CensusResult) {
    result.source = canonicalize_external_string(&result.source);
    canonicalize_external_hwp5_report(&mut result.hwp5);
    canonicalize_chart_evidence(&mut result.chart_evidence);
    if let Some(companion) = result.companion.as_mut() {
        canonicalize_external_hwpx_companion(companion);
    }
}

fn canonicalize_external_hwp5_report(report: &mut Hwp5CensusReport) {
    report.version = canonicalize_external_string(&report.version);
    for entry in &mut report.package_entries {
        entry.path = canonicalize_external_string(&entry.path);
    }
    canonicalize_external_stream_census(&mut report.doc_info);
    for section in &mut report.sections {
        for tag_count in &mut section.tag_counts {
            tag_count.tag_name = canonicalize_external_string(&tag_count.tag_name);
        }
        for ctrl_id in &mut section.ctrl_ids {
            ctrl_id.ctrl_id_ascii = canonicalize_external_string(&ctrl_id.ctrl_id_ascii);
            ctrl_id.ctrl_id_hex = canonicalize_external_string(&ctrl_id.ctrl_id_hex);
        }
    }
    for stream in &mut report.bin_data_streams {
        stream.name = canonicalize_external_string(&stream.name);
    }
}

fn canonicalize_external_stream_census(stream: &mut hwpforge_smithy_hwp5::Hwp5StreamCensus) {
    for tag_count in &mut stream.tag_counts {
        tag_count.tag_name = canonicalize_external_string(&tag_count.tag_name);
    }
    for record in &mut stream.bin_data_records {
        record.storage_name = canonicalize_external_string(&record.storage_name);
        record.extension = canonicalize_external_string(&record.extension);
        record.data_type = canonicalize_external_string(&record.data_type);
        record.compression = canonicalize_external_string(&record.compression);
    }
}

fn canonicalize_external_hwpx_companion(companion: &mut HwpxCompanionCensus) {
    companion.path = canonicalize_external_string(&companion.path);
    for entry in &mut companion.package_entries {
        entry.path = canonicalize_external_string(&entry.path);
    }
    for path in &mut companion.chart_xml_paths {
        *path = canonicalize_external_string(path);
    }
    for path in &mut companion.bindata_paths {
        *path = canonicalize_external_string(path);
    }
    for occurrence in &mut companion.path_inventory {
        occurrence.kind = canonicalize_external_string(&occurrence.kind);
        occurrence.path = canonicalize_external_string(&occurrence.path);
        occurrence.ref_id =
            occurrence.ref_id.as_ref().map(|value| canonicalize_external_string(value));
        occurrence.text = occurrence.text.as_ref().map(|value| canonicalize_external_string(value));
    }
}

fn canonicalize_chart_evidence(evidence: &mut ChartDiscoveryEvidence) {
    for path in &mut evidence.source.ole_bin_data_paths {
        *path = canonicalize_external_string(path);
    }
    if let Some(companion) = evidence.companion.as_mut() {
        for path in &mut companion.chart_xml_paths {
            *path = canonicalize_external_string(path);
        }
        for path in &mut companion.ole_bindata_paths {
            *path = canonicalize_external_string(path);
        }
    }
}

fn canonicalize_external_string(value: &str) -> String {
    let mut out: String = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            let codepoint: u32 = ch.into();
            out.push_str(&format!("\\u{codepoint:04x}"));
        } else {
            out.push(ch);
        }
    }
    out
}
