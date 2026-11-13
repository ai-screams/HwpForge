#![cfg(feature = "ops-hwpx")]
use hwpforge::core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

#[test]
fn json_base_profile_is_checked_even_for_legacy_exchange() {
    // 이것을 실패시키는 것: 구형 경고 분기의 모드 대조 우회 또는 base 부분 플래그 유실.
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let base =
        HwpxEncoder::encode(&doc.validate().unwrap(), &HwpxStyleStore::new(), &ImageStore::new())
            .unwrap();
    let hancom = base.clone();
    let base = package::splice_part(
        &base,
        "Contents/header.xml",
        r#"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument>"#,
        r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility><hh:useInnerUnderline/></hh:layoutCompatibility></hh:compatibleDocument>"#,
    );
    let original = to_json(&base, &ToJsonOptions::default()).unwrap().document;
    // 이것을 실패시키는 것: 프로필이 같은 원본 주장 오류도 encode 오류로 바꾸기.
    let mut invalid = original.clone();
    let mut start = hwpforge::core::control::FieldStart::default();
    start.id = "7".into();
    start.kind = "FORMULA".into();
    start.unclosed = true;
    invalid["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"] =
        serde_json::to_value(hwpforge::core::RunContent::Control(Box::new(
            hwpforge::core::Control::FieldBoundary {
                boundary: hwpforge::core::control::FieldBoundary::Start(Box::new(start)),
            },
        )))
        .unwrap();
    let error =
        from_json(&invalid.to_string(), &FromJsonOptions::default().with_base(base.clone()))
            .unwrap_err();
    assert_eq!(error.code().as_str(), "DECODE_FAILED");
    assert!(error.to_string().contains("unverified source field"));
    for legacy in [false, true] {
        let mut value = original.clone();
        if legacy {
            value.as_object_mut().unwrap().remove("format_version");
            fn legacy_measures(value: &mut serde_json::Value) {
                match value {
                    serde_json::Value::Object(map)
                        if map.contains_key("unit") && map.contains_key("value") =>
                    {
                        *value = map["value"].clone();
                    }
                    serde_json::Value::Object(map) => {
                        if let Some(reference) = map.remove("style_ref") {
                            map.insert("style_id".into(), reference);
                        }
                        map.values_mut().for_each(legacy_measures);
                    }
                    serde_json::Value::Array(items) => items.iter_mut().for_each(legacy_measures),
                    _ => {}
                }
            }
            legacy_measures(&mut value);
        }
        for (mode, changed) in
            [(None, false), (None, true), (Some("hancom"), false), (Some("hancom"), true)]
        {
            let mut bad = value.clone();
            bad["document"].as_object_mut().unwrap().remove("layout_profile");
            if let Some(mode) = mode {
                bad["document"]["layout_profile"] = mode.into();
            }
            if changed {
                bad["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Text"] =
                    "edited".into();
            }
            let error =
                from_json(&bad.to_string(), &FromJsonOptions::default().with_base(base.clone()))
                    .unwrap_err();
            assert_eq!(error.code().as_str(), "ENCODE_FAILED", "legacy {legacy}: {error}");
            assert!(error.to_string().contains("/layout_profile"));
        }
        value["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Text"] =
            "edited".into();
        let out =
            from_json(&value.to_string(), &FromJsonOptions::default().with_base(base.clone()))
                .unwrap();
        let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
        assert_eq!(h.find("hh:compatibleDocument").attr("targetProgram"), Some("MS_WORD"));
        assert_eq!(h.find("hh:layoutCompatibility").child_names(), ["hh:useInnerUnderline"]);
        // 이것을 실패시키는 것: Word base만 검사하고 반대 방향의 불일치를 허용.
        let error =
            from_json(&value.to_string(), &FromJsonOptions::default().with_base(hancom.clone()))
                .unwrap_err();
        assert_eq!(error.code().as_str(), "ENCODE_FAILED");
        assert!(error.to_string().contains("/layout_profile"));
    }
    let out = from_json(&original.to_string(), &FromJsonOptions::default()).unwrap();
    let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    assert_eq!(h.find("hh:layoutCompatibility").children.len(), 35);
}
