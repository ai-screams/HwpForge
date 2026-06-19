//! Task #75 probe — `idxm` surrogate-pair primary + trailer 관찰.
//!
//! `sample-indexmark-surrogate.hwp` 는 5개의 IndexMark 를 문서 순서대로
//! 담고 있다 (bmp-baseline / emoji-first / emoji-only / emoji-mid /
//! emoji-secondary). 이 probe 는 각 `idxm` CtrlHeader payload 전체를
//! hex dump 해서:
//!
//! 1. primary 가 surrogate pair 로 시작할 때 `properties.high` 에
//!    high surrogate (0xD83D) 가 단독 packing 되는지
//! 2. trailer 4 bytes 값 (native 0xFFFFFFFF vs round-trip 0x00000000)
//!
//! 을 확정한다.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_indexmark_surrogate -- \
//!     examples/hwp5_review/sample-indexmark-surrogate.hwp

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

const CTRL_HEADER_TAG: u32 = 0x47;
const CTRL_ID_INDEXMARK: u32 = 0x6964_786D; // "idxm" BE-ascii

const LABELS: &[&str] =
    &["bmp-baseline", "emoji-first", "emoji-only", "emoji-mid", "emoji-secondary"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/hwp5_review/sample-indexmark-surrogate.hwp".to_string());

    let bytes = std::fs::read(&path)?;
    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    let mut section_raw = Vec::new();
    comp.open_stream("/BodyText/Section0")?.read_to_end(&mut section_raw)?;
    let mut buf = Vec::new();
    DeflateDecoder::new(&section_raw[..]).read_to_end(&mut buf)?;

    println!("=== probe_indexmark_surrogate ===");
    println!("file: {path}");
    println!("section size: {} bytes\n", buf.len());

    let mut idx = 0usize;
    let mut pos = 0usize;
    while pos + 4 <= buf.len() {
        let header = u32::from_le_bytes(buf[pos..pos + 4].try_into()?);
        let tag = header & 0x3FF;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        let mut data_off = 4;
        if size == 0xFFF {
            size = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into()?) as usize;
            data_off = 8;
        }
        let data = &buf[pos + data_off..pos + data_off + size];
        if tag == CTRL_HEADER_TAG
            && data.len() >= 8
            && u32::from_le_bytes(data[0..4].try_into()?) == CTRL_ID_INDEXMARK
        {
            let label = LABELS.get(idx).copied().unwrap_or("?");
            let units_len = u16::from_le_bytes([data[4], data[5]]);
            let first = u16::from_le_bytes([data[6], data[7]]);
            println!("--- #{idx} [{label}] ({size} bytes) ---");
            println!("  primary_units_len={units_len}, primary[0]=U+{first:04X}");
            for (row, chunk) in data.chunks(16).enumerate() {
                let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
                println!("  [{:3}] {}", row * 16, hex.join(" "));
            }
            if data.len() >= 4 {
                let trailer = &data[data.len() - 4..];
                println!(
                    "  trailer (last 4): {:02X} {:02X} {:02X} {:02X}",
                    trailer[0], trailer[1], trailer[2], trailer[3]
                );
            }
            println!();
            idx += 1;
        }
        pos += data_off + size;
    }
    println!("found {idx} idxm CtrlHeaders");
    Ok(())
}
