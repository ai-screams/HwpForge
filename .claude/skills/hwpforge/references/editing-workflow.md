# JSON Round-Trip Editing Workflow

## When to Use

Use JSON round-trip editing (NOT Markdown conversion) when:

- Preserving images, charts, and embedded objects
- Surgical edits to specific sections
- Maintaining exact style and formatting

Use Markdown conversion when:

- Creating a new document from scratch
- Full content replacement is acceptable

## Step-by-Step Workflow

### 1. Inspect — Understand Structure

```bash
hwpforge inspect document.hwpx --json
```

Returns section count, paragraph counts, table/image/chart locations, and style info.

### 2. Export — Extract Target Section

```bash
# Export specific section (0-indexed)
hwpforge to-json document.hwpx --section 0 -o section0.json

# Export full document
hwpforge to-json document.hwpx -o full.json

# Without styles (smaller JSON)
hwpforge to-json document.hwpx --section 0 --no-styles -o section0.json
```

### 3. Modify — Edit the JSON

The exported JSON follows the `ExportedSection` schema:

```json
{
  "section_index": 0,
  "section": {
    "paragraphs": [
      {
        "runs": [
          {
            "content": { "Text": "본문 텍스트입니다." },
            "char_shape_id": 0
          }
        ],
        "para_shape_id": 0,
        "column_break": false,
        "page_break": false,
        "style_id": 0
      }
    ]
  },
  "styles": { ... }
}
```

**Editable fields:**

- `section.paragraphs[].runs[].content` — change text (e.g., `{"Text": "new text"}`)
- `section.paragraphs[].runs[].char_shape_id` — reference a different char shape
- `section.paragraphs[].para_shape_id` — reference a different paragraph shape
- Add/remove entire paragraphs in `section.paragraphs`
- Add/remove runs within a paragraph

**Read-only fields (do not modify):**

- `styles` — style registry (modify via presets instead)
- `section_index` — must match the `--section` argument in patch
- `style_id`, `char_shape_id`, `para_shape_id` — only change to existing valid IDs

### 4. Patch — Write Changes Back

```bash
# Replace section 0
hwpforge patch document.hwpx --section 0 section0.json -o updated.hwpx

# The first argument (base HWPX) provides image/style inheritance
hwpforge patch original.hwpx --section 0 section0.json -o updated.hwpx
```

The first positional argument is the base HWPX file. The patch command inherits binary resources (images, OLE objects) from it.

### 5. Verify — Confirm Result

```bash
hwpforge inspect updated.hwpx --json
```

Compare section counts, paragraph counts, and table/image counts with the original.

## JSON Schema

Get the full schema for programmatic validation:

```bash
# ExportedDocument schema (full document)
hwpforge schema exported-document

# ExportedSection schema (single section)
hwpforge schema exported-section
```

## Common Edit Patterns

### Replace text in a paragraph

```python
# Find paragraph by text content
for para in data["section"]["paragraphs"]:
    for run in para["runs"]:
        content = run.get("content", {})
        if "Text" in content and "기존 텍스트" in content["Text"]:
            run["content"]["Text"] = content["Text"].replace("기존 텍스트", "새 텍스트")
```

### Add a new paragraph

```python
existing = data["section"]["paragraphs"][0]
new_para = {
    "runs": [{"content": {"Text": "추가할 내용입니다."}}],
    "para_shape": existing["para_shape"]  # copy existing style
}
data["section"]["paragraphs"].append(new_para)
```

### Delete a paragraph

```python
# Remove paragraph at index 2
del data["section"]["paragraphs"][2]
```

## Tips

- Always inspect before editing to understand the document structure
- Use `--section N` to minimize JSON size (token efficiency)
- Back up the original file before patching
- The first positional argument (base HWPX) preserves binary resources (images)
- Verify the output with `inspect` after patching
