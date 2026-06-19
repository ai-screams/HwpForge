//! Probe memo wire structure: dump every record's tag/level + the
//! `ParaText` UTF-16 stream decoded into TextSegments (so we can see
//! exactly where `FieldBegin` markers land and what `extra` bytes carry).
//!
//! Usage: `cargo run -p hwpforge-smithy-hwp5 --example probe_memo -- <hwp_path>`

use std::env;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_memo <hwp_path>");
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
    println!();

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

        let tag_name = match tag {
            0x42 => "PARA_HEADER",
            0x43 => "PARA_TEXT",
            0x44 => "PARA_CHAR_SHAPE",
            0x45 => "PARA_LINE_SEG",
            0x46 => "PARA_RANGE_TAG",
            0x47 => "CTRL_HEADER",
            0x48 => "LIST_HEADER",
            0x49 => "PAGE_DEF",
            0x4A => "FOOTNOTE_SHAPE",
            0x4B => "PAGE_BORDER_FILL",
            0x4C => "SHAPE_COMPONENT",
            0x58 => "EQ_EDIT",
            0x5C => "MEMO_SHAPE",
            0x5D => "MEMO_LIST",
            _ => "?",
        };

        println!("[{idx:03}] tag=0x{tag:02X} ({tag_name}) lvl={level} size={size}");

        if tag == 0x47 && data.len() >= 8 {
            let ctrl_id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let ctrl_be_ascii =
                [data[3] as char, data[2] as char, data[1] as char, data[0] as char];
            let properties = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            println!(
                "      ctrl_id=0x{ctrl_id:08X} ('{}{}{}{}') properties=0x{properties:08X}",
                ctrl_be_ascii[0], ctrl_be_ascii[1], ctrl_be_ascii[2], ctrl_be_ascii[3]
            );
            // Full hex dump after the standard 8-byte header.
            let body = &data[8..];
            let hex: Vec<String> = body.iter().map(|b| format!("{b:02X}")).collect();
            println!("      payload[8..]: {}", hex.join(" "));
            // Also try UTF-16BE decode of payload tail (memo/dutmal style strings).
            if body.len() >= 2 {
                let mut u16s = Vec::new();
                let mut i2 = 0;
                while i2 + 1 < body.len() {
                    u16s.push(u16::from_be_bytes([body[i2], body[i2 + 1]]));
                    i2 += 2;
                }
                if let Ok(s) = String::from_utf16(&u16s) {
                    let p: String =
                        s.chars().map(|c| if c.is_control() { '.' } else { c }).collect();
                    println!("      payload as utf16be: {p:?}");
                }
                // Also LE.
                let mut u16s = Vec::new();
                let mut i2 = 0;
                while i2 + 1 < body.len() {
                    u16s.push(u16::from_le_bytes([body[i2], body[i2 + 1]]));
                    i2 += 2;
                }
                if let Ok(s) = String::from_utf16(&u16s) {
                    let p: String =
                        s.chars().map(|c| if c.is_control() { '.' } else { c }).collect();
                    println!("      payload as utf16le: {p:?}");
                }
            }
            // Try both LE and BE char_count interpretations and dump the
            // command string + slash split for %unk ctrls.
            if data.len() >= 10 && ctrl_id == 0x2575_6E6B {
                let attempts = [
                    ("LE/LE", u16::from_le_bytes([data[8], data[9]]) as usize, true),
                    ("BE/BE", u16::from_be_bytes([data[8], data[9]]) as usize, false),
                ];
                for (label, char_count, char_is_le) in attempts {
                    if char_count == 0 || char_count > 200 {
                        println!("      char_count({label})={char_count}  (out of range, skip)");
                        continue;
                    }
                    if data.len() < 10 + char_count * 2 {
                        println!("      char_count({label})={char_count}  (truncates payload)");
                        continue;
                    }
                    let mut u16s = Vec::with_capacity(char_count);
                    for k in 0..char_count {
                        let off = 10 + k * 2;
                        let u = if char_is_le {
                            u16::from_le_bytes([data[off], data[off + 1]])
                        } else {
                            u16::from_be_bytes([data[off], data[off + 1]])
                        };
                        u16s.push(u);
                    }
                    match String::from_utf16(&u16s) {
                        Ok(cmd) => {
                            println!("      char_count({label})={char_count}  command={cmd:?}");
                            let parts: Vec<&str> = cmd.split('/').collect();
                            for (i, p) in parts.iter().enumerate() {
                                println!("           slash[{i:>2}] = {p:?}");
                            }
                        }
                        Err(_) => {
                            println!("      char_count({label})={char_count}  (utf16 decode fail)")
                        }
                    }
                }
            }
        } else if tag == 0x5D && data.len() >= 4 {
            let memo_id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            println!("      memo_id(LE u32)={memo_id} (0x{memo_id:08X})");
        } else if tag == 0x43 {
            // Decode ParaText UTF-16 stream into segments inline.
            if !data.len().is_multiple_of(2) {
                println!("      ! ParaText odd byte count");
            } else {
                let code_units: Vec<u16> =
                    data.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
                dump_para_text(&code_units);
            }
        }
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}

fn dump_para_text(code_units: &[u16]) {
    let mut j = 0usize;
    let mut text_buf: Vec<u16> = Vec::new();
    let mut visible_utf16 = 0u32;
    let emit_text = |buf: &mut Vec<u16>, vis: &mut u32| {
        if !buf.is_empty() {
            let s = String::from_utf16(buf).unwrap_or_else(|_| String::from("<utf16 err>"));
            let len = s.encode_utf16().count() as u32;
            println!("      [vis={vis:>3}] TEXT({len}) {:?}", s);
            *vis += len;
            buf.clear();
        }
    };
    while j < code_units.len() {
        let cp = code_units[j];
        j += 1;
        match cp {
            0x00 => {
                emit_text(&mut text_buf, &mut visible_utf16);
                println!("      [vis={:>3}] CTRL 0x00 (reserved)", visible_utf16);
            }
            0x0D => {
                // ParaBreak: single u16, no extra bytes.
                emit_text(&mut text_buf, &mut visible_utf16);
                println!("      [vis={visible_utf16:>3}] PARA_BREAK");
                visible_utf16 += 1;
            }
            0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0B | 0x0C | 0x0E
            | 0x0F | 0x10 | 0x11 | 0x12 | 0x13 | 0x14 | 0x15 | 0x16 | 0x17 | 0x19 | 0x1A | 0x1B
            | 0x1C | 0x1D | 0x1E => {
                // 8 u16 inline control (1 + 7 extra)
                emit_text(&mut text_buf, &mut visible_utf16);
                if j + 7 > code_units.len() {
                    println!("      [vis={:>3}] CTRL 0x{cp:02X} (truncated extra)", visible_utf16);
                    break;
                }
                let extra: Vec<u8> =
                    code_units[j..j + 7].iter().flat_map(|u| u.to_le_bytes()).collect();
                let name = match cp {
                    0x02 => "SECTION_COLUMN_DEF",
                    0x03 => "FIELD_BEGIN",
                    0x04 => "FIELD_END",
                    0x06 => "FOOTNOTE_REF",
                    0x0B => "CONTROL_REF",
                    0x0C => "EXT_CONTROL_REF",
                    _ => "INLINE",
                };
                // Try multiple ctrl_id extractions for FIELD_BEGIN
                if cp == 0x03 {
                    let raw_le = u32::from_le_bytes([extra[0], extra[1], extra[2], extra[3]]);
                    let be_reversed = u32::from_be_bytes([extra[3], extra[2], extra[1], extra[0]]);
                    let ascii_natural =
                        [extra[0] as char, extra[1] as char, extra[2] as char, extra[3] as char];
                    let ascii_reversed =
                        [extra[3] as char, extra[2] as char, extra[1] as char, extra[0] as char];
                    println!("      [vis={:>3}] CTRL 0x{cp:02X} ({name})", visible_utf16);
                    println!(
                        "                    extra[0..4]: raw={:02X}{:02X}{:02X}{:02X}  ascii_natural='{}{}{}{}'  ascii_reversed='{}{}{}{}'",
                        extra[0],
                        extra[1],
                        extra[2],
                        extra[3],
                        ascii_natural[0],
                        ascii_natural[1],
                        ascii_natural[2],
                        ascii_natural[3],
                        ascii_reversed[0],
                        ascii_reversed[1],
                        ascii_reversed[2],
                        ascii_reversed[3],
                    );
                    println!(
                        "                    extracted u32: from_le_bytes=0x{raw_le:08X}  from_be_bytes_reversed=0x{be_reversed:08X}"
                    );
                    println!(
                        "                    extra[4..14]: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                        extra[4],
                        extra[5],
                        extra[6],
                        extra[7],
                        extra[8],
                        extra[9],
                        extra[10],
                        extra[11],
                        extra[12],
                        extra[13],
                    );
                } else {
                    println!(
                        "      [vis={:>3}] CTRL 0x{cp:02X} ({name}) extra={:02X?}",
                        visible_utf16, extra
                    );
                }
                j += 7;
                visible_utf16 += 1; // inline control takes 1 visible slot
            }
            0x0A => {
                emit_text(&mut text_buf, &mut visible_utf16);
                println!("      [vis={:>3}] LINE_BREAK", visible_utf16);
                visible_utf16 += 1;
            }
            0x18 => {
                emit_text(&mut text_buf, &mut visible_utf16);
                println!("      [vis={:>3}] NBSP", visible_utf16);
                visible_utf16 += 1;
            }
            0x1F => {
                emit_text(&mut text_buf, &mut visible_utf16);
                println!("      [vis={:>3}] FW_SPACE", visible_utf16);
                visible_utf16 += 1;
            }
            _ => {
                text_buf.push(cp);
            }
        }
    }
    emit_text(&mut text_buf, &mut visible_utf16);
}
