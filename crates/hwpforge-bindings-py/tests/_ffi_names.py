"""The 23 extension-module functions, in one place.

`test_contract.py` and `test_stubs.py` both read this, so the list of names,
the return shape of each function and the report type each one produces are
stated exactly once.
"""

from __future__ import annotations

FFI_FUNCTIONS = (
    "convert_md",
    "to_md",
    "to_json",
    "export_section",
    "from_json",
    "patch",
    "inspect",
    "outline",
    "fields",
    "validate",
    "stamp_plan",
    "read",
    "diff",
    "delete_para",
    "insert_para",
    "fill",
    "set_cell",
    "stamp",
    "restyle",
    "templates",
    "schema",
    "convert_hwp5",
    "to_pdf",
)
"""Every function `hwpforge._hwpforge` exports, in the order of the FFI table."""

BYTES_AND_REPORT = "bytes"
TEXT_AND_REPORT = "text"
REPORT_ONLY = "report"

SHAPES = {
    "convert_md": BYTES_AND_REPORT,
    "to_md": TEXT_AND_REPORT,
    "to_json": REPORT_ONLY,
    "export_section": REPORT_ONLY,
    "from_json": BYTES_AND_REPORT,
    "patch": BYTES_AND_REPORT,
    "inspect": REPORT_ONLY,
    "outline": REPORT_ONLY,
    "fields": REPORT_ONLY,
    "validate": REPORT_ONLY,
    "stamp_plan": REPORT_ONLY,
    "read": REPORT_ONLY,
    "diff": REPORT_ONLY,
    "delete_para": BYTES_AND_REPORT,
    "insert_para": BYTES_AND_REPORT,
    "fill": BYTES_AND_REPORT,
    "set_cell": BYTES_AND_REPORT,
    "stamp": BYTES_AND_REPORT,
    "restyle": BYTES_AND_REPORT,
    "templates": REPORT_ONLY,
    "schema": REPORT_ONLY,
    "convert_hwp5": BYTES_AND_REPORT,
    "to_pdf": BYTES_AND_REPORT,
}
"""What each function returns: bytes and a report, text and a report, or a report alone."""

REPORTS = {
    "convert_md": "ConvertMdReport",
    "to_md": "ToMdReport",
    "to_json": "ToJsonReport",
    "export_section": "ExportSectionReport",
    "from_json": "EncodeReport",
    "patch": "PatchReport",
    "inspect": "InspectReport",
    "outline": "OutlineReport",
    "fields": "FieldsReport",
    "validate": "ValidateReport",
    "stamp_plan": "StampPlanReport",
    "read": "ReadReport",
    "diff": "DiffReport",
    "delete_para": "StructuralReport",
    "insert_para": "StructuralReport",
    "fill": "FillReport",
    "set_cell": "SetCellReport",
    "stamp": "StampReport",
    "restyle": "RestyleReport",
    "templates": "TemplatesReport",
    "schema": None,
    "convert_hwp5": "ConvertHwp5Report",
    "to_pdf": "ToPdfReport",
}
"""The stub's `TypedDict` for each report. `schema` returns a JSON Schema, whose
keys are the schema's own, so no key set is pinned for it."""

assert set(SHAPES) == set(FFI_FUNCTIONS) == set(REPORTS)
