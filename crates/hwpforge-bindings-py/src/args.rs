//! Turning Python arguments into the values an operation takes.
//!
//! Only two kinds of work happen here: parsing the enumerated (`Literal`)
//! options the operation layer leaves to its frontends, and handing a
//! JSON-shaped Python object to the parser that owns its dispatch. Everything
//! else is a plain PyO3 conversion in the function signature.
//!
//! **No semantic validation lives here.** Mutually exclusive targets
//! (`read`, `set_cell`), empty value maps and empty spec lists are rejected by
//! the operations themselves with their own stable codes, so re-checking them
//! here would fork the rules and the messages.

use hwpforge::hwpx::stamp::{parse_stamp_map, StampMap};
use hwpforge::ops::OpsError;
use hwpforge_convert::ops::ConvertOpsError;
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_smithy_pdf::font::FontDiscovery;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use pythonize::depythonize;
use serde::de::DeserializeOwned;

use crate::errors::OrPy;

/// Parses the `discovery` option of `to_pdf`.
///
/// The renderer's option is already the typed enum by the time
/// [`ToPdfOptions`](hwpforge_convert::ops::ToPdfOptions) sees it, so parsing
/// the wire spelling is the frontend's job — and
/// [`ConvertOpsError::Rejected`] exists so that the rejection still reports
/// the canonical `INVALID_DISCOVERY`, with the wording the CLI prints.
///
/// # Errors
///
/// [`ConvertOpsError::Rejected`] (`INVALID_DISCOVERY`) for any other spelling.
pub(crate) fn discovery(name: &str) -> Result<FontDiscovery, ConvertOpsError> {
    match name {
        "explicit" => Ok(FontDiscovery::ExplicitOnly),
        "hancom" => Ok(FontDiscovery::HancomBundle),
        "platform" => Ok(FontDiscovery::Platform),
        other => Err(ConvertOpsError::Rejected {
            code: OpsCode::InvalidDiscovery,
            reason: format!("unknown discovery mode '{other}' (expected explicit|hancom|platform)"),
        }),
    }
}

/// Reads the paragraph texts of `insert_para`.
///
/// One string means one paragraph, and a sequence means one paragraph per
/// element. Both are accepted because the difference is invisible in Python:
/// a `str` *is* a sequence of one-character strings, so a caller that passes
/// prose where a list belongs would otherwise get one paragraph per character.
/// PyO3 refuses that shape on its own (`Vec<T>` rejects `PyString` before it
/// tries the sequence protocol), but refusing is not the useful answer when
/// the intent is unambiguous: a single string is a single paragraph.
///
/// `bytes` is rejected rather than decoded. It is a sequence too, and
/// `b"ab"` iterates into integers, so accepting it would mean guessing an
/// encoding for something the caller never said was text.
///
/// # Errors
///
/// `TypeError` naming the argument when the value is `bytes`, is not
/// iterable, or holds an element that is not a string.
pub(crate) fn paragraph_texts(name: &str, value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if let Ok(single) = value.cast::<PyString>() {
        return Ok(vec![single.extract::<String>()?]);
    }
    if value.is_instance_of::<PyBytes>() {
        return Err(PyTypeError::new_err(format!(
            "{name}: expected a str or a sequence of str, got bytes"
        )));
    }
    let items = value.try_iter().map_err(|_| {
        PyTypeError::new_err(format!(
            "{name}: expected a str or a sequence of str, got {}",
            type_name(value)
        ))
    })?;
    items
        .enumerate()
        .map(|(index, item)| {
            let item = item?;
            item.cast::<PyString>()
                .map_err(|_| {
                    PyTypeError::new_err(format!(
                        "{name}[{index}]: expected str, got {}",
                        type_name(&item)
                    ))
                })?
                .extract()
        })
        .collect()
}

/// The Python type name of a value, for a message a caller can act on.
fn type_name(value: &Bound<'_, PyAny>) -> String {
    value.get_type().name().map_or_else(|_| "?".to_owned(), |name| name.to_string())
}

/// Reads a stamp request — the v1 spec list or the v2 envelope — from the
/// Python object a caller passed.
///
/// The object is depythonized to JSON and handed to
/// [`parse_stamp_map`], which is the single place that decides *which* shape
/// arrived (a list is v1, an object is v2) and validates it. Re-implementing
/// that dispatch against Python types would give two answers to one question,
/// and would lose the `deny_unknown_fields` checking the request DTOs carry.
///
/// # Errors
///
/// A `TypeError`/`ValueError` when the object is not JSON-shaped (pythonize's
/// own classification), and `HwpForgeError(INVALID_STAMP_MAP)` when it is
/// well-formed JSON but not a stamp map.
pub(crate) fn stamp_map(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<StampMap> {
    let value: serde_json::Value = json_argument("request", request)?;
    stamp_map_from_json(&value).or_py(py)
}

/// Reads a JSON-shaped Python argument into the Rust type that describes it.
///
/// The conversion failure is reported as the caller's mistake it is, naming
/// the argument. pythonize raises a `TypeError` or a `ValueError` for an
/// object it cannot read at all, and those are kept; a serde rejection of a
/// **well-formed** object (a missing or misspelled field) arrives as the bare
/// `Exception`, which tells a caller nothing it can act on, so it becomes a
/// `ValueError`.
///
/// # Errors
///
/// `TypeError` or `ValueError`, never `HwpForgeError`: nothing here is an
/// operation failure.
pub(crate) fn json_argument<T>(name: &str, object: &Bound<'_, PyAny>) -> PyResult<T>
where
    T: DeserializeOwned,
{
    depythonize(object).map_err(|error| {
        let py = object.py();
        let error = PyErr::from(error);
        let message = format!("{name}: {}", error.value(py));
        let kind = error.get_type(py);
        if kind.is(py.get_type::<PyException>()) {
            PyValueError::new_err(message)
        } else {
            PyErr::from_type(kind, message)
        }
    })
}

/// The Python-free half of [`stamp_map`], so the bridge can be tested without
/// an interpreter.
fn stamp_map_from_json(value: &serde_json::Value) -> Result<StampMap, OpsError> {
    let json = serde_json::to_string(value).map_err(OpsError::json_serialize)?;
    Ok(parse_stamp_map(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// These need an interpreter, which the test binary has because it links
    /// against one (the extension-module feature is off outside maturin).
    mod paragraphs {
        use super::*;

        fn texts(source: &str) -> PyResult<Vec<String>> {
            // The test binary embeds an interpreter rather than being loaded
            // by one, so it has to start before any Python API is touched.
            Python::initialize();
            Python::attach(|py| {
                let value = py.eval(&std::ffi::CString::new(source).unwrap(), None, None)?;
                paragraph_texts("text", &value)
            })
        }

        #[test]
        fn one_string_is_one_paragraph_not_one_per_character() {
            assert_eq!(texts("'검토용 문단'").unwrap(), vec!["검토용 문단".to_owned()]);
        }

        #[test]
        fn a_sequence_is_one_paragraph_per_element() {
            assert_eq!(texts("['가', '나']").unwrap(), vec!["가".to_owned(), "나".to_owned()]);
            assert_eq!(texts("('가', '나')").unwrap(), vec!["가".to_owned(), "나".to_owned()]);
            assert!(texts("[]").unwrap().is_empty());
        }

        #[test]
        fn bytes_is_refused_rather_than_decoded() {
            let error = texts("b'ab'").expect_err("bytes is not text");

            Python::initialize();
            Python::attach(|py| {
                assert!(error.is_instance_of::<PyTypeError>(py));
                assert!(error.to_string().contains("text"), "{error}");
                assert!(error.to_string().contains("bytes"), "{error}");
            });
        }

        #[test]
        fn a_non_string_element_names_the_argument_and_its_index() {
            let error = texts("['가', 2]").expect_err("2 is not a paragraph");

            assert!(error.to_string().contains("text[1]"), "{error}");
        }

        #[test]
        fn something_that_is_not_a_sequence_at_all_is_refused() {
            let error = texts("7").expect_err("an int is not paragraphs");

            assert!(error.to_string().contains("text"), "{error}");
        }
    }

    #[test]
    fn every_discovery_spelling_maps_to_its_variant() {
        assert_eq!(discovery("explicit").unwrap(), FontDiscovery::ExplicitOnly);
        assert_eq!(discovery("hancom").unwrap(), FontDiscovery::HancomBundle);
        assert_eq!(discovery("platform").unwrap(), FontDiscovery::Platform);
    }

    #[test]
    fn an_unknown_discovery_spelling_reports_the_canonical_code_and_lists_the_valid_ones() {
        let error = discovery("bundled").expect_err("not a discovery mode");

        assert_eq!(error.code(), OpsCode::InvalidDiscovery);
        let message = error.to_string();
        assert!(message.contains("bundled"), "{message}");
        assert!(message.contains("explicit|hancom|platform"), "{message}");
    }

    #[test]
    fn a_list_request_is_the_legacy_map_and_an_object_is_the_v2_envelope() {
        let legacy = stamp_map_from_json(&json!([])).expect("an empty spec list is a v1 map");
        assert!(matches!(legacy, StampMap::Legacy(specs) if specs.is_empty()));

        let envelope = stamp_map_from_json(&json!({
            "schema_version": 2,
            "source_sha256": "0".repeat(64),
            "text": [],
            "cells": [],
        }))
        .expect("a v2 envelope");
        assert!(matches!(envelope, StampMap::V2(_)));
    }

    #[test]
    fn a_json_value_that_is_not_a_stamp_map_reports_invalid_stamp_map() {
        let error = stamp_map_from_json(&json!("map.json")).expect_err("a string is not a map");

        assert_eq!(error.code(), OpsCode::InvalidStampMap);
    }
}
