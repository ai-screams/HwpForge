//! Markdown codec for HwpForge.
//!
//! This crate provides a bidirectional bridge between Markdown and the
//! format-agnostic Core DOM:
//!
//! - Decode: Markdown + Template -> `Document<Draft>`
//! - Encode (lossy): `Document<Validated>` -> readable GFM
//! - Encode (lossless): `Document<Validated>` -> frontmatter + HTML-like markup
//!
//! # Architecture
//!
//! ```text
//! foundation (indices, units)
//!     |
//!     v
//! core (Document DOM)
//!     |
//!     v
//! blueprint (Template, StyleRegistry)
//!     |
//!     v
//! smithy-md (THIS CRATE)
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::all)]

pub mod assets;
mod decoder;
pub mod embed;
mod encoder;
mod eqn;
pub mod error;
pub mod frontmatter;
mod internal_styles;
mod mapper;

pub use assets::{
    collect_asset_plan, finish_assets, validate_assets, warnings_from, AssetIdentity, AssetOutcome,
    AssetPlanEntry, AssetReject, AssetSource, FinishedAssets, ProvidedAsset, RunLocator,
};
pub use decoder::{MdDecoder, MdDocument};
pub use embed::{load_referenced_images, EmbeddedImages, ImageEmbedSkipReason};
pub use encoder::{MdEncoder, MdOutput, MdWarning};
pub use error::{MdError, MdErrorCode, MdResult};
pub use frontmatter::Frontmatter;
