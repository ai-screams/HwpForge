//! Field markers preserve their own layer, while the visible body remains
//! ordinary runs. Malformed grammar and broken pairs fail independently.
use hwpforge_core::control::{FieldBoundary, FieldParameter, FieldStart};
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};

fn marker(b: FieldBoundary) -> Run {
    Run::control(Control::FieldBoundary { boundary: b }, CharShapeIndex::new(3))
}
fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(2))
}
fn doc(paragraphs: Vec<Paragraph>) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    d
}
fn start(id: &str) -> FieldStart {
    let mut s = FieldStart::default();
    s.id = id.into();
    s.kind = "FORMULA".into();
    s
}
fn end(id: &str) -> Run {
    marker(FieldBoundary::End { begin_id: id.into(), field_id: "end-tag".into() })
}
fn encode(s: FieldStart) -> String {
    let id = s.id.clone();
    MdEncoder::encode_lossless(
        &doc(vec![para(vec![marker(FieldBoundary::Start(Box::new(s))), end(&id)])])
            .validate()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn boundaries_and_parameters_round_trip_exactly() {
    let attack = "a\t\r\n<&>\"'&#13;</field-start><script>한글";
    for annotation in [
        None,
        Some(vec![]),
        Some(vec![para(vec![Run::text("annotation", CharShapeIndex::new(4))])]),
    ] {
        for parameter_name in [None, Some(String::new()), Some(attack.into())] {
            let mut s = start(attack);
            s.kind = attack.into();
            s.name = attack.into();
            s.editable = "false".into();
            s.dirty = "0".into();
            s.z_order = "-01".into();
            s.field_id = attack.into();
            s.meta_tag = attack.into();
            s.parameter_name = parameter_name;
            s.annotation = annotation.clone();
            s.parameters = vec![
                FieldParameter::Boolean { name: attack.into(), value: "TRUE".into() },
                FieldParameter::String {
                    name: attack.into(),
                    value: attack.into(),
                    xml_space: attack.into(),
                },
                FieldParameter::Integer { name: attack.into(), value: "-0007".into() },
                FieldParameter::String { name: "".into(), value: "".into(), xml_space: "".into() },
            ];
            let input = doc(vec![
                para(vec![
                    marker(FieldBoundary::Start(Box::new(s))),
                    Run::text("visible", CharShapeIndex::new(9)),
                ]),
                para(vec![end(attack)]),
            ])
            .validate()
            .unwrap();
            let md = MdEncoder::encode_lossless(&input).unwrap();
            assert!(md.contains("&#9;&#13;&#10;"));
            assert!(!md.contains("<script>"));
            let output = MdDecoder::decode_lossless(&md).unwrap().validate().unwrap();
            assert_eq!(input.sections(), output.sections());
            let empty_form =
                md.replace("<field-annotation></field-annotation>", "<field-annotation/>");
            let output = MdDecoder::decode_lossless(&empty_form).unwrap().validate().unwrap();
            assert_eq!(input.sections(), output.sections());
        }
    }
}

#[test]
fn empty_start_and_overlapping_ranges_round_trip() {
    let d = doc(vec![para(vec![
        marker(FieldBoundary::Start(Box::new(start("a")))),
        marker(FieldBoundary::Start(Box::new(start("b")))),
        end("a"),
        end("b"),
    ])])
    .validate()
    .unwrap();
    let md = MdEncoder::encode_lossless(&d).unwrap();
    assert!(md.contains("data-meta-tag=\"\"/>"));
    let read = MdDecoder::decode_lossless(&md).unwrap().validate().unwrap();
    assert_eq!(d.sections(), read.sections());
}

#[test]
fn field_grammar_is_closed() {
    let mut s = start("a");
    s.parameters.push(FieldParameter::String {
        name: "n".into(),
        value: "v".into(),
        xml_space: "preserve".into(),
    });
    s.annotation = Some(vec![]);
    let md = encode(s);
    for (from, to) in [
        ("<field-start ", "<field-start bad=\"1\" "),
        ("<field-start ", "<field-start data-id=\"duplicate\" "),
        ("<field-end ", "<field-end bad=\"1\" "),
        ("<field-end ", "<field-end data-begin-id=\"duplicate\" "),
        ("<field-param ", "<field-param bad=\"1\" "),
        ("data-field-id=\"end-tag\"/>", "data-field-id=\"end-tag\"></field-end>"),
        ("data-xml-space=\"preserve\"/>", "data-xml-space=\"preserve\"></field-param>"),
        ("<field-param ", "<field-param data-value=\"duplicate\" "),
        ("data-kind=\"String\"", "data-kind=\"Other\""),
        ("data-kind=\"String\"", "data-kind=\"Boolean\""),
        (" data-xml-space=\"preserve\"", ""),
        ("<field-annotation>", "<field-annotation bad=\"1\">"),
        ("</field-annotation>", "</field-annotation><field-annotation/>"),
        ("</field-annotation>", "</field-annotation><field-param data-kind=\"Integer\" data-name=\"n\" data-value=\"1\"/>"),
        ("<field-annotation>", "<field-annotation><field-end data-char-shape=\"0\" data-begin-id=\"a\" data-field-id=\"\"/>"),
        ("<field-annotation>", "<field-annotation>unexpected"),
    ] {
        let malformed = md.replace(from, to);
        assert_ne!(malformed, md);
        assert!(MdDecoder::decode_lossless(&malformed).is_err(), "{to}");
    }
    for fragment in ["<field-start/>", "<field-end/>", "<field-param/>", "<field-annotation/>"] {
        assert!(
            MdDecoder::decode_lossless(&format!("---\nformat: lossless\n---\n{fragment}")).is_err()
        );
    }
}

#[test]
fn broken_pairs_are_rejected_by_core() {
    let md = encode(start("a"));
    let read =
        MdDecoder::decode_lossless(&md.replace("data-begin-id=\"a\"", "data-begin-id=\"b\""))
            .unwrap();
    assert!(read.validate().is_err());
    assert!(doc(vec![para(vec![marker(FieldBoundary::Start(Box::new(start("a"))))])])
        .validate()
        .is_err());
    assert!(doc(vec![para(vec![end("a")])]).validate().is_err());
}

#[test]
fn field_depth_is_symmetric() {
    let mut inner = vec![];
    for depth in 1..=33 {
        let mut s = start("a");
        s.annotation = Some(inner);
        let paragraphs = vec![para(vec![marker(FieldBoundary::Start(Box::new(s))), end("a")])];
        let encoded = doc(paragraphs.clone())
            .validate()
            .map_err(|error| hwpforge_smithy_md::MdError::UnsupportedStructure {
                detail: error.to_string(),
            })
            .and_then(|d| MdEncoder::encode_lossless(&d));
        assert_eq!(encoded.is_ok(), depth <= 32, "{depth}");
        if let Ok(md) = encoded {
            assert!(MdDecoder::decode_lossless(&md).is_ok());
            if depth == 32 {
                let extra = encode({
                    let mut s = start("x");
                    s.annotation = Some(vec![]);
                    s
                });
                let open = extra
                    .split("<p data-para-shape=\"2\">")
                    .nth(1)
                    .unwrap()
                    .split("</p>")
                    .next()
                    .unwrap();
                let with_extra = md.replacen(
                    "<field-annotation></field-annotation>",
                    &format!(
                        "<field-annotation><p data-para-shape=\"0\">{open}</p></field-annotation>"
                    ),
                    1,
                );
                assert_ne!(with_extra, md);
                assert!(MdDecoder::decode_lossless(&with_extra).is_err());
            }
        }
        inner = paragraphs;
    }
}

#[test]
fn field_attributes_refuse_nul() {
    let mut s = start("a");
    s.name = "\0".into();
    let d = doc(vec![para(vec![marker(FieldBoundary::Start(Box::new(s))), end("a")])])
        .validate()
        .unwrap();
    assert!(MdEncoder::encode_lossless(&d).is_err());
}

#[test]
fn boundary_without_annotation_still_counts_toward_depth() {
    let mut inner =
        vec![para(vec![marker(FieldBoundary::Start(Box::new(start("leaf")))), end("leaf")])];
    for _ in 0..32 {
        let mut s = start("outer");
        s.annotation = Some(inner);
        inner = vec![para(vec![marker(FieldBoundary::Start(Box::new(s))), end("outer")])];
    }
    assert!(doc(inner).validate().is_err());
}

#[test]
fn fields_survive_nested_target_elements_and_refuse_flattened_captions() {
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::control::GroupChild;
    use hwpforge_core::{Image, ImageFormat, ObjectFrame, Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    let unit = HwpUnit::new(1000).unwrap();
    let cs = CharShapeIndex::new(3);
    let fields = || vec![para(vec![marker(FieldBoundary::Start(Box::new(start("a")))), end("a")])];
    let shape = Control::text_box(fields(), unit, unit);
    let group = Control::Group {
        children: vec![GroupChild::Shape(shape)],
        width: unit,
        height: unit,
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    let mut image = Image::new("image.png", unit, unit, ImageFormat::Png);
    image.caption = Some(Caption::new(fields(), CaptionSide::Bottom));
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(fields(), unit)])]);
    let mut annotation = start("annotation");
    annotation.annotation = Some(vec![para(vec![
        Run::control(group, cs),
        Run::image(image, cs),
        Run::table(table, cs),
    ])]);
    let d = doc(vec![para(vec![
        marker(FieldBoundary::Start(Box::new(annotation))),
        end("annotation"),
    ])])
    .validate()
    .unwrap();
    let md = MdEncoder::encode_lossless(&d).unwrap();
    let read = MdDecoder::decode_lossless(&md).unwrap().validate().unwrap();
    assert_eq!(d.sections(), read.sections());
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![para(vec![Run::text("cell", cs)])],
        unit,
    )])])
    .with_caption(Caption::new(fields(), CaptionSide::Bottom));
    let d = doc(vec![para(vec![Run::table(table, cs)])]).validate().unwrap();
    assert!(MdEncoder::encode_lossless(&d).is_err());
}

#[test]
fn unclosed_source_state_roundtrips_without_fabricating_an_end() {
    let mut s = start("source");
    s.unclosed = true;
    let mut draft = doc(vec![para(vec![
        marker(FieldBoundary::Start(Box::new(s))),
        Run::text("body", CharShapeIndex::new(0)),
    ])]);
    assert!(draft.clone().validate().is_err());
    // Start with a valid paired package, then remove the source fieldEnd.
    let bytes = hwpforge_smithy_hwpx::HwpxEncoder::encode(
        &doc(vec![para(vec![
            marker(FieldBoundary::Start(Box::new(start("source")))),
            Run::text("body", CharShapeIndex::new(0)),
            end("source"),
        ])])
        .validate()
        .unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &hwpforge_core::ImageStore::new(),
    )
    .unwrap();
    use std::io::{Cursor, Read, Write};
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("ZIP entry name") == "Contents/section0.xml" {
            let mut xml = String::from_utf8(bytes).unwrap();
            let start = xml.find("<hp:fieldEnd").unwrap();
            let end = start + xml[start..].find("/>").unwrap() + 2;
            xml.replace_range(start..end, "");
            bytes = xml.into_bytes();
        }
        output
            .start_file(
                entry.name().expect("ZIP entry name").to_string(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    let source_doc =
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&output.finish().unwrap().into_inner())
            .unwrap()
            .document;
    hwpforge_core::field_inventory::FieldInventory::restore_source_provenance(
        draft.sections_mut(),
        source_doc.sections(),
    );
    let d = draft.validate().unwrap();
    let md = MdEncoder::encode_lossless(&d).unwrap();
    assert!(md.contains("data-unclosed=\"true\""));
    assert!(!md.contains("<field-end"));
    let mut decoded = MdDecoder::decode_lossless(&md).unwrap();
    assert!(decoded.clone().validate().is_err());
    hwpforge_core::field_inventory::FieldInventory::restore_source_provenance(
        decoded.sections_mut(),
        d.sections(),
    );
    let decoded = decoded.validate().unwrap();
    assert_eq!(d.sections(), decoded.sections());
    assert_eq!(
        hwpforge_core::field_inventory::FieldInventory::from_sections(decoded.sections())
            .warnings()
            .len(),
        1
    );
    for replacement in ["", " data-unclosed=\"false\""] {
        let unmarked = md.replace(" data-unclosed=\"true\"", replacement);
        assert!(MdDecoder::decode_lossless(&unmarked).unwrap().validate().is_err());
    }
    for replacement in ["data-unclosed=\"yes\"", "data-unclosed=\"true\" data-unclosed=\"false\""] {
        assert!(
            MdDecoder::decode_lossless(&md.replace("data-unclosed=\"true\"", replacement)).is_err()
        );
    }
    let closed = encode(start("closed"));
    let explicit_false = closed.replace("<field-start ", "<field-start data-unclosed=\"false\" ");
    assert_eq!(
        MdDecoder::decode_lossless(&closed).unwrap().sections(),
        MdDecoder::decode_lossless(&explicit_false).unwrap().sections()
    );
}
