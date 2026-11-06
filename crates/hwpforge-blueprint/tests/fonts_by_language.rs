use hwpforge_blueprint::{
    registry::StyleRegistry,
    schema::template_schema,
    style::{CharShape, PartialCharShape},
    template::Template,
};
use serde_json::{json, Value};

fn template(slots: &str) -> Template {
    Template::from_yaml(&format!("meta:\n  name: fonts\nstyles:\n  body:\n    char_shape:\n      font: Base\n      size: 10pt\n{slots}")).unwrap()
}

#[test]
fn yaml_optional_fonts_resolve_and_register_in_order() {
    // 이것을 실패시키는 것: 칸 역직렬화/resolve/등록을 제거하거나 등록 순서를 뒤집기.
    for (slots, latin, east, fonts) in [
        ("", None, None, vec!["Base"]),
        ("      font_latin: Latin\n", Some("Latin"), None, vec!["Base", "Latin"]),
        ("      font_east_asian: East\n", None, Some("East"), vec!["Base", "East"]),
        (
            "      font_latin: Latin\n      font_east_asian: East\n",
            Some("Latin"),
            Some("East"),
            vec!["Base", "Latin", "East"],
        ),
        (
            "      font_latin: Base\n      font_east_asian: Base\n",
            Some("Base"),
            Some("Base"),
            vec!["Base"],
        ),
    ] {
        let t = template(slots);
        let partial = serde_json::to_value(t.styles["body"].char_shape.as_ref().unwrap()).unwrap();
        assert_eq!(partial.get("font_latin").and_then(Value::as_str), latin);
        assert_eq!(partial.get("font_east_asian").and_then(Value::as_str), east);
        let r = StyleRegistry::from_template(&t).unwrap();
        let shape = serde_json::to_value(&r.char_shapes[0]).unwrap();
        assert_eq!(shape.get("font_latin").and_then(Value::as_str), latin);
        assert_eq!(shape.get("font_east_asian").and_then(Value::as_str), east);
        assert_eq!(r.fonts.iter().map(|f| f.as_str()).collect::<Vec<_>>(), fonts);
        assert_eq!(r.get_style("body").unwrap().font_id.get(), 0);
    }
}

#[test]
fn partial_fonts_inherit_and_override_independently() {
    // 이것을 실패시키는 것: Some 병합 제거 또는 None 자식으로 부모 칸 지우기.
    let parent: PartialCharShape =
        serde_json::from_value(json!({"font_latin":"ParentLatin", "font_east_asian":"ParentEast"}))
            .unwrap();
    for (child, latin, east) in [
        (json!({}), "ParentLatin", "ParentEast"),
        (json!({"font_latin":"ChildLatin"}), "ChildLatin", "ParentEast"),
        (json!({"font_east_asian":"ChildEast"}), "ParentLatin", "ChildEast"),
        (
            json!({"font_latin":"ChildLatin", "font_east_asian":"ChildEast"}),
            "ChildLatin",
            "ChildEast",
        ),
    ] {
        let mut merged = parent.clone();
        merged.merge(&serde_json::from_value(child).unwrap());
        let value = serde_json::to_value(merged).unwrap();
        assert_eq!(value["font_latin"], latin);
        assert_eq!(value["font_east_asian"], east);
    }
}

#[test]
fn registry_json_preserves_fonts_and_omits_absent_slots() {
    // 이것을 실패시키는 것: 칸 직렬화/역직렬화 생략 또는 None 키를 null로 출력하기.
    for slots in ["", "      font_latin: Latin\n      font_east_asian: East\n"] {
        let registry = StyleRegistry::from_template(&template(slots)).unwrap();
        let value = serde_json::to_value(&registry).unwrap();
        let expected = if slots.is_empty() { None } else { Some(json!("Latin")) };
        assert_eq!(value["char_shapes"][0].get("font_latin"), expected.as_ref());
        let expected = if slots.is_empty() { None } else { Some(json!("East")) };
        assert_eq!(value["char_shapes"][0].get("font_east_asian"), expected.as_ref());
        let restored: StyleRegistry = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(restored, registry);
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
    }
    let partial = serde_json::to_value(PartialCharShape::default()).unwrap();
    assert_eq!(partial.get("font_latin"), None);
    assert_eq!(partial.get("font_east_asian"), None);
}

#[test]
fn schemas_expose_optional_font_slots() {
    // 이것을 실패시키는 것: schemars에서 칸을 숨기거나 필수 속성으로 만들기.
    let template = serde_json::to_value(template_schema()).unwrap();
    let resolved = serde_json::to_value(schemars::schema_for!(CharShape)).unwrap();
    for schema in [&template["$defs"]["PartialCharShape"], &resolved] {
        for field in ["font_latin", "font_east_asian"] {
            assert_eq!(schema["properties"][field]["type"], json!(["string", "null"]));
            assert!(!schema["required"]
                .as_array()
                .is_some_and(|items| items.iter().any(|v| v == field)));
        }
    }
}
