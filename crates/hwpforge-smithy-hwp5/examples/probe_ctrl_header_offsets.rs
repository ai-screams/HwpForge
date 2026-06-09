//! Wave 12p Step 5 (task #134) audit — record-aware ctrl header instId
//! offset 측정.
//!
//! Codex 가 "gso/tbl/eqed 도 data[36..40]" 라고 주장. 우리 자체 audit
//! 으로 검증: BodyText/Section0 의 decompressed bytes 를 record header
//! 단위로 scan 하면서 CtrlHeader 만나면 ctrl_id 와 함께 raw data 를
//! dump.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_ctrl_header_offsets -- \
//!     <native.hwp> <target_instId_dec>

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

const CTRL_HEADER_TAG: u32 = 0x42; // BodyText TagId::CtrlHeader

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: probe_ctrl_header_offsets <path.hwp> <id>")?;
    let target_id: u32 = args.next().ok_or("need target id")?.parse()?;

    let bytes = std::fs::read(&path)?;
    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    let mut section_raw = Vec::new();
    comp.open_stream("/BodyText/Section0")?.read_to_end(&mut section_raw)?;
    let mut buf = Vec::new();
    DeflateDecoder::new(&section_raw[..]).read_to_end(&mut buf)?;

    println!("=== probe_ctrl_header_offsets ===");
    println!("file: {path}");
    println!("target_id: {target_id} (0x{target_id:08x}) LE: [{}]", le_hex(target_id));
    println!("section size: {} bytes", buf.len());
    println!();

    // Scan records: HWP5 record header = 32-bit packed (tag 10bit | level 10bit | size 12bit)
    // If size == 0xFFF, extended size in next 4 bytes.
    let mut pos = 0;
    let mut record_idx = 0;
    while pos + 4 <= buf.len() {
        let header = u32::from_le_bytes(buf[pos..pos + 4].try_into()?);
        let tag = header & 0x3FF;
        let level = (header >> 10) & 0x3FF;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        let mut data_off = 4;
        if size == 0xFFF {
            if pos + 8 > buf.len() {
                break;
            }
            size = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into()?) as usize;
            data_off = 8;
        }
        let data_start = pos + data_off;
        let data_end = data_start + size;
        if data_end > buf.len() {
            break;
        }

        if tag == CTRL_HEADER_TAG && size >= 4 {
            let data = &buf[data_start..data_end];
            let ctrl_id = u32::from_le_bytes(data[0..4].try_into()?);
            // Print ALL CtrlHeaders (audit) even if target not found
            let ctrl_id_str = format!(
                "{}{}{}{}",
                (ctrl_id & 0xFF) as u8 as char,
                ((ctrl_id >> 8) & 0xFF) as u8 as char,
                ((ctrl_id >> 16) & 0xFF) as u8 as char,
                ((ctrl_id >> 24) & 0xFF) as u8 as char
            );
            let _suppress = &ctrl_id_str;
            println!(
                "  record #{record_idx} tag={tag} level={level} size={size} ctrl_id=0x{ctrl_id:08x} ({ctrl_id_str:?})"
            );
            // Search for target_id in this record's data buffer
            let needle = target_id.to_le_bytes();
            let positions: Vec<usize> = (0..data.len().saturating_sub(4))
                .filter(|&i| data[i..i + 4] == needle[..])
                .collect();

            let ctrl_id_str = format!(
                "{}{}{}{}",
                (ctrl_id & 0xFF) as u8 as char,
                ((ctrl_id >> 8) & 0xFF) as u8 as char,
                ((ctrl_id >> 16) & 0xFF) as u8 as char,
                ((ctrl_id >> 24) & 0xFF) as u8 as char
            );

            if !positions.is_empty() {
                println!(
                    "★ RECORD #{record_idx} CtrlHeader tag={tag} level={level} size={size} \
                     ctrl_id=0x{ctrl_id:08x} ({ctrl_id_str:?}) data_start@buf=0x{data_start:x}"
                );
                println!("  target_id found at data offsets: {:?}", positions);
                for pos in &positions {
                    print!("  @data[{pos}] context: ");
                    let lo = pos.saturating_sub(8);
                    let hi = (pos + 12).min(data.len());
                    for b in &data[lo..hi] {
                        print!("{b:02x} ");
                    }
                    println!();
                }
                // Dump first 48 bytes
                let dump_end = 48.min(data.len());
                print!("  first 48 bytes of data: ");
                for (i, b) in data[..dump_end].iter().enumerate() {
                    if i % 4 == 0 {
                        print!("  ");
                    }
                    print!("{b:02x} ");
                }
                println!();
            }
        }

        pos = data_end;
        record_idx += 1;
    }

    Ok(())
}

fn le_hex(v: u32) -> String {
    let b = v.to_le_bytes();
    format!("{:02x} {:02x} {:02x} {:02x}", b[0], b[1], b[2], b[3])
}
