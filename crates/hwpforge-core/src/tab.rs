//! Tab property definitions.
//!
//! Maps to HWPX `<hh:tabProperties>` and `<hh:tabPr>`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}

impl TabDef {
    /// Returns the 3 default tab properties (한글 Modern).
    ///
    /// Matches golden fixture `tests/fixtures/textbox.hwpx`:
    ///
    /// - id=0: no auto tabs (default for most paragraphs)
    /// - id=1: `autoTabLeft=1` (outline numbering auto-indent)
    /// - id=2: `autoTabRight=1` (right-aligned tab)
    pub fn defaults() -> [Self; 3] {
        [
            Self { id: 0, auto_tab_left: false, auto_tab_right: false },
            Self { id: 1, auto_tab_left: true, auto_tab_right: false },
            Self { id: 2, auto_tab_left: false, auto_tab_right: true },
        ]
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
        // id=1: auto tab left
        assert!(tabs[1].auto_tab_left);
        assert!(!tabs[1].auto_tab_right);
        // id=2: auto tab right
        assert!(!tabs[2].auto_tab_left);
        assert!(tabs[2].auto_tab_right);
    }
}
