//! Building the three return shapes.
//!
//! An operation returns `(bytes, dict)`, `(str, dict)` or a bare `dict`, and
//! the dictionary is always the pythonized `*Meta` of that operation — never an
//! object assembled here. That matters beyond tidiness: `MdExportOutput.images`
//! holds `Vec<u8>`, which pythonizes to `list[int]`, while its `meta()` converts
//! to `ByteBuf`, which pythonizes to `bytes`. Going through `meta()` is what
//! keeps the wire shape the type stub declares.

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pythonize::pythonize;
use serde::Serialize;

/// The pythonized metadata of an operation: the `dict` half of every return.
///
/// # Errors
///
/// Whatever pythonize raises when a payload cannot become a Python object
/// (a `TypeError` for an unsupported shape, a `ValueError` for a bad length).
pub(crate) fn meta<'py, M>(py: Python<'py>, meta: &M) -> PyResult<Bound<'py, PyAny>>
where
    M: Serialize + ?Sized,
{
    Ok(pythonize(py, meta)?)
}

/// The `(bytes, dict)` return of an operation that produces a document.
///
/// # Errors
///
/// See [`meta`].
pub(crate) fn bytes_and_meta<'py, M>(
    py: Python<'py>,
    bytes: &[u8],
    metadata: &M,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)>
where
    M: Serialize + ?Sized,
{
    Ok((PyBytes::new(py, bytes), meta(py, metadata)?))
}

/// The `(str, dict)` return of an operation that produces text.
///
/// # Errors
///
/// See [`meta`].
pub(crate) fn text_and_meta<'py, M>(
    py: Python<'py>,
    text: String,
    metadata: &M,
) -> PyResult<(String, Bound<'py, PyAny>)>
where
    M: Serialize + ?Sized,
{
    Ok((text, meta(py, metadata)?))
}
