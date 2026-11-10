use super::*;

// 이것을 실패시키는 것: 중간 반올림·절삭, 음수 반올림 방향 반전, 비유한 값 허용.
#[test]
fn rational_units_and_float_canonicalization() {
    for (text, want) in [
        ("0.005pt", 1),
        ("-0.005pt", -1),
        ("1in", 7200),
        ("914400EMU", 7200),
        ("1pc", 1200),
        ("25.4mm", 7200),
        ("2.54cm", 7200),
        ("-0pt", 0),
    ] {
        assert_eq!(units::length(text, 1, -100_000_000).unwrap(), want);
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2147483648.0] {
        assert!(units::canonical_float(value).is_err());
    }
    assert_eq!(units::canonical_float(-0.0).unwrap().to_bits(), 0);
    assert!(units::length("1000000.001pt", 1, -100_000_000).is_err());
    assert!(units::length("9999999999999999999999999999999999999999999", 1, -100_000_000).is_err());
}

fn definitions(styles: &str) -> Definitions {
    let mut d = Definitions::default();
    let xml = format!("<w:styles xmlns:w='{}'>{styles}</w:styles>", crate::xml::W_NS.1);
    d.read(
        xml.as_bytes(),
        "styles.xml",
        "styles",
        &mut crate::xml::TextBudget::default(),
        &mut vec![],
    )
    .unwrap();
    d
}

// 이것을 실패시키는 것: 표 단계 생략, 종류 사이 false 뒤집기, 직접 false 덮어쓰기 생략.
#[test]
fn table_context_precedes_numbering_paragraph_character_and_direct() {
    let mut d=definitions("<w:docDefaults><w:rPrDefault><w:rPr><w:b w:val='0'/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type='table' w:styleId='t'><w:rPr><w:b/></w:rPr><w:pPr><w:ind w:left='1440'/></w:pPr></w:style><w:style w:type='paragraph' w:styleId='p'><w:rPr><w:b/></w:rPr><w:pPr><w:ind w:left='360'/></w:pPr></w:style>");
    let mut level = Delta::default();
    level.para.left = Some(3600);
    d.abstracts.insert(
        1,
        std::collections::BTreeMap::from([(
            0,
            super::super::lists::Level { delta: level, defined: true, ..Default::default() },
        )]),
    );
    d.numbers.insert(
        7,
        super::super::definitions::Number { abstract_id: Some(1), ..Default::default() },
    );
    let mut warnings = vec![];
    let mut r = Resolver::new(d, &mut warnings).unwrap();
    let mut direct = Delta::default();
    direct.para.num_id = Some(7);
    let v = r
        .context(
            Some(r.definitions.style_key("t", "test", "style").unwrap()),
            Some(r.definitions.style_key("p", "test", "style").unwrap()),
            &direct,
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(v.para.left, Some(1800));
    assert_eq!(v.char.toggles[0], Some(false));
    let v = r
        .context(
            Some(r.definitions.style_key("t", "test", "style").unwrap()),
            None,
            &direct,
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(v.para.left, Some(3600));
    assert_eq!(v.char.toggles[0], Some(true));
    r.definitions.defaults.char.toggles[0] = Some(true);
    let from_true = r
        .context(
            Some(r.definitions.style_key("t", "test", "style").unwrap()),
            None,
            &direct,
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(from_true.char.toggles[0], Some(false));
    direct.char.toggles[0] = Some(false);
    direct.para.left = Some(900);
    let v = r
        .context(
            Some(r.definitions.style_key("t", "test", "style").unwrap()),
            None,
            &direct,
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(v.para.left, Some(900));
    assert_eq!(v.char.toggles[0], Some(false));
}

// 이것을 실패시키는 것: 해시 일치만으로 다른 모양 합치기, 모양·delta 한도 검사 제거.
#[test]
fn intern_collision_verification_and_admission() {
    let mut r = Resolver::new(Definitions::default(), &mut vec![]).unwrap();
    let plain = Delta::default();
    let a = r.char_shape(&plain).unwrap().id;
    // Place the wrong candidate into the real hash bucket before its correct candidate.
    let mut bold = plain.clone();
    bold.char.toggles[0] = Some(true);
    let b = r.char_shape(&bold).unwrap().id;
    let wrong = r.chars.values().flatten().find(|(_, id)| *id == b).copied().unwrap();
    for bucket in r.chars.values_mut() {
        bucket.insert(0, wrong);
    }
    assert_eq!(r.char_shape(&plain).unwrap().id, a);
    let p = r.para_shape(&plain).unwrap();
    let mut ind = plain.clone();
    ind.para.left = Some(100);
    let q = r.para_shape(&ind).unwrap();
    for bucket in r.paras.values_mut() {
        bucket.insert(0, q);
    }
    assert_eq!(r.para_shape(&plain).unwrap(), p);
    let shape = r.registry.para_shapes[0].clone();
    r.registry.para_shapes.resize(262_142, shape);
    assert!(r.admit_shape().is_err());
    r.inputs.resize(262_144, RunInput::default());
    assert!(r.run_input(bold, None).is_err());
}

// 이것을 실패시키는 것: 음수 0과 양수 0을 다른 key로 등록, None/false 같은 최종값 분리.
#[test]
fn canonical_effective_values_share_shapes() {
    let mut r = Resolver::new(Definitions::default(), &mut vec![]).unwrap();
    let a = Delta::default();
    let mut b = a.clone();
    b.char.toggles = [Some(false); 5];
    b.char.size = Some(1000);
    b.char.color = Some(0);
    b.char.fonts[1] = Some(r.fallback_font);
    b.para.line = Some(240);
    b.para.before = Some(0);
    assert_eq!(r.char_shape(&a).unwrap().id, r.char_shape(&b).unwrap().id);
    assert_eq!(r.para_shape(&a).unwrap(), r.para_shape(&b).unwrap());
}

// 이것을 실패시키는 것: 유효 조합 캐시를 매 run마다 다시 계산, 글꼴 문자열 확장 예산 누락.
#[test]
fn combination_cache_counts_and_font_storage_are_bounded() {
    let mut r = Resolver::new(Definitions::default(), &mut vec![]).unwrap();
    let id = r.run_input(Delta::default(), None).unwrap();
    let mut p = Paragraph::new(ParaShapeIndex::new(0));
    for _ in 0..100 {
        p.add_run(hwpforge_core::Run::text("A", id));
    }
    r.finish((&mut p, &[], None), None, &Delta::default(), None, "body", &mut vec![]).unwrap();
    assert_eq!(r.combinations_built, 1);
    r.owned_font_bytes = 64 * 1024 * 1024;
    let font = r.definitions.fonts.intern("another font", "test", "font").unwrap();
    assert!(r.font(font).is_err());
}

// 이것을 실패시키는 것: 여덟 theme 참조 중 하나를 다른 슬롯으로 연결, 조건부 표 bold 적용.
#[test]
fn theme_reference_table_and_whole_table_properties() {
    let theme = Theme {
        fonts: [
            [Some(FontKey(0)), Some(FontKey(1)), Some(FontKey(2))],
            [Some(FontKey(3)), Some(FontKey(4)), Some(FontKey(5))],
        ],
    };
    for (name, expected) in [
        ("majorAscii", FontKey(0)),
        ("majorHAnsi", FontKey(0)),
        ("majorEastAsia", FontKey(1)),
        ("majorBidi", FontKey(2)),
        ("minorAscii", FontKey(3)),
        ("minorHAnsi", FontKey(3)),
        ("minorEastAsia", FontKey(4)),
        ("minorBidi", FontKey(5)),
    ] {
        assert_eq!(theme.font(name), Some(expected));
    }
    assert!(theme.font("unknown").is_none());
    let d=definitions("<w:style w:type='table' w:styleId='t'><w:tblStylePr w:type='firstRow'><w:rPr><w:b/></w:rPr></w:tblStylePr><w:tblStylePr w:type='wholeTable'><w:rPr><w:i/></w:rPr></w:tblStylePr></w:style>");
    let r = Resolver::new(d, &mut vec![]).unwrap();
    let v = r
        .context(
            Some(r.definitions.style_key("t", "test", "style").unwrap()),
            None,
            &Delta::default(),
            "body",
            &mut vec![],
        )
        .unwrap();
    assert_eq!(v.char.toggles[0], None);
    assert_eq!(v.char.toggles[1], Some(true));
}

// 이것을 실패시키는 것: 출력되지 않는 cs 이름을 정의 한도에서 제외, 공유 이름을 매번 새 정의로 계수.
#[test]
fn source_fonts_and_definition_level_duplicates_are_bounded() {
    let mut d = Definitions::default();
    for i in 0..16_384 {
        d.fonts.intern(&format!("f{i}"), "part", "font").unwrap();
    }
    d.fonts.intern("f0", "part", "font").unwrap();
    assert!(matches!(
        d.fonts.intern("overflow", "part", "font"),
        Err(DocxError::Limit {
            resource: "source font definitions",
            observed: 16385,
            limit: 16384,
            ..
        })
    ));
    let xml=format!("<w:numbering xmlns:w='{}'><w:num w:numId='1'><w:lvlOverride w:ilvl='0'><w:lvl w:ilvl='0'/><w:lvl w:ilvl='0'/></w:lvlOverride></w:num></w:numbering>",crate::xml::W_NS.1);
    assert!(Definitions::default()
        .read(
            xml.as_bytes(),
            "part",
            "numbering",
            &mut crate::xml::TextBudget::default(),
            &mut vec![]
        )
        .is_err());
}

fn key_bytes(value: &impl std::hash::Hash) -> usize {
    struct Bytes(usize);
    impl std::hash::Hasher for Bytes {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, bytes: &[u8]) {
            self.0 += bytes.len();
        }
    }
    let mut bytes = Bytes(0);
    value.hash(&mut bytes);
    bytes.0
}

// 이것을 실패시키는 것: 정수 글꼴/스타일 키를 공유 문자열 해시로 복원, 문맥·표식 캐시 제거.
#[test]
fn shared_identifiers_do_not_scale_with_references() {
    for length in [1024, 4096] {
        let font = "F".repeat(length);
        let style = "S".repeat(length);
        let mut d = definitions(&format!("<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:hAnsi='{font}'/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type='paragraph' w:styleId='{style}' w:default='1'/>") );
        let font_key = d.defaults.char.fonts[1].unwrap();
        d.theme.fonts[1][0] = Some(font_key);
        let mut r = Resolver::new(d, &mut vec![]).unwrap();
        let mut delta = Delta::default();
        let e = Element {
            namespace: Rc::from(crate::xml::W_NS.1),
            namespace_id: crate::xml::W_NS.0,
            name: "rFonts".into(),
            attributes: vec![(crate::xml::W_NS.0, "hAnsiTheme".into(), "minorHAnsi".into())],
        };
        let initial = (
            r.definitions.fonts.hash_bytes,
            r.definitions.fonts.comparison_bytes,
            r.definitions.fonts.copied_bytes,
            r.definitions.style_lookup_bytes.get(),
            r.copied_font_bytes,
            r.definitions.style_comparison_bytes.get(),
            r.definitions.style_copied_bytes,
        );
        for _ in 0..128 {
            r.property(&mut delta, &e, true, "body", "rFonts", &mut vec![]).unwrap();
            let input = r.run_input(delta.clone(), None).unwrap();
            let mut p = Paragraph::new(ParaShapeIndex::new(0));
            p.add_run(hwpforge_core::Run::text("A", input));
            r.finish(
                (&mut p, &[], None),
                None,
                &Delta::default(),
                Some((&delta, None)),
                "body",
                &mut vec![],
            )
            .unwrap();
            assert_eq!(p.end_char_shape, Some(CharShapeIndex::new(0)));
        }
        assert_eq!(
            (
                r.definitions.fonts.hash_bytes - initial.0,
                r.definitions.fonts.comparison_bytes - initial.1,
                r.definitions.fonts.copied_bytes - initial.2,
                r.definitions.style_lookup_bytes.get() - initial.3,
                r.copied_font_bytes - initial.4,
                r.definitions.style_comparison_bytes.get() - initial.5,
                r.definitions.style_copied_bytes - initial.6
            ),
            (0, 0, 0, 0, 0, 0, 0)
        );
        assert_eq!(
            (r.contexts.len(), r.combinations.len(), r.marks.len(), r.char_shape_calls),
            (1, 1, 1, 3)
        );
        assert_eq!(key_bytes(&font_key), 8);
        assert_eq!(key_bytes(&r.default_paragraph.unwrap()), 8);
        // The projected marker adds one fixed-size optional discriminant.
        assert_eq!(key_bytes(&delta), 296);
        for i in 0..16 {
            let input = r.run_input(delta.clone(), None).unwrap();
            let mut p = Paragraph::new(ParaShapeIndex::new(0));
            p.add_run(hwpforge_core::Run::text("A", input));
            let mut direct = Delta::default();
            direct.para.left = Some(i);
            r.finish((&mut p, &[], None), None, &direct, Some((&delta, None)), "body", &mut vec![])
                .unwrap();
        }
        assert_eq!(r.copied_font_bytes - initial.4, 0);
        assert_eq!(r.registry.char_shape_count(), 1);
    }
}

// 이것을 실패시키는 것: 정의의 자식마다 현재 ID 재조회, 스타일마다 part 문자열 재할당.
#[test]
fn definition_children_and_shared_part_have_linear_byte_work() {
    for length in [1024, 4096] {
        let id = "s".repeat(length);
        let part = "p".repeat(60 * 1024);
        let children = "<w:name w:val='label'/><w:rPr><w:b/></w:rPr>".repeat(128);
        let styles = format!("<w:styles xmlns:w='{}'><w:style w:type='character' w:styleId='{id}'>{children}</w:style>{}</w:styles>", crate::xml::W_NS.1, (0..128).map(|i| format!("<w:style w:type='character' w:styleId='x{i}'/>")).collect::<String>());
        let mut d = Definitions::default();
        d.read(
            styles.as_bytes(),
            &part,
            "styles",
            &mut crate::xml::TextBudget::default(),
            &mut vec![],
        )
        .unwrap();
        assert_eq!(d.part_copied_bytes, 61440);
        assert_eq!(d.style_lookup_bytes.get() - length, 402);
        let parts: Vec<_> = d.styles.iter().flatten().map(|s| &s.part).collect();
        assert!(parts.iter().all(|p| Rc::ptr_eq(parts[0], p)));
    }
}

// 이것을 실패시키는 것: 문맥·최종 조합·문단 표식 개수 한도 삭제 또는 캐시 적중에 한도 적용.
#[test]
fn context_and_combination_limits_precede_allocation() {
    let mut r = Resolver::new(Definitions::default(), &mut vec![]).unwrap();
    let run = r.run_input(Delta::default(), None).unwrap();
    let make = || {
        let mut p = Paragraph::new(ParaShapeIndex::new(0));
        p.add_run(hwpforge_core::Run::text("A", run));
        p
    };
    for i in 0..262_144 {
        let mut d = Delta::default();
        d.para.left = Some(i);
        r.contexts.insert((d, CharDelta::default()), i as usize);
    }
    assert!(matches!(
        r.finish((&mut make(), &[], None), None, &Delta::default(), None, "body", &mut vec![]),
        Err(DocxError::Limit { resource: "format contexts", observed: 262145, limit: 262144, .. })
    ));
    r.contexts.clear();
    let c = r.char_shape(&Delta::default()).unwrap();
    for i in 0..262_144 {
        r.combinations.insert((i + 1, 0), c);
    }
    assert!(matches!(
        r.finish((&mut make(), &[], None), None, &Delta::default(), None, "body", &mut vec![]),
        Err(DocxError::Limit {
            resource: "format combinations",
            observed: 262145,
            limit: 262144,
            ..
        })
    ));
    r.combinations.clear();
    for i in 0..262_144 {
        r.marks.insert((i + 1, 0), c);
    }
    assert!(matches!(
        r.finish(
            (&mut make(), &[], None),
            None,
            &Delta::default(),
            Some((&Delta::default(), None)),
            "body",
            &mut vec![]
        ),
        Err(DocxError::Limit {
            resource: "paragraph mark combinations",
            observed: 262145,
            limit: 262144,
            ..
        })
    ));
    r.marks.insert((0, 0), c);
    assert!(r
        .finish(
            (&mut make(), &[], None),
            None,
            &Delta::default(),
            Some((&Delta::default(), None)),
            "body",
            &mut vec![]
        )
        .is_ok());
}

// 이것을 실패시키는 것: 공통 진단의 전달 위치/분류/동작 변경, missing이 없는 출력에도 경고 합성.
#[test]
fn diagnostic_projection_uses_only_supplied_bits_and_location() {
    let mut warnings = vec![];
    format_warnings(0, false, "part", "definition", &mut warnings).unwrap();
    assert_eq!(warnings.len(), 0);
    format_warnings(8, true, "part", "definition", &mut warnings).unwrap();
    assert_eq!(
        warnings
            .iter()
            .map(|w| (w.code(), w.loss(), w.action(), w.count(), w.part(), w.location()))
            .collect::<Vec<_>>(),
        [
            (
                "DOCX_FORMAT_DEFAULTED",
                "east_asian_font",
                "use_general_font",
                1,
                Some("part"),
                Some("definition")
            ),
            (
                "DOCX_SCRIPT_FORMAT_COLLAPSED",
                "script_format",
                "select_supported_slots",
                1,
                Some("part"),
                Some("definition")
            ),
        ]
    );
}

// 이것을 실패시키는 것: 부호를 잃은 절댓값 최솟값 비교, 반올림된 값 비교, 경계의 등호 거부.
#[test]
fn rational_minima_keep_signed_values_and_exact_boundaries() {
    for (value, minimum, expected) in [
        ("-0.004pt", 0, Err("length below minimum")),
        ("-0.004pt", -100_000_000, Ok(0)),
        ("-0.000pt", 0, Ok(0)),
        ("0.009pt", 1, Err("length below minimum")),
        ("0.01pt", 1, Ok(1)),
        ("-0.015pt", -1, Err("length below minimum")),
        ("-0.01pt", -1, Ok(-1)),
        ("1000000pt", 0, Ok(100_000_000)),
        ("-1000000pt", -100_000_000, Ok(-100_000_000)),
    ] {
        assert_eq!(units::length(value, 5, minimum), expected, "{value}");
    }
}

// 이것을 실패시키는 것: 표식 캐시 적중 무시, 캐시 입력에서 문자 스타일 제거, 직접 pPr를 문자 스타일 앞에만 적용.
#[test]
fn styled_marks_cache_character_resolution_and_keep_direct_precedence() {
    let mut r = Resolver::new(definitions("<w:style w:type='character' w:styleId='c'><w:rPr><w:b/><w:sz w:val='30'/></w:rPr></w:style>"), &mut vec![]).unwrap();
    let style = r.definitions.style_key("c", "body", "rStyle").unwrap();
    let mut direct = Delta::default();
    direct.char.toggles[0] = Some(false);
    let mark = Delta::default();
    let input = r.run_input(mark.clone(), Some(style)).unwrap();
    for _ in 0..3 {
        let mut p = Paragraph::new(ParaShapeIndex::new(0));
        p.add_run(hwpforge_core::Run::text("A", input));
        r.finish(
            (&mut p, &[], None),
            None,
            &direct,
            Some((&mark, Some(style))),
            "body",
            &mut vec![],
        )
        .unwrap();
        for id in [p.runs[0].char_shape_id, p.end_char_shape.unwrap()] {
            let c = &r.registry.char_shapes[id.get()];
            assert_eq!((c.size.as_i32(), c.bold), (1500, false));
        }
    }
    assert_eq!(
        (r.contexts.len(), r.combinations.len(), r.marks.len(), r.char_shape_calls),
        (1, 1, 1, 2)
    );
}

// 이것을 실패시키는 것: 같은 base를 가진 문맥에서 직접 문자 delta를 캐시 키에서 제외.
#[test]
fn character_cache_context_keeps_direct_delta_for_runs_and_marks() {
    let mut d =
        definitions("<w:style w:type='character' w:styleId='c'><w:rPr><w:b/></w:rPr></w:style>");
    d.defaults.char.toggles[0] = Some(false);
    let mut r = Resolver::new(d, &mut vec![]).unwrap();
    let style = r.definitions.style_key("c", "body", "rStyle").unwrap();
    let empty = Delta::default();
    let input = r.run_input(empty.clone(), Some(style)).unwrap();
    let mut direct = Delta::default();
    direct.char.toggles[0] = Some(false);
    for (delta, expected) in [(&empty, true), (&direct, false), (&empty, true), (&direct, false)] {
        let mut p = Paragraph::new(ParaShapeIndex::new(0));
        p.add_run(hwpforge_core::Run::text("A", input));
        r.finish(
            (&mut p, &[], None),
            None,
            delta,
            Some((&empty, Some(style))),
            "body",
            &mut vec![],
        )
        .unwrap();
        assert_eq!(r.registry.char_shapes[p.runs[0].char_shape_id.get()].bold, expected);
        assert_eq!(r.registry.char_shapes[p.end_char_shape.unwrap().get()].bold, expected);
    }
    assert_eq!(
        (r.contexts.len(), r.combinations.len(), r.marks.len(), r.char_shape_calls),
        (2, 2, 2, 4)
    );
}

// 이것을 실패시키는 것: 출력 글꼴 예산 누락·경계 거부 또는 캐시 적중에 예산 적용.
#[test]
fn output_font_storage_has_an_independent_budget() {
    let mut r = Resolver::new(Definitions::default(), &mut vec![]).unwrap();
    for i in 10..262_153 {
        r.fonts.insert(FontKey(i), FontIndex::new(0));
    }
    assert_eq!(r.font(r.fallback_font).unwrap().get(), 0);
    assert_eq!(r.fonts.len(), 262_144);
    assert_eq!(r.font(r.fallback_font).unwrap().get(), 0);
    let overflow = r.definitions.fonts.intern("overflow", "part", "font").unwrap();
    assert!(matches!(
        r.font(overflow),
        Err(DocxError::Limit { resource: "output fonts", observed: 262145, limit: 262144, .. })
    ));
}
