use super::*;

fn character() -> Tag {
    let languages = |name, value| {
        format!("<{name} hangul='{value}' latin='{value}' hanja='{value}' japanese='{value}' other='{value}' symbol='{value}' user='{value}'/>")
    };
    Tag::read(&format!("<charPr useFontSpace='0' useKerning='0' symMark='NONE'>{}{}{}{}<shadow type='NONE' offsetX='10' offsetY='10'/></charPr>", languages("ratio", 100), languages("spacing", 0), languages("relSz", 100), languages("offset", 0))).unwrap()
}

// 이것을 실패시키는 것: 7언어 동일성/존재/저장소 일치 검사 생략, offset 제외.
#[test]
fn language_metrics_require_all_seven_explicit_matching_values() {
    let shape = HwpxCharShape::default();
    assert!(character_allows(&character(), &shape).is_some());
    for name in ["ratio", "spacing", "relSz", "offset"] {
        let mut missing = character();
        missing.children.retain(|c| c.name != name);
        assert!(character_allows(&missing, &shape).is_none(), "missing {name}");
        for language in ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"] {
            for value in [None, Some("3"), Some("invalid")] {
                let mut tag = character();
                let child = tag.children.iter_mut().find(|c| c.name == name).unwrap();
                match value {
                    Some(value) => {
                        child.attrs.insert(language.into(), value.into());
                    }
                    None => {
                        child.attrs.remove(language);
                    }
                }
                assert!(character_allows(&tag, &shape).is_none(), "{name}/{language}/{value:?}");
            }
        }
        let mut tag = character();
        let child = tag.children.iter_mut().find(|c| c.name == name).unwrap();
        for value in child.attrs.values_mut() {
            *value = "3".into();
        }
        assert!(character_allows(&tag, &shape).is_none(), "store mismatch {name}");
        let mut shape = shape.clone();
        match name {
            "ratio" => shape.ratio = 3,
            "spacing" => shape.spacing = 3,
            "relSz" => shape.rel_sz = 3,
            _ => shape.char_offset = 3,
        }
        assert!(character_allows(&tag, &shape).is_some(), "uniform {name}");
    }
}

// 이것을 실패시키는 것: 효과 하나의 검사 삭제, unknown을 NONE으로 처리, NONE shadow offset 거부.
#[test]
fn character_effects_and_font_spacing_need_inactive_evidence() {
    let shape = HwpxCharShape::default();
    for attribute in ["useFontSpace", "useKerning", "symMark"] {
        for value in [None, Some("1"), Some("UNKNOWN")] {
            let mut tag = character();
            match value {
                Some(value) => {
                    tag.attrs.insert(attribute.into(), value.into());
                }
                None => {
                    tag.attrs.remove(attribute);
                }
            }
            assert!(character_allows(&tag, &shape).is_none(), "{attribute}/{value:?}");
        }
    }
    for name in ["bold", "italic", "emboss", "engrave", "supscript", "subscript"] {
        let mut tag = character();
        tag.children.push(Tag::read(&format!("<{name}/>")).unwrap());
        assert!(character_allows(&tag, &shape).is_none(), "{name}");
    }
    for (name, key) in
        [("underline", "type"), ("strikeout", "shape"), ("outline", "type"), ("shadow", "type")]
    {
        for value in ["NONE", "SOLID", "UNKNOWN"] {
            let mut tag = character();
            tag.children.retain(|c| c.name != name);
            tag.children.push(Tag::read(&format!("<{name} {key}='{value}'/>")).unwrap());
            assert_eq!(character_allows(&tag, &shape).is_some(), value == "NONE", "{name}/{value}");
        }
    }
    assert!(character_allows(&character(), &shape).is_some());
    let mut duplicate = character();
    duplicate.children.push(Tag::read("<shadow type='NONE'/>").unwrap());
    assert!(character_allows(&duplicate, &shape).is_none());
    let mut omitted = character();
    omitted.children.retain(|c| c.name != "shadow");
    assert!(character_allows(&omitted, &shape).is_some());
}

// 이것을 실패시키는 것: 서로 다른 빈 run 모양 무시, 끝 표시 허용, 원본 폭 재검사.
#[test]
fn all_direct_runs_including_empty_runs_must_have_one_style() {
    use hwpforge_core::{
        layout::{LayoutCache, LineSeg},
        Run,
    };
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_char_shape(HwpxCharShape::default());
    let mut p = Paragraph::with_runs(
        vec![Run::text("가나", CharShapeIndex::new(0)), Run::text("", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut line = LineSeg::default();
    line.horzsize = 1050;
    p.layout_cache = Some(LayoutCache::new(vec![line]));
    assert!(eligible(&p, "가나", "가", &styles).is_some());
    p.runs[1].char_shape_id = CharShapeIndex::new(1);
    assert!(eligible(&p, "가나", "가", &styles).is_none());
    p.runs[1].char_shape_id = CharShapeIndex::new(0);
    p.end_char_shape = Some(CharShapeIndex::new(0));
    assert!(eligible(&p, "가나", "가", &styles).is_none());
}
