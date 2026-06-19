//! Wave 12n Phase 1B — `%dte` / `%tim` / `%dts` / `%usr` 자동 필드 wire 덤프.
//!
//! 사용법:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_field_auto -- <hwp_path>
//!
//! 출력:
//!   - 발견된 자동 필드 CtrlHeader (tag 0x47) 와 payload
//!   - 그 뒤에 따라오는 lvl=2 sub-records (예: 0x57 CtrlData, 0x4C ListHeader)
//!   - Command 문자열 UTF-16LE 디코드 시도
//!
//! 목적: %clk 가 Wave 12l 에서 그랬듯, 자동 필드 4종 각각이 어떤 ctrl_id +
//! sub-record 구조로 인코딩되는지 사용자 한컴 native fixture 에서 직접 확인.

use std::env;
use std::io::Read;
use std::process::ExitCode;

const AUTO_FIELD_IDS: &[(u32, &str)] = &[
    // 확인된 ID (Wave 12n Phase 1)
    (0x2564_7465, "%dte (Date/Time — 형식으로 구분)"),
    (0x2573_6D72, "%smr (Summary — 만든 사람/저장한 사람/문서 제목 등)"),
    (0x2570_6174, "%pat (Path/File name)"),
    (0x6174_6E6F, "atno (AutoNum — 쪽 번호)"),
    // 추정 — fixture 검증 대기
    (0x2574_696D, "%tim (별도 ID 일 가능성)"),
    (0x2575_7372, "%usr (별도 ID 일 가능성)"),
];

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_field_auto <hwp_path>");
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
    let mut last_was_field = false;
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

        // %clk 류 CtrlHeader: tag 0x47 + 처음 4바이트가 ctrl_id (LE)
        let mut hit_label: Option<&'static str> = None;
        if tag == 0x47 && data.len() >= 4 {
            let cid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            for &(id, label) in AUTO_FIELD_IDS {
                if cid == id {
                    hit_label = Some(label);
                    break;
                }
            }
        }
        // 자동 필드 다음 lvl=2 sub-records (예: 0x57 CtrlData, 0x4C ListHeader)
        let is_following_sub = last_was_field && level == 2;

        if hit_label.is_some() || is_following_sub {
            let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
            print!("[{idx:03}] tag=0x{tag:02X} lvl={level} size={size}");
            if let Some(lbl) = hit_label {
                print!("  ← {lbl}");
            }
            println!();
            println!("      hex: {}", hex.join(" "));

            // UTF-16LE 디코드 시도 (Command/Hint/Name 가능성)
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

        last_was_field = hit_label.is_some();
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}
