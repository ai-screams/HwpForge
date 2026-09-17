#!/usr/bin/env python3
"""Assert that every artifact in a directory carries the version we resolved.

A wheel whose filename says one version and whose `METADATA` says another is the
failure that PyPI cannot undo: filenames are never reusable, so a wrong version
burns that filename forever. This runs in every build job, before anything is
uploaded, and also enforces acceptance criterion A-8: the wheel declares no
runtime dependencies, because the pure Python layer uses only the standard
library.

Usage: check_dist.py <dist-dir> <expected-version> [--expect-wheels N]
"""

from __future__ import annotations

import argparse
import pathlib
import sys
import tarfile
import zipfile


def _fail(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _metadata_field(text: str, field: str) -> str | None:
    prefix = f"{field}: "
    for line in text.splitlines():
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
        if not line.strip():
            break  # headers end at the first blank line; the body follows
    return None


def check_wheel(path: pathlib.Path, expected: str) -> list[str]:
    problems: list[str] = []
    parts = path.name.split("-")
    if len(parts) < 5:
        return [f"{path.name}: not a wheel filename"]
    if parts[1] != expected:
        problems.append(f"{path.name}: filename says {parts[1]}, expected {expected}")
    with zipfile.ZipFile(path) as archive:
        names = [n for n in archive.namelist() if n.endswith(".dist-info/METADATA")]
        if len(names) != 1:
            return problems + [f"{path.name}: expected exactly one METADATA, found {len(names)}"]
        text = archive.read(names[0]).decode("utf-8")
    version = _metadata_field(text, "Version")
    if version != expected:
        problems.append(f"{path.name}: METADATA Version is {version!r}, expected {expected!r}")
    requires = [line for line in text.splitlines() if line.startswith("Requires-Dist:")]
    if requires:
        problems.append(f"{path.name}: {len(requires)} Requires-Dist lines, expected 0: {requires}")
    return problems


def check_sdist(path: pathlib.Path, expected: str) -> list[str]:
    problems: list[str] = []
    stem = path.name[: -len(".tar.gz")]
    if not stem.endswith(f"-{expected}"):
        problems.append(f"{path.name}: filename does not end in -{expected}")
    with tarfile.open(path, "r:gz") as archive:
        names = [n for n in archive.getnames() if n.count("/") == 1 and n.endswith("/PKG-INFO")]
        if len(names) != 1:
            return problems + [f"{path.name}: expected exactly one PKG-INFO, found {len(names)}"]
        member = archive.extractfile(names[0])
        if member is None:
            return problems + [f"{path.name}: PKG-INFO is not a regular file"]
        text = member.read().decode("utf-8")
    version = _metadata_field(text, "Version")
    if version != expected:
        problems.append(f"{path.name}: PKG-INFO Version is {version!r}, expected {expected!r}")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dist", type=pathlib.Path)
    parser.add_argument("expected", help="the PEP 440 version the tag resolved to")
    parser.add_argument("--expect-wheels", type=int, default=None, help="require exactly this many wheels")
    parser.add_argument("--expect-sdists", type=int, default=None, help="require exactly this many sdists")
    args = parser.parse_args(argv)

    wheels = sorted(args.dist.glob("*.whl"))
    sdists = sorted(args.dist.glob("*.tar.gz"))
    if not wheels and not sdists:
        _fail(f"{args.dist} contains neither a wheel nor an sdist")
        return 1

    problems: list[str] = []
    if args.expect_wheels is not None and len(wheels) != args.expect_wheels:
        problems.append(f"expected {args.expect_wheels} wheels, found {len(wheels)}")
    if args.expect_sdists is not None and len(sdists) != args.expect_sdists:
        problems.append(f"expected {args.expect_sdists} sdists, found {len(sdists)}")
    for wheel in wheels:
        problems.extend(check_wheel(wheel, args.expected))
    for sdist in sdists:
        problems.extend(check_sdist(sdist, args.expected))

    for name in [p.name for p in wheels + sdists]:
        print(f"checked {name}")
    for problem in problems:
        _fail(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
