//! Loader for the shared error-contract manifest
//! (`crates/hwpforge/tests/data/error_contract_018.json`).
//!
//! The manifest is the single source of the causes to test and their
//! literal expected codes; every harness (ops, CLI, MCP, pytest) reads the
//! same file. This module only parses it and checks its own integrity —
//! harness-specific consumption lives with each harness. It depends on
//! `serde` alone so another crate's tests can include it with `#[path]`.

#![allow(dead_code)] // each test binary uses a different part of this module.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

/// Path of the manifest, relative to the workspace root.
pub const MANIFEST_PATH: &str = "crates/hwpforge/tests/data/error_contract_018.json";

/// `CodecStage` variant names, in declaration order.
pub const STAGES: &[&str] = &["Decode", "Admission", "Validation", "Encode", "Verification"];

/// The whole manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Closed list of `Enum::Variant` constructors that count as
    /// generation sites (besides `CodecStage::<X>` literals). Every entry
    /// must exist in the enum inventory, so a variant joins the list in the
    /// change that introduces it.
    pub audited_variants: Vec<String>,
    /// One entry per cause.
    pub cases: Vec<Case>,
}

/// One cause and what every surface must report for it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique case id.
    pub id: String,
    /// Generation sites this case covers.
    #[serde(default)]
    pub sites: Vec<Site>,
    /// Error-enum variants this case covers.
    #[serde(default)]
    pub inventory: Vec<InventoryRef>,
    /// Literal expected code.
    pub expected_code: String,
    /// Per-surface handling.
    pub surfaces: Surfaces,
}

/// One generation-site key as registered.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// Path relative to the workspace root.
    pub file: String,
    /// Enclosing fn key (`fn`, `Type::fn`, `Type as Trait::fn`).
    pub function: String,
    /// `CodecStage` variant name, when the site is a stage literal.
    #[serde(default)]
    pub stage: Option<String>,
    /// `Enum::Variant`, when the site is an audited constructor.
    #[serde(default)]
    pub variant: Option<String>,
    /// Ordinal of this stage or variant within the fn, from 1.
    pub occurrence: u32,
    /// Free-text note for reviewers; never part of the key.
    #[serde(default)]
    pub anchor: String,
}

/// One error-enum variant as registered.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryRef {
    /// Enum name.
    #[serde(rename = "enum")]
    pub enum_name: String,
    /// Variant name.
    pub variant: String,
    /// Stage, for a `Codec(CodecFailure)` wrapper.
    #[serde(default)]
    pub stage: Option<String>,
}

/// The surfaces a case can declare.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surfaces {
    /// Rust (ops classifier, Draft APIs).
    #[serde(default)]
    pub rust: Option<Surface>,
    /// CLI.
    #[serde(default)]
    pub cli: Option<Surface>,
    /// MCP.
    #[serde(default)]
    pub mcp: Option<Surface>,
    /// Python.
    #[serde(default)]
    pub python: Option<Surface>,
}

/// How one surface handles a case.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// fixture, injected, na or unreachable.
    pub mode: Mode,
    /// Operation or command name (required for fixture and injected).
    #[serde(default)]
    pub operation: Option<String>,
    /// Expected CLI exit status.
    #[serde(default)]
    pub exit: Option<i32>,
    /// Why there is no case (required for na and unreachable).
    #[serde(default)]
    pub reason: Option<String>,
}

/// How a surface exercises a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Real input through the public API, CLI or MCP.
    Fixture,
    /// A constructed error value through the classifier or adapter.
    Injected,
    /// The surface has no matching feature.
    Na,
    /// Not reachable on today's production path.
    Unreachable,
}

impl Manifest {
    /// Every surface entry with its case id and surface name.
    pub fn surfaces(&self) -> impl Iterator<Item = (&str, &'static str, &Surface)> {
        self.cases.iter().flat_map(|case| {
            let s = &case.surfaces;
            [("rust", &s.rust), ("cli", &s.cli), ("mcp", &s.mcp), ("python", &s.python)]
                .into_iter()
                .filter_map(move |(name, surface)| {
                    surface.as_ref().map(|surface| (case.id.as_str(), name, surface))
                })
        })
    }
}

/// Parses manifest text.
///
/// # Errors
///
/// The serde message when the text is not a manifest (unknown field, mode
/// or surface included).
pub fn parse(text: &str) -> Result<Manifest, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// Reads and parses the manifest under `workspace_root`.
///
/// # Panics
///
/// When the file is missing or does not parse.
#[must_use]
pub fn load(workspace_root: &Path) -> Manifest {
    let path = workspace_root.join(MANIFEST_PATH);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Integrity problems of a parsed manifest; empty when it is consistent.
#[must_use]
pub fn check(manifest: &Manifest) -> Vec<String> {
    let mut problems = Vec::new();

    let mut audited = BTreeSet::new();
    for entry in &manifest.audited_variants {
        if !audited.insert(entry.as_str()) {
            problems.push(format!("audited variant {entry} listed twice"));
        }
    }

    let mut ids = BTreeSet::new();
    for case in &manifest.cases {
        if !ids.insert(case.id.as_str()) {
            problems.push(format!("case id {} used twice", case.id));
        }
        for site in &case.sites {
            match (&site.stage, &site.variant) {
                (Some(stage), None) if STAGES.contains(&stage.as_str()) => {}
                (None, Some(variant)) if audited.contains(variant.as_str()) => {}
                _ => problems.push(format!(
                    "case {}: site {}::{} needs exactly one known stage or audited variant",
                    case.id, site.file, site.function
                )),
            }
        }
    }

    for case in &manifest.cases {
        for r in &case.inventory {
            if r.stage.as_deref().is_some_and(|stage| !STAGES.contains(&stage)) {
                problems.push(format!(
                    "case {}: inventory {}::{} has unknown stage {:?}",
                    case.id, r.enum_name, r.variant, r.stage
                ));
            }
        }
    }

    for (id, name, surface) in manifest.surfaces() {
        let needs = match surface.mode {
            Mode::Fixture | Mode::Injected => surface.operation.as_deref(),
            Mode::Na | Mode::Unreachable => surface.reason.as_deref(),
        };
        if needs.is_none_or(|text| text.trim().is_empty()) {
            problems.push(format!(
                "case {id}: {name} surface in mode {:?} lacks its operation or reason",
                surface.mode
            ));
        }
    }

    problems
}
