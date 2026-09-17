"""The pure-Python surface: the document value object, results and errors."""

from __future__ import annotations

import json
import pickle
import re
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from conftest import FONT_UNRESOLVED, all_paragraphs

import hwpforge
from hwpforge import BytesResult, Document, DocumentResult, HwpForgeError, TextResult

if TYPE_CHECKING:
    from hwpforge._hwpforge import StampRequestV2

EMAIL = "hanyul@example.com"


def test_version_is_a_real_version() -> None:
    assert re.match(r"^\d+\.\d+\.\d+", hwpforge.__version__), hwpforge.__version__


def test_open_and_save_round_trip(tmp_path, table_bytes: bytes) -> None:
    source = tmp_path / "in.hwpx"
    source.write_bytes(table_bytes)

    doc = Document.open(source)
    target = tmp_path / "out.hwpx"
    doc.save(target)

    assert target.read_bytes() == table_bytes
    assert Document.open(target) == doc


def test_a_document_is_a_value(table_bytes: bytes) -> None:
    one = Document.from_bytes(table_bytes)
    same = Document.from_bytes(bytes(table_bytes))
    other = Document.from_bytes(table_bytes + b"\0")

    assert one == same
    assert hash(one) == hash(same)
    assert one != other
    assert one != table_bytes
    assert bytes(one) == table_bytes
    assert one.to_bytes() == table_bytes
    assert len(one) == len(table_bytes)
    assert repr(one) == f"Document({len(table_bytes)} bytes)"


def test_a_document_cannot_be_changed(document: Document) -> None:
    with pytest.raises(AttributeError):
        document.anything = 1
    with pytest.raises(AttributeError):
        del document._data
    with pytest.raises(TypeError):
        # ty: ignore[invalid-argument-type] - the wrong type is the point of the test
        Document.from_bytes("not bytes")


def test_editing_leaves_the_receiver_alone(fields_bytes: bytes) -> None:
    document = Document.from_bytes(fields_bytes)
    before = document.to_bytes()

    result = document.fill({"user_email": EMAIL})

    assert isinstance(result, DocumentResult)
    assert isinstance(result.document, Document)
    assert result.document != document
    assert document.to_bytes() == before, "fill must not touch the document it was called on"
    assert [field["name"] for field in result.report["filled"]] == ["user_email"]


def test_results_are_frozen(fields_bytes: bytes) -> None:
    result = Document.from_bytes(fields_bytes).fill({"user_email": EMAIL})

    with pytest.raises(AttributeError):
        result.document = result.document  # ty: ignore[invalid-assignment] - frozen on purpose
    with pytest.raises(AttributeError):
        result.report = {}  # ty: ignore[invalid-assignment] - frozen on purpose


def test_open_outline_fill_save_to_json(tmp_path, fields_bytes: bytes) -> None:
    """The walk-through from the README, end to end."""
    document = Document.from_bytes(fields_bytes)
    outline = document.outline()
    assert "outline" in outline

    doc = document.fill({"user_email": EMAIL}).document
    path = tmp_path / "filled.hwpx"
    doc.save(path)

    exported = Document.open(path).to_json()
    assert isinstance(exported, TextResult)
    assert json.loads(exported.text) == exported.report["document"]


def test_export_section_text_matches_the_report(document: Document) -> None:
    exported = document.export_section(section=0)

    assert isinstance(exported, TextResult)
    assert json.loads(exported.text) == exported.report["section"]


def test_to_md_returns_text_and_a_report(document: Document) -> None:
    exported = document.to_md()

    assert isinstance(exported, TextResult)
    assert exported.text.strip()
    assert exported.report["mode"] == "styled"


def test_to_pdf_returns_bytes_and_a_page_count(
    synthetic_face_bytes: bytes, pdf_font_dir: str
) -> None:
    """The renderer's strict path through `Document`, with a `Path` for the font directory."""
    rendered = Document.from_bytes(synthetic_face_bytes).to_pdf(font_dirs=[Path(pdf_font_dir)])

    assert isinstance(rendered, BytesResult)
    assert rendered.data.startswith(b"%PDF")
    assert rendered.report["pages"] >= 1
    assert set(rendered.report) == {"pages", "warnings"}
    assert rendered.report["warnings"] == []


def test_to_pdf_refuses_a_face_it_cannot_find(document: Document) -> None:
    """A document naming a Hancom face is refused wherever Hancom is not installed."""
    with pytest.raises(HwpForgeError) as caught:
        document.to_pdf()

    assert caught.value.code == "PDF_RENDER_FAILED"
    assert caught.value.cause == FONT_UNRESOLVED


def test_queries_come_back_as_plain_dicts(document: Document) -> None:
    assert document.inspect()["sections"] >= 1
    assert isinstance(document.fields()["fields"], list)
    assert document.validate()["ok"] is True
    assert document.stamp_plan()["schema_version"] >= 1
    assert document.diff(document)["identical"] is True


def test_read_names_its_target(document: Document) -> None:
    read = document.read(section=0, paras="0")

    assert read["paragraphs"] is not None
    assert read["table"] is None
    assert read["fields"] is None


def test_convert_md_builds_a_document() -> None:
    result = hwpforge.convert_md("# 제목\n\n본문입니다.\n")

    assert isinstance(result, DocumentResult)
    assert result.document.inspect()["paragraphs"] >= 1
    assert result.report["assets"] == []


def test_from_json_rebuilds_what_to_json_exported(document: Document) -> None:
    exported = document.to_json()

    rebuilt = hwpforge.from_json(exported.text)

    assert isinstance(rebuilt, DocumentResult)
    assert rebuilt.document.inspect()["sections"] == document.inspect()["sections"]


def test_convert_hwp5_reads_a_binary_document(hwp5_bytes: bytes) -> None:
    result = hwpforge.convert_hwp5(hwp5_bytes)

    assert isinstance(result, DocumentResult)
    assert result.document.inspect()["sections"] >= 1


def test_templates_and_schema_need_no_document() -> None:
    presets = hwpforge.templates()["presets"]

    assert presets
    assert {"name", "description", "font", "page_size"} == set(presets[0])
    assert hwpforge.schema(kind="exported-section")


def test_the_error_carries_a_code_a_message_and_a_hint() -> None:
    with pytest.raises(HwpForgeError) as caught:
        Document.from_bytes(b"not a package").outline()
    error = caught.value

    assert error.code == "DECODE_FAILED"
    assert error.args == (error.message,)
    assert str(error).startswith(f"{error.code}: ")
    assert isinstance(error, Exception)


def test_the_error_survives_pickling() -> None:
    error = HwpForgeError("SOME_CODE", "something", "try that instead")

    revived = pickle.loads(pickle.dumps(error))

    assert (revived.code, revived.message, revived.hint) == (
        "SOME_CODE",
        "something",
        "try that instead",
    )
    assert str(revived) == "SOME_CODE: something\nhint: try that instead"
    assert repr(error).startswith("HwpForgeError(code='SOME_CODE'")


def test_the_error_without_a_hint_is_one_line() -> None:
    error = HwpForgeError("C", "m")

    assert str(error) == "C: m"
    assert error.hint is None
    assert error.cause is None


def test_the_extension_module_is_private() -> None:
    assert "_hwpforge" not in hwpforge.__all__


def test_every_editing_method_returns_a_new_document(
    generated_bytes: bytes, stamp_request: StampRequestV2
) -> None:
    """The `Document` wrappers, not just the extension functions underneath them."""
    document = Document.from_bytes(generated_bytes)
    before = document.to_bytes()

    edits = {
        "set_cell": document.set_cell(table=0, at="0,0", text="값"),
        "patch": document.patch(section=0, patch=document.export_section(section=0).text),
        "delete_para": document.delete_para(section=0, indexes=[1]),
        "insert_para": document.insert_para(section=0, anchor=0, text=["새 문단"]),
        "stamp": document.stamp(stamp_request),
        "restyle": document.restyle(preset="modern"),
    }

    for name, result in edits.items():
        assert isinstance(result, DocumentResult), name
        assert isinstance(result.document, Document), name
        assert result.document.to_bytes(), name
        assert isinstance(result.report, dict), name
    assert document.to_bytes() == before, "no edit may touch the document it was called on"
    assert edits["insert_para"].report["inserted"] == 1
    assert edits["delete_para"].report["deleted"] == 1
    assert edits["patch"].report["section"] == 0
    assert edits["restyle"].report["preset"] == "modern"


def test_a_cell_can_be_named_by_a_neighbouring_label(generated_bytes: bytes) -> None:
    """`right_of` and `below` find a cell by the label next to it."""
    document = Document.from_bytes(generated_bytes)

    result = document.set_cell(table=0, right_of="성명", text="류한율")

    assert result.report["results"][0]["table"] == 0
    assert result.document != document


def test_fields_reports_the_warnings_reading_the_document_produced(
    stale_line_cache_bytes: bytes,
) -> None:
    """Listing fields decodes the document, so it reports what decoding lost.

    Without this the operation would be the one query that drops its warnings
    on the floor: a caller listing the fields of a document with a stale line
    cache would never learn the cache had gone.
    """
    report = Document.from_bytes(stale_line_cache_bytes).fields()

    assert set(report) == {"fields", "warnings"}
    assert [warning["code"] for warning in report["warnings"]] == ["LAYOUT_CACHE_DROPPED"]
    assert "section[0].para[4]" in report["warnings"][0]["message"]


RELOAD_PROBE = """
import importlib, json

import hwpforge

broken = hwpforge.Document.from_bytes(b"not a package at all")
measured = {}


def raised():
    try:
        broken.outline()
    except BaseException as error:
        return error
    raise SystemExit("the call must fail")


# 1. Untouched: the class the extension raises is the one both names hold.
first = hwpforge.errors.HwpForgeError
error = raised()
measured["fresh_is_module_class"] = type(error) is first
measured["fresh_caught_by_package"] = isinstance(error, hwpforge.HwpForgeError)
measured["code"] = getattr(error, "code", None)

# 2. Submodule reloaded: a new class, and the raise follows it rather than a
#    cached one. The package alias still holds the previous class, which is
#    ordinary reload semantics for a re-exported name.
importlib.reload(hwpforge.errors)
second = hwpforge.errors.HwpForgeError
error = raised()
measured["reload_made_a_new_class"] = second is not first
measured["submodule_raises_live_class"] = type(error) is second
measured["submodule_raises_stale_class"] = type(error) is first
measured["submodule_caught_by_module"] = isinstance(error, hwpforge.errors.HwpForgeError)
measured["submodule_caught_by_package"] = isinstance(error, hwpforge.HwpForgeError)

# 3. Package reloaded: the alias is rebound to the live class and catches again.
importlib.reload(hwpforge)
third = hwpforge.errors.HwpForgeError
error = raised()
measured["package_reload_kept_the_class"] = third is second
measured["package_alias_rebound"] = hwpforge.HwpForgeError is third
measured["package_raises_live_class"] = type(error) is third
measured["package_caught_by_package"] = isinstance(error, hwpforge.HwpForgeError)

print(json.dumps(measured))
"""


def test_the_exception_survives_reloading_its_module() -> None:
    """The class the extension raises is looked up per raise, never cached.

    A cached class keeps pointing at the module object that existed when the
    cache was filled. After a reload that object is stale, so the extension
    would raise a class with the right name that is not the one the module now
    holds, and a caller's `except` would silently stop matching.

    Three states are checked. Untouched, both names agree. With the submodule
    reloaded, the raise follows the live class, while `hwpforge.HwpForgeError`
    still holds the previous one: that is what a re-exported name does on a
    reload, not a pinned class, so the assertion below states it rather than
    calling it a defect. With the package reloaded too, the alias is rebound
    and catches again.

    It runs in a subprocess. Rebinding a class that other test modules have
    already imported by name would break them for the rest of the session,
    which is the same staleness this test exists to catch.
    """
    finished = subprocess.run(
        [sys.executable, "-c", RELOAD_PROBE],
        capture_output=True,
        text=True,
        check=True,
        timeout=120,
        cwd=Path(__file__).resolve().parent,
    )
    measured = json.loads(finished.stdout.strip().splitlines()[-1])

    assert measured["fresh_is_module_class"]
    assert measured["fresh_caught_by_package"]
    assert measured["code"] == "DECODE_FAILED"

    assert measured["reload_made_a_new_class"], "the reload did not produce a new class"
    assert measured["submodule_raises_live_class"], (
        "the raised class is not the one the reloaded module holds, so it was cached"
    )
    assert not measured["submodule_raises_stale_class"]
    assert measured["submodule_caught_by_module"]
    assert not measured["submodule_caught_by_package"], (
        "the package alias is a re-exported name, so it keeps the previous class "
        "until the package itself is reloaded"
    )

    assert measured["package_reload_kept_the_class"]
    assert measured["package_alias_rebound"]
    assert measured["package_raises_live_class"]
    assert measured["package_caught_by_package"]


def _texts(document: Document) -> list[str]:
    """The text of every top-level paragraph of section 0, read back from the bytes."""
    return [paragraph["text"] for paragraph in all_paragraphs(document.to_bytes())]


def test_insert_para_takes_a_string_as_one_paragraph(generated_bytes: bytes) -> None:
    """A string is one paragraph, not one paragraph per character.

    `Vec<String>` on the Rust side and `Sequence[str]` in the stub are both
    satisfied by a `str`, which iterates as single characters, so this is the
    one shape a type checker cannot catch for us.
    """
    document = Document.from_bytes(generated_bytes)
    before = _texts(document)

    result = document.insert_para(section=0, anchor=0, text="가나다")
    after = _texts(result.document)

    assert result.report["inserted"] == 1
    assert len(after) == len(before) + 1, f"expected one new paragraph, got {after}"
    assert "가나다" in after
    assert "가" not in after, "the string must not have been split into characters"


def test_insert_para_takes_a_sequence_as_one_paragraph_each(generated_bytes: bytes) -> None:
    document = Document.from_bytes(generated_bytes)
    before = _texts(document)

    result = document.insert_para(section=0, anchor=0, text=["가", "나"])
    after = _texts(result.document)

    assert result.report["inserted"] == 2
    assert len(after) == len(before) + 2
    assert "가" in after
    assert "나" in after


def test_insert_para_refuses_bytes_and_non_string_elements(generated_bytes: bytes) -> None:
    document = Document.from_bytes(generated_bytes)

    with pytest.raises(TypeError):
        document.insert_para(section=0, anchor=0, text=b"x")  # ty: ignore[invalid-argument-type]
    with pytest.raises(TypeError):
        document.insert_para(section=0, anchor=0, text=["가", 1])  # ty: ignore[invalid-argument-type]


def test_insert_para_with_nothing_to_insert_is_refused(generated_bytes: bytes) -> None:
    with pytest.raises(HwpForgeError) as caught:
        Document.from_bytes(generated_bytes).insert_para(section=0, anchor=0, text=[])

    assert caught.value.code == "INSERT_TEXT_REQUIRED"


def test_a_fill_is_still_there_after_saving_and_reopening(tmp_path, fields_bytes: bytes) -> None:
    """The point of the library: the edit is in the file, not only in the report."""
    filled = Document.from_bytes(fields_bytes).fill({"user_email": EMAIL})
    path = tmp_path / "filled.hwpx"
    filled.document.save(path)

    reopened = Document.open(path)

    current = {field["name"]: field["current"] for field in reopened.fields()["fields"]}
    assert current["user_email"] == EMAIL


def test_a_cell_edit_is_still_there_after_saving_and_reopening(
    tmp_path, generated_bytes: bytes
) -> None:
    edited = Document.from_bytes(generated_bytes).set_cell(table=0, at="1,1", text="채운 값")
    path = tmp_path / "celled.hwpx"
    edited.document.save(path)

    table = Document.open(path).read(table=0)["table"]

    assert table is not None
    cells = {(cell["row"], cell["col"]): cell["text"] for cell in table["cells"]}
    assert cells[(1, 1)] == "채운 값"


def test_an_inserted_paragraph_is_still_there_after_saving_and_reopening(
    tmp_path, generated_bytes: bytes
) -> None:
    inserted = Document.from_bytes(generated_bytes).insert_para(
        section=0, anchor=0, text="저장 뒤에도 남는 문단"
    )
    path = tmp_path / "inserted.hwpx"
    inserted.document.save(path)

    reopened = _texts(Document.open(path))
    assert "저장 뒤에도 남는 문단" in reopened
    assert len(reopened) == len(_texts(Document.from_bytes(generated_bytes))) + 1, (
        f"exactly one paragraph should have survived the round trip, got {reopened}"
    )


@pytest.mark.parametrize(
    ("name", "call"),
    [
        ("font_dirs", lambda doc: doc.to_pdf(font_dirs="/tmp")),
        ("indexes", lambda doc: doc.delete_para(section=0, indexes="01")),
        ("specs", lambda doc: doc.set_cell(specs="x")),
    ],
)
def test_document_does_not_widen_a_string_into_a_sequence(
    name: str, call, generated_bytes: bytes
) -> None:
    """The wrapper passes sequences through so the extension's guard sees them.

    Coercing with `list()` here would turn `font_dirs="/tmp"` into four
    one-character directories and hand them on as a well-formed list, and the
    caller would only ever see a font that failed to resolve.
    """
    with pytest.raises(TypeError):
        call(Document.from_bytes(generated_bytes))
