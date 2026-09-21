//! `ops::from_json` — building a package from an exported JSON document.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("meta must serialise as an object, got {value}"))
        .keys()
        .cloned()
        .collect()
}

/// Exports `name` and returns the annotated JSON text, which is what a
/// caller of `to_json` would have written to a file.
fn exported_text(name: &str) -> String {
    let out = to_json(&fixture(name), &ToJsonOptions::default()).expect("to_json");
    serde_json::to_string(&out.document).expect("serialise")
}

#[test]
fn an_export_round_trips_back_into_a_decodable_package() {
    let json = exported_text("SimpleTable.hwpx");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    assert!(!out.bytes.is_empty());
    assert_eq!(out.sections, 1, "the fixture has one section");
    let reread = to_json(&out.bytes, &ToJsonOptions::default()).expect("re-export");
    assert_eq!(
        reread.exported.document.sections().len(),
        1,
        "the generated package decodes to the same section count"
    );
}

/// The annotated export carries `addr` fields. They must survive the trip in
/// as recognised, verified input rather than as an unknown field that breaks
/// deserialisation.
#[test]
fn the_grid_addresses_in_an_export_are_accepted_and_verified() {
    let json = exported_text("SimpleTable.hwpx");
    assert!(json.contains("\"addr\""), "the fixture's export must carry addresses");

    from_json(&json, &FromJsonOptions::default()).expect("annotated input is valid input");
}

/// A stale address means the caller edited against an older export. The
/// document is refused rather than written with the caller's assumption.
#[test]
fn a_mismatched_grid_address_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(&exported_text("SimpleTable.hwpx")).expect("parse");
    let mut patched = 0usize;
    corrupt_first_address(&mut value, &mut patched);
    assert_eq!(patched, 1, "the fixture must have an address to corrupt");

    let err = from_json(&value.to_string(), &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::GridAddrInvalid, "{err}");
    assert_eq!(err.code().as_str(), "GRID_ADDR_INVALID");
}

fn corrupt_first_address(value: &mut serde_json::Value, patched: &mut usize) {
    if *patched > 0 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            if let Some(addr) = map.get_mut("addr") {
                *addr = serde_json::json!({ "row": 99, "col": 99 });
                *patched += 1;
                return;
            }
            for child in map.values_mut() {
                corrupt_first_address(child, patched);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                corrupt_first_address(child, patched);
            }
        }
        _ => {}
    }
}

/// When the JSON carries no `styles` block, the fallback must be the full
/// `"default"` preset registry, not just a font list — a paragraph can
/// reference a char/para shape index, and a fonts-only store has none to
/// resolve it against.
///
/// Before this change, the fallback was
/// `HwpxStyleStore::with_default_fonts`, which builds seven font entries and
/// **zero** char shapes and **zero** para shapes (measured directly: see the
/// module doc rationale in `ops::exchange`). A rebuilt document's
/// `charPrIDRef`/`paraPrIDRef` attributes would then point at style headers
/// that do not exist in the output package — a dangling reference the
/// encoder never checks because it writes the index literally, not by
/// looking the shape up in the store. This test decodes the rebuilt package
/// and pins its style store to the shape counts the `"default"` preset
/// itself reports, which a fonts-only store could never produce.
#[test]
fn a_document_exported_without_styles_falls_back_to_the_full_default_preset() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default().with_styles(false))
        .expect("to_json");

    let built = from_json(&out.document.to_string(), &FromJsonOptions::default())
        .expect("the default preset registry fills in");
    assert!(!built.bytes.is_empty());

    let decoded =
        hwpforge::hwpx::HwpxDecoder::decode(&built.bytes).expect("decode rebuilt package");
    let preset = hwpforge::hwpx::style_store_for_preset("default").expect("default preset");
    assert_eq!(
        decoded.style_store.char_shape_count(),
        preset.char_shape_count(),
        "the rebuilt package's char shapes must match the default preset, not a fonts-only store"
    );
    assert_eq!(
        decoded.style_store.para_shape_count(),
        preset.para_shape_count(),
        "the rebuilt package's para shapes must match the default preset, not a fonts-only store"
    );
    assert!(decoded.style_store.char_shape_count() > 0, "a fonts-only fallback would report 0");
    assert!(decoded.style_store.para_shape_count() > 0, "a fonts-only fallback would report 0");
}

/// JSON carries image references, never image bytes, so a picture document
/// needs its original package to keep the pictures.
#[test]
fn a_base_package_supplies_the_images_the_json_only_references() {
    let bytes = fixture("SimplePicture.hwpx");
    let json = exported_text("SimplePicture.hwpx");

    let without = from_json(&json, &FromJsonOptions::default()).expect("no base");
    let with = from_json(&json, &FromJsonOptions::default().with_base(bytes)).expect("with base");

    assert!(
        with.bytes.len() > without.bytes.len(),
        "the inherited image store makes the package larger: {} vs {}",
        with.bytes.len(),
        without.bytes.len()
    );
}

#[test]
fn an_undecodable_base_is_a_decode_failure() {
    let json = exported_text("SimpleTable.hwpx");

    let err = from_json(&json, &FromJsonOptions::default().with_base(b"not a zip".to_vec()))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn text_that_is_not_json_is_a_parse_failure() {
    let err = from_json("{ not json", &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
    assert_eq!(err.code().as_str(), "JSON_PARSE_FAILED");
}

#[test]
fn json_that_is_not_an_exported_document_is_a_parse_failure() {
    let err =
        from_json(r#"{"unexpected": true}"#, &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
}

#[test]
fn meta_carries_paragraphs_beside_the_warnings() {
    let out =
        from_json(&exported_text("SimpleTable.hwpx"), &FromJsonOptions::default()).expect("ok");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(
        keys(&value),
        ["paragraphs", "warnings"].map(String::from).into_iter().collect(),
        "{value}"
    );
    assert!(value["warnings"].is_array());
    assert_eq!(value["paragraphs"], out.paragraphs);
}

/// The count is the whole generated document's paragraphs across every
/// section, taken before the encode consumes the typed tree — not a count of
/// what changed or what the JSON's top-level array happened to list.
#[test]
fn paragraphs_counts_every_paragraph_in_every_section() {
    let json = exported_text("SimpleTable.hwpx");
    let expected: usize = serde_json::from_str::<serde_json::Value>(&json)
        .expect("parse")
        .pointer("/document/sections")
        .and_then(serde_json::Value::as_array)
        .expect("sections array")
        .iter()
        .map(|section| {
            section
                .pointer("/paragraphs")
                .and_then(serde_json::Value::as_array)
                .expect("paragraphs")
                .len()
        })
        .sum();
    assert!(expected > 0, "the fixture must have at least one paragraph");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    assert_eq!(out.paragraphs, expected);
}

/// Generation is **not** fail-closed. The same encode warning that makes a
/// regenerating edit refuse its bytes (`restyle`, `stamp`, `set_cell` all end
/// in `ENCODE_SEMANTIC_LOSS`) comes back here beside the bytes, because a
/// generated document has no original meaning to lose.
///
/// A footnote whose body starts with a heading is the cheapest trigger: the
/// encoder cannot emit the visible number head for it and says so with
/// `NoteHeadSkipped`, which `EncodeWarning::is_semantic_loss` classifies as a
/// semantic loss.
#[test]
fn a_semantic_loss_warning_comes_back_with_the_bytes_instead_of_replacing_them() {
    let out = from_json(&document_with_a_title_mark_footnote(), &FromJsonOptions::default())
        .expect("generation must not fail closed");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(codes.iter().any(|c| c == "NOTE_HEAD_SKIPPED"), "expected a semantic loss: {codes:?}");
    assert!(!out.bytes.is_empty(), "the bytes are produced anyway");
    assert_eq!(out.sections, 1);
}

/// `to_json` promotes each paragraph's wire `linesegarray` into
/// `Paragraph::layout_cache` (W1d), but `from_json` always encodes with
/// `EncodeOptions::default`, whose `emit_layout_cache` stays off — a cache an
/// edited document no longer matches would be worse than none. Without a
/// warning that drop is silent: the round trip decodes fine, but `hwpforge`'s
/// own PDF path needs the cache and would fail later with no link back here.
///
/// `SimpleTable.hwpx` is confirmed (by the guard assertion below) to carry a
/// non-empty cache on at least one paragraph, so this is not a vacuous test.
#[test]
fn from_json_warns_when_the_input_carries_a_layout_cache_it_does_not_re_emit() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default()).expect("to_json");
    assert_eq!(out.exported.document.sections().len(), 1, "fixture has one section");
    let mut cached_paragraphs = 0usize;
    out.exported.document.sections()[0].for_each_paragraph(|p| {
        if p.layout_cache.as_ref().is_some_and(|c| !c.is_empty()) {
            cached_paragraphs += 1;
        }
    });
    assert!(cached_paragraphs > 0, "guard: the fixture must carry a promoted layout cache");

    let json = serde_json::to_string(&out.document).expect("serialise");
    let from = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    let codes: Vec<String> = from.meta().warnings.iter().map(|w| w.code.clone()).collect();
    assert_eq!(
        codes.iter().filter(|c| *c == "LAYOUT_CACHE_DROPPED").count(),
        1,
        "one warning per cached section, not per paragraph: {codes:?}"
    );
    let warning = from
        .meta()
        .warnings
        .into_iter()
        .find(|w| w.code == "LAYOUT_CACHE_DROPPED")
        .expect("warning present");
    // The path names the actual cached paragraph, not just the section —
    // `SimpleTable.hwpx`'s cache sits on the section's first body paragraph.
    assert!(warning.message.contains("section[0].para[0]"), "{}", warning.message);
    assert!(warning.message.contains("not re-emitted"), "{}", warning.message);

    // The drop is real, not just unreported: the rebuilt package carries no
    // promoted cache once re-decoded.
    let redecoded = hwpforge::hwpx::HwpxDecoder::decode(&from.bytes).expect("decode rebuilt");
    let mut redecoded_cached = 0usize;
    redecoded.document.sections()[0].for_each_paragraph(|p| {
        if p.layout_cache.as_ref().is_some_and(|c| !c.is_empty()) {
            redecoded_cached += 1;
        }
    });
    assert_eq!(redecoded_cached, 0, "emit_layout_cache stays off — the cache must not survive");
}

/// A document with no layout cache to drop must not carry the warning.
#[test]
fn from_json_reports_no_layout_cache_warning_when_there_is_none_to_drop() {
    use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let body = Paragraph::with_runs(
        vec![Run::text("본문", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    assert!(body.layout_cache.is_none(), "hand-built paragraphs carry no cache by construction");

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    let json = serde_json::to_string(&exported).expect("serialise");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(!codes.contains(&"LAYOUT_CACHE_DROPPED".to_string()), "{codes:?}");
}

/// The section's only cached paragraph lives inside an image's caption —
/// `Section::for_each_paragraph` deliberately does not visit those (a
/// documented gap on `Document::for_each_paragraph_mut`), so this exercises
/// `from_json`'s own local traversal rather than that Core walker. The
/// encoder drops that cache too, so the warning must still fire exactly
/// once, with a path that reaches into the caption rather than stopping at
/// the section.
#[test]
fn from_json_warns_when_only_an_image_caption_paragraph_carries_a_layout_cache() {
    use hwpforge::core::layout::{LayoutCache, LineSeg};
    use hwpforge::core::{
        Caption, CaptionSide, Document, Image, ImageFormat, PageSettings, Paragraph, Run, Section,
    };
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let line = LineSeg {
        textpos: 0,
        vertpos: 0,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 48188,
        flags: 0,
    };
    let mut caption_para = Paragraph::with_runs(
        vec![Run::text("caption", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    caption_para.layout_cache = Some(LayoutCache::new(vec![line]));

    let mut image = Image::new(
        "BinData/image1.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    image.caption = Some(Caption::new(vec![caption_para], CaptionSide::Bottom));

    let host = Paragraph::with_runs(
        vec![Run::text("본문", CharShapeIndex::new(0)), Run::image(image, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    assert!(host.layout_cache.is_none(), "the cache sits only on the caption paragraph");

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    let json = serde_json::to_string(&exported).expect("serialise");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    let codes: Vec<String> = out.meta().warnings.iter().map(|w| w.code.clone()).collect();
    assert_eq!(
        codes.iter().filter(|c| *c == "LAYOUT_CACHE_DROPPED").count(),
        1,
        "exactly one warning even though the cache is nested in an image caption: {codes:?}"
    );
    let warning = out
        .meta()
        .warnings
        .into_iter()
        .find(|w| w.code == "LAYOUT_CACHE_DROPPED")
        .expect("warning present");
    assert!(
        warning.message.contains("section[0].para[0].caption.npara[0]"),
        "path must reach into the image caption, not stop at the section: {}",
        warning.message
    );
}

/// A memo's `anchor_runs` can carry a non-text run — a table, in this
/// case — that validation permits there. The encoder flattens the anchor
/// into a single inline `<hp:t>`, keeping only `RunContent::plain_text`
/// (`build_memo_anchor_xml`), so the table and every paragraph inside it,
/// cache included, is dropped whole rather than walked. The drop is real
/// even though the encoder never reaches that far, so the warning must
/// still fire — with a path that stops at the memo, the nearest container
/// the encoder's own recursion ever reaches.
#[test]
fn from_json_warns_when_only_a_memo_anchor_table_carries_a_layout_cache() {
    use hwpforge::core::control::Control;
    use hwpforge::core::layout::{LayoutCache, LineSeg};
    use hwpforge::core::{
        Document, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
    };
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let line = LineSeg {
        textpos: 0,
        vertpos: 0,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 48188,
        flags: 0,
    };
    let mut cell_para = Paragraph::with_runs(
        vec![Run::text("anchor cell", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    cell_para.layout_cache = Some(LayoutCache::new(vec![line]));

    let width = HwpUnit::from_mm(20.0).expect("width");
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(vec![cell_para], width)])]);

    let memo_body = Paragraph::with_runs(
        vec![Run::text("memo body", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let anchor_runs = vec![Run::table(table, CharShapeIndex::new(0))];
    let memo = Control::memo_with_anchor(vec![memo_body], anchor_runs);

    let host = Paragraph::with_runs(
        vec![Run::text("본문", CharShapeIndex::new(0)), Run::control(memo, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    assert!(host.layout_cache.is_none(), "the cache sits only inside the anchor table's cell");

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    let json = serde_json::to_string(&exported).expect("serialise");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    let codes: Vec<String> = out.meta().warnings.iter().map(|w| w.code.clone()).collect();
    assert_eq!(
        codes.iter().filter(|c| *c == "LAYOUT_CACHE_DROPPED").count(),
        1,
        "exactly one warning even though the cache sits inside a dropped anchor run: {codes:?}"
    );
    let warning = out
        .meta()
        .warnings
        .into_iter()
        .find(|w| w.code == "LAYOUT_CACHE_DROPPED")
        .expect("warning present");
    assert!(
        warning.message.contains("section[0].para[0].memo"),
        "path should stop at the memo — the encoder never recurses into a dropped anchor run: {}",
        warning.message
    );
}

/// A group whose first child (`Equation`) the encoder never emits as a
/// container child (see `encode_group_child_xml`) sits before a text box
/// that does carry a layout cache. The encoder's own `emitted_idx` counter
/// only advances for a child it actually serializes, so the real encoder
/// would number the text box `group[0]`, not the source list's `group[1]` —
/// `from_json`'s warning must report the same number.
#[test]
fn from_json_group_child_path_uses_the_encoders_emitted_index_not_the_source_index() {
    use hwpforge::core::control::Control;
    use hwpforge::core::layout::{LayoutCache, LineSeg};
    use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let line = LineSeg {
        textpos: 0,
        vertpos: 0,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 48188,
        flags: 0,
    };
    let mut boxed_para = Paragraph::with_runs(
        vec![Run::text("boxed", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    boxed_para.layout_cache = Some(LayoutCache::new(vec![line]));

    let equation = Control::Equation {
        script: "a over b".to_string(),
        width: HwpUnit::new(1000).unwrap(),
        height: HwpUnit::new(1000).unwrap(),
        base_line: 60,
        text_color: Color::from_rgb(0, 0, 0),
        font: "HancomEQN".to_string(),
        inst_id: None,
    };
    let text_box = Control::TextBox {
        paragraphs: vec![boxed_para],
        width: HwpUnit::new(2000).unwrap(),
        height: HwpUnit::new(2000).unwrap(),
        placement: None,
        caption: None,
        style: None,
        text_vertical_align: Default::default(),
    };
    let group = Control::Group {
        // Source order: the dropped Equation comes first, then the cached
        // text box — the whole point of this test is that the encoder's
        // emitted-index counter does not shift with it.
        children: vec![equation, text_box],
        width: HwpUnit::new(3000).unwrap(),
        height: HwpUnit::new(2000).unwrap(),
        placement: None,
        inst_id: None,
    };

    let host = Paragraph::with_runs(
        vec![Run::control(group, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    let json = serde_json::to_string(&exported).expect("serialise");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    let warning = out
        .meta()
        .warnings
        .into_iter()
        .find(|w| w.code == "LAYOUT_CACHE_DROPPED")
        .expect("warning present");
    assert!(
        warning.message.contains("section[0].para[0].group[0]"),
        "the dropped Equation must not consume a GroupChild index — the encoder numbers the \
         text box group[0], not the source list's group[1]: {}",
        warning.message
    );
}

/// An exported document whose only paragraph carries a footnote whose body
/// is a heading. Built rather than loaded: no committed fixture triggers a
/// semantic-loss encode warning.
fn document_with_a_title_mark_footnote() -> String {
    use hwpforge::core::control::Control;
    use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let shape = CharShapeIndex::new(0);
    let mut note_body =
        Paragraph::with_runs(vec![Run::text("제목 각주", shape)], ParaShapeIndex::new(0));
    note_body.heading_level = Some(1);

    let body = Paragraph::with_runs(
        vec![Run::text("본문", shape), Run::control(Control::footnote(vec![note_body]), shape)],
        ParaShapeIndex::new(0),
    );

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));

    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    serde_json::to_string(&exported).expect("serialise")
}
