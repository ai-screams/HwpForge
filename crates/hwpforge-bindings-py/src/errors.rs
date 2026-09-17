//! The bridge from the Rust operation errors to the Python exception.
//!
//! Every failure of an operation becomes one Python class,
//! `hwpforge.errors.HwpForgeError`, carrying the stable code, the message and
//! the static hint. The class is **pure Python** (it lives in the package, not
//! here) because `abi3-py39` cannot define an exception type in Rust; it is
//! imported once per interpreter into a [`PyOnceLock`] and called like any
//! other callable.
//!
//! Nothing else is wrapped. Argument conversion failures stay as the
//! `TypeError`/`ValueError` PyO3 and pythonize raise, and a Rust panic stays a
//! `pyo3_runtime.PanicException` — both mean the caller passed something the
//! FFI never accepted, not that an operation failed.

use hwpforge::ops::OpsError;
use hwpforge_convert::ops::{ConvertOpsError, PdfCause};
use hwpforge_foundation::diagnostics::WarningInfo;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyType;

use crate::results::meta;

/// The four arguments `HwpForgeError(code, message, hint, cause)` takes.
struct ErrorParts {
    /// The stable `OpsCode` spelling, for example `DECODE_FAILED`.
    code: &'static str,
    /// The failure's own sentence.
    message: String,
    /// The static recovery suggestion, when the code has one.
    hint: Option<&'static str>,
    /// The renderer's own classification beside a `PDF_RENDER_FAILED`, which
    /// is the only failure that has one. Pythonized into the same
    /// `{stage, code, kind?, location?}` object the command line prints.
    cause: Option<PdfCause>,
}

/// Turns an operation failure into the Python exception.
///
/// Implemented for the two error types the operation layer has, so that a call
/// site reads `ops::fill(&data, &values, &opts).or_py(py)?` whichever crate the
/// operation came from.
pub(crate) trait OrPy<T> {
    /// Converts the error half into a `PyErr`, leaving the value untouched.
    fn or_py(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> OrPy<T> for Result<T, OpsError> {
    fn or_py(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|error| raise(py, ops_parts(&error)))
    }
}

impl<T> OrPy<T> for Result<T, ConvertOpsError> {
    fn or_py(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|error| raise(py, convert_parts(&error)))
    }
}

/// Builds `HwpForgeError(code, message, hint, cause)` and turns it into a
/// `PyErr`.
///
/// All four arguments are passed positionally on every call, so the pure-Python
/// class must take four. Importing the package's `errors` module is what makes
/// the exception available; if that import fails (a broken installation, or the
/// extension loaded without its pure-Python half) the `ImportError` is raised
/// as-is rather than being replaced by a generic exception, so the real cause
/// reaches the caller.
fn raise(py: Python<'_>, parts: ErrorParts) -> PyErr {
    static HWPFORGE_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();

    let class = match HWPFORGE_ERROR.import(py, "hwpforge.errors", "HwpForgeError") {
        Ok(class) => class,
        Err(error) => return error,
    };
    let cause = match parts.cause.as_ref().map(|cause| meta(py, cause)).transpose() {
        Ok(cause) => cause,
        Err(error) => return error,
    };
    match class.call1((parts.code, parts.message, parts.hint, cause)) {
        Ok(instance) => PyErr::from_value(instance),
        Err(error) => error,
    }
}

/// Classifies an umbrella operation failure.
///
/// `cause` is always absent: the second classification exists only for the PDF
/// renderer, which no umbrella operation reaches.
fn ops_parts(error: &OpsError) -> ErrorParts {
    ErrorParts {
        code: error.code().as_str(),
        message: ops_message(error),
        hint: error.hint(),
        cause: None,
    }
}

/// Classifies a conversion operation failure (`convert_hwp5`, `to_pdf`).
///
/// A render failure carries two classifications: `PDF_RENDER_FAILED` says which
/// operation refused, and [`ConvertOpsError::cause_info`] says what the renderer
/// met. The second one is the actionable half, so it crosses as the same
/// `{stage, code, kind?, location?}` object the command line prints under
/// `cause`, rather than being flattened to a code or folded into the message.
fn convert_parts(error: &ConvertOpsError) -> ErrorParts {
    ErrorParts {
        code: error.code().as_str(),
        message: error.to_string(),
        hint: error.hint(),
        cause: error.cause_info(),
    }
}

/// The sentence the exception carries.
///
/// Normally the error's own `Display`. The one exception is the fail-closed
/// refusal of a regenerating edit: its `Display` says only *that* meaning was
/// lost, and the warnings that say *what* was lost live in the variant's two
/// lists. A caller who only sees the exception would otherwise be told (by the
/// hint) to check a warning list it never received, so the semantic-loss
/// warnings are appended — the same choice the MCP server makes for
/// `ENCODE_SEMANTIC_LOSS`, which reports the joined warnings as its message.
fn ops_message(error: &OpsError) -> String {
    match error {
        OpsError::EncodeSemanticLoss { warnings, .. } if !warnings.is_empty() => {
            format!("{error}: {}", join_warnings(warnings))
        }
        other => other.to_string(),
    }
}

/// Renders warnings as `CODE: message`, joined the way the MCP server joins
/// them.
fn join_warnings(warnings: &[WarningInfo]) -> String {
    warnings
        .iter()
        .map(|warning| format!("{}: {}", warning.code, warning.message))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::diagnostics::OpsCode;

    fn info(code: &str, message: &str) -> WarningInfo {
        WarningInfo::new(code, message)
    }

    #[test]
    fn a_plain_failure_keeps_its_own_sentence_code_and_hint() {
        let error = OpsError::PresetNotFound { name: "gov".into() };

        let parts = ops_parts(&error);

        assert_eq!(parts.code, "PRESET_NOT_FOUND");
        assert_eq!(parts.message, "preset not found: gov");
        assert!(parts.hint.is_some(), "this code has a CLI hint to reproduce");
    }

    #[test]
    fn a_code_without_a_hint_reports_none() {
        let error = OpsError::InvalidInput { reason: "two targets".into() };

        assert!(ops_parts(&error).hint.is_none());
    }

    #[test]
    fn fail_closed_appends_the_warnings_the_hint_tells_the_caller_to_read() {
        let error = OpsError::EncodeSemanticLoss {
            warnings: vec![
                info("NOTE_HEAD_SKIPPED", "footnote head skipped"),
                info("TITLE_MARK_SKIPPED", "title mark skipped"),
            ],
            others: vec![info("LAYOUT_CACHE_DROPPED", "line cache dropped")],
        };

        let parts = ops_parts(&error);

        assert_eq!(parts.code, OpsCode::EncodeSemanticLoss.as_str());
        assert!(parts.message.contains("NOTE_HEAD_SKIPPED: footnote head skipped"));
        assert!(parts.message.contains("TITLE_MARK_SKIPPED: title mark skipped"));
        assert!(
            !parts.message.contains("LAYOUT_CACHE_DROPPED"),
            "only the losses that caused the refusal belong in the sentence"
        );
    }

    #[test]
    fn fail_closed_without_warnings_falls_back_to_the_plain_sentence() {
        let error = OpsError::EncodeSemanticLoss { warnings: Vec::new(), others: Vec::new() };

        assert_eq!(ops_parts(&error).message, error.to_string());
    }

    #[test]
    fn a_conversion_failure_uses_the_convert_crate_classification() {
        let error = ConvertOpsError::UnrecognizedFormat;

        let parts = convert_parts(&error);

        assert_eq!(parts.code, error.code().as_str());
        assert_eq!(parts.message, error.to_string());
        assert!(parts.cause.is_none(), "only a render failure classifies twice");
    }

    // A failure that never reached the renderer has no second classification.
    // The populated shape needs a real render failure, which needs fonts this
    // machine may not have, so it is measured from Python instead of asserted
    // here: `{stage, code, kind?, location?}`, pythonized straight from
    // `PdfCause` so its serde attributes decide which keys appear.
    #[test]
    fn a_failure_that_never_reached_the_renderer_has_no_cause() {
        let error = hwpforge_convert::ops::to_pdf(
            b"neither an OLE2 nor a ZIP container",
            &hwpforge_convert::ops::ToPdfOptions::default(),
        )
        .expect_err("unrecognised bytes");

        assert!(convert_parts(&error).cause.is_none());
    }

    #[test]
    fn an_operation_failure_never_carries_a_cause() {
        assert!(ops_parts(&OpsError::NoFonts).cause.is_none());
    }
}
