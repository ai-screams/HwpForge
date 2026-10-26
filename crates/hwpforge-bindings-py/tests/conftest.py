"""Fixtures for the Python binding tests.

Documents come from the Rust crates' own fixture directories rather than from
Python-only copies, so the two test suites exercise the same bytes.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

import pytest

import hwpforge
from hwpforge import _hwpforge

if TYPE_CHECKING:
    from hwpforge._hwpforge import StampRequestV2

CRATES = Path(__file__).resolve().parent.parent.parent
REPO = CRATES.parent
HWPX_FIXTURES = CRATES / "hwpforge-smithy-hwpx" / "tests" / "fixtures"
FIELD_FIXTURES = REPO / "tests" / "fixtures" / "fields"
LAYOUT_FIXTURES = REPO / "tests" / "fixtures" / "layout"
STAMP_FIXTURES = REPO / "tests" / "fixtures" / "stamp"
SYNTHETIC_FACE = Path(__file__).resolve().parent / "fixtures" / "synthetic_face.hwpx"
PDF_TEST_FONTS = Path(__file__).resolve().parent / "fixtures" / "fonts"

GENERATED_MARKDOWN = """# 제목

첫째 문단입니다.

둘째 문단입니다.

셋째 문단입니다.

| 이름 | 값 |
| -- | -- |
| 성명 | 빈칸 |
"""
"""Markdown for a document the editing operations accept.

The preserve-first editors refuse any input they cannot prove re-encodes losslessly
(`INPUT_NOT_ROUNDTRIP_SAFE`). A document this library generated itself is
round-trip safe by construction, so it is what the editing contract calls use.
"""


FONT_UNRESOLVED = {"stage": "render", "code": "FONT_UNRESOLVED"}
"""The whole `cause` of a face the renderer cannot find.

Asserted as an exact dict rather than by its `code`, so that the optional
`kind` and `location` keys are pinned as genuinely absent from the wire rather
than present and `None`.
"""


def _read(path: Path) -> bytes:
    """Read a committed fixture, failing loudly if it has moved.

    A missing fixture is a broken repository, not a property of this machine,
    so it must not become a skip: a skipped contract case would take one of the
    functions out of the gate while the suite still reported success.
    """
    if not path.is_file():
        pytest.fail(f"committed fixture is missing: {path}")
    return path.read_bytes()


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
def table_bytes() -> bytes:
    """A small document with one table."""
    return _read(HWPX_FIXTURES / "SimpleTable.hwpx")


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력 경로를 없는 파일로 바꾸기.
def picture_bytes() -> bytes:
    """A document with an embedded image, for the Markdown image path."""
    return _read(HWPX_FIXTURES / "green_swatch.hwpx")


@pytest.fixture(scope="session")
def sample_bytes() -> bytes:
    """Generated notes with a deliberately stale line cache."""
    return _read(HWPX_FIXTURES / "garden_notes.hwpx")


@pytest.fixture(scope="session")
def fields_bytes() -> bytes:
    """An encoder-generated document with one named, unfilled click-here field."""
    return _read(FIELD_FIXTURES / "clickhere_named.hwpx")


@pytest.fixture(scope="session")
def generated_bytes() -> bytes:
    """A document this library generated: three paragraphs and a table."""
    data, _report = _hwpforge.convert_md(GENERATED_MARKDOWN)
    return data


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력을 빈 바이트로 바꾸기.
def pdf_bytes() -> bytes:
    """A generated document with cached layout and an unavailable font."""
    return _read(HWPX_FIXTURES / "SimpleTable.hwpx")


@pytest.fixture(scope="session")
def pdf_font_dir() -> str:
    """The committed synthetic faces smithy-pdf renders its own tests with."""
    return str(PDF_TEST_FONTS)


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력을 빈 바이트로 바꾸기.
def checkable_list_bytes() -> bytes:
    """A document whose list items carry a checkbox, both ticked and not."""
    return _hwpforge.convert_md("- [x] Done\n- [ ] Pending\n")[0]


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력을 빈 바이트로 바꾸기.
def heading_and_numbered_bytes() -> bytes:
    """A document with a heading and a numbered list, for the other two variants."""
    return _read(REPO / "examples/showcase/lists/list_acceptance_visual/02_numbered_outline.hwpx")


@pytest.fixture(scope="session")
def stale_line_cache_bytes() -> bytes:
    """A document whose line layout cache the decoder has to drop, with a warning."""
    return _read(LAYOUT_FIXTURES / "stale-line-cache.hwpx")


@pytest.fixture(scope="session")
# 이것을 실패시키는 것: 생성 입력을 빈 바이트로 바꾸기.
def nested_table_bytes(generated_bytes: bytes) -> bytes:
    """A table nested inside another table's cell.

    `inspect`'s shallow (`top_level_*`) counts stop at the outer table, so
    this generated input makes the shallow and deep table
    counts of a section genuinely disagree.
    """
    import copy
    import json

    exported = _hwpforge.to_json(generated_bytes)["document"]
    paragraphs = exported["document"]["sections"][0]["paragraphs"]
    table = next(
        run["content"]["Table"]
        for para in paragraphs
        for run in para["runs"]
        if "Table" in run["content"]
    )
    nested = copy.deepcopy(table)
    table["rows"][0]["cells"][0]["paragraphs"][0]["runs"][0]["content"] = {"Table": nested}
    return _hwpforge.from_json(json.dumps(exported))[0]


@pytest.fixture(scope="session")
def stamp_placeholder_bytes() -> bytes:
    """A document with one unguarded class-A candidate per marker plus one
    guarded one (`※` instruction context), for the apply-phase counters."""
    return _read(STAMP_FIXTURES / "placeholder_basic.hwpx")


def all_paragraphs(data: bytes) -> list:
    """Every top-level paragraph of section 0, as `read` projects them.

    The range comes from `top_level_paragraphs` rather than the report's
    `paragraphs`, which counts deeply and includes the paragraphs inside table
    cells that `read` does not address.
    """
    count = _hwpforge.inspect(data)["section_details"][0]["top_level_paragraphs"]
    view = _hwpforge.read(data, section=0, paras=f"0..{count - 1}")["paragraphs"]
    assert view is not None
    return view["paragraphs"]


@pytest.fixture(scope="session")
def synthetic_face_bytes() -> bytes:
    """A document that renders anywhere, because its faces are the committed test fonts.

    The renderer resolves faces by their internal names and never substitutes.
    This one names `HwpForge Test`, which `pdf_font_dir` holds, and contains
    only characters that family draws — so it renders in the default fatal
    mode with no warnings at all.
    """
    return _read(SYNTHETIC_FACE)


@pytest.fixture(scope="session")
def document(table_bytes: bytes) -> hwpforge.Document:
    """`table_bytes` as a `Document`."""
    return hwpforge.Document.from_bytes(table_bytes)


@pytest.fixture(scope="session")
def stamp_request(generated_bytes: bytes) -> StampRequestV2:
    """A stamp request that ignores every candidate the document offers.

    Stamping refuses to run while any candidate is unclassified, so the request
    has to be built from the plan rather than written out by hand. Ignoring
    everything is the smallest request that is always valid for any document.
    """
    plan = _hwpforge.stamp_plan(generated_bytes)
    request: StampRequestV2 = {
        "schema_version": plan["schema_version"],
        "source_sha256": plan["source_sha256"],
        "text": [
            {
                "section": candidate["section"],
                "path": candidate["path"],
                "span": candidate["span"],
                "marker": candidate["marker"],
                "action": "ignore",
            }
            for candidate in plan["text"]
        ],
        "cells": [
            {"table": candidate["table"], "at": candidate["at"], "action": "ignore"}
            for candidate in plan["cells"]
        ],
    }
    return request


@pytest.fixture(scope="session")
def exported_json(table_bytes: bytes) -> str:
    """`table_bytes` exported as a whole-document JSON text."""
    return hwpforge.Document.from_bytes(table_bytes).to_json().text


@pytest.fixture(scope="session")
def section_json(table_bytes: bytes) -> str:
    """Section 0 of `table_bytes` exported as JSON, which `patch` takes back."""
    return hwpforge.Document.from_bytes(table_bytes).export_section(section=0).text


@pytest.fixture(scope="session")
def ffi_calls(
    table_bytes: bytes,
    picture_bytes: bytes,
    fields_bytes: bytes,
    generated_bytes: bytes,
    exported_json: str,
    section_json: str,
    stamp_request: StampRequestV2,
    synthetic_face_bytes: bytes,
    pdf_font_dir: str,
) -> dict:
    """The smallest valid call for each report contract.

    The value is `(args, kwargs)`. Keeping them in one table means the contract
    test can be a single parametrised case rather than 23 near-copies.
    """
    return {
        "convert_md": (("# 제목\n\n본문입니다.\n",), {}),
        "to_md": ((picture_bytes,), {"mode": "styled"}),
        "to_json": ((table_bytes,), {}),
        "export_section": ((table_bytes,), {"section": 0}),
        "from_json": ((exported_json,), {}),
        "patch": ((table_bytes,), {"section": 0, "patch": section_json}),
        "inspect": ((table_bytes,), {"styles": True}),
        "outline": ((table_bytes,), {}),
        "fields": ((table_bytes,), {}),
        "validate": ((table_bytes,), {}),
        "stamp_plan": ((table_bytes,), {}),
        "read": ((table_bytes,), {"section": 0, "paras": "0"}),
        "diff": ((table_bytes,), {"revised": picture_bytes}),
        "delete_para": ((generated_bytes,), {"section": 0, "indexes": [1]}),
        "insert_para": ((generated_bytes,), {"section": 0, "anchor": 0, "text": ["새 문단"]}),
        "fill": ((fields_bytes,), {"values": {"user_email": "soeburi@example.com"}}),
        "set_cell": ((generated_bytes,), {"table": 0, "at": "0,0", "text": "값"}),
        "stamp": ((generated_bytes,), {"request": stamp_request, "manifest": True}),
        "restyle": ((generated_bytes,), {"preset": "modern"}),
        "templates": ((), {}),
        "schema": ((), {"kind": "document"}),
        "to_pdf": ((synthetic_face_bytes,), {"font_dirs": [pdf_font_dir]}),
    }
