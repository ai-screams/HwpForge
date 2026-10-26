use super::*;

pub(super) fn xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
    text
}

pub(super) fn export(bytes: &[u8]) -> Value {
    // Typed export has no grid-address annotations to become stale when a
    // test intentionally inserts or removes a paragraph.
    serde_json::to_value(to_json(bytes, &ToJsonOptions::default()).unwrap().exported).unwrap()
}

pub(super) fn roundtrip(base: &[u8], value: &Value) -> hwpforge::ops::FromJsonOutput {
    from_json(&value.to_string(), &FromJsonOptions::default().with_base(base)).unwrap()
}

pub(super) fn flags(bytes: &[u8]) -> Vec<bool> {
    HwpxDecoder::decode(bytes).unwrap().document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| p.layout_cache.is_some())
        .collect()
}

pub(super) fn nested_base() -> Vec<u8> {
    use hwpforge::core::layout::{LayoutCache, LineSeg};
    use hwpforge::core::section::{HeaderFooter, MasterPage};
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::foundation::{ApplyPageType, HwpUnit};
    let p = |text: &str| {
        let mut p = Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let mut line = LineSeg::default();
        line.vertsize = 1000;
        line.horzsize = 42520;
        p.layout_cache = Some(LayoutCache::new(vec![line]));
        p
    };
    let cell = |prefix: &str| {
        TableCell::new(
            (0..3).map(|i| p(&format!("{prefix}{i}"))).collect(),
            HwpUnit::new(4000).unwrap(),
        )
    };
    let mut host = p("host");
    let mut table = Table::new(vec![TableRow::new(vec![cell("a"), cell("b")])]);
    table.placement = Some(hwpforge::core::placement::ObjectPlacement::legacy_inline_defaults());
    host.runs.push(Run::table(table, CharShapeIndex::new(0)));
    let mut sec = Section::with_paragraphs(vec![p("before"), host, p("after")], PageSettings::a4());
    sec.headers.push(HeaderFooter::new(vec![p("h0"), p("h1")], ApplyPageType::Both));
    sec.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![p("m0"), p("m1")])]);
    let mut doc = Document::new();
    doc.add_section(sec);
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}

pub(super) fn section_flags(bytes: &[u8], section: usize) -> Vec<bool> {
    HwpxDecoder::decode(bytes).unwrap().document.sections()[section]
        .paragraphs
        .iter()
        .map(|p| p.layout_cache.is_some())
        .collect()
}

pub(super) fn push_control(value: &mut Value, section: usize, paragraph: usize, control: Value) {
    value["document"]["sections"][section]["paragraphs"][paragraph]["runs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"char_shape_id":0, "content":{"Control":control}}));
}

pub(super) fn assert_section_dropped(
    out: &hwpforge::ops::FromJsonOutput,
    section: usize,
    reason: &str,
) {
    assert!(
        section_flags(&out.bytes, section).iter().all(|&cached| !cached),
        "section {section}: {:?}",
        out.warnings
    );
    assert!(
        out.meta()
            .warnings
            .iter()
            .any(|w| w.code == "LAYOUT_CACHE_DROPPED" && w.message.contains(reason)),
        "{:?}",
        out.warnings
    );
}

pub(super) fn edit_package(base: &[u8], part: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    use std::io::Write;
    let mut input = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("ZIP entry name") == part {
            bytes = edit(std::str::from_utf8(&bytes).unwrap()).into_bytes();
        }
        output
            .start_file(
                entry.name().expect("ZIP entry name"),
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

/// Construct trusted synthetic fixtures with caches before exercising the
/// public JSON route. Changes under test are always made after this encode.
pub(super) fn encode_fixture(value: &Value) -> Vec<u8> {
    let doc: Document<hwpforge::core::Draft> =
        serde_json::from_value(value["document"].clone()).unwrap();
    let styles = serde_json::from_value(value["styles"].clone()).unwrap();
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}
