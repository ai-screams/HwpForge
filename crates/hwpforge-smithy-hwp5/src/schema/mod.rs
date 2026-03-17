//! HWP5 binary record schema definitions.
//!
//! Submodules define the typed record structures that map onto the
//! HWP5 tag-length-value binary format:
//! - `record` — [`RecordHeader`] and [`TagId`] primitives
//! - `header` — `FileHeader` and `DocInfo` record types
//! - `section` — `BodyText` paragraph and run record types

pub mod border_fill;
pub mod header;
pub mod record;
pub mod section;
