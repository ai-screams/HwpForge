//! Wave 12m Phase 1 — `%xrf` (cross-reference) wire 덤프.
//!
//! 사용법:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_field_crossref -- <hwp_path>
//!
//! 출력:
//!   - 발견된 cross-ref CtrlHeader (tag 0x47, ctrl_id 0x2578_7266 "%xrf")
//!   - 그 뒤에 따라오는 lvl=2 sub-records
//!   - Command 문자열 UTF-16LE 디코드 시도
//!   - 가능한 RefType / RefContentType / as_hyperlink 후보 byte position 강조
//!
//! 목적: 현재 HWP5 projection 은 cross-ref 의 target_name 만 추출하고
//! ref_type / content_type / as_hyperlink 는 하드코딩
//! (Bookmark / Page / false). 실제 wire 에서 이 값들이 어디에 인코딩
//! 되는지 식별해서 #78 structured upgrade 의 prerequisite 으로 사용.

use std::env;
use std::io::Read;
use std::process::ExitCode;

const CTRL_ID_CROSSREF: u32 = 0x2578_7266; // "%xrf"

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_field_crossref <hwp_path>");
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

    println!("== {path} ==");
    println!("compressed: {compressed}, decoded section0 size: {}", payload.len());
    println!();

    let mut i = 0;
    let mut idx = 0;
    let mut last_was_xrf = false;
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

        let mut is_xrf = false;
        if tag == 0x47 && data.len() >= 4 {
            let cid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            if cid == CTRL_ID_CROSSREF {
                is_xrf = true;
            }
        }
        let is_following_sub = last_was_xrf && level == 2;

        if is_xrf || is_following_sub {
            let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
            print!("[{idx:03}] tag=0x{tag:02X} lvl={level} size={size}");
            if is_xrf {
                print!("  ← %xrf CtrlHeader");
            } else if is_following_sub {
                print!("  ← sub-record");
            }
            println!();
            println!("      hex: {}", hex.join(" "));

            // Annotate plausible field positions:
            //   - bytes 0..=3   ctrl_id (LE ascii "%xrf")
            //   - bytes 4..=7   properties bitfield (Wave 12l shape)
            //   - bytes 8       flag byte
            //   - bytes 9..=10  command UTF-16 length-prefix
            //   - bytes 11..    UTF-16LE command string
            //   - trailing 8B   commonly an id pair (begin / unique)
            if is_xrf && data.len() >= 13 {
                let properties = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let flag = data[8];
                let cmd_units = u16::from_le_bytes([data[9], data[10]]) as usize;
                let cmd_byte_len = cmd_units * 2;
                let cmd_start = 11;
                let cmd_end = cmd_start + cmd_byte_len;
                println!(
                    "      properties: 0x{properties:08X}  flag: 0x{flag:02X}  cmd_units: {cmd_units}",
                );
                if cmd_end <= data.len() {
                    let mut u16s = Vec::with_capacity(cmd_units);
                    let mut k = cmd_start;
                    while k + 1 < cmd_end {
                        u16s.push(u16::from_le_bytes([data[k], data[k + 1]]));
                        k += 2;
                    }
                    if let Ok(s) = String::from_utf16(&u16s) {
                        let printable: String =
                            s.chars().map(|c| if c.is_control() { '·' } else { c }).collect();
                        println!("      command: {printable:?}");
                    }
                    if data.len() > cmd_end {
                        let trailer = &data[cmd_end..];
                        let trail_hex: Vec<String> =
                            trailer.iter().map(|b| format!("{b:02X}")).collect();
                        println!(
                            "      trailer ({} bytes): {}",
                            trailer.len(),
                            trail_hex.join(" "),
                        );
                    }
                }
            } else if data.len() >= 2 {
                let mut u16s = Vec::new();
                let mut k = 0;
                while k + 1 < data.len() {
                    u16s.push(u16::from_le_bytes([data[k], data[k + 1]]));
                    k += 2;
                }
                if let Ok(s) = String::from_utf16(&u16s) {
                    let printable: String =
                        s.chars().map(|c| if c.is_control() { '·' } else { c }).collect();
                    println!("      as utf16le: {printable:?}");
                }
            }
            println!();
        }

        last_was_xrf = is_xrf;
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}
