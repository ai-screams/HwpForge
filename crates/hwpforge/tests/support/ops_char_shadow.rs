use super::*;

pub(super) fn header(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/header.xml").expect("header").read_to_string(&mut out).unwrap();
    out
}
