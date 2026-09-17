#!/usr/bin/env python3
"""Assert that the artifacts in a directory are exactly the ones we meant to build.

PyPI never lets a filename be reused, so a wheel that carries the wrong version,
the wrong platform tag or an unexpected dependency burns that filename forever.
This runs in every build job before anything is uploaded, and again in the
publish job over the merged set, where it also checks that the six artifacts are
six *different* platforms rather than the same one six times.

Checked per wheel: the filename parses as `hwpforge-<version>-cp39-abi3-<platform>`,
the platform tag matches the pattern the matrix expects, and `METADATA` agrees on
`Name` and `Version` and declares no `Requires-Dist` (acceptance criterion A-8).
Checked per sdist: the filename is `hwpforge-<version>.tar.gz` and `PKG-INFO`
agrees on `Name` and `Version`.
"""

from __future__ import annotations

import argparse
import fnmatch
import pathlib
import sys
import tarfile
import zipfile

PROJECT = "hwpforge"
PYTHON_TAG = "cp39"
ABI_TAG = "abi3"


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


def wheel_platform(path: pathlib.Path) -> str | None:
    """Return the platform tag of a wheel whose name has the shape we require."""
    parts = path.name[: -len(".whl")].split("-")
    if len(parts) != 5:
        return None
    return parts[4]


def check_wheel(path: pathlib.Path, expected: str, platform_pattern: str | None) -> list[str]:
    problems: list[str] = []
    parts = path.name[: -len(".whl")].split("-")
    if len(parts) != 5:
        return [
            f"{path.name}: expected name-version-{PYTHON_TAG}-{ABI_TAG}-platform, "
            f"found {len(parts)} filename components"
        ]
    name, version, python_tag, abi_tag, platform = parts
    if name != PROJECT:
        problems.append(f"{path.name}: distribution is {name!r}, expected {PROJECT!r}")
    if version != expected:
        problems.append(f"{path.name}: filename version is {version!r}, expected {expected!r}")
    if python_tag != PYTHON_TAG:
        problems.append(f"{path.name}: python tag is {python_tag!r}, expected {PYTHON_TAG!r}")
    if abi_tag != ABI_TAG:
        problems.append(f"{path.name}: ABI tag is {abi_tag!r}, expected {ABI_TAG!r}")
    if platform_pattern and not fnmatch.fnmatch(platform, platform_pattern):
        problems.append(f"{path.name}: platform tag {platform!r} does not match {platform_pattern!r}")

    with zipfile.ZipFile(path) as archive:
        names = [n for n in archive.namelist() if n.endswith(".dist-info/METADATA")]
        if len(names) != 1:
            return problems + [f"{path.name}: expected exactly one METADATA, found {len(names)}"]
        text = archive.read(names[0]).decode("utf-8")
    problems.extend(_check_core_metadata(path.name, text, expected, "METADATA"))
    return problems


def _check_core_metadata(label: str, text: str, expected: str, kind: str) -> list[str]:
    """Name, Version and the A-8 dependency-free rule, for both metadata kinds."""
    problems: list[str] = []
    name = _metadata_field(text, "Name")
    if name != PROJECT:
        problems.append(f"{label}: {kind} Name is {name!r}, expected {PROJECT!r}")
    version = _metadata_field(text, "Version")
    if version != expected:
        problems.append(f"{label}: {kind} Version is {version!r}, expected {expected!r}")
    # A-8 applies to the sdist too: a dependency declared there would install
    # from a source build even though no wheel carries it.
    requires = [line for line in text.splitlines() if line.startswith("Requires-Dist:")]
    if requires:
        problems.append(f"{label}: {kind} has {len(requires)} Requires-Dist lines, expected 0: {requires}")
    return problems


def check_sdist(path: pathlib.Path, expected: str) -> list[str]:
    problems: list[str] = []
    stem = path.name[: -len(".tar.gz")]
    if stem != f"{PROJECT}-{expected}":
        problems.append(f"{path.name}: expected {PROJECT}-{expected}.tar.gz")
    with tarfile.open(path, "r:gz") as archive:
        names = [n for n in archive.getnames() if n.count("/") == 1 and n.endswith("/PKG-INFO")]
        if len(names) != 1:
            return problems + [f"{path.name}: expected exactly one PKG-INFO, found {len(names)}"]
        member = archive.extractfile(names[0])
        if member is None:
            return problems + [f"{path.name}: PKG-INFO is not a regular file"]
        text = member.read().decode("utf-8")
    problems.extend(_check_core_metadata(path.name, text, expected, "PKG-INFO"))
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dist", type=pathlib.Path)
    parser.add_argument("expected", help="the PEP 440 version the tag resolved to")
    parser.add_argument("--expect-wheels", type=int, default=None, help="require exactly this many wheels")
    parser.add_argument("--expect-sdists", type=int, default=None, help="require exactly this many sdists")
    parser.add_argument("--platform-tag", default=None, help="fnmatch pattern every wheel tag must match")
    parser.add_argument(
        "--distinct-platforms",
        action="store_true",
        help="require every wheel to carry a different platform tag (the merged set)",
    )
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
        problems.extend(check_wheel(wheel, args.expected, args.platform_tag))
    for sdist in sdists:
        problems.extend(check_sdist(sdist, args.expected))

    if args.distinct_platforms:
        platforms = [wheel_platform(w) for w in wheels]
        known = [p for p in platforms if p]
        if len(set(known)) != len(known):
            problems.append(f"platform tags repeat across the merged set: {sorted(platforms)}")

    for path in wheels + sdists:
        detail = wheel_platform(path) if path.suffix == ".whl" else "sdist"
        print(f"checked {path.name} ({detail})")
    for problem in problems:
        _fail(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
