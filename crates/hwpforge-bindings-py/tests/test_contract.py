"""Every extension function, called once, with its return shape and keys checked.

The Rust layer is left out of the workspace coverage gate because a cdylib
cannot be linked into a test binary, so this file is what keeps the 23
functions exercised: each one is called with the smallest valid arguments and
its result is matched against `_hwpforge.pyi`.
"""

from __future__ import annotations

import base64
import json
from collections.abc import Mapping
from typing import TYPE_CHECKING

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
from conftest import FONT_UNRESOLVED, INVALID_CACHE, all_paragraphs

from hwpforge import HwpForgeError, _hwpforge

if TYPE_CHECKING:
    from hwpforge._hwpforge import StampAction, StampCandidate, StampRequestV2, StampSpec


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


def _keys_match_stub(value: Mapping[str, object], stub_name: str) -> None:
    """Assert one object's keys against the stub's TypedDict for it."""
    required, optional = _stub.typed_dict_keys(stub_name)
    observed = set(value)
    assert required <= observed, f"{stub_name} is missing {sorted(required - observed)}"
    extra = sorted(observed - required - optional)
    assert not extra, f"{stub_name} has keys the stub does not declare: {extra}"


def test_a_body_paragraph_carries_no_variant_keys(table_bytes: bytes) -> None:
    body = [para for para in all_paragraphs(table_bytes) if para["kind"] == "body"]

    assert body, "the fixture must have at least one plain paragraph"
    for paragraph in body:
        _keys_match_stub(paragraph, "BodyParagraph")


def test_a_heading_paragraph_carries_its_level(heading_and_numbered_bytes: bytes) -> None:
    headings = [
        para for para in all_paragraphs(heading_and_numbered_bytes) if para["kind"] == "heading"
    ]

    assert headings, "the fixture must have at least one heading"
    for paragraph in headings:
        _keys_match_stub(paragraph, "HeadingParagraph")
        assert 1 <= paragraph["level"] <= 6


def test_a_list_paragraph_carries_its_family_depth_and_checkbox(
    checkable_list_bytes: bytes, heading_and_numbered_bytes: bytes
) -> None:
    """`checked` is present on every list item and `None` when it is not checkable."""
    checkable = [para for para in all_paragraphs(checkable_list_bytes) if para["kind"] == "list"]
    numbered = [
        para
        for para in all_paragraphs(heading_and_numbered_bytes)
        if para["kind"] == "list" and para["numbered"]
    ]

    assert checkable, "the fixture must have checkable list items"
    assert numbered, "the fixture must have a numbered list item"
    for paragraph in checkable + numbered:
        _keys_match_stub(paragraph, "ListParagraph")
        assert isinstance(paragraph["numbered"], bool)
        assert isinstance(paragraph["level"], int)
    assert {para["checked"] for para in checkable} == {True, False}
    assert all(para["checked"] is None for para in numbered)


def test_inspect_includes_the_style_summary_only_when_asked(table_bytes: bytes) -> None:
    """`styles` is absent, not `None`, when it was not requested."""
    without = _hwpforge.inspect(table_bytes, styles=False)
    with_styles = _hwpforge.inspect(table_bytes, styles=True)

    assert "styles" not in without
    assert "styles" in with_styles
    assert set(with_styles["styles"]) == {"fonts", "char_shapes", "para_shapes"}


def test_stamp_includes_the_manifest_only_when_asked(
    generated_bytes: bytes, stamp_request: StampRequestV2
) -> None:
    """`manifest` is the only key that comes and goes; the apply-phase outcome
    (`stamped`, `stamped_cells`, `ignored`, `skipped_guarded`) is always there,
    manifest or not."""
    _bytes, without = _hwpforge.stamp(generated_bytes, request=stamp_request, manifest=False)
    _again, with_manifest = _hwpforge.stamp(generated_bytes, request=stamp_request, manifest=True)

    apply_phase = {"stamped", "stamped_cells", "ignored", "skipped_guarded", "warnings"}
    assert set(without) == apply_phase
    assert set(with_manifest) == apply_phase | {"manifest"}
    _keys_match_stub(with_manifest["manifest"], "StampManifest")


def test_a_warning_is_absent_or_a_string_never_none(stale_line_cache_bytes: bytes) -> None:
    """The optional `hint` follows the same absent-not-null rule as every other key."""
    report = _hwpforge.outline(stale_line_cache_bytes)

    assert report["warnings"], "the fixture must produce at least one warning"
    for warning in report["warnings"]:
        _keys_match_stub(warning, "WarningInfo")
        if "hint" in warning:
            assert isinstance(warning["hint"], str)
            assert warning["hint"]


def _delete_with_a_string(data: bytes) -> object:
    return _hwpforge.delete_para(data, section=0, indexes="01")  # ty: ignore[invalid-argument-type]


def _set_cell_with_a_string(data: bytes) -> object:
    return _hwpforge.set_cell(data, specs="x")  # ty: ignore[invalid-argument-type]


def _render_with_a_string(data: bytes) -> object:
    # ty cannot flag this one: a `str` genuinely satisfies `Sequence[str]`, so
    # the runtime refusal is the only thing standing between a caller and four
    # one-character font directories.
    return _hwpforge.to_pdf(data, font_dirs="/tmp")


@pytest.mark.parametrize(
    "call",
    [
        pytest.param(_delete_with_a_string, id="indexes"),
        pytest.param(_set_cell_with_a_string, id="specs"),
        pytest.param(_render_with_a_string, id="font_dirs"),
    ],
)
def test_a_string_is_never_taken_as_a_sequence_of_characters(call, generated_bytes) -> None:
    """Every sequence parameter refuses a bare string rather than iterating it.

    A `str` satisfies `Sequence[str]`, so a caller who passes one where a list
    belongs gets no help from the type checker. Each of these would otherwise
    mean something absurd but well-formed: two paragraph indexes from `"01"`,
    four one-character font directories from a path.
    """
    with pytest.raises(TypeError):
        call(generated_bytes)


ACCEPTED_TEXT = [
    pytest.param(lambda: "가", 1, id="str"),
    pytest.param(lambda: ["가", "나"], 2, id="list"),
    pytest.param(lambda: ("가", "나"), 2, id="tuple"),
]

REFUSED_TEXT = [
    pytest.param(lambda: {"a": 1}, id="dict"),
    pytest.param(lambda: {"가"}, id="set"),
    pytest.param(lambda: (character for character in "가나"), id="generator"),
    pytest.param(lambda: bytearray(), id="bytearray"),
    pytest.param(lambda: b"x", id="bytes"),
    pytest.param(lambda: memoryview(b"x"), id="memoryview"),
    pytest.param(lambda: 3, id="int"),
    pytest.param(lambda: None, id="none"),
]


@pytest.mark.parametrize(("make", "expected"), ACCEPTED_TEXT)
def test_insert_para_accepts_a_string_or_a_real_sequence(
    make, expected: int, generated_bytes: bytes
) -> None:
    """One paragraph for a string, one per element for a list or a tuple."""
    _data, report = _hwpforge.insert_para(generated_bytes, section=0, anchor=0, text=make())

    assert report["inserted"] == expected


@pytest.mark.parametrize("make", REFUSED_TEXT)
def test_insert_para_refuses_anything_else_iterable(make, generated_bytes: bytes) -> None:
    """Being iterable is not enough to be a list of paragraphs.

    A mapping would insert its keys, a set would insert in an order nobody
    chose, a generator would be consumed by the call and leave the caller with
    an empty one, and the buffer types iterate into integers. Each is a
    mistake worth a `TypeError` rather than a document the caller did not ask
    for.
    """
    with pytest.raises(TypeError):
        _hwpforge.insert_para(generated_bytes, section=0, anchor=0, text=make())


def test_insert_para_names_the_element_that_was_not_a_string(generated_bytes: bytes) -> None:
    """The message points at the offending index, not just at the argument."""
    with pytest.raises(TypeError, match=r"text\[1\]"):
        # ty: ignore[invalid-argument-type] - the wrong element type is the point
        _hwpforge.insert_para(generated_bytes, section=0, anchor=0, text=["가", 1])


def test_insert_para_names_the_type_it_would_not_take(generated_bytes: bytes) -> None:
    with pytest.raises(TypeError, match="got generator"):
        # ty: ignore[invalid-argument-type] - a generator is not a sequence, which is the point
        _hwpforge.insert_para(
            generated_bytes, section=0, anchor=0, text=(character for character in "가나")
        )


def test_insert_para_with_an_empty_sequence_is_an_operation_error(generated_bytes: bytes) -> None:
    """Empty is well-typed but meaningless, so the operation refuses it, not the converter."""
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.insert_para(generated_bytes, section=0, anchor=0, text=[])

    assert caught.value.code == "INSERT_TEXT_REQUIRED"


# ── inspect: shallow vs. deep counts ───────────────────────────


def test_inspect_reports_shallow_and_deep_table_counts_separately(
    nested_table_bytes: bytes,
) -> None:
    """A table nested inside another table's cell is deep-only for the outer count."""
    report = _hwpforge.inspect(nested_table_bytes)

    section = report["section_details"][0]
    assert section["top_level_tables"] == 1, "only the outer table is a top-level run"
    assert section["tables"] == 2, "the nested table is counted deeply"
    assert section["top_level_tables"] != section["tables"]
    assert section["top_level_images"] == 0
    assert section["top_level_charts"] == 0


def test_inspect_section_matches_its_stub(table_bytes: bytes) -> None:
    """`InspectSection` carries nine `all_`/`deep_`/`top_level_non_empty_`
    counts beside the top-level ones — the runtime dict's key set must match
    the stub exactly, so a rename on the Rust side cannot land without the
    stub following it."""
    sections = _hwpforge.inspect(table_bytes)["section_details"]

    assert sections, "the fixture must have at least one section"
    for section in sections:
        _keys_match_stub(section, "InspectSection")


# ── from_json ───────────────────────────────────────────────────


def test_from_json_reports_the_generated_paragraph_count(table_bytes: bytes) -> None:
    """`paragraphs` counts top-level (body-flow) paragraphs only, the same
    definition `InspectSection.top_level_paragraphs` uses — not the deep count
    that also walks into table cells."""
    exported = _hwpforge.to_json(table_bytes)
    section_details = _hwpforge.inspect(table_bytes)["section_details"]
    expected = sum(section["top_level_paragraphs"] for section in section_details)

    _data, report = _hwpforge.from_json(json.dumps(exported["document"]))

    assert report["paragraphs"] == expected


def test_from_json_without_styles_falls_back_to_the_full_default_registry(
    table_bytes: bytes,
) -> None:
    """No `styles` in the JSON and no `base` still yields a document with real
    char/paragraph shapes, not a bare font-only stand-in."""
    exported = _hwpforge.to_json(table_bytes, styles=False)
    assert "styles" not in exported["document"]

    data, _report = _hwpforge.from_json(json.dumps(exported["document"]))

    insp = _hwpforge.inspect(data)
    assert insp["sections"] >= 1
    view = _hwpforge.read(data, section=0, paras="0")
    assert view["paragraphs"] is not None

    with_styles = _hwpforge.to_json(data, styles=True)["document"]["styles"]
    assert with_styles["fonts"], "the default preset registry defines fonts"
    assert with_styles["char_shapes"], "the default preset registry defines char shapes"
    assert with_styles["para_shapes"], "the default preset registry defines para shapes"


# ── stamp: apply-phase counters ────────────────────────────────


def test_stamp_reports_the_apply_phase_outcome(stamp_placeholder_bytes: bytes) -> None:
    """One candidate named, one explicitly ignored, one left because it is guarded."""
    plan = _hwpforge.stamp_plan(stamp_placeholder_bytes)
    unguarded = [c for c in plan["text"] if c["guard"] is None]
    assert len(unguarded) >= 2, "the fixture must offer at least two unguarded candidates"

    def spec(candidate: StampCandidate, action: StampAction) -> StampSpec:
        return {
            "section": candidate["section"],
            "path": candidate["path"],
            "span": candidate["span"],
            "marker": candidate["marker"],
            "action": action,
        }

    text: list[StampSpec] = [
        spec(unguarded[0], {"field": {"name": "게이트필드1", "hint": None}}),
        spec(unguarded[1], "ignore"),
    ]
    request: StampRequestV2 = {
        "schema_version": plan["schema_version"],
        "source_sha256": plan["source_sha256"],
        "text": text,
        "cells": [],
    }

    _data, report = _hwpforge.stamp(stamp_placeholder_bytes, request=request, manifest=True)

    assert len(report["stamped"]) == 1
    assert report["stamped"][0]["name"] == "게이트필드1"
    assert report["stamped_cells"] == []
    assert report["ignored"] == 1
    assert report["skipped_guarded"] >= 1, "the ※ instruction-context checkbox is guarded"
    for field in report["stamped"]:
        _keys_match_stub(field, "StampedField")


# ── convert_md: every built-in preset ──────────────────────────


def test_convert_md_accepts_every_builtin_preset() -> None:
    presets = [p["name"] for p in _hwpforge.templates()["presets"]]
    assert presets, "the preset table must not be empty"

    for name in presets:
        data, report = _hwpforge.convert_md("# 제목\n\n본문입니다.\n", preset=name)
        assert data, f"preset {name!r} produced no bytes"
        assert report["sections"] == 1


def test_convert_md_rejects_an_unknown_preset() -> None:
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.convert_md("# 제목\n\n본문입니다.\n", preset="no-such-preset")

    assert caught.value.code == "PRESET_NOT_FOUND"
