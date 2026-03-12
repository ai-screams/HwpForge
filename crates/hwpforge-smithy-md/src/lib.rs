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

mod decoder;
mod encoder;
mod eqn;
pub mod error;
pub mod frontmatter;
mod mapper;

pub use decoder::{MdDecoder, MdDocument};
pub use encoder::{MdEncoder, MdOutput};
pub use error::{MdError, MdErrorCode, MdResult};
pub use frontmatter::Frontmatter;
