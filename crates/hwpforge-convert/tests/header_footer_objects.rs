//! A table inside a cell's footer (§19 F-T7, review r1 of §19.7 F).
//!
//! x4c (Hancom's footer in a spanning cell) is rewritten so the footer
//! paragraph also holds a two-cell table copied from `table_05`: the table
//! control goes into the footer paragraph's text and its records sit one
//! level below that paragraph. Before, the decoder read the table's cell
//! paragraphs as more footer paragraphs and dropped the table.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::HwpxDecoder;

const TAG_CTRL_HEADER: u16 = 0x47;
const TAG_PARA_HEADER: u16 = 0x42;
const TAG_PARA_TEXT: u16 = 0x43;

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

/// x4c with `table_05`'s table moved into the cell footer's paragraph.
fn x4c_with_a_footer_table() -> Vec<u8> {
    let donor = section_records(&read_streams(&fixture("tables/table_05_cell_margin_padding.hwp")));
    let table_at = donor.iter().position(|r| ctrl_id(r) == Some(*b"tbl ")).expect("donor table");
    let table_level = donor[table_at].level;
    let table_end = donor[table_at + 1..]
        .iter()
        .position(|r| r.level <= table_level)
        .map_or(donor.len(), |n| table_at + 1 + n);
    // The host paragraph's text: the 8-unit table control, then the CR.
    let host_text =
        donor[..table_at].iter().rev().find(|r| r.tag == TAG_PARA_TEXT).expect("donor host text");
    let table_control = host_text.data[..16].to_vec();

    let mut streams = read_streams(&fixture("header_footer/x4c-footer-in-spanning-cell.hwp"));
    let mut records = section_records(&streams);
    let footer_at = records.iter().position(|r| ctrl_id(r) == Some(*b"foot")).expect("footer");
    let para_at = footer_at
        + records[footer_at..].iter().position(|r| r.tag == TAG_PARA_HEADER).expect("footer para");
    let para_level = records[para_at].level;
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

/// The first table's `<hp:sz …/>` element.
fn table_size(xml: &str) -> String {
    let at = xml.find("<hp:tbl ").expect("table");
    let sz = at + xml[at..].find("<hp:sz ").expect("sz");
    xml[sz..sz + xml[sz..].find("/>").expect("end")].to_string()
}

fn section0(hwpx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    xml
}

/// The footer keeps its table — two cells, the footer text intact, the
/// body's tables unchanged — and the conversion succeeds with and without
/// the layout cache (the footer table takes the footer's table hint; with
/// the cache its cells carry their line segments).
///
/// 이것을 실패시키는 것: 칸 꼬리말 수집기의 `tbl` 갈래를 지우기(표가 사라지고
/// 칸 글이 꼬리말 문단으로) / 끝난 표를 칸 문단에 붙이기(본문 표 수가 는다) /
/// hint 수집에서 꼬리말 문단의 표를 빼기(캐시 켠 변환이 hint 부족으로 실패).
#[test]
fn cell_footer_table_survives_with_and_without_the_layout_cache() {
    let input = x4c_with_a_footer_table();
    let (plain, _) = hwp5_to_hwpx_bytes_with_diagnostics(
        &fixture("header_footer/x4c-footer-in-spanning-cell.hwp"),
        ConvertOptions::default(),
    )
    .expect("plain x4c");
    let plain_tables = section0(&plain).matches("<hp:tbl ").count();
    // The donor table converted on its own: its height is the one the
    // footer table must keep.
    let (donor, _) = hwp5_to_hwpx_bytes_with_diagnostics(
        &fixture("tables/table_05_cell_margin_padding.hwp"),
        ConvertOptions::default().with_carry_layout_cache(true),
    )
    .expect("donor");
    let donor_size = table_size(&section0(&donor));
    for carry in [false, true] {
        let (hwpx, _) = hwp5_to_hwpx_bytes_with_diagnostics(
            &input,
            ConvertOptions::default().with_carry_layout_cache(carry),
        )
        .unwrap_or_else(|e| panic!("carry={carry}: {e}"));
        let xml = section0(&hwpx);
        let start = xml.find("<hp:footer ").expect("footer");
        let end = xml.find("</hp:footer>").expect("footer end");
        let footer = &xml[start..end];
        assert_eq!(footer.matches("<hp:tbl ").count(), 1, "carry={carry}");
        assert_eq!(xml.matches("<hp:tbl ").count(), plain_tables + 1, "carry={carry}");
        assert_eq!(footer.contains("<hp:linesegarray>"), carry, "carry={carry}");
        assert_eq!(table_size(footer), donor_size, "carry={carry}");

        let doc = HwpxDecoder::decode(&hwpx).expect("decode");
        let section = &doc.document.sections()[0];
        let footer = &section.footers[0];
        assert_eq!(footer.paragraphs.len(), 1, "carry={carry}");
        let tables: Vec<_> = footer.paragraphs[0]
            .runs
            .iter()
            .filter_map(|r| match &r.content {
                RunContent::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1, "carry={carry}");
        assert_eq!(tables[0].rows.iter().map(|r| r.cells.len()).sum::<usize>(), 2);
        assert!(footer.paragraphs[0].text_content().contains("꼬리말 D"));
    }
}
