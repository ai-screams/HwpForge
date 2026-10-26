#!/usr/bin/env python3
"""Binary documents saved by other applications are not committed; only encoder output .hwpx and text inputs are."""

from __future__ import annotations

import subprocess
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree

EXCEPTIONS = {
    f"crates/hwpforge/tests/data/{name}": "error-contract input"
    for name in (
        "corrupt-input.hwpx",
        "depth-group-32.hwpx",
        "depth-table-33.hwpx",
        "depth-table-42.hwpx",
        "depth-xml-broken.hwpx",
        "hwp5-dangling-table-border-fill.hwp",
    )
}


def check(
    root: Path, paths: list[str], exceptions: dict[str, str] = EXCEPTIONS
) -> list[str]:
    violations = []
    for path in sorted(exceptions):
        # 이것을 실패시키는 것: 추적 목록 또는 디스크에서 예외 입력 제거.
        if path not in paths or not (root / path).is_file():
            violations.append(f"{path}: stale exception ({exceptions[path]})")
    for path in sorted(paths):
        suffix = Path(path).suffix.lower()
        # 이것을 실패시키는 것: 텍스트 입력까지 검사하거나 예외를 거부하기.
        if suffix not in {".hwp", ".hwpx", ".pdf"} or path in exceptions:
            continue
        # 이것을 실패시키는 것: .hwp 또는 .pdf 허용.
        if suffix != ".hwpx":
            violations.append(f"{path}: binary document format is not permitted")
            continue
        try:
            with zipfile.ZipFile(root / path) as package:
                version = ElementTree.fromstring(package.read("version.xml"))
            # 이것을 실패시키는 것: 다른 appVersion 또는 누락된 appVersion 허용.
            if not version.get("appVersion", "").startswith("12.0.0.0"):
                violations.append(f"{path}: appVersion must start with 12.0.0.0")
        except Exception as exc:
            # 이것을 실패시키는 것: 읽을 수 없는 ZIP/version.xml 허용.
            violations.append(f"{path}: unreadable version.xml ({type(exc).__name__})")
    return violations


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    # NUL separation preserves whitespace and non-ASCII filenames.
    paths = (
        subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
        .decode("utf-8")
        .split("\0")[:-1]
    )
    violations = check(root, paths)
    print(__doc__)
    for violation in violations:
        print(violation)
    print(f"Fixture policy: {len(violations)} violation(s)")
    return int(bool(violations))


if __name__ == "__main__":
    sys.exit(main())
