#!/usr/bin/env python3
"""Resolve and validate the tag that drives `pypi-publish.yml`.

Two tag languages reach this script, and they mean different things:

* ``vX.Y.Z`` is a workspace release made by release-plz. ``X.Y.Z`` must equal the
  Cargo workspace version exactly; the Python wheel simply carries that version.
* ``py-v…`` is a Python-only release. The design (epic section 2.4, row "Python
  전용 수정 경로") allows exactly four forms: ``py-vX.Y.Z``, ``py-vX.Y.Z.postM``,
  ``py-vX.Y.Z.N`` and ``py-vX.Y.Z.N.postM``, with ``N >= 1`` and ``M >= 1``. The
  first three release components must equal the Cargo version, epoch/pre/dev/local
  segments are rejected, a fifth release component is rejected, and PEP 440
  normalization must return the tag byte for byte (so ``py-v0.16.04`` and
  ``py-v0.16.4-1`` are rejected rather than silently renamed).

Run ``--self-check`` to exercise the accept/reject table without a repository.
"""

from __future__ import annotations

import argparse
import os
import re
import sys

from packaging.version import InvalidVersion, Version

PY_PREFIX = "py-v"
WORKSPACE_VERSION_RE = re.compile(
    r"^\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"",
    re.MULTILINE | re.DOTALL,
)


class TagError(ValueError):
    """The tag is not one this workflow is allowed to publish."""


def cargo_version(manifest: str) -> str:
    """Read `[workspace.package] version` out of the root Cargo.toml text."""
    match = WORKSPACE_VERSION_RE.search(manifest)
    if not match:
        raise TagError("could not find [workspace.package] version in Cargo.toml")
    return match.group(1)


def _release_tuple(text: str) -> tuple[int, ...]:
    try:
        return tuple(int(part) for part in text.split("."))
    except ValueError as exc:  # pragma: no cover - guarded by the caller
        raise TagError(f"cargo version {text!r} is not numeric") from exc


def resolve_version(tag: str, cargo: str) -> tuple[str, bool]:
    """Return `(pep440 version, is_python_only)` or raise `TagError`."""
    if tag.startswith(PY_PREFIX):
        return _python_only_version(tag[len(PY_PREFIX) :], cargo), True
    if tag.startswith("v"):
        rest = tag[1:]
        if rest != cargo:
            raise TagError(
                f"workspace tag {tag!r} does not match the Cargo version {cargo!r}; "
                "version bumps are owned by release-plz"
            )
        return rest, False
    raise TagError(f"tag {tag!r} is neither a workspace tag (vX.Y.Z) nor a Python tag (py-v…)")


def _python_only_version(raw: str, cargo: str) -> str:
    try:
        version = Version(raw)
    except InvalidVersion as exc:
        raise TagError(f"{raw!r} is not a PEP 440 version: {exc}") from exc
    if str(version) != raw:
        raise TagError(
            f"{raw!r} is not in PEP 440 normal form (normalizes to {str(version)!r}); "
            "tag the normal form so the wheel filename and the tag agree"
        )
    if version.epoch:
        raise TagError(f"{raw!r} carries an epoch, which this workflow does not publish")
    for name, value in (("pre-release", version.pre), ("dev", version.dev), ("local", version.local)):
        if value is not None:
            raise TagError(f"{raw!r} carries a {name} segment, which this workflow does not publish")
    release = version.release
    if len(release) not in (3, 4):
        raise TagError(
            f"{raw!r} has {len(release)} release components; only X.Y.Z and X.Y.Z.N are allowed"
        )
    if release[:3] != _release_tuple(cargo):
        raise TagError(
            f"{raw!r} starts with {'.'.join(str(n) for n in release[:3])}, "
            f"which is not the current Cargo version {cargo!r}"
        )
    if len(release) == 4 and release[3] < 1:
        raise TagError(f"{raw!r} uses a fourth component of {release[3]}; a Python-only release is N >= 1")
    if version.post is not None and version.post < 1:
        raise TagError(f"{raw!r} uses .post{version.post}; a rebuild is M >= 1")
    return raw


def resolve_target(event_name: str, input_target: str, is_python_only: bool) -> str:
    """Production is reachable only from a release, a `py-v*` tag, or an explicit input."""
    if event_name == "release":
        return "pypi"
    if event_name == "push":
        if not is_python_only:
            raise TagError("only py-v* tag pushes publish; workspace tags publish via release: published")
        return "pypi"
    if event_name == "workflow_dispatch":
        target = input_target or "testpypi"
        if target not in ("testpypi", "pypi"):
            raise TagError(f"target {target!r} is neither testpypi nor pypi")
        return target
    raise TagError(f"event {event_name!r} does not select a publish target")


ACCEPT_REJECT_TABLE: tuple[tuple[str, str, bool], ...] = (
    ("v0.16.4", "0.16.4", True),
    ("v0.16.5", "0.16.4", False),
    ("v0.16.04", "0.16.4", False),
    ("py-v0.16.4", "0.16.4", True),
    ("py-v0.16.4.post1", "0.16.4", True),
    ("py-v0.16.4.1", "0.16.4", True),
    ("py-v0.16.4.1.post2", "0.16.4", True),
    ("py-v0.16.4.0", "0.16.4", False),
    ("py-v0.16.4.post0", "0.16.4", False),
    ("py-v0.16.4rc1", "0.16.4", False),
    ("py-v0.16.4.dev1", "0.16.4", False),
    ("py-v0.16.4+local", "0.16.4", False),
    ("py-v0.16.4.1.2", "0.16.4", False),
    ("py-v0.16.5.1", "0.16.4", False),
    ("py-v0.16.04", "0.16.4", False),
    ("py-v1!0.16.4", "0.16.4", False),
    ("0.16.4", "0.16.4", False),
)


def self_check() -> int:
    failures = 0
    for tag, cargo, should_accept in ACCEPT_REJECT_TABLE:
        try:
            version, python_only = resolve_version(tag, cargo)
            got, detail = True, f"version={version} python_only={python_only}"
        except TagError as exc:
            got, detail = False, str(exc)
        ok = got == should_accept
        failures += not ok
        verdict = "accept" if got else "reject"
        print(f"{'ok  ' if ok else 'FAIL'} {tag:<22} -> {verdict:<6} {detail}")
    print(f"\n{len(ACCEPT_REJECT_TABLE) - failures}/{len(ACCEPT_REJECT_TABLE)} cases as expected")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-check", action="store_true", help="run the accept/reject table and exit")
    parser.add_argument("--tag", help="the tag to validate")
    parser.add_argument("--cargo-toml", default="Cargo.toml", help="path to the workspace manifest")
    parser.add_argument("--event-name", default="", help="the GitHub event that triggered the run")
    parser.add_argument("--input-target", default="", help="the workflow_dispatch target input")
    args = parser.parse_args(argv)

    if args.self_check:
        return self_check()
    if not args.tag:
        parser.error("--tag is required unless --self-check is given")

    with open(args.cargo_toml, encoding="utf-8") as handle:
        cargo = cargo_version(handle.read())
    try:
        version, python_only = resolve_version(args.tag, cargo)
        target = resolve_target(args.event_name, args.input_target, python_only)
    except TagError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        return 1

    print(f"tag={args.tag} cargo={cargo} version={version} python_only={python_only} target={target}")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"tag={args.tag}\n")
            handle.write(f"version={version}\n")
            handle.write(f"is_python_only={str(python_only).lower()}\n")
            handle.write(f"target={target}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
