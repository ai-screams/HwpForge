#[path = "support/decode_depth.rs"]
mod shared;
use shared::*;

// 이것을 실패시키는 것: 객체 패턴의 여는 괄호 경계 삭제 → ConnectLine·ColumnLine·BorderLine 오계수.
#[test]
fn object_counts_require_variant_boundary() {
    let debug = "Control(Line { }), Shape(Line { }), Control(ConnectLine { }), ColumnLine { }, BorderLine { }";
    assert_eq!(debug_object_count(debug, "Line"), 2);
}
