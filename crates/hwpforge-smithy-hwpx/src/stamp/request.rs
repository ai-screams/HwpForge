//! Stamp map v2 envelope — Wave 2, P0-ⓑ.
//!
//! The caller-approval map grows a versioned object form carrying class-B
//! cell specs next to the legacy class-A text specs:
//!
//! ```json
//! {
//!   "schema_version": 2,
//!   "source_sha256": "…64 hex…",
//!   "text":  [ { "section": 0, "path": "…", "span": {"start": 0, "end": 3},
//!                "marker": "□", "action": {"field": {"name": "동의"}} } ],
//!   "cells": [ { "table": 3, "at": {"row": 5, "col": 3},
//!                "label": {"at": {"row": 5, "col": 2}, "text": "성 명"},
//!                "action": {"field": {"name": "성명", "hint": "지원자 성명"}} } ]
//! }
//! ```
//!
//! Contract (design §7.3-5, Codex-settled):
//!
//! - The legacy flat `[StampSpec…]` array keeps parsing unchanged; the two
//!   shapes are sniffed by the top-level JSON token, never guessed.
//! - The object form is **versioned and closed**: `schema_version` must be
//!   `2`, unknown fields are rejected (a typo must fail, not be silently
//!   swallowed), and `source_sha256` is mandatory so a map can never be
//!   replayed against a drifted document.
//! - Class-B `field` actions REQUIRE a non-blank `hint` — an empty cell has
//!   no marker to seed the unfilled ClickHere display, and the engine never
//!   invents one (labels only ever appear as plan-side suggestions).

use hwpforge_core::table::grid::GridCoord;

use super::apply::StampSpec;

/// The stamp map schema version this build accepts in the object form.
pub const STAMP_MAP_VERSION: u32 = 2;

/// Caller decision for one class-B cell target.
///
/// Unlike class-A [`StampAction`](super::StampAction), the `hint` is
/// **required**: a canonical
/// empty cell carries no marker text, so the unfilled field body must come
/// from the caller (see module docs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CellStampAction {
    /// Promote the cell content to a named ClickHere field.
    Field {
        /// Unique field name (unique across text+cell specs AND existing
        /// fields).
        name: String,
        /// Unfilled field body and hint (must not be blank).
        hint: String,
    },
    /// Explicitly leave this candidate unstamped.
    Ignore,
}

/// Drift re-verification claim for a detected candidate's label.
///
/// Present on specs approving a detected candidate: preflight re-checks
/// that the label cell at `at` still normalize-matches `text`. Absent for
/// explicit (caller-authored) orphan targets.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CellLabelClaim {
    /// Anchor coordinate of the label cell.
    pub at: GridCoord,
    /// Label text as reported by plan (compared normalized).
    pub text: String,
}

/// One approved class-B target: identity + drift claim + action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CellStampSpec {
    /// Table ordinal in shared inventory (DFS pre-order) document order.
    pub table: usize,
    /// Canonical anchor coordinate of the target cell (covered coordinates
    /// are rejected at preflight, never silently resolved).
    pub at: GridCoord,
    /// Label drift claim; `None` marks an explicit orphan target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<CellLabelClaim>,
    /// What to do with this target.
    pub action: CellStampAction,
}

/// Versioned stamp map (object form).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StampRequestV2 {
    /// Must equal [`STAMP_MAP_VERSION`].
    pub schema_version: u32,
    /// SHA-256 (64 hex chars) of the exact input bytes the plan ran on.
    pub source_sha256: String,
    /// Class-A text specs (same shape as the legacy array).
    #[serde(default)]
    pub text: Vec<StampSpec>,
    /// Class-B cell specs.
    #[serde(default)]
    pub cells: Vec<CellStampSpec>,
}

/// A parsed stamp map: either the legacy flat array or the v2 envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampMap {
    /// Legacy `[StampSpec…]` array — class-A only, no source-hash pinning.
    Legacy(Vec<StampSpec>),
    /// Versioned v2 envelope.
    V2(StampRequestV2),
}

/// Stamp map parse/validation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum StampMapError {
    /// JSON syntax or shape error (including unknown fields).
    Parse(String),
    /// The top-level JSON value is neither an array nor an object.
    UnsupportedShape,
    /// `schema_version` is not [`STAMP_MAP_VERSION`].
    UnsupportedVersion(u32),
    /// `source_sha256` is not a 64-char hex SHA-256.
    InvalidSourceHash(String),
    /// A cell `field` action has a blank hint.
    BlankHint {
        /// The offending field name.
        name: String,
    },
}

impl std::fmt::Display for StampMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(detail) => write!(f, "stamp map parse error: {detail}"),
            Self::UnsupportedShape => {
                write!(f, "stamp map must be a spec array (legacy) or a versioned object (v2)")
            }
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported stamp map schema_version {v} (expected {STAMP_MAP_VERSION})")
            }
            Self::InvalidSourceHash(got) => {
                write!(f, "source_sha256 must be 64 hex chars, got {got:?}")
            }
            Self::BlankHint { name } => {
                write!(f, "cell spec {name:?}: hint must not be blank (empty cells have no marker to fall back on)")
            }
        }
    }
}

impl std::error::Error for StampMapError {}

/// Parses a stamp map in either shape and validates the v2 contract.
///
/// The shape is decided by the top-level JSON token: an array parses as the
/// legacy `Vec<StampSpec>` (unchanged semantics), an object as
/// [`StampRequestV2`] with unknown fields rejected, `schema_version`
/// pinned, `source_sha256` well-formed, and every cell `field` hint
/// non-blank. Anything else is [`StampMapError::UnsupportedShape`].
pub fn parse_stamp_map(json: &str) -> Result<StampMap, StampMapError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| StampMapError::Parse(e.to_string()))?;
    match value {
        serde_json::Value::Array(_) => {
            let specs: Vec<StampSpec> =
                serde_json::from_value(value).map_err(|e| StampMapError::Parse(e.to_string()))?;
            Ok(StampMap::Legacy(specs))
        }
        serde_json::Value::Object(_) => {
            let request: StampRequestV2 =
                serde_json::from_value(value).map_err(|e| StampMapError::Parse(e.to_string()))?;
            validate_request(&request)?;
            Ok(StampMap::V2(request))
        }
        _ => Err(StampMapError::UnsupportedShape),
    }
}

fn validate_request(request: &StampRequestV2) -> Result<(), StampMapError> {
    if request.schema_version != STAMP_MAP_VERSION {
        return Err(StampMapError::UnsupportedVersion(request.schema_version));
    }
    let sha = &request.source_sha256;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(StampMapError::InvalidSourceHash(sha.clone()));
    }
    for spec in &request.cells {
        if let CellStampAction::Field { name, hint } = &spec.action {
            if hint.trim().is_empty() {
                return Err(StampMapError::BlankHint { name: name.clone() });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stamp::StampAction;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn v2_json(body: &str) -> String {
        format!(r#"{{"schema_version":2,"source_sha256":"{SHA}",{body}}}"#)
    }

    #[test]
    fn legacy_flat_array_parses_unchanged() {
        let json = r#"[{"section":0,"path":"paragraphs[0].runs[0].text",
            "span":{"start":0,"end":3},"marker":"□","action":"ignore"}]"#;
        match parse_stamp_map(json).unwrap() {
            StampMap::Legacy(specs) => {
                assert_eq!(specs.len(), 1);
                assert_eq!(specs[0].marker, "□");
                assert_eq!(specs[0].action, StampAction::Ignore);
            }
            other => panic!("expected legacy map, got {other:?}"),
        }
    }

    #[test]
    fn v2_object_parses_with_text_and_cells() {
        let json = v2_json(
            r#""text":[{"section":0,"path":"p","span":{"start":0,"end":1},
                "marker":"□","action":{"field":{"name":"동의","hint":null}}}],
              "cells":[{"table":3,"at":{"row":5,"col":3},
                "label":{"at":{"row":5,"col":2},"text":"성 명"},
                "action":{"field":{"name":"성명","hint":"지원자 성명"}}}]"#,
        );
        match parse_stamp_map(&json).unwrap() {
            StampMap::V2(req) => {
                assert_eq!(req.schema_version, 2);
                assert_eq!(req.source_sha256, SHA);
                assert_eq!(req.text.len(), 1);
                assert_eq!(req.cells.len(), 1);
                let cell = &req.cells[0];
                assert_eq!(cell.table, 3);
                assert_eq!(cell.at, GridCoord::new(5, 3));
                assert_eq!(cell.label.as_ref().unwrap().text, "성 명");
                assert_eq!(
                    cell.action,
                    CellStampAction::Field {
                        name: "성명".into(), hint: "지원자 성명".into()
                    },
                );
            }
            other => panic!("expected v2 map, got {other:?}"),
        }
    }

    #[test]
    fn v2_defaults_empty_spec_lists() {
        let json = v2_json(r#""cells":[]"#);
        match parse_stamp_map(&json).unwrap() {
            StampMap::V2(req) => {
                assert!(req.text.is_empty());
                assert!(req.cells.is_empty());
            }
            other => panic!("expected v2 map, got {other:?}"),
        }
    }

    #[test]
    fn v2_unknown_top_level_field_rejected() {
        let json = v2_json(r#""cell":[]"#); // typo of "cells"
        let err = parse_stamp_map(&json).unwrap_err();
        assert!(matches!(err, StampMapError::Parse(ref d) if d.contains("cell")), "{err}");
    }

    #[test]
    fn v2_unknown_cell_spec_field_rejected() {
        let json = v2_json(
            r#""cells":[{"table":0,"at":{"row":0,"col":1},"labe":null,"action":"ignore"}]"#,
        );
        assert!(matches!(parse_stamp_map(&json).unwrap_err(), StampMapError::Parse(_)));
    }

    #[test]
    fn v2_wrong_schema_version_rejected() {
        for version in [1, 3] {
            let json =
                format!(r#"{{"schema_version":{version},"source_sha256":"{SHA}","cells":[]}}"#);
            assert!(matches!(
                parse_stamp_map(&json).unwrap_err(),
                StampMapError::UnsupportedVersion(v) if v == version,
            ));
        }
    }

    #[test]
    fn v2_malformed_source_hash_rejected() {
        for sha in ["", "abc", &format!("{}g", &SHA[..63])] {
            let json = format!(r#"{{"schema_version":2,"source_sha256":"{sha}","cells":[]}}"#);
            assert!(matches!(
                parse_stamp_map(&json).unwrap_err(),
                StampMapError::InvalidSourceHash(_),
            ));
        }
    }

    #[test]
    fn v2_blank_cell_hint_rejected() {
        for hint in ["", "   "] {
            let json = v2_json(&format!(
                r#""cells":[{{"table":0,"at":{{"row":0,"col":1}},
                    "action":{{"field":{{"name":"성명","hint":"{hint}"}}}}}}]"#,
            ));
            assert!(matches!(
                parse_stamp_map(&json).unwrap_err(),
                StampMapError::BlankHint { ref name } if name == "성명",
            ));
        }
    }

    #[test]
    fn v2_ignore_action_needs_no_hint() {
        let json = v2_json(r#""cells":[{"table":0,"at":{"row":0,"col":1},"action":"ignore"}]"#);
        match parse_stamp_map(&json).unwrap() {
            StampMap::V2(req) => {
                assert_eq!(req.cells[0].action, CellStampAction::Ignore);
                assert!(req.cells[0].label.is_none());
            }
            other => panic!("expected v2 map, got {other:?}"),
        }
    }

    #[test]
    fn non_array_non_object_rejected() {
        assert!(matches!(parse_stamp_map("42").unwrap_err(), StampMapError::UnsupportedShape));
        assert!(matches!(parse_stamp_map("\"x\"").unwrap_err(), StampMapError::UnsupportedShape));
    }

    #[test]
    fn cell_spec_serialization_shape_is_locked() {
        // Golden shape — the CLI/MCP JSON contract for one cell spec.
        let spec = CellStampSpec {
            table: 3,
            at: GridCoord::new(5, 3),
            label: Some(CellLabelClaim { at: GridCoord::new(5, 2), text: "성 명".into() }),
            action: CellStampAction::Field {
                name: "성명".into(), hint: "지원자 성명".into()
            },
        };
        let expected = serde_json::json!({
            "table": 3,
            "at": {"row": 5, "col": 3},
            "label": {"at": {"row": 5, "col": 2}, "text": "성 명"},
            "action": {"field": {"name": "성명", "hint": "지원자 성명"}},
        });
        assert_eq!(serde_json::to_value(&spec).unwrap(), expected);

        // Explicit orphan spec omits `label` entirely.
        let orphan = CellStampSpec {
            table: 0,
            at: GridCoord::new(2, 2),
            label: None,
            action: CellStampAction::Ignore,
        };
        let expected = serde_json::json!({
            "table": 0,
            "at": {"row": 2, "col": 2},
            "action": "ignore",
        });
        assert_eq!(serde_json::to_value(&orphan).unwrap(), expected);
    }
}
