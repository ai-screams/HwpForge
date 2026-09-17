"""The extension releases the GIL, so another thread runs while an operation does.

If a native call held the interpreter for its whole duration, no other thread
would be scheduled until it returned. The test puts the call in a worker and
counts in the main thread, and the counting window is bounded by two events the
worker sets around the call, so what is measured is progress *during* the call
rather than progress after it returned.

The pass criterion is a count, not a ratio of wall-clock times: a held
interpreter yields no ticks at all inside the window, while a released one
yields millions, so the threshold below sits far from both the noise and the
handful of ticks the microsecond gap between `started` and the call itself
could produce.
"""

from __future__ import annotations

import threading

import hwpforge
from hwpforge import Document

PARAGRAPHS = 20_000
"""Enough that one re-encode runs for tens of milliseconds."""

MINIMUM_TICKS = 1_000
"""Far below the millions a released interpreter yields, far above the gap's few."""


def _big_document() -> Document:
    """A document large enough that re-encoding it takes long enough to observe."""
    body = "\n\n".join(
        f"{n}번째 문단입니다. 충분히 긴 본문을 넣어 둔다." for n in range(PARAGRAPHS)
    )
    return hwpforge.convert_md(f"# 제목\n\n{body}\n").document


def test_the_main_thread_runs_while_a_native_call_is_in_flight() -> None:
    document = _big_document()
    started = threading.Event()
    finished = threading.Event()
    failure: list[BaseException] = []

    def work() -> None:
        started.set()
        try:
            document.restyle(preset="modern")
        except BaseException as error:
            failure.append(error)
        finally:
            finished.set()

    worker = threading.Thread(target=work)
    worker.start()
    assert started.wait(timeout=30), "the worker never began"

    ticks = 0
    while not finished.is_set():
        ticks += 1

    worker.join(timeout=60)
    assert not worker.is_alive(), "the worker did not finish"
    assert not failure, failure
    assert ticks > MINIMUM_TICKS, (
        f"the main thread advanced only {ticks} times while the re-encode was in flight, "
        "which is what holding the interpreter across the call would look like"
    )


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
