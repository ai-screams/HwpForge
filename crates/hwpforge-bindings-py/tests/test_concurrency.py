"""The extension releases the GIL, so another thread runs while an operation does.

If a long operation held the GIL for its whole duration, a second thread would
not be scheduled until it finished. The test measures that it is: a counter
thread must make progress while a whole-document re-encode is in flight.
"""

from __future__ import annotations

import sys
import threading
import time

import hwpforge
from hwpforge import Document

PARAGRAPHS = 20_000
"""Enough that one re-encode runs for tens of milliseconds, many switch intervals."""


def _big_document() -> Document:
    """A document large enough that re-encoding it takes long enough to observe."""
    body = "\n\n".join(
        f"{n}번째 문단입니다. 충분히 긴 본문을 넣어 둔다." for n in range(PARAGRAPHS)
    )
    return hwpforge.convert_md(f"# 제목\n\n{body}\n").document


def test_another_thread_progresses_during_an_operation() -> None:
    document = _big_document()
    ticks = 0
    stop = threading.Event()

    def count() -> None:
        nonlocal ticks
        while not stop.is_set():
            ticks += 1

    counter = threading.Thread(target=count, daemon=True)
    counter.start()
    try:
        before = ticks
        started = time.perf_counter()
        document.restyle(preset="modern")
        elapsed = time.perf_counter() - started
        during = ticks - before
    finally:
        stop.set()
        counter.join(timeout=5)

    minimum = 5 * sys.getswitchinterval()
    assert elapsed > minimum, (
        f"the re-encode finished in {elapsed * 1000:.1f} ms, too fast to prove anything "
        f"against a {sys.getswitchinterval() * 1000:.1f} ms switch interval — raise PARAGRAPHS"
    )
    assert during > 0, "no other thread ran while the re-encode held the interpreter"


def test_the_same_document_can_be_read_from_several_threads() -> None:
    document = _big_document()
    results: list[int] = []
    errors: list[BaseException] = []

    def work() -> None:
        try:
            results.append(document.inspect()["paragraphs"])
        except BaseException as error:
            errors.append(error)

    threads = [threading.Thread(target=work) for _ in range(4)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(timeout=60)

    assert not errors, errors
    assert len(set(results)) == 1, f"the same document read differently per thread: {results}"
