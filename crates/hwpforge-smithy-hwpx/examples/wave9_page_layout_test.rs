//! Wave 9: Page Layout Completion test
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example wave9_page_layout_test

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{
    BeginNum, LineNumberShape, MasterPage, PageBorderFillEntry, Section, Visibility,
};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ApplyPageType, CharShapeIndex, GutterType, HwpUnit, ParaShapeIndex, ShowMode,
};
use hwpforge_smithy_hwpx::style_store::{HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

const CS_NORMAL: usize = 0;
const CS_TITLE: usize = 1;
const PS_BODY: usize = 0;
const PS_CENTER: usize = 1;

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));

    let cs_normal = hwpforge_smithy_hwpx::style_store::HwpxCharShape::default();
    store.push_char_shape(cs_normal);

    let mut cs_title = hwpforge_smithy_hwpx::style_store::HwpxCharShape::default();
    cs_title.height = HwpUnit::new(1400).unwrap();
    cs_title.bold = true;
    store.push_char_shape(cs_title);

    let mut ps_body = HwpxParaShape::default();
    ps_body.alignment = Alignment::Justify;
    ps_body.line_spacing = 160;
    store.push_para_shape(ps_body);

    let mut ps_center = HwpxParaShape::default();
    ps_center.alignment = Alignment::Center;
    ps_center.line_spacing = 160;
    store.push_para_shape(ps_center);

    store
}

fn build_section_gutter_mirror() -> Section {
    let ps = PageSettings {
        gutter: HwpUnit::from_mm(10.0).unwrap(),
        gutter_type: GutterType::LeftOnly,
        mirror_margins: true,
        ..PageSettings::a4()
    };
    Section::with_paragraphs(
        vec![
            text_para("Section 1: Gutter + Mirror Margins", CS_TITLE, PS_CENTER),
            text_para("gutter=10mm, gutterType=LEFT_ONLY, mirror=true", CS_NORMAL, PS_BODY),
        ],
        ps,
    )
}

fn build_section_visibility_linenumber() -> Section {
    let vis = Visibility {
        hide_first_header: true,
        hide_first_footer: false,
        hide_first_master_page: false,
        hide_first_page_num: true,
        hide_first_empty_line: false,
        show_line_number: true,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowOdd,
    };
    let lns = LineNumberShape {
        restart_type: 0,
        count_by: 5,
        distance: HwpUnit::new(850).unwrap(),
        start_number: 1,
    };
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Section 2: Visibility + LineNumberShape", CS_TITLE, PS_CENTER),
            text_para("hideFirstHeader=true, fill=SHOW_ODD", CS_NORMAL, PS_BODY),
        ],
        PageSettings::a4(),
    );
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);
    section
}

fn build_section_page_border_fill() -> Section {
    let entries = vec![
        PageBorderFillEntry {
            apply_type: "BOTH".to_string(),
            border_fill_id: 1,
            text_border: "PAPER".to_string(),
            header_inside: false,
            footer_inside: false,
            fill_area: "PAPER".to_string(),
            offset: [
                HwpUnit::new(1417).unwrap(),
                HwpUnit::new(1417).unwrap(),
                HwpUnit::new(1417).unwrap(),
                HwpUnit::new(1417).unwrap(),
            ],
        },
        PageBorderFillEntry {
            apply_type: "EVEN".to_string(),
            border_fill_id: 1,
            text_border: "CONTENT".to_string(),
            header_inside: true,
            footer_inside: true,
            fill_area: "PAGE".to_string(),
            offset: [
                HwpUnit::new(2834).unwrap(),
                HwpUnit::new(2834).unwrap(),
                HwpUnit::new(2834).unwrap(),
                HwpUnit::new(2834).unwrap(),
            ],
        },
        PageBorderFillEntry::default(),
    ];
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Section 3: PageBorderFill", CS_TITLE, PS_CENTER),
            text_para("BOTH/EVEN/ODD 3 entries", CS_NORMAL, PS_BODY),
        ],
        PageSettings::a4(),
    );
    section.page_border_fills = Some(entries);
    section
}

fn build_section_begin_num() -> Section {
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Section 4: BeginNum", CS_TITLE, PS_CENTER),
            text_para("page=10, footnote=5, endnote=3", CS_NORMAL, PS_BODY),
        ],
        PageSettings::a4(),
    );
    section.begin_num =
        Some(BeginNum { page: 10, footnote: 5, endnote: 3, pic: 1, tbl: 1, equation: 1 });
    section
}

fn build_section_master_page() -> Section {
    let master = MasterPage::new(
        ApplyPageType::Both,
        vec![text_para("[ watermark ]", CS_NORMAL, PS_CENTER)],
    );
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Section 5: MasterPage", CS_TITLE, PS_CENTER),
            text_para("BOTH type, background text", CS_NORMAL, PS_BODY),
        ],
        PageSettings::a4(),
    );
    section.master_pages = Some(vec![master]);
    section
}

fn build_section_facing_pages() -> Section {
    let ps = PageSettings {
        gutter: HwpUnit::from_mm(15.0).unwrap(),
        gutter_type: GutterType::LeftRight,
        mirror_margins: true,
        ..PageSettings::a4()
    };
    let vis = Visibility {
        hide_first_header: false,
        hide_first_footer: false,
        hide_first_master_page: false,
        hide_first_page_num: false,
        hide_first_empty_line: false,
        show_line_number: false,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowAll,
    };
    let lns = LineNumberShape {
        restart_type: 1,
        count_by: 10,
        distance: HwpUnit::new(1200).unwrap(),
        start_number: 1,
    };
    let entries = vec![
        PageBorderFillEntry {
            apply_type: "BOTH".to_string(),
            border_fill_id: 1,
            text_border: "PAPER".to_string(),
            header_inside: true,
            footer_inside: true,
            fill_area: "PAPER".to_string(),
            offset: [
                HwpUnit::new(1000).unwrap(),
                HwpUnit::new(1000).unwrap(),
                HwpUnit::new(1000).unwrap(),
                HwpUnit::new(1000).unwrap(),
            ],
        },
        PageBorderFillEntry { apply_type: "EVEN".to_string(), ..PageBorderFillEntry::default() },
        PageBorderFillEntry { apply_type: "ODD".to_string(), ..PageBorderFillEntry::default() },
    ];
    let mut section = Section::with_paragraphs(
        vec![
            text_para("Section 6: Facing Pages", CS_TITLE, PS_CENTER),
            text_para("All Wave 9 features combined", CS_NORMAL, PS_BODY),
        ],
        ps,
    );
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);
    section.page_border_fills = Some(entries);
    section.begin_num = Some(BeginNum::default());
    section
}

fn main() {
    println!("=== Wave 9: Page Layout Completion Test ===\n");

    let style_store = build_style_store();
    let image_store = ImageStore::new();

    let mut doc = Document::new();
    doc.add_section(build_section_gutter_mirror());
    doc.add_section(build_section_visibility_linenumber());
    doc.add_section(build_section_page_border_fill());
    doc.add_section(build_section_begin_num());
    doc.add_section(build_section_master_page());
    doc.add_section(build_section_facing_pages());

    let validated = doc.validate().expect("validation should pass");
    println!("Document validated: {} sections", validated.section_count());

    let bytes =
        HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode should succeed");
    let path = "wave9_page_layout_output.hwpx";
    std::fs::write(path, &bytes).expect("write should succeed");
    println!("Written: {path} ({} bytes)", bytes.len());

    let decoded = HwpxDecoder::decode(&bytes).expect("decode should succeed");
    let sections = decoded.document.sections();
    println!("Decoded: {} sections", sections.len());

    let s1 = &sections[0];
    assert_eq!(s1.page_settings.gutter_type, GutterType::LeftOnly);
    // mirror_margins is lossy: no HWPX attribute exists, always decodes as false
    assert!(!s1.page_settings.mirror_margins);
    println!(
        "  S1: gutter={}, mirror={}",
        s1.page_settings.gutter, s1.page_settings.mirror_margins
    );

    let s6 = &sections[5];
    assert!(!s6.page_settings.mirror_margins);
    assert_eq!(s6.page_settings.gutter_type, GutterType::LeftRight);
    println!(
        "  S6: mirror={}, gutterType={:?}",
        s6.page_settings.mirror_margins, s6.page_settings.gutter_type
    );

    println!("\n=== All Wave 9 tests passed! ===");
}
