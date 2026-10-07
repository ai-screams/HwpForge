//! The wire facts §19.22 (P20) fixes, against Hancom's own HWPX of the same
//! document.
//!
//! Each fixture is a HWP5 file and the HWPX Hancom saved from it. The HWP5
//! file is converted and both packages are reduced to, in document order:
//! every table's outer and inner margin, every cell's `hasMargin` and line
//! wrap, every drawn text's vertical alignment and margin, every ellipse's
//! arc flag, and for every paragraph its end-mark run's char shape and, when
//! it holds nothing, its first run's char shape. The lists must be equal.
//! The fixtures between them carry cells without their own margin (D2), a
//! centred text box (D4) and empty paragraphs with their own char shape
//! (B); their ellipses and arc pin the arc flag as Hancom writes it. None of
//! them has squeezed cells, margins other than the defaults or end runs —
//! the repo twins that do also differ from Hancom in ways P20 does not
//! address — so those are pinned by the unit tests of each decoder and
//! encoder.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn sections(hwpx: &[u8]) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("Contents/section") && n.ends_with(".xml"))
        .map(str::to_string)
        .collect();
    names.sort_by_key(|n| {
        n.trim_start_matches("Contents/section").trim_end_matches(".xml").parse::<u32>().unwrap()
    });
    names
        .iter()
        .map(|n| {
            let mut xml = String::new();
            zip.by_name(n).unwrap().read_to_string(&mut xml).unwrap();
            xml
        })
        .collect()
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap().to_string()
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find(|a| local(a.key.as_ref()) == key).map(|a| a.value.to_string())
}

fn margin(e: &BytesStart<'_>) -> String {
    ["left", "right", "top", "bottom"].map(|k| attr(e, k).unwrap_or_default()).join(" ")
}

#[derive(Default)]
struct Run {
    char_pr: String,
    /// Direct children, by name.
    children: Vec<String>,
    /// A `<hp:t>` with text or with children.
    nonempty_t: bool,
}

#[derive(Default)]
struct Facts {
    out: Vec<String>,
}

/// The facts of one package, one line each.
fn facts(hwpx: &[u8]) -> Vec<String> {
    let mut f = Facts::default();
    for xml in sections(hwpx) {
        let mut reader = Reader::from_str(&xml);
        let mut stack: Vec<String> = Vec::new();
        let mut paragraphs: Vec<Vec<Run>> = Vec::new();
        loop {
            let event = reader.read_event().expect("xml");
            let (start, empty) = match &event {
                Event::Start(e) => (Some(e.clone()), false),
                Event::Empty(e) => (Some(e.clone()), true),
                _ => (None, false),
            };
            if let Some(e) = start {
                let name = local(e.name().as_ref());
                let parent = stack.last().cloned().unwrap_or_default();
                // A child of a run.
                if parent == "run" {
                    if let Some(run) = paragraphs.last_mut().and_then(|p| p.last_mut()) {
                        run.children.push(name.clone());
                    }
                }
                if parent == "t" {
                    if let Some(run) = paragraphs.last_mut().and_then(|p| p.last_mut()) {
                        run.nonempty_t = true;
                    }
                }
                match name.as_str() {
                    "outMargin" | "inMargin" if parent == "tbl" => {
                        f.out.push(format!("tbl {name} {}", margin(&e)))
                    }
                    "tc" => f.out.push(format!(
                        "tc hasMargin {}",
                        attr(&e, "hasMargin").unwrap_or_default()
                    )),
                    "subList" if parent == "tc" => f
                        .out
                        .push(format!("tc lineWrap {}", attr(&e, "lineWrap").unwrap_or_default())),
                    "subList" if parent == "drawText" => f.out.push(format!(
                        "text vertAlign {}",
                        attr(&e, "vertAlign").unwrap_or_default()
                    )),
                    "textMargin" if parent == "drawText" => {
                        f.out.push(format!("text margin {}", margin(&e)))
                    }
                    "ellipse" => f.out.push(format!(
                        "ellipse hasArcPr {}",
                        attr(&e, "hasArcPr").unwrap_or_default()
                    )),
                    "p" => paragraphs.push(Vec::new()),
                    "run" if parent == "p" => {
                        if let Some(p) = paragraphs.last_mut() {
                            p.push(Run {
                                char_pr: attr(&e, "charPrIDRef").unwrap_or_default(),
                                ..Run::default()
                            });
                        }
                    }
                    _ => {}
                }
                if !empty {
                    stack.push(name);
                }
                continue;
            }
            match event {
                Event::Text(t) if stack.last().map(String::as_str) == Some("t") => {
                    if !t.is_empty() {
                        if let Some(run) = paragraphs.last_mut().and_then(|p| p.last_mut()) {
                            run.nonempty_t = true;
                        }
                    }
                }
                Event::End(_) => {
                    let name = stack.pop().unwrap_or_default();
                    if name == "p" {
                        let runs = paragraphs.pop().unwrap_or_default();
                        let empty = |r: &Run| {
                            r.children.is_empty() || (r.children == ["t"] && !r.nonempty_t)
                        };
                        if runs.len() >= 2 && empty(runs.last().unwrap()) {
                            f.out.push(format!("p end {}", runs.last().unwrap().char_pr));
                        }
                        if !runs.is_empty() && runs.iter().all(empty) {
                            f.out.push(format!("p empty {}", runs[0].char_pr));
                        }
                        f.out.push("p".to_string());
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    f.out
}

/// 이것을 실패시키는 것: 칸 `hasMargin` 을 늘 1 로(D2) / 글상자 정렬 오프셋
/// 복귀(D4) / 빈 문단을 글자 모양 0 으로(B, 평면 투영 경로).
#[test]
fn p20_wire_facts_match_hancom_twins() {
    for name in [
        "tables/table_02_merge_col_row",
        "tables/table_18_public_document_composite",
        "shapes/group-f12-cell-group",
        "images/textbox_anchored",
        "user_samples/sample-gso-ellipse",
        "user_samples/sample-gso-arc",
        "user_samples/pages/sample-charbg-hatch-slash",
    ] {
        let (ours, _) = hwp5_to_hwpx_bytes_with_diagnostics(
            &fixture(&format!("{name}.hwp")),
            ConvertOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let hancom = facts(&fixture(&format!("{name}.hwpx")));
        let ours = facts(&ours);
        assert!(hancom.iter().any(|l| l != "p"), "{name}: no facts");
        assert_eq!(ours, hancom, "{name}");
    }
}
