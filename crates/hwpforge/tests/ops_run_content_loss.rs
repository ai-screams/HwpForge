//! What the reader drops inside a run, a `<hp:t>`, a `<hp:ctrl>` or a
//! `<hp:subList>` is reported as `ContentLost`, in every container, and the
//! editors that re-encode the decode refuse the document; the preserving
//! editors still work on it.
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read, Write};

use hwpforge::core::chart::{ChartData, ChartType};
use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::{Run, RunContent};
use hwpforge::core::section::{HeaderFooter, MasterPage};
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{DecodeWarning, HwpxDecoder, HwpxDocument, HwpxEncoder};
use hwpforge::ops::edit::{
    fill, insert_para, set_cell, FillOptions, InsertParaOptions, SetCellOptions,
};
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

// ── the document ─────────────────────────────────────────────────

fn para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

const SECTION: &str = "Contents/section0.xml";
const MASTER: &str = "Contents/masterpage0.xml";

/// Body paragraph index of `BODY` and of the chart paragraph.
const BODY_PARA: usize = 1;
const CHART_PARA: usize = 5;

/// A stamp candidate, a marker in every container the tests reach (body
/// `BODY`, table cell `CELLA`/`CELLB`, header `HEAD`, footer `FOOT`, footnote
/// `NOTE`, endnote `END`, master page `MASTER`), a named field `이름` for
/// fill, an empty cell for set-cell and, when asked, a chart.
fn base(chart: bool) -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![para("CELLA"), para("CELLB")], width),
        TableCell::new(vec![para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let notes = Paragraph::with_runs(
        vec![
            Run::text("본문", CharShapeIndex::new(0)),
            Run::control(Control::footnote(vec![para("NOTE")]), CharShapeIndex::new(0)),
            Run::control(Control::endnote(vec![para("END")]), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    let field = Paragraph::with_runs(
        vec![Run::control(
            Control::Field {
                field_type: hwpforge::foundation::FieldType::ClickHere,
                hint_text: Some("이름".into()),
                help_text: None,
                name: Some("이름".into()),
                display_text: String::new(),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    let mut paragraphs = vec![para("소속: (   )"), para("BODY"), host, notes, field];
    if chart {
        let chart = Control::chart(
            ChartType::Column,
            ChartData::category(&["Q1", "Q2"], &[("S", &[1.0, 2.0])]),
        );
        paragraphs.push(Paragraph::with_runs(
            vec![Run::control(chart, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
    }
    let mut section = Section::with_paragraphs(paragraphs, PageSettings::a4());
    section.headers = vec![HeaderFooter::new(vec![para("HEAD")], ApplyPageType::Both)];
    section.footers = vec![HeaderFooter::new(vec![para("FOOT")], ApplyPageType::Both)];
    section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![para("MASTER")])]);
    let mut doc = Document::new();
    doc.add_section(section);
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles(), &ImageStore::new())
        .expect("encode")
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}")).read_to_string(&mut out).unwrap();
    out
}

/// `bytes` with entry `name` replaced by `xml` (`None` drops the entry).
fn with_part(bytes: &[u8], name: &str, xml: Option<&str>) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let entry_name = entry.name().expect("ZIP entry name").to_string();
        if entry_name == name && xml.is_none() {
            continue;
        }
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        out.start_file(&entry_name, options).unwrap();
        match xml {
            Some(xml) if entry_name == name => out.write_all(xml.as_bytes()).unwrap(),
            _ => {
                std::io::copy(&mut entry, &mut out).unwrap();
            }
        }
    }
    out.finish().unwrap().into_inner()
}

/// The part that holds `marker`'s `<hp:t>`.
fn part_of(marker: &str) -> &'static str {
    if marker == "MASTER" {
        MASTER
    } else {
        SECTION
    }
}

/// `bytes` with `<hp:t>{marker}</hp:t>` replaced by `with`.
fn replace_marker(bytes: &[u8], marker: &str, with: &str) -> Vec<u8> {
    let name = part_of(marker);
    let xml = part(bytes, name);
    let t = format!("<hp:t>{marker}</hp:t>");
    assert_eq!(xml.matches(&t).count(), 1, "{marker} once in {name}");
    with_part(bytes, name, Some(&xml.replace(&t, with)))
}

const LINESEG: &str = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/></hp:linesegarray>"#;

/// `bytes` with the paragraph holding `marker` given a line cache (`cached`)
/// or none.
fn with_cache(bytes: &[u8], marker: &str, cached: bool) -> Vec<u8> {
    let name = part_of(marker);
    let xml = part(bytes, name);
    let at = xml.find(&format!(">{marker}<")).expect("marker");
    let end = at + xml[at..].find("</hp:p>").expect("paragraph end");
    let body = &xml[at..end];
    let body = match body.find("<hp:linesegarray>") {
        Some(start) => {
            let stop = start + body[start..].find("</hp:linesegarray>").unwrap() + 18;
            format!("{}{}", &body[..start], &body[stop..])
        }
        None => body.to_string(),
    };
    let body = if cached { format!("{body}{LINESEG}") } else { body };
    let xml = format!("{}{body}{}", &xml[..at], &xml[end..]);
    with_part(bytes, name, Some(&xml))
}

fn decode(bytes: &[u8]) -> HwpxDocument {
    HwpxDecoder::decode(bytes).expect("decode")
}

/// `(path, kind)` of every `ContentLost`.
fn lost(bytes: &[u8]) -> Vec<(String, String)> {
    decode(bytes)
        .warnings
        .iter()
        .filter_map(|w| match w {
            DecodeWarning::ContentLost { path, kind, .. } => Some((path.to_string(), kind.clone())),
            _ => None,
        })
        .collect()
}

fn kinds(bytes: &[u8]) -> Vec<String> {
    lost(bytes).into_iter().map(|(_, kind)| kind).collect()
}

fn stamp_all(bytes: &[u8]) -> Result<hwpforge::ops::stamp::StampOutput, hwpforge::ops::OpsError> {
    let specs: Vec<StampSpec> = stamp_plan(bytes)
        .expect("plan")
        .plan
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker.clone(),
            action: StampAction::Field { name: "소속".into(), hint: None },
        })
        .collect();
    assert!(!specs.is_empty(), "the paren blank is a stamp candidate");
    stamp(bytes, &StampMap::Legacy(specs), &StampOptions::default())
}

fn set_cell_once(bytes: &[u8]) -> Result<(), hwpforge::ops::OpsError> {
    set_cell(bytes, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("쇠부리"))
        .map(|_| ())
}

/// restyle, set-cell and stamp all refuse `bytes` naming `kind`.
fn assert_rewriters_refuse(bytes: &[u8], kind: &str, label: &str) {
    let opts = RestyleOptions::default().with_preset("modern");
    for (surface, err) in [
        ("restyle", restyle(bytes, &opts).map(|_| ()).unwrap_err()),
        ("set-cell", set_cell_once(bytes).unwrap_err()),
        ("stamp", stamp_all(bytes).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{label} {surface}: {err}");
        assert!(err.to_string().contains(kind), "{label} {surface}: {err}");
    }
}

/// Containers: marker and a piece every path through it contains.
const CONTAINERS: [(&str, &str); 7] = [
    ("BODY", "section[0].para[1].run[0]"),
    ("CELLA", "cell[0][0].npara[0].run[0]"),
    ("HEAD", "header[0].npara[0].run[0]"),
    ("FOOT", "footer[0].npara[0].run[0]"),
    // Run indexes in a note count from after the number head it strips.
    ("NOTE", "footnote.npara[0].run[0]"),
    ("END", "endnote.npara[0].run[0]"),
    ("MASTER", "masterpage[0].npara[0].run[0]"),
];

// ── D-1: run children ─────────────────────────────────────────────

/// 이것을 실패시키는 것: `note_unsupported_run_child` 의 본문 갈래 삭제(`ole`·
/// 미지 요소가 바탕쪽·묶음 밖에서 무음), `StrayText` 보고 삭제, 또는 공백
/// 판정을 `trim()` 으로(NBSP·U+3000 사례가 무음).
#[test]
fn run_children_the_reader_drops_are_reported_lost() {
    let source = base(false);
    // The document itself is clean and every editor takes it.
    assert!(lost(&source).is_empty(), "{:?}", lost(&source));
    restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle");
    set_cell_once(&source).expect("set-cell");
    stamp_all(&source).expect("stamp");

    let children = [
        ("<hp:ole/>", "ole"),
        ("<hp:imaginary/>", "unknown_element"),
        ("글", "run_text"),
        ("\u{a0}", "run_text"),
        ("\u{3000}", "run_text"),
    ];
    for (marker, at) in CONTAINERS {
        for (child, kind) in children {
            for cached in [false, true] {
                let label = format!("{marker} {child:?} cached={cached}");
                let bytes = replace_marker(
                    &with_cache(&source, marker, cached),
                    marker,
                    &format!("<hp:t>{marker}</hp:t>{child}"),
                );
                let found = lost(&bytes);
                assert_eq!(found.len(), 1, "{label}: {found:?}");
                assert_eq!(found[0].1, kind, "{label}: {found:?}");
                assert!(found[0].0.ends_with(at), "{label}: {found:?}");
                assert_rewriters_refuse(&bytes, kind, &label);
                let md =
                    hwpforge::ops::markdown::to_md(&bytes, &Default::default()).expect("to-md");
                assert!(
                    md.warnings.iter().any(|w| format!("{w:?}").contains(kind)),
                    "{label}: {:?}",
                    md.warnings
                );
            }
        }
    }
}

// ── D-2: unknown elements inside <hp:t> ───────────────────────────

/// 이것을 실패시키는 것: `inline_element` 검사를 필드 본문 `continue` 뒤로
/// 옮기기(필드 안 사례가 무음), 또는 검사 삭제.
#[test]
fn unknown_elements_inside_t_are_reported_in_and_out_of_fields() {
    let source = base(false);
    let outside =
        replace_marker(&source, "BODY", "<hp:t>BO<hp:imaginary>x</hp:imaginary>DY</hp:t>");
    assert_eq!(kinds(&outside), ["inline_element"]);
    assert_rewriters_refuse(&outside, "inline_element", "outside");

    let begin = r#"<hp:ctrl><hp:fieldBegin id="7" type="HYPERLINK" name="" editable="0" dirty="0" zorder="-1" fieldid="7"><hp:parameters cnt="1" name=""><hp:stringParam name="Command">https\://example.com;1;0;0;</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl>"#;
    let end = r#"<hp:ctrl><hp:fieldEnd beginIDRef="7" fieldid="7"/></hp:ctrl>"#;
    let inside =
        replace_marker(&source, "BODY", &format!("{begin}<hp:t>BO<hp:imaginary/>DY</hp:t>{end}"));
    let found = kinds(&inside);
    assert_eq!(found.iter().filter(|k| *k == "inline_element").count(), 1, "{found:?}");
    assert_rewriters_refuse(&inside, "inline_element", "inside a field");
}

// ── D-3: charts ───────────────────────────────────────────────────

fn chart_switch(xml: &str) -> &str {
    let start = xml.find("<hp:switch>").expect("chart switch");
    let end = start + xml[start..].find("</hp:switch>").unwrap() + "</hp:switch>".len();
    &xml[start..end]
}

fn chart_part(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().expect("ZIP entry name").to_string())
        .find(|n| n.starts_with("Chart/"))
        .expect("chart part")
}

/// 이것을 실패시키는 것: 면제를 `depth == 0` 으로 넓히기(머리말 차트가 무음),
/// 회수 순회의 경로 재설정 삭제(XML 없는 차트의 경로가 `section[0]` 만),
/// 또는 회수 순회의 `chart` 보고 삭제.
#[test]
fn charts_are_reported_lost_where_the_section_does_not_recover_them() {
    let source = base(true);
    // A chart in a top-level body paragraph is recovered: no warning.
    assert!(lost(&source).is_empty(), "{:?}", lost(&source));

    // Its chart XML missing: reported at the chart's run.
    let missing = with_part(&source, &chart_part(&source), None);
    assert_eq!(
        lost(&missing),
        [(format!("section[0].para[{CHART_PARA}].run[0]"), "chart".to_string())]
    );

    // Moved into another container: nothing recovers it.
    let xml = part(&source, SECTION);
    let switch = chart_switch(&xml).to_string();
    for (marker, at) in CONTAINERS.into_iter().filter(|(m, _)| *m != "MASTER" && *m != "BODY") {
        let moved = xml
            .replacen(&switch, "", 1)
            .replace(&format!("<hp:t>{marker}</hp:t>"), &format!("<hp:t>{marker}</hp:t>{switch}"));
        let bytes = with_part(&source, SECTION, Some(&moved));
        let found = lost(&bytes);
        assert_eq!(found.len(), 1, "{marker}: {found:?}");
        assert_eq!(found[0].1, "chart", "{marker}: {found:?}");
        assert!(found[0].0.ends_with(at), "{marker}: {found:?}");
    }
}

// ── D-4: ctrl and subList ─────────────────────────────────────────

/// `bytes` with `extra` placed right after the opening `<hp:subList ...>` of
/// the sub-list that holds `marker`.
fn into_sublist(bytes: &[u8], marker: &str, extra: &str) -> Vec<u8> {
    let name = part_of(marker);
    let xml = part(bytes, name);
    let at = xml.find(&format!(">{marker}<")).expect("marker");
    let open = xml[..at].rfind("<hp:subList").expect("sub-list");
    let body = open + xml[open..].find('>').unwrap() + 1;
    let xml = format!("{}{extra}{}", &xml[..body], &xml[body..]);
    with_part(bytes, name, Some(&xml))
}

/// 이것을 실패시키는 것: `convert_ctrl_child` 첫머리 `ctrl_content` 보고 삭제,
/// `note_sublist_content` 호출 지점 하나 삭제(그 컨테이너가 무음), 또는 반복문
/// 안으로 옮기기(문단 2개인 칸이 둘, 문단 0개인 머리말이 0).
#[test]
fn ctrl_and_sublist_content_is_reported_lost() {
    let source = base(false);
    for (label, ctrl) in [
        ("element", r#"<hp:ctrl><hp:bookmark name="bm"/><hp:imaginary/></hp:ctrl>"#),
        ("text", r#"<hp:ctrl><hp:bookmark name="bm"/>글</hp:ctrl>"#),
    ] {
        let bytes = replace_marker(&source, "BODY", &format!("<hp:t>BODY</hp:t>{ctrl}"));
        assert_eq!(
            lost(&bytes),
            [(format!("section[0].para[{BODY_PARA}].run[0]"), "ctrl_content".to_string())],
            "{label}"
        );
        // The known child beside it is still read.
        let doc = decode(&bytes).document;
        let runs = &doc.sections()[0].paragraphs[BODY_PARA].runs;
        assert!(
            runs.iter().any(|r| matches!(
                &r.content,
                RunContent::Control(c) if matches!(c.as_ref(), Control::Bookmark { name, .. } if name == "bm")
            )),
            "{label}: {runs:?}"
        );
        assert_rewriters_refuse(&bytes, "ctrl_content", label);
    }

    // Every sub-list conversion: the cell loop (two paragraphs), the
    // header/footer loop, and the shared one (notes, master pages).
    for (marker, at) in [
        ("CELLA", "cell[0][0]"),
        ("HEAD", "header[0]"),
        ("FOOT", "footer[0]"),
        ("NOTE", "footnote"),
        ("END", "endnote"),
        ("MASTER", "masterpage[0]"),
    ] {
        let bytes = into_sublist(&source, marker, "<hp:imaginary/>");
        let found = lost(&bytes);
        assert_eq!(found.len(), 1, "{marker}: {found:?}");
        assert_eq!(found[0].1, "sublist_content", "{marker}: {found:?}");
        assert!(found[0].0.ends_with(at), "{marker}: {found:?}");
        assert_rewriters_refuse(&bytes, "sublist_content", marker);
    }

    // The shared conversion with two paragraphs: a footnote `A`, `B`.
    // 이것을 실패시키는 것: 공통 헬퍼(`decode_sublist_paragraphs_skipping`)의
    // `note_sublist_content` 호출을 반복문 첫머리로 옮기기(보고가 둘).
    let xml = part(&source, SECTION);
    let at = xml.find(">NOTE<").unwrap();
    let para_end = at + xml[at..].find("</hp:p>").unwrap() + "</hp:p>".len();
    let second = r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>NOTEB</hp:t></hp:run></hp:p>"#;
    let xml = format!("{}{second}{}", &xml[..para_end], &xml[para_end..]);
    let two = into_sublist(&with_part(&source, SECTION, Some(&xml)), "NOTE", "<hp:imaginary/>");
    assert_eq!(kinds(&two), ["sublist_content"]);
    let doc = decode(&two).document;
    let notes: Vec<Vec<String>> = doc.sections()[0].paragraphs[3]
        .runs
        .iter()
        .filter_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::Footnote { paragraphs, .. } => {
                    Some(paragraphs.iter().map(Paragraph::text_content).collect())
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(notes, [["NOTE", "NOTEB"]]);

    // A sub-list with no paragraph at all.
    let xml = part(&source, SECTION);
    let at = xml.find(">HEAD<").unwrap();
    let open = xml[..at].rfind("<hp:subList").unwrap();
    let body = open + xml[open..].find('>').unwrap() + 1;
    let close = at + xml[at..].find("</hp:subList>").unwrap();
    let xml = format!("{}<hp:imaginary/>{}", &xml[..body], &xml[close..]);
    let bytes = with_part(&source, SECTION, Some(&xml));
    assert_eq!(kinds(&bytes), ["sublist_content"]);
}

/// What sits directly under `<hs:sec>` besides `<hp:p>` — text, an unknown
/// element, a `<hp:t>` — is reported once at the section, and the paragraphs
/// around it are still read. Indentation between paragraphs stays silent
/// (`indentation_and_every_known_ctrl_child_stay_silent`).
/// 이것을 실패시키는 것: 구역 디코드의 `section_content` 보고 삭제.
#[test]
fn section_content_is_reported_lost() {
    let source = base(false);
    let xml = part(&source, SECTION);
    let body = xml.find(">BODY<").unwrap();
    let para_start = xml[..body].rfind("<hp:p ").unwrap();
    for (label, stray) in
        [("text", "구역글"), ("element", "<hp:imaginary/>"), ("t", "<hp:t>구역글</hp:t>")]
    {
        let edited = format!("{}{stray}\n  {}", &xml[..para_start], &xml[para_start..]);
        let bytes = with_part(&source, SECTION, Some(&edited));
        assert_eq!(
            lost(&bytes),
            [("section[0]".to_string(), "section_content".to_string())],
            "{label}"
        );
        let doc = decode(&bytes).document;
        let texts: Vec<String> =
            doc.sections()[0].paragraphs.iter().map(Paragraph::text_content).collect();
        assert!(texts.iter().any(|t| t == "BODY"), "{label}: {texts:?}");
        assert!(!texts.iter().any(|t| t.contains("구역글")), "{label}: {texts:?}");
        assert_rewriters_refuse(&bytes, "section_content", label);
    }
}

// ── D-5: what is not reported ─────────────────────────────────────

/// `xml` indented the way a pretty-printer would: line breaks and spaces
/// around runs, controls, sub-lists and paragraphs, never inside `<hp:t>`.
fn indented(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() * 2);
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let structural = [
            "<hp:run",
            "</hp:run>",
            "<hp:ctrl",
            "</hp:ctrl>",
            "<hp:subList",
            "</hp:subList>",
            "<hp:p ",
            "</hp:p>",
        ]
        .iter()
        .any(|tag| rest.starts_with(tag));
        if structural {
            out.push_str("\n    ");
        }
        let end = rest.find('>').unwrap() + 1;
        out.push_str(&rest[..end]);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// 이것을 실패시키는 것: `$value` 가 이름 있는 `HxCtrl` 자식을 가로채기(예:
/// `newNum` 의 역직렬화 이름을 바꾸면 `ctrl_content` 가 생긴다), 또는 들여쓰기
/// 공백을 내용으로 보기.
#[test]
fn indentation_and_every_known_ctrl_child_stay_silent() {
    let source = base(false);
    let known = concat!(
        r#"<hp:ctrl><hp:bookmark name="bm"/></hp:ctrl>"#,
        r#"<hp:ctrl><hp:indexmark><hp:firstKey>k</hp:firstKey></hp:indexmark></hp:ctrl>"#,
        r#"<hp:ctrl><hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/></hp:autoNum></hp:ctrl>"#,
        r#"<hp:ctrl><hp:newNum num="1" numType="PAGE"/></hp:ctrl>"#,
        r#"<hp:ctrl><hp:pageHiding hideHeader="0" hideFooter="0" hideMasterPage="0" hideBorder="0" hideFill="0" hidePageNum="1"/></hp:ctrl>"#,
        r#"<hp:ctrl><hp:pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="-"/></hp:ctrl>"#,
        r#"<hp:t> </hp:t>"#,
    );
    let plain = replace_marker(&source, "BODY", &format!("<hp:t>BODY</hp:t>{known}"));
    // colPr, header, footer, footNote, endNote and the field pair come from
    // the encoder's own output.
    let section = part(&plain, SECTION);
    for tag in [
        "<hp:colPr",
        "<hp:header",
        "<hp:footer",
        "<hp:footNote",
        "<hp:endNote",
        "<hp:fieldBegin",
        "<hp:fieldEnd",
    ] {
        assert!(section.contains(tag), "{tag} in the base document");
    }
    assert!(lost(&plain).is_empty(), "{:?}", lost(&plain));

    let pretty = with_part(&plain, SECTION, Some(&indented(&section)));
    let pretty = with_part(&pretty, MASTER, Some(&indented(&part(&plain, MASTER))));
    assert!(lost(&pretty).is_empty(), "{:?}", lost(&pretty));
    assert!(decode(&plain).document == decode(&pretty).document, "indentation changes nothing");
}

/// 이것을 실패시키는 것: 보고하면서 이웃 자식·문단을 버리기, 또는 같은 자리를
/// 두 번 보고하기.
#[test]
fn an_unknown_child_between_known_ones_is_reported_once_and_they_stay() {
    let source = base(false);
    let run = replace_marker(&source, "BODY", "<hp:t>BO</hp:t><hp:imaginary/><hp:t>DY</hp:t>");
    assert_eq!(kinds(&run), ["unknown_element"]);
    let doc = decode(&run).document;
    assert_eq!(doc.sections()[0].paragraphs[BODY_PARA].text_content(), "BODY");

    let xml = part(&source, SECTION);
    let between = "</hp:p><hp:p";
    let at = xml.find(">CELLA<").unwrap();
    let gap = at + xml[at..].find(between).unwrap() + "</hp:p>".len();
    let xml = format!("{}<hp:imaginary/>{}", &xml[..gap], &xml[gap..]);
    let sub = with_part(&source, SECTION, Some(&xml));
    assert_eq!(kinds(&sub), ["sublist_content"]);
    let doc = decode(&sub).document;
    let RunContent::Table(table) = &doc.sections()[0].paragraphs[2].runs[0].content else {
        panic!("table")
    };
    let texts: Vec<String> =
        table.rows[0].cells[0].paragraphs.iter().map(Paragraph::text_content).collect();
    assert_eq!(texts, ["CELLA", "CELLB"]);
}

// ── D-6: the preserving editors ───────────────────────────────────

/// 이것을 실패시키는 것: fill·patch 에 `ContentLost` 거부를 넣기, 또는 보존
/// 편집기가 그 자리의 원문을 버리기.
#[test]
fn fill_and_patch_keep_the_unread_content() {
    let source = base(false);
    for (marker, child) in [
        ("BODY", "<hp:ole/>"),
        ("CELLA", "<hp:imaginary/>"),
        ("HEAD", "글"),
        ("NOTE", "\u{a0}"),
        ("MASTER", "<hp:ole/>"),
    ] {
        let snippet = format!("<hp:t>{marker}</hp:t>{child}");
        let bytes = replace_marker(&source, marker, &snippet);
        assert_eq!(lost(&bytes).len(), 1, "{marker}");

        let filled = fill(&bytes, &[("이름".into(), "쇠부리".into())], &FillOptions::default())
            .unwrap_or_else(|e| panic!("{marker} fill: {e}"));
        assert!(part(&filled.bytes, part_of(marker)).contains(&snippet), "{marker} fill");

        let exported = export_section(&bytes, &ExportSectionOptions::default().with_section(0))
            .expect("export");
        let json = serde_json::to_string(&exported.section).expect("json");
        let patched = patch(&bytes, &PatchOptions::default().with_patch(json))
            .unwrap_or_else(|e| panic!("{marker} patch: {e}"));
        assert!(part(&patched.bytes, part_of(marker)).contains(&snippet), "{marker} patch");
    }
}

// ── D-7: the footnote number head ─────────────────────────────────

/// The footnote's first paragraph in Core, and whether any run of it is an
/// automatic number.
fn note_first(doc: &Document) -> (String, bool) {
    for para in &doc.sections()[0].paragraphs {
        for run in &para.runs {
            if let RunContent::Control(c) = &run.content {
                if let Control::Footnote { paragraphs, .. } = c.as_ref() {
                    let first = &paragraphs[0];
                    let numbered = first.runs.iter().any(|r| {
                        matches!(&r.content, RunContent::Control(c) if matches!(c.as_ref(), Control::AutoNumber { .. }))
                    });
                    return (first.text_content(), numbered);
                }
            }
        }
    }
    panic!("no footnote")
}

/// The section XML from the encoder's head pair on: `(head run, spacer run)`.
fn head_pair(xml: &str) -> (String, String) {
    let at = xml.find(r#"numType="FOOTNOTE""#).expect("footnote head");
    let head = xml[..at].rfind("<hp:run").unwrap();
    let head_end = at + xml[at..].find("</hp:run>").unwrap() + "</hp:run>".len();
    let spacer_end = head_end + xml[head_end..].find("</hp:run>").unwrap() + "</hp:run>".len();
    (xml[head..head_end].to_string(), xml[head_end..spacer_end].to_string())
}

fn with_head(
    source: &[u8],
    head: impl Fn(&str) -> String,
    spacer: impl Fn(&str) -> String,
) -> Vec<u8> {
    let xml = part(source, SECTION);
    let (h, s) = head_pair(&xml);
    let xml = xml.replacen(&format!("{h}{s}"), &format!("{}{}", head(&h), spacer(&s)), 1);
    with_part(source, SECTION, Some(&xml))
}

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let decoded = decode(bytes);
    let doc = decoded.document.validate().expect("validate");
    HwpxEncoder::encode(&doc, &decoded.style_store, &decoded.image_store).expect("re-encode")
}

fn insert_one(bytes: &[u8]) -> Result<(), hwpforge::ops::OpsError> {
    insert_para(bytes, &InsertParaOptions::default().with_anchor(0).with_text(vec!["끼움".into()]))
        .map(|_| ())
}

/// 이것을 실패시키는 것: 번호 머리 판정에서 `child_order` 무시(h·i·j·b·e 가
/// 떼어져 무음), `has_unknown_part` 생략(c), ctrl 비교에서 새 플래그 제외(d),
/// parts 를 정확 일치로 좁히기(f·g 가 남음), structural admission 에
/// `ContentLost` 거부를 넣기(l 의 insert-para 실패).
#[test]
fn note_number_heads_with_unknown_content_are_kept_and_reported() {
    let source = base(false);
    let same = |s: &str| s.to_string();
    let spacer_t = |with: &'static str| move |s: &str| s.replacen("<hp:t> </hp:t>", with, 1);
    let ctrl_end = |with: &'static str| move |h: &str| h.replacen("</hp:ctrl>", with, 1);

    // Kept and reported: (b)–(e), (h)–(j).
    let kept: Vec<(&str, Vec<u8>, &str)> = vec![
        ("b", with_head(&source, same, spacer_t("<hp:t> </hp:t><hp:ole/>")), "ole"),
        (
            "c",
            with_head(&source, same, spacer_t("<hp:t> <hp:imaginary/></hp:t>")),
            "inline_element",
        ),
        (
            "d",
            with_head(
                &source,
                |h: &str| h.replacen("</hp:autoNum>", "</hp:autoNum><hp:imaginary/>", 1),
                same,
            ),
            "ctrl_content",
        ),
        ("e", with_head(&source, same, spacer_t("<hp:t> </hp:t>글")), "run_text"),
        ("h", with_head(&source, ctrl_end("</hp:ctrl><hp:imaginary/>"), same), "unknown_element"),
        ("i", with_head(&source, ctrl_end("</hp:ctrl><hp:ole/>"), same), "ole"),
        ("j", with_head(&source, ctrl_end("</hp:ctrl>x"), same), "run_text"),
    ];
    for (case, bytes, kind) in &kept {
        assert_eq!(kinds(bytes), [*kind], "({case})");
        let first = decode(bytes).document;
        // The number is not in Core; the spacer's space is.
        assert_eq!(note_first(&first), (" NOTE".to_string(), false), "({case})");
        // decode→encode→decode is a fixed point; the re-encoded paragraph
        // has one number head.
        let again = reencode(bytes);
        assert!(decode(&again).document == first, "({case}) fixed point");
        assert_eq!(part(&again, SECTION).matches(r#"numType="FOOTNOTE""#).count(), 1, "({case})");
        insert_one(bytes).unwrap_or_else(|e| panic!("({case}) insert-para: {e}"));
        let err = set_cell_once(bytes).unwrap_err();
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "({case}) set-cell: {err}");
    }

    // Stripped as before: (a), (f), (g), (k).
    let (h, s) = head_pair(&part(&source, SECTION));
    let pretty_pair = indented(&format!("{h}{s}"));
    let stripped: Vec<(&str, Vec<u8>)> = vec![
        ("a", source.clone()),
        (
            "f",
            with_head(&source, same, spacer_t("<hp:t> <hp:markpenBegin/><hp:markpenEnd/></hp:t>")),
        ),
        ("g", with_head(&source, same, spacer_t("<hp:t><hp:titleMark/> </hp:t>"))),
        ("k", {
            let xml = part(&source, SECTION).replacen(&format!("{h}{s}"), &pretty_pair, 1);
            with_part(&source, SECTION, Some(&xml))
        }),
    ];
    let reference = decode(&source).document;
    for (case, bytes) in &stripped {
        assert!(lost(bytes).is_empty(), "({case}) {:?}", lost(bytes));
        let doc = decode(bytes).document;
        assert_eq!(note_first(&doc), ("NOTE".to_string(), false), "({case})");
        assert!(doc == reference, "({case}) Core as before");
        insert_one(bytes).unwrap_or_else(|e| panic!("({case}) insert-para: {e}"));
    }
}
