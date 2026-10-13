//! W0 G-det (b)(c): in one process, converting the same file again — and
//! again after converting another file — gives the same bytes, with and
//! without the layout cache (nextest runs each test in its own process,
//! so the repeats happen inside the test).

use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn convert(bytes: &[u8], carry: bool) -> Vec<u8> {
    hwp5_to_hwpx_bytes_with_diagnostics(
        bytes,
        ConvertOptions::default().with_carry_layout_cache(carry),
    )
    .expect("convert")
    .0
}

/// 이것을 실패시키는 것: 프로세스 전역 id 카운터로 되돌리기(두 번째 A 가 다른
/// id).
#[test]
fn repeat_and_interleaved_conversions_match() {
    let inputs = [
        "images/img_03_two_images_png_jpg.hwp",
        "user_samples/sample-memo-basic.hwp",
        "user_samples/sample-memo-multiple.hwp",
        "charts/chart_01_single_column.hwp",
    ];
    for carry in [false, true] {
        let first: Vec<Vec<u8>> = inputs.iter().map(|n| convert(&fixture(n), carry)).collect();
        // (b) the same file again, straight after.
        for (name, bytes) in inputs.iter().zip(&first) {
            assert_eq!(&convert(&fixture(name), carry), bytes, "{name} carry={carry} again");
        }
        // (c) A after B: every file once more, in reverse order.
        for (name, bytes) in inputs.iter().zip(&first).rev() {
            assert_eq!(
                &convert(&fixture(name), carry),
                bytes,
                "{name} carry={carry} after another"
            );
        }
        // On another thread.
        let name = inputs[0];
        let again = std::thread::spawn(move || convert(&fixture(name), carry)).join().unwrap();
        assert_eq!(again, first[0], "{name} carry={carry} other thread");
    }
}

/// Q8: a converted memo carries the file's last-saved time (its summary
/// information), not the time of the conversion.
/// 이것을 실패시키는 것: HWP5 메모 시각 채우기 삭제(epoch 가 나온다).
#[test]
fn converted_memo_keeps_the_files_last_saved_time() {
    let out = convert(&fixture("user_samples/sample-memo-basic.hwp"), false);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(out)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Contents/section0.xml").unwrap(), &mut xml)
        .unwrap();
    let tag = r#"<hp:stringParam name="CreateDateTime">"#;
    let at = xml.find(tag).expect("CreateDateTime") + tag.len();
    let written = &xml[at..at + xml[at..].find('<').unwrap()];
    // Read from the stream's 0x0D (last saved) with `olefile`, not this
    // parser; its 0x0C (created) is 2026-05-29T14:09:24Z.
    assert_eq!(written, "2026-09-26T02:31:10Z");
    let doc = hwpforge_smithy_hwp5::decode_hwp5_with_images(&fixture(
        "user_samples/sample-memo-basic.hwp",
    ))
    .unwrap();
    // The image-decoding entry point fills it too.
    let mut memo_times = Vec::new();
    doc.document.for_each_paragraph(|p| {
        for run in &p.runs {
            if let hwpforge_core::run::RunContent::Control(c) = &run.content {
                if let hwpforge_core::Control::Memo { metadata, .. } = c.as_ref() {
                    memo_times.push(metadata.create_datetime.clone());
                }
            }
        }
    });
    assert!(!memo_times.is_empty());
    assert!(memo_times.iter().all(|t| t == written), "{memo_times:?}");
}
