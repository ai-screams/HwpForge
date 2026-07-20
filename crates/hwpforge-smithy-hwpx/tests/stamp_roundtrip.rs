//! E6 Wave 1A end-to-end gate: plan → apply → encode → `fields`/`fill`.
//!
//! Locks the invariant that a stamped document is immediately consumable by
//! the shipped fill delta API (v0.11.1): stamped fields are discoverable via
//! `list_fields`, fillable via `fill`, and a re-plan of the stamped document
//! yields zero candidates.

use std::collections::BTreeMap;

use hwpforge_core::image::ImageStore;
use hwpforge_core::page::PageSettings;
use hwpforge_core::run::Run;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, Draft, Paragraph, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::stamp::{apply, plan, StampAction, StampSpec};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxFiller};

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &styles, &ImageStore::new()).expect("encode")
}

#[test]
fn stamped_document_round_trips_through_fields_and_fill() {
    // Body paren blank + table-cell checkbox — the two dominant class-A
    // shapes from the corpus survey.
    let width = HwpUnit::new(8000).unwrap();
    let row = TableRow::new(vec![TableCell::new(vec![text_para("□ 동의")], width)]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("성명: (   )"), host],
        PageSettings::default(),
    ));

    // plan → approve everything with names
    let candidates = plan(&doc);
    assert_eq!(candidates.len(), 2, "expected paren blank + checkbox: {candidates:?}");
    let specs: Vec<StampSpec> = candidates
        .iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Field {
                name: if c.marker == "□" { "동의".into() } else { "성명".into() },
                hint: None,
            },
        })
        .collect();
    let outcome = apply(&mut doc, &specs).expect("apply");
    assert_eq!(outcome.stamped.len(), 2);

    // encode → the stamped output must be immediately consumable by the
    // shipped fill surface
    let bytes = encode(doc);

    let fields = HwpxFiller::list_fields(&bytes).expect("list_fields");
    let mut names: Vec<(Option<String>, String, bool)> =
        fields.iter().map(|f| (f.name.clone(), f.current.clone(), f.fillable)).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            (Some("동의".to_string()), "□".to_string(), true),
            (Some("성명".to_string()), "(   )".to_string(), true),
        ],
        "stamped fields must be discoverable, marker-bodied, and fillable"
    );

    // fill both fields through the delta API
    let mut values = BTreeMap::new();
    values.insert("성명".to_string(), "박한국".to_string());
    values.insert("동의".to_string(), "☑".to_string());
    let filled = HwpxFiller::fill(&bytes, &values).expect("fill");
    assert_eq!(filled.filled.len(), 2);

    let after = HwpxFiller::list_fields(&filled.bytes).expect("list_fields after fill");
    let mut currents: Vec<(Option<String>, String)> =
        after.iter().map(|f| (f.name.clone(), f.current.clone())).collect();
    currents.sort();
    assert_eq!(
        currents,
        vec![
            (Some("동의".to_string()), "☑".to_string()),
            (Some("성명".to_string()), "박한국".to_string()),
        ]
    );

    // the stamped (and filled) document has no residual candidates
    let decoded = HwpxDecoder::decode(&filled.bytes).expect("decode");
    assert_eq!(plan(&decoded.document).len(), 0, "no candidates may survive stamping");
}

#[test]
fn stamped_output_decode_encode_is_a_fixed_point() {
    // Stamp-delta gate precondition (§3-6③): the stamped artifact itself
    // must be stable under decode→encode.
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("이메일:  @ "), text_para("작성일: 년  월  일")],
        PageSettings::default(),
    ));
    let candidates = plan(&doc);
    assert_eq!(candidates.len(), 2);
    let specs: Vec<StampSpec> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Field { name: format!("필드{i}"), hint: None },
        })
        .collect();
    apply(&mut doc, &specs).expect("apply");
    let bytes = encode(doc);

    let d1 = HwpxDecoder::decode(&bytes).expect("d1");
    let mut styles2 = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles2.push_char_shape(HwpxCharShape::default());
    styles2.push_para_shape(HwpxParaShape::default());
    let e1 = HwpxEncoder::encode(
        &d1.document.clone().validate().expect("validate d1"),
        &d1.style_store,
        &d1.image_store,
    )
    .expect("e1");
    let d2 = HwpxDecoder::decode(&e1).expect("d2");
    assert_eq!(d1.document, d2.document, "stamped output must be a decode/encode fixed point");
    assert_eq!(d1.style_store, d2.style_store);
}
