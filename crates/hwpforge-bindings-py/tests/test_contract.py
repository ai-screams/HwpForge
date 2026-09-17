"""Every extension function, called once, with its return shape and keys checked.

The Rust layer is left out of the workspace coverage gate because a cdylib
cannot be linked into a test binary, so this file is what keeps the 23
functions exercised: each one is called with the smallest valid arguments and
its result is matched against `_hwpforge.pyi`.
"""

from __future__ import annotations

import base64

import _stub
import pytest
from _ffi_names import (
    BYTES_AND_REPORT,
    FFI_FUNCTIONS,
    REPORT_ONLY,
    REPORTS,
    SHAPES,
    TEXT_AND_REPORT,
)
from conftest import FONT_UNRESOLVED, INVALID_CACHE

from hwpforge import HwpForgeError, _hwpforge


def _always_reports_warnings(name: str) -> bool:
    report = REPORTS[name]
    return report is not None and "warnings" in _stub.typed_dict_keys(report)[0]


WARNING_FUNCTIONS = tuple(name for name in FFI_FUNCTIONS if _always_reports_warnings(name))
"""Every function whose report always carries a `warnings` list."""


def _report_of(name: str, returned: object) -> dict:
    """Unwrap the report from what `name` returned, checking the shape on the way."""
    shape = SHAPES[name]
    if shape is REPORT_ONLY:
        assert isinstance(returned, dict), f"{name} must return a dict, got {type(returned)}"
        return returned

    assert isinstance(returned, tuple), f"{name} must return a tuple, got {type(returned)}"
    assert len(returned) == 2, f"{name} must return two values, got {len(returned)}"
    output, report = returned
    if shape is BYTES_AND_REPORT:
        assert isinstance(output, bytes), f"{name} must produce bytes, got {type(output)}"
        assert output, f"{name} produced empty bytes"
    else:
        assert shape is TEXT_AND_REPORT
        assert isinstance(output, str), f"{name} must produce str, got {type(output)}"
    assert isinstance(report, dict), f"{name} must report a dict, got {type(report)}"
    return report


def _call(name: str, ffi_calls: dict) -> dict:
    """Call `name` with its smallest valid arguments and return its report."""
    args, kwargs = ffi_calls[name]
    return _report_of(name, getattr(_hwpforge, name)(*args, **kwargs))


@pytest.mark.parametrize("name", FFI_FUNCTIONS)
def test_ffi_contract(name: str, ffi_calls: dict) -> None:
    report = _call(name, ffi_calls)

    stub_name = REPORTS[name]
    if stub_name is None:
        assert report, f"{name} returned an empty schema"
        return
    required, optional = _stub.typed_dict_keys(stub_name)
    observed = set(report)
    assert required <= observed, f"{name} is missing {sorted(required - observed)}"
    extra = sorted(observed - required - optional)
    assert not extra, f"{name} reports keys the stub does not declare: {extra}"


@pytest.mark.parametrize("name", WARNING_FUNCTIONS)
def test_warnings_are_data_not_python_warnings(name: str, ffi_calls: dict) -> None:
    """A warning is always a list of `{code, message}` objects in the report."""
    report = _call(name, ffi_calls)

    assert isinstance(report["warnings"], list)
    for warning in report["warnings"]:
        assert set(warning) <= {"code", "message", "hint"}
        assert isinstance(warning["code"], str)
        assert warning["code"]
        assert isinstance(warning["message"], str)


def test_to_md_carries_the_images_as_bytes(picture_bytes: bytes) -> None:
    """`images` must be a map of `bytes`, not of lists of integers."""
    _text, report = _hwpforge.to_md(picture_bytes, mode="styled")

    assert report["images"], "the picture fixture must export at least one image"
    for key, value in report["images"].items():
        assert isinstance(key, str)
        assert isinstance(value, bytes), f"{key} came back as {type(value)}"


def test_lossy_markdown_reports_what_it_dropped(table_bytes: bytes) -> None:
    """Markdown cannot express a merged cell, and the export says so rather than lying."""
    _text, report = _hwpforge.to_md(table_bytes, mode="lossy")

    assert report["mode"] == "lossy"
    assert "TABLE_MERGE_FLATTENED" in [warning["code"] for warning in report["warnings"]]


def test_to_pdf_renders_with_the_committed_test_fonts(
    synthetic_face_bytes: bytes, pdf_font_dir: str
) -> None:
    """The strict path: default fatal mode, every face resolved, nothing lost."""
    data, report = _hwpforge.to_pdf(synthetic_face_bytes, font_dirs=[pdf_font_dir])

    assert data.startswith(b"%PDF")
    assert report["pages"] >= 1
    assert set(report) == {"pages", "warnings"}
    assert report["warnings"] == [], "a document whose faces all resolve loses nothing"


def test_an_unresolved_face_is_refused_rather_than_drawn_as_blanks(pdf_bytes: bytes) -> None:
    """No font substitution, ever: a face the renderer cannot find is an error."""
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.to_pdf(pdf_bytes)

    assert caught.value.code == "PDF_RENDER_FAILED"
    assert caught.value.cause == FONT_UNRESOLVED
    assert "함초롬" in caught.value.message


def test_a_document_without_a_layout_cache_says_which_section(pdf_font_dir: str) -> None:
    """The second classification carries `location` when the renderer knows one."""
    generated, _report = _hwpforge.convert_md("# 제목\n\n본문입니다.\n")

    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.to_pdf(generated, font_dirs=[pdf_font_dir])

    assert caught.value.cause == {
        "stage": "render",
        "code": "NO_RENDERABLE_CACHE",
        "location": "s0",
    }


def test_a_cache_that_disagrees_with_the_document_is_refused(
    stale_cache_bytes: bytes, pdf_font_dir: str
) -> None:
    """A third reachable cause: the cached layout does not survive being replayed.

    The refusal comes before font resolution, so it wins even though this
    document also names faces no machine here can resolve. That ordering is
    the point: the renderer will not draw a page it has already proved wrong.
    """
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.to_pdf(stale_cache_bytes, font_dirs=[pdf_font_dir])

    assert caught.value.code == "PDF_RENDER_FAILED"
    assert caught.value.cause == INVALID_CACHE
    assert "layout cache" in caught.value.message


def test_a_failure_outside_the_renderer_classifies_once() -> None:
    """`cause` is `None`, not an empty dict, for everything that is not a render."""
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.outline(b"not an hwpx package at all")

    assert caught.value.cause is None


@pytest.mark.parametrize(
    ("call", "code"),
    [
        # ty: ignore[invalid-argument-type] - the value outside the set is the point
        (lambda data: _hwpforge.to_md(data, mode="nope"), "INVALID_INPUT"),
        # ty: ignore[invalid-argument-type] - the value outside the set is the point
        (lambda data: _hwpforge.to_pdf(data, discovery="nope"), "INVALID_DISCOVERY"),
        # ty: ignore[invalid-argument-type] - a JSON string is exactly what is refused
        (lambda data: _hwpforge.stamp(data, request="{}"), "INVALID_STAMP_MAP"),
    ],
)
def test_a_value_outside_the_allowed_set_is_an_operation_error(call, code, table_bytes) -> None:
    """A wrong enum value is refused by the operation, not by PyO3's converter."""
    with pytest.raises(HwpForgeError) as caught:
        call(table_bytes)

    assert caught.value.code == code
    assert caught.value.cause is None


def test_an_unknown_schema_kind_is_an_operation_error() -> None:
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.schema(kind="nope")  # ty: ignore[invalid-argument-type] - refused on purpose

    assert caught.value.code == "INVALID_INPUT"


def test_a_malformed_spec_is_a_python_value_error(table_bytes: bytes) -> None:
    """Structural argument failures stay as what the converter raised."""
    with pytest.raises(ValueError, match="specs") as caught:
        # ty: ignore[invalid-argument-type,missing-typed-dict-key] - malformed on purpose
        _hwpforge.set_cell(table_bytes, specs=[{"text": "x"}])

    assert not isinstance(caught.value, HwpForgeError)


def test_the_assets_list_is_tagged_by_kind() -> None:
    """`convert_md` reports each image as one of three tagged shapes."""
    _data, report = _hwpforge.convert_md("# 제목\n\n![a](missing.png)\n")

    assets = report["assets"]
    assert [asset["kind"] for asset in assets] == ["dropped"]
    dropped = assets[0]
    assert dropped["kind"] == "dropped"
    assert set(dropped) == {"kind", "occurrence", "reason"}
    assert set(dropped["occurrence"]) == {"paragraph", "run"}
    assert dropped["reason"] == "no_base_dir"


def test_an_embedded_asset_names_its_key_and_format(tmp_path) -> None:
    """A resolvable image is embedded, and its format keeps Core's own spelling."""
    png = base64.b64decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
    )
    (tmp_path / "a.png").write_bytes(png)

    _data, report = _hwpforge.convert_md("# 제목\n\n![a](a.png)\n", base_dir=tmp_path)

    embedded = report["assets"][0]
    assert embedded["kind"] == "embedded"
    assert set(embedded) == {"kind", "occurrence", "key", "format"}
    assert embedded["format"] == "Png", "Core's ImageFormat is not renamed"
    assert embedded["key"].endswith(".png")


def test_a_failure_raises_hwpforge_error_with_a_code() -> None:
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.outline(b"not an hwpx package at all")

    assert caught.value.code == "DECODE_FAILED"
    assert caught.value.message
    assert str(caught.value).startswith("DECODE_FAILED: ")


def test_an_unknown_preset_is_refused(table_bytes: bytes) -> None:
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.restyle(table_bytes, preset="no-such-preset")

    assert caught.value.code == "PRESET_NOT_FOUND"


def test_a_bad_argument_type_is_a_python_error(table_bytes: bytes) -> None:
    """PyO3's own conversion errors are not wrapped."""
    with pytest.raises(TypeError):
        # ty: ignore[invalid-argument-type] - the wrong type is the point of the test
        _hwpforge.inspect(table_bytes, styles="yes")


def test_read_without_a_target_is_refused(table_bytes: bytes) -> None:
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.read(table_bytes)

    assert caught.value.code == "READ_TARGET_REQUIRED"
