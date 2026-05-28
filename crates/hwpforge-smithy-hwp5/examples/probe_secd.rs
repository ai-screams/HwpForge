//! Probe `secd` (구역 정의) ctrl payload to inspect HWP5 spec §4.3.10.1
//! table 130 property bits (hideFirst* mapping for gap B).
//!
//! Usage: `cargo run -p hwpforge-smithy-hwp5 --example probe_secd -- <hwp_path>`

use std::env;
use std::io::Read;
use std::process::ExitCode;

// 'secd' interpreted as a big-endian u32 (matches decoder/section.rs).
const CTRL_ID_SECD: u32 = 0x7365_6364;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_secd <hwp_path>");
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

    let mut sec0 = Vec::new();
    cfb.open_stream("/BodyText/Section0")
        .and_then(|mut s| s.read_to_end(&mut sec0))
        .expect("section0");
    let payload = if compressed {
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(&sec0[..])
            .read_to_end(&mut out)
            .expect("inflate section0");
        out
    } else {
        sec0
    };

    println!("section0 bytes: {}", payload.len());

    let mut i = 0;
    while i + 4 <= payload.len() {
        let hdr = u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
        i += 4;
        let tag = hdr & 0x3FF;
        let _level = (hdr >> 10) & 0x3FF;
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
        // HWPTAG_CTRL_HEADER == 0x47 (71). HWP5 spec lists tag 0x47 as
        // CtrlHeader (TagId::CtrlHeader -> 0x47 in our decoder).
        if tag == 0x47 && size >= 8 {
            let ctrl_id =
                u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
            let properties = u32::from_le_bytes([
                payload[i + 4],
                payload[i + 5],
                payload[i + 6],
                payload[i + 7],
            ]);
            let ctrl_chars = [
                payload[i + 3] as char,
                payload[i + 2] as char,
                payload[i + 1] as char,
                payload[i] as char,
            ];
            println!(
                "ctrl id=0x{ctrl_id:08X} ({}{}{}{})  properties=0x{properties:08X}  bits={:032b}",
                ctrl_chars[0], ctrl_chars[1], ctrl_chars[2], ctrl_chars[3], properties
            );
            if ctrl_id == CTRL_ID_SECD {
                println!("  -> SECD (section def, table 130 properties):");
                println!("     bit 0  hide header           = {}", properties & 1);
                println!("     bit 1  hide footer           = {}", (properties >> 1) & 1);
                println!("     bit 2  hide masterpage       = {}", (properties >> 2) & 1);
                println!("     bit 3  hide border           = {}", (properties >> 3) & 1);
                println!("     bit 4  hide fill             = {}", (properties >> 4) & 1);
                println!("     bit 5  hide page_num         = {}", (properties >> 5) & 1);
                println!("     bit 8  first-only border     = {}", (properties >> 8) & 1);
                println!("     bit 9  first-only fill       = {}", (properties >> 9) & 1);
                println!("     bit 19 hide empty line       = {}", (properties >> 19) & 1);
            }
        }
        i = end;
    }
    ExitCode::SUCCESS
}
