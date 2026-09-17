//! Audit helper: parses the upstream error/warning enums that `ops` wraps.
//!
//! This is a **manifest-scoped audit**, not global discovery. The manifest
//! below names every file and enum the audit covers; anything outside it is
//! invisible on purpose, and [`ManifestEntry::not_wrapped`] records the
//! public `*Error`/`*Warning` enums that live in those files but that
//! `OpsError`/`OpsWarning` deliberately do not wrap, each with its reason.
//!
//! Parsing uses `syn`, never a regex or a brace counter, so variant shapes,
//! attributes and discriminants are read the way the compiler reads them.
//! **Every syntactic variant is recorded regardless of `cfg`**, with the
//! `cfg` expression kept verbatim: an audit run on one platform or feature
//! set must not miss a variant that only exists on another.

#![allow(dead_code)] // each test binary uses a different part of this module.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use syn::{visit::Visit, Attribute, Fields, Item, ItemEnum, Visibility};

/// One file's worth of audit scope.
pub struct ManifestEntry {
    /// Crate that owns the file.
    pub krate: &'static str,
    /// Path relative to the workspace root.
    pub file: &'static str,
    /// Enums that `OpsError`/`OpsWarning` wrap: every variant must map.
    pub wrapped: &'static [&'static str],
    /// Public `*Error`/`*Warning` enums in the same file that are not
    /// wrapped, with the reason. They are inventoried but not mapped.
    pub not_wrapped: &'static [(&'static str, &'static str)],
}

/// The audit scope. Extend it whenever `OpsError`/`OpsWarning` grows an arm.
pub const MANIFEST: &[ManifestEntry] = &[
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/error.rs",
        wrapped: &["HwpxError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/fill.rs",
        wrapped: &["FillError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/cell_edit.rs",
        wrapped: &["CellEditError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/read.rs",
        wrapped: &["ReadError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/structural.rs",
        wrapped: &["StructuralEditError"],
        not_wrapped: &[(
            "StructuralWarning",
            "surfaced by insert_para/delete_para, which W1b phase 2 adds; \
             OpsWarning gains the arm with the operation",
        )],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/grid_addr.rs",
        wrapped: &["GridAddrError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/section_workflow.rs",
        wrapped: &["SectionWorkflowError"],
        not_wrapped: &[(
            "SectionWorkflowWarning",
            "surfaced by to_json/patch, which W1b phase 2 adds; \
             OpsWarning gains the arm with the operation",
        )],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/stamp/stamper.rs",
        wrapped: &["StamperError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/stamp/apply.rs",
        wrapped: &["StampError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/stamp/apply_cells.rs",
        wrapped: &["CellStampError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/stamp/request.rs",
        wrapped: &["StampMapError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/encoder/mod.rs",
        wrapped: &["EncodeWarning"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-hwpx",
        file: "crates/hwpforge-smithy-hwpx/src/decoder/mod.rs",
        wrapped: &["DecodeWarning"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-core",
        file: "crates/hwpforge-core/src/error.rs",
        wrapped: &["CoreError"],
        not_wrapped: &[(
            "ValidationError",
            "reached only through CoreError::Validation, which classifies the \
             whole family as VALIDATION_FAILED",
        )],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-md",
        file: "crates/hwpforge-smithy-md/src/error.rs",
        wrapped: &["MdError"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-md",
        file: "crates/hwpforge-smithy-md/src/encoder/mod.rs",
        wrapped: &["MdWarning"],
        not_wrapped: &[],
    },
    ManifestEntry {
        krate: "hwpforge-smithy-md",
        file: "crates/hwpforge-smithy-md/src/assets/mod.rs",
        wrapped: &["AssetOutcome"],
        not_wrapped: &[],
    },
];

/// One variant as the audit records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantRecord {
    /// Variant identifier.
    pub name: String,
    /// `unit`, `tuple` or `struct`.
    pub shape: String,
    /// `cfg` attributes, verbatim and unevaluated.
    pub cfg: Vec<String>,
    /// Explicit discriminant, if any.
    pub discriminant: Option<String>,
}

/// One enum as the audit records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumRecord {
    /// Owning crate.
    pub krate: String,
    /// Path relative to the workspace root.
    pub file: String,
    /// Enum identifier.
    pub name: String,
    /// Visibility as written.
    pub visibility: String,
    /// Whether the enum carries `#[non_exhaustive]`.
    pub non_exhaustive: bool,
    /// Whether `OpsError`/`OpsWarning` wraps it (and so must map it).
    pub wrapped: bool,
    /// Why it is not wrapped, when it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_wrapped_reason: Option<String>,
    /// Variants in declaration order.
    pub variants: Vec<VariantRecord>,
}

/// The whole audit result — the shape of the tracked JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// Every enum the manifest names, in manifest order.
    pub enums: Vec<EnumRecord>,
    /// Public `*Error`/`*Warning` enums found in manifest files that the
    /// manifest does not name at all. Must stay empty.
    pub unlisted_public_error_or_warning_enums: Vec<String>,
}

/// The workspace root, derived from this crate's manifest directory.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

/// Parses every manifest file and returns the audit result.
///
/// # Panics
///
/// When a manifest file cannot be read or parsed, or when it does not
/// contain an enum the manifest names — both mean the manifest is stale.
#[must_use]
pub fn collect() -> Inventory {
    let root = workspace_root();
    let mut enums = Vec::new();
    let mut unlisted = Vec::new();

    for entry in MANIFEST {
        let path = root.join(entry.file);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("manifest file {}: {e}", entry.file));
        let ast = syn::parse_file(&source)
            .unwrap_or_else(|e| panic!("manifest file {} does not parse: {e}", entry.file));

        let named: Vec<&str> = entry
            .wrapped
            .iter()
            .copied()
            .chain(entry.not_wrapped.iter().map(|(name, _)| *name))
            .collect();

        let mut found = Vec::new();
        walk_items(&ast.items, &named, entry, &mut found);
        for want in &named {
            assert!(
                found.iter().any(|record: &EnumRecord| record.name == *want),
                "manifest names {want} but {} does not define it",
                entry.file
            );
        }
        enums.extend(found);

        let mut sweep = Sweep { named: &named, unlisted: Vec::new() };
        sweep.visit_file(&ast);
        unlisted.extend(sweep.unlisted.into_iter().map(|name| format!("{}::{name}", entry.file)));
    }

    Inventory { enums, unlisted_public_error_or_warning_enums: unlisted }
}

fn walk_items(items: &[Item], named: &[&str], entry: &ManifestEntry, out: &mut Vec<EnumRecord>) {
    for item in items {
        match item {
            Item::Enum(item_enum) if named.contains(&item_enum.ident.to_string().as_str()) => {
                out.push(record_enum(item_enum, entry));
            }
            // Inline `mod foo { … }` blocks are part of the same file.
            Item::Mod(item_mod) => {
                if let Some((_, inner)) = &item_mod.content {
                    walk_items(inner, named, entry, out);
                }
            }
            _ => {}
        }
    }
}

fn record_enum(item: &ItemEnum, entry: &ManifestEntry) -> EnumRecord {
    let name = item.ident.to_string();
    let wrapped = entry.wrapped.contains(&name.as_str());
    let not_wrapped_reason = entry
        .not_wrapped
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, reason)| (*reason).to_owned());

    EnumRecord {
        krate: entry.krate.to_owned(),
        file: entry.file.to_owned(),
        name,
        visibility: visibility_of(&item.vis),
        non_exhaustive: item.attrs.iter().any(|a| a.path().is_ident("non_exhaustive")),
        wrapped,
        not_wrapped_reason,
        variants: item
            .variants
            .iter()
            .map(|variant| VariantRecord {
                name: variant.ident.to_string(),
                shape: match &variant.fields {
                    Fields::Unit => "unit",
                    Fields::Unnamed(_) => "tuple",
                    Fields::Named(_) => "struct",
                }
                .to_owned(),
                cfg: variant
                    .attrs
                    .iter()
                    .filter(|a| a.path().is_ident("cfg"))
                    .map(attribute_text)
                    .collect(),
                discriminant: variant
                    .discriminant
                    .as_ref()
                    .map(|(_, expr)| quote::quote!(#expr).to_string()),
            })
            .collect(),
    }
}

fn visibility_of(vis: &Visibility) -> String {
    match vis {
        Visibility::Public(_) => "pub".to_owned(),
        Visibility::Restricted(restricted) => quote::quote!(#restricted).to_string(),
        Visibility::Inherited => "private".to_owned(),
    }
}

fn attribute_text(attr: &Attribute) -> String {
    quote::quote!(#attr).to_string()
}

/// Second pass: public `*Error`/`*Warning` enums the manifest never names.
struct Sweep<'a> {
    named: &'a [&'a str],
    unlisted: Vec<String>,
}

impl<'ast> Visit<'ast> for Sweep<'_> {
    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        let name = item.ident.to_string();
        let is_public = matches!(item.vis, Visibility::Public(_));
        let is_diagnostic = name.ends_with("Error") || name.ends_with("Warning");
        if is_public && is_diagnostic && !self.named.contains(&name.as_str()) {
            self.unlisted.push(name);
        }
        syn::visit::visit_item_enum(self, item);
    }
}
