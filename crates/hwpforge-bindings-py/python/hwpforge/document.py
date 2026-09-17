"""The immutable document value object and the operations that run on one."""

from __future__ import annotations

import json
import os
from collections.abc import Mapping, Sequence
from typing import TYPE_CHECKING, Any

from . import _hwpforge
from .results import BytesResult, DocumentResult, TextResult

if TYPE_CHECKING:
    from typing import Literal

    from ._hwpforge import (
        CellSpec,
        DiffReport,
        ExportSectionReport,
        FieldsReport,
        FillReport,
        InspectReport,
        OutlineReport,
        PatchReport,
        ReadReport,
        RestyleReport,
        SetCellReport,
        StampPlanReport,
        StampReport,
        StampRequest,
        StructuralReport,
        ToJsonReport,
        ToMdReport,
        ToPdfReport,
        ValidateReport,
    )

__all__ = ["Document"]


class Document:
    """An HWPX document held in memory.

    A document is an immutable value: two documents are equal when their bytes
    are equal, and every editing method returns a new document rather than
    changing this one.

    The format runs one way: an old binary HWP5 file can be read in through
    [`convert_hwp5`][hwpforge.convert_hwp5], but everything from there on is
    HWPX, and there is no path back from HWPX to HWP5.

    Example:
        >>> doc = Document.open("proposal.hwpx")  # doctest: +SKIP
        >>> doc = doc.fill({"applicant": "홍길동"}).document  # doctest: +SKIP
        >>> doc.save("filled.hwpx")  # doctest: +SKIP
    """

    __slots__ = ("_data",)

    def __init__(self, data: bytes) -> None:
        """Wrap HWPX bytes.

        Prefer [`Document.open`][hwpforge.Document.open] or
        [`Document.from_bytes`][hwpforge.Document.from_bytes], which say where
        the bytes came from.

        Args:
            data: The bytes of an HWPX package.

        Raises:
            TypeError: If `data` is not `bytes`.
        """
        if not isinstance(data, bytes):
            raise TypeError(f"Document takes bytes, not {type(data).__name__}")
        object.__setattr__(self, "_data", data)

    # ── constructors ────────────────────────────────────────────

    @classmethod
    def open(cls, path: str | os.PathLike[str]) -> Document:
        """Read an HWPX package from a file.

        Args:
            path: The file to read.

        Returns:
            The document the file holds. Nothing is decoded until an operation
            asks for it.

        Raises:
            OSError: If the file cannot be read.
        """
        with open(path, "rb") as handle:
            return cls(handle.read())

    @classmethod
    def from_bytes(cls, data: bytes) -> Document:
        """Take an HWPX package that is already in memory.

        Args:
            data: The bytes of an HWPX package.

        Returns:
            The document those bytes hold.
        """
        return cls(data)

    # ── the bytes back out ──────────────────────────────────────

    def to_bytes(self) -> bytes:
        """Return the document's bytes, which are always an HWPX package.

        Returns:
            The HWPX package, byte for byte as it is held.
        """
        return self._data

    def save(self, path: str | os.PathLike[str]) -> None:
        """Write the document to a file, always as an HWPX package.

        HwpForge does not write the old binary HWP5 format, whatever the
        document was read from, so name the file `.hwpx`.

        Args:
            path: The file to write. An existing file is replaced.

        Raises:
            OSError: If the file cannot be written.
        """
        with open(path, "wb") as handle:
            handle.write(self._data)

    # ── value semantics ─────────────────────────────────────────

    def __bytes__(self) -> bytes:
        """Return the document's bytes."""
        return self._data

    def __len__(self) -> int:
        """Return the size of the document in bytes."""
        return len(self._data)

    def __eq__(self, other: object) -> bool:
        """Compare two documents by their bytes."""
        if not isinstance(other, Document):
            return NotImplemented
        return self._data == other._data

    def __hash__(self) -> int:
        """Hash the document by its bytes."""
        return hash(self._data)

    def __repr__(self) -> str:
        """Render as ``Document(<n> bytes)``."""
        return f"Document({len(self._data)} bytes)"

    def __setattr__(self, name: str, value: object) -> None:
        """Refuse every attribute assignment: a document is immutable."""
        raise AttributeError(f"Document is immutable; cannot set {name!r}")

    def __delattr__(self, name: str) -> None:
        """Refuse every attribute deletion: a document is immutable."""
        raise AttributeError(f"Document is immutable; cannot delete {name!r}")

    # ── queries ─────────────────────────────────────────────────

    def inspect(self, *, styles: bool = False) -> InspectReport:
        """Count what the document contains.

        Args:
            styles: Also summarise the fonts and the character and paragraph
                shapes the document defines.

        Returns:
            The counts, the metadata, a summary per section and any warnings
            reading the document produced.
        """
        return _hwpforge.inspect(self._data, styles=styles)

    def outline(self) -> OutlineReport:
        """List the document's headings, tables, fields and bookmarks.

        Returns:
            The outline and any warnings reading the document produced.
        """
        return _hwpforge.outline(self._data)

    def fields(self) -> FieldsReport:
        """List the fillable fields the document defines.

        Returns:
            One entry per field, with its current text.
        """
        return _hwpforge.fields(self._data)

    def validate(self) -> ValidateReport:
        """Check that the document decodes into a valid model.

        A document that fails validation reports ``ok: False`` rather than
        raising; only a document that cannot be decoded at all is an error.

        Returns:
            Whether the document is valid, its section and paragraph counts,
            the validation errors and any warnings.
        """
        return _hwpforge.validate(self._data)

    def read(
        self,
        *,
        section: int | None = None,
        paras: str | None = None,
        table: int | None = None,
        field: str | None = None,
    ) -> ReadReport:
        """Read one part of the document.

        Exactly one target must be given: a paragraph range (`section` with
        `paras`), a table (`table`) or a field (`field`).

        Args:
            section: The section the paragraph range belongs to.
            paras: The paragraph range, for example ``"0..3"``.
            table: The ordinal of the table to read.
            field: The name of the field to read. Every field with that name
                comes back.

        Returns:
            The paragraphs, the table or the fields that were asked for. The
            three keys are always present and the ones not asked for are
            ``None``.

        Raises:
            HwpForgeError: If no target or more than one target was given, or
                if the target does not exist.
        """
        return _hwpforge.read(self._data, section=section, paras=paras, table=table, field=field)

    def diff(self, revised: Document) -> DiffReport:
        """Compare this document against a revised one.

        Args:
            revised: The document to compare against.

        Returns:
            Whether the two are identical, what changed semantically and which
            package entries differ.
        """
        return _hwpforge.diff(self._data, revised=revised._data)

    def stamp_plan(self) -> StampPlanReport:
        """Find the places a template could be stamped.

        Returns:
            The text and cell candidates, the tables that had to be skipped and
            the hash of the document the plan was built from.
        """
        return _hwpforge.stamp_plan(self._data)

    # ── exports ─────────────────────────────────────────────────

    def to_json(self, *, styles: bool = True) -> TextResult[ToJsonReport]:
        """Export the whole document as JSON.

        The export carries grid addresses for table cells, and
        [`from_json`][hwpforge.from_json] reads it back.

        Args:
            styles: Include the style store. Without it the JSON describes
                structure only.

        Returns:
            The JSON text, and a report holding the same document as a `dict`.
        """
        report: ToJsonReport = _hwpforge.to_json(self._data, styles=styles)
        return TextResult(_dumps(report["document"]), report)

    def export_section(
        self, *, section: int, styles: bool = True
    ) -> TextResult[ExportSectionReport]:
        """Export one section as JSON for editing.

        Args:
            section: The index of the section to export.
            styles: Include the style store.

        Returns:
            The JSON text of that section, and a report holding the same
            section as a `dict`.

        Raises:
            HwpForgeError: If the section does not exist.
        """
        report: ExportSectionReport = _hwpforge.export_section(
            self._data, section=section, styles=styles
        )
        return TextResult(_dumps(report["section"]), report)

    def to_md(
        self, *, mode: Literal["styled", "lossy", "lossless"] = "styled"
    ) -> TextResult[ToMdReport]:
        """Export the document as Markdown.

        Args:
            mode: How much to carry across. ``"styled"`` keeps the style
                frontmatter, ``"lossy"`` drops what Markdown cannot express and
                warns about it, ``"lossless"`` refuses to lose anything.

        Returns:
            The Markdown text, and a report holding the mode, the images the
            document referenced and any warnings.
        """
        text, report = _hwpforge.to_md(self._data, mode=mode)
        return TextResult(text, report)

    def to_pdf(
        self,
        *,
        font_dirs: Sequence[str | os.PathLike[str]] = (),
        discovery: Literal["explicit", "hancom", "platform"] = "explicit",
        degraded: bool = False,
        partial_cache_reject: bool = False,
    ) -> BytesResult[ToPdfReport]:
        """Render the document to PDF.

        Args:
            font_dirs: Directories to load fonts from. A bare string is
                refused rather than read as one directory per character.
            discovery: Where else to look for fonts. ``"explicit"`` looks only
                in `font_dirs`, which is the only deterministic choice.
            degraded: Substitute a fallback face instead of failing when a font
                the document names cannot be resolved.
            partial_cache_reject: Refuse to render when the document's layout
                cache is only partly present, rather than laying it out again.

        Returns:
            The PDF bytes, and a report holding the page count and any warnings
            from converting, decoding and rendering.

        Raises:
            HwpForgeError: If a font cannot be resolved, or if rendering fails.
        """
        data, report = _hwpforge.to_pdf(
            self._data,
            font_dirs=font_dirs,
            discovery=discovery,
            degraded=degraded,
            partial_cache_reject=partial_cache_reject,
        )
        return BytesResult(data, report)

    # ── edits ───────────────────────────────────────────────────

    def fill(self, values: Mapping[str, str]) -> DocumentResult[FillReport]:
        """Return a new document with the named fields filled; the receiver is unchanged.

        Args:
            values: The text to put in each field, by field name.

        Returns:
            The filled document, and a report listing what each field held
            before.

        Raises:
            HwpForgeError: If a named field does not exist or cannot be filled.
        """
        data, report = _hwpforge.fill(self._data, values=dict(values))
        return DocumentResult(Document(data), report)

    def set_cell(
        self,
        *,
        table: int | None = None,
        at: str | None = None,
        right_of: str | None = None,
        below: str | None = None,
        text: str | None = None,
        specs: Sequence[CellSpec] | None = None,
    ) -> DocumentResult[SetCellReport]:
        """Return a new document with table cells rewritten; the receiver is unchanged.

        Either name a single cell with `table` and one of `at`, `right_of` or
        `below`, or pass `specs` for several. The two forms are mutually
        exclusive.

        Args:
            table: The ordinal of the table to edit.
            at: The cell to write, as zero-based ``"row,col"``, for example
                ``"1,2"``.
            right_of: Write the cell to the right of the one holding this label.
            below: Write the cell below the one holding this label.
            text: The text to write into the named cell.
            specs: Several cells to write at once.

        Returns:
            The edited document, and a report saying which cell each write
            resolved to.

        Raises:
            HwpForgeError: If the target is missing, ambiguous, or names a cell
                that does not exist.
        """
        data, report = _hwpforge.set_cell(
            self._data,
            table=table,
            at=at,
            right_of=right_of,
            below=below,
            text=text,
            specs=specs,
        )
        return DocumentResult(Document(data), report)

    def patch(self, *, section: int, patch: str) -> DocumentResult[PatchReport]:
        """Return a new document with one section replaced; the receiver is unchanged.

        Args:
            section: The index of the section to replace.
            patch: The JSON of the replacement section, as
                [`export_section`][hwpforge.Document.export_section] produced it.

        Returns:
            The patched document, and a report naming the section that changed.

        Raises:
            HwpForgeError: If the JSON is not a valid section, or if it carries
                a grid address that no longer matches.
        """
        data, report = _hwpforge.patch(self._data, section=section, patch=patch)
        return DocumentResult(Document(data), report)

    def delete_para(
        self, *, section: int, indexes: Sequence[int]
    ) -> DocumentResult[StructuralReport]:
        """Return a new document with paragraphs removed; the receiver is unchanged.

        Args:
            section: The index of the section to edit.
            indexes: The paragraphs to delete, by index within the section.

        Returns:
            The edited document, and a report counting what was deleted.

        Raises:
            HwpForgeError: If an index is out of range, or if deleting would
                leave the section unreadable.
        """
        data, report = _hwpforge.delete_para(self._data, section=section, indexes=indexes)
        return DocumentResult(Document(data), report)

    def insert_para(
        self, *, section: int, anchor: int, text: str | Sequence[str], before: bool = False
    ) -> DocumentResult[StructuralReport]:
        """Return a new document with paragraphs inserted; the receiver is unchanged.

        Args:
            section: The index of the section to edit.
            anchor: The paragraph to insert next to, by index.
            text: One paragraph as a string, or one string per paragraph. A
                string is a single paragraph, never one paragraph per
                character.
            before: Insert above the anchor instead of below it.

        Returns:
            The edited document, and a report counting what was inserted.

        Raises:
            HwpForgeError: If the anchor is out of range.
        """
        data, report = _hwpforge.insert_para(
            self._data, section=section, anchor=anchor, text=text, before=before
        )
        return DocumentResult(Document(data), report)

    def stamp(self, request: StampRequest, *, manifest: bool = True) -> DocumentResult[StampReport]:
        """Return a new document with the template stamped; the receiver is unchanged.

        This operation re-encodes the whole document, so it refuses to produce
        bytes at all if encoding would lose meaning, and raises
        ``ENCODE_SEMANTIC_LOSS`` instead.

        Args:
            request: What to stamp where, either as a list of stamps or as the
                versioned request object [`stamp_plan`][hwpforge.Document.stamp_plan]
                describes.
            manifest: Include the manifest of what was stamped in the report.

        Returns:
            The stamped document, and a report holding the manifest and any
            warnings.

        Raises:
            HwpForgeError: If the request does not match the document, or if
                encoding would lose meaning.
        """
        data, report = _hwpforge.stamp(self._data, request=request, manifest=manifest)
        return DocumentResult(Document(data), report)

    def restyle(self, *, preset: str) -> DocumentResult[RestyleReport]:
        """Return a new document in a different house style; the receiver is unchanged.

        This operation re-encodes the whole document, so it refuses to produce
        bytes at all if encoding would lose meaning, and raises
        ``ENCODE_SEMANTIC_LOSS`` instead.

        Args:
            preset: The style preset to apply. [`templates`][hwpforge.templates]
                lists them.

        Returns:
            The restyled document, and a report naming the preset and holding
            any warnings.

        Raises:
            HwpForgeError: If the preset does not exist, or if encoding would
                lose meaning.
        """
        data, report = _hwpforge.restyle(self._data, preset=preset)
        return DocumentResult(Document(data), report)


def _dumps(value: Any) -> str:
    """Render an exported document or section as JSON text.

    Args:
        value: The `dict` the extension module returned.

    Returns:
        Compact JSON, with Korean text left as it is rather than escaped.
    """
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
