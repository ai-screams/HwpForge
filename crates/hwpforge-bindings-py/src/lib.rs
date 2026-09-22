//! `_hwpforge` — the private extension module behind the `hwpforge` Python
//! package.
//!
//! # What this crate is
//!
//! A translation layer and nothing else: 23 functions, one per operation of
//! `hwpforge::ops` and `hwpforge_convert::ops`, each of which converts Python
//! arguments to Rust values, calls the operation, and converts the result
//! back. No document logic lives here — if something about an operation looks
//! wrong, the answer is in the operation, not in this crate.
//!
//! The module also exports one constant, `MAX_FILE_SIZE` (W6b audit
//! follow-up) — the shared frontend input size limit `Document.open` checks
//! against, re-exported verbatim from `hwpforge::ops::fs::MAX_FILE_SIZE` so
//! it cannot drift from what the CLI and the MCP server enforce. It is not
//! one of the 23 operations above: no document logic runs, and nothing is
//! converted — Python just reads the integer.
//!
//! # The three shapes
//!
//! An operation that produces a document returns `(bytes, dict)`, one that
//! produces text returns `(str, dict)`, and a query returns a bare `dict`. The
//! dictionary is always the pythonized `*Meta` of that operation, so its keys
//! are that type's serde field names.
//!
//! # Errors
//!
//! An operation failure becomes `hwpforge.errors.HwpForgeError(code, message,
//! hint)`, a pure-Python class this crate imports (the `errors` module).
//! Argument conversion failures stay as the `TypeError`/`ValueError` PyO3
//! raises, and a panic stays a `pyo3_runtime.PanicException` — always a bug.
//!
//! # The GIL
//!
//! Each function extracts and validates its arguments while attached, releases
//! the GIL for the Rust work with `Python::detach`, and reattaches to build the
//! result. `templates` is the exception: it reads a static list, so detaching
//! would cost more than it saves.
//!
//! # Building
//!
//! `extension-module` is a feature of this crate that only maturin turns on, so
//! that `cargo check`, `cargo clippy` and `cargo test` still link. Without an
//! interpreter, `PYO3_NO_PYTHON=1` is enough to check the crate, because the
//! `abi3-py39` build needs no interpreter of its own.

#![deny(missing_docs)]

use pyo3::prelude::*;

mod args;
mod convert;
mod edit;
mod errors;
mod exchange;
mod markdown;
mod queries;
mod results;
mod style;

/// The extension module `hwpforge._hwpforge`.
///
/// Private by design: the supported surface is the `hwpforge` package that
/// wraps it, and these functions may change without notice.
#[pymodule]
mod _hwpforge {
    /// The shared frontend input size limit (100 MB) — see the crate docs.
    #[pymodule_export]
    const MAX_FILE_SIZE: u64 = hwpforge::ops::fs::MAX_FILE_SIZE;
    #[pymodule_export]
    use crate::convert::{convert_hwp5, to_pdf};
    #[pymodule_export]
    use crate::edit::{delete_para, fill, insert_para, restyle, set_cell, stamp};
    #[pymodule_export]
    use crate::exchange::{export_section, from_json, patch, to_json};
    #[pymodule_export]
    use crate::markdown::{convert_md, to_md};
    #[pymodule_export]
    use crate::queries::{diff, fields, inspect, outline, read, stamp_plan, validate};
    #[pymodule_export]
    use crate::style::{schema, templates};
}
