//! The only file I/O in the operation layer.
//!
//! Every function in [`crate::ops`] works on bytes the caller already holds.
//! Asset (image) resolution is the one step that must reach the filesystem,
//! and it happens here so that a frontend without a filesystem — the Python
//! bindings fed from memory, an object store, an FFI peer — can replace this
//! one function and keep the rest of the pipeline untouched.
//!
//! The pipeline is three steps, and only the middle one is impure:
//!
//! 1. `collect_asset_plan` (pure) — what the document references.
//! 2. [`resolve_files_from_dir`] (**this module**) — read the files a plan
//!    asks for, with `base_dir` containment enforced by canonicalisation.
//! 3. `finish_assets` (pure) — embed the bytes and report the outcomes.

/// Reads the `file:` entries of an asset plan, confined to `base_dir`.
///
/// Re-exported from `hwpforge_smithy_md::assets::fs` so that callers reach
/// the resolver through the operation layer they already use. `data:` and
/// remote entries are left untouched — this step never opens a network
/// connection.
pub use hwpforge_smithy_md::assets::fs::resolve_files_from_dir;
