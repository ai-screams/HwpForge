//! Public decode and editing paths must bound quick-xml's replay buffer.
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read, Write};

use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
use hwpforge::ops::inspect::{inspect, InspectOptions};
use hwpforge::ops::OpsError;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

// Independent boundary oracle: do not derive input sizes from the production
// constant, or raising that constant would silently enlarge the attack too.
const LIMIT: usize = 65_536;

/// Exactly `events` replay events: start/end junk, two per empty x, and an
/// optional text event. The largest attack is LIMIT + 1, even with no guard.
fn junk(events: usize) -> String {
    format!(
        "<junk>{}{}</junk>",
        "<x/>".repeat((events - 2) / 2),
        if events % 2 == 1 { "synthetic" } else { "" }
    )
}

fn base() -> Vec<u8> {
    base_with_paragraphs(&["가상 문장 하나", "가상 문장 둘"])
}

fn base_with_paragraphs(texts: &[&str]) -> Vec<u8> {
    let mut document = Document::new();
    let paragraphs = texts
        .iter()
        .map(|&text| {
            let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
            paragraph.runs.push(Run::text(text, CharShapeIndex::new(0)));
            paragraph
        })
        .collect();
    document.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    HwpxEncoder::encode(
        &document.validate().unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn interleave(base: &[u8], header: bool, events: usize) -> Vec<u8> {
    let part = if header { "Contents/header.xml" } else { "Contents/section0.xml" };
    rewrite_part(base, part, |xml| {
        // Put one large subtree between the first two list entries.
        let anchor = if header { "</hh:fontface>" } else { "</hp:p>" };
        assert!(xml.matches(anchor).count() >= 2);
        xml.replacen(anchor, &format!("{anchor}{}", junk(events)), 1)
    })
}

fn rewrite_part(base: &[u8], part: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == part {
            let xml = String::from_utf8(bytes).unwrap();
            bytes = edit(&xml).into_bytes();
        }
        target
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        target.write_all(&bytes).unwrap();
    }
    target.finish().unwrap().into_inner()
}

fn assert_rejected(error: OpsError, part: &str, code: &str) {
    assert_eq!(error.code().as_str(), code, "{error}");
    let message = error.to_string();
    assert!(message.contains(part), "{message}");
    assert!(
        message.contains("more than 65536 buffered XML events; HwpForge does not read documents that require more"),
        "{message}"
    );
    // An error is returned normally, and another decode in the same process
    // succeeds. Panic/abort or a poisoned decoder cannot satisfy this.
    inspect(&base(), &InspectOptions::default()).expect("process remains usable");
}

// 이것을 실패시키는 것: XML_EVENT_BUFFER_LIMIT 를 4096 으로 되돌리기.
#[test]
fn header_event_buffer_boundary_passes() {
    let base = base();
    for events in [LIMIT - 1, LIMIT] {
        let bytes = interleave(&base, true, events);
        inspect(&bytes, &InspectOptions::default()).expect("header boundary");
    }
}

// 이것을 실패시키는 것: event_buffer_size 호출 삭제, 한도 131072 로 올리기,
// 또는 xml_error_detail 의 TooManyEvents 전용 문구 삭제.
#[test]
fn header_event_buffer_overflow_is_decode_failed() {
    let bytes = interleave(&base(), true, LIMIT + 1);
    assert_rejected(
        inspect(&bytes, &InspectOptions::default()).unwrap_err(),
        "header.xml",
        "DECODE_FAILED",
    );
}

// 이것을 실패시키는 것: XML_EVENT_BUFFER_LIMIT 를 4096 으로 되돌리기.
#[test]
fn section_event_buffer_boundary_passes() {
    let base = base();
    for events in [LIMIT - 1, LIMIT] {
        let bytes = interleave(&base, false, events);
        let decoded = HwpxDecoder::decode(&bytes).expect("section boundary");
        assert_eq!(decoded.document.sections()[0].paragraphs.len(), 2);
    }
}

// 이것을 실패시키는 것: event_buffer_size 호출 삭제, 한도 131072 로 올리기,
// 또는 xml_error_detail 의 TooManyEvents 전용 문구 삭제.
#[test]
fn section_event_buffer_overflow_is_decode_failed() {
    let bytes = interleave(&base(), false, LIMIT + 1);
    assert_rejected(
        inspect(&bytes, &InspectOptions::default()).unwrap_err(),
        "section0.xml",
        "DECODE_FAILED",
    );
}

fn patch_options(bytes: &[u8]) -> PatchOptions {
    let exported = export_section(bytes, &ExportSectionOptions::default()).unwrap();
    PatchOptions::default().with_patch(serde_json::to_string(&exported.section).unwrap())
}

// 이것을 실패시키는 것: XML_EVENT_BUFFER_LIMIT 를 4096 으로 되돌리기.
#[test]
fn patch_event_buffer_boundary_passes() {
    let base = base();
    for events in [LIMIT - 1, LIMIT] {
        let bytes = interleave(&base, false, events);
        let options = patch_options(&bytes);
        let patched = patch(&bytes, &options).expect("patch re-decodes the boundary base");
        assert_eq!(
            HwpxDecoder::decode(&patched.bytes).unwrap().document,
            HwpxDecoder::decode(&bytes).unwrap().document
        );
    }
}

// 이것을 실패시키는 것: event_buffer_size 호출 삭제, 한도 131072 로 올리기,
// 또는 xml_error_detail 의 TooManyEvents 전용 문구 삭제.
#[test]
fn patch_event_buffer_overflow_keeps_patch_failed() {
    let base = base();
    let options = patch_options(&base);
    let bytes = interleave(&base, false, LIMIT + 1);
    // Options come from a clean base so export cannot reject the attack before
    // patch is called. Decode must reject before the stale-preservation check.
    assert_rejected(patch(&bytes, &options).unwrap_err(), "section0.xml", "PATCH_FAILED");
}

// 이것을 실패시키는 것: XML_EVENT_BUFFER_LIMIT 를 4096 으로 되돌리기.
#[test]
fn single_paragraph_with_2048_line_segments_passes() {
    let sentence = "가상 문장입니다. ";
    let text = sentence.repeat(2048);
    let base = base_with_paragraphs(&[&text]);
    // The encoder prefixes secPr and colPr, each occupying 8 wire units.
    let lines: String = (0..2048)
        .map(|i| format!(r#"<hp:lineseg textpos="{}" vertpos="{}" vertsize="1000" textheight="1000" baseline="800" spacing="600" horzpos="0" horzsize="40000" flags="393216"/>"#, 16 + i * sentence.encode_utf16().count(), i * 1600))
        .collect();
    // The array after run requires 2 + 2 * 2048 = 4098 replay events.
    let bytes = rewrite_part(&base, "Contents/section0.xml", |xml| {
        assert!(!xml.contains("<hp:linesegarray"));
        xml.replacen("</hp:p>", &format!("<hp:linesegarray>{lines}</hp:linesegarray></hp:p>"), 1)
    });
    let decoded = HwpxDecoder::decode(&bytes).expect("long single paragraph must remain readable");
    let paragraphs = &decoded.document.sections()[0].paragraphs;
    assert_eq!(paragraphs.len(), 1);
    assert_eq!(paragraphs[0].text_content(), text);
    assert!(paragraphs[0].layout_cache.is_some(), "{:?}", decoded.warnings);
    assert_eq!(paragraphs[0].layout_cache.as_ref().unwrap().lines.len(), 2048);
}
