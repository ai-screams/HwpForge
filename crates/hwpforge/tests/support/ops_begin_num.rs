use super::*;

pub(super) fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name(name).expect(name).read_to_string(&mut out).expect("utf-8");
    out
}

pub(super) fn element<'a>(xml: &'a str, tag: &str) -> &'a str {
    let at = xml.find(&format!("<{tag} ")).unwrap_or_else(|| panic!("no {tag}"));
    &xml[at..at + xml[at..].find('>').expect("tag end")]
}

pub(super) fn assert_hancom_numbers(bytes: &[u8], surface: &str) {
    let header = part(bytes, "Contents/header.xml");
    assert_eq!(
        element(&header, "hh:beginNum"),
        r#"<hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/"#,
        "{surface}"
    );
    let section = part(bytes, "Contents/section0.xml");
    assert!(element(&section, "hp:startNum").contains(r#" page="5""#), "{surface}");
}

/// `bytes` with its header's start numbers replaced by `beginnum`.
pub(super) fn with_header_numbers(bytes: &[u8], beginnum: &str) -> Vec<u8> {
    let header = part(bytes, "Contents/header.xml");
    let old = element(&header, "hh:beginNum").to_string() + ">";
    let header = header.replace(&old, beginnum);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        let data = if name == "Contents/header.xml" { header.as_bytes().to_vec() } else { data };
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}
