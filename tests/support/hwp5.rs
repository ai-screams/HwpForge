use std::io::Write;

/// Build a minimal valid CFB file with FileHeader + DocInfo + Section0.
pub fn make_test_cfb(doc_info: &[u8], section0: &[u8]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut comp = cfb::CompoundFile::create(cursor).unwrap();

    // FileHeader (256 bytes) — v5.0.2.5, uncompressed
    let mut header_buf = vec![0u8; 256];
    header_buf[..18].copy_from_slice(b"HWP Document File\0");
    let version: u32 = (5 << 24) | (2 << 8) | 5;
    header_buf[32..36].copy_from_slice(&version.to_le_bytes());
    header_buf[36..40].copy_from_slice(&0u32.to_le_bytes()); // flags=0: uncompressed
    let mut stream = comp.create_stream("/FileHeader").unwrap();
    stream.write_all(&header_buf).unwrap();
    drop(stream);

    // DocInfo
    let mut stream = comp.create_stream("/DocInfo").unwrap();
    stream.write_all(doc_info).unwrap();
    drop(stream);

    // BodyText/Section0
    comp.create_storage("/BodyText").unwrap();
    let mut stream = comp.create_stream("/BodyText/Section0").unwrap();
    stream.write_all(section0).unwrap();
    drop(stream);

    comp.into_inner().into_inner()
}

pub fn make_record(tag_id: u16, level: u16, data: &[u8]) -> Vec<u8> {
    let size = data.len() as u32;
    let word = (tag_id as u32) | ((level as u32) << 10) | (size.min(0xFFE) << 20);
    let mut buf = word.to_le_bytes().to_vec();
    if size > 0xFFE {
        let word = (tag_id as u32) | ((level as u32) << 10) | (0xFFF << 20);
        buf = word.to_le_bytes().to_vec();
        buf.extend_from_slice(&size.to_le_bytes());
    }
    buf.extend_from_slice(data);
    buf
}
