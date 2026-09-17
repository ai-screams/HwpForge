"""Editing a large document costs a bounded fraction of what holding it costs.

Every editing method returns a new document, so the original and the result are
both in memory when the method returns. That is a deliberate cost of the
immutable value model, and the risk it carries is that an operation quietly
copies the document several times over. This test bounds the marginal cost of
an edit against the document's own in-memory footprint, which is the only
denominator that means anything: the HWPX package is a zip, and a document of
20,000 short paragraphs compresses to about 0.1 MiB while occupying more than a
hundred times that once decoded.

The three readings are taken inside one freshly started interpreter, so the
high-water mark starts from a bare process and both deltas are attributable to
the work between the readings.

Reading that mark portably is the subtle part. On Linux `ru_maxrss` lives in
the kernel's `signal_struct`: `fork` seeds the child with the parent's current
resident size and `exec` does not reset it, so a subprocess of a pytest run
that has already touched hundreds of megabytes reports that figure as its own
peak and the document's cost vanishes into it. `/proc/self/status`'s `VmHWM`
lives in `mm_struct`, which `exec` replaces, so it measures only this process.
macOS has no `/proc` and does not fork here, so `ru_maxrss` is honest there and
is the fallback. The child says which one it used.

Units differ too: `ru_maxrss` is bytes on macOS and kibibytes elsewhere, and
`VmHWM` is kibibytes. The child normalises to bytes before reporting.
"""

from __future__ import annotations

import json
import subprocess
import sys

import pytest

pytestmark = pytest.mark.skipif(
    sys.platform == "win32",
    reason="peak resident memory is read through /proc or `resource`, which Windows has neither of",
)

PARAGRAPHS = 20_000
"""Enough paragraphs that the document dwarfs the interpreter's own footprint."""

FACTOR = 3.0
"""How many times the document's footprint an edit may add on top of it.

Measured on an Apple M-series machine: importing the package costs about 23
MiB, holding the document adds about 108 MiB, and the edit adds about 31 MiB
on top, which is 0.28 times the document. The bound leaves room for a slower
allocator without leaving room for a second full copy.
"""

PROBE = """
import json
import resource
import sys


def peak_bytes():
    try:
        with open("/proc/self/status", encoding="ascii") as handle:
            for line in handle:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1]) * 1024, "VmHWM"
    except OSError:
        pass
    scale = 1 if sys.platform == "darwin" else 1024
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * scale, "ru_maxrss"


import hwpforge

baseline, source = peak_bytes()

body = "\\n\\n".join(f"{n}번째 문단입니다." for n in range(PARAGRAPHS))
document = hwpforge.convert_md(body + "\\n").document
after_document, _ = peak_bytes()

edited = document.restyle(preset="modern").document
assert len(edited.to_bytes()) > 0
after_edit, _ = peak_bytes()

print(json.dumps({
    "baseline": baseline,
    "after_document": after_document,
    "after_edit": after_edit,
    "source": source,
    "package": len(document.to_bytes()),
}))
""".replace("PARAGRAPHS", str(PARAGRAPHS))


def _measure() -> dict:
    """Run the probe in a bare interpreter and return its three readings."""
    finished = subprocess.run(
        [sys.executable, "-I", "-c", PROBE],
        capture_output=True,
        text=True,
        check=True,
        timeout=900,
    )
    return json.loads(finished.stdout.strip().splitlines()[-1])


def test_an_edit_costs_a_bounded_fraction_of_the_document() -> None:
    measured = _measure()
    mib = 1024 * 1024

    document_cost = (measured["after_document"] - measured["baseline"]) / mib
    edit_cost = (measured["after_edit"] - measured["after_document"]) / mib

    assert document_cost > 0, (
        f"holding the document did not move the {measured['source']} high-water mark "
        f"({measured['after_document']} vs {measured['baseline']} bytes), so the "
        "measurement says nothing about what the edit cost"
    )
    assert edit_cost <= FACTOR * document_cost, (
        f"editing added {edit_cost:.1f} MiB on top of a document that costs "
        f"{document_cost:.1f} MiB, more than the {FACTOR} times allowed"
    )
