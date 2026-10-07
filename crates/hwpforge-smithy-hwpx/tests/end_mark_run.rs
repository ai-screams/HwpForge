//! The paragraph-end mark's char shape (§19.22 D5, S17).
//!
//! Hancom writes it as an empty last run (`<hp:run charPrIDRef="N"/>`) of a
//! paragraph with two or more runs. The decoder dropped that run, so a
//! re-saved document lost it (the height of the paragraph's last line).
//! Inputs are repo fixtures; the footnote case adds an end run to a
//! footnote paragraph of one.

use std::io::{Cursor, Read, Write};

use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxPatcher};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        let body = if name == "Contents/section0.xml" { xml.as_bytes() } else { &data };
        out.write_all(body).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// The char shapes of the empty last runs of paragraphs with two or more
/// runs, in document order.
fn end_runs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<hp:p ") {
        let body_start = at + rest[at..].find('>').unwrap() + 1;
        // The paragraph's own runs: stop at its linesegarray or its end tag,
        // skipping nested paragraphs.
        let mut depth = 0usize;
        let mut i = body_start;
        let mut runs: Vec<(usize, usize)> = Vec::new();
        loop {
            let next_p = rest[i..].find("<hp:p ").map(|n| i + n);
            let next_end = rest[i..].find("</hp:p>").map(|n| i + n).unwrap();
            let next_run = rest[i..].find("<hp:run ").map(|n| i + n);
            let candidates = [next_p, Some(next_end), next_run];
            let n = candidates.iter().flatten().min().copied().unwrap();
            if Some(n) == next_p {
                depth += 1;
                i = n + 6;
            } else if n == next_end {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i = n + 7;
            } else {
                if depth == 0 {
                    runs.push((n, n + rest[n..].find('>').unwrap() + 1));
                }
                i = n + 8;
            }
        }
        if runs.len() >= 2 {
            let (s, e) = *runs.last().unwrap();
            let tag = &rest[s..e];
            let after = &rest[e..];
            let empty = tag.ends_with("/>")
                || after.starts_with("<hp:t/></hp:run>")
                || after.starts_with("</hp:run>");
            if empty {
                let cs = tag.split("charPrIDRef=\"").nth(1).unwrap().split('"').next().unwrap();
                out.push(cs.to_string());
            }
        }
        rest = &rest[body_start..];
    }
    out
}

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

/// End runs reach `Paragraph::end_char_shape` — in body paragraphs and in
/// table cells — and come back out of the encoder at the same places with
/// the same char shapes.
///
/// 이것을 실패시키는 것: 디코더가 끝 run 을 읽지 않기 / 인코더가 쓰지 않기.
#[test]
fn end_mark_run_round_trips() {
    for path in [
        "user_samples/sample-char-breakwordlatin-variants.hwpx",
        "tables/table_20_real_world_ministry_stress.hwpx",
    ] {
        let source = fixture(path);
        let before = end_runs(&section_xml(&source));
        assert!(!before.is_empty(), "{path}");
        let doc = HwpxDecoder::decode(&source).expect("decode");
        let mut carried = Vec::new();
        for p in &doc.document.sections()[0].paragraphs {
            p.for_each_paragraph(|q| {
                if let Some(c) = q.end_char_shape {
                    carried.push(c.get().to_string());
                }
            });
        }
        assert_eq!(carried, before, "{path}");
        assert_eq!(end_runs(&section_xml(&reencode(&source))), before, "{path}");
    }
}

/// A preserving text edit of a paragraph with an end run — in the body, in
/// a table cell and in a footnote — succeeds and keeps every end run.
///
/// 이것을 실패시키는 것: 끝 run 을 Core run 으로 남기기(슬롯 수가 어긋나 보존
/// 편집 실패) / 패치 출력에서 끝 run 을 잃기.
#[test]
fn preserving_edits_keep_end_mark_runs() {
    let footnote_source = {
        let source = fixture("structural/crossref_para.hwpx");
        let xml = section_xml(&source);
        let old = "<hp:t> 각주 내용 1</hp:t></hp:run><hp:linesegarray>";
        assert_eq!(xml.matches(old).count(), 1);
        with_section(
            &source,
            &xml.replacen(
                old,
                "<hp:t> 각주 내용 1</hp:t></hp:run><hp:run charPrIDRef=\"0\"/><hp:linesegarray>",
                1,
            ),
        )
    };
    let cases: [(&str, Vec<u8>); 3] = [
        ("body", fixture("user_samples/sample-char-breakwordlatin-variants.hwpx")),
        ("cell", fixture("tables/table_20_real_world_ministry_stress.hwpx")),
        ("footnote", footnote_source),
    ];
    for (name, base) in cases {
        let before = end_runs(&section_xml(&base));
        assert!(!before.is_empty(), "{name}");
        let decoded = HwpxDecoder::decode(&base).expect("decode");
        let original = decoded.document.sections()[0].clone();
        let mut replacement = original.clone();
        // Edit the first text run of the first paragraph that has an end mark.
        let mut edited = false;
        for p in &mut replacement.paragraphs {
            p.for_each_paragraph_mut(|q| {
                if edited || q.end_char_shape.is_none() {
                    return;
                }
                if let Some(RunContent::Text(t)) = q
                    .runs
                    .iter_mut()
                    .map(|r| &mut r.content)
                    .find(|c| matches!(c, RunContent::Text(t) if !t.is_empty()))
                {
                    t.push_str(" 고침");
                    edited = true;
                }
            });
        }
        assert!(edited, "{name}");
        let preservation =
            HwpxPatcher::export_section_preservation(&base, 0, &original).expect("preservation");
        let patched = HwpxPatcher::patch_section_preserving(
            &base,
            0,
            &replacement,
            None,
            Some(&preservation),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let xml = section_xml(&patched);
        assert!(xml.contains(" 고침"), "{name}");
        assert_eq!(end_runs(&xml), before, "{name}");
    }
}

/// An end run holding one empty `<hp:t/>` (Hancom writes both forms) is the
/// end mark too.
///
/// 이것을 실패시키는 것: 빈 `<hp:t/>` 형을 끝 run 으로 보지 않기.
#[test]
fn empty_text_end_run_is_an_end_mark() {
    let source = fixture("user_samples/sample-char-breakwordlatin-variants.hwpx");
    let xml = section_xml(&source);
    let form = "<hp:run charPrIDRef=\"0\"/>";
    assert!(xml.contains(form));
    let input =
        with_section(&source, &xml.replace(form, "<hp:run charPrIDRef=\"0\"><hp:t/></hp:run>"));
    let count = |bytes: &[u8]| {
        let doc = HwpxDecoder::decode(bytes).expect("decode");
        let mut n = 0;
        for p in &doc.document.sections()[0].paragraphs {
            p.for_each_paragraph(|q| n += usize::from(q.end_char_shape.is_some()));
        }
        n
    };
    assert_eq!(count(&input), count(&source));
    assert!(count(&input) > 0);
}

fn end_mark_count(bytes: &[u8]) -> usize {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let mut n = 0;
    for p in &doc.document.sections()[0].paragraphs {
        p.for_each_paragraph(|q| n += usize::from(q.end_char_shape.is_some()));
    }
    n
}

/// A last run whose `<hp:t>` holds only a marker — a highlight end (also
/// after a space), a track-change mark, a title mark — is not an end mark
/// (review r1: the decoder drops the marker, which made the run look empty).
///
/// 이것을 실패시키는 것: 표지만 든 `<hp:t>` 를 끝 run 으로 보기.
#[test]
fn marker_runs_are_not_end_marks() {
    let source = fixture("user_samples/sample-char-breakwordlatin-variants.hwpx");
    let xml = section_xml(&source);
    // The end runs: an empty run right before the paragraph's line cache.
    let form = "<hp:run charPrIDRef=\"0\"/><hp:linesegarray>";
    let all = xml.matches(form).count();
    assert!(all > 0);
    // With those runs removed, only the other end marks remain.
    let others = end_mark_count(&with_section(&source, &xml.replace(form, "<hp:linesegarray>")));
    assert!(others < end_mark_count(&source));
    for marker in [
        "<hp:markpenEnd/>",
        " <hp:markpenEnd/>",
        "<hp:deleteBegin Id=\"1\" TcId=\"2\"/>",
        "<hp:titleMark ignore=\"1\"/>",
    ] {
        let edited = xml.replace(
            form,
            &format!("<hp:run charPrIDRef=\"0\"><hp:t>{marker}</hp:t></hp:run><hp:linesegarray>"),
        );
        assert_eq!(end_mark_count(&with_section(&source, &edited)), others, "{marker}");
    }
}

/// An end run's char shape reference is bounded like every run's — in a
/// table cell, whose paragraphs only the run conversion checked (review r1).
///
/// 이것을 실패시키는 것: 끝 run 의 참조 검사를 지우기.
#[test]
fn end_run_reference_is_bounded() {
    let source = fixture("tables/table_20_real_world_ministry_stress.hwpx");
    let xml = section_xml(&source);
    // The only occurrence: a cell paragraph's end run.
    let end_run = "</hp:run><hp:run charPrIDRef=\"13\"/><hp:linesegarray>";
    assert_eq!(xml.matches(end_run).count(), 1);
    let with_ref = |id: u32| {
        with_section(
            &source,
            &xml.replace(
                end_run,
                &format!("</hp:run><hp:run charPrIDRef=\"{id}\"/><hp:linesegarray>"),
            ),
        )
    };
    assert!(HwpxDecoder::decode(&with_ref(100_001)).is_err());
    assert!(HwpxDecoder::decode(&with_ref(100_000)).is_ok());
}
