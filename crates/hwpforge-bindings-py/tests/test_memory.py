"""Editing a large document costs a bounded fraction of what holding it costs.

Every editing method returns a new document, so the original and the result are
both in memory when the method returns. That is a deliberate cost of the
immutable value model, and the risk it carries is that an operation quietly
copies the document several times over. This test bounds the marginal cost of
an edit against the document's own in-memory footprint, which is the only
denominator that means anything: the HWPX package is a zip, and a document of
20,000 short paragraphs compresses to about 0.1 MiB while occupying more than
a hundred times that once decoded.

Each measurement runs in its own subprocess because `ru_maxrss` is a
high-water mark for the whole process: two readings inside one test would
measure nothing once an earlier test had already peaked higher.

Measured on an Apple M-series machine, 2026-09-17: importing the package costs
about 23 MiB, holding the document adds about 108 MiB, and the edit adds about
31 MiB on top, which is 0.28 times the document. The factor below leaves room
for a slower allocator without leaving room for a second full copy.
"""

from __future__ import annotations

import subprocess
import sys

import pytest

pytestmark = pytest.mark.skipif(
    sys.platform == "win32",
    reason="peak resident memory is read through `resource`, which POSIX has and Windows does not",
)

PARAGRAPHS = 20_000
"""Enough paragraphs that the document dwarfs the interpreter's own footprint."""

FACTOR = 3.0
"""How many times the document's footprint an edit may add on top of it."""

_PROBE = """
import resource
import hwpforge

mode = "{mode}"
if mode != "baseline":
    body = "\\n\\n".join(f"{{n}}번째 문단입니다." for n in range({paragraphs}))
    document = hwpforge.convert_md(body + "\\n").document
    if mode == "edit":
        edited = document.restyle(preset="modern").document
        assert len(edited.to_bytes()) > 0
print(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
"""


def _peak_mib(mode: str) -> float:
    """Run one probe and return its peak resident memory in MiB."""
    script = _PROBE.format(mode=mode, paragraphs=PARAGRAPHS)
    finished = subprocess.run(
        [sys.executable, "-c", script], capture_output=True, text=True, check=True, timeout=900
    )
    ru_maxrss = int(finished.stdout.strip().splitlines()[-1])
    scale = 1024 * 1024 if sys.platform == "darwin" else 1024
    return ru_maxrss / scale


def test_an_edit_costs_a_bounded_fraction_of_the_document() -> None:
    baseline = _peak_mib("baseline")
    holding = _peak_mib("build")
    editing = _peak_mib("edit")

    document_cost = holding - baseline
    edit_cost = editing - holding

    assert document_cost > 0, f"the document must cost something: {holding} vs {baseline}"
    assert edit_cost <= FACTOR * document_cost, (
        f"editing added {edit_cost:.1f} MiB on top of a document that costs "
        f"{document_cost:.1f} MiB, more than the {FACTOR} times allowed"
    )
