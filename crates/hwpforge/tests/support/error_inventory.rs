//! Audit helper: parses the upstream error/warning enums that `ops` wraps,
//! and collects the error-contract generation sites ([`collect_sites`]).
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

use std::collections::BTreeMap;
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
        krate: "hwpforge-foundation",
        file: "crates/hwpforge-foundation/src/error.rs",
        wrapped: &["FoundationError"],
        not_wrapped: &[],
    },
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
        file: "crates/hwpforge-smithy-hwpx/src/patch.rs",
        wrapped: &["PatchError"],
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
        wrapped: &["StructuralEditError", "StructuralWarning"],
        not_wrapped: &[],
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
        wrapped: &["SectionWorkflowError", "SectionWorkflowWarning"],
        not_wrapped: &[],
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
        file: "crates/hwpforge-core/src/table/grid.rs",
        wrapped: &[],
        not_wrapped: &[(
            "GridError",
            "table-grid projection failure; never returned by an API the ops layer calls \
             directly — it reaches ops only wrapped as `CoreError`, `GridAddrError` or \
             `ReadError::TableUnaddressable`",
        )],
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
    /// Public `*Error`/`*Warning` enums found anywhere in the audited
    /// crates' module trees that the manifest does not name. Must stay empty.
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

    // A diagnostic must be recorded exactly once: two manifest entries naming
    // the same (crate, enum) would double it in the tracked inventory and let
    // one entry call it wrapped while the other calls it not.
    let mut seen = std::collections::BTreeSet::new();
    for entry in MANIFEST {
        for name in entry.wrapped.iter().copied().chain(entry.not_wrapped.iter().map(|(n, _)| *n)) {
            assert!(
                seen.insert((entry.krate, name)),
                "manifest names {}::{name} twice",
                entry.krate
            );
        }
    }

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
    }

    // Second pass, crate-wide: every source file reachable from each
    // audited crate's `lib.rs` through out-of-line `mod` declarations is
    // swept for public `*Error`/`*Warning` enums the manifest never names.
    // Limiting the sweep to manifest files would let a new diagnostic enum
    // in an unlisted module (or a re-export from one) go unnoticed.
    //
    // The inventory identifies a diagnostic by `(crate, enum name)` and the
    // ops payload derivation keeps only the last path segment, so two public
    // diagnostic enums with the same name in different modules of one crate
    // would be conflated. That is legal Rust; here it is flagged so the
    // manifest (and the ops arm) name the module explicitly when it happens.
    let mut public_diagnostics: std::collections::BTreeMap<(&str, String), Vec<String>> =
        std::collections::BTreeMap::new();
    let mut unlisted = Vec::new();
    for krate in audited_crates() {
        let named: Vec<&str> = MANIFEST
            .iter()
            .filter(|entry| entry.krate == krate)
            .flat_map(|entry| {
                entry.wrapped.iter().copied().chain(entry.not_wrapped.iter().map(|(n, _)| *n))
            })
            .collect();
        let lib = root.join("crates").join(krate).join("src/lib.rs");
        for file in module_tree(&lib) {
            let source = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let ast = syn::parse_file(&source)
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", file.display()));
            let mut sweep = Sweep { named: &named, unlisted: Vec::new() };
            sweep.visit_file(&ast);
            let rel = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
            let mut all_public = PublicDiagnostics::default();
            all_public.visit_file(&ast);
            for name in all_public.names {
                public_diagnostics.entry((krate, name)).or_default().push(rel.clone());
            }
            unlisted.extend(sweep.unlisted.into_iter().map(|name| format!("{rel}::{name}")));
        }
    }
    let collisions: Vec<String> = public_diagnostics
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|((krate, name), files)| format!("{krate}::{name} in {files:?}"))
        .collect();
    assert!(
        collisions.is_empty(),
        "same-named public diagnostic enums in one crate (the inventory cannot tell them apart): {collisions:?}"
    );

    Inventory { enums, unlisted_public_error_or_warning_enums: unlisted }
}

/// The crates the manifest covers, each swept from its `src/lib.rs`.
fn audited_crates() -> Vec<&'static str> {
    let mut crates: Vec<&'static str> = MANIFEST.iter().map(|entry| entry.krate).collect();
    crates.sort_unstable();
    crates.dedup();
    crates
}

/// Every source file of a crate's module tree, starting at `lib.rs` and
/// following out-of-line `mod name;` declarations (`name.rs`,
/// `name/mod.rs`, or a `#[path = "…"]` override). Inline modules are part
/// of their file and need no resolution. An unresolvable declaration is a
/// panic, never a silent skip.
///
/// # Panics
///
/// When a file cannot be read or parsed, or a `mod name;` has no file.
#[must_use]
pub fn module_tree(lib: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_module_file(lib, &mut files, false);
    files
}

/// [`module_tree`] without the out-of-line modules declared under
/// `#[cfg(test)]` — the production source the generation-site audit scans.
#[must_use]
pub fn production_module_tree(lib: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_module_file(lib, &mut files, true);
    files
}

fn visit_module_file(file: &Path, files: &mut Vec<PathBuf>, skip_test: bool) {
    let source =
        std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let ast = syn::parse_file(&source)
        .unwrap_or_else(|e| panic!("{} does not parse: {e}", file.display()));
    files.push(file.to_path_buf());

    let dir = file.parent().expect("module file has a directory");
    let is_root = matches!(file.file_name().and_then(|n| n.to_str()), Some("lib.rs" | "mod.rs"));
    let stem = file.file_stem().and_then(|s| s.to_str()).expect("module file stem");
    // `foo.rs` declaring `mod bar;` resolves to `foo/bar.rs`; a root file
    // (`lib.rs` / `mod.rs`) resolves siblings in its own directory. An
    // explicit `#[path]` on a top-level declaration is relative to the
    // directory of the file that carries it, whatever kind of file it is.
    let child_dir = if is_root { dir.to_path_buf() } else { dir.join(stem) };

    collect_out_of_line_mods(&ast.items, &child_dir, dir, files, skip_test);
}

fn collect_out_of_line_mods(
    items: &[Item],
    child_dir: &Path,
    path_base: &Path,
    files: &mut Vec<PathBuf>,
    skip_test: bool,
) {
    for item in items {
        let Item::Mod(item_mod) = item else { continue };
        if skip_test && is_test_only(&item_mod.attrs) {
            continue;
        }
        match &item_mod.content {
            // Inline module: its items live in the same file, but it may
            // itself declare out-of-line children under `<dir>/<name>/`;
            // inside an inline module an explicit `#[path]` is relative to
            // that nested directory too.
            Some((_, inner)) => {
                let nested = child_dir.join(item_mod.ident.to_string());
                collect_out_of_line_mods(inner, &nested, &nested, files, skip_test);
            }
            None => {
                let name = item_mod.ident.to_string();
                let explicit = item_mod.attrs.iter().find_map(path_attribute);
                let candidates = match explicit {
                    Some(rel) => vec![path_base.join(rel)],
                    None => vec![
                        child_dir.join(format!("{name}.rs")),
                        child_dir.join(&name).join("mod.rs"),
                    ],
                };
                let Some(found) = candidates.iter().find(|c| c.is_file()) else {
                    panic!(
                        "`mod {name};` in {} has no file (tried {:?})",
                        child_dir.display(),
                        candidates
                    );
                };
                visit_module_file(found, files, skip_test);
            }
        }
    }
}

/// The payload type names that `OpsError` and `OpsWarning` wrap, read from
/// the ops module's own source so the manifest cannot silently fall behind a
/// new arm. Only the last path segment is kept (`serde_json::Error` →
/// `Error`); every tuple-variant payload is returned, whatever its name.
///
/// Known limits, on purpose: this is syntactic. Diagnostics produced by
/// `include!` or by a macro, and enums re-exported from another crate under
/// a different name, are not discovered — the manifest still has to list
/// them by hand.
#[must_use]
pub fn wrapped_payloads_of_ops() -> Vec<String> {
    let file = workspace_root().join("crates/hwpforge/src/ops/mod.rs");
    let source =
        std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let ast = syn::parse_file(&source)
        .unwrap_or_else(|e| panic!("{} does not parse: {e}", file.display()));
    let mut out = Vec::new();
    for item in &ast.items {
        let Item::Enum(item_enum) = item else { continue };
        let ident = item_enum.ident.to_string();
        if ident != "OpsError" && ident != "OpsWarning" {
            continue;
        }
        for variant in &item_enum.variants {
            let Fields::Unnamed(unnamed) = &variant.fields else { continue };
            for field in &unnamed.unnamed {
                let syn::Type::Path(type_path) = &field.ty else { continue };
                let Some(last) = type_path.path.segments.last() else { continue };
                out.push(last.ident.to_string());
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The value of a `#[path = "…"]` attribute, if the item carries one.
fn path_attribute(attr: &Attribute) -> Option<String> {
    if !attr.path().is_ident("path") {
        return None;
    }
    let syn::Meta::NameValue(nv) = &attr.meta else { return None };
    let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value else {
        return None;
    };
    Some(s.value())
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

/// Every public `*Error`/`*Warning` enum name in one file (for the
/// same-name collision check).
#[derive(Default)]
struct PublicDiagnostics {
    names: Vec<String>,
}

impl<'ast> Visit<'ast> for PublicDiagnostics {
    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        let name = item.ident.to_string();
        if matches!(item.vis, Visibility::Public(_))
            && (name.ends_with("Error") || name.ends_with("Warning"))
        {
            self.names.push(name);
        }
        syn::visit::visit_item_enum(self, item);
    }
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

// ── generation sites (error contract 0.18, `error_contract_018.json`) ──

/// Crates whose production module trees the generation-site audit scans,
/// each from its `src/lib.rs`. The whole tree is scanned, not only the files
/// the manifest names, so a site in a new file cannot hide.
pub const SITE_AUDIT_CRATES: &[&str] = &["hwpforge-smithy-hwpx", "hwpforge", "hwpforge-convert"];

/// One generation site: `(file, enclosing fn, stage or variant, ordinal)`.
///
/// `kind` is `"stage"` (a `CodecStage::<X>` literal, `name` = `X`) or
/// `"variant"` (an audited error constructor, `name` = `Enum::Variant`).
/// The ordinal counts the same `(kind, name)` within one fn, in source
/// order, from 1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SiteKey {
    /// Path relative to the workspace root.
    pub file: String,
    /// `fn`, `Type::fn` or `Type as Trait::fn`, prefixed by inline modules
    /// and enclosing named fns.
    pub function: String,
    /// `"stage"` or `"variant"`.
    pub kind: String,
    /// Stage name or `Enum::Variant`.
    pub name: String,
    /// Ordinal within the fn.
    pub occurrence: u32,
}

/// The production files scanned and the sites found in them.
pub struct SiteScan {
    /// Every scanned file, relative to the workspace root, with the number
    /// of named fns visited in it (a scan that visits nothing proves nothing).
    pub files: Vec<(String, usize)>,
    /// Every site, sorted.
    pub sites: Vec<SiteKey>,
}

/// Scans every production file of [`SITE_AUDIT_CRATES`].
///
/// `audited_variants` is the manifest's closed `Enum::Variant` list.
///
/// # Panics
///
/// When a file cannot be read or parsed, or [`sites_in_source`] reports a
/// problem (duplicate key, audited path inside a macro or outside a fn).
#[must_use]
pub fn collect_sites(audited_variants: &[String]) -> SiteScan {
    let root = workspace_root();
    let mut files = Vec::new();
    let mut sites = Vec::new();
    let mut problems = Vec::new();
    for krate in SITE_AUDIT_CRATES {
        let lib = root.join("crates").join(krate).join("src/lib.rs");
        for file in production_module_tree(&lib) {
            let rel = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
            let source = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            match sites_in_source(&rel, &source, audited_variants) {
                Ok((found, fns)) => {
                    sites.extend(found);
                    files.push((rel, fns));
                }
                Err(mut found) => problems.append(&mut found),
            }
        }
    }
    assert!(problems.is_empty(), "generation-site scan failed: {problems:#?}");
    sites.sort();
    SiteScan { files, sites }
}

/// The generation sites of one source file, and how many named fns the
/// scan visited.
///
/// # Errors
///
/// Every problem found: a key produced twice (two fn bodies with the same
/// name both producing it), an audited path inside a macro body (syn does
/// not parse macro tokens, so such a site would go unseen), or an audited
/// path outside any fn.
///
/// # Panics
///
/// When `source` does not parse or an audited entry is not `Enum::Variant`.
pub fn sites_in_source(
    file: &str,
    source: &str,
    audited_variants: &[String],
) -> Result<(Vec<SiteKey>, usize), Vec<String>> {
    let ast = syn::parse_file(source).unwrap_or_else(|e| panic!("{file} does not parse: {e}"));
    let audited: Vec<(String, String)> = audited_variants
        .iter()
        .map(|entry| {
            let (enum_name, variant) = entry
                .split_once("::")
                .unwrap_or_else(|| panic!("audited variant `{entry}` is not `Enum::Variant`"));
            (enum_name.to_owned(), variant.to_owned())
        })
        .collect();
    let mut visitor = SiteVisitor {
        file,
        audited: &audited,
        scope: Vec::new(),
        self_enum: None,
        fn_name: None,
        fns: 0,
        counters: BTreeMap::new(),
        sites: Vec::new(),
        problems: Vec::new(),
    };
    visitor.visit_file(&ast);
    let mut seen = std::collections::BTreeSet::new();
    for site in &visitor.sites {
        if !seen.insert(site.clone()) {
            visitor.problems.push(format!("duplicate generation-site key {site:?}"));
        }
    }
    if visitor.problems.is_empty() {
        Ok((visitor.sites, visitor.fns))
    } else {
        Err(visitor.problems)
    }
}

/// `#[test]`, or `#[cfg(test)]` exactly: code the audit leaves out.
fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string() == "test"))
    })
}

/// Token text without whitespace, so the key does not depend on layout.
fn compact(tokens: impl quote::ToTokens) -> String {
    tokens.to_token_stream().to_string().split_whitespace().collect()
}

struct SiteVisitor<'a> {
    file: &'a str,
    audited: &'a [(String, String)],
    /// Inline modules, then impl/trait prefix, then enclosing named fns.
    scope: Vec<String>,
    /// The impl's self type name (resolves `Self::V`).
    self_enum: Option<String>,
    /// The enclosing named fn's key, `None` outside any fn.
    fn_name: Option<String>,
    /// Named fns visited.
    fns: usize,
    counters: BTreeMap<(String, String), u32>,
    sites: Vec<SiteKey>,
    problems: Vec<String>,
}

impl SiteVisitor<'_> {
    /// `(kind, name)` when `path` is a stage literal or an audited variant.
    fn classify(&self, path: &syn::Path) -> Option<(String, String)> {
        let idents: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let [.., owner, last] = idents.as_slice() else { return None };
        self.classify_pair(owner, last)
    }

    fn classify_pair(&self, owner: &str, last: &str) -> Option<(String, String)> {
        // `Self` resolves first, for stages and variants alike.
        let owner = if owner == "Self" { self.self_enum.as_deref()? } else { owner };
        if owner == "CodecStage" {
            return Some(("stage".to_owned(), last.to_owned()));
        }
        self.audited
            .iter()
            .any(|(e, v)| e == owner && v == last)
            .then(|| ("variant".to_owned(), format!("{owner}::{last}")))
    }

    /// Whether a comparison operand is only an audited path, under any
    /// parentheses and `&` — the one rule for "compared, not constructed".
    /// Anything else (a call holding a stage, …) is visited as usual.
    fn is_compared_path(&self, operand: &syn::Expr) -> bool {
        match operand {
            syn::Expr::Paren(inner) => self.is_compared_path(&inner.expr),
            syn::Expr::Reference(inner) => self.is_compared_path(&inner.expr),
            syn::Expr::Path(path) => self.classify(&path.path).is_some(),
            _ => false,
        }
    }

    fn record(&mut self, (kind, name): (String, String)) {
        let Some(function) = self.fn_name.clone() else {
            self.problems.push(format!("{}: {kind} {name} outside any fn", self.file));
            return;
        };
        let counter = self.counters.entry((kind.clone(), name.clone())).or_insert(0);
        *counter += 1;
        self.sites.push(SiteKey {
            file: self.file.to_owned(),
            function,
            kind,
            name,
            occurrence: *counter,
        });
    }

    /// Visits a named fn body with a fresh ordinal counter.
    fn in_fn(&mut self, name: String, body: impl FnOnce(&mut Self)) {
        self.fns += 1;
        self.scope.push(name);
        let key = self.scope.join("::");
        let outer_name = self.fn_name.replace(key);
        let outer_counters = std::mem::take(&mut self.counters);
        body(self);
        self.counters = outer_counters;
        self.fn_name = outer_name;
        self.scope.pop();
    }

    /// Ident/punct tokens of a macro body, groups flattened.
    fn flat_tokens(cursor: syn::buffer::Cursor<'_>, out: &mut Vec<String>) {
        let mut cursor = cursor;
        while !cursor.eof() {
            if let Some((ident, next)) = cursor.ident() {
                out.push(ident.to_string());
                cursor = next;
            } else if let Some((punct, next)) = cursor.punct() {
                out.push(punct.as_char().to_string());
                cursor = next;
            } else if let Some((inside, _, _, next)) = cursor.any_group() {
                Self::flat_tokens(inside, out);
                cursor = next;
            } else {
                // A literal or a lifetime: never part of an audited path.
                cursor = cursor.token_tree().expect("not at eof").1;
            }
        }
    }
}

impl<'ast> Visit<'ast> for SiteVisitor<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.scope.push(item.ident.to_string());
        syn::visit::visit_item_mod(self, item);
        self.scope.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.in_fn(item.sig.ident.to_string(), |v| syn::visit::visit_item_fn(v, item));
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if is_test_only(&item.attrs) {
            return;
        }
        let self_ty = compact(&item.self_ty);
        let prefix = match &item.trait_ {
            Some((trait_path, _)) => format!("{self_ty} as {}", compact(trait_path)),
            None => self_ty,
        };
        let self_enum = match &*item.self_ty {
            syn::Type::Path(ty) => ty.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        };
        let outer_enum = std::mem::replace(&mut self.self_enum, self_enum);
        self.scope.push(prefix);
        syn::visit::visit_item_impl(self, item);
        self.scope.pop();
        self.self_enum = outer_enum;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.in_fn(item.sig.ident.to_string(), |v| syn::visit::visit_impl_item_fn(v, item));
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        match expr {
            // A constructor call counts once; only its arguments are visited.
            syn::Expr::Call(call) => {
                if let syn::Expr::Path(callee) = &*call.func {
                    if let Some(site) = self.classify(&callee.path) {
                        self.record(site);
                        for arg in &call.args {
                            self.visit_expr(arg);
                        }
                        return;
                    }
                }
            }
            // The struct path is visited as a `Path`, not an `Expr`, so the
            // default recursion below cannot count it again.
            syn::Expr::Struct(lit) => {
                if let Some(site) = self.classify(&lit.path) {
                    self.record(site);
                }
            }
            // A unit variant, a stage literal, or a variant used as a fn value.
            syn::Expr::Path(path) => {
                if let Some(site) = self.classify(&path.path) {
                    self.record(site);
                    return;
                }
            }
            // `stage == CodecStage::Decode` compares; it constructs nothing.
            syn::Expr::Binary(binary)
                if matches!(
                    binary.op,
                    syn::BinOp::Eq(_)
                        | syn::BinOp::Ne(_)
                        | syn::BinOp::Lt(_)
                        | syn::BinOp::Le(_)
                        | syn::BinOp::Gt(_)
                        | syn::BinOp::Ge(_)
                ) =>
            {
                for side in [&*binary.left, &*binary.right] {
                    if !self.is_compared_path(side) {
                        self.visit_expr(side);
                    }
                }
                return;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let buffer = syn::buffer::TokenBuffer::new2(mac.tokens.clone());
        let mut tokens = Vec::new();
        Self::flat_tokens(buffer.begin(), &mut tokens);
        let hit = tokens.windows(4).find_map(|w| {
            (w[1] == ":" && w[2] == ":").then(|| self.classify_pair(&w[0], &w[3])).flatten()
        });
        if let Some((kind, name)) = hit {
            self.problems.push(format!(
                "{}: {kind} {name} inside `{}!` in {:?} — syn cannot see it; move it out of the macro",
                self.file,
                compact(&mac.path),
                self.fn_name
            ));
        }
    }
}
