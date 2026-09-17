"""Result objects for operations that produce both an output and a report.

Every operation that reports something returns one of these frozen dataclasses
rather than a bare value, so the report is never lost and never has to be
fetched separately. The type parameter is the report's ``TypedDict``, declared
in the extension module's stub.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Generic, TypeVar

if TYPE_CHECKING:
    from .document import Document

__all__ = ["BytesResult", "DocumentResult", "R", "TextResult"]

R = TypeVar("R")
"""The report a given operation produces."""


@dataclass(frozen=True)
class DocumentResult(Generic[R]):
    """A new document and the report of the edit that produced it.

    Attributes:
        document: The edited document. The document the method was called on is
            unchanged.
        report: What the operation did, including any warnings.
    """

    __slots__ = ("document", "report")

    document: Document
    report: R


@dataclass(frozen=True)
class TextResult(Generic[R]):
    """Exported text and the report of the export that produced it.

    Attributes:
        text: The exported Markdown or JSON.
        report: What the operation did, including any warnings.
    """

    __slots__ = ("report", "text")

    text: str
    report: R


@dataclass(frozen=True)
class BytesResult(Generic[R]):
    """Exported bytes and the report of the export that produced them.

    Attributes:
        data: The exported document, for example a PDF.
        report: What the operation did, including any warnings.
    """

    __slots__ = ("data", "report")

    data: bytes
    report: R
