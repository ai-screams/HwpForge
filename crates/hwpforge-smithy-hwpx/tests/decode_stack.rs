//! The 1 MiB minimum stack documented on `HwpxDecoder::decode` for optimized
//! builds, checked on the public path with real packages.
//!
//! This lives in an integration test, not in the library's unit tests, so it
//! measures the library as it ships. The unit-test binary compiles the
//! `#[cfg(test)]` code together with the decoder, and with
//! `codegen-units = 1` a unit test that calls the deserializer on an inner
//! type changes how the recursive deserialize chain is inlined: there the
//! same inputs needed about twice the stack (1,329 KiB against 631 KiB for
//! B 44 on macOS arm64), while the shipped decoder did not change.
//!
//! Only optimized builds are covered (an unoptimized build needs more), so
//! the whole file is compiled out of debug builds and CI runs it with
//! `--cargo-profile release` in `Verify › Python`. An overflow aborts the
//! test process, so a failure shows as SIGABRT.
#![cfg(not(debug_assertions))]

use std::io::{Read, Write};
use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxError, PackageReader};

/// The minimum stack `HwpxDecoder::decode` documents for optimized builds.
const DOCUMENTED_MIN_STACK: usize = 1 << 20;
/// The A1 cell run of `tables/table_01_basic_2x2.hwpx`.
const TABLE_ANCHOR: &str = r#"<hp:run charPrIDRef="0"><hp:t>A1</hp:t></hp:run>"#;
/// The drawText run of `images/textbox_anchored.hwpx`.
const TEXTBOX_ANCHOR: &str =
    r#"<hp:run charPrIDRef="0"><hp:t>앵커형 글상자입니다.</hp:t></hp:run>"#;
/// Text placed in the innermost level; a decode that drops it is a silent
/// loss, not a success.
const DEEPEST: &str = "XML_LIMIT_DEEPEST";

fn fixture_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel)
}

fn fixture_section(rel: &str) -> String {
    let bytes = std::fs::read(fixture_path(rel)).expect("read fixture");
    PackageReader::new(&bytes)
        .expect("open fixture package")
        .read_section_xml(0)
        .expect("read section0.xml")
}

/// Byte range of the first `<open …>…</close>` element.
fn first_element(xml: &str, open: &str, close: &str) -> std::ops::Range<usize> {
    let start = xml.find(open).expect("element start");
    let end = xml.find(close).expect("element end") + close.len();
    start..end
}

/// The table fixture's section with its table replaced by `levels` nested
/// levels following `pattern` (`T` = table, `B` = textbox), the outermost
/// level being `pattern[0]`. Real fixture fragments keep the per-level
/// element depth equal to the measured one (T 6, B 5). Same construction as
/// the boundary cases in the decoder's `xml_limit_tests`.
fn nested_section(pattern: &str, levels: usize) -> String {
    let section = fixture_section("tables/table_01_basic_2x2.hwpx");
    let table_span = first_element(&section, "<hp:tbl", "</hp:tbl>");
    let table = &section[table_span.clone()];
    assert_eq!(table.matches(TABLE_ANCHOR).count(), 1, "table anchor");
    let box_section = fixture_section("images/textbox_anchored.hwpx");
    let textbox = &box_section[first_element(&box_section, "<hp:rect", "</hp:rect>")];
    assert_eq!(textbox.matches(TEXTBOX_ANCHOR).count(), 1, "textbox anchor");

    let kinds: Vec<char> = pattern.chars().cycle().take(levels).collect();
    let mut fragment = String::new();
    for (index, kind) in kinds.iter().rev().enumerate() {
        let text = if index == 0 { DEEPEST } else { "outer" };
        let run = format!(r#"<hp:run charPrIDRef="0">{fragment}<hp:t>{text}</hp:t></hp:run>"#);
        fragment = match kind {
            'T' => table.replacen(TABLE_ANCHOR, &run, 1),
            'B' => textbox.replacen(TEXTBOX_ANCHOR, &run, 1),
            other => panic!("unknown nesting kind {other}"),
        };
    }
    format!("{}{}{}", &section[..table_span.start], fragment, &section[table_span.end..])
}

/// `tables/table_01_basic_2x2.hwpx` with `section0.xml` replaced by
/// [`nested_section`]; every other entry is copied as is, so the result is a
/// real package for the public [`HwpxDecoder::decode`] path.
fn nested_package(pattern: &str, levels: usize) -> Vec<u8> {
    let source =
        std::fs::read(fixture_path("tables/table_01_basic_2x2.hwpx")).expect("read fixture");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(source)).expect("open zip");
    let section = nested_section(pattern, levels);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read zip entry");
        if name == "Contents/section0.xml" {
            bytes = section.clone().into_bytes();
        }
        writer.start_file(name, options).expect("start zip entry");
        writer.write_all(&bytes).expect("write zip entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

/// Runs the public `HwpxDecoder::decode` on a thread with `stack` bytes;
/// returns the decoded sections' debug text.
fn decode_package_on_stack(bytes: Vec<u8>, stack: usize) -> Result<String, HwpxError> {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || HwpxDecoder::decode(&bytes).map(|d| format!("{:?}", d.document.sections())))
        .expect("spawn decode thread")
        .join()
        .expect("decode thread panicked")
}

/// B 44 is the deepest text-box input the recursion budget still admits and
/// the input that needs the most stack; B 100 is past the budget.
// 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 10000 으로 올리기 — B 100 이
// 한도에 막히지 않고 1 MiB 스택을 넘쳐 테스트 프로세스가 abort 한다.
// 스택을 512 KiB 로 줄이기 — B 44 가 abort 한다.
#[test]
fn xml_limit_release_decodes_on_documented_1_mib_stack() {
    let decode = |pattern: &str, levels: usize| {
        decode_package_on_stack(nested_package(pattern, levels), DOCUMENTED_MIN_STACK)
    };
    for (pattern, levels) in [("T", 32), ("B", 32), ("TB", 32)] {
        let debug = decode(pattern, levels).expect("32 levels decode on 1 MiB");
        assert!(debug.contains(DEEPEST), "{pattern} x{levels}: deepest text lost");
    }
    assert!(matches!(
        decode("B", 44),
        Err(HwpxError::InvalidStructure { detail }) if detail.starts_with("sublist nesting depth")
    ));
    assert!(matches!(
        decode("B", 100),
        Err(HwpxError::XmlParse { detail, .. }) if detail.contains("recursion limit of 224")
    ));
}
