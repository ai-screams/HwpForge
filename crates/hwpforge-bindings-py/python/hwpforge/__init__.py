r"""Read, edit and generate Korean HWP/HWPX documents.

The package is a thin layer over the HwpForge Rust library. Operations carry
the same names, options and error codes as the command line tool and the MCP
server, so what you learn in one place holds in the others.

Documents are immutable values: an editing method returns a new
[`Document`][hwpforge.Document] in a result object next to the operation's
report, and leaves the one it was called on alone.

Example:
    >>> import hwpforge
    >>> doc = hwpforge.convert_md("# 제목\\n\\n본문입니다.").document
    >>> doc.inspect()["paragraphs"] > 0
    True
"""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError as _PackageNotFoundError
from importlib.metadata import version as _version
from typing import TYPE_CHECKING

# The extension module resolves `hwpforge.errors.HwpForgeError` by name every
# time it raises, so that module has to be importable before any operation
# runs. Importing it first makes the dependency explicit rather than incidental.
from .errors import HwpForgeError

# isort: split

from . import _hwpforge
from .document import Document
from .results import BytesResult, DocumentResult, TextResult

if TYPE_CHECKING:
    import os
    from typing import Literal

    from ._hwpforge import (
        ConvertHwp5Report,
        ConvertMdReport,
        EncodeReport,
        SchemaReport,
        TemplatesReport,
    )

__all__ = [
    "BytesResult",
    "Document",
    "DocumentResult",
    "HwpForgeError",
    "TextResult",
    "__version__",
    "convert_hwp5",
    "convert_md",
    "from_json",
    "schema",
    "templates",
]

try:
    __version__ = _version("hwpforge")
except _PackageNotFoundError:  # pragma: no cover - only when run from a source tree
    __version__ = "0.0.0+unknown"


def convert_md(
    text: str,
    *,
    preset: str = "default",
    base_dir: str | os.PathLike[str] | None = None,
) -> DocumentResult[ConvertMdReport]:
    """Build a document from Markdown.

    Args:
        text: The Markdown source. A YAML frontmatter block sets the style.
        preset: The style preset to build with. [`templates`][hwpforge.templates]
            lists them.
        base_dir: The directory image paths are resolved against. Without it,
            images that name a file are dropped and reported. No path outside
            this directory is read.

    Returns:
        The document, and a report saying what became of each image and holding
        any warnings.

    Raises:
        HwpForgeError: If the Markdown cannot be parsed, or if the preset does
            not exist.
    """
    data, report = _hwpforge.convert_md(text, preset=preset, base_dir=base_dir)
    return DocumentResult(Document(data), report)


def from_json(text: str, *, base: Document | None = None) -> DocumentResult[EncodeReport]:
    """Build a document from the JSON that [`Document.to_json`][hwpforge.Document.to_json] exports.

    Args:
        text: The exported JSON.
        base: A document to take the style store and package parts from, for
            JSON that was exported without styles.

    Returns:
        The document, and a report holding any warnings encoding produced.
        Unlike an edit, generation is not fail-closed: a warning that meaning
        was lost comes back beside the bytes.

    Raises:
        HwpForgeError: If the JSON does not describe a document, or if it
            carries a grid address that no longer matches.
    """
    data, report = _hwpforge.from_json(text, base=None if base is None else base.to_bytes())
    return DocumentResult(Document(data), report)


def convert_hwp5(
    data: bytes, *, carry_layout_cache: bool = False
) -> DocumentResult[ConvertHwp5Report]:
    """Convert an old binary HWP5 document to HWPX.

    Args:
        data: The bytes of a `.hwp` file.
        carry_layout_cache: Carry the line layout cache across, so a renderer
            reproduces Hancom's own page breaks instead of laying the text out
            again.

    Returns:
        The converted document, and a report holding every warning from the
        conversion, including what HWP5 carries that HWPX cannot.

    Raises:
        HwpForgeError: If the bytes are not a readable HWP5 document.
    """
    out, report = _hwpforge.convert_hwp5(data, carry_layout_cache=carry_layout_cache)
    return DocumentResult(Document(out), report)


def templates() -> TemplatesReport:
    """List the style presets a document can be built or restyled with.

    Returns:
        One entry per preset, with its description, body font and page size.
    """
    return _hwpforge.templates()


def schema(
    *, kind: Literal["document", "exported-document", "exported-section"] = "document"
) -> SchemaReport:
    """Return the JSON Schema for one of the exchange formats.

    Args:
        kind: Which schema to return. ``"document"`` describes the document
            model, and the two ``"exported-*"`` kinds describe what
            [`Document.to_json`][hwpforge.Document.to_json] and
            [`Document.export_section`][hwpforge.Document.export_section] write.

    Returns:
        The schema, as a `dict`.
    """
    return _hwpforge.schema(kind=kind)


# The public surface is `__all__` plus the submodules. The names imported to
# build it are not part of it, so the ones that survive to runtime are unbound
# here: `os` and `Literal` are only ever annotations (postponed, so never
# evaluated), the metadata helpers are aliased private, and these two are done
# with their work by the time the module finishes executing.
del TYPE_CHECKING, annotations
