//! Task #74 probe — compose (`tcps`) properties.high shape glyph +
//! layout variant 관찰.
//!
//! `sample-compose-all-shapes.hwp` 는 OWPML SHAPECIRCLETYPE 14종 ×
//! COMPOSETYPE 2종 = 28개의 글자겹침을 문서 순서대로 담고 있다
//! (SPREAD 14개 → OVERLAP 14개, 각 그룹 안에서 OWPML enum 순서).
//! 이 probe 는 각 `tcps` CtrlHeader 의 `properties.low`(layout
//! discriminator) / `properties.high`(shape glyph 후보) 와 body trailer
//! (`circle_type_raw`, `char_sz`, `compose_type_raw`, `char_pr_cnt`) 를
//! 표로 dump 한다 — glyph ↔ circleType 매핑과 미관찰 layout variant
//! 존재 여부를 한 번에 귀속한다.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_compose_variants -- \
//!     examples/hwp5_review/sample-compose-all-shapes.hwp

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

const CTRL_HEADER_TAG: u32 = 0x47;
const CTRL_ID_COMPOSE: u32 = 0x7463_7073; // "tcps" BE-ascii

const CIRCLE_TYPES: &[&str] = &[
    "CHAR",
    "SHAPE_CIRCLE",
    "SHAPE_REVERSAL_CIRCLE",
    "SHAPE_RECTANGLE",
    "SHAPE_REVERSAL_RECTANGLE",
    "SHAPE_TRIANGLE",
    "SHAPE_REVERSAL_TIRANGLE",
    "SHAPE_LIGHT",
    "SHAPE_RHOMBUS",
    "SHAPE_REVERSAL_RHOMBUS",
    "SHAPE_ROUNDED_RECTANGLE",
    "SHAPE_EMPTY_CIRCULATE_TRIANGLE",
    "SHAPE_THIN_CIRCULATE_TRIANGLE",
    "SHAPE_THICK_CIRCULATE_TRIANGLE",
];
const COMPOSE_TYPES: &[&str] = &["SPREAD", "OVERLAP"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/hwp5_review/sample-compose-all-shapes.hwp".to_string());

    let bytes = std::fs::read(&path)?;
    let cursor = Cursor::new(&bytes[..]);
    let mut comp = CompoundFile::open(cursor)?;

    let mut section_raw = Vec::new();
    comp.open_stream("/BodyText/Section0")?.read_to_end(&mut section_raw)?;
    let mut buf = Vec::new();
    DeflateDecoder::new(&section_raw[..]).read_to_end(&mut buf)?;

    println!("=== probe_compose_variants ===");
    println!("file: {path}");
    println!("section size: {} bytes\n", buf.len());

    let mut idx = 0usize;
    let mut pos = 0usize;
    println!(
        "{:<4} {:<34} {:>9} {:>14} {:>5} {:>11} {:>7} {:>6} {:>5}",
        "#", "label", "props.lo", "props.hi", "size", "circle", "char_sz", "ctype", "prcnt"
    );
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
            && u32::from_le_bytes(data[0..4].try_into()?) == CTRL_ID_COMPOSE
        {
            let ct = COMPOSE_TYPES[idx / CIRCLE_TYPES.len() % COMPOSE_TYPES.len()];
            let circle = CIRCLE_TYPES[idx % CIRCLE_TYPES.len()];
            let label = format!("{circle}/{ct}");
            let props_low = u16::from_le_bytes([data[4], data[5]]);
            let props_high = u16::from_le_bytes([data[6], data[7]]);
            let glyph = char::from_u32(u32::from(props_high))
                .filter(|c| !c.is_control())
                .map(|c| format!("U+{props_high:04X} '{c}'"))
                .unwrap_or_else(|| format!("U+{props_high:04X}"));
            // Body trailer (unpacked layout): last 44 bytes = 4 meta + 40 charPr.
            let body = &data[8..];
            let (circle_raw, char_sz, ctype_raw, prcnt) = if body.len() >= 44 {
                let m = body.len() - 44;
                (body[m], body[m + 1] as i8, body[m + 2], body[m + 3])
            } else {
                (0xFF, 0, 0xFF, 0xFF)
            };
            println!(
                "{idx:<4} {label:<34} {props_low:>#9x} {glyph:>14} {size:>5} {circle_raw:>11} {char_sz:>7} {ctype_raw:>6} {prcnt:>5}"
            );
            idx += 1;
        }
        pos += data_off + size;
    }
    println!("\nfound {idx} tcps CtrlHeaders");
    Ok(())
}
