//! Wave 12p task #122 진단: HWP5 paragraph shape 의 outline level 7~9
//! source 추적.
//!
//! `Hwp5RawParaShape.property1` bit 25-27 은 3 bits — level 0~7 만 표현.
//! 한컴이 level 8/9 를 어디에 저장하는지 모름.
//!
//! 직접 OLE2/CFB 에서 DocInfo 스트림을 읽어 ParaShape (TagId 0x19) record
//! 의 property1 / property2 / property3 / numbering_bullet_id 를 dump.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_outline_level -- path/to/sample.hwp

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe_outline_level <path.hwp>")?;
    let bytes = std::fs::read(&path)?;

    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    // Read DocInfo stream
    let mut doc_info = Vec::new();
    comp.open_stream("/DocInfo")?.read_to_end(&mut doc_info)?;

    // HWP5 stores DocInfo with DEFLATE compression
    let mut decompressed = Vec::new();
    let mut dec = DeflateDecoder::new(&doc_info[..]);
    dec.read_to_end(&mut decompressed)?;

    // Scan records, looking for ParaShape (tag id 0x19)
    let mut pos = 0;
    let mut para_shape_idx = 0;
    let mut style_idx = 0;
    while pos + 4 <= decompressed.len() {
        let header = u32::from_le_bytes(decompressed[pos..pos + 4].try_into()?);
        let tag = header & 0x3FF;
        let _level = (header >> 10) & 0x3FF;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        let mut data_offset = 4;
        // Extended size: if size == 0xFFF, next 4 bytes are u32 size
        if size == 0xFFF {
            if pos + 8 > decompressed.len() {
                break;
            }
            size = u32::from_le_bytes(decompressed[pos + 4..pos + 8].try_into()?) as usize;
            data_offset = 8;
        }
        let end = pos + data_offset + size;
        if end > decompressed.len() {
            break;
        }
        let data = &decompressed[pos + data_offset..end];

        if tag == 0x19 {
            // ParaShape record
            print_para_shape(para_shape_idx, data);
            para_shape_idx += 1;
        } else if tag == 0x1A {
            // Style record
            print_style(style_idx, data);
            style_idx += 1;
        }

        pos = end;
    }

    Ok(())
}

fn print_style(idx: usize, data: &[u8]) {
    // Style record layout: name (utf-16 len-prefix) + english_name + kind(u8)
    // + next_style_id(u8) + lang_id(i16) + para_shape_id(u16) + char_shape_id(u16)
    // + lock_form(u16). + 추가 outline level 같은 후행 필드 가능.
    let mut pos = 0;
    let name = read_utf16le_pstring(data, &mut pos).unwrap_or_default();
    let _english_name = read_utf16le_pstring(data, &mut pos).unwrap_or_default();
    if data.len() < pos + 10 {
        println!("  Style id={idx}: TRUNCATED name={name:?}");
        return;
    }
    let kind = data[pos];
    let next_style_id = data[pos + 1];
    let lang_id = i16::from_le_bytes(data[pos + 2..pos + 4].try_into().unwrap());
    let para_shape_id = u16::from_le_bytes(data[pos + 4..pos + 6].try_into().unwrap());
    let char_shape_id = u16::from_le_bytes(data[pos + 6..pos + 8].try_into().unwrap());
    let lock_form = u16::from_le_bytes(data[pos + 8..pos + 10].try_into().unwrap());
    let trailing = &data[pos + 10..];
    let name_repr = format!("{name:?}");
    println!(
        "  Style id={idx:>3}: name={name_repr:<22} kind={kind} para_shape={para_shape_id:>3} char_shape={char_shape_id} next={next_style_id} lang=0x{lang_id:04x} lock={lock_form} trailing_bytes={} (hex={})",
        trailing.len(),
        trailing.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
    );
}

fn read_utf16le_pstring(data: &[u8], pos: &mut usize) -> Option<String> {
    if data.len() < *pos + 2 {
        return None;
    }
    let len = u16::from_le_bytes(data[*pos..*pos + 2].try_into().ok()?) as usize;
    *pos += 2;
    let byte_len = len * 2;
    if data.len() < *pos + byte_len {
        return None;
    }
    let units: Vec<u16> = data[*pos..*pos + byte_len]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    *pos += byte_len;
    String::from_utf16(&units).ok()
}

fn print_para_shape(idx: usize, data: &[u8]) {
    if data.len() < 4 {
        println!("  id={idx}: TRUNCATED (size={})", data.len());
        return;
    }
    let property1 = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let level_3bit = (property1 >> 25) & 0b111;
    let kind = (property1 >> 23) & 0b11;
    let bullet_id =
        if data.len() >= 32 { u16::from_le_bytes(data[30..32].try_into().unwrap()) } else { 0 };
    let property2 = if data.len() >= 46 {
        Some(u32::from_le_bytes(data[42..46].try_into().unwrap()))
    } else {
        None
    };
    let property3 = if data.len() >= 50 {
        Some(u32::from_le_bytes(data[46..50].try_into().unwrap()))
    } else {
        None
    };
    println!(
        "  id={idx:>3}: size={:>3}  property1=0x{property1:08x}  kind={kind} level_3bit={level_3bit}  bullet_id={bullet_id}  property2={}  property3={}",
        data.len(),
        property2.map_or("None".to_string(), |v| format!("0x{v:08x}")),
        property3.map_or("None".to_string(), |v| format!("0x{v:08x}")),
    );
}
