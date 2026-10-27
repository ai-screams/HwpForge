#[path = "document.rs"]
mod document;
#[path = "package.rs"]
mod package;

use super::*;

pub(super) fn part(bytes: &[u8], name: &str) -> String {
    package::part(bytes, name)
}

/// `bytes` with entry `name` replaced by `xml`; every other entry copied as is.
pub(super) fn with_part(bytes: &[u8], name: &str, xml: &str) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let entry_name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        out.start_file(&entry_name, options).unwrap();
        if entry_name == name {
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            std::io::copy(&mut entry, &mut out).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

pub(super) fn para(text: &str) -> Paragraph {
    document::text_para(text)
}

/// `bytes` with master page part `n` given a new root `type` and subList body.
pub(super) fn with_master(bytes: &[u8], n: usize, root_attrs: &str, body: &str) -> Vec<u8> {
    let name = format!("Contents/masterpage{n}.xml");
    let xml = part(bytes, &name);
    let root_at = xml.find("<masterPage").unwrap();
    let id_at = root_at + xml[root_at..].find(" id=").unwrap();
    let root_end = root_at + xml[root_at..].find('>').unwrap();
    let sub_open = xml.find("<hp:subList").unwrap();
    let sub_body = sub_open + xml[sub_open..].find('>').unwrap() + 1;
    let sub_close = xml.find("</hp:subList>").unwrap();
    let new = format!(
        "{} id=\"masterpage{n}\" {root_attrs}>{}{body}{}",
        &xml[..id_at],
        &xml[root_end + 1..sub_body],
        &xml[sub_close..]
    );
    with_part(bytes, &name, &new)
}

pub(super) const BOTH: &str = r#"type="BOTH" pageNumber="0" pageDuplicate="0" pageFront="0""#;

pub(super) fn p(runs: &str) -> String {
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">{runs}</hp:p>"#
    )
}

pub(super) fn decode(bytes: &[u8]) -> HwpxDocument {
    HwpxDecoder::decode(bytes).expect("decode")
}

pub(super) fn masters(doc: &HwpxDocument, section: usize) -> &[MasterPage] {
    doc.document.sections()[section].master_pages.as_deref().unwrap_or_default()
}

pub(super) fn content_lost(doc: &HwpxDocument) -> Vec<(String, String, String)> {
    doc.warnings
        .iter()
        .filter_map(|w| match w {
            DecodeWarning::ContentLost { part, path, kind, .. } => {
                Some((part.clone(), path.to_string(), kind.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn reencode(doc: HwpxDocument) -> Vec<u8> {
    let validated = doc.document.validate().expect("validate");
    HwpxEncoder::encode(&validated, &doc.style_store, &doc.image_store).expect("encode")
}
