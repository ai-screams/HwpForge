use super::*;
use std::io::{Read as _, Write as _};

#[test]
fn inspect_nonexistent_file() {
    let (_, _, code) = run(&["inspect", "/nonexistent/file.hwpx"]);
    assert_eq!(code, 1);
}

#[test]
fn inspect_complex_doc() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn inspect_complex_section_counts() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec0 = &val["sections"][0];
    assert_eq!(sec0["paragraphs"], 31);
    assert_eq!(sec0["tables"], 1);
    assert_eq!(sec0["images"], 1);
    assert_eq!(sec0["has_header"], true);
    assert_eq!(sec0["has_footer"], true);
}

#[test]
fn inspect_complex_charts() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec3 = &val["sections"][3];
    assert_eq!(sec3["charts"], 4);
    assert_eq!(sec3["has_page_number"], true);
}

#[test]
fn inspect_complex_styles() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap(), "--styles"]);
    assert_eq!(code, 0);
    let styles = &val["styles"];
    assert_eq!(styles["fonts"].as_array().unwrap().len(), 14);
    assert_eq!(styles["char_shapes"].as_array().unwrap().len(), 8);
    assert_eq!(styles["para_shapes"].as_array().unwrap().len(), 5);
}

#[test]
fn inspect_multicol_paragraphs() {
    // 이것을 실패시키는 것: 생성 문서의 문단 하나를 제거하기.
    let f = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hwpforge-smithy-hwpx/tests/fixtures/garden_columns.hwpx");
    // 이것을 실패시키는 것: 생성 문서의 단 설정을 단일 단으로 바꾸기.
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode_file(&f).unwrap();
    assert_eq!(decoded.document.sections()[0].column_settings.as_ref().unwrap().columns.len(), 2);
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["paragraphs"], 6);
}

/// Value-parity regression: a table/image/chart nested inside an *image's* caption is invisible to
/// `ops::inspect`'s shared paragraph traversal — Core's
/// `image_caption_paragraphs_are_skipped_documents_known_gap` test
/// documents that image captions (unlike table/textbox captions) are not
/// walked. `inspect.rs`'s `tables`/`images`/`charts`/`deep_paragraphs`
/// fields must keep reading from the pre-migration local scanner (via
/// `SectionInfo::merge`'s `deep` parameter — the raw-XML
/// `count_occurrences` scan for the first three, `count_paragraphs_recursive`
/// for the last), or these counts silently drop content or change value on
/// any document exercising these two independent blind spots. No fixture
/// under `tests/fixtures/**` contains an `<hp:caption` at all (checked by
/// scanning every `.hwpx` fixture's section XML for the element), so this
/// test builds the document with Core's API and encodes it, rather than
/// editing fixture bytes by hand.
///
/// This fixture pins two *different* legacy-vs-`ops` divergences, so a
/// regression on either field is caught even though the mechanisms differ:
///
/// - `tables`/`images`/`charts`: a table, a second image and a chart are
///   nested inside the outer image's caption. The raw-XML scan sees every
///   `<hp:tbl>`/`<hp:pic>`/`<hp:chart>` in the section regardless of
///   nesting; `ops::inspect`'s traversal never enters the caption at all,
///   so it would see only the outer image and nothing nested in it.
///   **If `SectionInfo::merge` read `ops_section.{tables,images,charts}`
///   instead of `deep.{tables,images,charts}`, this test would see
///   `tables=0, images=1, charts=0` instead of the asserted `1, 2, 1`.**
/// - `deep_paragraphs`: the section also carries one master-page paragraph.
///   `ops::inspect`'s traversal (`Section::for_each_paragraph` /
///   `walk_paragraphs`, `crates/hwpforge-core/src/section.rs`) explicitly
///   walks `section.master_pages`; the CLI's own local
///   `count_paragraphs_recursive` (`analysis/deep_counts.rs`) sums only
///   `section.paragraphs` plus headers and footers — no `master_pages` term
///   exists in that file at all. Both scopes independently treat an
///   `Image` run as a paragraph-less leaf (skip its caption), which is why
///   the caption's three nested paragraphs contribute to neither scope's
///   count — the master page is the only paragraph source that tells them
///   apart. **If `SectionInfo::merge` read `ops_section.paragraphs` instead
///   of `deep.deep_paragraphs`, this test would see `deep_paragraphs=2`
///   (1 body + 1 master page) instead of the asserted `1`.**
#[test]
fn inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page() {
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
    use hwpforge_core::control::Control;
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::{MasterPage, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::Document;
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    // Caption content: a table, a second image and a chart, all nested
    // inside the *outer* image's caption — exactly the traversal's blind
    // spot for these three fields.
    let nested_table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![text_para("caption-table-cell")],
        HwpUnit::from_pt(100.0).unwrap(),
    )])]);
    let nested_image = Image::new(
        "BinData/nested.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    let nested_chart = Control::Chart {
        chart_type: ChartType::Bar,
        data: ChartData::category(&["A", "B"], &[("Series1", [1.0, 2.0].as_slice())]),
        width: HwpUnit::new(10000).unwrap(),
        height: HwpUnit::new(8000).unwrap(),
        title: None,
        legend: LegendPosition::default(),
        grouping: ChartGrouping::Clustered,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    let mut caption_table_para = text_para("caption-table-host");
    caption_table_para.add_run(Run::table(nested_table, CharShapeIndex::new(0)));
    let mut caption_image_para = text_para("caption-image-host");
    caption_image_para.add_run(Run::image(nested_image, CharShapeIndex::new(0)));
    let mut caption_chart_para = text_para("caption-chart-host");
    caption_chart_para.add_run(Run::control(nested_chart, CharShapeIndex::new(0)));

    let mut host_image = Image::new(
        "BinData/host.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    host_image.caption = Some(Caption::new(
        vec![text_para("caption text"), caption_table_para, caption_image_para, caption_chart_para],
        CaptionSide::Bottom,
    ));

    let mut host = text_para("host");
    host.add_run(Run::image(host_image, CharShapeIndex::new(0)));

    let mut section = Section::with_paragraphs(vec![host], PageSettings::a4());
    // Second, independent blind spot: a master-page paragraph, present
    // only in `ops_section.paragraphs`'s scope (see doc comment above).
    section.master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_para("master-page-para")])]);

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    // `HwpxEncoder` silently skips images missing from the `ImageStore`
    // (XML reference only, no binary data) — no real image bytes are
    // needed to prove the structural-count divergence. Neither `validate`
    // nor the encoder inspect an `Image`'s caption or a section's master
    // pages any deeper than the round-trip needs (`RunContent::Image` is a
    // validation no-op; master pages aren't in `validate_sections` at all).
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    let dir = test_tmp();
    let src = dir.join("image-caption-and-masterpage-nested-controls.hwpx");
    std::fs::write(&src, &bytes).expect("write fixture");

    let (val, _, code) = run_json(&["inspect", src.to_str().unwrap()]);
    assert_eq!(code, 0, "{val}");
    let sec0 = &val["sections"][0];
    // Legacy (pinned) values. `ops` scope, listed for contrast, is in the
    // doc comment above.
    assert_eq!(sec0["images"], 2, "{val}");
    assert_eq!(sec0["tables"], 1, "{val}");
    assert_eq!(sec0["charts"], 1, "{val}");
    assert_eq!(sec0["deep_paragraphs"], 1, "{val}");
}

/// An empty reference carries Hancom's assetless frame; both inspect counts see it.
/// 이것을 실패시키는 것: convert_picture 의 빈 참조 조기 반환 복원.
#[test]
fn crafted_assetless_pic_is_counted_by_raw_scan_and_decode() {
    check_crafted_picture_counts(false);
}

/// 기존 raw-scan 계약 고정. 이것을 실패시키는 것: CLI images를 all_images로 대체.
#[test]
fn crafted_pic_without_img_is_counted_only_by_raw_scan() {
    check_crafted_picture_counts(true);
}

fn check_crafted_picture_counts(remove_img: bool) {
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::Document;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let image = Image::new(
        "BinData/host.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    let mut host = text_para("host");
    host.add_run(Run::image(image, CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    // Hancom clears a lost asset reference while retaining the picture frame.
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    let mut section_xml = String::new();
    archive
        .by_name("Contents/section0.xml")
        .expect("section0")
        .read_to_string(&mut section_xml)
        .expect("read section0");
    assert!(
        section_xml.contains(r#"binaryItemIDRef="host""#),
        "encoder must have written the reference to tamper with: {section_xml}"
    );
    let tampered_xml = if remove_img {
        let start = section_xml.find("<hc:img ").expect("img start");
        let end = start + section_xml[start..].find("/>").expect("img end") + 2;
        format!("{}{}", &section_xml[..start], &section_xml[end..])
    } else {
        section_xml.replace(r#"binaryItemIDRef="host""#, r#"binaryItemIDRef="""#)
    };

    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).expect("entry").name().expect("ZIP entry name").to_string())
        .collect();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (index, name) in names.iter().enumerate() {
        if name == "Contents/section0.xml" {
            continue;
        }
        let entry = archive.by_index_raw(index).expect("raw entry");
        writer.raw_copy_file(entry).expect("copy entry");
    }
    writer
        .start_file("Contents/section0.xml", zip::write::SimpleFileOptions::default())
        .expect("start section0");
    writer.write_all(tampered_xml.as_bytes()).expect("write section0");
    let tampered_bytes = writer.finish().expect("finish zip").into_inner();

    let dir = test_tmp();
    let path = dir.join("crafted-pic-no-binary-ref.hwpx");
    std::fs::write(&path, &tampered_bytes).expect("write fixture");

    let out = hwpforge::ops::inspect(&tampered_bytes, &hwpforge::ops::InspectOptions::default())
        .expect("assetless picture is valid");
    assert_eq!(
        out.report.section_details[0].all_images,
        usize::from(!remove_img),
        "decoded-object count requires hc:img, even with an empty reference: {:?}",
        out.report.section_details[0]
    );

    // CLI `--json`: the raw scan still sees the `<hp:pic>` element, exactly
    // as the pre-migration scanner would have.
    let (val, _, code) = run_json(&["inspect", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{val}");
    assert_eq!(
        val["sections"][0]["images"], 1,
        "raw scan includes hp:pic regardless of hc:img presence: {val}"
    );
}

#[test]
fn inspect_json_error() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
}

// ═══════════════════════════════════════════════════════════════
// 후속: MCP/Python 처럼 ops 디코드 경고(`LAYOUT_CACHE_DROPPED` 등)를
// CLI 도 노출한다 — `--json`의 `warnings` 키(비었으면 생략) +
// 텍스트 모드 stderr 한 줄씩. `delete-para`/`insert-para`는 기존
// `warnings` 관례(항상 존재, 구조적 자문과 같은 배열)를 따른다.
// ═══════════════════════════════════════════════════════════════

#[test]
fn inspect_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, stderr, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");
    assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());

    let (_, text_stderr, code2) = run(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[inspect] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
    let _ = stderr; // --json 모드는 stdout 의 warnings 키만 쓰고 stderr 는 비운다.
}
