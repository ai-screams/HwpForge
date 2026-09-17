//! The Markdown bridge: building a document from Markdown, and exporting one
//! back to Markdown.

use std::path::PathBuf;

use hwpforge::ops;
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::PyBytes;

use crate::errors::OrPy;
use crate::results::{bytes_and_meta, text_and_meta};

/// Builds a document from Markdown.
///
/// Images the Markdown references are read **here**, inside the operation
/// layer, so that the path containment rules (a relative path may not escape
/// `base_dir`, a missing or unreadable file is reported rather than guessed at)
/// have one implementation rather than one per frontend. Without `base_dir` no
/// file is read at all and every local image is reported as dropped.
///
/// Args:
///     text: The Markdown source, with optional YAML frontmatter.
///     preset: Name of the style preset to apply.
///     base_dir: Directory that relative image paths resolve against.
///
/// Returns:
///     The HWPX package, one outcome per referenced image under `assets`, and
///     `warnings`.
#[pyfunction]
#[pyo3(signature = (text, /, *, preset = "default", base_dir = None))]
pub(crate) fn convert_md<'py>(
    py: Python<'py>,
    text: PyBackedStr,
    preset: &str,
    base_dir: Option<PathBuf>,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let markdown: &str = &text;
    let options = ops::ConvertMdOptions::default().with_preset(preset);
    let base_dir = base_dir.as_deref();
    let output = py.detach(|| ops::convert_md(markdown, base_dir, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Exports a document as Markdown.
///
/// Args:
///     data: The HWPX package.
///     mode: `"styled"` for GFM with a style frontmatter, `"lossy"` for plain
///         GFM, `"lossless"` for the round-trip rendering. Any other spelling
///         raises `HwpForgeError(INVALID_INPUT)` listing the three.
///
/// Returns:
///     The Markdown text, and a report whose `images` maps each referenced
///     key to its `bytes`.
#[pyfunction]
#[pyo3(signature = (data, /, *, mode = "styled"))]
pub(crate) fn to_md<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    mode: &str,
) -> PyResult<(String, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::MdExportOptions::default().with_mode_name(mode).or_py(py)?;
    let output = py.detach(|| ops::to_md(bytes, &options)).or_py(py)?;
    let meta = output.meta();
    text_and_meta(py, output.markdown, &meta)
}
