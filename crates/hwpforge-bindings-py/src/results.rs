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

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::diagnostics::{WarningContext, WarningInfo, WarningStage};

    // 이것을 실패시키는 것: context None 키 출력, Some 소실, 위치·횟수 소실,
    // count=1 출력, 정수가 아닌 count 전달 또는 stage 철자 변경.
    #[test]
    fn warning_context_reaches_python_dict_without_reaggregation() {
        Python::initialize();
        Python::attach(|py| {
            let mut warning = WarningInfo::new("TEST", "message");
            let mut expected = serde_json::json!({"code": "TEST", "message": "message"});
            assert!(meta(py, &warning).unwrap().eq(pythonize(py, &expected).unwrap()).unwrap());
            for (stage, wire) in [
                (WarningStage::Decode, "decode"),
                (WarningStage::Project, "project"),
                (WarningStage::Encode, "encode"),
            ] {
                for positioned in [false, true] {
                    let mut context = WarningContext::new("docx", stage.clone(), "color", "omit");
                    let mut expected_context = serde_json::json!({
                        "format": "docx", "stage": wire, "loss": "color", "action": "omit"
                    });
                    if positioned {
                        context.part = Some("word/document.xml".into());
                        context.location = Some("paragraph[2]/run[3]".into());
                        context.count = 20_000;
                        expected_context["part"] = "word/document.xml".into();
                        expected_context["location"] = "paragraph[2]/run[3]".into();
                        expected_context["count"] = 20_000.into();
                    }
                    warning.context = Some(context);
                    expected["context"] = expected_context;
                    let actual = meta(py, &warning).unwrap();
                    assert!(actual.eq(pythonize(py, &expected).unwrap()).unwrap());
                    if positioned {
                        assert!(actual
                            .get_item("context")
                            .unwrap()
                            .get_item("count")
                            .unwrap()
                            .is_instance_of::<pyo3::types::PyInt>());
                    }
                }
            }
        });
    }
}
