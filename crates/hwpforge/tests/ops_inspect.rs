//! `ops::inspect` — the template operation: pure, byte-in, report-out.
#![cfg(feature = "ops-hwpx")]

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::{inspect, InspectOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[test]
fn reports_structure_of_a_table_fixture() {
    let bytes = fixture("SimpleTable.hwpx");

    let out = inspect(&bytes, &InspectOptions::default()).expect("inspect");

    assert_eq!(out.report.sections, 1, "fixture has one section");
    assert_eq!(out.report.tables, 1, "fixture has one table");
    assert_eq!(out.report.section_details.len(), out.report.sections);
    assert_eq!(out.report.section_details[0].index, 0);
    assert_eq!(out.report.section_details[0].tables, 1);
    assert!(out.report.paragraphs >= out.report.section_details[0].top_level_paragraphs);
    assert!(out.report.styles.is_none(), "styles are opt-in");

    // The fixture's one table is a top-level run (not nested inside
    // another table cell or a note), so the shallow and deep counts must
    // agree here — the divergence case lives in
    // `a_table_nested_in_a_footnote_is_deep_only`.
    let decoded = hwpforge::hwpx::HwpxDecoder::decode(&bytes).expect("decode for cross-check");
    let want = decoded.document.sections()[0].content_counts();
    assert_eq!(out.report.section_details[0].top_level_tables, want.tables);
    assert_eq!(out.report.section_details[0].top_level_images, want.images);
    assert_eq!(out.report.section_details[0].top_level_charts, want.charts);
    assert_eq!(
        out.report.section_details[0].top_level_tables,
        out.report.section_details[0].tables
    );
}

#[test]
fn section_deserializes_the_key_set_0_16_5_wrote() {
    // Exactly the nine keys hwpforge 0.16.5's `InspectSection` wrote. Every
    // field added since carries `#[serde(default)]`, so all twelve of them
    // must default rather than fail the whole parse.
    let old_json = r#"{
        "index": 0,
        "top_level_paragraphs": 2,
        "paragraphs": 3,
        "tables": 1,
        "images": 0,
        "charts": 0,
        "has_header": false,
        "has_footer": false,
        "has_page_number": false
    }"#;

    let section: hwpforge::ops::inspect::InspectSection =
        serde_json::from_str(old_json).expect("older-writer JSON must still deserialize");

    assert_eq!(section.tables, 1, "the deep field the older writer already had must round-trip");
    assert_eq!(section.top_level_tables, 0, "no `top_level_tables` key — must default, not fail");
    assert_eq!(section.top_level_images, 0, "no `top_level_images` key — must default, not fail");
    assert_eq!(section.top_level_charts, 0, "no `top_level_charts` key — must default, not fail");
    // The `all_`/`deep_` counts and the narrowed top-level one: absent from
    // that key set too, so each must default. Asserted per field rather than
    // in bulk so dropping one field's `#[serde(default)]` fails here instead
    // of turning every older payload into a parse error at a caller.
    assert_eq!(section.all_tables, 0, "no `all_tables` key — must default, not fail");
    assert_eq!(section.all_images, 0, "no `all_images` key — must default, not fail");
    assert_eq!(section.all_text_boxes, 0, "no `all_text_boxes` key — must default, not fail");
    assert_eq!(section.all_lines, 0, "no `all_lines` key — must default, not fail");
    assert_eq!(section.all_rectangles, 0, "no `all_rectangles` key — must default, not fail");
    assert_eq!(section.all_polygons, 0, "no `all_polygons` key — must default, not fail");
    assert_eq!(
        section.top_level_non_empty_paragraphs, 0,
        "no `top_level_non_empty_paragraphs` key — must default, not fail"
    );
    assert_eq!(section.deep_paragraphs, 0, "no `deep_paragraphs` key — must default, not fail");
    assert_eq!(
        section.deep_non_empty_paragraphs, 0,
        "no `deep_non_empty_paragraphs` key — must default, not fail"
    );
}

#[test]
fn a_table_nested_in_a_footnote_is_deep_only() {
    // Built rather than loaded: no fixture has a table that only a note
    // carries. A footnote body is exactly the case the deep traversal
    // descends into (`Section::for_each_paragraph`'s docs list notes among
    // its recursion targets) but `Section::content_counts()` — the
    // shallow, top-level rule the CLI/MCP `inspect` contract uses — does
    // not: it only inspects `Section::paragraphs`' own runs.
    use hwpforge::core::control::Control;
    use hwpforge::core::image::ImageStore;
    use hwpforge::core::{
        Document, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
    };
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("셀", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::from_mm(50.0).unwrap(),
    );
    let table = Table::new(vec![TableRow::new(vec![cell])]);
    let note_body = vec![Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )];
    let top = Paragraph::with_runs(
        vec![Run::control(Control::footnote(note_body), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![top], PageSettings::a4()));
    let validated = document.validate().expect("validate");
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let bytes = hwpforge::hwpx::HwpxEncoder::encode(&validated, &styles, &ImageStore::default())
        .expect("encode");

    let out = inspect(&bytes, &InspectOptions::default()).expect("inspect");
    let section = &out.report.section_details[0];

    assert_eq!(section.tables, 1, "the deep traversal must see the footnote's table");
    assert_eq!(section.top_level_tables, 0, "content_counts() never opens a note body");

    let decoded = hwpforge::hwpx::HwpxDecoder::decode(&bytes).expect("decode for cross-check");
    let want = decoded.document.sections()[0].content_counts();
    assert_eq!(
        section.top_level_tables, want.tables,
        "must match Section::content_counts() exactly"
    );
}

#[test]
fn image_fixture_counts_images() {
    let out = inspect(&fixture("SimplePicture.hwpx"), &InspectOptions::default()).expect("inspect");

    assert_eq!(out.report.images, 1);
    assert_eq!(out.report.tables, 0);
}

#[test]
fn chart_fixture_counts_charts() {
    let out = inspect(&fixture("charts.hwpx"), &InspectOptions::default()).expect("inspect");

    assert!(out.report.charts > 0, "the chart fixture must report charts");
    assert_eq!(
        out.report.charts,
        out.report.section_details.iter().map(|s| s.charts).sum::<usize>()
    );
}

#[test]
fn a_fixture_without_fields_reports_none() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    assert!(out.report.fields.is_empty(), "{:?}", out.report.fields);
}

#[test]
fn with_styles_adds_the_style_summary_and_nothing_else() {
    let bytes = fixture("SimpleTable.hwpx");

    let plain = inspect(&bytes, &InspectOptions::default()).expect("inspect");
    let styled =
        inspect(&bytes, &InspectOptions::default().with_styles(true)).expect("inspect styled");

    assert!(plain.report.styles.is_none());
    let styles = styled.report.styles.as_ref().expect("styles requested");
    assert!(!styles.fonts.is_empty(), "a real document declares fonts");
    assert!(!styles.char_shapes.is_empty());
    assert!(!styles.para_shapes.is_empty());
    // The rest of the report must not depend on the flag.
    assert_eq!(plain.report.sections, styled.report.sections);
    assert_eq!(plain.report.paragraphs, styled.report.paragraphs);
    assert_eq!(plain.report.tables, styled.report.tables);
    assert_eq!(plain.report.fields, styled.report.fields);
}

#[test]
fn styles_serialise_only_when_present() {
    let bytes = fixture("SimpleTable.hwpx");

    let plain = serde_json::to_value(inspect(&bytes, &InspectOptions::default()).unwrap().report)
        .expect("serialise");
    let styled = serde_json::to_value(
        inspect(&bytes, &InspectOptions::default().with_styles(true)).unwrap().report,
    )
    .expect("serialise");

    assert!(plain.get("styles").is_none(), "empty Option is skipped: {plain}");
    assert!(styled.get("styles").is_some());
    assert!(plain.get("section_details").is_some());
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = inspect(b"not a zip at all", &InspectOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
    assert!(err.hint().is_some(), "decode failures carry the CLI hint");
}

#[test]
fn empty_input_is_a_decode_failure() {
    let err = inspect(&[], &InspectOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn metadata_carries_the_full_document_metadata_projection() {
    // Review R2 #6: the report must carry everything the MCP inspect tool
    // projects today (subject, created, modified, keywords) so the W2
    // migration needs no second decode. Values are fixture-dependent; the
    // contract here is the key set and the "empty, never missing" rule.
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");
    let json = serde_json::to_value(&out.report.metadata).expect("serialise");
    let mut keys: Vec<&str> =
        json.as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "author",
            "created",
            "description",
            "keywords",
            "last_saved_by",
            "modified",
            "subject",
            "title"
        ]
    );
    assert!(json["keywords"].is_array(), "keywords is a list even when empty");
    assert!(json["title"].is_string(), "text fields are empty strings, never null");
    assert!(json["created"].is_null() || json["created"].is_string());
}
