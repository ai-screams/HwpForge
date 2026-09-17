"""`_hwpforge.pyi` must describe the module that is actually loaded."""

from __future__ import annotations

import inspect

import _stub
import pytest
from _ffi_names import FFI_FUNCTIONS, REPORTS

from hwpforge import _hwpforge


def test_pyi_matches_ffi() -> None:
    """The stub, the shared name list and the loaded module declare the same functions."""
    exported = {
        name
        for name, value in vars(_hwpforge).items()
        if not name.startswith("_") and inspect.isroutine(value)
    }

    assert set(_stub.function_names()) == set(FFI_FUNCTIONS)
    assert exported == set(FFI_FUNCTIONS)


def test_the_name_list_has_no_duplicates() -> None:
    assert len(set(FFI_FUNCTIONS)) == len(FFI_FUNCTIONS) == 23


@pytest.mark.parametrize("name", FFI_FUNCTIONS)
def test_every_report_type_is_declared(name: str) -> None:
    """Each function's report has a `TypedDict` in the stub, or is exempt on purpose."""
    stub_name = REPORTS[name]
    if stub_name is None:
        assert name == "schema", "only the JSON Schema report may go untyped"
        return

    required, optional = _stub.typed_dict_keys(stub_name)

    assert required or optional, f"{stub_name} declares no keys"


def test_the_public_surface_is_what_init_advertises() -> None:
    import hwpforge

    assert set(hwpforge.__all__) == {
        "BytesResult",
        "Document",
        "DocumentResult",
        "HwpForgeError",
        "TextResult",
        "__version__",
        "convert_hwp5",
        "convert_md",
        "from_json",
        "schema",
        "templates",
    }
    for name in hwpforge.__all__:
        assert hasattr(hwpforge, name), f"__all__ names {name}, which does not exist"


def test_every_document_method_has_a_matching_ffi_function() -> None:
    """A `Document` method is named after the operation it calls."""
    import hwpforge

    methods = {
        name
        for name, value in vars(hwpforge.Document).items()
        if not name.startswith("_") and (callable(value) or isinstance(value, classmethod))
    }
    loaders = {"open", "from_bytes", "to_bytes", "save"}

    assert methods - loaders <= set(FFI_FUNCTIONS)


def test_the_error_and_its_cause_are_declared() -> None:
    """The exception the extension raises is typed, and so is its second classification."""
    import inspect as inspect_module

    from hwpforge import HwpForgeError

    required, optional = _stub.typed_dict_keys("PdfCause")
    assert required == {"stage", "code"}
    assert optional == {"kind", "location"}

    parameters = inspect_module.signature(HwpForgeError.__init__).parameters
    assert list(parameters) == ["self", "code", "message", "hint", "cause"]
    assert parameters["hint"].default is None
    assert parameters["cause"].default is None


def test_the_package_is_marked_as_typed() -> None:
    from pathlib import Path

    import hwpforge

    package = Path(next(iter(hwpforge.__path__)))

    assert (package / "py.typed").is_file()
    assert (package / "_hwpforge.pyi").is_file()
