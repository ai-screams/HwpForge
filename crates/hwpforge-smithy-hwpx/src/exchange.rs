//! Shared exchange types for JSON round-trip editing.
//!
//! These types define the wire format used by both the CLI (`hwpforge to-json` / `patch`)
//! and the MCP server (`hwpforge_to_json` / `hwpforge_patch`). Having a single definition
//! ensures both bindings produce and consume identical JSON.

use serde::{Deserialize, Serialize};

use hwpforge_core::document::Document;
use hwpforge_core::section::Section;
use hwpforge_core::Draft;

use crate::HwpxStyleStore;

/// Current version of section preservation metadata embedded in section JSON exports.
///
/// v2: 누름틀(ClickHere) 본문 `<hp:t>` 가 `…control.field` 텍스트
/// 슬롯으로 추가됐다. v1 export 는 필드 슬롯이 없어 patch 시 슬롯 개수가
/// 어긋나므로 재-export 를 요구한다.
pub const SECTION_PRESERVATION_VERSION: u32 = 2;

/// Full document export for JSON round-trip.
///
/// Produced by `to-json` (without `--section`) and consumed by `from-json`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ExportedDocument {
    /// Exchange format version, independent of section preservation metadata.
    #[serde(deserialize_with = "deserialize_version")]
    #[cfg_attr(feature = "schemars", schemars(extend("const" = 2)))]
    pub format_version: u32,
    /// The Core document in Draft state.
    pub document: Document<Draft>,
    /// Optional style information for round-trip fidelity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<HwpxStyleStore>,
}

/// Preservation metadata required for byte-preserving section patching.
///
/// This payload is intentionally opaque to end users: `to-json --section`
/// produces it, and `patch` consumes it to update only the touched `<hp:t>`
/// payloads inside the original `sectionN.xml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SectionPreservation {
    /// Version of the preservation metadata contract.
    #[serde(default)]
    pub preservation_version: u32,
    /// Archive path of the source section entry (for example `Contents/section0.xml`).
    pub section_path: String,
    /// SHA-256 digest of the original section XML bytes in lowercase hex.
    pub section_sha256: String,
    /// Text slots, in semantic traversal order, that can be safely rewritten.
    pub text_slots: Vec<PreservedTextSlot>,
}

/// One patchable text slot inside a section export.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PreservedTextSlot {
    /// Stable semantic path (for debugging and validation).
    pub path: String,
    /// Original decoded text content for this slot.
    pub original_text: String,
    /// Whether the original `<hp:t>` contained inline HWPX child markup such
    /// as `<hp:tab/>` or `<hp:fwSpace/>`.
    #[serde(default)]
    pub has_inline_markup: bool,
    /// Raw XML locator used to apply the patch without regenerating the section.
    pub locator: TextLocator,
}

/// Raw XML locator for a patchable text slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum TextLocator {
    /// Existing `<hp:t>` element inside the raw section XML.
    TextElement {
        /// Start byte offset of the full `<hp:t...>` element.
        element_start: usize,
        /// End byte offset of the full `<hp:t...>` element.
        element_end: usize,
        /// Start byte offset of the element's inner text content, if the
        /// element is not self-closing.
        content_start: Option<usize>,
        /// End byte offset of the element's inner text content, if the
        /// element is not self-closing.
        content_end: Option<usize>,
    },
    /// Empty `<hp:run>` placeholder that Core normalized into an empty text run.
    EmptyRun {
        /// Start byte offset of the `<hp:run...>` element.
        run_start: usize,
        /// End byte offset of the `<hp:run...>` element.
        run_end: usize,
    },
}

/// Section-only export for targeted editing.
///
/// Produced by `to-json --section N` and consumed by `patch`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ExportedSection {
    /// Exchange format version, independent of section preservation metadata.
    #[serde(deserialize_with = "deserialize_version")]
    #[cfg_attr(feature = "schemars", schemars(extend("const" = 2)))]
    pub format_version: u32,
    /// Which section index this was extracted from.
    pub section_index: usize,
    /// The section data.
    pub section: Section,
    /// Optional style information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<HwpxStyleStore>,
    /// Optional preservation metadata for byte-preserving section patching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preservation: Option<SectionPreservation>,
}

/// Version of document, section and standalone style exchange.
pub const FORMAT_VERSION: u32 = 2;

/// Standalone style exchange, with the same units and version as document exports.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ExportedStyles {
    /// Exchange format version.
    #[serde(deserialize_with = "deserialize_version")]
    #[cfg_attr(feature = "schemars", schemars(extend("const" = 2)))]
    pub format_version: u32,
    /// Definitions with their source IDs.
    pub styles: HwpxStyleStore,
}

fn deserialize_version<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(d)?;
    if version != FORMAT_VERSION {
        return Err(serde::de::Error::custom(format!("unsupported format_version: {version}")));
    }
    Ok(version)
}

fn invalid_exchange(message: impl std::fmt::Display) -> crate::HwpxError {
    crate::HwpxError::XmlParse { file: "JSON exchange".into(), detail: message.to_string() }
}

const MARGINS: [&str; 5] =
    ["margin_left", "margin_right", "indent", "spacing_before", "spacing_after"];

/// Validates the common exchange envelope and migrates only unversioned legacy margins.
/// Returned diagnostics must be surfaced by the caller. No tab unit is guessed.
pub fn read_exchange_v2(
    value: &mut serde_json::Value,
) -> crate::HwpxResult<Vec<hwpforge_foundation::diagnostics::WarningInfo>> {
    use serde_json::Value;
    if !value.is_object() {
        return Err(invalid_exchange("exchange must be an object"));
    }
    match value.get("format_version") {
        None => migrate_legacy_exchange(value),
        Some(v) if v.as_u64() == Some(u64::from(FORMAT_VERSION)) => {
            if let Some(styles) = value.get("styles").filter(|v| !v.is_null()) {
                let _: HwpxStyleStore =
                    serde_json::from_value(styles.clone()).map_err(invalid_exchange)?;
            }
            Ok(Vec::new())
        }
        Some(Value::Number(v)) => Err(invalid_exchange(format!("unsupported format_version: {v}"))),
        Some(_) => Err(invalid_exchange("format_version must be the integer 2")),
    }
}

/// Migrates the five legacy packed paragraph margins; all other measurements stay intact.
/// Missing source IDs become legacy positional identifiers, never reconstructed source IDs.
pub fn migrate_legacy_exchange(
    value: &mut serde_json::Value,
) -> crate::HwpxResult<Vec<hwpforge_foundation::diagnostics::WarningInfo>> {
    use hwpforge_foundation::diagnostics::WarningInfo;
    use serde_json::{json, Value};
    if !value.is_object() {
        return Err(invalid_exchange("exchange must be an object"));
    }
    if value.get("format_version").is_some() {
        return Err(invalid_exchange("legacy migration requires an unversioned exchange"));
    }
    let mut migrated = value.clone();
    let mut paths = Vec::new();
    if let Some(styles) = migrated.get_mut("styles").filter(|v| !v.is_null()) {
        for (i, tab) in styles["tabs"].as_array().into_iter().flatten().enumerate() {
            for (j, stop) in tab["stops"].as_array().into_iter().flatten().enumerate() {
                if stop.get("position").is_some() {
                    paths.push(format!("$.styles.tabs[{i}].stops[{j}].position"));
                }
            }
        }
        if !paths.is_empty() {
            return Err(crate::HwpxError::LegacyJsonTabs {
                count: paths.len(),
                paths: paths.join(", "),
            });
        }
        if let Some(paras) = styles.get_mut("para_shapes").and_then(Value::as_array_mut) {
            for (i, para) in paras.iter_mut().enumerate() {
                for field in MARGINS {
                    let path = format!("$.styles.para_shapes[{i}].{field}");
                    let raw = para.get(field).and_then(Value::as_i64)
                        .and_then(|n| i32::try_from(n).ok())
                        .ok_or_else(|| invalid_exchange(format!("{path}: legacy margin must be an i32 number; unversioned objects and mixed representations are forbidden")))?;
                    para[field] = serde_json::to_value(
                        crate::paragraph_measure::unpack_margin(raw)
                            .map_err(|e| invalid_exchange(format!("{path}: {e}")))?,
                    )
                    .map_err(invalid_exchange)?;
                    paths.push(path);
                }
            }
        }
    }
    migrated["format_version"] = json!(FORMAT_VERSION);
    // Validate unit objects and ranges before publishing the migration.
    read_exchange_v2(&mut migrated)?;
    *value = migrated;
    Ok(vec![WarningInfo::new("LEGACY_PARAGRAPH_MEASURES", format!(
        "버전 없는 JSON의 문단 여백 {}개를 unpack했습니다. 이미 사라진 direct 여백·case 단위는 복구할 수 없습니다. 정의 참조는 배열 위치입니다. 원본 기반 캐시 보존을 허용하지 않습니다. 원본 HWPX/HWP5에서 to-json을 다시 실행하십시오. 경로: {}",
        paths.len(), paths.join(", ")
    ))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn styles() -> Value {
        serde_json::to_value(ExportedStyles {
            format_version: FORMAT_VERSION,
            styles: crate::style_store_for_preset("default").unwrap(),
        })
        .unwrap()
    }

    // 이것을 실패시키는 것: 변이 8 — 모든 JSON 숫자 ÷2 / v2 재이행.
    #[test]
    fn legacy_migrates_only_five_margins_and_v2_is_idempotent() {
        let mut old = styles();
        old.as_object_mut().unwrap().remove("format_version");
        for para in old["styles"]["para_shapes"].as_array_mut().unwrap() {
            for field in MARGINS {
                para[field] = json!(-1499);
            }
        }
        old["styles"]["char_shapes"][0]["height"] = json!(1800);
        old["styles"]["para_shapes"][0]["line_spacing"] = json!(160);
        old["styles"]["para_shapes"][0]["border_offset"] = json!([100, 200, 300, 400]);
        let before = old.clone();
        let warnings = read_exchange_v2(&mut old).unwrap();
        assert_eq!(warnings[0].code, "LEGACY_PARAGRAPH_MEASURES");
        assert_eq!(old["styles"]["para_shapes"][0]["indent"], json!({"unit":"char","value":-750}));
        assert_eq!(
            old["styles"]["char_shapes"][0]["height"],
            before["styles"]["char_shapes"][0]["height"]
        );
        assert_eq!(old["styles"]["para_shapes"][0]["line_spacing"], json!(160));
        assert_eq!(
            old["styles"]["para_shapes"][0]["border_offset"],
            before["styles"]["para_shapes"][0]["border_offset"]
        );
        let v2 = old.clone();
        assert!(read_exchange_v2(&mut old).unwrap().is_empty());
        assert_eq!(old, v2);
        let parsed: ExportedStyles = serde_json::from_value(old).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["styles"]["para_shapes"][0]["indent"],
            json!({"unit":"char","value":-750})
        );
    }

    // 이것을 실패시키는 것: 옛 탭을 그대로 감싸거나 unpack하여 수용.
    #[test]
    fn legacy_tabs_report_all_paths_and_auto_tabs_are_not_stops() {
        let mut old = styles();
        old.as_object_mut().unwrap().remove("format_version");
        for para in old["styles"]["para_shapes"].as_array_mut().unwrap() {
            for field in MARGINS {
                para[field] = json!(0);
            }
        }
        old["styles"]["tabs"] = json!([{ "id":0, "auto_tab_left":true, "auto_tab_right":false, "stops":[
            {"position":0,"align":"Left","leader":"NONE"},
            {"position":2001,"align":"Left","leader":"NONE"}
        ]}]);
        let error = read_exchange_v2(&mut old).unwrap_err().to_string();
        assert!(error.contains("원래 단위와 case/default 척도를 복구할 수 없어"));
        assert!(error.contains("2건"));
        for i in 0..2 {
            assert!(error.contains(&format!("$.styles.tabs[0].stops[{i}].position")));
        }
        old["styles"]["tabs"][0]["stops"] = json!([]);
        assert!(
            read_exchange_v2(&mut old).is_ok(),
            "automatic tabs are not explicit legacy positions"
        );
    }

    #[test]
    fn future_mixed_unversioned_objects_and_invalid_measures_are_rejected() {
        let mut value = styles();
        value["format_version"] = json!(3);
        assert!(read_exchange_v2(&mut value).is_err());
        value.as_object_mut().unwrap().remove("format_version");
        assert!(read_exchange_v2(&mut value).is_err());
        for bad in [
            json!(100),
            json!({"unit":"char","value":0.5}),
            json!({"unit":"pt","value":100}),
            json!({"unit":"hwpunit","value":100000001}),
            json!({"unit":"char","value":2147483648i64}),
            json!({"unit":"char","value":0,"extra":true}),
        ] {
            let mut value = styles();
            value["styles"]["para_shapes"][0]["indent"] = bad;
            assert!(read_exchange_v2(&mut value).is_err(), "{value}");
        }
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn document_section_and_style_schemas_include_units_and_version() {
        for schema in [
            schemars::schema_for!(ExportedDocument),
            schemars::schema_for!(ExportedSection),
            schemars::schema_for!(ExportedStyles),
        ] {
            let value = serde_json::to_value(schema).unwrap();
            assert_eq!(value["properties"]["format_version"]["const"], 2);
            assert!(value["properties"].get("styles").is_some());
            assert!(value["$defs"].get("ParagraphMeasure").is_some());
            assert_eq!(value["$defs"]["HwpUnit"]["minimum"], -100000000);
        }
    }
}
