---
name: hwpforge
description: "Generate, inspect, and edit Korean HWPX documents using HwpForge. Use when the user asks to create a Korean government document, proposal, report, official letter, convert markdown to HWP/HWPX, edit an existing HWPX file, inspect HWPX structure, or work with Korean document templates. Supports Markdown-to-HWPX conversion, JSON round-trip editing, style template application, and Korean document formatting scenarios."
license: MIT
compatibility: claude-code, openai-codex, cursor, windsurf, vscode-copilot
metadata:
  author: ai-screams
  version: "0.1.0"
allowed-tools: Bash Read Write
---

# HwpForge Skill

## Overview

HwpForge converts Markdown to HWPX (KS X 6101), the Korean national standard document format used in government proposals, official reports, and administrative documents. It supports bidirectional JSON editing, style template application, and structural inspection of existing HWPX files.

The CLI binary is `hwpforge`. All commands support `--json` for machine-readable output and structured error codes for AI agent integration.

## Available Commands

### convert — Markdown to HWPX

```bash
# File input
hwpforge convert input.md -o output.hwpx

# With preset (only 'default' available currently)
hwpforge convert input.md -o output.hwpx --preset default

# stdin input (use - as path)
echo "# Title" | hwpforge convert - -o out.hwpx

# Pipe from file
cat document.md | hwpforge convert - -o document.hwpx
```

Presets: `default`. See [templates.md](references/templates.md) for details. Additional presets (`government`, `report`, `official`) are planned.

### inspect — HWPX Structure Summary

```bash
# Human-readable summary
hwpforge inspect document.hwpx

# Include style registry
hwpforge inspect document.hwpx --styles

# JSON output for AI parsing
hwpforge inspect document.hwpx --json
```

### to-json — Export HWPX to JSON

```bash
# Export full document
hwpforge to-json document.hwpx -o doc.json

# Export single section (0-indexed)
hwpforge to-json document.hwpx --section 0 -o section0.json

# Pipe to stdout
hwpforge to-json document.hwpx
```

### patch — Replace Section in HWPX

```bash
# Replace section 0 with edited JSON
hwpforge patch document.hwpx --section 0 section.json -o updated.hwpx

# The first argument is the base HWPX file (preserves images and styles)
hwpforge patch original.hwpx --section 0 section.json -o updated.hwpx
```

### templates — List and Inspect Presets

```bash
# List all presets
hwpforge templates list

# JSON output
hwpforge templates list --json

# Show preset details
hwpforge templates show government
```

## Editing Workflow

Use JSON round-trip for surgical edits to existing HWPX files. This preserves images, styles, and binary content that Markdown conversion would lose.

1. **Inspect** — understand structure before editing

   ```bash
   hwpforge inspect document.hwpx --json
   ```

2. **Export** — extract the target section

   ```bash
   hwpforge to-json document.hwpx --section 0 -o section0.json
   ```

3. **Modify** — edit the JSON (AI or human)

   See [editing-workflow.md](references/editing-workflow.md) for the `ExportedSection` schema and editable fields.

4. **Patch** — write changes back

   ```bash
   hwpforge patch document.hwpx --section 0 section0.json -o updated.hwpx
   ```

5. **Verify** — confirm the result

   ```bash
   hwpforge inspect updated.hwpx
   ```

## Document Scenarios

Scenario reference files are in the `references/` directory:

| Scenario                          | File                                                    | Use When                                     |
| --------------------------------- | ------------------------------------------------------- | -------------------------------------------- |
| 정부 제안서 (Government Proposal) | [scenario-proposal.md](references/scenario-proposal.md) | RFP response, project bid, government tender |
| 보고서 (Report)                   | [scenario-report.md](references/scenario-report.md)     | Research report, progress report, analysis   |
| 공문서 (Official Document)        | [scenario-official.md](references/scenario-official.md) | Administrative correspondence, formal notice |

## Korean Markdown Best Practices

See [markdown-guide.md](references/markdown-guide.md) for:

- GFM table syntax for Korean content
- YAML frontmatter fields (`title`, `author`, `date`, `preset`)
- Image path conventions (relative paths, absolute paths)
- Horizontal rule (`---`) as page break signal
- Korean special character handling

## Agent Behavior Rules

### Output: No Raw JSON

Never show raw JSON output to the user during JSON round-trip workflows. Always present results as a summarized table, structure diagram, or concise description. Save intermediate JSON to temporary files for internal processing only.

### Edit: In-Place by Default

When the user asks to modify a specific HWPX file, overwrite the original file unless they explicitly specify a different output path. Set the `-o` flag to the same path as the input file.

```bash
# Default behavior: overwrite the original
hwpforge patch document.hwpx --section 0 modified.json -o document.hwpx

# Only create a new file when the user explicitly specifies a different output path
hwpforge patch document.hwpx --section 0 modified.json -o new_document.hwpx
```

## Error Handling

All commands return structured errors when `--json` is passed:

```json
{
  "error": {
    "code": "FILE_NOT_FOUND",
    "message": "Input file not found: input.md",
    "hint": "Check the file path and try again."
  }
}
```

Exit codes:

- `1` — user error (bad input, missing file, invalid format)
- `2` — internal error (encoding failure, corrupt HWPX)

Use `--json` in all AI agent workflows to parse errors programmatically.
