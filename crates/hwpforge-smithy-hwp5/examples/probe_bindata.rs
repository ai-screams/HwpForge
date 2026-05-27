//! Probe a HWP5 file's BinData entries via raw CFB access.
//!
//! Reports each BinData stream's name, size, and head-byte signature so we
//! can determine whether HWP5 charts carry raw OOXML XML or an OLE blob.
//!
//! Run with:
//! ```text
//! cargo run -p hwpforge-smithy-hwp5 --example probe_bindata -- <path>
//! ```

use std::env;
use std::fs::File;
use std::io::{Cursor, Read};
use std::process::ExitCode;

use cfb::CompoundFile;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_bindata <hwp_path>");
        return ExitCode::FAILURE;
    };
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("open {path} failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut cfb = match CompoundFile::open(file) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("cfb open {path} failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!("path: {path}");
    let entries: Vec<_> = cfb
        .walk()
        .map(|e| (e.path().to_string_lossy().into_owned(), e.is_storage(), e.len()))
        .collect();
    for (entry_path, is_storage, len) in entries {
        if !entry_path.starts_with("/BinData") {
            continue;
        }
        if is_storage {
            println!("[storage] {entry_path}");
            continue;
        }
        let mut buf = Vec::with_capacity(len as usize);
        if let Err(err) = cfb.open_stream(&entry_path).and_then(|mut s| s.read_to_end(&mut buf)) {
            println!("[stream-err] {entry_path}: {err}");
            continue;
        }
        println!("[stream] {entry_path} raw_size={}", buf.len());

        // HWP5 BinData streams are DEFLATE-compressed.
        let mut decoder = flate2::read::DeflateDecoder::new(&buf[..]);
        let mut inflated = Vec::new();
        if let Err(err) = decoder.read_to_end(&mut inflated) {
            println!("    inflate_err={err}");
            continue;
        }
        println!("    inflated_size={}", inflated.len());

        // Convention seen in HWP5 OLE-backed BinData: first 4 bytes are u32
        // little-endian length of an inner OLE2 compound file.
        if inflated.len() < 4 {
            continue;
        }
        let prefix_len = u32::from_le_bytes([inflated[0], inflated[1], inflated[2], inflated[3]]);
        println!("    declared_prefix_len={prefix_len}");
        let cf_slice = &inflated[4..];
        if cf_slice.len() < 8 || &cf_slice[..4] != b"\xD0\xCF\x11\xE0" {
            println!(
                "    inner_head={:?}",
                cf_slice.iter().take(16).map(|b| format!("{:02X}", b)).collect::<Vec<_>>()
            );
            continue;
        }
        let cursor = Cursor::new(cf_slice.to_vec());
        match CompoundFile::open(cursor) {
            Ok(mut inner_cfb) => {
                let inner_entries: Vec<_> = inner_cfb
                    .walk()
                    .map(|e| (e.path().to_string_lossy().into_owned(), e.is_storage(), e.len()))
                    .collect();
                println!("    inner_cfb_entries={}", inner_entries.len());
                for (ip, is_store, ilen) in inner_entries {
                    if is_store {
                        println!("      [inner-storage] {ip}");
                        continue;
                    }
                    let mut ibuf = Vec::with_capacity(ilen as usize);
                    let _ = inner_cfb.open_stream(&ip).and_then(|mut s| s.read_to_end(&mut ibuf));
                    let head: String = ibuf
                        .iter()
                        .take(48)
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii: String = ibuf
                        .iter()
                        .take(96)
                        .map(|&b| if (32..127).contains(&b) { b as char } else { '.' })
                        .collect();
                    println!("      [inner-stream] {ip} size={}", ibuf.len());
                    println!("        head={head}");
                    println!("        ascii={ascii:?}");
                }
            }
            Err(err) => println!("    inner_cfb_open_err={err}"),
        }
    }
    ExitCode::SUCCESS
}
