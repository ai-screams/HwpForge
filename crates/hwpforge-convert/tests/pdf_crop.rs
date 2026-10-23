//! A picture whose crop could not be read is not drawn whole (§19.21 H-v4,
//! review r1 of P18). `real_crop_vs_original_two_objects.hwpx` is rewritten
//! so its cropped picture has no `<hp:imgDim>` and a clip that reads validly
//! in both orders: the decoder reports `picture_crop_order` and keeps no
//! crop, so the PDF must refuse rather than show the whole image.

use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
use hwpforge_smithy_hwpx::HwpxDecoder;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn with_section(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == "Contents/section0.xml" {
            data = edit(&String::from_utf8(data).expect("utf-8")).into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

const CROP: &str = r#"<hp:imgClip left="54464" right="151936" top="21128" bottom="115200"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="206400" dimheight="115200"/>"#;
const TWO_WAY: &str = r#"<hp:imgClip left="0" right="5618" top="7260" bottom="9036"/><hp:inMargin left="0" right="0" top="0" bottom="0"/>"#;

fn refused_for_the_crop(bytes: &[u8]) -> bool {
    to_pdf(bytes, &ToPdfOptions::default())
        .err()
        .is_some_and(|e| e.to_string().contains("picture whose crop could not be read"))
}

/// The two-way clip is reported lost and the PDF refuses the document; the
/// untouched fixture is not refused for its crop.
///
/// 이것을 실패시키는 것: `to_pdf` 의 자르기 손실 검사를 지우기(통째 그림으로
/// 렌더 성공).
#[test]
fn crop_order_loss_is_refused() {
    let source = fixture("images/real_crop_vs_original_two_objects.hwpx");
    let edited = with_section(&source, |xml| {
        assert_eq!(xml.matches(CROP).count(), 1, "fixture crop");
        xml.replacen(CROP, TWO_WAY, 1)
    });
    let decoded = HwpxDecoder::decode(&edited).expect("decode");
    assert!(
        decoded.warnings.iter().any(|w| format!("{w:?}").contains("picture_crop_order")),
        "{:?}",
        decoded.warnings
    );
    assert!(refused_for_the_crop(&edited));
    assert!(!refused_for_the_crop(&source));
}
