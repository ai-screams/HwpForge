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
            name: "anchored_zero_origin_png.hwp",
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
        ("anchored_zero_origin_png.hwp", &["BIN0001.png"]),
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
        let image_assets = join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED)
            .expect("image assets should join")
            .0;
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
    let image_assets = join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED)
        .expect("image assets should join")
        .0;
    let first_asset = image_assets
        .asset_for_binary_data_id(1)
        .expect("full_report should expose binary image id 1");

    assert!(
        first_asset.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "full_report joined image bytes must be actual PNG payload, not compressed raw data"
    );
}

/// Direct unit coverage for `Hwp5StyleStore::border_fill_image_binary_ids`,
/// which is `pub(crate)` and therefore unreachable from the conversion tests
/// that moved to `hwpforge-convert` (E5). It is consumed by
/// `supplement_border_fill_image_assets`; this asserts it collects the binary
/// data id of every image-fill border directly, restoring the assertion that
/// the E5 move could only leave as a comment in the convert crate.
#[test]
fn border_fill_image_binary_ids_collects_image_fill_ids() {
    use crate::decoder::header::Hwp5DocInfoBorderFillSlot;
    use crate::schema::border_fill::{
        Hwp5BorderLineKind, Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5RawBorderFill,
        Hwp5RawBorderFillFill, Hwp5RawBorderLine, Hwp5RawImageFill,
    };
    use crate::style_store::Hwp5StyleStore;

    let none_line =
        || Hwp5RawBorderLine { kind: Hwp5BorderLineKind::None, width: 0, color: 0x0000_0000 };
    let image_fill = Hwp5RawBorderFill {
        property: 0,
        three_d: false,
        shadow: false,
        slash_diagonal_shape: 0,
        back_slash_diagonal_shape: 0,
        center_line: false,
        left: none_line(),
        right: none_line(),
        top: none_line(),
        bottom: none_line(),
        diagonal: none_line(),
        fill: Hwp5RawBorderFillFill::Image(Hwp5RawImageFill {
            mode: Hwp5FillImageMode::TileAll,
            brightness: 0,
            contrast: 0,
            effect: Hwp5FillImageEffect::RealPic,
            bindata_id: 1,
            extra_data: Vec::new(),
        }),
    };
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![Hwp5DocInfoBorderFillSlot { id: 4, fill: Some(image_fill) }],
    };

    assert_eq!(store.border_fill_image_binary_ids().into_iter().collect::<Vec<_>>(), vec![1]);
}

// ── R0-2: chart and image decompression are bounded by the document budget ──

fn read_cfb_stream(bytes: &[u8], path: &str) -> Vec<u8> {
    use std::io::Read;
    let mut comp = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).expect("CFB opens");
    let mut stream = comp.open_stream(path).expect("stream exists");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("stream reads");
    buf
}

/// Returns `cfb_bytes` with the stream at `path` replaced by `data`.
fn replace_cfb_stream(cfb_bytes: Vec<u8>, path: &str, data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut comp =
        cfb::CompoundFile::open(std::io::Cursor::new(cfb_bytes)).expect("CFB opens read-write");
    let mut stream = comp.create_stream(path).expect("stream is replaced");
    stream.write_all(data).expect("stream writes");
    drop(stream);
    comp.flush().expect("CFB flushes");
    comp.into_inner().into_inner()
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).expect("deflate");
    encoder.finish().expect("deflate finish")
}

fn chart_01_bytes() -> Vec<u8> {
    std::fs::read(fixture_path("charts/chart_01_single_column.hwp")).expect("chart fixture exists")
}

fn embedded_chart_count(document: &Document<Draft>) -> usize {
    document
        .sections()
        .iter()
        .flat_map(|section| section.paragraphs.iter())
        .flat_map(|paragraph| paragraph.runs.iter())
        .filter(|run| {
            matches!(run.content.as_control(), Some(hwpforge_core::Control::EmbeddedChart { .. }))
        })
        .count()
}

fn total_budget_error_message<T: std::fmt::Debug>(result: Hwp5Result<T>) -> String {
    let err = result.expect_err("document budget overrun must fail the document");
    assert!(matches!(err, Hwp5Error::Cfb { .. }), "got: {err:?}");
    let msg = err.to_string();
    assert!(msg.contains("total decompressed data"), "got: {msg}");
    msg
}

fn zlib(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).expect("zlib");
    encoder.finish().expect("zlib finish")
}

/// Inflated `/BinData/BIN0001.OLE` of `chart_01`.
fn chart_01_inflated_ole(fixture: &[u8]) -> Vec<u8> {
    decoder::package::decompress_stream(&read_cfb_stream(fixture, "/BinData/BIN0001.OLE"))
        .expect("fixture OLE inflates")
}

/// The fixture chart with `/OOXMLChartContents` padded by 4 MiB of trailing
/// whitespace (still well-formed XML) inside the real inner OLE2, rebuilt as
/// length prefix + inner OLE2. A valid chart whose compression ratio is far
/// over 100.
fn padded_chart_inflated(fixture: &[u8]) -> Vec<u8> {
    use crate::decoder::chart_ole::extract_chart_payload_from_inflated;

    let inner = chart_01_inflated_ole(fixture)[4..].to_vec();
    let mut chart_xml = read_cfb_stream(&inner, "/OOXMLChartContents");
    chart_xml.extend(std::iter::repeat_n(b' ', 4 * 1024 * 1024));
    let padded_inner = replace_cfb_stream(inner, "/OOXMLChartContents", &chart_xml);
    let mut padded_inflated = (padded_inner.len() as u32).to_le_bytes().to_vec();
    padded_inflated.extend_from_slice(&padded_inner);
    // Precondition: the chart itself is valid, so only the ratio limit can
    // reject it.
    extract_chart_payload_from_inflated(&padded_inflated, padded_inflated.len() as u64)
        .expect("padded chart is a valid chart");
    padded_inflated
}

fn has_ratio_drop(warnings: &[Hwp5Warning]) -> bool {
    warnings.iter().any(|warning| {
        matches!(
            warning,
            Hwp5Warning::DroppedControl { control: "ole_object", reason }
                if reason.contains("decompression ratio")
        )
    })
}

// 이것을 실패시키는 것: OLE join 에서 `decompress_ratio_checked` 대신 압축비 검사가
// 없는 `decompress_stream` 쓰기 (공백으로 늘린 정상 차트가 추출되어 경고가 사라진다).
#[test]
fn high_ratio_valid_chart_is_dropped_with_ratio_warning() {
    let fixture = chart_01_bytes();
    let padded_inflated = padded_chart_inflated(&fixture);
    let compressed = deflate(&padded_inflated);
    assert!(
        padded_inflated.len() / compressed.len() > 100,
        "precondition: ratio {} must exceed 100",
        padded_inflated.len() / compressed.len()
    );

    let bytes = replace_cfb_stream(fixture, "/BinData/BIN0001.OLE", &compressed);
    let decoded = decode_hwp5_to_core(&bytes).expect("a rejected chart must not fail the document");
    assert_eq!(embedded_chart_count(&decoded.document), 0);
    assert!(
        has_ratio_drop(&decoded.warnings),
        "expected an ole_object drop citing the ratio, got: {:?}",
        decoded.warnings
    );
}

// Behaviour change pinned here: chart OLE now goes through the same stream
// decompressor as other HWP5 streams, including its zlib fallback, so a
// zlib-framed chart (previously dropped as `ole_chart_inflate_failed`) is
// extracted — and the fallback path is still under the ratio limit.
// 이것을 실패시키는 것 (각각 따로 실행):
// - `decompress_stream_capped` 의 zlib 폴백 제거 → zlib 차트가 드롭되어 차트 수 0.
// - OLE join 에서 압축비 검사 없는 `decompress_stream` 쓰기 → zlib 로 감싼 고압축
//   차트가 추출되어 압축비 경고가 사라진다.
#[test]
fn zlib_framed_chart_ole_is_extracted_and_still_ratio_checked() {
    let fixture = chart_01_bytes();

    let zlib_ole = zlib(&chart_01_inflated_ole(&fixture));
    let bytes = replace_cfb_stream(fixture.clone(), "/BinData/BIN0001.OLE", &zlib_ole);
    let decoded = decode_hwp5_to_core(&bytes).expect("zlib-framed chart decodes");
    assert_eq!(embedded_chart_count(&decoded.document), 1, "zlib chart must be extracted");
    assert!(
        !decoded.warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control: "ole_object", .. }
        )),
        "no ole_object drop expected, got: {:?}",
        decoded.warnings
    );

    let padded_inflated = padded_chart_inflated(&fixture);
    let zlib_padded = zlib(&padded_inflated);
    assert!(padded_inflated.len() / zlib_padded.len() > 100, "precondition: ratio over 100");
    let bytes = replace_cfb_stream(fixture, "/BinData/BIN0001.OLE", &zlib_padded);
    let decoded = decode_hwp5_to_core(&bytes).expect("a rejected chart must not fail the document");
    assert_eq!(embedded_chart_count(&decoded.document), 0);
    assert!(
        has_ratio_drop(&decoded.warnings),
        "the zlib fallback must keep the ratio limit, got: {:?}",
        decoded.warnings
    );
}

// 이것을 실패시키는 것 (각각 따로 실행):
// - OLE join 의 `budget.charge` 제거 → 첫 단(OLE inflate 몫 부족)이 `Ok` 가 된다.
// - projection 에 OLE join 이전 예산 사본 넘기기 → 둘째 단이 `Ok` 가 된다.
#[test]
fn chart_document_budget_ladder_covers_ole_join_and_extraction() {
    use std::io::Read;

    let bytes = chart_01_bytes();
    let intermediate = decoder::decode_intermediate(&bytes).expect("fixture decodes");
    let base = join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED)
        .expect("images join")
        .1
        .used();

    // Independent measurements of the two chart charges.
    let inflated =
        decoder::package::decompress_stream(&read_cfb_stream(&bytes, "/BinData/BIN0001.OLE"))
            .expect("fixture OLE inflates");
    let inner = &inflated[4..];
    let mut inner_cfb = cfb::CompoundFile::open(std::io::Cursor::new(inner)).expect("inner CFB");
    let mut xml = Vec::new();
    inner_cfb
        .open_stream("/OOXMLChartContents")
        .expect("chart stream")
        .read_to_end(&mut xml)
        .expect("chart stream reads");
    let xml_len = if xml.starts_with(&[0xEF, 0xBB, 0xBF]) { xml.len() - 3 } else { xml.len() };
    let inflated_len = inflated.len() as u64;
    let extraction_len = (xml_len + inner.len()) as u64;

    let msg = total_budget_error_message(decode_hwp5_to_core_with_budget_limit(
        &bytes,
        base + inflated_len - 1,
    ));
    assert!(msg.contains("(inflated OLE)"), "OLE join must hit the limit first: {msg}");

    let msg = total_budget_error_message(decode_hwp5_to_core_with_budget_limit(
        &bytes,
        base + inflated_len + extraction_len - 1,
    ));
    assert!(msg.contains("(extracted)"), "chart extraction must hit the limit: {msg}");

    let decoded =
        decode_hwp5_to_core_with_budget_limit(&bytes, base + inflated_len + extraction_len)
            .expect("exactly enough budget");
    assert_eq!(embedded_chart_count(&decoded.document), 1);
}

fn compressed_bmp_record() -> Hwp5BinDataRecordSummary {
    Hwp5BinDataRecordSummary {
        binary_data_id: 1,
        storage_name: "BIN0001.bmp".to_string(),
        extension: "bmp".to_string(),
        data_type: "Embedding".to_string(),
        compression: "Compress".to_string(),
        should_decompress: true,
    }
}

// 이것을 실패시키는 것 (각각 따로 실행):
// - 이미지 해제에 `decompress_ratio_checked` 쓰기 → 압축비 100 초과 BMP 가 거부된다.
// - `decode_bin_data_payload` 의 `budget.charge` 제거 → 1 바이트 부족한 예산이 통과한다.
#[test]
fn image_decompression_is_charged_without_a_ratio_limit() {
    use decoder::package::DecompressBudget;

    // A flat-colour BMP-like payload: legitimate images compress past 100x.
    let image = vec![0xFFu8; 1024 * 1024];
    let raw = deflate(&image);
    assert!(image.len() / raw.len() > 100, "precondition: ratio over 100");
    let record = compressed_bmp_record();

    let mut budget = DecompressBudget::new(image.len() as u64);
    let data = decode_bin_data_payload(&raw, &record, "BIN0001.bmp", &mut budget)
        .expect("high-ratio image is accepted at exactly the budget");
    assert_eq!(data, image);
    assert_eq!(budget.used(), image.len() as u64);

    let mut budget = DecompressBudget::new(image.len() as u64 - 1);
    let msg = total_budget_error_message(decode_bin_data_payload(
        &raw,
        &record,
        "BIN0001.bmp",
        &mut budget,
    ));
    assert!(msg.contains("BIN0001.bmp (decompressed image)"), "got: {msg}");
}

// 이것을 실패시키는 것: 진입점 하나가 받은 예산 한도 대신 `MAX_TOTAL_DECOMPRESSED`
// 로 이미지 join 을 부르기 (진입점 넷 각각 따로 실행 — 그 진입점 단언이 `Ok` 로 실패).
#[test]
fn every_entry_point_charges_image_decompression_to_the_budget() {
    let bytes = std::fs::read(fixture_path("full_report.hwp")).expect("full_report fixture");
    let intermediate = decoder::decode_intermediate(&bytes).expect("fixture decodes");
    let (_, budget) =
        join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED).expect("images join");
    let open_used = decoder::package::PackageReader::open(&bytes)
        .expect("package opens")
        .remaining_budget()
        .used();
    let after_images = budget.used();
    assert!(after_images > open_used, "precondition: full_report decompresses images");
    let short = after_images - 1;

    let entry_points: [(&str, Box<dyn Fn(u64) -> Result<(), Hwp5Error>>); 4] = [
        (
            "Hwp5Decoder::decode",
            Box::new(|limit| Hwp5Decoder::decode_with_budget_limit(&bytes, limit).map(drop)),
        ),
        (
            "build_hwp5_semantic",
            Box::new(|limit| build_hwp5_semantic_with_budget_limit(&bytes, limit).map(drop)),
        ),
        (
            "decode_hwp5_with_images",
            Box::new(|limit| decode_hwp5_with_images_with_budget_limit(&bytes, limit).map(drop)),
        ),
        (
            "decode_hwp5_to_core",
            Box::new(|limit| decode_hwp5_to_core_with_budget_limit(&bytes, limit).map(drop)),
        ),
    ];
    for (name, decode) in entry_points {
        let msg = total_budget_error_message(decode(short));
        assert!(
            msg.contains("(decompressed image)"),
            "{name}: image join must hit the limit: {msg}"
        );
        decode(MAX_TOTAL_DECOMPRESSED).unwrap_or_else(|err| panic!("{name}: full budget: {err}"));
    }
}
