//! Probe DocInfo stream for `MemoShape (0x5C)` records — what HWPX's
//! `MemoShapeIDRef` parameter is supposed to point at.
//!
//! Usage: `cargo run -p hwpforge-smithy-hwp5 --example probe_memo_docinfo -- <hwp_path>`

use std::env;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_memo_docinfo <hwp_path>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("read {path} failed: {err}");
            return ExitCode::FAILURE;
        }
    };

    let file = match cfb::CompoundFile::open(std::io::Cursor::new(&bytes[..])) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("cfb open failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut cfb = file;
    let mut header = Vec::new();
    cfb.open_stream("/FileHeader")
        .and_then(|mut s| s.read_to_end(&mut header))
        .expect("file header");
    let compressed = (header[36] & 0x01) != 0;

    let mut docinfo = Vec::new();
    cfb.open_stream("/DocInfo").and_then(|mut s| s.read_to_end(&mut docinfo)).expect("docinfo");
    let payload = if compressed {
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(&docinfo[..])
            .read_to_end(&mut out)
            .expect("inflate docinfo");
        out
    } else {
        docinfo
    };

    println!("DocInfo bytes: {}", payload.len());

    let mut i = 0;
    let mut idx = 0;
    while i + 4 <= payload.len() {
        let hdr = u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
        i += 4;
        let tag = hdr & 0x3FF;
        let level = (hdr >> 10) & 0x3FF;
        let mut size = ((hdr >> 20) & 0xFFF) as usize;
        if size == 0xFFF {
            size = u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]])
                as usize;
            i += 4;
        }
        let end = i + size;
        if end > payload.len() {
            break;
        }
        let data = &payload[i..end];

        let tag_name = match tag {
            0x10 => "DOCUMENT_PROPERTIES",
            0x11 => "ID_MAPPINGS",
            0x12 => "BIN_DATA",
            0x13 => "FACE_NAME",
            0x14 => "BORDER_FILL",
            0x15 => "CHAR_SHAPE",
            0x16 => "TAB_DEF",
            0x17 => "NUMBERING",
            0x18 => "BULLET",
            0x19 => "PARA_SHAPE",
            0x1A => "STYLE",
            0x1B => "DOC_DATA",
            0x1C => "DISTRIBUTE_DOC_DATA",
            0x1E => "COMPATIBLE_DOCUMENT",
            0x1F => "LAYOUT_COMPATIBILITY",
            0x20 => "TRACKCHANGE",
            0x4F => "MEMO_SHAPE",
            0x5C => "MEMO_SHAPE",
            0x5D => "MEMO_LIST",
            _ => "?",
        };

        // Show 0x10 (DOC_PROPERTIES) and 0x20 (TRACKCHANGE) in full — that's
        // where document-level timestamps and author metadata typically live.
        // Skip 0x11/0x13 (id mappings / face names) which would drown the dump.
        let show = tag != 0x11 && tag != 0x13;
        let full_hex = tag == 0x10 || tag == 0x20 || tag == 0x5E || tag == 0x5C || tag == 0x4F;
        if show {
            println!("[{idx:03}] tag=0x{tag:02X} ({tag_name}) lvl={level} size={size}");
            let n = if full_hex { data.len() } else { data.len().min(64) };
            let hex: Vec<String> = data[..n].iter().map(|b| format!("{b:02X}")).collect();
            println!("      hex[0..{n}]: {}", hex.join(" "));
            if full_hex {
                // Also try decoding as UTF-16LE characters to spot timestamps/names.
                let mut u16s = Vec::new();
                let mut i2 = 0;
                while i2 + 1 < data.len() {
                    u16s.push(u16::from_le_bytes([data[i2], data[i2 + 1]]));
                    i2 += 2;
                }
                if let Ok(s) = String::from_utf16(&u16s) {
                    let printable: String =
                        s.chars().map(|c| if c.is_control() { '.' } else { c }).collect();
                    println!("      as utf16le: {printable:?}");
                }
            }
        }
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}
