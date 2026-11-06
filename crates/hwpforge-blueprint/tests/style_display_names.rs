use hwpforge_blueprint::registry::{StyleEntry, StyleRegistry};
use hwpforge_foundation::{CharShapeIndex, FontIndex, ParaShapeIndex, StyleRef};
use serde_json::{json, Value};

#[test]
fn orphan_display_name_is_a_data_error_with_its_key() {
    // 이것을 실패시키는 것: 표시 이름 키 검사 제거 또는 오류에서 키 제거.
    let error = serde_json::from_value::<StyleRegistry>(json!({
        "fonts": [], "char_shapes": [], "para_shapes": [], "style_entries": {},
        "style_display_names": {"missing_style": "제목"}
    }))
    .unwrap_err();
    assert_eq!(error.classify(), serde_json::error::Category::Data);
    assert_eq!(
        error.to_string(),
        "invalid style name 'missing_style': display name references unknown style"
    );
}

#[test]
fn orphan_display_name_is_rejected_from_yaml_too() {
    // 이것을 실패시키는 것: 읽기 경계(try_from)의 고아 키 검사 제거. YAML 읽기도 같은 경계를 지난다.
    let error = serde_yaml::from_str::<StyleRegistry>(
        "fonts: []\nchar_shapes: []\npara_shapes: []\nstyle_entries: {}\nstyle_display_names:\n  missing_style: 제목\n",
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid style name 'missing_style': display name references unknown style"
    );
}

#[test]
fn schema_exposes_optional_string_map() {
    // 이것을 실패시키는 것: wire 스키마에서 맵 숨김, default 제거, 값 타입 또는 registry 제목 변경.
    let schema = serde_json::to_value(schemars::schema_for!(StyleRegistry)).unwrap();
    assert_eq!(schema["title"], "StyleRegistry");
    assert_eq!(schema["properties"]["style_display_names"]["type"], "object");
    assert_eq!(
        schema["properties"]["style_display_names"]["additionalProperties"]["type"],
        "string"
    );
    assert!(!schema["required"].as_array().unwrap().contains(&Value::from("style_display_names")));
}

// Existing JSON may have non-positional refs and unresolved shape indices.
const OLD_JSON: &str = r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{"body":{"style_ref":41,"char_shape_id":7,"para_shape_id":8,"font_id":9}}}"#;

fn registry() -> StyleRegistry {
    serde_json::from_str(OLD_JSON).unwrap()
}

#[test]
fn old_json_is_byte_identical_without_display_names() {
    // 이것을 실패시키는 것: 빈 맵 생략 제거 또는 wire의 맵 default 제거.
    let registry = registry();
    assert_eq!(serde_json::to_string(&registry).unwrap(), OLD_JSON);
    assert!(serde_json::to_value(&registry).unwrap().get("style_display_names").is_none());
    let restored: StyleRegistry =
        serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), OLD_JSON);
}

#[test]
fn display_names_preserve_text_overwrite_and_empty_strings() {
    // 이것을 실패시키는 것: trim 적용, setter 삽입/덮어쓰기 제거, 빈 이름 무시 또는 wire 맵 유실.
    let mut registry = registry();
    let style = StyleRef::new(41);
    for name in ["  한글 제목 · (초안)!  ", "바꾼 이름", ""] {
        registry.set_style_display_name(style, name).unwrap();
        assert_eq!(registry.style_display_name(style), Some(name));
        let value = serde_json::to_value(&registry).unwrap();
        assert_eq!(value["style_display_names"]["body"], name);
        let restored: StyleRegistry = serde_json::from_value(value).unwrap();
        assert_eq!(restored.style_display_name(style), Some(name));
        assert_eq!(restored, registry);
    }
}

#[test]
fn distinct_display_names_stay_with_their_own_styles() {
    // 이것을 실패시키는 것: getter가 요청한 스타일과 무관하게 첫 저장 이름을 돌려주기, 덮어쓰기가 다른 스타일 이름을 바꾸기.
    let mut registry = registry();
    let first = StyleRef::new(41);
    let second = registry
        .register_style(
            "other".into(),
            CharShapeIndex::new(7),
            ParaShapeIndex::new(8),
            FontIndex::new(9),
        )
        .unwrap();
    registry.set_style_display_name(first, "본문").unwrap();
    registry.set_style_display_name(second, "제목").unwrap();
    registry.set_style_display_name(second, "제목 2").unwrap();
    let restored: StyleRegistry =
        serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
    for r in [&registry, &restored] {
        assert_eq!(r.style_display_name(first), Some("본문"));
        assert_eq!(r.style_display_name(second), Some("제목 2"));
    }
}

#[test]
fn identical_display_names_keep_distinct_style_refs() {
    // 이것을 실패시키는 것: 같은 표시 이름을 가진 entry 병합.
    let mut registry = registry();
    let first = StyleRef::new(41);
    let second = registry
        .register_style(
            "other".into(),
            CharShapeIndex::new(7),
            ParaShapeIndex::new(8),
            FontIndex::new(9),
        )
        .unwrap();
    registry.set_style_display_name(first, "같은 제목").unwrap();
    registry.set_style_display_name(second, "같은 제목").unwrap();
    let registry: StyleRegistry =
        serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
    assert_ne!(first, second);
    assert_eq!(registry.style_count(), 2);
    assert_eq!(registry.get_style("body").unwrap().style_ref(), first);
    assert_eq!(registry.get_style("other").unwrap().style_ref(), second);
    assert_eq!(registry.style_display_name(first), Some("같은 제목"));
    assert_eq!(registry.style_display_name(second), Some("같은 제목"));
}

#[test]
fn unknown_ref_is_an_error_without_mutation() {
    // 이것을 실패시키는 것: 없는 ref를 첫 entry로 대체하거나 오류 전에 맵 변경.
    let mut registry = registry();
    registry.set_style_display_name(StyleRef::new(41), "기존").unwrap();
    let before = registry.clone();
    let unknown = StyleRef::new(999);
    let error = registry.set_style_display_name(unknown, "잘못된 이름").unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid style name 'StyleRef(999)': no registered style has this StyleRef"
    );
    assert_eq!(registry, before);
    assert_eq!(registry.style_display_name(unknown), None);
}

#[test]
fn updating_entry_keeps_name_and_new_entry_starts_without_one() {
    // 이것을 실패시키는 것: register_style 갱신 때 표시 이름 삭제 또는 새 키에 이름 복사.
    let mut registry = registry();
    let style = StyleRef::new(41);
    registry.set_style_display_name(style, "원래 이름").unwrap();
    let updated = registry
        .register_style(
            "body".into(),
            CharShapeIndex::new(2),
            ParaShapeIndex::new(3),
            FontIndex::new(4),
        )
        .unwrap();
    assert_eq!(updated, style);
    assert_eq!(registry.style_display_name(updated), Some("원래 이름"));
    let new = registry
        .register_style(
            "new".into(),
            CharShapeIndex::new(2),
            ParaShapeIndex::new(3),
            FontIndex::new(4),
        )
        .unwrap();
    assert_eq!(registry.style_display_name(new), None);
}

#[test]
fn style_entry_remains_copy() {
    // 이것을 실패시키는 것: StyleEntry의 Copy derive 제거 (컴파일 실패).
    fn assert_copy<T: Copy>() {}
    assert_copy::<StyleEntry>();
}
