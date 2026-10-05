//! HWP5 BinData references count `DocInfo` `BIN_DATA` records (§19 C).
//!
//! A picture, border-fill or OLE record names its image by the 1-based
//! position of a `BIN_DATA` record in `DocInfo`, not by the id that record
//! stores. The two only differ when the stored ids are not `1..n` — which
//! Hancom never writes on save (it renumbers), so the fixtures here take a
//! Hancom file (F17: three pictures, x7: two pictures and a chart) and
//! rewrite its `DocInfo` and `/BinData` streams to the orders found in the
//! corpus. Every stream gets different bytes, so the converted package
//! shows which record each reference reached.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions, ConvertWarning};
use hwpforge_smithy_hwp5::Hwp5Warning;

const TAG_ID_MAPPINGS: u16 = 0x11;
const TAG_BIN_DATA: u16 = 0x12;
const TAG_BORDER_FILL: u16 = 0x14;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

// ── HWP5 rewriting ───────────────────────────────────────────────────

/// One `DocInfo` record.
struct Record {
    tag: u16,
    level: u16,
    data: Vec<u8>,
}

fn parse_records(stream: &[u8]) -> Vec<Record> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= stream.len() {
        let header = u32::from_le_bytes(stream[at..at + 4].try_into().expect("4 bytes"));
        at += 4;
        let mut size = (header >> 20) as usize;
        if size == 0xFFF {
            size = u32::from_le_bytes(stream[at..at + 4].try_into().expect("4 bytes")) as usize;
            at += 4;
        }
        let tag = (header & 0x3FF) as u16;
        let level = ((header >> 10) & 0x3FF) as u16;
        out.push(Record { tag, level, data: stream[at..at + size].to_vec() });
        at += size;
    }
    out
}

fn write_records(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let size = u32::try_from(record.data.len()).expect("record size");
        let head = u32::from(record.tag) | (u32::from(record.level) << 10);
        if size >= 0xFFF {
            out.extend_from_slice(&(head | (0xFFF << 20)).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
        } else {
            out.extend_from_slice(&(head | (size << 20)).to_le_bytes());
        }
        out.extend_from_slice(&record.data);
    }
    out
}

fn inflate(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out).expect("raw deflate");
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).expect("deflate");
    encoder.finish().expect("deflate")
}

/// The streams of an HWP5 file, by path (`/DocInfo`, `/BinData/BIN0001.png`).
fn read_streams(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes)).expect("cfb");
    let paths: Vec<PathBuf> =
        file.walk().filter(|e| e.is_stream()).map(|e| e.path().to_path_buf()).collect();
    paths
        .into_iter()
        .map(|path| {
            let mut data = Vec::new();
            file.open_stream(&path).expect("open").read_to_end(&mut data).expect("read");
            (path.to_string_lossy().into_owned(), data)
        })
        .collect()
}

fn write_streams(streams: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create");
    for (path, data) in streams {
        let path = Path::new(path);
        if let Some(parent) = path.parent().filter(|p| *p != Path::new("/")) {
            if !out.exists(parent) {
                out.create_storage_all(parent).expect("storage");
            }
        }
        out.create_stream(path).expect("stream").write_all(data).expect("write");
    }
    out.flush().expect("flush");
    out.into_inner().into_inner()
}

/// One `BIN_DATA` slot to write.
enum Slot {
    /// An embedded image or OLE entry storing `id`, its stream raw
    /// (`compression` bits as given).
    Embedded { property: u16, id: u16, extension: &'static str },
    /// An external link (type 0): two paths, no id, no stream.
    Link,
    /// A record too short to parse.
    Truncated,
}

fn slot_payload(slot: &Slot) -> Vec<u8> {
    let utf16 = |text: &str, out: &mut Vec<u8>| {
        let units: Vec<u16> = text.encode_utf16().collect();
        out.extend_from_slice(&u16::try_from(units.len()).expect("short").to_le_bytes());
        units.iter().for_each(|unit| out.extend_from_slice(&unit.to_le_bytes()));
    };
    match slot {
        Slot::Embedded { property, id, extension } => {
            let mut out = property.to_le_bytes().to_vec();
            out.extend_from_slice(&id.to_le_bytes());
            utf16(extension, &mut out);
            out
        }
        Slot::Link => {
            let mut out = 0_u16.to_le_bytes().to_vec();
            utf16("C:\\images\\linked.jpg", &mut out);
            utf16("linked.jpg", &mut out);
            out
        }
        Slot::Truncated => vec![0x01, 0x00],
    }
}

/// Embedded, stored without compression (bits 4–5 = 2), so a stream holds
/// the image bytes as they are.
const RAW_EMBEDDED: u16 = 0x0021;

/// `source` with its `BIN_DATA` records replaced by `slots`, its `/BinData`
/// streams replaced by `streams`, and `border_fill_refs` image border fills
/// appended after the existing ones. The `ID_MAPPINGS` counts follow.
fn rewrite(
    source: &[u8],
    slots: &[Slot],
    streams: &[(&str, Vec<u8>)],
    border_fill_refs: &[u16],
) -> Vec<u8> {
    let mut files = read_streams(source);
    let compressed = files["/FileHeader"][36] & 1 == 1;
    let doc_info = &files["/DocInfo"];
    let records = parse_records(&if compressed { inflate(doc_info) } else { doc_info.clone() });

    let mut out: Vec<Record> = Vec::new();
    let mut bin_data_written = false;
    let last_border_fill = records.iter().rposition(|r| r.tag == TAG_BORDER_FILL);
    for (index, record) in records.into_iter().enumerate() {
        match record.tag {
            TAG_ID_MAPPINGS => {
                let mut data = record.data;
                data[0..4].copy_from_slice(&u32::try_from(slots.len()).unwrap().to_le_bytes());
                let fills = u32::from_le_bytes(data[32..36].try_into().unwrap());
                let fills = fills + u32::try_from(border_fill_refs.len()).unwrap();
                data[32..36].copy_from_slice(&fills.to_le_bytes());
                out.push(Record { data, ..record });
            }
            TAG_BIN_DATA => {
                if !bin_data_written {
                    out.extend(slots.iter().map(|slot| Record {
                        tag: TAG_BIN_DATA,
                        level: record.level,
                        data: slot_payload(slot),
                    }));
                    bin_data_written = true;
                }
            }
            _ => {
                let level = record.level;
                out.push(record);
                if Some(index) == last_border_fill {
                    out.extend(border_fill_refs.iter().map(|reference| Record {
                        tag: TAG_BORDER_FILL,
                        level,
                        data: image_border_fill(*reference),
                    }));
                }
            }
        }
    }
    assert!(bin_data_written, "the source has BIN_DATA records to replace");

    let doc_info = write_records(&out);
    files.insert("/DocInfo".into(), if compressed { deflate(&doc_info) } else { doc_info });
    files.retain(|path, _| !path.starts_with("/BinData/"));
    for (name, bytes) in streams {
        files.insert(format!("/BinData/{name}"), bytes.clone());
    }
    write_streams(&files)
}

/// A `BORDER_FILL` record with no borders and an image fill (TOTAL) whose
/// BinData reference is `reference`.
fn image_border_fill(reference: u16) -> Vec<u8> {
    let mut data = vec![0_u8; 2 + 5 * 6]; // property + five empty border lines
    data.extend_from_slice(&2_u32.to_le_bytes()); // fill kind: image
    data.extend_from_slice(&[1, 0, 0, 0]); // mode TOTAL, brightness, contrast, effect
    data.extend_from_slice(&reference.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data
}

// ── reading the converted package ────────────────────────────────────

struct Package {
    entries: BTreeMap<String, Vec<u8>>,
}

impl Package {
    fn open(bytes: &[u8]) -> Self {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
        let entries = (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).expect("entry");
                let mut data = Vec::new();
                entry.read_to_end(&mut data).expect("read");
                (entry.name().to_string(), data)
            })
            .collect();
        Self { entries }
    }

    fn text(&self, name: &str) -> String {
        String::from_utf8(self.entries[name].clone()).expect("utf-8")
    }

    /// The `binaryItemIDRef` values of `name`, in document order.
    fn image_refs(&self, name: &str) -> Vec<String> {
        let text = self.text(name);
        text.match_indices("binaryItemIDRef=\"")
            .map(|(at, needle)| {
                let rest = &text[at + needle.len()..];
                rest[..rest.find('"').expect("closing quote")].to_string()
            })
            .collect()
    }

    /// The bytes of the BinData entry manifest id `id` names.
    fn image(&self, id: &str) -> &[u8] {
        let prefix = format!("BinData/{id}.");
        let (_, bytes) = self
            .entries
            .iter()
            .find(|(name, _)| name.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no entry for {id}: {:?}", self.entries.keys()));
        bytes
    }
}

fn convert(bytes: &[u8]) -> (Package, Vec<ConvertWarning>) {
    let (hwpx, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(bytes, ConvertOptions::default()).expect("convert");
    (Package::open(&hwpx), warnings)
}

fn dropped_image_reasons(warnings: &[ConvertWarning]) -> Vec<&str> {
    warnings
        .iter()
        .filter_map(|w| match w {
            ConvertWarning::Hwp5(Hwp5Warning::DroppedControl { control: "image", reason }) => {
                Some(reason.as_str())
            }
            _ => None,
        })
        .collect()
}

fn fallback_reasons<'w>(warnings: &'w [ConvertWarning], subject: &str) -> Vec<&'w str> {
    warnings
        .iter()
        .filter_map(|w| match w {
            ConvertWarning::Hwp5(Hwp5Warning::ProjectionFallback { subject: s, reason })
                if *s == subject =>
            {
                Some(reason.as_str())
            }
            _ => None,
        })
        .collect()
}

/// Five distinct image payloads: F17's three logos (inflated) and two more
/// made from the first one with different trailing bytes.
fn five_images() -> Vec<Vec<u8>> {
    let streams = read_streams(&fixture("images/f17-caption-numbering.hwp"));
    let mut images: Vec<Vec<u8>> =
        (1..=3).map(|n| inflate(&streams[&format!("/BinData/BIN000{n}.png")])).collect();
    for tail in [b"four".as_slice(), b"five".as_slice()] {
        images.push([images[0].as_slice(), tail].concat());
    }
    for (i, a) in images.iter().enumerate() {
        assert!(images[i + 1..].iter().all(|b| a != b), "image {i} is not unique");
    }
    images
}

// ── the tests ────────────────────────────────────────────────────────

/// C-T1 + C-T4: stored ids `[6, 1, 2, 3, 4]` (the corpus's out-of-order
/// form). Pictures reference 1, 2, 3 → the records at those positions,
/// stored as 6, 1, 2. Border fills reference 5 and 4 → stored 4 and 3. Each
/// written image is byte-for-byte the stream of the record it reached.
///
/// 이것을 실패시키는 것: 이미지 자산 맵 키를 저장 id 로 되돌리는 것(그림이
/// BIN0001·2·3 의 바이트), convert 가 테두리 채우기를 다시 `BIN{참조}` 로
/// 쓰는 것(BIN0005·BIN0004 — 앞의 것은 자산이 없어 채우기가 빠진다),
/// 변환 경로가 해석 표를 저장 이름이 아니라 순번 문자열로 만드는 것.
#[test]
fn references_reach_the_record_at_their_position() {
    let images = five_images();
    let ids = [6_u16, 1, 2, 3, 4];
    let slots: Vec<Slot> = ids
        .iter()
        .map(|&id| Slot::Embedded { property: RAW_EMBEDDED, id, extension: "png" })
        .collect();
    let names: Vec<String> = ids.iter().map(|id| format!("BIN{id:04X}.png")).collect();
    let streams: Vec<(&str, Vec<u8>)> =
        names.iter().map(String::as_str).zip(images.iter().cloned()).collect();
    let bytes = rewrite(&fixture("images/f17-caption-numbering.hwp"), &slots, &streams, &[5, 4]);
    let bytes_of = |id: u16| &images[ids.iter().position(|&x| x == id).expect("stored id")];

    let (package, warnings) = convert(&bytes);

    let pictures = package.image_refs("Contents/section0.xml");
    assert_eq!(pictures, ["BIN0006", "BIN0001", "BIN0002"], "{warnings:?}");
    for (id, stored) in pictures.iter().zip([6, 1, 2]) {
        assert_eq!(package.image(id), bytes_of(stored).as_slice(), "picture {id}");
    }
    let fills = package.image_refs("Contents/header.xml");
    assert_eq!(fills, ["BIN0004", "BIN0003"], "{warnings:?}");
    for (id, stored) in fills.iter().zip([4, 3]) {
        assert_eq!(package.image(id), bytes_of(stored).as_slice(), "border fill {id}");
    }
    assert!(dropped_image_reasons(&warnings).is_empty(), "{warnings:?}");
    assert!(fallback_reasons(&warnings, "style.border_fill.image").is_empty(), "{warnings:?}");
}

/// C-T3: the first record is an external link and the second stores an
/// extension that is not an image this converter carries. Each takes its
/// reference, and a picture or border fill that names one is dropped with
/// that cause — not as a missing asset. Reference 3 reaches the third
/// record.
///
/// 이것을 실패시키는 것: 링크·확장자 이유 문구를 지우는 것(옛 "missing"
/// 이유로 돌아간다), 링크 갈래를 지워 경로 길이를 id 로 읽는 것, 테두리
/// 채우기 보충이 고정 "missing" 이유를 쓰는 것, 저장 이름 표에 링크를 빈
/// 이름으로 넣는 것.
#[test]
fn a_reference_to_a_link_or_an_unsupported_entry_says_why() {
    let images = five_images();
    let slots = [
        Slot::Link,
        Slot::Embedded { property: RAW_EMBEDDED, id: 1, extension: "ai" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 2, extension: "png" },
    ];
    let streams = [("BIN0001.ai", images[0].clone()), ("BIN0002.png", images[1].clone())];
    let bytes = rewrite(&fixture("images/f17-caption-numbering.hwp"), &slots, &streams, &[1]);
    let decoded = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).expect("decode");
    let names: Vec<(u16, &str)> =
        decoded.bin_data_reference_names.iter().map(|(r, n)| (*r, n.as_str())).collect();
    assert_eq!(names, [(2, "BIN0001.ai"), (3, "BIN0002.png")], "the link has no name");

    let (package, warnings) = convert(&bytes);

    assert_eq!(
        dropped_image_reasons(&warnings),
        [
            "bin_data_reference=1 is an external link (not carried)",
            "bin_data_reference=2 extension 'ai' is not a supported image",
        ]
    );
    assert_eq!(
        fallback_reasons(&warnings, "style.border_fill.image"),
        ["bin_data_reference=1 is an external link (not carried)"]
    );
    assert_eq!(package.image_refs("Contents/section0.xml"), ["BIN0002"]);
    assert_eq!(package.image("BIN0002"), images[1].as_slice());
    assert!(package.image_refs("Contents/header.xml").is_empty(), "the link fill is left out");
}

/// The same causes reach a picture inside a group (F3a: three pictures in
/// one group, referencing 1, 2, 3).
///
/// 이것을 실패시키는 것: 묶음 계획기가 고정 "missing" 이유를 쓰는 것, 묶음
/// 그림의 자산을 저장 id 로 찾는 것(둘째 그림이 BIN0002 대신 BIN0001).
#[test]
fn a_group_picture_reference_reaches_its_position_and_says_why() {
    let images = five_images();
    let slots = [
        Slot::Link,
        Slot::Embedded { property: RAW_EMBEDDED, id: 2, extension: "png" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 1, extension: "png" },
    ];
    let streams = [("BIN0001.png", images[0].clone()), ("BIN0002.png", images[1].clone())];
    let bytes = rewrite(&fixture("shapes/group-f3a-logo.hwp"), &slots, &streams, &[]);

    let (package, warnings) = convert(&bytes);

    assert_eq!(
        dropped_image_reasons(&warnings),
        ["bin_data_reference=1 is an external link (not carried)"]
    );
    let pictures = package.image_refs("Contents/section0.xml");
    assert_eq!(pictures, ["BIN0002", "BIN0001"], "{warnings:?}");
    assert_eq!(package.image("BIN0002"), images[1].as_slice());
}

/// C-T2, end to end: the second record cannot be parsed. It still takes
/// reference 2, so the third picture reaches the third record.
///
/// 이것을 실패시키는 것: 순번을 파싱 뒤에 매기는 것(셋째 그림이 BIN0003).
#[test]
fn a_record_that_fails_to_parse_keeps_its_reference() {
    let images = five_images();
    let slots = [
        Slot::Embedded { property: RAW_EMBEDDED, id: 1, extension: "png" },
        Slot::Truncated,
        Slot::Embedded { property: RAW_EMBEDDED, id: 2, extension: "png" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 3, extension: "png" },
    ];
    let streams = [
        ("BIN0001.png", images[0].clone()),
        ("BIN0002.png", images[1].clone()),
        ("BIN0003.png", images[2].clone()),
    ];
    let bytes = rewrite(&fixture("images/f17-caption-numbering.hwp"), &slots, &streams, &[]);

    let (package, warnings) = convert(&bytes);

    assert_eq!(dropped_image_reasons(&warnings), ["missing_image_asset_for_binary_data_id=2"]);
    assert_eq!(package.image_refs("Contents/section0.xml"), ["BIN0001", "BIN0002"]);
    assert_eq!(package.image("BIN0002"), images[1].as_slice());
}

/// x7 (two pictures and a chart) with its OLE record moved: `slots` gives
/// the records, the chart's `CtrlHeader` keeps reference 3.
fn x7_with(slots: &[Slot], ole_name: &str) -> Vec<u8> {
    let source = fixture("charts/x7-ole-ordinal.hwp");
    let streams = read_streams(&source);
    let ole = streams["/BinData/BIN0003.OLE"].clone();
    let png1 = streams["/BinData/BIN0001.png"].clone();
    let png2 = streams["/BinData/BIN0002.png"].clone();
    let png3 = [inflate(&png1), b"three".to_vec()].concat();
    let streams = [
        ("BIN0001.png", inflate(&png1)),
        ("BIN0002.png", inflate(&png2)),
        ("BIN0003.png", png3),
        (ole_name, ole),
    ];
    rewrite(&source, slots, &streams, &[])
}

/// x7 stores its OLE entry as `Storage` (type 2), default compression.
const OLE_STORAGE: u16 = 0x0002;

/// C-T5: the chart names reference 3. By position that is the OLE record
/// (stored id 4); by stored id it would be a picture. The chart is read by
/// position, and the disagreement is reported.
///
/// 이것을 실패시키는 것: OLE 맵을 저장 id 로 키잉하는 것(차트가 그림 3 을
/// 찾아 빠진다), 두 해석이 다를 때의 경고 갈래를 지우는 것.
#[test]
fn an_ole_reference_is_read_by_position_and_a_disagreement_is_reported() {
    let slots = [
        Slot::Embedded { property: RAW_EMBEDDED, id: 1, extension: "png" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 2, extension: "png" },
        Slot::Embedded { property: OLE_STORAGE, id: 4, extension: "OLE" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 3, extension: "png" },
    ];
    let (package, warnings) = convert(&x7_with(&slots, "BIN0004.OLE"));

    assert!(package.text("Contents/section0.xml").contains("<hp:chart"), "{warnings:?}");
    assert_eq!(
        fallback_reasons(&warnings, "ole.bin_data_reference"),
        ["ordinal 3 → BIN0004.OLE, storage id 3 → BIN0003.png"]
    );
}

/// C-T5, the other way: by position reference 3 is an external link, by
/// stored id it would be the OLE entry. Position wins (the chart is
/// dropped) and both readings are reported.
///
/// 이것을 실패시키는 것: 비교를 로드된 OLE 자산으로만 좁히는 것(링크 슬롯은
/// 자산이 아니어서 경고가 사라진다), OLE 맵을 저장 id 로 키잉하는 것(차트가
/// 살아난다).
#[test]
fn an_ole_reference_to_a_link_slot_is_reported_with_both_readings() {
    let slots = [
        Slot::Embedded { property: RAW_EMBEDDED, id: 1, extension: "png" },
        Slot::Embedded { property: RAW_EMBEDDED, id: 2, extension: "png" },
        Slot::Link,
        Slot::Embedded { property: OLE_STORAGE, id: 3, extension: "OLE" },
    ];
    let (package, warnings) = convert(&x7_with(&slots, "BIN0003.OLE"));

    assert!(!package.text("Contents/section0.xml").contains("<hp:chart"), "{warnings:?}");
    assert_eq!(
        fallback_reasons(&warnings, "ole.bin_data_reference"),
        ["ordinal 3 → external link, storage id 3 → BIN0003.OLE"]
    );
}

/// The unmodified Hancom files: ids are `1..n`, so both readings agree and
/// nothing is reported.
///
/// 이것을 실패시키는 것: 두 해석이 같을 때도 경고하는 것.
#[test]
fn hancom_files_with_ids_in_order_report_nothing() {
    for name in ["images/f17-caption-numbering.hwp", "charts/x7-ole-ordinal.hwp"] {
        let (_, warnings) = convert(&fixture(name));
        assert!(fallback_reasons(&warnings, "ole.bin_data_reference").is_empty(), "{name}");
        assert!(dropped_image_reasons(&warnings).is_empty(), "{name}: {warnings:?}");
    }
}
