//! Read-only operations: they answer a question about a document and return
//! one `dict`.
//!
//! Every function here follows the same three steps — borrow the input bytes
//! without copying, run the operation with the GIL released, and pythonize the
//! operation's own `*Meta`.

use hwpforge::ops;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;

use crate::errors::OrPy;
use crate::results::meta;

/// Counts the structure of a document.
///
/// Args:
///     data: The HWPX package.
///     styles: Whether to include the font, character and paragraph summaries.
///
/// Returns:
///     The inspection report, with a `styles` key only when `styles` is true.
#[pyfunction]
#[pyo3(signature = (data, /, *, styles = false))]
pub(crate) fn inspect<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    styles: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let options = ops::InspectOptions::default().with_styles(styles);
    let output = py.detach(|| ops::inspect(bytes, &options)).or_py(py)?;
    meta(py, &output.meta())
}

/// Lists the headings of a document in reading order.
///
/// Args:
///     data: The HWPX package.
///
/// Returns:
///     The outline and the warnings raised while decoding.
#[pyfunction]
#[pyo3(signature = (data, /))]
pub(crate) fn outline<'py>(py: Python<'py>, data: PyBackedBytes) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let output = py.detach(|| ops::outline(bytes)).or_py(py)?;
    meta(py, &output.meta())
}

/// Lists the click-here fields of a document.
///
/// Args:
///     data: The HWPX package.
///
/// Returns:
///     ``{"fields", "warnings"}``: the fields in document order and the
///     decoder warnings raised while reading the input (empty list when
///     the read was clean).
#[pyfunction]
#[pyo3(signature = (data, /))]
pub(crate) fn fields<'py>(py: Python<'py>, data: PyBackedBytes) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let output = py.detach(|| ops::fields(bytes)).or_py(py)?;
    meta(py, &output.meta())
}

/// Checks a document against the Core invariants.
///
/// A document that fails validation is **not** an error: the report says
/// `ok: false` and lists what failed. Only bytes that cannot be decoded at all
/// raise `HwpForgeError(DECODE_FAILED)`.
///
/// Args:
///     data: The HWPX package.
///
/// Returns:
///     The validation report.
#[pyfunction]
#[pyo3(signature = (data, /))]
pub(crate) fn validate<'py>(py: Python<'py>, data: PyBackedBytes) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let output = py.detach(|| ops::validate(bytes)).or_py(py)?;
    meta(py, &output.meta())
}

/// Reads one addressable part of a document.
///
/// Exactly one of `section`, `table` and `field` must be given; `paras`
/// narrows a `section` read and is not a target of its own. The operation
/// enforces both rules and reports `READ_TARGET_REQUIRED` or
/// `READ_PARAS_WITHOUT_SECTION`.
///
/// Args:
///     data: The HWPX package.
///     section: Index of the section to read paragraphs from.
///     paras: Inclusive paragraph range, `"A..B"` or a single `"N"`.
///     table: Ordinal of the table to read.
///     field: Name of the click-here field to read.
///
/// Returns:
///     A report whose `paragraphs`, `table` and `fields` keys are always
///     present, with `None` for the targets that were not requested.
#[pyfunction]
#[pyo3(signature = (data, /, *, section = None, paras = None, table = None, field = None))]
pub(crate) fn read<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    section: Option<usize>,
    paras: Option<String>,
    table: Option<usize>,
    field: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let mut options = ops::ReadOptions::default();
    if let Some(section) = section {
        options = options.with_section(section);
    }
    if let Some(paras) = paras {
        options = options.with_paras(paras);
    }
    if let Some(table) = table {
        options = options.with_table(table);
    }
    if let Some(field) = field {
        options = options.with_field(field);
    }
    let output = py.detach(|| ops::read(bytes, &options)).or_py(py)?;
    meta(py, &output.meta())
}

/// Compares two documents on both the Core-structure and ZIP-entry channels.
///
/// Args:
///     base: The HWPX package to compare from.
///     revised: The HWPX package to compare to.
///
/// Returns:
///     The two-channel diff report, plus the decoder warnings of both inputs.
#[pyfunction]
#[pyo3(signature = (base, /, *, revised))]
pub(crate) fn diff<'py>(
    py: Python<'py>,
    base: PyBackedBytes,
    revised: PyBackedBytes,
) -> PyResult<Bound<'py, PyAny>> {
    let (base, revised): (&[u8], &[u8]) = (&base, &revised);
    let output = py.detach(|| ops::diff(base, revised)).or_py(py)?;
    meta(py, &output.meta())
}

/// Enumerates the stamp candidates a document offers.
///
/// Discovery only: nothing is mutated. Copy `source_sha256` verbatim into the
/// approval map so that `stamp` can reject a map authored against a document
/// that has since changed.
///
/// Args:
///     data: The HWPX package.
///
/// Returns:
///     The plan, flattened into the report beside `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /))]
pub(crate) fn stamp_plan<'py>(py: Python<'py>, data: PyBackedBytes) -> PyResult<Bound<'py, PyAny>> {
    let bytes: &[u8] = &data;
    let output = py.detach(|| ops::stamp_plan(bytes)).or_py(py)?;
    meta(py, &output.meta())
}
