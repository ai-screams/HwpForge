//! Binding-only input conversion and additive metadata. Format and loss policy
//! stay in convert and the format owners.
use hwpforge_convert::ops::{open_as_hwpx, ConversionMeta, OpenOptions, OpenedHwpx};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;
use serde::Serialize;

use crate::errors::OrPy;

pub(crate) fn open<'a>(py: Python<'_>, bytes: &'a [u8]) -> PyResult<OpenedHwpx<'a>> {
    py.detach(|| open_as_hwpx(bytes, &OpenOptions::default())).or_py(py)
}

#[derive(Serialize)]
pub(crate) struct Meta<M, C> {
    #[serde(flatten)]
    inner: M,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversion: Option<C>,
}

pub(crate) fn metadata<M>(inner: M, input: &OpenedHwpx<'_>) -> Meta<M, ConversionMeta> {
    Meta { inner, conversion: input.conversion.as_ref().map(|report| report.meta()) }
}

#[derive(Serialize)]
pub(crate) struct DiffConversion {
    #[serde(skip_serializing_if = "Option::is_none")]
    base: Option<ConversionMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revised: Option<ConversionMeta>,
}

pub(crate) fn diff_metadata<M>(
    inner: M,
    base: &OpenedHwpx<'_>,
    revised: &OpenedHwpx<'_>,
) -> Meta<M, DiffConversion> {
    // 이것을 실패시키는 것: 어느 한 입력만 변환된 diff의 conversion 제거.
    let conversion = if base.conversion.is_some() || revised.conversion.is_some() {
        Some(DiffConversion {
            base: base.conversion.as_ref().map(|report| report.meta()),
            revised: revised.conversion.as_ref().map(|report| report.meta()),
        })
    } else {
        None
    };
    Meta { inner, conversion }
}

pub(crate) fn accept(py: Python<'_>, input: &OpenedHwpx<'_>, accepted: bool) -> PyResult<()> {
    // 이것을 실패시키는 것: 손실 거부 우회 또는 명시적 허용 무시.
    if let Some(report) = &input.conversion {
        report.ensure_accepted(accepted).or_py(py)?;
    }
    Ok(())
}

/// Prepare bytes for the Python path writer without exposing a Rust API.
#[pyfunction]
#[pyo3(signature = (data, /, *, accept_conversion_loss = false))]
pub(crate) fn _prepare_save<'py>(
    py: Python<'py>,
    data: PyBackedBytes,
    accept_conversion_loss: bool,
) -> PyResult<(Option<Bound<'py, PyBytes>>, Bound<'py, PyAny>)> {
    let input = open(py, &data)?;
    accept(py, &input, accept_conversion_loss)?;
    // 이것을 실패시키는 것: 저장 준비에서 변환 경고를 빈 메타데이터로 대체.
    let conversion = input.conversion.as_ref().map(|report| report.meta());
    Ok((
        conversion.as_ref().map(|_| PyBytes::new(py, &input.bytes)),
        crate::results::meta(py, &conversion)?,
    ))
}

#[cfg(all(test, not(feature = "extension-module")))]
mod tests {
    use super::*;

    #[test]
    // 이것을 실패시키는 것: Python 입력을 open_as_hwpx에 넘기기 전에 복사.
    fn non_cfb_input_keeps_python_allocation() {
        Python::initialize();
        Python::attach(|py| {
            let bytes = PyBytes::new(py, b"unchanged invalid input");
            let backed: PyBackedBytes = bytes.extract().unwrap();
            let opened = open(py, &backed).unwrap();
            assert_eq!(opened.bytes.as_ptr(), backed.as_ptr());
            assert!(matches!(opened.bytes, std::borrow::Cow::Borrowed(_)));
        });
    }
}
