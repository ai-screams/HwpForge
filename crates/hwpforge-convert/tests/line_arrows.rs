//! Hancom fixture 30: 25 objects = 24 outlines + one group container.
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{events::Event, Reader};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

type Attributes = BTreeMap<String, String>;
fn fixture(ext: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../tests/fixtures/shapes/arrows.{ext}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn outlines(bytes: &[u8]) -> (usize, Vec<Attributes>) {
    let xml = PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut count = 0;
    let mut lines = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "line" | "connectLine" | "arc" | "ellipse" | "curve" | "rect" | "polygon"
                | "container" => count += 1,
                "lineShape" => {
                    let mut a: Attributes = e
                        .attributes()
                        .map(|a| {
                            let a = a.unwrap();
                            (a.key.as_ref().to_string(), a.value.to_string())
                        })
                        .collect();
                    a.retain(|k, _| {
                        matches!(
                            k.as_str(),
                            "color"
                                | "width"
                                | "style"
                                | "headStyle"
                                | "tailStyle"
                                | "headSz"
                                | "tailSz"
                                | "headfill"
                                | "tailfill"
                        )
                    });
                    lines.push(a);
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    (count, lines)
}
fn reencode(bytes: &[u8]) -> Vec<u8> {
    let d = HwpxDecoder::decode(bytes).unwrap();
    HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store).unwrap()
}

/// 이것을 실패시키는 것: 모양/크기/채움 비트 이동 오류, 묶음 look 소실,
/// 묶음 색·굵기·선종류 누락, 정렬 선 재생성 뒤 스타일 소실.
#[test]
fn hancom_25_objects_and_24_line_shapes_match() {
    let expected = outlines(&fixture("hwpx"));
    assert_eq!((expected.0, expected.1.len()), (25, 24));
    let (out, warnings) =
        hwp5_to_hwpx_bytes_with_options(&fixture("hwp"), ConvertOptions::default()).unwrap();
    assert!(!warnings.iter().any(|w| format!("{w:?}").contains("shape.arrow")), "{warnings:?}");
    assert_eq!(outlines(&out), expected);
    assert_eq!(outlines(&reencode(&out)), expected);
    assert_eq!(outlines(&reencode(&fixture("hwpx"))), expected);
    let sizes: BTreeSet<_> = expected.1.iter().map(|a| a["tailSz"].as_str()).collect();
    assert_eq!(sizes.len(), 9);
    assert_eq!(expected.1[22]["color"], "#FF0000");
    assert_eq!(expected.1[22]["width"], "141");
    assert_eq!(expected.1[22]["style"], "DASH");
}
fn rewrite(bytes: &[u8], change: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut change = Some(change);
    for i in 0..z.len() {
        let mut entry = z.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if entry.name().unwrap() == "Contents/section0.xml" {
            data = change.take().unwrap()(String::from_utf8(data).unwrap()).into_bytes();
        }
        out.start_file(
            entry.name().unwrap(),
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}
/// 이것을 실패시키는 것: unknown을 Medium/None 기본값으로 조용히 대체,
/// 한쪽 오류로 정상 반대쪽 화살표까지 제거, NORMAL 쪽 미지원 크기의 검증 생략.
#[test]
fn unknown_hwpx_arrow_endpoint_warns_and_is_omitted() {
    for (attr, raw) in [
        ("headStyle", "FUTURE_SHAPE"),
        ("headSz", "FUTURE_SIZE"),
        ("tailStyle", "FUTURE_SHAPE"),
        ("tailSz", "FUTURE_SIZE"),
    ] {
        let input = rewrite(&fixture("hwpx"), |mut xml| {
            let start = xml.find("<hp:lineShape ").unwrap();
            let end = start + xml[start..].find("/>").unwrap() + 2;
            let mut line = xml[start..end].to_string();
            for (key, val) in [("headStyle", "ARROW"), ("tailStyle", "ARROW"), (attr, raw)] {
                let begin = line.find(&format!(" {key}=\"")).unwrap() + key.len() + 3;
                let finish = begin + line[begin..].find('"').unwrap();
                line.replace_range(begin..finish, val);
            }
            xml.replace_range(start..end, &line);
            xml
        });
        let d = HwpxDecoder::decode(&input).unwrap();
        let warnings: Vec<_> = d.warnings.iter().filter(|w| matches!(w,
            hwpforge_smithy_hwpx::DecodeWarning::UnknownEnumValue { attribute, raw: r, fallback }
            if *attribute == format!("lineShape@{attr}") && r == raw && *fallback == "arrow omitted"
        )).collect();
        assert_eq!(warnings.len(), 1, "{attr}: {:?}", d.warnings);
        let lines = outlines(&reencode(&input)).1;
        let side = if attr.starts_with("head") { "headStyle" } else { "tailStyle" };
        let other = if side == "headStyle" { "tailStyle" } else { "headStyle" };
        assert_eq!(lines[0][side], "NORMAL");
        assert_eq!(lines[0][other], "ARROW");
    }
}

/// 이것을 실패시키는 것: NORMAL 쪽 크기를 검증 전에 생략하거나 비기본 값을 잃기.
#[test]
fn absent_arrow_size_and_fill_are_validated_and_preserved() {
    for side in ["head", "tail"] {
        for size in ["LARGE_LARGE", "FUTURE_SIZE"] {
            let input = rewrite(&fixture("hwpx"), |mut xml| {
                let start = xml.find("<hp:lineShape ").unwrap();
                let end = start + xml[start..].find("/>").unwrap() + 2;
                let mut line = xml[start..end].to_string();
                for (key, val) in [
                    (format!("{side}Style"), "NORMAL"),
                    (format!("{side}Sz"), size),
                    (format!("{side}fill"), "0"),
                ] {
                    let begin = line.find(&format!(" {key}=\"")).unwrap() + key.len() + 3;
                    let finish = begin + line[begin..].find('"').unwrap();
                    line.replace_range(begin..finish, val);
                }
                xml.replace_range(start..end, &line);
                xml
            });
            let d = HwpxDecoder::decode(&input).unwrap();
            let warning = d.warnings.iter().any(|w| {
                matches!(w,
                hwpforge_smithy_hwpx::DecodeWarning::UnknownEnumValue { attribute, raw, .. }
                if *attribute == format!("lineShape@{side}Sz") && raw == size)
            });
            assert_eq!(warning, size == "FUTURE_SIZE");
            let lines = outlines(&reencode(&input)).1;
            assert_eq!(lines[0][&format!("{side}Style")], "NORMAL");
            if size == "LARGE_LARGE" {
                assert_eq!(lines[0][&format!("{side}Sz")], size);
                assert_eq!(lines[0][&format!("{side}fill")], "0");
            }
        }
    }
}
