//! Wave 12p Step 5 (task #134) Step 0 강화 probe — 한컴 HWPX 의 instId
//! 값이 HWP5 binary wire 의 어디에 (혹은 정말로 없는지) hex-search.
//!
//! Codex(architect) §"MEDIUM — Step 0 hex-search는 필요하지만 충분하지
//! 않습니다." 권장:
//! - compressed + decompressed BodyText stream 둘 다 검색
//! - 전체 OLE stream 대상 batch
//! - Command target_id 모두 batch search
//! - 발견 위치 ±64 bytes dump
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_crossref_wire_search -- \
//!     <native.hwp> <target_instId_dec> [<more_ids>...]
//!
//! Example:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_crossref_wire_search -- \
//!     examples/hwp5_review/sample-field-crossref-footnote.hwp 1108165575

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: probe_crossref_wire_search <path.hwp> <id> [<id>...]")?;
    let target_ids: Vec<u32> = args.map(|s| s.parse::<u32>().unwrap()).collect();
    if target_ids.is_empty() {
        return Err("Need at least one target instId".into());
    }

    let bytes = std::fs::read(&path)?;
    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    println!("=== probe_crossref_wire_search ===");
    println!("file: {path}");
    println!("target instIds (LE hex):");
    for id in &target_ids {
        println!("  {id:>11} = 0x{id:08x} = LE [{}]", le_hex(*id));
    }
    println!();

    // Enumerate all streams
    let stream_paths: Vec<String> = comp
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();

    for stream_path in &stream_paths {
        let mut stream_bytes = Vec::new();
        if comp.open_stream(stream_path)?.read_to_end(&mut stream_bytes).is_err() {
            continue;
        }

        // Search in compressed (raw) bytes
        search_in_buffer(&stream_bytes, &target_ids, stream_path, "raw");

        // If the stream looks like deflate-compressed (BodyText/Section*, DocInfo),
        // try decompressing and searching again.
        if stream_path.contains("/BodyText/Section") || stream_path == "/DocInfo" {
            if let Ok(decompressed) = decompress(&stream_bytes) {
                search_in_buffer(&decompressed, &target_ids, stream_path, "decompressed");
            }
        }
    }

    Ok(())
}

fn le_hex(v: u32) -> String {
    let b = v.to_le_bytes();
    format!("{:02x} {:02x} {:02x} {:02x}", b[0], b[1], b[2], b[3])
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    DeflateDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
}

fn search_in_buffer(buffer: &[u8], target_ids: &[u32], stream: &str, kind: &str) {
    for &id in target_ids {
        let needle = id.to_le_bytes();
        let mut positions = Vec::new();
        let mut start = 0;
        while let Some(pos) = buffer[start..].windows(4).position(|w| w == needle.as_slice()) {
            let abs = start + pos;
            positions.push(abs);
            start = abs + 1;
            if positions.len() > 20 {
                break;
            }
        }
        if positions.is_empty() {
            continue;
        }
        println!(
            "[stream={stream} kind={kind}] id={id} (0x{id:08x}) found at {} positions:",
            positions.len()
        );
        for pos in &positions {
            print!("  @0x{pos:08x} (offset {pos}) ");
            // ±32 byte hex dump around
            let lo = pos.saturating_sub(32);
            let hi = (pos + 36).min(buffer.len());
            let highlight_lo = *pos;
            let highlight_hi = pos + 4;
            print!("dump=[");
            for (i, b) in buffer[lo..hi].iter().enumerate() {
                let abs = lo + i;
                if abs == highlight_lo {
                    print!(" <<");
                }
                print!("{b:02x}");
                if abs == highlight_hi - 1 {
                    print!(">>");
                }
                print!(" ");
            }
            println!("]");
        }
        println!();
    }
}
