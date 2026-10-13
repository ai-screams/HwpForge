//! Line boundaries through both PDF paths (S22 gate G5).
//!
//! A table host whose first line holds only zero-width controls (section and
//! column definition, page-number position) and whose second line starts at
//! the table is the shape the line boundary exists for: both line starts sit
//! at Core offset 0, so only the boundary — the tokens a line passes before
//! it starts — tells the encoder where the second line begins.
//!
//! No checked-in fixture has that shape, so both cases are built here from
//! `sample-table-caption-fullsz.hwp` (an empty first paragraph, then a table
//! host one line below):
//!
//! - **restorable** merges the two paragraphs into one host holding
//!   `secd · cold · pgnp · tbl`, keeping both lines' cached positions. The
//!   encoder writes `secPr · colPr · pageNum` ahead of the table in that
//!   order, so the second line is placed after them.
//! - **unrestorable** leaves the first paragraph alone and puts `pgnp` ahead
//!   of the table in the host. A page-number position outside a section's
//!   first paragraph is consumed, not written, so the boundary names a token
//!   the output does not have and the paragraph's cache is dropped.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions, ConvertWarning};
use hwpforge_smithy_pdf::PdfErrorCode;

const TAG_PARA_HEADER: u16 = 0x42;
const TAG_PARA_TEXT: u16 = 0x43;
const TAG_PARA_LINE_SEG: u16 = 0x45;
const TAG_CTRL_HEADER: u16 = 0x47;

/// The `pgnp` control of `sample-field-page-number-basic.hwp`: id, position
/// attribute `0x500`, no user/prefix/suffix character, dash `-`.
const PGNP_CTRL_HEADER: [u8; 16] = [
    0x70, 0x6E, 0x67, 0x70, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2D, 0x00,
];

/// The face `sample-table-caption-fullsz.hwp` asks for (its only `<hh:font>`).
const FIXTURE_FACE: &str = "함초롬돋움";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[derive(Clone)]
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

/// One extended control character in `PARA_TEXT`: code, id, padding, code.
fn ctrl_char(code: u16, id: [u8; 4]) -> Vec<u8> {
    let mut out = code.to_le_bytes().to_vec();
    out.extend(id.iter().rev());
    out.extend([0; 8]);
    out.extend(code.to_le_bytes());
    out
}

/// `PARA_TEXT` of the given extended controls and the paragraph end.
fn para_text(controls: &[(u16, [u8; 4])]) -> Vec<u8> {
    let mut out: Vec<u8> = controls.iter().flat_map(|&(code, id)| ctrl_char(code, id)).collect();
    out.extend(0x0Du16.to_le_bytes());
    out
}

/// The paragraph header with its character count, control mask and line
/// count replaced (the last-paragraph flag in the count is kept).
fn para_header(base: &Record, chars: u32, mask: u32, lines: u16) -> Record {
    let mut data = base.data.clone();
    let last = u32::from_le_bytes(data[0..4].try_into().unwrap()) & 0x8000_0000;
    data[0..4].copy_from_slice(&(chars | last).to_le_bytes());
    data[4..8].copy_from_slice(&mask.to_le_bytes());
    data[16..18].copy_from_slice(&lines.to_le_bytes());
    Record { tag: TAG_PARA_HEADER, level: base.level, data }
}

/// A one-line `PARA_LINE_SEG` record's line with its text position replaced.
fn line_at(record: &Record, textpos: u32) -> Vec<u8> {
    assert_eq!(record.tag, TAG_PARA_LINE_SEG);
    assert_eq!(record.data.len(), 36, "one line");
    let mut line = record.data.clone();
    line[0..4].copy_from_slice(&textpos.to_le_bytes());
    line
}

const SECD: (u16, [u8; 4]) = (2, *b"secd");
const COLD: (u16, [u8; 4]) = (2, *b"cold");
const PGNP: (u16, [u8; 4]) = (0x15, *b"pgnp");
const TBL: (u16, [u8; 4]) = (11, *b"tbl ");

/// The fixture's records split at its first two paragraphs.
struct Fullsz {
    streams: BTreeMap<String, Vec<u8>>,
    records: Vec<Record>,
    /// Index of the second paragraph's header (the table host).
    host: usize,
    /// Index of the third paragraph's header (after the table).
    after: usize,
}

fn fullsz() -> Fullsz {
    let streams = read_streams(&fixture("user_samples/sample-table-caption-fullsz.hwp"));
    let records = section_records(&streams);
    let tops: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.tag == TAG_PARA_HEADER && r.level == 0)
        .map(|(i, _)| i)
        .collect();
    let (host, after) = (tops[1], tops[2]);
    // The shape the construction relies on, read from the fixture.
    assert_eq!(records[1].tag, TAG_PARA_TEXT);
    assert_eq!(records[1].data, para_text(&[SECD, COLD]), "first paragraph = secd · cold");
    assert_eq!(records[host + 1].data, para_text(&[TBL]), "second paragraph = the table");
    assert_eq!(ctrl_id(&records[host + 4]), Some(*b"tbl "));
    Fullsz { streams, records, host, after }
}

fn pgnp_record(level: u16) -> Record {
    Record { tag: TAG_CTRL_HEADER, level, data: PGNP_CTRL_HEADER.to_vec() }
}

/// One host `secd · cold · pgnp · tbl`: line 0 at text position 0 (the first
/// paragraph's cached line), line 1 at the table (wire 24, the second
/// paragraph's cached line).
/// Also returns the two source lines, each as its nine `PARA_LINE_SEG`
/// fields in order.
fn restorable() -> (Vec<u8>, Vec<[i64; 9]>) {
    let Fullsz { mut streams, records, host, after } = fullsz();
    let first = &records[..host];
    let mask = 1 << 2 | 1 << 11 | 1 << 21;
    let mut lines = line_at(&first[3], 0);
    lines.extend(line_at(&records[host + 3], 24));
    let mut out = vec![
        para_header(&first[0], 33, mask, 2),
        Record { data: para_text(&[SECD, COLD, PGNP, TBL]), ..first[1].clone() },
        first[2].clone(),
        Record { data: lines, ..first[3].clone() },
    ];
    out.extend(first[4..].iter().cloned()); // secd and cold
    out.push(pgnp_record(first[4].level));
    out.extend(records[host + 4..after].iter().cloned()); // the table
    out.extend(records[after..].iter().cloned());
    let source = out[3]
        .data
        .chunks(36)
        .map(|line| {
            let field = |i: usize| -> [u8; 4] { line[4 * i..4 * i + 4].try_into().unwrap() };
            let mut fields = [0i64; 9];
            for (i, value) in fields.iter_mut().enumerate() {
                // textpos and flags are unsigned, the rest signed.
                *value = if i == 0 || i == 8 {
                    i64::from(u32::from_le_bytes(field(i)))
                } else {
                    i64::from(i32::from_le_bytes(field(i)))
                };
            }
            fields
        })
        .collect();
    set_section(&mut streams, &out);
    (write_streams(&streams), source)
}

/// The first paragraph as is; the host becomes `pgnp · tbl` with line 1 at
/// the table (wire 8).
fn unrestorable() -> Vec<u8> {
    let Fullsz { mut streams, records, host, after } = fullsz();
    let seg = &records[host + 3];
    let mut lines = line_at(seg, 0);
    lines.extend(line_at(seg, 8));
    let mut out: Vec<Record> = records[..host].to_vec();
    out.extend([
        para_header(&records[host], 17, 1 << 11 | 1 << 21, 2),
        Record { data: para_text(&[PGNP, TBL]), ..records[host + 1].clone() },
        records[host + 2].clone(),
        Record { data: lines, ..seg.clone() },
        pgnp_record(records[host + 4].level),
    ]);
    out.extend(records[host + 4..after].iter().cloned());
    out.extend(records[after..].iter().cloned());
    set_section(&mut streams, &out);
    write_streams(&streams)
}

/// The cache-drop warnings of a default conversion, as debug text.
fn cache_drops(warnings: &[ConvertWarning]) -> Vec<String> {
    warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .filter(|text| text.contains("LayoutCacheDropped"))
        .collect()
}

/// Each `<hp:lineseg>` of the first top-level paragraph, as its nine
/// attributes in `PARA_LINE_SEG` order.
fn first_paragraph_lines(hwpx: &[u8]) -> Vec<[i64; 9]> {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    // The paragraph holds a table whose cells hold paragraphs, so find its
    // own end by depth and take the last cache before it.
    let start = xml.find("<hp:p ").expect("a paragraph");
    let (mut depth, mut at) = (0usize, start);
    let end = loop {
        let open = xml[at..].find("<hp:p ").map(|n| at + n);
        let close = at + xml[at..].find("</hp:p>").expect("a closed paragraph");
        match open {
            Some(open) if open < close => {
                depth += 1;
                at = open + 1;
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    break close;
                }
                at = close + 1;
            }
        }
    };
    let cache = start + xml[start..end].rfind("<hp:linesegarray>").expect("a cache");
    xml[cache..end]
        .split("<hp:lineseg ")
        .skip(1)
        .map(|seg| {
            let seg = format!(" {seg}");
            let names = [
                "textpos",
                "vertpos",
                "vertsize",
                "textheight",
                "baseline",
                "spacing",
                "horzpos",
                "horzsize",
                "flags",
            ];
            names.map(|name| {
                let key = format!(" {name}=\"");
                let at = seg.find(&key).unwrap_or_else(|| panic!("{name}")) + key.len();
                seg[at..at + seg[at..].find('"').unwrap()].parse().expect("a number")
            })
        })
        .collect()
}

// ── fonts (see `ops_to_pdf.rs` for why a synthetic face is renamed) ──

fn with_declared_face(font: &[u8], face: &str) -> Vec<u8> {
    fn utf16be(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }
    let entries: [(u16, Vec<u8>); 3] =
        [(1, utf16be(face)), (2, utf16be("Regular")), (4, utf16be(face))];
    let count = u16::try_from(entries.len()).expect("three records");
    let mut records = Vec::new();
    let mut storage = Vec::new();
    for (name_id, text) in &entries {
        records.extend_from_slice(&3u16.to_be_bytes());
        records.extend_from_slice(&1u16.to_be_bytes());
        records.extend_from_slice(&0x0409u16.to_be_bytes());
        records.extend_from_slice(&name_id.to_be_bytes());
        records.extend_from_slice(&u16::try_from(text.len()).expect("short name").to_be_bytes());
        records
            .extend_from_slice(&u16::try_from(storage.len()).expect("short table").to_be_bytes());
        storage.extend_from_slice(text);
    }
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&count.to_be_bytes());
    table.extend_from_slice(&(6 + 12 * count).to_be_bytes());
    table.extend_from_slice(&records);
    table.extend_from_slice(&storage);

    let num_tables = usize::from(u16::from_be_bytes([font[4], font[5]]));
    let mut out = font.to_vec();
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let offset = u32::try_from(out.len()).expect("font is small");
    let length = u32::try_from(table.len()).expect("table is small");
    out.extend_from_slice(&table);
    let mut repointed = false;
    for index in 0..num_tables {
        let record = 12 + 16 * index;
        if &font[record..record + 4] == b"name" {
            out[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
            out[record + 12..record + 16].copy_from_slice(&length.to_be_bytes());
            repointed = true;
        }
    }
    assert!(repointed, "the committed synthetic face has a name table");
    out
}

fn pdf_options() -> ToPdfOptions {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("line-boundary-face");
    std::fs::create_dir_all(&dir).expect("create the font directory");
    let source = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hwpforge-smithy-pdf/tests/fonts/HwpForgeTest-Regular.ttf"),
    )
    .expect("the committed synthetic face");
    std::fs::write(dir.join("fixture-face.ttf"), with_declared_face(&source, FIXTURE_FACE))
        .expect("write the renamed face");
    ToPdfOptions::default()
        .with_font_dirs(vec![dir])
        .with_degraded(true)
        .with_partial_cache_reject(true)
}

/// G5 (1): the boundary restores the host's second line, nothing is dropped,
/// and the HWP5 and HWPX paths reach the same PDF outcome.
///
/// The outcome is not yet a PDF: the renderer places a table at its host's
/// first line, so a table on the host's second line fails table pagination
/// (`InvalidCache`, off by that line's 1,600 HU). The geometry is the usual
/// one, not an artefact of the construction: in the converted frozen corpus,
/// 614 table hosts have no text before the table and a second line after
/// zero-width tokens, and the gap between their first two lines is mostly a
/// full line (most often 1,432 and 1,320 HU); 9 have none. That is a renderer
/// limit, recorded and left to its own change.
///
/// 이것을 실패시키는 것: 인코더 ledger 가 쪽 번호 토큰을 단 정의 앞에 기록하는
/// 것(실제 직렬화 순서와 어긋나 경계 불일치 → 캐시 drop 경고), 경계 자리를
/// 한 토큰 앞(wire 16)으로 놓는 것.
#[test]
fn a_restorable_table_host_keeps_its_lines_on_both_pdf_paths() {
    let (hwp, source) = restorable();
    let (hwpx, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&hwp, ConvertOptions::default()).expect("convert");
    assert!(cache_drops(&warnings).is_empty(), "{warnings:?}");
    // The output writes the same `secPr · colPr · pageNum · tbl`, so both
    // lines come back exactly as the source stored them: line 1 at wire 24.
    let lines = first_paragraph_lines(&hwpx);
    assert_eq!(lines.iter().map(|l| l[0]).collect::<Vec<_>>(), [0, 24]);
    assert_eq!(lines, source, "every cached field carried as stored");

    // Both paths reach the renderer and stop at the same table-pagination
    // check (not, say, at font resolution). When the renderer places a table
    // on its own line, this becomes a rendered PDF and the assertion changes.
    let refusals: Vec<(Option<PdfErrorCode>, Option<String>, String)> = [&hwp[..], &hwpx[..]]
        .iter()
        .map(|bytes| {
            let error = to_pdf(bytes, &pdf_options()).expect_err("table pagination refuses");
            let location = error.cause_info().and_then(|cause| cause.location);
            (error.cause(), location, error.to_string())
        })
        .collect();
    assert_eq!(refusals[0].0, Some(PdfErrorCode::InvalidCache), "{}", refusals[0].2);
    assert!(refusals[0].2.contains("computed table pagination"), "{}", refusals[0].2);
    assert_eq!(refusals[0], refusals[1], "both paths refuse the same way");
}

/// G5 (2): the boundary names a page-number token the output does not have,
/// so the host's cache is dropped with one warning, and both PDF paths refuse
/// the same paragraph for its missing cache.
///
/// 이것을 실패시키는 것: 종류가 맞지 않는 경계를 drop 대신 좌표로만 놓는 것
/// (경고 0 이 되고 두 경로가 렌더를 시도).
#[test]
fn an_unrestorable_table_host_is_dropped_and_refused_at_the_same_place() {
    let hwp = unrestorable();
    let (hwpx, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&hwp, ConvertOptions::default()).expect("convert");
    let drops = cache_drops(&warnings);
    assert_eq!(drops.len(), 1, "{warnings:?}");
    assert!(drops[0].contains("BodyParagraph(1)]"), "{drops:?}");
    assert!(drops[0].contains("skip [PageNumber]"), "{drops:?}");

    let locations: Vec<Option<String>> = [&hwp[..], &hwpx[..]]
        .iter()
        .map(|bytes| {
            let error = to_pdf(bytes, &pdf_options()).expect_err("refused");
            assert_eq!(error.cause(), Some(PdfErrorCode::MissingLayoutCache), "{error}");
            error.cause_info().and_then(|cause| cause.location)
        })
        .collect();
    assert_eq!(locations[0], locations[1]);
    assert_eq!(locations[0].as_deref(), Some("s0/p1"));
}
