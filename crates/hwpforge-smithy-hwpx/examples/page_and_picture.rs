//! Small documents demonstrating page settings, columns, images and cached text.
//! Run from the workspace root: cargo run -p hwpforge-smithy-hwpx --example page_and_picture
use hwpforge_core::{
    column::ColumnSettings,
    document::Document,
    image::{Image, ImageFormat, ImageStore},
    layout::{LayoutCache, LineSeg},
    paragraph::Paragraph,
    run::Run,
    section::Section,
    PageSettings,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, EncodeOptions, HwpxEncoder};
use std::io::{Cursor, Read, Write};

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn save(name: &str, section: Section, images: &ImageStore, stale_cache: bool) {
    let mut doc = Document::new();
    doc.add_section(section);
    let bytes = HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        images,
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    // A deliberately stale line offset exercises decode diagnostics on otherwise editable text.
    let bytes = if stale_cache {
        let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..input.len() {
            let mut entry = input.by_index(i).unwrap();
            let name = entry.name().unwrap().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if name == "Contents/section0.xml" {
                data = String::from_utf8(data)
                    .unwrap()
                    .replace("textpos=\"0\"", "textpos=\"99999\"")
                    .into_bytes();
            }
            output
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(entry.compression()),
                )
                .unwrap();
            output.write_all(&data).unwrap();
        }
        output.finish().unwrap().into_inner()
    } else {
        bytes
    };
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/hwpforge-smithy-hwpx/tests/fixtures".into());
    std::fs::write(std::path::Path::new(&directory).join(name), bytes).unwrap();
}
fn main() {
    let empty = ImageStore::new();
    let mut cached = paragraph("작은 정원의 관찰 기록입니다.");
    let mut line = LineSeg::default();
    line.vertsize = 1000;
    line.textheight = 1000;
    line.baseline = 850;
    line.horzsize = 40000;
    cached.layout_cache = Some(LayoutCache::new(vec![line]));
    save(
        "garden_notes.hwpx",
        Section::with_paragraphs(
            vec![cached, paragraph("새 잎을 살펴봅니다.")],
            PageSettings::a4(),
        ),
        &empty,
        true,
    );
    let mut images = ImageStore::new();
    // One green RGB pixel, encoded as PNG; no external image input.
    images.insert(
        "swatch.png",
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 176, 89,
            16, 0, 0, 2, 72, 1, 45, 108, 176, 0, 156, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ],
    );
    let mut image = Image::new(
        "swatch.png",
        HwpUnit::new(25942).unwrap(),
        HwpUnit::new(8775).unwrap(),
        ImageFormat::Png,
    );
    image.frame.original_size = Some(hwpforge_foundation::Size::new(
        HwpUnit::new(17664).unwrap(),
        HwpUnit::new(5975).unwrap(),
    ));
    save(
        "green_swatch.hwpx",
        Section::with_paragraphs(
            vec![
                paragraph("정원의 색상 표본"),
                Paragraph::with_runs(
                    vec![Run::image(image, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
            ],
            PageSettings::a4(),
        ),
        &images,
        false,
    );
    let mut page = PageSettings::a4();
    page.width = HwpUnit::from_mm(180.0).unwrap();
    page.height = HwpUnit::from_mm(240.0).unwrap();
    page.margin_left = HwpUnit::from_mm(17.0).unwrap();
    page.margin_right = HwpUnit::from_mm(23.0).unwrap();
    save(
        "small_page.hwpx",
        Section::with_paragraphs(vec![paragraph("작은 쪽의 여백을 확인합니다.")], page),
        &empty,
        false,
    );
    let mut section = Section::with_paragraphs(
        (1..=6).map(|i| paragraph(&format!("관찰 기록 {i}"))).collect(),
        PageSettings::a4(),
    );
    section.column_settings =
        Some(ColumnSettings::equal_columns(2, HwpUnit::new(1200).unwrap()).unwrap());
    save("garden_columns.hwpx", section, &empty, false);
}
