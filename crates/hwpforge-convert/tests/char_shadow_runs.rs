//! Character shadows by run (S21 7.5): the shadow each character is drawn
//! with, in the converted HWPX and in the HWPX Hancom saved from the same
//! HWP5 file. Definition equality alone would pass a converter that keeps
//! every `charPr` right but points runs at the wrong one; this follows each
//! character's run to its `charPrIDRef` and that `charPr`'s `<hh:shadow>`.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use quick_xml::events::Event;
use quick_xml::Reader;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn parts(hwpx: &[u8]) -> (String, Vec<String>) {
    let mut zip = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut read = |n: &str| {
        let mut xml = String::new();
        zip.by_name(n).unwrap().read_to_string(&mut xml).unwrap();
        xml
    };
    let header = read("Contents/header.xml");
    let mut sections = Vec::new();
    for i in 0.. {
        let name = format!("Contents/section{i}.xml");
        if zip::ZipArchive::new(Cursor::new(hwpx)).unwrap().by_name(&name).is_err() {
            break;
        }
        sections.push(read(&name));
    }
    (header, sections)
}

/// `charPr` id → its `<hh:shadow …/>` element (absent = no entry).
fn shadow_by_char_pr(header: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = header;
    while let Some(at) = rest.find("<hh:charPr ") {
        let block = &rest[at..at + rest[at..].find("</hh:charPr>").expect("end")];
        let id_at = block.find(" id=\"").expect("id") + 5;
        let id = block[id_at..id_at + block[id_at..].find('"').unwrap()].to_string();
        if let Some(s) = block.find("<hh:shadow ") {
            out.insert(id, block[s..s + block[s..].find("/>").unwrap() + 2].to_string());
        }
        rest = &rest[at + block.len()..];
    }
    out
}

/// Every character inside `<hp:t>`, in document order, with the shadow of
/// its nearest enclosing run (`None` when that `charPr` has no element).
fn shadow_per_char(hwpx: &[u8]) -> Vec<(char, Option<String>)> {
    let (header, sections) = parts(hwpx);
    let shadows = shadow_by_char_pr(&header);
    let mut out = Vec::new();
    for xml in sections {
        let mut reader = Reader::from_str(&xml);
        let mut runs: Vec<String> = Vec::new();
        let mut in_t = 0usize;
        loop {
            match reader.read_event().expect("xml") {
                Event::Start(e) if e.name().as_ref() == "hp:run" => {
                    let id = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "charPrIDRef")
                        .map(|a| a.value.to_string())
                        .unwrap_or_default();
                    runs.push(id);
                }
                Event::End(e) if e.name().as_ref() == "hp:run" => {
                    runs.pop();
                }
                Event::Start(e) if e.name().as_ref() == "hp:t" => in_t += 1,
                Event::End(e) if e.name().as_ref() == "hp:t" => in_t -= 1,
                Event::Text(t) if in_t > 0 => {
                    let text: &str = t.as_ref();
                    let shadow = runs.last().and_then(|id| shadows.get(id)).cloned();
                    out.extend(
                        text.chars().filter(|c| !c.is_control()).map(|c| (c, shadow.clone())),
                    );
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    out
}

fn convert(name: &str) -> Vec<u8> {
    hwp5_to_hwpx_bytes_with_diagnostics(&fixture(&format!("{name}.hwp")), ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .0
}

/// Compares the two per-character lists; characters whose Hancom `charPr`
/// has no `<hh:shadow>` are skipped. Returns how many were compared.
fn assert_same_shadows(
    name: &str,
    ours: &[(char, Option<String>)],
    hancom: &[(char, Option<String>)],
) -> usize {
    let text = |v: &[(char, Option<String>)]| v.iter().map(|(c, _)| *c).collect::<String>();
    assert_eq!(text(ours), text(hancom), "{name}: text");
    let mut compared = 0;
    for (i, ((c, o), (_, h))) in ours.iter().zip(hancom).enumerate() {
        if h.is_none() {
            continue;
        }
        assert_eq!(o, h, "{name}: char {i} {c:?}");
        compared += 1;
    }
    compared
}

/// The fixture's three shadowed characters, and a self-check that the
/// comparison sees a converter pointing runs at each other's shadow.
///
/// 이것을 실패시키는 것: 색·간격 미전달 · 출력 header 의 charPr 8·9 그림자를 맞바꾸는 것
/// (아래에서 직접 만들어 실패를 확인).
#[test]
fn fixture_shadows_match_hancom_per_character() {
    let ours = shadow_per_char(&convert("user_samples/sample-char-shadow"));
    let hancom = shadow_per_char(&fixture("user_samples/sample-char-shadow.hwpx"));
    let picked: Vec<(char, String)> = hancom
        .iter()
        .filter(|(_, s)| s.as_deref().is_some_and(|s| !s.contains(r#"type="NONE""#)))
        .map(|(c, s)| (*c, s.clone().unwrap()))
        .collect();
    assert_eq!(
        picked,
        [
            (
                '가',
                r##"<hh:shadow type="DROP" color="#C0C0C0" offsetX="10" offsetY="10"/>"##.into()
            ),
            ('나', r##"<hh:shadow type="DROP" color="#C0C0C0" offsetX="5" offsetY="5"/>"##.into()),
            (
                '다',
                r##"<hh:shadow type="CONTINUOUS" color="#123456" offsetX="-3" offsetY="4"/>"##
                    .into()
            ),
        ]
    );
    assert_eq!(assert_same_shadows("fixture", &ours, &hancom), 3);

    // Self-check: swapping the shadows of the 나 and 다 characters must fail.
    let mut swapped = ours.clone();
    let (i, j) = (
        swapped.iter().position(|(c, _)| *c == '나').unwrap(),
        swapped.iter().position(|(c, _)| *c == '다').unwrap(),
    );
    let tmp = swapped[i].1.clone();
    swapped[i].1 = swapped[j].1.clone();
    swapped[j].1 = tmp;
    let caught = std::panic::catch_unwind(|| assert_same_shadows("swapped", &swapped, &hancom));
    assert!(caught.is_err(), "the comparison must see swapped shadows");
}

/// The repo's Hancom twins (macOS 12.30): every character's shadow, all of
/// kind NONE here, matches — including `#C0C0C0`, which the converter used
/// to write as `#B2B2B2`.
///
/// 이것을 실패시키는 것: 종류 0 정의의 색 미전달(DEFAULT 와 다른 색을 쓰는 문서에서).
#[test]
fn twin_shadows_match_hancom_per_character() {
    for name in [
        "tables/table_02_merge_col_row",
        "tables/table_18_public_document_composite",
        "shapes/group-f12-cell-group",
        "images/textbox_anchored",
        "user_samples/sample-gso-ellipse",
        "user_samples/sample-gso-arc",
        "user_samples/pages/sample-charbg-hatch-slash",
    ] {
        let ours = shadow_per_char(&convert(name));
        let hancom = shadow_per_char(&fixture(&format!("{name}.hwpx")));
        let compared = assert_same_shadows(name, &ours, &hancom);
        assert!(compared > 0, "{name}: nothing compared");
    }
}
