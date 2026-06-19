//! Chart size carry 진단 — gso geometry vs ShapeComponent(0x4C) vs
//! ShapeComponentOle(0x54) extent 의 값/단위 비교.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_chart_geometry -- \
//!     tests/fixtures/charts/chart_02_single_pie.hwp

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/fixtures/charts/chart_02_single_pie.hwp".to_string());

    let bytes = std::fs::read(&path)?;
    let mut comp = CompoundFile::open(Cursor::new(&bytes[..]))?;
    let mut raw = Vec::new();
    comp.open_stream("/BodyText/Section0")?.read_to_end(&mut raw)?;
    let mut buf = Vec::new();
    DeflateDecoder::new(&raw[..]).read_to_end(&mut buf)?;

    println!("file: {path}\nsection: {} bytes\n", buf.len());

    let mut pos = 0usize;
    while pos + 4 <= buf.len() {
        let header = u32::from_le_bytes(buf[pos..pos + 4].try_into()?);
        let tag = header & 0x3FF;
        let level = (header >> 10) & 0x3FF;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        let mut off = 4;
        if size == 0xFFF {
            size = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into()?) as usize;
            off = 8;
        }
        let data = &buf[pos + off..pos + off + size];
        match tag {
            0x47 if data.len() >= 4 => {
                let id = u32::from_le_bytes(data[0..4].try_into()?);
                let ascii: String = id.to_be_bytes().iter().map(|b| *b as char).collect();
                if ascii == "gso " {
                    println!("CtrlHeader gso (lvl={level}, {size}B):");
                    for (row, c) in data.chunks(16).enumerate().take(4) {
                        let hex: Vec<String> = c.iter().map(|b| format!("{b:02X}")).collect();
                        println!("  [{:3}] {}", row * 16, hex.join(" "));
                    }
                }
            }
            0x4C => {
                println!("ShapeComponent 0x4C (lvl={level}, {size}B):");
                for (row, c) in data.chunks(16).enumerate().take(4) {
                    let hex: Vec<String> = c.iter().map(|b| format!("{b:02X}")).collect();
                    println!("  [{:3}] {}", row * 16, hex.join(" "));
                }
            }
            0x54 => {
                println!("ShapeComponentOle 0x54 (lvl={level}, {size}B):");
                for (row, c) in data.chunks(16).enumerate().take(3) {
                    let hex: Vec<String> = c.iter().map(|b| format!("{b:02X}")).collect();
                    println!("  [{:3}] {}", row * 16, hex.join(" "));
                }
                if data.len() >= 12 {
                    let w = i32::from_le_bytes(data[4..8].try_into()?);
                    let h = i32::from_le_bytes(data[8..12].try_into()?);
                    println!("  parsed extent: {w} x {h}");
                }
            }
            _ => {}
        }
        pos += off + size;
    }
    Ok(())
}
