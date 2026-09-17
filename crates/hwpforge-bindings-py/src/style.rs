//! The two document-independent queries: the style presets that ship with the
//! library, and the JSON schemas of the exchange format.

use hwpforge::ops;
use pyo3::prelude::*;

use crate::errors::OrPy;
use crate::results::meta;

/// Lists the style presets that ship with the library.
///
/// Returns:
///     The presets under `presets`, each with its `name` and `description`.
#[pyfunction]
#[pyo3(signature = ())]
pub(crate) fn templates(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    // The only operation that is not detached: it reads a static list, so
    // releasing and reacquiring the GIL would cost more than the call itself.
    meta(py, &ops::templates().meta())
}

/// Returns the JSON schema of one exchange payload.
///
/// The schema is the patched one the command line publishes, with the
/// synthesised cell-address property, so a generated client agrees with what
/// `to_json` actually writes.
///
/// Args:
///     kind: `"document"`, `"exported-document"` or `"exported-section"`. Any
///         other spelling raises `HwpForgeError(INVALID_INPUT)` listing the
///         three.
///
/// Returns:
///     The schema itself, as a dictionary.
#[pyfunction]
#[pyo3(signature = (*, kind = "document"))]
pub(crate) fn schema<'py>(py: Python<'py>, kind: &str) -> PyResult<Bound<'py, PyAny>> {
    let options = ops::SchemaOptions::default().with_kind_name(kind).or_py(py)?;
    let output = py.detach(|| ops::schema(&options)).or_py(py)?;
    meta(py, &output.schema)
}
