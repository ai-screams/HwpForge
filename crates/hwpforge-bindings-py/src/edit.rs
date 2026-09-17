//! The editing operations: filling fields, replacing cell text, adding and
//! removing paragraphs, stamping an approved map, and restyling.
//!
//! Two families live here. The **preserving** edits (`fill`, `insert_para`,
//! `delete_para`) apply a delta to the input package and carry everything else
//! over untouched. The **regenerating** edits (`set_cell`, `stamp`, `restyle`)
//! encode the document again, and refuse to return bytes when that encode
//! reported a semantic loss — `HwpForgeError(ENCODE_SEMANTIC_LOSS)` rather
//! than a document that quietly means something else.

use std::collections::BTreeMap;

use hwpforge::hwpx::CellSpec;
use hwpforge::ops;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;

use crate::args;
use crate::errors::OrPy;
use crate::results::bytes_and_meta;

/// Fills click-here fields by name.
///
/// Args:
///     data: The HWPX package.
///     values: Field name to replacement text. An empty mapping is rejected
///         with `NO_VALUES`, and an empty string is not a valid value.
///
/// Returns:
///     The filled HWPX package, the fields that were filled, and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, values))]
pub(crate) fn fill<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    values: BTreeMap<String, String>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let values: Vec<(String, String)> = values.into_iter().collect();
    let options = ops::FillOptions::default();
    let output = py.detach(|| ops::fill(bytes, &values, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Replaces the text of one table cell, or of a batch of them.
///
/// Describe a single cell with `table`, `text` and exactly one of `at`,
/// `right_of` and `below`, **or** pass a `specs` batch. The operation rejects
/// any mix with `INVALID_SET_CELL_ARGS`.
///
/// Args:
///     data: The HWPX package.
///     table: Ordinal of the table to edit.
///     at: Zero-based grid coordinate as `"row,col"`, for example `"1,0"`.
///     right_of: Label of the cell to the left of the target.
///     below: Label of the cell above the target.
///     text: The replacement text. The empty string clears the cell.
///     specs: A batch of `{"table": int, "text": str}` objects, each with one
///         target key: `{"at": {"row": int, "col": int}}`, `{"right_of": str}`
///         or `{"below": str}`. The coordinate is an object here, not the
///         `"row,col"` string the single-target form takes — this is the
///         shape the command line's `--map` file uses.
///
/// Returns:
///     The edited HWPX package, one result per cell, and `warnings`.
#[pyfunction]
#[pyo3(signature = (
    data, /, *,
    table = None,
    at = None,
    right_of = None,
    below = None,
    text = None,
    specs = None,
))]
#[allow(clippy::too_many_arguments)] // One parameter per CLI flag: the FFI table is the contract.
pub(crate) fn set_cell<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    table: Option<usize>,
    at: Option<String>,
    right_of: Option<String>,
    below: Option<String>,
    text: Option<String>,
    specs: Option<&Bound<'py, PyAny>>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let mut options = ops::SetCellOptions::default();
    if let Some(table) = table {
        options = options.with_table(table);
    }
    if let Some(at) = at {
        options = options.with_at(at);
    }
    if let Some(right_of) = right_of {
        options = options.with_right_of(right_of);
    }
    if let Some(below) = below {
        options = options.with_below(below);
    }
    if let Some(text) = text {
        options = options.with_text(text);
    }
    if let Some(specs) = specs {
        options = options.with_specs(args::json_argument::<Vec<CellSpec>>("specs", specs)?);
    }
    let output = py.detach(|| ops::set_cell(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Inserts paragraphs next to an anchor paragraph.
///
/// Args:
///     data: The HWPX package.
///     section: Index of the section to edit.
///     anchor: Index of the paragraph the new ones go next to.
///     text: The paragraphs to insert — one string for a single paragraph, or
///         a sequence of strings for one paragraph each. A `str` is not
///         exploded into its characters, and `bytes` is refused.
///     before: Whether to insert above the anchor instead of below it.
///
/// Returns:
///     The edited HWPX package, the insert and delete counts, and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, section, anchor, text, before = false))]
pub(crate) fn insert_para<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    section: usize,
    anchor: usize,
    text: &Bound<'py, PyAny>,
    before: bool,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::InsertParaOptions::default()
        .with_section(section)
        .with_anchor(anchor)
        .with_text(args::paragraph_texts("text", text)?)
        .with_before(before);
    let output = py.detach(|| ops::insert_para(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Deletes paragraphs by index.
///
/// Args:
///     data: The HWPX package.
///     section: Index of the section to edit.
///     indexes: Indexes of the paragraphs to delete, in the input document's
///         numbering.
///
/// Returns:
///     The edited HWPX package, the insert and delete counts, and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, section, indexes))]
pub(crate) fn delete_para<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    section: usize,
    indexes: Vec<usize>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::DeleteParaOptions::default().with_section(section).with_indexes(indexes);
    let output = py.detach(|| ops::delete_para(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Applies an approved stamp map, all or nothing.
///
/// Args:
///     data: The HWPX package.
///     request: The approval map — a list of specs (v1), or the
///         `{schema_version, source_sha256, text, cells}` envelope (v2) that
///         `stamp_plan` feeds. Anything else raises
///         `HwpForgeError(INVALID_STAMP_MAP)`.
///     manifest: Whether to return the output inventory. The library always
///         builds and validates it, so turning this off saves the copy, never
///         the work; the `manifest` key is then absent from the report.
///
/// Returns:
///     The stamped HWPX package, the manifest and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, request, manifest = true))]
pub(crate) fn stamp<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    request: &Bound<'py, PyAny>,
    manifest: bool,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let request = args::stamp_map(py, request)?;
    let options = ops::StampOptions::default().with_manifest(manifest);
    let output = py.detach(|| ops::stamp(bytes, &request, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Rebinds a document's fonts onto a style preset.
///
/// Args:
///     data: The HWPX package.
///     preset: Name of the preset, as `templates` lists it.
///
/// Returns:
///     The restyled HWPX package, the preset name and `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, preset))]
pub(crate) fn restyle<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    preset: String,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::RestyleOptions::default().with_preset(preset);
    let output = py.detach(|| ops::restyle(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}
