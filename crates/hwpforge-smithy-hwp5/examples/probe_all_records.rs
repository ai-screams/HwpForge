use std::env;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let path = env::args().nth(1).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut cfb = cfb::CompoundFile::open(std::io::Cursor::new(&bytes[..])).unwrap();
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

    println!("section0 decoded: {} bytes", payload.len());

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

        let cid_str = if tag == 0x47 && data.len() >= 4 {
            let b = &data[..4];
            let s: String = b
                .iter()
                .rev()
                .map(|&c| if c.is_ascii_graphic() || c == b' ' { c as char } else { '.' })
                .collect();
            format!("  ctrl_id={:02X}{:02X}{:02X}{:02X} ('{}')", b[3], b[2], b[1], b[0], s)
        } else {
            String::new()
        };
        println!("[{idx:03}] tag=0x{tag:02X} lvl={level} size={size}{cid_str}");
        i = end;
        idx += 1;
    }
    ExitCode::SUCCESS
}
