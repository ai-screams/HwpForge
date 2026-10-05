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
    let first_asset =
        image_assets.asset_for_reference(1).expect("full_report should expose binary image id 1");

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
        reference_ordinal: 1,
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

/// A group holding a text box with a table and a picture in its text: the
/// picture's own `ShapeComponent`/`ShapePicture` records and the table
/// cells' paragraphs sit deeper than the text box's own records. Read as
/// the text box's own, the picture made it a two-payload `Unknown` (the
/// whole text box dropped) and the cell text overwrote its paragraphs.
///
/// 이것을 실패시키는 것: 자식 도형 기록을 자기 수준으로 제한하지 않는 것(글상자가
/// 사라짐), 문단 기록을 자기 수준으로 제한하지 않는 것(표 칸 글자가 둘째 문단을
/// 덮음).
#[test]
fn grouped_text_box_keeps_its_own_records_only() {
    use hwpforge_core::control::Control;
    use hwpforge_core::run::RunContent;
    let bytes = std::fs::read(fixture_path("shapes/group-f2-textbox-table.hwp")).unwrap();
    let doc = decode_hwp5_with_images(&bytes).unwrap();
    let group = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| match &r.content {
            RunContent::Control(c) if matches!(**c, Control::Group { .. }) => Some((**c).clone()),
            _ => None,
        })
        .expect("a group");
    let Control::Group { children, .. } = &group else { unreachable!() };
    let texts: Vec<Vec<String>> = children
        .iter()
        .filter_map(|c| match c {
            hwpforge_core::control::GroupChild::Shape(Control::TextBox { paragraphs, .. }) => Some(
                paragraphs
                    .iter()
                    .map(|p| {
                        p.runs
                            .iter()
                            .filter_map(|r| match &r.content {
                                RunContent::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect()
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(texts, [["글상자 안 표", "", "글상자 안 그림", ""]]);
}

/// Hancom's object-link record (`CtrlData`) as saved under a logo picture.
const LINK_RECORD: &str = "1b02010000006f0200806f0201000000650201001b0068007400740070005c003a002f002f007700770077002e006b006f007200650061002e006b0072003b0031003b0030003b0030003b00";

/// One TLV record: header (tag, level, size) then the payload.
fn tlv(tag: u16, level: u16, data: &[u8]) -> Vec<u8> {
    let size = u32::try_from(data.len()).unwrap();
    let mut out = (u32::from(tag) | (u32::from(level) << 10) | (size << 20)).to_le_bytes().to_vec();
    out.extend_from_slice(data);
    out
}

/// Section 0's records of an HWP5 file, as `(tag, level, payload)`.
fn section0_records(bytes: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    use std::io::Read;
    let raw = read_cfb_stream(bytes, "/BodyText/Section0");
    let mut inflated = Vec::new();
    flate2::read::DeflateDecoder::new(&raw[..]).read_to_end(&mut inflated).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= inflated.len() {
        let h = u32::from_le_bytes(inflated[i..i + 4].try_into().unwrap());
        i += 4;
        let (tag, level) = ((h & 0x3ff) as u16, ((h >> 10) & 0x3ff) as u16);
        let mut size = (h >> 20) as usize;
        if size == 0xfff {
            size = u32::from_le_bytes(inflated[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
        }
        out.push((tag, level, inflated[i..i + size].to_vec()));
        i += size;
    }
    out
}

/// F3a with a link record added under the group's own component and under
/// the first picture's component, each one level below it.
fn f3a_with_links() -> Vec<u8> {
    let bytes = std::fs::read(fixture_path("shapes/group-f3a-logo.hwp")).unwrap();
    let link: Vec<u8> = (0..LINK_RECORD.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&LINK_RECORD[i..i + 2], 16).unwrap())
        .collect();
    let mut stream = Vec::new();
    let (mut group_done, mut pic_done) = (false, false);
    for (tag, level, data) in section0_records(&bytes) {
        stream.extend(tlv(tag, level, &data));
        if tag == 0x4C && !group_done && data.windows(4).any(|w| w == b"noc$") {
            stream.extend(tlv(0x57, level + 1, &link));
            group_done = true;
        } else if tag == 0x4C && group_done && !pic_done && data.windows(4).any(|w| w == b"cip$") {
            stream.extend(tlv(0x57, level + 1, &link));
            pic_done = true;
        }
    }
    assert!(group_done && pic_done, "F3a has a group of pictures");
    replace_cfb_stream(bytes, "/BodyText/Section0", &deflate(&stream))
}

/// A picture's link record inside a group is read into the picture's
/// hyperlink, and the group's own link record is reported as lost: the
/// group-child and group record paths of the decoder.
///
/// 이것을 실패시키는 것: 묶음 자식 빌더의 `CtrlData` 갈래 삭제(그림 링크가 사라진다) /
/// 묶음 빌더의 자기 수준 `CtrlData` 갈래 삭제(묶음 링크 경고가 사라진다).
#[test]
fn group_and_group_picture_links_are_read() {
    use hwpforge_core::control::{Control, GroupChild};
    use hwpforge_core::run::RunContent;
    let doc = decode_hwp5_with_images(&f3a_with_links()).unwrap();
    let lost: Vec<&str> = doc
        .warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ContentLost { kind, .. } => Some(kind.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lost, ["group_hyperlink"]);
    let group = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| match &r.content {
            RunContent::Control(c) if matches!(**c, Control::Group { .. }) => Some((**c).clone()),
            _ => None,
        })
        .expect("a group");
    let Control::Group { children, .. } = &group else { unreachable!() };
    let links: Vec<Option<&str>> = children
        .iter()
        .map(|c| match c {
            GroupChild::Picture(image) => image.hyperlink.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(links, [Some("http://www.korea.kr"), None, None]);
}

// ── BinData references are DocInfo positions (§19 C) ─────────────────

/// One level-0 `DocInfo` record: TLV header (tag, level 0, size) + payload.
fn doc_info_record(tag: u16, payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len()).expect("small record");
    assert!(size < 0xFFF, "test records stay under the extended-size marker");
    let header = u32::from(tag) | (size << 20);
    let mut out = header.to_le_bytes().to_vec();
    out.extend_from_slice(payload);
    out
}

/// An embedded `BIN_DATA` payload storing `id` and `extension`.
fn embedded_bin_data(id: u16, extension: &str) -> Vec<u8> {
    let mut out = 0x0001_u16.to_le_bytes().to_vec();
    out.extend_from_slice(&id.to_le_bytes());
    let units: Vec<u16> = extension.encode_utf16().collect();
    out.extend_from_slice(&u16::try_from(units.len()).expect("short").to_le_bytes());
    units.iter().for_each(|unit| out.extend_from_slice(&unit.to_le_bytes()));
    out
}

/// An external-link `BIN_DATA` payload (type 0): two length-prefixed
/// UTF-16 paths, no id or extension.
fn link_bin_data(absolute: &str, relative: &str) -> Vec<u8> {
    let mut out = 0x0000_u16.to_le_bytes().to_vec();
    for path in [absolute, relative] {
        let units: Vec<u16> = path.encode_utf16().collect();
        out.extend_from_slice(&u16::try_from(units.len()).expect("short").to_le_bytes());
        units.iter().for_each(|unit| out.extend_from_slice(&unit.to_le_bytes()));
    }
    out
}

fn reference_and_id(records: &[Hwp5BinDataRecordSummary]) -> Vec<(u16, u16)> {
    records.iter().map(|record| (record.reference_ordinal, record.binary_data_id)).collect()
}

/// C-T2: a `BIN_DATA` record that fails to parse still uses up its
/// position, so the records after it keep the reference numbers pictures use.
///
/// 이것을 실패시키는 것: 위치를 파싱 뒤에 매기는 것(`enumerate` 를
/// `filter_map` 뒤로) — 셋째 레코드가 순번 2 가 된다.
#[test]
fn a_bin_data_record_that_fails_to_parse_keeps_its_position() {
    let doc_info = [
        doc_info_record(0x12, &embedded_bin_data(1, "png")),
        doc_info_record(0x12, &[0x01, 0x00]),
        doc_info_record(0x12, &embedded_bin_data(2, "png")),
    ]
    .concat();
    let records = summarize_doc_info_bin_data_records(&doc_info, false).expect("summary");
    assert_eq!(reference_and_id(&records), [(1, 1), (3, 2)]);
}

/// C-T3 (record level): an external link stores two paths, not an id and
/// an extension. It takes its position but names no stream — before, the
/// absolute path's length was read as its id and the path's characters as
/// its extension.
///
/// 이것을 실패시키는 것: 링크 갈래를 지우는 것 — 경로 길이(10)가 id 가 되고
/// 저장 이름이 `BIN000A.…` 로 지어진다.
#[test]
fn an_external_link_takes_a_position_but_names_no_stream() {
    let doc_info = [
        doc_info_record(0x12, &link_bin_data("C:\\a\\b.jpg", "b.jpg")),
        doc_info_record(0x12, &embedded_bin_data(1, "png")),
    ]
    .concat();
    let records = summarize_doc_info_bin_data_records(&doc_info, false).expect("summary");
    assert_eq!(reference_and_id(&records), [(1, 0), (2, 1)]);
    assert_eq!(records[0].data_type, "Link");
    assert_eq!(records[0].storage_name, "");
    assert_eq!(records[0].extension, "");
    assert_eq!(records[1].storage_name, "BIN0001.png");
}

/// C-T1 (join level): image assets and the size hint taken from the
/// pictures are both keyed by reference. The F17 fixture's three records
/// are re-presented as stored ids `[3, 1, 2]`, so reference 1 is
/// `BIN0003.png`; each picture gets its own width.
///
/// 이것을 실패시키는 것: 자산 맵 키를 저장 id 로 되돌리는 것(참조 1 이
/// `BIN0001.png`), 크기 힌트를 저장 id 로 찾는 것(참조 1 의 힌트가 참조 3
/// 그림의 너비).
#[test]
fn image_assets_and_size_hints_are_keyed_by_reference() {
    let bytes = std::fs::read(fixture_path("images/f17-caption-numbering.hwp")).expect("fixture");
    let mut intermediate = crate::decoder::decode_intermediate(&bytes).expect("intermediate");
    let originals = intermediate.bin_data_records.clone();
    assert_eq!(reference_and_id(&originals), [(1, 1), (2, 2), (3, 3)], "fixture as made");
    intermediate.bin_data_records = [2, 0, 1]
        .into_iter()
        .zip(1..)
        .map(|(source, reference_ordinal)| Hwp5BinDataRecordSummary {
            reference_ordinal,
            ..originals[source].clone()
        })
        .collect();
    let mut width_of_reference = BTreeMap::new();
    let mut next_width = 1000_u32;
    for paragraph in &mut intermediate.sections[0].paragraphs {
        for control in &mut paragraph.controls {
            if let crate::decoder::section::Hwp5Control::Image(image) = control {
                image.geometry.width = next_width;
                width_of_reference.insert(image.binary_data_id, next_width);
                next_width += 1000;
            }
        }
    }
    assert_eq!(width_of_reference.len(), 3, "the three pictures reference 1, 2, 3");

    let plan =
        join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED).expect("join").0;
    for (reference, storage_name) in [(1, "BIN0003.png"), (2, "BIN0001.png"), (3, "BIN0002.png")] {
        let asset = plan.asset_for_reference(reference).expect("asset");
        assert_eq!(asset.payload.storage_name, storage_name, "reference {reference}");
        let width = i32::try_from(width_of_reference[&reference]).expect("small");
        assert_eq!(asset.payload.width_hwp, Some(width), "reference {reference}");
    }
}

fn summary(
    reference_ordinal: u16,
    binary_data_id: u16,
    storage_name: &str,
) -> Hwp5BinDataRecordSummary {
    Hwp5BinDataRecordSummary {
        binary_data_id,
        storage_name: storage_name.to_string(),
        extension: storage_name.rsplit_once('.').map_or("", |(_, ext)| ext).to_string(),
        data_type: if storage_name.is_empty() { "Link" } else { "Embedding" }.to_string(),
        compression: "Default".to_string(),
        should_decompress: false,
        reference_ordinal,
    }
}

/// C-T5 (plan level): the OLE reading check covers every `BIN_DATA` slot,
/// external links included, and spells out both readings.
///
/// 이것을 실패시키는 것: 비교를 로드된 OLE 자산으로만 좁히는 것(순번 3 의
/// 링크는 자산이 아니어서 빠진다), 외부 링크를 id 0 의 저장 레코드로 세는
/// 것(이유 문구가 바뀐다).
#[test]
fn ole_reference_ambiguity_covers_every_slot() {
    let records = [
        summary(1, 1, "BIN0001.png"),
        summary(2, 2, "BIN0002.png"),
        summary(3, 0, ""),
        summary(4, 3, "BIN0003.OLE"),
    ];
    let ambiguities = reference_ambiguities(&records);
    assert_eq!(
        ambiguities.get(&3).map(String::as_str),
        Some("ordinal 3 → external link, storage id 3 → BIN0003.OLE")
    );
    assert_eq!(
        ambiguities.get(&4).map(String::as_str),
        Some("ordinal 4 → BIN0003.OLE, storage id 4 → no parsed record")
    );
    assert!(!ambiguities.contains_key(&1) && !ambiguities.contains_key(&2), "{ambiguities:?}");
}

/// The OLE reading check indexes both readings once: 65,535 records (the
/// most a document can name) finish within half a second (about 40 ms
/// measured).
/// A per-reference scan of all records would make that billions of
/// comparisons.
///
/// 이것을 실패시키는 것: 참조마다 레코드 전체를 다시 훑는 것(두 `position()`
/// — 시간 한도를 넘는다).
#[test]
fn ole_reference_ambiguity_is_linear_in_the_records() {
    let records: Vec<Hwp5BinDataRecordSummary> =
        (1..=u16::MAX).map(|ordinal| summary(ordinal, 0, "")).collect();
    let started = std::time::Instant::now();
    let ambiguities = reference_ambiguities(&records);
    let elapsed = started.elapsed();
    assert_eq!(ambiguities.len(), usize::from(u16::MAX), "every link slot has no id reading");
    assert!(elapsed < std::time::Duration::from_millis(500), "took {elapsed:?}");
}
