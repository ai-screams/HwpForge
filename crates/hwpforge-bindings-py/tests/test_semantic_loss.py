"""An edit that re-encodes the document refuses to lose meaning.

`stamp`, `set_cell` and `restyle` rebuild the whole package, so an encode
warning that means something visible disappeared is an error rather than a note
in the report: they return no bytes at all and raise `ENCODE_SEMANTIC_LOSS`.
Generating a document is not fail-closed, because a generated document has no
original meaning to lose, so the same warning comes back beside the bytes.

The document that triggers it is a footnote whose body starts with a heading:
the encoder cannot emit the visible note number for it and says so with
`NOTE_HEAD_SKIPPED`. No committed fixture does this, so the test builds one
through the JSON exchange format, which is the only route Python has to the
document model.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

import hwpforge
from hwpforge import Document, HwpForgeError

if TYPE_CHECKING:
    from hwpforge._hwpforge import StampRequestV2


@pytest.fixture(scope="module")
def note_head_document() -> Document:
    """A document whose re-encode drops a footnote's visible number."""
    built = hwpforge.convert_md("본문입니다.\n")
    payload = json.loads(built.document.to_json().text)

    paragraph = payload["document"]["sections"][0]["paragraphs"][0]
    char_shape = paragraph["runs"][0]["char_shape_id"]
    note_body = {
        "runs": [{"content": {"Text": "제목 각주"}, "char_shape_id": char_shape}],
        "para_shape_id": paragraph["para_shape_id"],
        "heading_level": 1,
    }
    paragraph["runs"].append(
        {
            "content": {"Control": {"Footnote": {"inst_id": None, "paragraphs": [note_body]}}},
            "char_shape_id": char_shape,
        }
    )

    generated = hwpforge.from_json(json.dumps(payload, ensure_ascii=False))
    codes = [warning["code"] for warning in generated.report["warnings"]]
    assert "NOTE_HEAD_SKIPPED" in codes, f"the fixture must lose a note head, got {codes}"
    return generated.document


def test_generation_is_not_fail_closed(note_head_document: Document) -> None:
    """Building the document succeeded even though the warning was raised."""
    assert note_head_document.to_bytes()


def test_regenerating_edit_fails_closed(note_head_document: Document) -> None:
    with pytest.raises(HwpForgeError) as caught:
        note_head_document.restyle(preset="modern")

    assert caught.value.code == "ENCODE_SEMANTIC_LOSS"
    assert caught.value.message


def test_the_refusal_carries_the_warnings_that_caused_it(note_head_document: Document) -> None:
    """The refusal is not a bare code: it hands back what it refused over.

    `warnings` holds the semantic losses that made the encode fail closed, and
    `others` keeps the non-semantic warnings from the same encode rather than
    discarding them, so a caller can tell the two apart without re-running
    anything.
    """
    with pytest.raises(HwpForgeError) as caught:
        note_head_document.restyle(preset="modern")
    details = caught.value.details

    assert details is not None, "a semantic-loss refusal must say what was lost"
    assert set(details) == {"warnings", "others"}
    assert details["warnings"], "the losses that caused the refusal must be listed"
    assert "NOTE_HEAD_SKIPPED" in [warning["code"] for warning in details["warnings"]]
    assert isinstance(details["others"], list)
    for warning in details["warnings"] + details["others"]:
        assert set(warning) <= {"code", "message", "hint"}


def test_a_failure_that_lost_nothing_carries_no_details() -> None:
    """`details` is `None`, not an empty pair of lists, for every other failure."""
    with pytest.raises(HwpForgeError) as caught:
        Document.from_bytes(b"not a package").outline()

    assert caught.value.details is None


def test_stamping_fails_closed_on_the_same_document(note_head_document: Document) -> None:
    plan = note_head_document.stamp_plan()
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

    with pytest.raises(HwpForgeError) as caught:
        note_head_document.stamp(request)

    assert caught.value.code == "ENCODE_SEMANTIC_LOSS"
