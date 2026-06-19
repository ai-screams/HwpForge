//! Probe `%clk` (CLICK_HERE press-field) wire layout: dump every record's
//! payload (especially tag 0x57 sub-records that may carry the field name).
//!
//! Usage: `cargo run -p hwpforge-smithy-hwp5 --example probe_clickhere -- <hwp_path>`

use std::env;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_clickhere <hwp_path>");
        return ExitCode::FAILURE;
    };
    let bytes = std::fs::read(&path).expect("read");
    let mut cfb = cfb::CompoundFile::open(std::io::Cursor::new(&bytes[..])).expect("cfb");
    let mut header = Vec::new();
    cfb.open_stream("/FileHeader").unwrap().read_to_end(&mut header).unwrap();
    let compressed = (header[36] & 0x01) != 0;
    let mut sec0 = Vec::new();
    cfb.open_stream("/BodyText/Section0").unwrap().read_to_end(&mut sec0).unwrap();
    let payload = if compressed {
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(&sec0[..]).read_to_end(&mut out).unwrap();
        out
    } else {
        sec0
    };

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

        // Focus on lvl=2 sub-records that follow %clk (likely tag 0x57)
        // and the %clk ctrl_header itself.
        let is_clk = tag == 0x47
            && data.len() >= 4
            && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == 0x2563_6C6B;
        let is_sub = level == 2 && (tag == 0x57 || tag == 0x4C);

        if is_clk || is_sub {
            let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
            println!("[{idx:03}] tag=0x{tag:02X} lvl={level} size={size}");
            println!("      hex: {}", hex.join(" "));
            // Try UTF-16LE decode for printable chars
            if data.len() >= 2 {
                let mut u16s = Vec::new();
                let mut k = 0;
                while k + 1 < data.len() {
                    u16s.push(u16::from_le_bytes([data[k], data[k + 1]]));
                    k += 2;
                }
                if let Ok(s) = String::from_utf16(&u16s) {
                    let printable: String =
                        s.chars().map(|c| if c.is_control() { '.' } else { c }).collect();
                    println!("      as utf16le: {printable:?}");
                }
            }
            println!();
        }
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}
