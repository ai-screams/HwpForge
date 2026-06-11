//! Task #73 probe — dutmal (`tdut`) CtrlHeader tail-field offset 측정.
//!
//! `sample-dutmal-variants.hwp` 는 한 문단에 한 속성만 baseline 에서 바꾼
//! 6개의 덧말을 문서 순서대로 담고 있다 (baseline / szratio-50 /
//! szratio-75 / align-left / align-right / pos-bottom). 이 probe 는
//! BodyText/Section0 의 record 를 scan 하면서 `tdut` CtrlHeader payload
//! 전체를 hex dump 하고, 첫 번째(=baseline) 와의 byte diff 를 표시한다 —
//! 바뀐 offset 이 곧 해당 속성의 wire 위치다.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_dutmal_tail -- \
//!     examples/hwp5_review/sample-dutmal-variants.hwp

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

const CTRL_HEADER_TAG: u32 = 0x47; // BodyText TagId::CtrlHeader
const CTRL_ID_DUTMAL: u32 = 0x7464_7574; // "tdut" BE-ascii

const LABELS: &[&str] =
    &["baseline", "szratio-50", "szratio-75", "align-left", "align-right", "pos-bottom"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/hwp5_review/sample-dutmal-variants.hwp".to_string());

    let bytes = std::fs::read(&path)?;
    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    let mut section_raw = Vec::new();
    comp.open_stream("/BodyText/Section0")?.read_to_end(&mut section_raw)?;
    let mut buf = Vec::new();
    DeflateDecoder::new(&section_raw[..]).read_to_end(&mut buf)?;

    println!("=== probe_dutmal_tail ===");
    println!("file: {path}");
    println!("section size: {} bytes", buf.len());
    println!();

    let mut dutmals: Vec<Vec<u8>> = Vec::new();
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let header = u32::from_le_bytes(buf[pos..pos + 4].try_into()?);
        let tag = header & 0x3FF;
        let level = (header >> 10) & 0x3FF;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        let mut data_off = 4;
        if size == 0xFFF {
            size = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into()?) as usize;
            data_off = 8;
        }
        let data = &buf[pos + data_off..pos + data_off + size];
        if tag == CTRL_HEADER_TAG && data.len() >= 4 {
            // ctrl_id is stored LE in the stream; from_le_bytes yields the
            // BE-ascii numeric value our constants use (wire `74 75 64 74`
            // → 0x7464_7574 "tdut").
            let ctrl_id = u32::from_le_bytes(data[0..4].try_into()?);
            if ctrl_id == CTRL_ID_DUTMAL {
                println!(
                    "tdut #{} @ record offset {pos} (lvl={level}, size={size})",
                    dutmals.len()
                );
                dutmals.push(data.to_vec());
            }
        }
        pos += data_off + size;
    }

    println!("\nfound {} tdut CtrlHeaders\n", dutmals.len());

    for (i, d) in dutmals.iter().enumerate() {
        let label = LABELS.get(i).copied().unwrap_or("?");
        println!("--- #{i} [{label}] ({} bytes) ---", d.len());
        hexdump(d);
        if i > 0 {
            if let Some(base) = dutmals.first() {
                let diffs = diff(base, d);
                if diffs.is_empty() {
                    println!("  diff vs baseline: (identical)");
                } else {
                    for (off, b0, b1) in diffs {
                        println!("  diff vs baseline @ [{off}]: {b0:02X} -> {b1:02X}");
                    }
                }
            }
        }
        println!();
    }

    Ok(())
}

fn hexdump(data: &[u8]) {
    for (row, chunk) in data.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
        println!("  [{:3}] {}", row * 16, hex.join(" "));
    }
}

fn diff(a: &[u8], b: &[u8]) -> Vec<(usize, u8, u8)> {
    let mut out = Vec::new();
    let max = a.len().max(b.len());
    for i in 0..max {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            out.push((i, x, y));
        }
    }
    out
}
