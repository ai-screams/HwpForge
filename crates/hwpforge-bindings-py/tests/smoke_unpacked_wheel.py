"""Acceptance criterion A-1: an unpacked wheel imports and works without pip.

Someone who cannot install packages should be able to download the wheel, unzip
it, put the directory on `PYTHONPATH` and use the library. This script proves
that by running a round trip against a real document: open, outline, fill, save
and export back to JSON.

It has two modes, because two callers need it.

    python3 smoke_unpacked_wheel.py <document.hwpx>

runs the round trip in whatever interpreter it was started with, importing
`hwpforge` from wherever `PYTHONPATH` points. That is how CI calls it, inside a
slim container with the wheel unzipped to a directory and nothing installed.
The exit code is the gate.

    python3 smoke_unpacked_wheel.py

with no arguments does the whole thing locally: builds the wheel, unzips it
somewhere empty and re-runs itself in that environment. It uses the base
interpreter rather than the project's virtual environment, which already has
the package installed in editable mode and would satisfy the import from the
source tree instead of from the wheel. It also must not use `-I`, since
isolated mode ignores `PYTHONPATH`, the very thing being tested.

This file is deliberately outside the pytest suite: it must run where pytest is
not installed, so it imports nothing but the standard library and `hwpforge`,
and it works from any working directory.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRATE = HERE.parent
DEFAULT_FIXTURE = CRATE.parent.parent / "tests" / "fixtures" / "fields" / "clickhere_named.hwpx"
FILL_TEXT = "hanyul@example.com"


def round_trip(source: Path) -> int:
    """Open, outline, fill, save and re-export `source`, printing each step."""
    import hwpforge

    print(f"hwpforge {hwpforge.__version__} from {hwpforge.__file__}")

    document = hwpforge.Document.open(source)
    print(f"opened {source.name}: {len(document)} bytes")

    outline = document.outline()["outline"]
    print(f"outline: {len(outline['sections'])} section(s), {len(outline['fields'])} field(s)")

    fillable = [
        field["name"]
        for field in document.fields()["fields"]
        if field["fillable"] and field["name"]
    ]
    if not fillable:
        print(f"FAIL: {source.name} has no fillable field, so the round trip cannot fill one")
        return 1
    result = document.fill(dict.fromkeys(fillable, FILL_TEXT))
    print(f"filled {[field['name'] for field in result.report['filled']]}")

    with tempfile.TemporaryDirectory() as raw:
        target = Path(raw) / "filled.hwpx"
        result.document.save(target)
        print(f"saved {target.stat().st_size} bytes")

        exported = hwpforge.Document.open(target).to_json()

    sections = exported.report["document"]["document"]["sections"]
    print(f"exported JSON: {len(exported.text)} characters, {len(sections)} section(s)")

    if document.to_bytes() == result.document.to_bytes():
        print("FAIL: filling returned an unchanged document")
        return 1
    print("unpacked wheel OK")
    return 0


def base_interpreter() -> Path:
    """The interpreter this environment was built from, with nothing installed into it."""
    root = Path(sys.base_prefix)
    for candidate in (root / "bin" / "python3", root / "bin" / "python", root / "python.exe"):
        if candidate.is_file():
            return candidate
    raise SystemExit(f"no base interpreter found under {root}")


def build_and_run_locally() -> int:
    """Build a wheel, unpack it and run the round trip against only that."""
    if not DEFAULT_FIXTURE.is_file():
        raise SystemExit(f"fixture is missing: {DEFAULT_FIXTURE}")

    with tempfile.TemporaryDirectory() as raw:
        workspace = Path(raw)
        dist = workspace / "dist"
        subprocess.run(["uv", "build", "--wheel", "--out-dir", str(dist)], cwd=CRATE, check=True)
        wheels = sorted(dist.glob("*.whl"))
        if len(wheels) != 1:
            raise SystemExit(f"expected exactly one wheel in {dist}, found {wheels}")
        print(f"built {wheels[0].name}")

        unpacked = workspace / "unpacked"
        with zipfile.ZipFile(wheels[0]) as archive:
            archive.extractall(unpacked)

        finished = subprocess.run(
            [str(base_interpreter()), "-s", str(Path(__file__).resolve()), str(DEFAULT_FIXTURE)],
            cwd=workspace,
            env={"PYTHONPATH": str(unpacked), "PATH": "/usr/bin:/bin"},
            capture_output=True,
            text=True,
            check=False,
        )
        sys.stdout.write(finished.stdout)
        sys.stderr.write(finished.stderr)
        if finished.returncode == 0 and "site-packages" in finished.stdout:
            print("FAIL: the round trip imported an installed copy, not the unpacked wheel")
            return 1
        return finished.returncode


def main(argv: list[str]) -> int:
    """Run the round trip on `argv[0]`, or build and unpack a wheel first when given nothing."""
    if len(argv) > 1:
        raise SystemExit(f"usage: {Path(__file__).name} [document.hwpx]")
    if argv:
        source = Path(argv[0])
        if not source.is_file():
            raise SystemExit(f"no such document: {source}")
        return round_trip(source)
    return build_and_run_locally()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
