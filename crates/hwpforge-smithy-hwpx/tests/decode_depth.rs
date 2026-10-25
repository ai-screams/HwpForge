//! Public decode admission: parser depth and semantic depth are distinct.
#[path = "support/depth_package.rs"]
mod support;
use hwpforge_core::CoreError;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxError};
use support::*;

// Core objects appear inside RunContent or GroupChild tuple variants in Debug.
fn debug_object_count(debug: &str, object: &str) -> usize {
    debug.matches(&format!("({object} {{")).count()
}

// 이것을 실패시키는 것: 객체 패턴의 여는 괄호 경계 삭제 → ConnectLine·ColumnLine·BorderLine 오계수.
#[test]
fn object_counts_require_variant_boundary() {
    let debug = "Control(Line { }), Shape(Line { }), Control(ConnectLine { }), ColumnLine { }, BorderLine { }";
    assert_eq!(debug_object_count(debug, "Line"), 2);
}

// 이것을 실패시키는 것: Core 상한 31, 파서 상한 31.
#[test]
fn accepted_boundaries_preserve_deepest_and_objects() {
    on_big_stack(|| {
        let cases = [
            (nested_section("T", 32), Place::Body, 32, 0, 0, 0, 0),
            (nested_section("B", 32), Place::Body, 0, 0, 32, 0, 0),
            (section_with(&groups(31)), Place::Body, 0, 31, 1, 0, 0),
            (section_with(&pairs(16)), Place::Body, 0, 16, 16, 0, 0),
            (section_with(&pictures(16, false)), Place::Body, 0, 0, 0, 16, 0),
            (section_with(&pictures(16, true)), Place::Body, 0, 0, 0, 16, 0),
            (tables_with_leaf(31, &pictures(1, false)), Place::Body, 31, 0, 0, 1, 0),
            (tables_with_leaf(31, &line()), Place::Body, 31, 0, 0, 0, 1),
            (nested_section("T", 31), Place::Master(7), 31, 0, 0, 0, 0),
            (tables_with_leaf(31, &pictures(1, false)), Place::Master(7), 31, 0, 0, 1, 0),
            (section_with(&pairs(16)), Place::Master(7), 0, 16, 16, 0, 0),
        ];
        for (xml, place, tables, groups, boxes, images, lines) in cases {
            let doc = HwpxDecoder::decode(&package(xml, place)).unwrap();
            for section in doc.document.sections() {
                section.check_nesting_depth().unwrap();
            }
            // Debug is a complete tree projection, including GroupChild and auxiliary roots.
            let debug = format!("{:?}", doc.document.sections());
            assert!(debug.contains(DEEPEST), "{place:?}: deepest text lost");
            for (needle, expected) in [
                ("Table", tables),
                ("Group", groups),
                ("TextBox", boxes),
                ("Image", images),
                ("Line", lines),
            ] {
                assert_eq!(debug_object_count(&debug, needle), expected, "{place:?}: {needle}");
            }
        }
    });
}

// 이것을 실패시키는 것: decode 관문 삭제, 본문만 검사, section= 보정 삭제, 첫 구역만 검사, E4006 오매핑.
#[test]
fn semantic_overflow_is_core_error_in_every_root_and_section() {
    on_big_stack(|| {
        for (xml, place, section) in [
            (section_with(&groups(32)), Place::Body, 0),
            (section_with(&pairs(17)), Place::Body, 0),
            (section_with(&pictures(17, false)), Place::Body, 0),
            (section_with(&pictures(17, true)), Place::Body, 0),
            (tables_with_leaf(32, &pictures(1, false)), Place::Body, 0),
            (tables_with_leaf(32, &line()), Place::Body, 0),
            (section_with(&groups(32)), Place::Header, 0),
            (section_with(&pairs(17)), Place::Footer, 0),
            (section_with(&pairs(17)), Place::Master(7), 0),
            (section_with(&pictures(17, false)), Place::Master(7), 0),
            (section_with(&groups(32)), Place::Second, 1),
        ] {
            let err =
                HwpxDecoder::decode(&package(xml, place)).expect_err("depth must be rejected");
            assert_eq!(err.code().to_string(), "E4008", "{place:?}: {err}");
            match err {
                HwpxError::Core(CoreError::InvalidStructure { context, reason }) => {
                    assert_eq!(context, "nesting_depth");
                    assert!(reason.contains(&format!("section={section},")), "{place:?}: {reason}");
                }
                other => panic!("{place:?}: {other:?}"),
            }
        }
    });
}

// 이것을 실패시키는 것: 각 가드의 depth + 1 → depth, part 삭제/section0 고정,
// masterpage part 고정; 파서 상한 33 (Core 수용 가능한 바탕쪽 T32도 파서 거절).
#[test]
fn parser_guards_report_attempted_level_and_actual_part() {
    on_big_stack(|| {
        for place in [Place::Second, Place::Master(7), Place::Master(11)] {
            let (n, part) = match place {
                Place::Second => (33, "Contents/section1.xml".to_owned()),
                Place::Master(n) => (32, format!("Contents/masterpage{n}.xml")),
                _ => unreachable!(),
            };
            for (xml, guard) in [
                (nested_section("T", n), "table"),
                (nested_section("B", n), "sublist"),
                (section_with(&groups(n)), "group"),
                (tables_with_leaf(n, &pictures(1, false)), "table"),
            ] {
                let err =
                    HwpxDecoder::decode(&package(xml, place)).expect_err("depth must be rejected");
                assert_eq!(err.code().to_string(), "E4006");
                match err {
                    HwpxError::InvalidStructure { detail } => assert_eq!(
                        detail,
                        format!("{guard} nesting depth 33 exceeds limit of 32 in {part}")
                    ),
                    other => panic!("{place:?} {guard}: {other:?}"),
                }
            }
        }
    });
}

// 이것을 실패시키는 것: XML_RECURSION_LIMIT 10000, XmlParse file을 section0 고정.
#[test]
fn xml_budget_keeps_its_error_and_second_section_path() {
    on_big_stack(|| {
        let err = HwpxDecoder::decode(&package(nested_section("T", 42), Place::Second))
            .expect_err("depth must be rejected");
        assert_eq!(err.code().to_string(), "E4003");
        match err {
            HwpxError::XmlParse { file, detail } => {
                assert_eq!(file, "Contents/section1.xml");
                assert!(detail.contains("recursion limit of 224"));
            }
            other => panic!("{other:?}"),
        }
    });
}
