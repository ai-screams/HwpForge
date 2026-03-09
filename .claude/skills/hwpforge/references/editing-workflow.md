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
  "paragraphs": [
    {
      "runs": [
        {
          "type": "text",
          "text": "본문 텍스트입니다.",
          "char_shape": { ... }
        }
      ],
      "para_shape": { ... }
    }
  ],
  "style_store": { ... }
}
```

**Editable fields:**

- `runs[].char_shape.font` — change font
- `runs[].char_shape.size` — change font size (in HwpUnit, 1pt = 100)
- `paragraphs[].para_shape.alignment` — change alignment
- Add/remove entire paragraphs

- Add/remove runs within a paragraph

**Read-only fields (do not modify):**

- `style_store` — style registry (modify via presets instead)
- Internal IDs and indices

### 4. Patch — Write Changes Back

```bash
# Replace section 0
hwpforge patch document.hwpx --section 0 section0.json -o updated.hwpx

# With base file for image inheritance
hwpforge patch document.hwpx --section 0 section0.json --base original.hwpx -o updated.hwpx
```

Use `--base` when the original document contains images. The patch command inherits binary resources (images, OLE objects) from the base file.

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
for para in section["paragraphs"]:
    for run in para["runs"]:
        if run.get("text") and "기존 텍스트" in run["text"]:
            run["text"] = run["text"].replace("기존 텍스트", "새 텍스트")
```

### Add a new paragraph

```python
new_para = {
    "runs": [{"type": "text", "text": "추가할 내용입니다."}],
    "para_shape": section["paragraphs"][0]["para_shape"]  # copy existing style
}
section["paragraphs"].append(new_para)
```

### Delete a paragraph

```python
# Remove paragraph at index 2
del section["paragraphs"][2]
```

## Tips

- Always inspect before editing to understand the document structure
- Use `--section N` to minimize JSON size (token efficiency)
- Back up the original file before patching
- Use `--base` for documents with images to preserve binary resources
- Verify the output with `inspect` after patching
