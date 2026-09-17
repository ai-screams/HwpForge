# Type stub for the `hwpforge._hwpforge` extension module.
#
# Written by hand and checked against the running module by
# `tests/test_stubs.py` and `tests/test_contract.py`. Every `*Report` below is
# the `pythonize`d form of an ops `*Meta` struct, so its keys are that struct's
# serde field names. A field with `#[serde(skip_serializing_if = ...)]` is
# `NotRequired` here; a bare `Option<T>` is a key that is always present and
# may be `None`.
#
# This module is private. Its surface can change in any release.

import os
from collections.abc import Mapping, Sequence
from typing import Any, Literal, TypedDict

from typing_extensions import NotRequired, TypeAlias

# ── shared shapes ───────────────────────────────────────────────

# A JSON document whose shape is the exchange schema, not a fixed key set.
Json: TypeAlias = dict[str, Any]

class WarningInfo(TypedDict):
    code: str
    message: str
    hint: NotRequired[str]

# The second, narrower classification a render failure carries, reached as
# `HwpForgeError.cause`. Same keys as the command line prints under `cause`.
# `stage` is `"render"` today; `code` is the renderer's own SCREAMING_SNAKE
# spelling, for example `FONT_UNRESOLVED` or `NO_RENDERABLE_CACHE`. `kind` and
# `location` are omitted unless the failure carries them.
class PdfCause(TypedDict):
    stage: str
    code: str
    kind: NotRequired[str]
    location: NotRequired[str]

class GridCoord(TypedDict):
    row: int
    col: int

class ParaLocator(TypedDict):
    section: int
    para: int

class Span(TypedDict):
    start: int
    end: int

class RunLocator(TypedDict):
    paragraph: int
    run: int

class FieldInfo(TypedDict):
    name: str | None
    hint: str | None
    current: str
    section: int
    fillable: bool

# ── input shapes ────────────────────────────────────────────────

class _CellSpecRequired(TypedDict):
    table: int
    text: str

# One cell to write. Exactly one of `at`, `right_of`, `below` is given.
class CellSpec(_CellSpecRequired, total=False):
    at: GridCoord
    right_of: str
    below: str

# Either the string `"ignore"` or `{"field": {"name": ..., "hint": ...}}`.
StampAction: TypeAlias = str | dict[str, Any]

class StampSpec(TypedDict):
    section: int
    path: str
    span: Span
    marker: str
    action: StampAction

class _StampRequestV2Required(TypedDict):
    schema_version: int
    source_sha256: str

class StampRequestV2(_StampRequestV2Required, total=False):
    text: Sequence[StampSpec]
    cells: Sequence[dict[str, Any]]

StampRequest: TypeAlias = Sequence[StampSpec] | StampRequestV2

# ── report shapes ───────────────────────────────────────────────

class FilledField(TypedDict):
    name: str
    section: int
    previous: str

class FillReport(TypedDict):
    filled: list[FilledField]
    warnings: list[WarningInfo]

class SetCellResult(TypedDict):
    table: int
    requested: GridCoord
    anchor: GridCoord
    resolution: str
    cleared: bool

class SetCellReport(TypedDict):
    results: list[SetCellResult]
    warnings: list[WarningInfo]

class StructuralReport(TypedDict):
    inserted: int
    deleted: int
    warnings: list[WarningInfo]

class PatchReport(TypedDict):
    section: int
    warnings: list[WarningInfo]

# The V1 and V2 manifests share these keys; only `fields` differs inside.
class StampManifest(TypedDict):
    schema_version: int
    source_sha256: str
    output_sha256: str
    fields: list[dict[str, Any]]

class StampReport(TypedDict):
    manifest: NotRequired[StampManifest]
    warnings: list[WarningInfo]

class RestyleReport(TypedDict):
    preset: str
    warnings: list[WarningInfo]

class EncodeReport(TypedDict):
    warnings: list[WarningInfo]

# What became of one image reference in the Markdown source. Internally tagged
# on `kind`. Note the asymmetry, which is deliberate rather than an oversight:
# `kind` and `reason` are snake_case, while `format` keeps Core's own
# PascalCase spelling because it is already part of the `to_json` wire schema.
class EmbeddedAsset(TypedDict):
    kind: Literal["embedded"]
    occurrence: RunLocator
    key: str
    format: str

class DroppedAsset(TypedDict):
    kind: Literal["dropped"]
    occurrence: RunLocator
    reason: str

class RemoteAsset(TypedDict):
    kind: Literal["remote"]
    occurrence: RunLocator

AssetOutcome: TypeAlias = EmbeddedAsset | DroppedAsset | RemoteAsset

class ConvertMdReport(TypedDict):
    assets: list[AssetOutcome]
    warnings: list[WarningInfo]

class ConvertHwp5Report(TypedDict):
    warnings: list[WarningInfo]

class ToPdfReport(TypedDict):
    pages: int
    warnings: list[WarningInfo]

class ToMdReport(TypedDict):
    mode: str
    images: dict[str, bytes]
    warnings: list[WarningInfo]

class ToJsonReport(TypedDict):
    document: Json
    warnings: list[WarningInfo]

class ExportSectionReport(TypedDict):
    section: Json
    warnings: list[WarningInfo]

class InspectMetadata(TypedDict):
    title: str
    author: str
    subject: str
    description: str
    last_saved_by: str
    created: str | None
    modified: str | None
    keywords: list[str]

class InspectSection(TypedDict):
    index: int
    top_level_paragraphs: int
    paragraphs: int
    tables: int
    images: int
    charts: int
    has_header: bool
    has_footer: bool
    has_page_number: bool

class FontSummary(TypedDict):
    id: int
    face_name: str
    lang: str

class CharShapeSummary(TypedDict):
    id: int
    font_id: int
    size_pt: float
    bold: bool
    italic: bool
    color: str

class ParaShapeSummary(TypedDict):
    id: int
    alignment: str
    line_spacing: int

class InspectStyles(TypedDict):
    fonts: list[FontSummary]
    char_shapes: list[CharShapeSummary]
    para_shapes: list[ParaShapeSummary]

class InspectReport(TypedDict):
    metadata: InspectMetadata
    sections: int
    paragraphs: int
    tables: int
    images: int
    charts: int
    fields: list[str]
    section_details: list[InspectSection]
    styles: NotRequired[InspectStyles]
    warnings: list[WarningInfo]

class SectionOutline(TypedDict):
    section: int
    paragraphs: int
    tables: int
    images: int
    charts: int

class OutlineHeading(TypedDict):
    text: str
    level: int
    source: str
    at: ParaLocator

class OutlineTable(TypedDict):
    ordinal: int
    at: ParaLocator
    rows: int | None
    cols: int | None
    addressable: bool
    caption: str | None

class OutlineBookmark(TypedDict):
    name: str
    at: ParaLocator

class DocumentOutline(TypedDict):
    title: str | None
    sections: list[SectionOutline]
    headings: list[OutlineHeading]
    tables: list[OutlineTable]
    fields: list[FieldInfo]
    bookmarks: list[OutlineBookmark]

class OutlineReport(TypedDict):
    outline: DocumentOutline
    warnings: list[WarningInfo]

class ParagraphView(TypedDict):
    at: ParaLocator
    kind: str
    text: str
    contains: NotRequired[list[dict[str, Any]]]

ParagraphsView = TypedDict(
    "ParagraphsView",
    {"section": int, "from": int, "to": int, "paragraphs": list[ParagraphView]},
)

class CellView(TypedDict):
    row: int
    col: int
    row_span: int
    col_span: int
    text: str
    contains: NotRequired[list[dict[str, Any]]]

class TableView(TypedDict):
    ordinal: int
    at: ParaLocator
    rows: int
    cols: int
    cells: list[CellView]

class ReadReport(TypedDict):
    paragraphs: ParagraphsView | None
    table: TableView | None
    fields: list[FieldInfo] | None
    warnings: list[WarningInfo]

class FieldsReport(TypedDict):
    fields: list[FieldInfo]

class SemanticDiff(TypedDict):
    field_values: list[dict[str, Any]]
    cells: list[dict[str, Any]]
    paragraphs: list[dict[str, Any]]
    structure: list[dict[str, Any]]
    raw: list[dict[str, Any]]
    raw_dropped: int

class PackageDiff(TypedDict):
    added: list[str]
    removed: list[str]
    changed: list[str]

class DiffReport(TypedDict):
    identical: bool
    note: str
    semantic: SemanticDiff
    package: PackageDiff
    warnings: list[WarningInfo]

class StampCandidate(TypedDict):
    section: int
    path: str
    span: Span
    marker: str
    pattern: str
    guard: str | None

class LabelRef(TypedDict):
    direction: str
    at: GridCoord
    raw: str
    normalized: str
    guard: str | None
    duplicate_count: int

class CellStampCandidate(TypedDict):
    table: int
    section: int
    at: GridCoord
    labels: list[LabelRef]
    guarded: bool
    suggested_name: NotRequired[str]
    suggested_hint: NotRequired[str]

class SkippedTable(TypedDict):
    table: int
    path: str
    error: str

class StampPlanReport(TypedDict):
    schema_version: int
    source_sha256: str
    text: list[StampCandidate]
    cells: list[CellStampCandidate]
    skipped_tables: list[SkippedTable]
    warnings: list[WarningInfo]

class ValidateReport(TypedDict):
    ok: bool
    sections: int
    paragraphs: int
    errors: list[WarningInfo]
    warnings: list[WarningInfo]

class PresetInfo(TypedDict):
    name: str
    description: str
    font: str
    page_size: str

class TemplatesReport(TypedDict):
    presets: list[PresetInfo]

# A JSON Schema document. Its keys are the schema's own, so no key set is pinned.
SchemaReport: TypeAlias = Json

# ── the 23 operations ───────────────────────────────────────────

def convert_md(
    text: str, /, *, preset: str = ..., base_dir: str | os.PathLike[str] | None = ...
) -> tuple[bytes, ConvertMdReport]: ...
def to_md(
    data: bytes, /, *, mode: Literal["styled", "lossy", "lossless"] = ...
) -> tuple[str, ToMdReport]: ...
def to_json(data: bytes, /, *, styles: bool = ...) -> ToJsonReport: ...
def export_section(data: bytes, /, *, section: int, styles: bool = ...) -> ExportSectionReport: ...
def from_json(text: str, /, *, base: bytes | None = ...) -> tuple[bytes, EncodeReport]: ...
def patch(data: bytes, /, *, section: int, patch: str) -> tuple[bytes, PatchReport]: ...
def inspect(data: bytes, /, *, styles: bool = ...) -> InspectReport: ...
def outline(data: bytes, /) -> OutlineReport: ...
def fields(data: bytes, /) -> FieldsReport: ...
def validate(data: bytes, /) -> ValidateReport: ...
def stamp_plan(data: bytes, /) -> StampPlanReport: ...
def read(
    data: bytes,
    /,
    *,
    section: int | None = ...,
    paras: str | None = ...,
    table: int | None = ...,
    field: str | None = ...,
) -> ReadReport: ...
def diff(base: bytes, /, *, revised: bytes) -> DiffReport: ...
def delete_para(
    data: bytes, /, *, section: int, indexes: Sequence[int]
) -> tuple[bytes, StructuralReport]: ...
def insert_para(
    data: bytes, /, *, section: int, anchor: int, text: Sequence[str], before: bool = ...
) -> tuple[bytes, StructuralReport]: ...
def fill(data: bytes, /, *, values: Mapping[str, str]) -> tuple[bytes, FillReport]: ...

# `at` is zero-based `"row,col"` here; `CellSpec.at` is a `GridCoord` object.
def set_cell(
    data: bytes,
    /,
    *,
    table: int | None = ...,
    at: str | None = ...,
    right_of: str | None = ...,
    below: str | None = ...,
    text: str | None = ...,
    specs: Sequence[CellSpec] | None = ...,
) -> tuple[bytes, SetCellReport]: ...
def stamp(
    data: bytes, /, *, request: StampRequest, manifest: bool = ...
) -> tuple[bytes, StampReport]: ...
def restyle(data: bytes, /, *, preset: str) -> tuple[bytes, RestyleReport]: ...
def templates() -> TemplatesReport: ...
def schema(
    *, kind: Literal["document", "exported-document", "exported-section"] = ...
) -> SchemaReport: ...
def convert_hwp5(
    data: bytes, /, *, carry_layout_cache: bool = ...
) -> tuple[bytes, ConvertHwp5Report]: ...
def to_pdf(
    data: bytes,
    /,
    *,
    font_dirs: Sequence[str | os.PathLike[str]] = ...,
    discovery: Literal["explicit", "hancom", "platform"] = ...,
    degraded: bool = ...,
    partial_cache_reject: bool = ...,
) -> tuple[bytes, ToPdfReport]: ...
