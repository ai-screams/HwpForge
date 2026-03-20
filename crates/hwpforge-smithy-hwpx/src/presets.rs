//! Built-in style presets for HWPX document generation.
//!
//! Shared between CLI and MCP bindings so that preset data stays in sync.

use serde::Serialize;

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_foundation::FontId;

use crate::HwpxStyleStore;

/// Information about a single style preset.
#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    /// Preset name (e.g., `"default"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Base font name.
    pub font: String,
    /// Page size (e.g., `"A4"`).
    pub page_size: String,
}

/// Returns the list of built-in presets.
///
/// Available presets: `default`, `modern`, `classic`, `latest`.
pub fn builtin_presets() -> Vec<PresetInfo> {
    vec![
        PresetInfo {
            name: "default".to_string(),
            description: "한컴 Modern 기본 스타일 (함초롬돋움 10pt, A4)".to_string(),
            font: "함초롬돋움".to_string(),
            page_size: "A4".to_string(),
        },
        PresetInfo {
            name: "modern".to_string(),
            description: "깔끔한 현대적 스타일 (맑은 고딕, A4)".to_string(),
            font: "맑은 고딕".to_string(),
            page_size: "A4".to_string(),
        },
        PresetInfo {
            name: "classic".to_string(),
            description: "전통적 문서 스타일 (바탕, A4)".to_string(),
            font: "바탕".to_string(),
            page_size: "A4".to_string(),
        },
        PresetInfo {
            name: "latest".to_string(),
            description: "최신 한컴 스타일 (함초롬바탕 10pt, A4)".to_string(),
            font: "함초롬바탕".to_string(),
            page_size: "A4".to_string(),
        },
    ]
}

/// Create an [`HwpxStyleStore`] from a preset name.
///
/// Returns `None` if the preset name is not recognized.
///
/// # Examples
///
/// ```
/// use hwpforge_smithy_hwpx::presets::style_store_for_preset;
///
/// assert!(style_store_for_preset("modern").is_some());
/// assert!(style_store_for_preset("unknown").is_none());
/// ```
pub fn style_store_for_preset(name: &str) -> Option<HwpxStyleStore> {
    let presets = builtin_presets();
    let preset = presets.iter().find(|p| p.name == name)?;
    // Build a complete style store via from_registry (includes default char shapes,
    // para shapes, styles, and border fills). with_default_fonts only creates fonts.
    // Two font entries satisfy FontIndex(0) and FontIndex(1) in default char shapes.
    let font_id = FontId::new(&preset.font).ok()?;
    let registry = StyleRegistry::with_fonts(vec![font_id.clone(), font_id]);
    HwpxStyleStore::from_registry(&registry).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_returns_four() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 4);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["default", "modern", "classic", "latest"]);
    }

    #[test]
    fn style_store_for_preset_default() {
        let store = style_store_for_preset("default").unwrap();
        // 2 font entries × 7 language groups = 14 fonts
        assert_eq!(store.font_count(), 14);
        assert_eq!(store.iter_fonts().next().unwrap().face_name, "함초롬돋움");
        // Must have default char/para shapes (not just fonts)
        assert!(store.char_shape_count() >= 7);
        assert!(store.para_shape_count() >= 20);
    }

    #[test]
    fn style_store_for_preset_modern() {
        let store = style_store_for_preset("modern").unwrap();
        assert_eq!(store.iter_fonts().next().unwrap().face_name, "맑은 고딕");
    }

    #[test]
    fn style_store_for_preset_classic() {
        let store = style_store_for_preset("classic").unwrap();
        assert_eq!(store.iter_fonts().next().unwrap().face_name, "바탕");
    }

    #[test]
    fn style_store_for_preset_latest() {
        let store = style_store_for_preset("latest").unwrap();
        assert_eq!(store.iter_fonts().next().unwrap().face_name, "함초롬바탕");
    }

    #[test]
    fn style_store_for_preset_unknown_returns_none() {
        assert!(style_store_for_preset("unknown").is_none());
    }
}
