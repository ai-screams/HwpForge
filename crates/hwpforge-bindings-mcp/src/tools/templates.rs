//! `hwpforge_templates` — Style preset discovery tool.

use serde::Serialize;

use hwpforge::ops::{self, OpsError};
use hwpforge_smithy_hwpx::presets::PresetInfo;

use crate::compat::{self, Tool};
use crate::output::ToolErrorInfo;

/// Output data from the templates tool.
#[derive(Debug, Serialize)]
pub struct TemplatesData {
    /// List of available presets.
    pub templates: Vec<PresetInfo>,
}

/// Get available templates, optionally filtered by name.
///
/// `ops::templates` has no name-filter or not-found concept of its own (it
/// just lists every built-in preset); the by-name lookup and its
/// `PRESET_NOT_FOUND` refusal stay MCP-local, built on top of the shared
/// preset list.
pub fn run_templates(name: Option<&str>) -> Result<TemplatesData, ToolErrorInfo> {
    let presets = ops::templates().presets;

    if let Some(name) = name {
        let filtered: Vec<PresetInfo> = presets.into_iter().filter(|p| p.name == name).collect();
        if filtered.is_empty() {
            return Err(compat::tool_error(
                Tool::Templates,
                OpsError::PresetNotFound { name: name.to_string() },
            ));
        }
        Ok(TemplatesData { templates: filtered })
    } else {
        Ok(TemplatesData { templates: presets })
    }
}
