#!/usr/bin/env python3
"""Replace `dynamic = ["version"]` with a static version, for a Python-only tag.

Normally the wheel version comes from Cargo (`dynamic = ["version"]`, resolved by
maturin). A `py-vX.Y.Z.N` release publishes a Python version that no Cargo crate
carries, so the build needs a static one. This patches the file in place in the
runner's ephemeral checkout; the workflow restores the original afterwards and
proves the restore with `git diff --exit-code`. Nothing is ever committed.

It fails loudly rather than guessing: exactly one `dynamic` line must be present,
and the file must not already declare a static `version`.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

DYNAMIC_RE = re.compile(r'^dynamic\s*=\s*\[\s*"version"\s*\]\s*$', re.MULTILINE)
STATIC_RE = re.compile(r"^version\s*=", re.MULTILINE)


def pin(text: str, version: str) -> str:
    if STATIC_RE.search(text):
        raise SystemExit("::error::pyproject.toml already declares a static version")
    matches = DYNAMIC_RE.findall(text)
    if len(matches) != 1:
        raise SystemExit(f'::error::expected exactly one `dynamic = ["version"]` line, found {len(matches)}')
    return DYNAMIC_RE.sub(f'version = "{version}"', text, count=1)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pyproject", type=pathlib.Path)
    parser.add_argument("version", help="the PEP 440 version the tag resolved to")
    args = parser.parse_args(argv)

    original = args.pyproject.read_text(encoding="utf-8")
    args.pyproject.write_text(pin(original, args.version), encoding="utf-8")
    print(f"pinned {args.pyproject} to version {args.version}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
