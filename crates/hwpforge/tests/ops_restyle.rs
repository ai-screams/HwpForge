//! `ops::style::restyle` — a regenerating edit, and therefore fail-closed.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::style::{restyle, RestyleOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// A package whose decode raises `LAYOUT_CACHE_DROPPED` and which is still
/// admissible — the same fixture `fill`/`insert_para`/`delete_para`/`stamp`
/// use for their own decode-warning tests (`ops_insert_para.rs`,
/// `ops_delete_para.rs`), so the surfaces cannot disagree about what "a
/// document that warns" means.
fn warning_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/");
    std::fs::read(format!("{path}stale-line-cache.hwpx")).expect("stale-line-cache.hwpx")
}

/// A store with every language slot filled, which is what the encoder needs
/// before it will write a font table.
fn minimal_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

/// A document whose footnote body starts with a heading.
///
/// The encoder has no safe place to put the note's number head there, so it
/// reports `NoteHeadSkipped` — a semantic loss. This is the same
/// construction `hwpforge-smithy-hwpx`'s `note_numbering_roundtrip` tests
/// use for the stamper's fail-closed case.
fn document_that_loses_a_note_head() -> Vec<u8> {
    let mut heading_body = Paragraph::with_runs(
        vec![Run::text("제목 각주", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    heading_body.heading_level = Some(1);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode")
}

#[test]
fn applies_the_preset_font_and_keeps_the_document_readable() {
    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("modern"))
            .expect("restyle");

    assert_eq!(out.preset, "modern");
    let decoded = HwpxDecoder::decode(&out.bytes).expect("the output must decode");
    assert!(!decoded.document.sections().is_empty());
    assert!(
        decoded.style_store.iter_fonts().any(|f| f.face_name == "맑은 고딕"),
        "the modern preset's base font must reach the font table"
    );
}

#[test]
fn shape_indices_survive_so_the_document_is_not_restyled_into_nonsense() {
    // The store is kept and only face names are swapped; a fresh preset
    // store would renumber char/para shapes the document still references.
    let source = fixture("SimpleTable.hwpx");
    let before = HwpxDecoder::decode(&source).expect("decode source");

    let out = restyle(&source, &RestyleOptions::default().with_preset("classic")).expect("restyle");
    let after = HwpxDecoder::decode(&out.bytes).expect("decode output");

    assert_eq!(before.style_store.char_shape_count(), after.style_store.char_shape_count());
    assert_eq!(before.style_store.para_shape_count(), after.style_store.para_shape_count());
}

#[test]
fn an_unknown_preset_is_refused_before_anything_is_decoded() {
    // Bad bytes *and* a bad preset: reporting `PRESET_NOT_FOUND` proves the
    // lookup happens first, so a typo never costs a decode.
    let err = restyle(b"not an hwpx", &RestyleOptions::default().with_preset("neo"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
    assert!(err.to_string().contains("neo"), "{err}");
    assert!(err.hint().is_some(), "the CLI hint points at `templates`");
}

#[test]
fn a_caller_who_forgets_the_preset_gets_an_error_not_a_house_style() {
    let err =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = restyle(b"not a zip at all", &RestyleOptions::default().with_preset("modern"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn semantic_loss_returns_no_bytes_at_all() {
    // Restyling regenerates the whole package. If the encoder could not
    // place a footnote's number head, the output means something different
    // from the input, so the caller must get an error rather than a
    // plausible-looking file.
    let err = restyle(
        &document_that_loses_a_note_head(),
        &RestyleOptions::default().with_preset("modern"),
    )
    .expect_err("a lost note head must not pass silently");

    assert_eq!(err.code(), OpsCode::EncodeSemanticLoss, "{err}");
    let hwpforge::ops::OpsError::EncodeSemanticLoss { warnings, others } = &err else {
        panic!("wrong variant: {err:?}");
    };
    assert!(!warnings.is_empty(), "the refusal must say what was lost");
    assert!(warnings.iter().all(|w| w.code == "NOTE_HEAD_SKIPPED"), "{warnings:?}");
    assert!(
        !others.iter().any(|w| w.code == "NOTE_HEAD_SKIPPED"),
        "semantic losses must not also appear in `others`: {others:?}"
    );
}

/// A picture whose `binaryItemIDRef` names no package item (fixture F6:
/// `image2` with neither a manifest item nor a BinData entry). Restyling
/// re-encodes the whole package, which would carry the broken picture over
/// without a word, so it is refused.
///
/// 이것을 실패시키는 것: `EncodeWarning::is_semantic_loss` 에서
/// `ImageReferenceUnresolved` 를 `false` 갈래로 옮기는 것.
#[test]
fn a_dangling_image_reference_refuses_the_restyle() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/images/");
    let bytes = std::fs::read(format!("{path}f6-dangling-image.hwpx")).expect("f6 fixture");

    let err = restyle(&bytes, &RestyleOptions::default().with_preset("modern"))
        .expect_err("a dangling image reference must not pass silently");

    assert_eq!(err.code(), OpsCode::EncodeSemanticLoss, "{err}");
    let hwpforge::ops::OpsError::EncodeSemanticLoss { warnings, .. } = &err else {
        panic!("wrong variant: {err:?}");
    };
    let codes: Vec<&str> = warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["IMAGE_REFERENCE_UNRESOLVED"], "{warnings:?}");
    assert!(warnings[0].message.contains("image2"), "{warnings:?}");
}

#[test]
fn a_document_without_fonts_has_nothing_to_rebind() {
    // A store with shapes but no font table. If the encoder accepts it, the
    // package decodes into a document `restyle` cannot act on, which is the
    // one input `NoFonts` exists for.
    let mut store = HwpxStyleStore::new();
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("글꼴 없음", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new())
        .expect("a fontless store must still encode for this branch to be reachable");
    assert!(
        HwpxDecoder::decode(&bytes).expect("decode").style_store.iter_fonts().next().is_none(),
        "the package must really come back without a font table"
    );

    let err = restyle(&bytes, &RestyleOptions::default().with_preset("modern"))
        .expect_err("there is no base font to replace");

    assert_eq!(err.code(), OpsCode::NoFonts, "{err}");
    assert!(err.hint().is_some(), "the CLI hint points at `validate`");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("latest"))
            .expect("restyle");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    let mut fields: Vec<&str> =
        value.as_object().expect("object").keys().map(String::as_str).collect();
    fields.sort_unstable();
    // Actual: {"preset":"latest","sections":1,"paragraphs":2,"warnings":[]}
    assert_eq!(fields, ["paragraphs", "preset", "sections", "warnings"]);
    assert_eq!(value["preset"], "latest");
    assert!(value["warnings"].is_array());
}

#[test]
fn meta_deserializes_json_written_before_sections_and_paragraphs_existed() {
    // The exact key set hwpforge 0.16.5's `RestyleMeta` wrote — pinned by
    // `meta_carries_exactly_the_preset_and_warnings_keys` at main@350851f,
    // before this review fix (`git show 350851f:crates/hwpforge/tests/ops_restyle.rs`).
    let old_json = r#"{"preset":"modern","warnings":[]}"#;

    let meta: hwpforge::ops::RestyleMeta =
        serde_json::from_str(old_json).expect("older-writer JSON must still deserialize");

    assert_eq!(meta.preset, "modern");
    assert_eq!(meta.sections, 0, "no `sections` key in older JSON — must default, not fail");
    assert_eq!(meta.paragraphs, 0, "no `paragraphs` key in older JSON — must default, not fail");
}

#[test]
fn sections_and_paragraphs_match_an_inspect_of_the_output_without_a_second_decode() {
    // `restyle` measures its own counts on the document it already
    // decoded, before re-encoding — this pins that measurement against an
    // independent `inspect` of the bytes it produced, so the two can never
    // silently drift.
    use hwpforge::ops::{inspect, InspectOptions};

    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("modern"))
            .expect("restyle");

    let inspected = inspect(&out.bytes, &InspectOptions::default()).expect("inspect the output");
    let top_level_paragraphs: usize =
        inspected.report.section_details.iter().map(|s| s.top_level_paragraphs).sum();

    assert_eq!(out.sections, inspected.report.sections);
    assert_eq!(out.paragraphs, top_level_paragraphs);
}

/// Before this fix, `restyle` decoded the package but never read
/// `decoded.warnings`, so a stale line-layout cache vanished silently in
/// both the MCP `hwpforge_restyle` tool and Python's `Document.restyle()`
/// (both already had a `warnings` channel — it just stayed empty).
#[test]
fn decode_warnings_reach_the_restyle_output() {
    let out = restyle(&warning_fixture(), &RestyleOptions::default().with_preset("modern"))
        .expect("restyle");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert_eq!(codes, ["LAYOUT_CACHE_DROPPED"], "one input decode, reported once: {codes:?}");
}

/// A clean fixture must not gain a warning from this fix.
#[test]
fn a_clean_fixture_still_reports_no_warnings() {
    let out =
        restyle(&fixture("SimpleTable.hwpx"), &RestyleOptions::default().with_preset("modern"))
            .expect("restyle");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// Anything reaching `warnings` is something the caller may ignore, and
/// never a semantic loss — those are refused outright (see
/// `semantic_loss_returns_no_bytes_at_all`), so this list must never carry
/// one of those codes.
///
/// `sample1.hwpx` decodes with its own `LAYOUT_CACHE_DROPPED` — a
/// pre-existing property of that fixture (it also has known malformed XML,
/// see `hwpforge-smithy-hwpx/tests/golden.rs`'s `#[ignore]` on it), unrelated
/// to this restyle's own encode: the only non-semantic `EncodeWarning` is
/// `LayoutCacheDropped`, which the encoder raises solely under
/// `EncodeOptions::emit_layout_cache` — an opt-in no editing surface sets.
/// Before this fix the decode half of `warnings` was silently dropped
/// (`decode_warnings_reach_the_restyle_output`), so this fixture used to
/// pin an empty list; now it pins the one warning the decode legitimately
/// found, and the loop below still guards against a semantic-loss code ever
/// joining it.
#[test]
fn every_returned_warning_is_non_semantic() {
    let out = restyle(&warning_fixture(), &RestyleOptions::default().with_preset("modern"))
        .expect("restyle");

    let codes: Vec<String> = out.warnings.iter().map(|w| w.info().code).collect();
    assert_eq!(
        codes,
        ["LAYOUT_CACHE_DROPPED"],
        "the decode's own warning, carried through: {codes:?}"
    );

    for code in &codes {
        assert_ne!(code, "NOTE_HEAD_SKIPPED", "{codes:?}");
        assert_ne!(code, "NOTE_RESTART_IGNORED", "{codes:?}");
        assert_ne!(code, "TITLE_MARK_SKIPPED", "{codes:?}");
    }
}

fn shapes_fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/shapes/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// An input whose decode lost content is refused before encoding: the
/// regenerated package would make the loss permanent. Both fixtures decode
/// with exactly one `CONTENT_LOST` (a sheared group child; a group child the
/// group slants).
///
/// 이것을 실패시키는 것: `restyle` 의 `reject_input_content_loss` 호출을 지우는 것.
#[test]
fn input_with_lost_content_is_refused() {
    for name in ["group-sheared-child.hwpx", "group-slanted-child.hwpx"] {
        let bytes = shapes_fixture(name);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(
            decoded.warnings.iter().filter(|w| w.is_content_loss()).count(),
            1,
            "{name}: {:?}",
            decoded.warnings
        );
        let err = restyle(&bytes, &RestyleOptions::default().with_preset("modern")).unwrap_err();
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{name}: {err}");
        assert!(err.to_string().contains("input is not round-trip safe"), "{err}");
    }
}

/// A table inside a grouped text box is no longer a loss: the group encoder
/// keeps its size and place, so restyle goes through.
///
/// 이것을 실패시키는 것: 묶음 자식 안 개체에 임시 `group_nested_object` 경고를
/// 남기는 것(거부로 바뀜).
#[test]
fn grouped_text_box_table_is_restyled() {
    let bytes = shapes_fixture("group-nested-table.hwpx");
    assert!(HwpxDecoder::decode(&bytes).unwrap().warnings.is_empty());
    restyle(&bytes, &RestyleOptions::default().with_preset("modern")).expect("restyle");
}

/// A TextArt whose outline Core cannot keep makes restyle refuse the input
/// (Hancom's F14 with its TextArt given a 33-unit solid outline); the
/// untouched F14 goes through.
///
/// 이것을 실패시키는 것: 글맵시 외곽선 손실 경고를 지우는 것(restyle 이 외곽선
/// 없이 다시 씀).
#[test]
fn text_art_whose_outline_is_lost_is_refused() {
    let opts = RestyleOptions::default().with_preset("modern");
    restyle(&shapes_fixture("group-f14-textart.hwpx"), &opts).expect("plain F14 restyles");
    let err = restyle(&shapes_fixture("group-textart-outline.hwpx"), &opts).unwrap_err();
    assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{err}");
    assert!(err.to_string().contains("textart_outline"), "{err}");
}

/// A TextArt whose font type is HTF (Hangul font; Core keeps only TTF)
/// makes restyle refuse the input (Hancom's F14 with `fontType="HTF"`).
///
/// 이것을 실패시키는 것: 글맵시 글꼴 유형 손실 경고를 지우는 것.
#[test]
fn text_art_with_a_hangul_font_type_is_refused() {
    let opts = RestyleOptions::default().with_preset("modern");
    let err = restyle(&shapes_fixture("group-textart-htf.hwpx"), &opts).unwrap_err();
    assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{err}");
    assert!(err.to_string().contains("textart_font_type"), "{err}");
}

/// §19.22 D5: a restyle keeps every paragraph's end-mark char shape (the
/// fixture's paragraphs end in empty runs, as Hancom writes them).
///
/// 이것을 실패시키는 것: 디코더가 끝 run 을 읽지 않기 / 인코더가 쓰지 않기 /
/// 다른 문단으로 옮기거나 다른 글자 모양으로 쓰기.
#[test]
fn restyle_keeps_end_mark_char_shapes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/user_samples/sample-char-breakwordlatin-variants.hwpx"
    );
    let source = std::fs::read(path).expect("fixture");
    let ends = |bytes: &[u8]| {
        let doc = HwpxDecoder::decode(bytes).expect("decode");
        let mut out = Vec::new();
        for p in &doc.document.sections()[0].paragraphs {
            p.for_each_paragraph(|q| out.push(q.end_char_shape));
        }
        out
    };
    let before = ends(&source);
    assert!(before.iter().any(Option::is_some));
    let out = restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle");
    // Paragraph by paragraph (review r1: a count let a moved or renumbered
    // end mark pass).
    assert_eq!(ends(&out.bytes), before);
}

/// S19: a restyle keeps a cell's dormant margin (its own values while it
/// uses the table's padding).
///
/// 이것을 실패시키는 것: 디코더가 휴면 값을 읽지 않기 / 인코더가 쓰지 않기.
#[test]
fn restyle_keeps_dormant_cell_margins() {
    use hwpforge::core::table::{Table, TableCell, TableMargin, TableRow};
    use hwpforge::foundation::HwpUnit;

    let cell = || {
        TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("칸", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(5000).unwrap(),
        )
    };
    let mut own = cell();
    let unit = |v| HwpUnit::new(v).unwrap();
    own.dormant_margin =
        Some(TableMargin { left: unit(11), right: unit(12), top: unit(13), bottom: unit(14) });
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(
                Table::new(vec![TableRow::new(vec![own, cell()])]),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let source = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &minimal_store(),
        &ImageStore::new(),
    )
    .expect("encode");
    let dormant = |bytes: &[u8]| {
        let doc = HwpxDecoder::decode(bytes).expect("decode");
        let mut out = Vec::new();
        for p in &doc.document.sections()[0].paragraphs {
            for run in &p.runs {
                if let Some(t) = run.content.as_table() {
                    out.extend(t.rows.iter().flat_map(|r| &r.cells).map(|c| c.dormant_margin));
                }
            }
        }
        out
    };
    let before = dormant(&source);
    assert!(before[0].is_some() && before[1].is_none(), "{before:?}");
    let out = restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle");
    assert_eq!(dormant(&out.bytes), before);
}

/// S19: a restyle writes a dormant margin Hancom left unset (`4294967295`
/// on every side, read as -1) back as `4294967295`, not `-1` — the margin
/// attributes are `xs:nonNegativeInteger`.
///
/// 이것을 실패시키는 것: 여백 속성을 i32 로 쓰기(`-1` 이 나간다) / 한 변만
/// 부호 없는 값으로 쓰기.
#[test]
fn restyle_writes_an_unset_dormant_cell_margin_unsigned() {
    use std::io::{Cursor, Read};

    use hwpforge::core::table::{Table, TableCell, TableMargin, TableRow};
    use hwpforge::foundation::HwpUnit;

    let unset = HwpUnit::new(-1).unwrap();
    let mut cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("칸", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(5000).unwrap(),
    );
    cell.dormant_margin =
        Some(TableMargin { left: unset, right: unset, top: unset, bottom: unset });
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let source = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &minimal_store(),
        &ImageStore::new(),
    )
    .expect("encode");
    let out = restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle");
    let mut zip = zip::ZipArchive::new(Cursor::new(&out.bytes)).expect("zip");
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut xml).unwrap();
    let wire = r#"<hp:cellMargin left="4294967295" right="4294967295" top="4294967295" bottom="4294967295"/>"#;
    assert_eq!(xml.matches(wire).count(), 1, "{xml}");
    let negative = xml
        .split("<hp:cellMargin ")
        .skip(1)
        .filter(|rest| rest[..rest.find("/>").expect("self-closing")].contains('-'))
        .count();
    assert_eq!(negative, 0, "{xml}");
}

/// C: restyling an HFT face with a substitute onto the preset's face writes
/// the new face as TTF with no substitute and the default classification —
/// the old face's kind, substitute and classification do not describe it.
/// 이것을 실패시키는 것: `replace_font` 의 초기화 생략(출력에 `HFT`·`substFont`
/// 가 남음).
#[test]
fn restyle_clears_the_replaced_face_metadata() {
    use hwpforge::hwpx::style_store::{HwpxFontType, HwpxFontTypeInfo, HwpxSubstFont};
    let mut store = HwpxStyleStore::new();
    for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        let mut font = HwpxFont::new(0, "휴먼명조", lang);
        font.font_type = Some(HwpxFontType::Hft);
        font.substitute = Some(HwpxSubstFont::new(Some(HwpxFontType::Hft), "한컴바탕"));
        font.type_info = Some(HwpxFontTypeInfo::new("FCAT_MYUNGJO", [5, 4, 0, 1, 1, 1, 1, 1]));
        store.push_font(font);
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("글꼴", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    let before = HwpxDecoder::decode(&bytes).unwrap();
    assert_eq!(before.style_store.iter_fonts().next().unwrap().font_type, Some(HwpxFontType::Hft));

    let out = restyle(&bytes, &RestyleOptions::default().with_preset("modern")).expect("restyle");
    let after = HwpxDecoder::decode(&out.bytes).unwrap();
    let font = after.style_store.iter_fonts().next().unwrap();
    assert_eq!(font.face_name, "맑은 고딕");
    assert_eq!(font.font_type, Some(HwpxFontType::Ttf));
    assert_eq!(font.substitute, None);
    assert_eq!(font.type_info.as_ref().unwrap().family_type, "FCAT_GOTHIC");
}
