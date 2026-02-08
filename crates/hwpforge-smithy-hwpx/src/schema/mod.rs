//! XML schema types (DTOs) for HWPX format.
//!
//! These types map directly to HWPX XML elements and exist only as an
//! internal deserialization boundary. They are **not** re-exported from
//! the crate's public API.
//!
//! The `Hx` prefix distinguishes these types from their Core
//! counterparts (e.g. `HxParagraph` vs `Paragraph`).

pub(crate) mod header;
pub(crate) mod section;
