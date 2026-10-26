//! Synthetic depth packages built from committed fixtures only.
#![allow(dead_code)]

#[path = "depth_package_builder.rs"]
mod builder;
pub use builder::*;

pub fn fixture_path(rel: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel)
}
