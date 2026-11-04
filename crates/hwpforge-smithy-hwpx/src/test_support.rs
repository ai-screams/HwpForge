//! Shared helpers that lib tests use, included once: one file compiled as two
//! modules of this crate is a duplicate module. Integration tests include them
//! on their own.

#[path = "../tests/support/xml.rs"]
pub(crate) mod xml;
