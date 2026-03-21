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
    /// Matches golden fixture `tests/fixtures/shapes/textbox.hwpx`:
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

    /// Returns built-in tab definitions merged with explicit overrides/custom tabs.
    ///
    /// Built-in ids `0..=2` are always present in the result. Incoming
    /// definitions with the same ids override the built-in defaults.
    pub fn merged_with_defaults<'a>(tabs: impl IntoIterator<Item = &'a Self>) -> Vec<Self> {
        let mut merged = Self::defaults().to_vec();
        for tab in tabs {
            if let Some(existing) = merged.iter_mut().find(|candidate| candidate.id == tab.id) {
                *existing = tab.clone();
            } else {
                merged.push(tab.clone());
            }
        }
        merged.sort_by_key(|tab| tab.id);
        merged
    }

    /// Returns true when `id` resolves to either a built-in definition or one
    /// of the provided custom definition ids.
    pub fn reference_is_known(id: u32, known_custom_ids: impl IntoIterator<Item = u32>) -> bool {
        Self::is_builtin_id(id) || known_custom_ids.into_iter().any(|candidate| candidate == id)
    }

    /// Clamps an unsigned raw tab position into the valid [`HwpUnit`] range.
    pub fn clamp_position_from_unsigned(raw: u64) -> HwpUnit {
        let clamped = raw.min(HwpUnit::MAX_VALUE as u64) as i32;
        HwpUnit::new(clamped).expect("tab stop positions must clamp into valid HwpUnit range")
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

    #[test]
    fn merged_with_defaults_keeps_builtins_and_appends_customs() {
        let tabs = vec![
            TabDef {
                id: 1,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![TabStop {
                    position: HwpUnit::new(5000).unwrap(),
                    align: TabAlign::Right,
                    leader: TabLeader::from_hwpx_str("DASH"),
                }],
            },
            TabDef {
                id: 3,
                auto_tab_left: false,
                auto_tab_right: false,
                stops: vec![TabStop {
                    position: HwpUnit::new(7500).unwrap(),
                    align: TabAlign::Left,
                    leader: TabLeader::none(),
                }],
            },
        ];

        let merged = TabDef::merged_with_defaults(&tabs);

        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].id, 0);
        assert_eq!(merged[1].id, 1);
        assert_eq!(merged[2].id, 2);
        assert_eq!(merged[3].id, 3);
        assert_eq!(merged[1].stops.len(), 1);
        assert_eq!(merged[3].stops.len(), 1);
    }

    #[test]
    fn reference_is_known_accepts_builtin_and_custom_ids() {
        assert!(TabDef::reference_is_known(0, []));
        assert!(TabDef::reference_is_known(3, [3, 5]));
        assert!(!TabDef::reference_is_known(4, [3, 5]));
    }

    #[test]
    fn clamp_position_from_unsigned_caps_large_values() {
        let clamped = TabDef::clamp_position_from_unsigned(u64::MAX);
        assert_eq!(clamped, HwpUnit::new(HwpUnit::MAX_VALUE).unwrap());
    }
}
