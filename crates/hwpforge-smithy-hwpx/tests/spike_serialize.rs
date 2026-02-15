//! T0 Spike: Validate quick-xml 0.36 namespace serialization.
//!
//! GO/NO-GO gate for Phase 4 dual serde rename approach.
//! All tests must pass before proceeding with encoder implementation.

use quick_xml::de::from_str;
use quick_xml::se::to_string;
use serde::{Deserialize, Serialize};

// ─── Test 1: Dual rename — hh: prefix on serialize ───

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeHead {
    #[serde(rename = "@version", default)]
    version: String,
    #[serde(rename(serialize = "hh:refList", deserialize = "refList"), default)]
    ref_list: Option<SpikeRefList>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeRefList {
    #[serde(rename(serialize = "hh:fontfaces", deserialize = "fontfaces"), default)]
    fontfaces: Option<SpikeFontFaces>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeFontFaces {
    #[serde(rename = "@itemCnt", default)]
    item_cnt: u32,
}

#[test]
fn test1_dual_rename_serialize_hh_prefix() {
    let head = SpikeHead {
        version: "1.4".into(),
        ref_list: Some(SpikeRefList { fontfaces: Some(SpikeFontFaces { item_cnt: 7 }) }),
    };
    let xml = to_string(&head).unwrap();
    // Must contain hh: prefix in output
    assert!(xml.contains("hh:refList"), "Expected hh:refList in output, got: {xml}");
    assert!(xml.contains("hh:fontfaces"), "Expected hh:fontfaces in output, got: {xml}");
}

// ─── Test 2: Bidirectional — serialize then deserialize ───

#[test]
fn test2_bidirectional_roundtrip() {
    let original = SpikeHead {
        version: "1.4".into(),
        ref_list: Some(SpikeRefList { fontfaces: Some(SpikeFontFaces { item_cnt: 3 }) }),
    };
    let xml = to_string(&original).unwrap();

    // Deserialize the serialized output — it will have hh: prefixes
    // which quick-xml strips during deserialization
    let roundtripped: SpikeHead = from_str(&xml).unwrap();
    assert_eq!(original, roundtripped);
}

// ─── Test 3: Option<Presence> → empty element or absent ───

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
struct SpikePresence;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeCharPr {
    #[serde(rename = "@id")]
    id: u32,
    #[serde(
        rename(serialize = "hh:bold", deserialize = "bold"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    bold: Option<SpikePresence>,
    #[serde(
        rename(serialize = "hh:italic", deserialize = "italic"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    italic: Option<SpikePresence>,
}

#[test]
fn test3_presence_serialization() {
    let with_bold = SpikeCharPr { id: 0, bold: Some(SpikePresence), italic: None };
    let xml = to_string(&with_bold).unwrap();
    assert!(xml.contains("hh:bold"), "Expected hh:bold element, got: {xml}");
    assert!(!xml.contains("hh:italic"), "italic should be absent, got: {xml}");

    // Roundtrip
    let rt: SpikeCharPr = from_str(&xml).unwrap();
    assert_eq!(with_bold, rt);
}

// ─── Test 4: $text content ───

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeText {
    #[serde(rename = "$text")]
    text: String,
}

#[test]
fn test4_text_content() {
    let t = SpikeText { text: "안녕하세요".into() };
    let xml = to_string(&t).unwrap();
    assert!(xml.contains("안녕하세요"), "Expected Korean text in output, got: {xml}");

    let rt: SpikeText = from_str(&xml).unwrap();
    assert_eq!(t, rt);
}

// ─── Test 5: @attribute output ───

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeAttr {
    #[serde(rename = "@id")]
    id: u32,
    #[serde(rename = "@height")]
    height: u32,
    #[serde(rename = "@textColor", default)]
    text_color: String,
}

#[test]
fn test5_attribute_output() {
    let a = SpikeAttr { id: 0, height: 1000, text_color: "#000000".into() };
    let xml = to_string(&a).unwrap();
    assert!(xml.contains(r#"id="0""#), "Expected id attr, got: {xml}");
    assert!(xml.contains(r#"height="1000""#), "Expected height attr, got: {xml}");
    assert!(xml.contains(r##"textColor="#000000""##), "Expected textColor attr, got: {xml}");
}

// ─── Test 6: Namespaced attribute @hp:required-namespace ───

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeSwitchCase {
    #[serde(
        rename(serialize = "@hp:required-namespace", deserialize = "@required-namespace"),
        default
    )]
    required_namespace: String,
    #[serde(
        rename(serialize = "hh:margin", deserialize = "margin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    margin: Option<SpikeMargin>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeMargin {
    #[serde(
        rename(serialize = "hc:intent", deserialize = "intent"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    indent: Option<SpikeUnitValue>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeUnitValue {
    #[serde(rename = "@value", default)]
    value: u32,
    #[serde(rename = "@unit", default)]
    unit: String,
}

#[test]
fn test6_namespaced_attribute() {
    let case = SpikeSwitchCase {
        required_namespace: "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar".into(),
        margin: Some(SpikeMargin {
            indent: Some(SpikeUnitValue { value: 0, unit: "HWPUNIT".into() }),
        }),
    };
    let xml = to_string(&case).unwrap();
    assert!(
        xml.contains("hp:required-namespace"),
        "Expected hp:required-namespace attr, got: {xml}"
    );
    assert!(xml.contains("hh:margin"), "Expected hh:margin element, got: {xml}");
    assert!(xml.contains("hc:intent"), "Expected hc:intent element, got: {xml}");
}

// ─── Test 7: Mixed prefix nesting — hp:switch → hh:margin → hc:intent ───

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeSwitch {
    #[serde(
        rename(serialize = "hp:case", deserialize = "case"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    case: Option<SpikeSwitchCase>,
    #[serde(
        rename(serialize = "hp:default", deserialize = "default"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    default: Option<SpikeSwitchDefault>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeSwitchDefault {
    #[serde(
        rename(serialize = "hh:margin", deserialize = "margin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    margin: Option<SpikeMargin>,
    #[serde(
        rename(serialize = "hh:lineSpacing", deserialize = "lineSpacing"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    line_spacing: Option<SpikeLineSpacing>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SpikeLineSpacing {
    #[serde(rename = "@type", default)]
    spacing_type: String,
    #[serde(rename = "@value", default)]
    value: u32,
    #[serde(rename = "@unit", default)]
    unit: String,
}

#[test]
fn test7_mixed_prefix_nesting() {
    let switch = SpikeSwitch {
        case: Some(SpikeSwitchCase {
            required_namespace: "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar".into(),
            margin: Some(SpikeMargin {
                indent: Some(SpikeUnitValue { value: 850, unit: "HWPUNIT".into() }),
            }),
        }),
        default: Some(SpikeSwitchDefault {
            margin: Some(SpikeMargin {
                indent: Some(SpikeUnitValue { value: 850, unit: "HWPUNIT".into() }),
            }),
            line_spacing: Some(SpikeLineSpacing {
                spacing_type: "PERCENT".into(),
                value: 160,
                unit: "HWPUNIT".into(),
            }),
        }),
    };

    let xml = to_string(&switch).unwrap();

    // Verify 3-level prefix nesting
    assert!(xml.contains("hp:case"), "Expected hp:case element, got: {xml}");
    assert!(xml.contains("hp:default"), "Expected hp:default element, got: {xml}");
    assert!(xml.contains("hh:margin"), "Expected hh:margin element, got: {xml}");
    assert!(xml.contains("hc:intent"), "Expected hc:intent element, got: {xml}");
    assert!(xml.contains("hh:lineSpacing"), "Expected hh:lineSpacing element, got: {xml}");

    // Roundtrip: serialize → deserialize
    let rt: SpikeSwitch = from_str(&xml).unwrap();
    assert_eq!(switch, rt);
}
