use hwpforge_blueprint::{registry::StyleRegistry, template::Template};
use hwpforge_core::{
    Document, ImageStore, NumberingDef, PageSettings, Paragraph, ParagraphListRef, Run, Section,
};
use hwpforge_foundation::{CharShapeIndex, FontId, NumberingIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    HancomStyleSet, HwpxEncoder, HwpxRegistryBridge, ListHeadPolicy, RegistryBridgeWarning,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

fn registry() -> StyleRegistry {
    StyleRegistry::from_template(&Template::from_yaml("meta:\n  name: bridge\nstyles:\n  body:\n    char_shape:\n      font: Base\n      size: 10pt\n").unwrap()).unwrap()
}

fn encode(bridge: &HwpxRegistryBridge, registry: &StyleRegistry) -> Vec<u8> {
    let paragraphs = registry
        .para_shapes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            Paragraph::with_runs(
                vec![Run::text("text", CharShapeIndex::new(0))],
                ParaShapeIndex::new(i),
            )
        })
        .collect::<Vec<_>>();
    let mut doc = Document::new();
    for _ in 0..2 {
        doc.add_section(Section::with_paragraphs(paragraphs.clone(), PageSettings::a4()));
    }
    let doc = bridge.rebind_draft_document(doc).unwrap().validate().unwrap();
    HwpxEncoder::encode(&doc, bridge.style_store(), &ImageStore::new()).unwrap()
}

fn lists(titles: bool, lists: bool) -> StyleRegistry {
    let mut r = registry();
    r.numberings.clear();
    if lists {
        for id in [1, 2] {
            let mut def = NumberingDef::default_outline();
            def.id = id;
            r.numberings.push(def);
        }
    }
    let base = r.para_shapes[0].clone();
    if titles {
        for level in 0..10 {
            let mut shape = base.clone();
            shape.list = Some(ParagraphListRef::Outline { level });
            r.para_shapes.push(shape);
        }
    }
    if lists {
        for index in 0..2 {
            let mut shape = base.clone();
            shape.list = Some(ParagraphListRef::Number {
                numbering_id: NumberingIndex::new(index),
                level: 0,
            });
            r.para_shapes.push(shape);
        }
    }
    r
}

fn check(policy: ListHeadPolicy, titles: bool, numbered: bool) {
    let r = lists(titles, numbered);
    let diagnosed =
        HwpxRegistryBridge::from_registry_with_diagnostics(&r, HancomStyleSet::Modern, policy)
            .unwrap();
    if policy == ListHeadPolicy::PreserveEffective {
        assert_eq!(
            diagnosed.value.style_store().iter_numberings().map(|n| n.id).collect::<Vec<_>>(),
            if numbered { vec![3, 1, 2] } else { vec![1] }
        );
    }
    let bytes = encode(&diagnosed.value, &r);
    let header = xml::Element::parse(&package::part(&bytes, "Contents/header.xml"));
    let numberings = header.find("hh:numberings");
    let preserve = policy == ListHeadPolicy::PreserveEffective;
    let ids = match (preserve, numbered) {
        (true, true) => vec!["1", "2", "3"],
        (false, true) => vec!["1", "2"],
        (_, false) => vec!["1"],
    };
    assert_eq!(numberings.attr("itemCnt"), Some(ids.len().to_string().as_str()));
    assert_eq!(numberings.children.iter().map(|n| n.attr("id").unwrap()).collect::<Vec<_>>(), ids);
    let outline = &numberings.children[0];
    assert_eq!(outline.attr("start"), Some("0"));
    assert_eq!(outline.children.len(), 10);
    let texts = ["^1.", "^2.", "^3)", "^4)", "(^5)", "(^6)", "^7", "^8", "", ""];
    let formats = [
        "DIGIT",
        "HANGUL_SYLLABLE",
        "DIGIT",
        "HANGUL_SYLLABLE",
        "DIGIT",
        "HANGUL_SYLLABLE",
        "CIRCLED_DIGIT",
        "CIRCLED_HANGUL_SYLLABLE",
        "HANGUL_JAMO",
        "ROMAN_SMALL",
    ];
    for (i, head) in outline.children.iter().enumerate() {
        assert_eq!(head.name, "hh:paraHead");
        assert_eq!(head.text, if preserve { "" } else { texts[i] });
        let expected = [
            ("start", "1"),
            ("level", ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"][i]),
            ("align", "LEFT"),
            ("useInstWidth", "1"),
            ("autoIndent", "1"),
            ("widthAdjust", "0"),
            ("textOffsetType", "PERCENT"),
            ("textOffset", if preserve { "0" } else { "50" }),
            ("numFormat", formats[i]),
            ("charPrIDRef", "4294967295"),
            ("checkable", ["0", "0", "0", "0", "0", "0", "1", "1", "0", "1"][i]),
        ];
        assert_eq!(
            head.attributes,
            expected.into_iter().map(|(k, v)| (k.into(), v.into())).collect()
        );
    }
    if numbered {
        for (i, def) in numberings.children.iter().enumerate().skip(usize::from(preserve)) {
            let first = &def.children[0];
            assert_eq!(first.text, "^1.");
            let template_list = !preserve && i == 1;
            assert_eq!(first.attr("textOffset"), Some(if template_list { "100" } else { "50" }));
            assert_eq!(first.attr("useInstWidth"), Some(if template_list { "0" } else { "1" }));
        }
    }
    for section in ["Contents/section0.xml", "Contents/section1.xml"] {
        let section = xml::Element::parse(&package::part(&bytes, section));
        assert_eq!(section.find("hp:secPr").attr("outlineShapeIDRef"), Some("1"));
        let properties = header.find("hh:paraProperties");
        for (p, shape) in section.children.iter().zip(&r.para_shapes) {
            let index: usize = p.attr("paraPrIDRef").unwrap().parse().unwrap();
            let heading = properties.children[index].child("hh:heading");
            let (kind, id, level) = match shape.list {
                Some(ParagraphListRef::Outline { level }) => ("OUTLINE", "0", level.to_string()),
                Some(ParagraphListRef::Number { numbering_id, .. }) => (
                    "NUMBER",
                    match (preserve, numbering_id.get()) {
                        (true, 0) | (false, 1) => "2",
                        (true, 1) => "3",
                        _ => "1",
                    },
                    "0".into(),
                ),
                None => ("NONE", "0", "0".into()),
                _ => unreachable!(),
            };
            assert_eq!(heading.attr("type"), Some(kind));
            assert_eq!(heading.attr("idRef"), Some(id));
            assert_eq!(heading.attr("level"), Some(level.as_str()));
        }
    }
}

#[test]
fn effective_outline_and_lists_keep_distinct_positions() {
    // 이것을 실패시키는 것: 개요 삽입/빈 텍스트/offset 초기화/번호 매핑/충돌 없는 id 배정/section 참조 제거·변경.
    // 기존 개요 속성·일반 목록 텍스트·문단의 wire 종류/수준/idRef를 바꾸어도 실패한다.
    for titles in [false, true] {
        for lists in [false, true] {
            check(ListHeadPolicy::PreserveEffective, titles, lists);
        }
    }
}

#[test]
fn template_outline_and_list_wire_values_stay_unchanged() {
    // 이것을 실패시키는 것: Template에도 빈 개요 삽입, 첫 번호 폭 덮어쓰기, 두 번째 번호 폭 설정 생략, 빈 store fallback 변경.
    for titles in [false, true] {
        for lists in [false, true] {
            check(ListHeadPolicy::Template, titles, lists);
        }
    }
}

#[test]
fn legacy_entrypoints_match_diagnosed_template_bytes() {
    // 이것을 실패시키는 것: 기존 두 진입점 중 하나를 PreserveEffective로 연결하거나 style_set을 무시하기.
    let r = lists(true, true);
    let default = HwpxRegistryBridge::from_registry(&r).unwrap();
    for style_set in [HancomStyleSet::Modern, HancomStyleSet::Classic, HancomStyleSet::Latest] {
        let old = HwpxRegistryBridge::from_registry_with(&r, style_set).unwrap();
        let new = HwpxRegistryBridge::from_registry_with_diagnostics(
            &r,
            style_set,
            ListHeadPolicy::Template,
        )
        .unwrap();
        assert_eq!(encode(&old, &r), encode(&new.value, &r));
        assert_eq!(new.value.style_store().style_set(), style_set);
        if style_set == HancomStyleSet::Modern {
            assert_eq!(encode(&default, &r), encode(&new.value, &r));
        }
    }
}

#[test]
fn font_fallback_reports_only_the_missing_explicit_slot() {
    // 이것을 실패시키는 것: 특정 칸 경고 생략, 기본 글꼴을 쓰는 빈 칸에도 경고, code/첫 이름/위치/count 변경, 대체 위치를 0 대신 1로.
    for slot in ["font", "latin", "east_asian"] {
        let mut r = registry();
        r.fonts.insert(0, FontId::new("Other").unwrap());
        let cs = &mut r.char_shapes[0];
        match slot {
            "font" => cs.font = "Missing".into(),
            "latin" => cs.font_latin = Some("Missing".into()),
            _ => cs.font_east_asian = Some("Missing".into()),
        }
        let result = HwpxRegistryBridge::from_registry_with_diagnostics(
            &r,
            HancomStyleSet::Modern,
            ListHeadPolicy::Template,
        )
        .unwrap();
        let bytes = encode(&result.value, &r);
        let header = xml::Element::parse(&package::part(&bytes, "Contents/header.xml"));
        let font = header.find("hh:charProperties").children[7].child("hh:fontRef");
        let refs = ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"]
            .map(|key| font.attr(key).unwrap());
        assert_eq!(
            refs,
            match slot {
                "font" => ["0"; 7],
                "latin" => ["1", "0", "1", "1", "1", "1", "1"],
                _ => ["0", "1", "0", "0", "1", "1", "1"],
            }
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code(), "FONT_FALLBACK");
        assert_eq!(
            result.warnings[0],
            RegistryBridgeWarning::FontFallback {
                requested_font: "Missing".into(),
                slot,
                char_shape: 0,
                count: 1
            }
        );
    }
}

#[test]
fn repeated_missing_fonts_keep_first_case_per_slot() {
    // 이것을 실패시키는 것: 이름/위치를 집계 키로 쓰기, 첫 사례 덮어쓰기, 횟수 증가 생략, 칸들을 한 묶음으로 합치기, 첫 발생 순서 뒤집기.
    let mut r = registry();
    let mut first = r.char_shapes[0].clone();
    first.font = "FirstBase".into();
    first.font_latin = Some("FirstLatin".into());
    first.font_east_asian = Some("FirstEast".into());
    r.char_shapes.push(first.clone());
    first.font = "LaterBase".into();
    first.font_latin = Some("LaterLatin".into());
    first.font_east_asian = Some("LaterEast".into());
    r.char_shapes.extend(std::iter::repeat_n(first, 19_999));
    let result = HwpxRegistryBridge::from_registry_with_diagnostics(
        &r,
        HancomStyleSet::Modern,
        ListHeadPolicy::PreserveEffective,
    )
    .unwrap();
    assert_eq!(
        result.warnings,
        [("font", "FirstBase"), ("latin", "FirstLatin"), ("east_asian", "FirstEast")].map(
            |(slot, requested_font)| RegistryBridgeWarning::FontFallback {
                requested_font: requested_font.into(),
                slot,
                char_shape: 1,
                count: 20_000
            }
        )
    );
}

#[test]
fn implicit_and_registered_fonts_need_no_fallback_warning() {
    // 이것을 실패시키는 것: 빈 fonts에 경고, 등록된 이름/비어 있는 칸에도 경고.
    let mut r = registry();
    for empty in [false, true] {
        if empty {
            r.fonts.clear();
        }
        for explicit in [false, true] {
            r.char_shapes[0].font_latin = explicit.then(|| "Base".into());
            r.char_shapes[0].font_east_asian = explicit.then(|| "Base".into());
            let result = HwpxRegistryBridge::from_registry_with_diagnostics(
                &r,
                HancomStyleSet::Modern,
                ListHeadPolicy::Template,
            )
            .unwrap();
            assert!(result.warnings.is_empty());
        }
    }
}

#[test]
fn bullet_marker_width_follows_the_list_policy() {
    // 이것을 실패시키는 것: 입력 서식 보존 정책에서도 글머리 폭을 템플릿 값으로 덮어쓰기, 또는 템플릿 정책에서 덮어쓰기를 빼기.
    let mut r = registry();
    r.bullets.push(hwpforge_core::BulletDef {
        id: 1,
        bullet_char: "•".into(),
        checked_char: None,
        use_image: false,
        para_head: NumberingDef::default_outline().levels[0].clone(),
    });
    let mut shape = r.para_shapes[0].clone();
    shape.list = Some(ParagraphListRef::Bullet {
        bullet_id: hwpforge_foundation::BulletIndex::new(0),
        level: 0,
    });
    r.para_shapes.push(shape);
    for (policy, offset, inst) in
        [(ListHeadPolicy::Template, "100", "0"), (ListHeadPolicy::PreserveEffective, "50", "0")]
    {
        let bridge =
            HwpxRegistryBridge::from_registry_with_diagnostics(&r, HancomStyleSet::Modern, policy)
                .unwrap()
                .value;
        let bytes = encode(&bridge, &r);
        let header = xml::Element::parse(&package::part(&bytes, "Contents/header.xml"));
        let head = header.find("hh:bullets").children[0].child("hh:paraHead");
        assert_eq!(head.attr("textOffset"), Some(offset), "{policy:?}");
        assert_eq!(head.attr("useInstWidth"), Some(inst), "{policy:?}");
    }
}
