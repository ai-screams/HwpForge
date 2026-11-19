use hwpforge_blueprint::{
    border_fill::{Border, BorderFill, Fill},
    registry::StyleRegistry,
};
use hwpforge_foundation::{BorderLineType, Color, FoundationError, HwpUnit};

fn definition(mask: u8) -> BorderFill {
    let mut border = Border::default();
    for (i, side) in [&mut border.left, &mut border.right, &mut border.top, &mut border.bottom]
        .into_iter()
        .enumerate()
    {
        if mask & (1 << i) != 0 {
            side.line_type = BorderLineType::Solid;
        }
    }
    BorderFill { border, fill: Fill::default() }
}

// 이것을 실패시키는 것: 0 기반 참조, 중복 등록, 한 면의 검증 누락.
#[test]
fn sixteen_border_combinations_are_interned() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    assert!(registry.border_fill(0).is_none());
    for mask in 0..16 {
        let value = definition(mask);
        assert_eq!(registry.register_border_fill(value.clone()).unwrap(), u32::from(mask) + 1);
        assert_eq!(registry.register_border_fill(value.clone()).unwrap(), u32::from(mask) + 1);
        assert_eq!(registry.border_fill(u32::from(mask) + 1), Some(&value));
    }
    assert!(registry.border_fill(17).is_none());
    assert!(registry.border_fill(u32::MAX).is_none());
}

// 이것을 실패시키는 것: 색/굵기/선/채움 검증 삭제 또는 검증 전 삽입.
#[test]
fn invalid_border_definitions_leave_registry_unchanged() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    registry.register_border_fill(definition(3)).unwrap();
    let original = registry.clone();
    for index in 0..4 {
        for kind in 0..3 {
            let mut value = definition(0);
            let side = match index {
                0 => &mut value.border.left,
                1 => &mut value.border.right,
                2 => &mut value.border.top,
                _ => &mut value.border.bottom,
            };
            match kind {
                0 => side.width = Some(HwpUnit::new(1).unwrap()),
                1 => side.color = Some(Color::BLACK),
                _ => side.line_type = BorderLineType::Dash,
            }
            assert!(matches!(
                registry.register_border_fill(value),
                Err(FoundationError::InvalidField { .. })
            ));
            assert_eq!(registry, original);
        }
    }
    let mut value = definition(0);
    value.fill.color = Some(Color::WHITE);
    assert!(registry.register_border_fill(value).is_err());
    assert_eq!(registry, original);
}

// 이것을 실패시키는 것: 빈 목록 직렬화, 역직렬화 값 검증/중복 검사 삭제.
#[test]
fn border_serialization_preserves_ids_and_rejects_invalid_definitions() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let empty = serde_json::to_value(&registry).unwrap();
    assert!(empty.get("border_fills").is_none());
    assert_eq!(serde_json::from_value::<StyleRegistry>(empty).unwrap(), registry);
    for mask in [7, 0, 15] {
        registry.register_border_fill(definition(mask)).unwrap();
    }
    let json = serde_json::to_value(&registry).unwrap();
    assert_eq!(serde_json::from_value::<StyleRegistry>(json.clone()).unwrap(), registry);
    let mut duplicate = json.clone();
    duplicate["border_fills"].as_array_mut().unwrap().push(json["border_fills"][0].clone());
    assert!(serde_json::from_value::<StyleRegistry>(duplicate).is_err());
    let mut invalid = json;
    invalid["border_fills"][0]["border"]["left"]["line_type"] = serde_json::json!("Dash");
    assert!(serde_json::from_value::<StyleRegistry>(invalid).is_err());
}
// 이것을 실패시키는 것: 추가 선 종류나 채움 필드를 허용해 등록 범위를 넓힘.
#[test]
fn every_nondefault_fill_and_line_is_rejected() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for line in [
        BorderLineType::Dash,
        BorderLineType::Dot,
        BorderLineType::DashDot,
        BorderLineType::DashDotDot,
        BorderLineType::LongDash,
        BorderLineType::TripleDot,
        BorderLineType::Double,
        BorderLineType::DoubleSlim,
        BorderLineType::ThickBetweenSlim,
    ] {
        let mut value = definition(0);
        value.border.left.line_type = line;
        assert!(matches!(
            registry.register_border_fill(value),
            Err(FoundationError::InvalidField { .. })
        ));
    }
    for kind in 0..3 {
        let mut value = definition(0);
        match kind {
            0 => value.fill.brush_type = hwpforge_foundation::FillBrushType::Solid,
            1 => value.fill.color = Some(Color::BLACK),
            _ => value.fill.color2 = Some(Color::WHITE),
        }
        assert!(matches!(
            registry.register_border_fill(value),
            Err(FoundationError::InvalidField { .. })
        ));
    }
    assert!(registry.border_fill(1).is_none());
}

// 이것을 실패시키는 것: 양수 Solid 폭 거부, 폭을 뺀 정의 식별성, 복원 색인 누락.
#[test]
fn positive_widths_keep_identity_across_restore() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let widthless = definition(15);
    assert_eq!(registry.register_border_fill(widthless.clone()).unwrap(), 1);
    for (index, width) in [1, 38, 50, 75, 150, 100_000_000].into_iter().enumerate() {
        let mut value = widthless.clone();
        value.border.left.width = Some(HwpUnit::new(width).unwrap());
        assert_eq!(registry.register_border_fill(value.clone()).unwrap(), index as u32 + 2);
        assert_eq!(registry.register_border_fill(value).unwrap(), index as u32 + 2);
    }
    let mut restored: StyleRegistry =
        serde_json::from_value(serde_json::to_value(&registry).unwrap()).unwrap();
    assert_eq!(restored, registry);
    assert_eq!(restored.register_border_fill(widthless).unwrap(), 1);
    for width in [0, -1] {
        let before = restored.clone();
        let mut invalid = definition(15);
        invalid.border.bottom.width = Some(HwpUnit::new(width).unwrap());
        assert!(restored.register_border_fill(invalid).is_err());
        assert_eq!(restored, before);
    }
}

// 이것을 실패시키는 것: 양수 폭 거부 또는 JSON 복원에서 폭 정의를 합침.
#[test]
fn ten_thousand_width_definitions_register_and_restore() {
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for width in 1..=10_000 {
        let mut value = definition(1);
        value.border.left.width = Some(HwpUnit::new(width).unwrap());
        assert_eq!(registry.register_border_fill(value).unwrap(), width as u32);
    }
    let restored: StyleRegistry =
        serde_json::from_value(serde_json::to_value(&registry).unwrap()).unwrap();
    assert_eq!(restored, registry);
    assert_eq!(restored.border_fill(10_000).unwrap().border.left.width.unwrap().as_i32(), 10_000);
}
