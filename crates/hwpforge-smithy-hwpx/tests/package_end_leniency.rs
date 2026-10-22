//! W1 end-record policy: generated HWPX and existing regression fixtures.
use std::io::{Cursor, Write};

use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};

fn u16_at(bytes: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}

/// 이것을 실패시키는 것: EOF 정확 일치 복원 / 앞 payload EOCD 중복·개수 제한 복원.
#[test]
fn w1_generated_hwpx_tails_and_payload_have_no_loss() {
    let mut document = Document::new();
    let mut section = Section::new(PageSettings::default());
    let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
    paragraph.runs.push(Run::text("W1 합성 문서", CharShapeIndex::new(0)));
    section.paragraphs.push(paragraph);
    document.add_section(section);
    let bytes = HwpxEncoder::encode(
        &document.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    for tail in [b"abcdefghijk".as_slice(), &[0; 11]] {
        let input = [bytes.as_slice(), tail].concat();
        let decoded = HwpxDecoder::decode(&input).unwrap();
        assert!(!decoded.warnings.iter().any(|w| w.is_content_loss()), "{:?}", decoded.warnings);
    }
    // An unused entry contains signatures before the normal final directory.
    for n in [1, 65] {
        let mut input = Vec::new();
        for _ in 0..n {
            input.extend(b"PK\x05\x06");
            input.extend([0; 16]);
            input.extend(if n == 65 { [255; 2] } else { [0; 2] });
        }
        let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes.clone())).unwrap();
        writer
            .start_file(
                "Unused/probe.bin",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(&input).unwrap();
        let input = writer.finish().unwrap().into_inner();
        let decoded = HwpxDecoder::decode(&input).unwrap();
        assert!(!decoded.warnings.iter().any(|w| w.is_content_loss()), "{:?}", decoded.warnings);
    }
}

/// 이것을 실패시키는 것: 다른 물리 CD 거부 삭제·꼬리로 한정 / 실패 CD 증거 삭제·앞 후보로 덮어쓰기.
#[test]
fn w1_last_raw_other_hwpx_directory_is_rejected_with_or_without_tail() {
    let encode = |paragraphs| {
        let mut document = Document::new();
        let mut section = Section::new(PageSettings::default());
        for _ in 0..paragraphs {
            let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
            paragraph.runs.push(Run::text("W1 교차 문서", CharShapeIndex::new(0)));
            section.paragraphs.push(paragraph);
        }
        document.add_section(section);
        HwpxEncoder::encode(
            &document.validate().unwrap(),
            &style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap()
    };
    let a = encode(2);
    let mut b = encode(1);
    assert_eq!(HwpxDecoder::decode(&a).unwrap().document.sections()[0].paragraphs.len(), 2);
    assert_eq!(HwpxDecoder::decode(&b).unwrap().document.sections()[0].paragraphs.len(), 1);
    let a_end = a.len() - 22;
    let b_end = b.len() - 22;
    let b_cd = u32_at(&b, b_end + 16) as usize;
    // B remains a complete HWPX: rebase every local offset to the combined input.
    let mut p = b_cd;
    while p < b_end {
        assert_eq!(&b[p..p + 4], b"PK\x01\x02");
        let offset = u32_at(&b, p + 42) + a.len() as u32;
        b[p + 42..p + 46].copy_from_slice(&offset.to_le_bytes());
        p += 46
            + usize::from(u16_at(&b, p + 28))
            + usize::from(u16_at(&b, p + 30))
            + usize::from(u16_at(&b, p + 32));
    }
    assert_eq!(p, b_end);
    let extra_at = b_cd + 46 + usize::from(u16_at(&b, b_cd + 28));
    let extra_len = u16_at(&b, b_cd + 30) + 4;
    b[b_cd + 30..b_cd + 32].copy_from_slice(&extra_len.to_le_bytes());
    let cd_size = u32_at(&b, b_end + 12) + 4;
    b[b_end + 12..b_end + 16].copy_from_slice(&cd_size.to_le_bytes());
    b[b_end + 16..b_end + 20].copy_from_slice(&((a.len() + b_cd) as u32).to_le_bytes());
    // Start with a valid unknown extra to verify B's payload/CRC and document.
    b.splice(extra_at..extra_at, [0; 4]);
    let b_len = u16::try_from(b.len()).unwrap();
    let mut input = [a.as_slice(), b.as_slice()].concat();
    let mut archive = zip::ZipArchive::new(Cursor::new(&input)).unwrap();
    assert_eq!(archive.central_directory_start(), (a.len() + b_cd) as u64);
    for i in 0..archive.len() {
        std::io::copy(&mut archive.by_index(i).unwrap(), &mut std::io::sink()).unwrap();
    }
    let decoded = HwpxDecoder::decode(&input).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs.len(), 1);
    // Only UnicodePath's missing version/CRC/path is malformed; framing stays intact.
    input[a.len() + extra_at..a.len() + extra_at + 4].copy_from_slice(&[0x75, 0x70, 0, 0]);
    for comment_len in [0, b_len] {
        input[a_end + 20..a_end + 22].copy_from_slice(&comment_len.to_le_bytes());
        let fallback = zip::ZipArchive::new(Cursor::new(&input)).unwrap();
        assert_eq!(fallback.central_directory_start(), u64::from(u32_at(&a, a_end + 16)));
        assert!(HwpxDecoder::decode(&input).is_err(), "A comment length = {comment_len}");
    }
}

/// 이것을 실패시키는 것: 거부 전에 참조 디렉터리를 읽을 수 있어야 한다는 조건 추가.
#[test]
fn w1_reference_position_comment_and_cd_padding_cross_product() {
    // 이것을 실패시키는 것: 기존 경로의 다중 적격 후보 판정을 생략.
    let a = include_bytes!("../../../tests/fixtures/user_samples/text/sample-fwspace.hwpx");
    let b = include_bytes!("../../../tests/fixtures/user_samples/text/sample-empty.hwpx");
    let a_end = a.len() - 22;
    let mut input = a.to_vec();
    // An unreadable candidate between A and B must not replace reference evidence.
    input.extend(b"PK\x05\x06");
    input.extend([0; 4]);
    input.extend([1, 0, 1, 0]);
    input.extend(46u32.to_le_bytes());
    input.extend(((a.len() + 1) as u32).to_le_bytes());
    input.extend([0; 2]);
    let b_base = input.len();
    input.extend(b);
    let b_end = input.len() - 22;
    let b_cd = b_base + u32_at(&input, b_end + 16) as usize;
    let mut p = b_cd;
    while p < b_end {
        assert_eq!(&input[p..p + 4], b"PK\x01\x02");
        let offset = u32_at(&input, p + 42) + b_base as u32;
        input[p + 42..p + 46].copy_from_slice(&offset.to_le_bytes());
        p += 46
            + usize::from(u16_at(&input, p + 28))
            + usize::from(u16_at(&input, p + 30))
            + usize::from(u16_at(&input, p + 32));
    }
    assert_eq!(p, b_end);
    input[b_end + 16..b_end + 20].copy_from_slice(&(b_cd as u32).to_le_bytes());
    assert_eq!((input.len(), a_end, b_cd, b_end), (49_841, 25_444, 49_118, 49_819));
    // Verify all B payloads and CRCs before changing either comment field.
    let mut archive = zip::ZipArchive::new(Cursor::new(&input)).unwrap();
    assert_eq!(archive.central_directory_start(), b_cd as u64);
    for i in 0..archive.len() {
        std::io::copy(&mut archive.by_index(i).unwrap(), &mut std::io::sink()).unwrap();
    }
    for padding in [b"".as_slice(), &[0], b"PK\x05\x05\0\0"] {
        let mut padded = input.clone();
        let cd_size = u32_at(&padded, b_end + 12) + padding.len() as u32;
        padded[b_end + 12..b_end + 16].copy_from_slice(&cd_size.to_le_bytes());
        padded.splice(b_end..b_end, padding.iter().copied());
        let padded_end = b_end + padding.len();
        for a_comment in [0u16, 24_375 + padding.len() as u16] {
            for b_comment in [0u16, 1] {
                padded[a_end + 20..a_end + 22].copy_from_slice(&a_comment.to_le_bytes());
                padded[padded_end + 20..padded_end + 22].copy_from_slice(&b_comment.to_le_bytes());
                let selected = zip::ZipArchive::new(Cursor::new(&padded)).unwrap();
                let expected_cd =
                    if b_comment == 0 { b_cd } else { u32_at(a, a_end + 16) as usize };
                assert_eq!(selected.central_directory_start(), expected_cd as u64);
                // Padding still violates the existing selected-CD framing contract.
                let result = HwpxDecoder::decode(&padded);
                assert_eq!(
                    result.is_ok(),
                    b_comment == 0 && padding.is_empty(),
                    "padding={padding:?}, A={a_comment}, B={b_comment}"
                );
                if let Ok(decoded) = result {
                    assert_eq!(decoded.document.sections()[0].paragraphs.len(), 3);
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: count sentinel이면 신규 경로의 참조 해석 대조 생략.
#[test]
fn w1_count_sentinel_other_document_in_zip64_extension() {
    let a = include_bytes!("../../../tests/fixtures/user_samples/text/sample-fwspace.hwpx");
    let b = include_bytes!("../../../tests/fixtures/user_samples/text/sample-empty.hwpx");
    let ae = a.len() - 22;
    let be = b.len() - 22;
    let acd = u32_at(a, ae + 16) as usize;
    let bcd = u32_at(b, be + 16) as usize;
    let mut bytes = a[..acd].to_vec();
    let mut cd = a[acd..ae].to_vec();
    // Unique unused entries make A's directory large enough for B's 65,535
    // literal reference records. No duplicate names in the selected A directory.
    for i in 0..64 {
        let name = format!("Unused/r4-{i:02}.bin");
        let local = bytes.len() as u32;
        let mut lfh = [0u8; 30];
        lfh[..4].copy_from_slice(b"PK\x03\x04");
        lfh[4..6].copy_from_slice(&20u16.to_le_bytes());
        lfh[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend(lfh);
        bytes.extend(name.as_bytes());
        let mut header = [0u8; 46];
        header[..4].copy_from_slice(b"PK\x01\x02");
        header[4..6].copy_from_slice(&20u16.to_le_bytes());
        header[6..8].copy_from_slice(&20u16.to_le_bytes());
        header[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        header[32..34].copy_from_slice(&u16::MAX.to_le_bytes());
        header[42..46].copy_from_slice(&local.to_le_bytes());
        cd.extend(header);
        cd.extend(name.as_bytes());
        cd.extend(vec![b'x'; u16::MAX as usize]);
    }
    let start = bytes.len();
    let size = cd.len();
    bytes.extend(cd);
    let zip64 = bytes.len();
    let extension_len = size - 20 + bcd;
    let reference_base = extension_len + 76;
    let b_local = zip64 + 56;
    let reference_start = b_local + bcd;
    let mut b_records = b[bcd..be].to_vec();
    let mut p = 0;
    let mut first_end = 0;
    while p < b_records.len() {
        let local = b_local + u32_at(&b_records, p + 42) as usize - reference_base;
        b_records[p + 42..p + 46].copy_from_slice(&(local as u32).to_le_bytes());
        p += 46
            + usize::from(u16_at(&b_records, p + 28))
            + usize::from(u16_at(&b_records, p + 30))
            + usize::from(u16_at(&b_records, p + 32));
        if first_end == 0 {
            first_end = p;
        }
    }
    let first = b_records[..first_end].to_vec();
    for _ in 11..65_535 {
        b_records.extend(&first);
    }
    assert!(bcd + b_records.len() <= extension_len);
    bytes.extend(b"PK\x06\x06");
    bytes.extend((44 + extension_len as u64).to_le_bytes());
    bytes.extend([45, 0, 45, 0]);
    bytes.extend([0; 8]);
    for value in [75u64, 75, size as u64, start as u64] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(&b[..bcd]);
    bytes.extend(&b_records);
    bytes.resize(zip64 + 56 + extension_len, 0);
    bytes.extend(b"PK\x06\x07");
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((zip64 as u64).to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    let eocd = bytes.len();
    let mut footer = a[ae..].to_vec();
    footer[8..12].fill(0xff);
    footer[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    footer[16..20].copy_from_slice(&(start as u32).to_le_bytes());
    bytes.extend(footer);
    assert_eq!(eocd - size, reference_start);
    assert_eq!(eocd - size - start, reference_base);
    assert_ne!(reference_start, start);
    let selected = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(
        (selected.len(), selected.offset(), selected.central_directory_start()),
        (75, 0, start as u64)
    );
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs.len(), 1);
    // All B locals are copied verbatim; every reference record must point to
    // their original name, compressed size and CRC. Decode all B payloads below.
    assert_eq!(&bytes[b_local..reference_start], &b[..bcd]);
    let mut records = reference_start;
    for _ in 0..65_535 {
        assert_eq!(&bytes[records..records + 4], b"PK\x01\x02");
        let local = reference_base + u32_at(&bytes, records + 42) as usize;
        let name_len = usize::from(u16_at(&bytes, records + 28));
        assert_eq!(&bytes[records + 16..records + 28], &bytes[local + 14..local + 26]);
        assert_eq!(&bytes[local..local + 4], b"PK\x03\x04");
        assert_eq!(
            &bytes[local + 30..local + 30 + name_len],
            &bytes[records + 46..records + 46 + name_len]
        );
        records += 46
            + name_len
            + usize::from(u16_at(&bytes, records + 30))
            + usize::from(u16_at(&bytes, records + 32));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(b.as_slice())).unwrap();
    for i in 0..archive.len() {
        std::io::copy(&mut archive.by_index(i).unwrap(), &mut std::io::sink()).unwrap();
    }
    assert_eq!(HwpxDecoder::decode(b).unwrap().document.sections()[0].paragraphs.len(), 3);
    bytes.extend([0; 11]);
    assert!(HwpxDecoder::decode(&bytes).is_err());
}
