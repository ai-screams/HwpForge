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
use pyo3::types::{PyByteArray, PyBytes, PyMemoryView, PySequence, PyString};
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
///
/// Everything else is refused, because "iterable" is a much weaker promise
/// than "sequence" and the difference decides what a caller gets:
///
/// - a **mapping** would insert its keys, which is never what the caller meant;
/// - a **set** has no order, so the paragraphs would land in a different order
///   on a different run;
/// - a **generator** has no length and is consumed by reading it, so a retry
///   after any later failure would insert nothing;
/// - **`bytes`**, **`bytearray`** and **`memoryview`** are sequences of
///   integers, and turning them into text means guessing an encoding the
///   caller never gave.
///
/// So the check is `collections.abc.Sequence` (which `list`, `tuple` and a
/// registered custom sequence satisfy, and a `dict`, `set` or generator does
/// not), minus the binary sequences, and the elements are read by index.
///
/// An empty sequence is passed through: refusing it here would take the
/// decision away from the operation, which reports `INSERT_TEXT_REQUIRED`.
///
/// # Errors
///
/// `TypeError` naming the argument when the value is not a string or a
/// sequence of strings, and naming the index when one element is not a string.
pub(crate) fn paragraph_texts(name: &str, value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if let Ok(single) = value.cast::<PyString>() {
        return Ok(vec![single.extract::<String>()?]);
    }
    if value.is_instance_of::<PyBytes>()
        || value.is_instance_of::<PyByteArray>()
        || value.is_instance_of::<PyMemoryView>()
    {
        return Err(not_paragraphs(name, value));
    }
    let Ok(sequence) = value.cast::<PySequence>() else {
        return Err(not_paragraphs(name, value));
    };

    let length = sequence.len()?;
    let mut texts = Vec::with_capacity(length);
    for index in 0..length {
        let item = sequence.get_item(index)?;
        let text = item.cast::<PyString>().map_err(|_| {
            PyTypeError::new_err(format!("{name}[{index}]: expected str, got {}", type_name(&item)))
        })?;
        texts.push(text.extract::<String>()?);
    }
    Ok(texts)
}

/// The refusal both non-sequence rejections share.
fn not_paragraphs(name: &str, value: &Bound<'_, PyAny>) -> PyErr {
    PyTypeError::new_err(format!(
        "{name}: expected a str or a sequence of str, got {}",
        type_name(value)
    ))
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

        /// `Result::unwrap_err`, but reporting the success value's contents.
        trait UnwrapErrOr {
            fn unwrap_err_or_else(self, message: impl FnOnce(&Vec<String>) -> String) -> String;
        }

        impl UnwrapErrOr for PyResult<Vec<String>> {
            fn unwrap_err_or_else(self, message: impl FnOnce(&Vec<String>) -> String) -> String {
                match self {
                    Ok(texts) => panic!("{}", message(&texts)),
                    Err(error) => error.to_string(),
                }
            }
        }

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

        /// Iterating anything iterable would take a mapping's keys, a set's
        /// arbitrary order, and a generator that a retry cannot read twice.
        #[test]
        fn an_iterable_that_is_not_a_sequence_is_refused() {
            for (source, named) in [
                ("{'first': 1, 'second': 2}", "dict"),
                ("{'가', '나'}", "set"),
                ("(c for c in ['가', '나'])", "generator"),
                ("iter(['가'])", "list_iterator"),
            ] {
                let error = texts(source)
                    .unwrap_err_or_else(|texts| format!("{source} was accepted as {texts:?}"));

                assert!(error.contains("expected a str or a sequence of str"), "{source}: {error}");
                assert!(error.contains(named), "{source}: {error}");
            }
        }

        #[test]
        fn a_binary_sequence_is_refused_even_though_it_is_a_sequence() {
            for source in ["b'ab'", "bytearray(b'ab')", "bytearray()", "memoryview(b'ab')"] {
                let error = texts(source).expect_err("binary is not text");

                assert!(
                    error.to_string().contains("text: expected a str or a sequence of str"),
                    "{source}: {error}"
                );
            }
        }

        /// The empty list is the operation's call, not this helper's: it
        /// reports `INSERT_TEXT_REQUIRED`.
        #[test]
        fn an_empty_sequence_is_passed_through_but_an_empty_bytearray_is_not() {
            assert!(texts("[]").expect("an empty list is a valid argument").is_empty());
            assert!(texts("()").expect("an empty tuple is a valid argument").is_empty());
            texts("bytearray()").expect_err("an empty bytearray is still binary");
        }

        /// A `str` subclass is still one paragraph, and a registered custom
        /// sequence is still a sequence.
        #[test]
        fn subclasses_and_registered_sequences_are_honoured() {
            assert_eq!(
                texts("type('S', (str,), {})('가나')").expect("a str subclass is a str"),
                vec!["가나".to_owned()]
            );
            assert_eq!(
                texts("__import__('collections').UserList(['가', '나'])").expect("a sequence"),
                vec!["가".to_owned(), "나".to_owned()]
            );
        }

        /// `UserString` wraps a string rather than subclassing one, so it is
        /// not a `str`, and as a registered sequence its elements are more
        /// `UserString`s. Reading it as paragraphs would be the per-character
        /// explosion again, so it is refused and the caller converts it.
        #[test]
        fn a_string_like_wrapper_is_refused_rather_than_read_as_characters() {
            let error = texts("__import__('collections').UserString('가나')")
                .expect_err("not a str and not a sequence of str");

            assert!(error.to_string().contains("expected str, got UserString"), "{error}");
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
