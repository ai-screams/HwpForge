#!/usr/bin/env python3
"""Documents saved by other applications are not committed; only encoder output .hwpx and text inputs are.

Each tracked path is judged once, in this order. An input listed in EXCEPTIONS is skipped. A .hwpx is rejected
unless its version.xml can be read and its appVersion starts with 12.0.0.0. Any other file is rejected when its
extension is .hwp, .pdf or .hml, or when it starts with the signature of an OLE compound file (.hwp, .doc, .xls,
.ppt), a ZIP package (.docx, .xlsx, .pptx, .odt), a PDF or an RTF file, whatever its name; the signature check
does not follow symlinks and does not count a file it cannot read. Documents saved in a text format such as
Word XML or flat ODF are not detected."""

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
# .hwp also covers HWP 3.x files, which are not OLE; .hml is Hancom's XML text format.
DOCUMENT_SUFFIXES = {".hwp", ".hwpx", ".pdf", ".hml"}
SIGNATURES = {
    b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1": "OLE compound file",
    b"PK\x03\x04": "ZIP package",
    b"PK\x05\x06": "ZIP package",
    b"PK\x07\x08": "ZIP package",
    b"%PDF-": "PDF",
    b"{\\rtf": "RTF",
}
HOW_TO_FIX = (
    "make the input reproducible from code in the repository, "
    "or list it in EXCEPTIONS in scripts/check_fixture_policy.py with the reason"
)


def signature(file: Path) -> str | None:
    # 이것을 실패시키는 것: 심볼릭 링크를 따라가 대상 내용으로 판정하기(git 은 링크만 저장한다).
    if file.is_symlink():
        return None
    try:
        with file.open("rb") as handle:
            head = handle.read(8)
    except OSError:
        # 이것을 실패시키는 것: 폴더(하위 모듈)나 디스크에 없는 추적 경로에서 예외로 멈추기.
        return None
    for magic, kind in SIGNATURES.items():
        # 이것을 실패시키는 것: 파일 맨 앞이 아닌 곳의 서명까지 문서로 보기.
        if head.startswith(magic):
            return kind
    return None


def check(
    root: Path, paths: list[str], exceptions: dict[str, str] = EXCEPTIONS
) -> list[str]:
    violations = []
    for path in sorted(exceptions):
        # 이것을 실패시키는 것: 추적 목록 또는 디스크에서 예외 입력 제거.
        if path not in paths or not (root / path).is_file():
            violations.append(f"{path}: stale exception ({exceptions[path]})")
    for path in sorted(paths):
        # 이것을 실패시키는 것: 예외를 거부하거나, 예외 건너뛰기를 확장자·서명 검사 뒤로 옮기기.
        if path in exceptions:
            continue
        suffix = Path(path).suffix.lower()
        if suffix != ".hwpx":
            kind = signature(root / path)
            # 이것을 실패시키는 것: .hwp·.pdf·.hml 허용, 또는 서명 검사를 빼서 이름만 바꾼 문서 허용.
            if suffix in DOCUMENT_SUFFIXES or kind:
                found = kind or f"{suffix} file"
                violations.append(
                    f"{path}: document format ({found}) is not permitted; {HOW_TO_FIX}"
                )
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
