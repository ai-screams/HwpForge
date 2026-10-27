//! Integration-style tests for the crate-root decode/census/image-join entry
//! points (`census_hwp5`, `join_hwp5_image_assets`, …).
//!
//! HWP5 → HWPX *conversion* tests live in the `hwpforge-convert` crate; this
//! module only exercises HWP5-native decode/census/join paths that touch this
//! crate's private modules.

use super::*;

pub(crate) fn asset_package(extensions: &[&str], chart: bool) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    let mut header = vec![0; 256];
    header[..18].copy_from_slice(b"HWP Document File\0");
    header[32..36].copy_from_slice(&0x05000205u32.to_le_bytes());
    header[36..40].copy_from_slice(&1u32.to_le_bytes());
    cfb.create_stream("/FileHeader").unwrap().write_all(&header).unwrap();
    cfb.create_storage("/BodyText").unwrap();
    cfb.create_storage("/BinData").unwrap();
    let mut info = [
        doc_info_record(0x15, &[0; 68]),
        doc_info_record(0x19, &[0; 42]),
        doc_info_record(0x1a, &[0; 14]),
    ]
    .concat();
    let mut section = Vec::new();
    for (i, ext) in extensions.iter().enumerate() {
        let id = (i + 1) as u16;
        info.extend(doc_info_record(0x12, &embedded_bin_data(id, ext)));
        let payload = if chart {
            decoder::chart_ole::tests::chart_fixture_inflated()
        } else {
            vec![0x55; 256]
        };
        cfb.create_stream(format!("/BinData/BIN{id:04X}.{ext}"))
            .unwrap()
            .write_all(&deflate(&payload))
            .unwrap();
        let wire = decoder::section::tests::r3_asset_wire(id);
        // Keep the picture or OLE control and its inline marker, respectively.
        let mut records = schema::record::Record::parse_stream(&mut Cursor::new(wire)).unwrap();
        let first = if chart { 2 } else { 5 };
        let controls = records[first..first + 3].to_vec();
        records.truncate(2);
        records[1].data.truncate(16);
        records.extend(controls);
        for r in records {
            let tag = u32::from(r.header.tag_id)
                | (u32::from(r.header.level) << 10)
                | ((r.data.len() as u32) << 20);
            section.extend(tag.to_le_bytes());
            section.extend(r.data);
        }
    }
    cfb.create_stream("/DocInfo").unwrap().write_all(&deflate(&info)).unwrap();
    cfb.create_stream("/BodyText/Section0").unwrap().write_all(&deflate(&section)).unwrap();
    cfb.into_inner().into_inner()
}

fn joined_asset_storage_names(plan: &Hwp5JoinedImageAssetPlan) -> Vec<String> {
    let mut names: Vec<String> =
        plan.ordered_assets.iter().map(|asset| asset.payload.storage_name.clone()).collect();
    names.sort();
    names
}

#[test]
// 이것을 실패시키는 것: joined 자산 누락 (실제 음성 대조 실행).
fn join_hwp5_image_assets_matches_fixture_bindata_inventory() {
    let cases: [&[&str]; 2] = [&["png"], &["png", "jpeg"]];
    for extensions in cases {
        let name = "synthetic asset package";
        let expected_storage_names: Vec<String> = extensions
            .iter()
            .enumerate()
            .map(|(i, ext)| format!("BIN{:04X}.{ext}", i + 1))
            .collect();
        let bytes = asset_package(extensions, false);
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

/// Direct unit coverage for `Hwp5StyleStore::border_fill_image_binary_ids`,
/// which is `pub(crate)` and therefore unreachable from the conversion tests
/// that moved to `hwpforge-convert`. It is consumed by
/// `supplement_border_fill_image_assets`; this asserts it collects the binary
/// data id of every image-fill border directly, restoring the assertion that
/// the move could only leave as a comment in the convert crate.
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

// ── chart and image decompression are bounded by the document budget ──

fn read_cfb_stream(bytes: &[u8], path: &str) -> Vec<u8> {
    use std::io::Read;
    let mut comp = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).expect("CFB opens");
    let mut stream = comp.open_stream(path).expect("stream exists");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("stream reads");
    buf
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
    asset_package(&["OLE"], true)
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

// 이것을 실패시키는 것 (각각 따로 실행):
// - OLE join 의 `budget.charge` 제거 → 첫 단(OLE inflate 몫 부족)이 `Ok` 가 된다.
// - projection 에 OLE join 이전 예산 사본 넘기기 → 둘째 단이 `Ok` 가 된다.
#[test]
// 이것을 실패시키는 것: OLE join 부족 예산 제거 (실제 음성 대조 실행).
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
// 이것을 실패시키는 것: 이미지 해제 부족 예산 제거 (실제 음성 대조 실행).
fn every_entry_point_charges_image_decompression_to_the_budget() {
    let bytes = asset_package(&["png"], false);
    let intermediate = decoder::decode_intermediate(&bytes).expect("fixture decodes");
    let (_, budget) =
        join_hwp5_image_assets(&bytes, &intermediate, MAX_TOTAL_DECOMPRESSED).expect("images join");
    let open_used = decoder::package::PackageReader::open(&bytes)
        .expect("package opens")
        .remaining_budget()
        .used();
    let after_images = budget.used();
    assert!(after_images > open_used, "precondition: synthetic package decompresses images");
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

// ── BinData references are DocInfo positions ─────────────────

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

/// A `BIN_DATA` record that fails to parse still uses up its
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

/// At record level, an external link stores two paths, not an id and
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

/// At join level, image assets and the size hint taken from the
/// pictures are both keyed by reference. The three synthetic records
/// are re-presented as stored ids `[3, 1, 2]`, so reference 1 is
/// `BIN0003.png`; each picture gets its own width.
///
/// 이것을 실패시키는 것: 자산 맵 키를 저장 id 로 되돌리는 것(참조 1 이
/// `BIN0001.png`), 크기 힌트를 저장 id 로 찾는 것(참조 1 의 힌트가 참조 3
/// 그림의 너비).
#[test]
// 이것을 실패시키는 것: 저장 ID 순열 제거 (실제 음성 대조 실행).
fn image_assets_and_size_hints_are_keyed_by_reference() {
    let bytes = asset_package(&["png", "png", "png"], false);
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

/// At plan level, the OLE reading check covers every `BIN_DATA` slot,
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

/// The OLE reading check indexes both readings once, so the records it
/// looks at grow with the number of records, not with its square: counted,
/// not timed (65,535 records — the most a document can name — are visited
/// once each).
///
/// 이것을 실패시키는 것: 참조마다 레코드 전체를 다시 훑는 것(방문 수가
/// 레코드 수의 제곱으로 는다).
#[test]
fn ole_reference_ambiguity_is_linear_in_the_records() {
    let visits_for = |count: u16| {
        let records: Vec<Hwp5BinDataRecordSummary> =
            (1..=count).map(|ordinal| summary(ordinal, 0, "")).collect();
        RECORD_VISITS.with(|visits| visits.set(0));
        let ambiguities = reference_ambiguities(&records);
        assert_eq!(ambiguities.len(), usize::from(count), "every link slot has no id reading");
        RECORD_VISITS.with(std::cell::Cell::get)
    };
    let count = u16::MAX;
    assert!(visits_for(count) <= 2 * u64::from(count), "{} visits", visits_for(count));
}

// 이것을 실패시키는 것: master geometry 누락 / 본문→master 단순 연결로 first-wins 순서 변경.
#[test]
// 이것을 실패시키는 것: master/body wire 순서 역전 (실제 음성 대조 실행).
fn master_image_geometry_keeps_wire_first_wins() {
    use crate::decoder::section::Hwp5Control;
    let bytes = asset_package(&["png"], false);
    let mut decoded = crate::decoder::decode_intermediate(&bytes).unwrap();
    let section = &mut decoded.sections[0];
    let Hwp5Control::Image(image) = section.paragraphs[0].controls[0].clone() else {
        panic!("image")
    };
    section.master_pages.push(crate::decoder::section::Hwp5MasterPage {
        paragraphs: vec![section.paragraphs[0].clone()],
        raw: vec![],
        extended: false,
        index: 0,
        ordinary_index: 0,
        record_index: 0,
        level: 2,
        section_properties: 0,
        body_paragraph_index: 0,
        body_control_index: 0,
    });
    let master = &mut section.master_pages[0];
    let mut body_image = image.clone();
    body_image.geometry.width = image.geometry.width + 100;
    section.paragraphs[0].controls = vec![Hwp5Control::Image(body_image.clone())];
    master.body_paragraph_index = 0;
    master.body_control_index = 0;
    assert_eq!(
        collect_image_geometry_hints(&decoded.sections)[&image.binary_data_id].width_hwp,
        image.geometry.width as i32
    );
    decoded.sections[0].master_pages[0].body_control_index = 1;
    assert_eq!(
        collect_image_geometry_hints(&decoded.sections)[&image.binary_data_id].width_hwp,
        body_image.geometry.width as i32
    );
}
