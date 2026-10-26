//! Synthetic depth packages built from committed fixtures only.
#![allow(dead_code)]
use super::fixture_path;
use hwpforge_smithy_hwpx::PackageReader;
use std::io::{Read, Write};
const TABLE_ANCHOR: &str = r#"<hp:run charPrIDRef="0"><hp:t>1</hp:t></hp:run>"#;
/// Text placed in the innermost level; a decode that drops it is a silent
/// loss, not a success.
pub const DEEPEST: &str = "XML_LIMIT_DEEPEST";

fn base_bytes() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hwpforge-smithy-hwpx/tests/fixtures/SimpleTable.hwpx"),
    )
    .unwrap()
}

fn base_section() -> String {
    PackageReader::new(&base_bytes()).unwrap().read_section_xml(0).unwrap()
}

pub fn fixture_section(rel: &str) -> String {
    let bytes = std::fs::read(fixture_path(rel)).expect("read fixture");
    PackageReader::new(&bytes)
        .expect("open fixture package")
        .read_section_xml(0)
        .expect("read section0.xml")
}

/// Byte range of the first `<open …>…</close>` element.
pub fn first_element(xml: &str, open: &str, close: &str) -> std::ops::Range<usize> {
    let start = xml.find(open).expect("element start");
    let end = xml.find(close).expect("element end") + close.len();
    start..end
}

/// The generated table section with its table replaced by `levels` nested
/// levels following `pattern` (`T` = table, `B` = textbox), the outermost
/// level being `pattern[0]`. The generated fragments keep the per-level
/// element depth equal to the measured one (T 6, B 5). Same construction as
/// the boundary cases in the decoder's `xml_limit_tests`.
// 이것을 실패시키는 것: 조립 깊이를 levels + 1로 늘리면 release의 32단 허용 검사가 실패한다.
pub fn nested_section(pattern: &str, levels: usize) -> String {
    let section = base_section();
    let table_span = first_element(&section, "<hp:tbl", "</hp:tbl>");
    let table = &section[table_span.clone()];
    assert_eq!(table.matches(TABLE_ANCHOR).count(), 1, "table anchor");
    let textbox = textbox("ANCHOR");
    let textbox_anchor = r#"<hp:run charPrIDRef="0">ANCHOR<hp:t>outer</hp:t></hp:run>"#;

    let kinds: Vec<char> = pattern.chars().cycle().take(levels).collect();
    let mut fragment = String::new();
    for (index, kind) in kinds.iter().rev().enumerate() {
        let text = if index == 0 { DEEPEST } else { "outer" };
        let run = format!(r#"<hp:run charPrIDRef="0">{fragment}<hp:t>{text}</hp:t></hp:run>"#);
        fragment = match kind {
            'T' => table.replacen(TABLE_ANCHOR, &run, 1),
            'B' => textbox.replacen(textbox_anchor, &run, 1),
            other => panic!("unknown nesting kind {other}"),
        };
    }
    format!("{}{}{}", &section[..table_span.start], fragment, &section[table_span.end..])
}

const GROUP_OPEN: &str = r#"<hp:container id="1000" zOrder="0" numberingType="PICTURE" textWrap="IN_FRONT_OF_TEXT" textFlow="BOTH_SIDES" lock="0" groupLevel="0" instid="1000"><hp:offset x="0" y="0"/><hp:orgSz width="15000" height="13000"/><hp:curSz width="0" height="0"/><hp:sz width="15000" widthRelTo="ABSOLUTE" height="13000" heightRelTo="ABSOLUTE" protect="0"/>"#;

pub fn paragraph(fragment: &str) -> String {
    format!(
        r#"<hp:p paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">{fragment}<hp:t>{DEEPEST}</hp:t></hp:run></hp:p>"#
    )
}

pub fn section_with(fragment: &str) -> String {
    let mut xml = base_section();
    let span = first_element(&xml, "<hp:tbl", "</hp:tbl>");
    xml.replace_range(span, fragment);
    xml
}

/// Five nested elements per text-box level, matching the XML recursion boundary.
pub fn textbox(fragment: &str) -> String {
    let text = if fragment.is_empty() { DEEPEST } else { "outer" };
    format!(
        r#"<hp:rect id="1001"><hp:orgSz width="1000" height="1000"/><hp:curSz width="1000" height="1000"/><hp:sz width="1000" height="1000"/><hp:drawText><hp:subList><hp:p paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">{fragment}<hp:t>{text}</hp:t></hp:run></hp:p></hp:subList></hp:drawText></hp:rect>"#
    )
}

pub fn group(fragment: &str) -> String {
    format!("{GROUP_OPEN}{fragment}</hp:container>")
}

pub fn groups(n: usize) -> String {
    (0..n).fold(textbox(""), |inner, _| group(&inner))
}

pub fn pairs(n: usize) -> String {
    (0..n).fold(String::new(), |inner, _| group(&textbox(&inner)))
}

pub fn pictures(n: usize, last_caption: bool) -> String {
    let mut inner = if last_caption { paragraph("") } else { String::new() };
    for i in 0..n {
        let caption = if inner.is_empty() {
            String::new()
        } else {
            format!(
                r#"<hp:caption side="BOTTOM" width="1000"><hp:subList>{inner}</hp:subList></hp:caption>"#
            )
        };
        let picture = format!(
            r#"<hp:pic id="{}"><hp:sz width="1000" height="1000"/><hc:img binaryItemIDRef=""/>{caption}</hp:pic>"#,
            2000 + i
        );
        inner = if i + 1 == n {
            picture
        } else {
            let text = if i == 0 && !last_caption { DEEPEST } else { "outer" };
            format!(
                r#"<hp:p paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">{picture}<hp:t>{text}</hp:t></hp:run></hp:p>"#
            )
        };
    }
    inner
}

pub fn line() -> String {
    r#"<hp:line id="2001"><hp:orgSz width="1000" height="1000"/><hp:sz width="1000" height="1000"/><hc:startPt x="0" y="0"/><hc:endPt x="1000" y="1000"/></hp:line>"#.into()
}

pub fn tables_with_leaf(n: usize, leaf: &str) -> String {
    let xml = nested_section("T", n);
    xml.replace(&format!("<hp:t>{DEEPEST}</hp:t>"), &format!("{leaf}<hp:t>{DEEPEST}</hp:t>"))
}

#[derive(Clone, Copy, Debug)]
pub enum Place {
    Body,
    Second,
    Header,
    Footer,
    Master(usize),
}

/// Preserves the baseline package, adds a real spine entry for section 1,
/// or a manifest entry + secPr reference for a numbered master page.
pub fn package(section: String, place: Place) -> Vec<u8> {
    let base = base_bytes();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
    let mut parts = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        parts.insert(entry.name().unwrap().into_owned(), bytes);
    }
    let mut manifest = String::from_utf8(parts["Contents/content.hpf"].clone()).unwrap();
    match place {
        Place::Body => {
            parts.insert("Contents/section0.xml".into(), section.into_bytes());
        }
        Place::Second => {
            parts.insert("Contents/section1.xml".into(), section.into_bytes());
            manifest = manifest.replace("</opf:manifest>", r#"<opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/></opf:manifest>"#)
                .replace("</opf:spine>", r#"<opf:itemref idref="section1" linear="yes"/></opf:spine>"#);
            let header = String::from_utf8(parts["Contents/header.xml"].clone())
                .unwrap()
                .replace("secCnt=\"1\"", "secCnt=\"2\"");
            parts.insert("Contents/header.xml".into(), header.into_bytes());
        }
        Place::Header | Place::Footer | Place::Master(_) => {
            // Keep the nested object but remove section properties from this auxiliary flow.
            let start = section.find("<hp:p ").unwrap();
            let end = section.rfind("</hs:sec>").unwrap();
            let mut body = section[start..end].to_owned();
            let secpr = first_element(&body, "<hp:secPr ", "</hp:secPr>");
            body.replace_range(secpr, "");
            match place {
                Place::Master(n) => {
                    let root = &section[..start];
                    let root = root.replace("<hs:sec ", "<hm:masterPage type=\"BOTH\" ");
                    let master = format!("{root}<hp:subList>{body}</hp:subList></hm:masterPage>");
                    parts.insert(format!("Contents/masterpage{n}.xml"), master.into_bytes());
                    let body = section_with("").replace(
                        "</hp:secPr>",
                        &format!(r#"<hp:masterPage idRef="masterpage{n}"/></hp:secPr>"#),
                    );
                    parts.insert("Contents/section0.xml".into(), body.into_bytes());
                    manifest = manifest.replace("</opf:manifest>", &format!(r#"<opf:item id="masterpage{n}" href="Contents/masterpage{n}.xml" media-type="application/xml"/></opf:manifest>"#));
                }
                _ => {
                    let kind = match place {
                        Place::Header => "header",
                        _ => "footer",
                    };
                    let control = format!(
                        r#"<hp:ctrl><hp:{kind} id="100" applyPageType="BOTH"><hp:subList>{body}</hp:subList></hp:{kind}></hp:ctrl>"#
                    );
                    parts.insert(
                        "Contents/section0.xml".into(),
                        section_with(&control).into_bytes(),
                    );
                }
            }
        }
    }
    parts.insert("Contents/content.hpf".into(), manifest.into_bytes());
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "mimetype",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(&parts.remove("mimetype").unwrap()).unwrap();
    for (name, bytes) in parts {
        writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(64 << 20).spawn(f).unwrap().join().unwrap()
}
