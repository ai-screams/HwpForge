//! Integration-style tests for the crate-root decode/census/image-join entry
//! points (`census_hwp5`, `join_hwp5_image_assets`, …).
//!
//! HWP5 → HWPX *conversion* tests live in the `hwpforge-convert` crate; this
//! module only exercises HWP5-native decode/census/join paths that touch this
//! crate's private modules.

use super::*;

use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
struct ImageFixtureExpectation {
    name: &'static str,
    expected_storage_names: &'static [&'static str],
    expected_gso_count: usize,
    expected_shape_picture_count: usize,
}

fn fixture_path(name: &str) -> PathBuf {
    crate::test_support::workspace_fixture_path(name)
}

fn shape_picture_count(report: &Hwp5CensusReport) -> usize {
    report
        .sections
        .iter()
        .flat_map(|section| section.tag_counts.iter())
        .filter(|entry| entry.tag_name == "ShapePicture")
        .map(|entry| entry.count)
        .sum()
}

fn ctrl_count(report: &Hwp5CensusReport, ctrl_id_ascii: &str) -> usize {
    report
        .sections
        .iter()
        .flat_map(|section| section.ctrl_ids.iter())
        .filter(|entry| entry.ctrl_id_ascii == ctrl_id_ascii)
        .map(|entry| entry.count)
        .sum()
}

fn storage_names(report: &Hwp5CensusReport) -> Vec<String> {
    let mut names: Vec<String> =
        report.doc_info.bin_data_records.iter().map(|record| record.storage_name.clone()).collect();
    names.sort();
    names
}

fn stream_names(report: &Hwp5CensusReport) -> Vec<String> {
    let mut names: Vec<String> =
        report.bin_data_streams.iter().map(|stream| stream.name.clone()).collect();
    names.sort();
    names
}

fn joined_asset_storage_names(plan: &Hwp5JoinedImageAssetPlan) -> Vec<String> {
    let mut names: Vec<String> =
        plan.ordered_assets.iter().map(|asset| asset.payload.storage_name.clone()).collect();
    names.sort();
    names
}

#[test]
fn census_image_fixture_matrix_reports_expected_bindata_and_gso_inventory() {
    let cases: [ImageFixtureExpectation; 8] = [
        ImageFixtureExpectation {
            name: "img_01_single_png_inline.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 1,
            expected_shape_picture_count: 1,
        },
        ImageFixtureExpectation {
            name: "img_03_two_images_png_jpg.hwp",
            expected_storage_names: &["BIN0001.png", "BIN0002.jpeg"],
            expected_gso_count: 2,
            expected_shape_picture_count: 2,
        },
        ImageFixtureExpectation {
            name: "img_05_image_in_table_cell.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 1,
            expected_shape_picture_count: 1,
        },
        ImageFixtureExpectation {
            name: "mixed_02a_header_image_footer_text_real.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 1,
            expected_shape_picture_count: 1,
        },
        ImageFixtureExpectation {
            name: "mixed_02b_textbox_with_image_real.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 2,
            expected_shape_picture_count: 1,
        },
        ImageFixtureExpectation {
            name: "floating_image_not_treat_as_char.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 1,
            expected_shape_picture_count: 1,
        },
        ImageFixtureExpectation {
            name: "two_same_image_refs_different_places.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 2,
            expected_shape_picture_count: 2,
        },
        ImageFixtureExpectation {
            name: "real_crop_vs_original_two_objects.hwp",
            expected_storage_names: &["BIN0001.png"],
            expected_gso_count: 2,
            expected_shape_picture_count: 2,
        },
    ];

    for case in cases {
        let path = fixture_path(case.name);
        if !path.exists() {
            continue;
        }

        let report = census_hwp5_file(&path).expect("fixture census should succeed");
        let expected_storage_names: Vec<String> =
            case.expected_storage_names.iter().map(|value| (*value).to_string()).collect();

        assert_eq!(storage_names(&report), expected_storage_names, "fixture={}", case.name);
        assert_eq!(stream_names(&report), expected_storage_names, "fixture={}", case.name);
        assert_eq!(ctrl_count(&report, "gso "), case.expected_gso_count, "fixture={}", case.name);
        assert_eq!(
            shape_picture_count(&report),
            case.expected_shape_picture_count,
            "fixture={}",
            case.name
        );
    }
}

#[test]
fn join_hwp5_image_assets_matches_fixture_bindata_inventory() {
    let cases: [(&str, &[&str]); 2] = [
        ("img_01_single_png_inline.hwp", &["BIN0001.png"]),
        ("img_03_two_images_png_jpg.hwp", &["BIN0001.png", "BIN0002.jpeg"]),
    ];

    for (name, expected_storage_names) in cases {
        let path = fixture_path(name);
        if !path.exists() {
            continue;
        }

        let bytes = std::fs::read(&path).expect("fixture bytes should be readable");
        let intermediate =
            crate::decoder::decode_intermediate(&bytes).expect("fixture intermediate decode");
        let image_assets =
            join_hwp5_image_assets(&bytes, &intermediate).expect("image assets should join");
        let expected_storage_names: Vec<String> =
            expected_storage_names.iter().map(|value| (*value).to_string()).collect();

        assert_eq!(
            joined_asset_storage_names(&image_assets),
            expected_storage_names,
            "fixture={name}"
        );
        assert!(
            image_assets.ordered_assets.iter().all(|asset| {
                asset.payload.width_hwp.is_some_and(|width| width > 0)
                    && asset.payload.height_hwp.is_some_and(|height| height > 0)
            }),
            "joined image assets should preserve positive geometry hints: fixture={name}"
        );
        assert!(
            image_assets.ordered_assets.iter().all(|asset| !asset.bytes.is_empty()),
            "fixture={name}"
        );
    }
}

#[test]
fn join_hwp5_image_assets_decompresses_full_report_png_payload() {
    let path = fixture_path("full_report.hwp");
    if !path.exists() {
        return;
    }

    let bytes = std::fs::read(&path).expect("fixture bytes should be readable");
    let intermediate =
        crate::decoder::decode_intermediate(&bytes).expect("fixture intermediate decode");
    let image_assets =
        join_hwp5_image_assets(&bytes, &intermediate).expect("image assets should join");
    let first_asset = image_assets
        .asset_for_binary_data_id(1)
        .expect("full_report should expose binary image id 1");

    assert!(
        first_asset.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "full_report joined image bytes must be actual PNG payload, not compressed raw data"
    );
}
