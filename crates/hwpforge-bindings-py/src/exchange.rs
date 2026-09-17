//! The JSON exchange operations: exporting a document or one section, and
//! rebuilding or patching a document from JSON.

use hwpforge::ops;
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::PyBytes;

use crate::errors::OrPy;
use crate::results::{bytes_and_meta, meta};

/// Exports a document as the editable JSON tree.
///
/// The tree carries the synthesised cell addresses (`addr`) that the grid
/// operations expect, exactly as the command line writes them.
///
/// Args:
///     data: The HWPX package.
///     styles: Whether to include the style definitions.
///
/// Returns:
///     The exported document under `document`, plus `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, styles = true))]
pub(crate) fn to_json<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    styles: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let options = ops::ToJsonOptions::default().with_styles(styles);
    let output = py.detach(|| ops::to_json(bytes, &options)).or_py(py)?;
    meta(py, &output.meta())
}

/// Exports one section as the JSON tree `patch` takes back.
///
/// Args:
///     data: The HWPX package.
///     section: Index of the section to export.
///     styles: Whether to include the style definitions.
///
/// Returns:
///     The exported section under `section`, plus `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, section, styles = true))]
pub(crate) fn export_section<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    section: usize,
    styles: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let options = ops::ExportSectionOptions::default().with_section(section).with_styles(styles);
    let output = py.detach(|| ops::export_section(bytes, &options)).or_py(py)?;
    meta(py, &output.meta())
}

/// Builds a document from the JSON tree `to_json` produces.
///
/// Args:
///     text: The exported document, as JSON.
///     base: The package whose preserved parts the result inherits. Without
///         it the document is generated from scratch.
///
/// Returns:
///     The HWPX package and its `warnings`.
#[pyfunction]
#[pyo3(signature = (text, /, *, base = None))]
pub(crate) fn from_json<'py>(
    py: Python<'py>,
    text: PyBackedStr,
    base: Option<PyBackedBytes>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let json: &str = &text;
    let mut options = ops::FromJsonOptions::default();
    if let Some(base) = &base {
        options = options.with_base(&base[..]);
    }
    let output = py.detach(|| ops::from_json(json, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Replaces one section of a document with an edited JSON tree.
///
/// This is the preserving path: everything the section does not describe is
/// carried over from the input package byte for byte.
///
/// Args:
///     data: The HWPX package.
///     section: Index of the section to replace.
///     patch: The edited section tree, as JSON.
///
/// Returns:
///     The patched HWPX package, plus the section index and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, section, patch))]
pub(crate) fn patch<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    section: usize,
    patch: PyBackedStr,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::PatchOptions::default().with_section(section).with_patch(&patch[..]);
    let output = py.detach(|| ops::patch(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}
