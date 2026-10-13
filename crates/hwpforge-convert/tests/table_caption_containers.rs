//! D review r1 HIGH-2: a captioned table inside a container — header,
//! footer, footnote, endnote, text box, table cell — converts with and
//! without the layout cache.
//!
//! The first captioned table of `sample-table-caption-fullsz` (Hancom) is
//! moved into the first paragraph of each container in a Hancom fixture:
//! its control goes in front of that paragraph's text and its records sit
//! one level below the paragraph. Before the fix, the container scopes'
//! layout hints left out the caption paragraph: the header, footer,
//! footnote, endnote and text box conversions failed with a hint count
//! underflow, with and without the cache (10 of the 12 cases, run on the
//! code before the fix). The cell case already converted — the table flow
//! collector counted its caption.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

const TAG_CTRL_HEADER: u16 = 0x47;
const TAG_PARA_HEADER: u16 = 0x42;
const TAG_PARA_TEXT: u16 = 0x43;
const TAG_LIST_HEADER: u16 = 0x48;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

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

fn compressed(streams: &BTreeMap<String, Vec<u8>>) -> bool {
    streams["/FileHeader"][36] & 1 != 0
}

fn section_records(streams: &BTreeMap<String, Vec<u8>>) -> Vec<Record> {
    let raw = &streams["/BodyText/Section0"];
    if compressed(streams) {
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(&raw[..]).read_to_end(&mut out).expect("inflate");
        parse_records(&out)
    } else {
        parse_records(raw)
    }
}

fn set_section(streams: &mut BTreeMap<String, Vec<u8>>, records: &[Record]) {
    let data = write_records(records);
    let data = if compressed(streams) {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data).expect("deflate");
        encoder.finish().expect("deflate")
    } else {
        data
    };
    streams.insert("/BodyText/Section0".to_string(), data);
}

fn ctrl_id(record: &Record) -> Option<[u8; 4]> {
    (record.tag == TAG_CTRL_HEADER && record.data.len() >= 4)
        .then(|| [record.data[3], record.data[2], record.data[1], record.data[0]])
}

/// `host` with the donor's first table moved into the first paragraph that
/// follows the first control `container` (a four-byte control id).
fn with_a_captioned_table(host: &str, container: [u8; 4]) -> Vec<u8> {
    let donor =
        section_records(&read_streams(&fixture("user_samples/sample-table-caption-fullsz.hwp")));
    let table_at = donor.iter().position(|r| ctrl_id(r) == Some(*b"tbl ")).expect("donor table");
    let table_level = donor[table_at].level;
    let table_end = donor[table_at + 1..]
        .iter()
        .position(|r| r.level <= table_level)
        .map_or(donor.len(), |n| table_at + 1 + n);
    assert_eq!(donor[table_at + 1].tag, TAG_LIST_HEADER, "the donor table has a caption");
    let host_text =
        donor[..table_at].iter().rev().find(|r| r.tag == TAG_PARA_TEXT).expect("donor host text");
    let table_control = host_text.data[..16].to_vec();

    let mut streams = read_streams(&fixture(host));
    let mut records = section_records(&streams);
    let ctrl_at = records.iter().position(|r| ctrl_id(r) == Some(container)).expect("container");
    let para_at = ctrl_at
        + records[ctrl_at..].iter().position(|r| r.tag == TAG_PARA_HEADER).expect("container para");
    let para_level = records[para_at].level;
    assert!(para_level > records[ctrl_at].level, "the paragraph is inside the container");
    let text_at = para_at + 1;
    assert_eq!(records[text_at].tag, TAG_PARA_TEXT);
    let para_end = para_at
        + 1
        + records[para_at + 1..].iter().position(|r| r.level <= para_level).expect("para end");

    // Paragraph header: 8 more characters, and the extended-control bit.
    let header = &mut records[para_at].data;
    let count = u32::from_le_bytes(header[0..4].try_into().unwrap());
    header[0..4].copy_from_slice(&(count + 8).to_le_bytes());
    let mask = u32::from_le_bytes(header[4..8].try_into().unwrap()) | 1 << 11;
    header[4..8].copy_from_slice(&mask.to_le_bytes());
    let mut text = table_control;
    text.extend_from_slice(&records[text_at].data);
    records[text_at].data = text;

    let shift = para_level + 1 - table_level;
    let moved: Vec<Record> = donor[table_at..table_end]
        .iter()
        .map(|r| Record { tag: r.tag, level: r.level + shift, data: r.data.clone() })
        .collect();
    records.splice(para_end..para_end, moved);
    set_section(&mut streams, &records);
    write_streams(&streams)
}

fn section0(hwpx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    xml
}

/// Each container keeps the moved table's caption — inside the container,
/// with its text, and with its line cache exactly when the cache is
/// carried — and both conversions succeed.
///
/// 이것을 실패시키는 것: 컨테이너 scope 수집기에서 캡션 문단 hint 를 빼기
/// (셀 밖 다섯 컨테이너가 carry 양쪽에서 hint 부족으로 변환 실패).
#[test]
fn captioned_table_in_every_container_converts_with_and_without_the_cache() {
    let cases: [(&str, &str, [u8; 4], &str, &str); 6] = [
        (
            "header",
            "mixed/mixed_02a_header_image_footer_text.hwp",
            *b"head",
            "<hp:header ",
            "</hp:header>",
        ),
        (
            "footer",
            "mixed/mixed_02a_header_image_footer_text.hwp",
            *b"foot",
            "<hp:footer ",
            "</hp:footer>",
        ),
        (
            "footnote",
            "hwp5/crossref/sample-footnote-number.hwp",
            *b"fn  ",
            "<hp:footNote",
            "</hp:footNote>",
        ),
        (
            "endnote",
            "hwp5/crossref/sample-endnote-page-and-number.hwp",
            *b"en  ",
            "<hp:endNote",
            "</hp:endNote>",
        ),
        ("text box", "images/textbox_anchored.hwp", *b"gso ", "<hp:drawText", "</hp:drawText>"),
        ("cell", "tables/table_05_cell_margin_padding.hwp", *b"tbl ", "<hp:tc", "</hp:tc>"),
    ];
    for (name, host, container, open, close) in cases {
        let input = with_a_captioned_table(host, container);
        for carry in [false, true] {
            let (hwpx, _) = hwp5_to_hwpx_bytes_with_diagnostics(
                &input,
                ConvertOptions::default().with_carry_layout_cache(carry),
            )
            .unwrap_or_else(|e| panic!("{name} carry={carry}: {e}"));
            let xml = section0(&hwpx);
            let start = xml.find(open).unwrap_or_else(|| panic!("{name}: no {open}"));
            let end = start + xml[start..].find(close).expect("container end");
            let inside = &xml[start..end];
            let caption_at = inside
                .find("<hp:caption ")
                .unwrap_or_else(|| panic!("{name} carry={carry}: no caption"));
            let caption_end =
                caption_at + inside[caption_at..].find("</hp:caption>").expect("caption end");
            let caption = &inside[caption_at..caption_end];
            assert!(caption.contains("여백 미포함 캡션"), "{name} carry={carry}: {caption}");
            assert_eq!(caption.contains("<hp:linesegarray>"), carry, "{name} carry={carry}");
        }
    }
}
