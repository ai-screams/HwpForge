"""Build every workspace example and run each one in an empty temporary folder.

Clippy compiles the examples but never runs them, so an example that builds
and then fails at run time (the encoder rejecting an undefined reference, an
input file that moved) goes unnoticed. This script runs every example cargo
builds, each in its own empty temporary folder with a copy of the shared
inputs, and fails when one exits non-zero. Examples that cannot run that way
are listed in SKIP with the reason; every other example, including new ones,
must exit 0.

Usage: python3 scripts/run_examples.py
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Same profile, feature set and exclusion as the nextest step, so the build
# reuses its compiled dependencies.
BUILD = [
    "cargo",
    "build",
    "--profile",
    "test",
    "--workspace",
    "--examples",
    "--all-features",
    "--exclude",
    "hwpforge-bindings-py",
    "--message-format=json-render-diagnostics",
]

# Examples that cannot run with no arguments in an empty folder.
SKIP = {
    "audit_batch": "needs a document folder argument",
    "census_layout_carry": "needs a corpus folder argument",
    "roundtrip_save": "needs input and output path arguments",
    "merged_cells": "writes a test fixture; tests/generated_inputs.rs checks its output",
    "page_and_picture": "writes test fixtures; tests/generated_inputs.rs checks their output",
    "hwpx_json_roundtrip": "reads its inputs from examples/*.hwpx, which no longer exist",
    "hwpx_md_convert": "reads its inputs from examples/*.hwpx, which no longer exist",
}

# Repository folders that examples read relative to the working directory,
# copied so that an example cannot change the originals. Without the copy, the
# guide and report examples that read assets/mascot-main.png fail.
SHARED_INPUTS = ["assets"]


def build_examples() -> dict[str, str]:
    """Build the examples and return each example name with its executable."""
    result = subprocess.run(BUILD, cwd=REPO, stdout=subprocess.PIPE, text=True, check=False)
    if result.returncode != 0:
        sys.exit(f"cargo build failed with exit code {result.returncode}")
    return example_executables(result.stdout.splitlines())


def example_executables(lines: list[str]) -> dict[str, str]:
    """Collect example executables from cargo's JSON messages.

    Lines that are not a JSON object (a proc macro printing to stdout) are
    passed through. Two examples with the same name in different packages stop
    the run, because SKIP and the report identify examples by name and cargo
    may link both to the same path.
    """
    executables: dict[str, str] = {}
    packages: dict[str, str] = {}
    for line in lines:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            message = None
        if not isinstance(message, dict):
            print(line)
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        target = message["target"]
        # cargo always sends the field; it is null only when there is no binary.
        if "example" not in target["kind"] or message["executable"] is None:
            continue
        name = target["name"]
        if packages.setdefault(name, message["package_id"]) != message["package_id"]:
            sys.exit(f"two examples are named {name}; rename one")
        executables[name] = message["executable"]
    return executables


def run_example(executable: str) -> subprocess.CompletedProcess[str]:
    """Run one example in a fresh temporary folder with a copy of the shared inputs."""
    with tempfile.TemporaryDirectory(prefix="hwpforge-example-") as work:
        for name in SHARED_INPUTS:
            shutil.copytree(REPO / name, Path(work) / name)
        return subprocess.run(
            [executable],
            cwd=work,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )


def main() -> int:
    executables = build_examples()
    # A SKIP entry left behind after its example was renamed or removed fails
    # here, so the list only names examples that exist.
    unknown = sorted(set(SKIP) - set(executables))
    if unknown:
        print(f"SKIP names examples that do not exist: {', '.join(unknown)}", file=sys.stderr)
        return 1

    failed = []
    for name in sorted(executables):
        if name in SKIP:
            print(f"skip  {name}: {SKIP[name]}")
            continue
        result = run_example(executables[name])
        if result.returncode == 0:
            print(f"ok    {name}")
        else:
            print(f"FAIL  {name} (exit code {result.returncode})")
            print(result.stdout.rstrip())
            print(result.stderr.rstrip())
            failed.append(name)

    ran = len(executables) - len(SKIP)
    print(f"{ran} examples ran, {len(SKIP)} skipped, {len(failed)} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
