use super::*;
// 이것을 실패시키는 것: 글꼴 intern의 hash 충돌에서 원문 완전 비교 생략.
#[test]
fn font_hash_collisions_preserve_distinct_keys() {
    let mut fonts = Fonts::default();
    let a = fonts.intern("A", "part", "font").unwrap();
    let b = fonts.intern("B", "part", "font").unwrap();
    let hash = fonts.hasher.hash_one("B");
    fonts.buckets.get_mut(&hash).unwrap().insert(0, a);
    assert_eq!(fonts.intern("B", "part", "font").unwrap(), b);
    assert_eq!(fonts.name(a), "A");
    assert_eq!(fonts.name(b), "B");
}

// 이것을 실패시키는 것: 내장 대체 글꼴을 입력 글꼴 예산에 포함.
#[test]
fn internal_fallback_does_not_consume_source_font_limit() {
    let mut fonts = Fonts::default();
    let fallback = fonts.fallback().unwrap();
    for i in 0..16_384 {
        fonts.intern(&format!("f{i}"), "part", "font").unwrap();
    }
    assert_eq!(fonts.source_fonts.len(), 16_384);
    assert_eq!(fonts.fallback().unwrap(), fallback);
    assert!(matches!(
        fonts.intern("overflow", "part", "font"),
        Err(DocxError::Limit {
            resource: "source font definitions",
            observed: 16385,
            limit: 16384,
            ..
        })
    ));
    let mut explicit = Fonts::default();
    explicit.fallback().unwrap();
    explicit.intern("함초롬바탕", "part", "font").unwrap();
    explicit.intern("함초롬바탕", "part", "font").unwrap();
    assert_eq!(explicit.source_fonts.len(), 1);
}
