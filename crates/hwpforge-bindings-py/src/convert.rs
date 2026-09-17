//! The cross-format conversions, which live in `hwpforge_convert::ops` rather
//! than the umbrella: reading the legacy HWP5 binary, and rendering to PDF.

use std::path::PathBuf;

use hwpforge_convert::ops;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;

use crate::args;
use crate::errors::OrPy;
use crate::results::bytes_and_meta;

/// Converts a legacy HWP5 binary to HWPX.
///
/// Args:
///     data: The HWP5 (OLE2/CFB) document.
///     carry_layout_cache: Whether to carry the line-layout cache over. It
///         keeps the page breaks of the source, at the cost of caches that
///         describe the source's own layout.
///
/// Returns:
///     The HWPX package and its `warnings`.
#[pyfunction]
#[pyo3(signature = (data, /, *, carry_layout_cache = false))]
pub(crate) fn convert_hwp5<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    carry_layout_cache: bool,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::ConvertHwp5Options::default().with_carry_layout_cache(carry_layout_cache);
    let output = py.detach(|| ops::convert_hwp5(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}

/// Renders a document to PDF.
///
/// The container is detected by content, not by file name, so both an HWP5
/// binary and an HWPX package are accepted.
///
/// Args:
///     data: The HWP5 or HWPX document.
///     font_dirs: Directories to search for font files. Each entry may be a
///         string or any `os.PathLike`.
///     discovery: Where the renderer may look beyond `font_dirs` —
///         `"explicit"` (nowhere, the deterministic default), `"hancom"` (the
///         bundled Hancom Office fonts) or `"platform"` (those plus the
///         system font directories). Any other spelling raises
///         `HwpForgeError(INVALID_DISCOVERY)`.
///     degraded: Whether a font or image the renderer cannot honour degrades
///         the page instead of failing the render.
///     partial_cache_reject: Whether a paragraph without a layout cache
///         rejects the whole render instead of being skipped with a warning.
///
/// Returns:
///     The PDF bytes, the page count and `warnings`.
#[pyfunction]
#[pyo3(signature = (
    data, /, *,
    font_dirs = Vec::new(),
    discovery = "explicit",
    degraded = false,
    partial_cache_reject = false,
))]
pub(crate) fn to_pdf<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    font_dirs: Vec<PathBuf>,
    discovery: &str,
    degraded: bool,
    partial_cache_reject: bool,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
    let bytes: &[u8] = &data;
    let options = ops::ToPdfOptions::default()
        .with_font_dirs(font_dirs)
        .with_discovery(args::discovery(discovery).or_py(py)?)
        .with_degraded(degraded)
        .with_partial_cache_reject(partial_cache_reject);
    let output = py.detach(|| ops::to_pdf(bytes, &options)).or_py(py)?;
    bytes_and_meta(py, &output.bytes, &output.meta())
}
