"""Reject guidance that incorrectly excludes Python from automatic conversion."""

from __future__ import annotations

import re

import pytest
from conftest import REPO


@pytest.mark.parametrize(
    "path",
    [
        "crates/hwpforge-bindings-py/python/hwpforge/errors.py",
        "skills/hwpforge/references/errors.md",
        "docs/reference/cli.md",
        "docs/guide/python/editing.md",
        "crates/hwpforge-bindings-py/README.md",
        "skills/hwpforge/references/python.md",
    ],
)
def test_guidance_does_not_deny_python_conversion(path):
    # 이것을 실패시키는 것: Python 자동 입력 변환이 비지원이라는 안내 재도입.
    text = " ".join((REPO / path).read_text().split())
    assert not re.search(
        r"automatic python input conversion is not enabled"
        r"|(?:MCP and )?Python(?:/MCP)? (?:do|does) not automatically convert"
        r"|Python(?:/MCP)?\s+입력은 자동 변환 대상이 아닙니다",
        text,
        flags=re.I,
    )
