//! Probe a HWP5 file's `TabDef` records and dump raw values.
//!
//! Uses the public `census_hwp5_file` path; if that doesn't expose tab
//! definitions, reach into raw record bytes via `Hwp5Decoder::decode_file`
//! is not enough because the public surface doesn't expose `tab_defs`. This
//! example calls into raw record parsing directly via the package reader.

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_tabdef <hwp_path>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("read {path} failed: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Walk DocInfo records manually to find TabDef (TagId 0x16).
    // OLE2 → /DocInfo stream → (optional zlib inflate) → TLV records.
    let file = match cfb::CompoundFile::open(std::io::Cursor::new(&bytes[..])) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("cfb open failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut cfb = file;
    // Read FileHeader for compressed flag.
    let mut header = Vec::new();
    if let Err(err) = cfb.open_stream("/FileHeader").and_then(|mut s| {
        use std::io::Read;
        s.read_to_end(&mut header)
    }) {
        eprintln!("/FileHeader read failed: {err}");
        return ExitCode::FAILURE;
    }
    if header.len() < 40 {
        eprintln!("FileHeader too short");
        return ExitCode::FAILURE;
    }
    let compressed = (header[36] & 0x01) != 0;
    println!("compressed: {compressed}");

    let mut doc_info = Vec::new();
    if let Err(err) = cfb.open_stream("/DocInfo").and_then(|mut s| {
        use std::io::Read;
        s.read_to_end(&mut doc_info)
    }) {
        eprintln!("/DocInfo read failed: {err}");
        return ExitCode::FAILURE;
    }
    let payload = if compressed {
        use std::io::Read;
        let mut out = Vec::new();
        let mut dec = flate2::read::DeflateDecoder::new(&doc_info[..]);
        if let Err(err) = dec.read_to_end(&mut out) {
            eprintln!("DocInfo inflate failed: {err}");
            return ExitCode::FAILURE;
        }
        out
    } else {
        doc_info
    };
    println!("doc_info bytes: {}", payload.len());

    // Walk records.
    let mut i = 0;
    while i + 4 <= payload.len() {
        let hdr = u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
        i += 4;
        let tag = hdr & 0x3FF;
        let level = (hdr >> 10) & 0x3FF;
        let mut size = ((hdr >> 20) & 0xFFF) as usize;
        if size == 0xFFF {
            if i + 4 > payload.len() {
                break;
            }
            size = u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]])
                as usize;
            i += 4;
        }
        let end = i + size;
        if end > payload.len() {
            break;
        }
        if tag == 0x16 {
            let slice = &payload[i..end];
            let property = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
            let count = i32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
            println!("TAB_DEF level={level} size={size} property=0x{property:08X} count={count}");
            for k in 0..count.max(0) as usize {
                let base = 8 + k * 8;
                if base + 8 > slice.len() {
                    break;
                }
                let pos = u32::from_le_bytes([
                    slice[base],
                    slice[base + 1],
                    slice[base + 2],
                    slice[base + 3],
                ]);
                let ttype = slice[base + 4];
                let ftype = slice[base + 5];
                let rsv = u16::from_le_bytes([slice[base + 6], slice[base + 7]]);
                println!(
                    "  stop[{k}] position={pos} tab_type={ttype} fill_type={ftype} reserved=0x{rsv:04X}"
                );
            }
        }
        i = end;
    }
    ExitCode::SUCCESS
}
