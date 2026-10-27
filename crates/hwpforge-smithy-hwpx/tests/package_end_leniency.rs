#[path = "support/package_end_leniency.rs"]
mod shared;
use shared::*;
use std::io::{Cursor, Write};

use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};

/// 이것을 실패시키는 것: EOF 정확 일치 복원 / 앞 payload EOCD 중복·개수 제한 복원.
#[test]
fn generated_hwpx_tails_and_payload_have_no_loss() {
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

/// 이것을 실패시키는 것: 다른 물리 CD 거부 삭제·꼬리로 한정 / 실패 CD 증거 삭제·앞 후보로 덮어쓰기 /
/// `directory.rs` 의 `reference.eocd != self.eocd` 거부 삭제(zip 이 여는 B 건너뛴 입력).
#[test]
fn last_raw_other_hwpx_directory_is_rejected_with_or_without_tail() {
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
    // B's EOCD claims one comment byte past EOF, so zip passes over it and
    // opens A's directory without complaint, B left as a tail. Only the
    // decoder's own rule (the last raw end record must be the selected one)
    // refuses the input. (With A's comment covering B the 0.17.0 path accepts
    // A, as before.)
    let b_eocd = input.len() - 22;
    let mut skipped = input.clone();
    skipped[b_eocd + 20..b_eocd + 22].copy_from_slice(&1u16.to_le_bytes());
    skipped[a_end + 20..a_end + 22].copy_from_slice(&0u16.to_le_bytes());
    let archive = zip::ZipArchive::new(Cursor::new(&skipped)).unwrap();
    assert_eq!(archive.central_directory_start(), u64::from(u32_at(&a, a_end + 16)), "zip opens A");
    assert!(HwpxDecoder::decode(&skipped).is_err());
    // Only UnicodePath's missing version/CRC/path is malformed; framing stays
    // intact. zip refuses B's directory here by itself, so this case does not
    // depend on the decoder's rule; it pins that the input stays refused.
    input[a.len() + extra_at..a.len() + extra_at + 4].copy_from_slice(&[0x75, 0x70, 0, 0]);
    for comment_len in [0, b_len] {
        input[a_end + 20..a_end + 22].copy_from_slice(&comment_len.to_le_bytes());
        assert!(HwpxDecoder::decode(&input).is_err(), "A comment length = {comment_len}");
    }
}
