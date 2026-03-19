//! Tab property definitions.
//!
//! Maps to HWPX `<hh:tabProperties>` and `<hh:tabPr>`.

use hwpforge_foundation::{HwpUnit, TabAlign, TabLeader};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A single explicit tab stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TabStop {
    /// Stop position from the paragraph start.
    pub position: HwpUnit,
    /// Alignment mode at this stop.
    pub align: TabAlign,
    /// Leader style used to fill the gap before the stop.
    pub leader: TabLeader,
}

/// A single tab property definition.
///
/// Maps to HWPX `<hh:tabPr>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TabDef {
    /// Tab property ID (0-based).
    pub id: u32,
    /// Auto-insert tab at left margin.
    pub auto_tab_left: bool,
    /// Auto-insert tab at right margin.
    pub auto_tab_right: bool,
    /// Explicit tab stops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<TabStop>,
}

impl TabDef {
    /// Number of built-in tab definitions reserved by modern Hancom HWPX.
    pub const BUILTIN_COUNT: u32 = 3;
    /// First ID available for user-defined/custom tab definitions.
    pub const FIRST_CUSTOM_ID: u32 = Self::BUILTIN_COUNT;

    /// Returns the 3 default tab properties (한글 Modern).
    ///
    /// Matches golden fixture `tests/fixtures/textbox.hwpx`:
    ///
    /// - id=0: no auto tabs (default for most paragraphs)
    /// - id=1: `autoTabLeft=1` (outline numbering auto-indent)
    /// - id=2: `autoTabRight=1` (right-aligned tab)
    pub fn defaults() -> [Self; 3] {
        [
            Self { id: 0, auto_tab_left: false, auto_tab_right: false, stops: Vec::new() },
            Self { id: 1, auto_tab_left: true, auto_tab_right: false, stops: Vec::new() },
            Self { id: 2, auto_tab_left: false, auto_tab_right: true, stops: Vec::new() },
        ]
    }

    /// Returns true when `id` points at a built-in Hancom tab definition.
    pub fn is_builtin_id(id: u32) -> bool {
        id < Self::BUILTIN_COUNT
    }

    /// Returns true when `id` points at a custom/user-defined tab definition.
    pub fn is_custom_id(id: u32) -> bool {
        id >= Self::FIRST_CUSTOM_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_has_3_entries() {
        let tabs = TabDef::defaults();
        assert_eq!(tabs.len(), 3);
    }

    #[test]
    fn defaults_ids_sequential() {
        let tabs = TabDef::defaults();
        assert_eq!(tabs[0].id, 0);
        assert_eq!(tabs[1].id, 1);
        assert_eq!(tabs[2].id, 2);
    }

    #[test]
    fn defaults_auto_tab_values() {
        let tabs = TabDef::defaults();
        // id=0: no auto tabs
        assert!(!tabs[0].auto_tab_left);
        assert!(!tabs[0].auto_tab_right);
        assert!(tabs[0].stops.is_empty());
        // id=1: auto tab left
        assert!(tabs[1].auto_tab_left);
        assert!(!tabs[1].auto_tab_right);
        assert!(tabs[1].stops.is_empty());
        // id=2: auto tab right
        assert!(!tabs[2].auto_tab_left);
        assert!(tabs[2].auto_tab_right);
        assert!(tabs[2].stops.is_empty());
    }

    #[test]
    fn builtin_id_helpers_match_defaults_boundary() {
        assert!(TabDef::is_builtin_id(0));
        assert!(TabDef::is_builtin_id(2));
        assert!(!TabDef::is_builtin_id(3));
        assert!(!TabDef::is_custom_id(2));
        assert!(TabDef::is_custom_id(3));
    }

    #[test]
    fn tab_stop_preserves_position_and_semantics() {
        let stop = TabStop {
            position: HwpUnit::new(8000).unwrap(),
            align: TabAlign::Decimal,
            leader: TabLeader::dot(),
        };
        assert_eq!(stop.position, HwpUnit::new(8000).unwrap());
        assert_eq!(stop.align, TabAlign::Decimal);
        assert_eq!(stop.leader.as_hwpx_str(), "DOT");
    }
}
