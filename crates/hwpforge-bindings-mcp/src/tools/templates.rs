//! `hwpforge_templates` — Style preset discovery tool.

use serde::Serialize;

use hwpforge_smithy_hwpx::presets::{builtin_presets, PresetInfo};

use crate::output::ToolErrorInfo;

/// Output data from the templates tool.
#[derive(Debug, Serialize)]
pub struct TemplatesData {
    /// List of available presets.
    pub templates: Vec<PresetInfo>,
}

/// Get available templates, optionally filtered by name.
pub fn run_templates(name: Option<&str>) -> Result<TemplatesData, ToolErrorInfo> {
    let presets = builtin_presets();

    if let Some(name) = name {
        let filtered: Vec<PresetInfo> = presets.into_iter().filter(|p| p.name == name).collect();
        if filtered.is_empty() {
            return Err(ToolErrorInfo::new(
                "PRESET_NOT_FOUND",
                format!("Preset '{name}' not found"),
                "Available presets: default. Use hwpforge_templates without a name to list all.",
            ));
        }
        Ok(TemplatesData { templates: filtered })
    } else {
        Ok(TemplatesData { templates: presets })
    }
}
