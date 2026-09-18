//! Manage style presets.

use hwpforge::ops::{self, OpsError};

use crate::compat::{self, Command};

/// List all available presets.
pub fn run_list(json_mode: bool) {
    let presets = ops::templates().presets;
    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "presets": presets,
        });
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Available presets:");
        for p in &presets {
            println!("  {} — {}", p.name, p.description);
        }
    }
}

/// Show details of a specific preset.
///
/// `ops::templates` has no name-filter or not-found concept of its own (it
/// just lists every built-in preset, matching `hwpforge-bindings-mcp`'s
/// `hwpforge_templates`); the by-name lookup and its `PRESET_NOT_FOUND`
/// refusal stay CLI-local, built on top of the shared preset list and
/// routed through the same `OpsError::PresetNotFound` the MCP surface uses,
/// so the `(code, hint, exit)` shape comes from the same `compat` table row.
pub fn run_show(name: &str, json_mode: bool) {
    let presets = ops::templates().presets;
    match presets.into_iter().find(|p| p.name == name) {
        Some(p) => {
            if json_mode {
                let result = serde_json::json!({
                    "status": "ok",
                    "preset": p,
                });
                println!("{}", serde_json::to_string(&result).unwrap());
            } else {
                println!("Preset: {}", p.name);
                println!("  Description: {}", p.description);
                println!("  Font: {}", p.font);
                println!("  Page: {}", p.page_size);
            }
        }
        None => {
            let err = compat::cli_error(
                Command::Templates,
                OpsError::PresetNotFound { name: name.to_string() },
            );
            let exit = compat::exit_code(Command::Templates, &err);
            err.exit(json_mode, exit);
        }
    }
}
