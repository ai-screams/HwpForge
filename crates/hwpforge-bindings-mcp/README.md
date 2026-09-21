# hwpforge-mcp

MCP (Model Context Protocol) server for HwpForge — enables AI agents to create and edit Korean HWPX documents.

## Installation

```bash
cargo install hwpforge-bindings-mcp
```

Or build from source:

```bash
cargo build --release -p hwpforge-bindings-mcp
# Binary: target/release/hwpforge-mcp
```

## Platform Setup

### Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "hwpforge-mcp"
    }
  }
}
```

### Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "hwpforge-mcp"
    }
  }
}
```

### VS Code Copilot

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "hwpforge": {
      "type": "stdio",
      "command": "hwpforge-mcp"
    }
  }
}
```

### Claude Code

Add to `.claude/settings.json`:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "hwpforge-mcp"
    }
  }
}
```

## Tools

| Tool                   | Description                                            |
| ---------------------- | ------------------------------------------------------ |
| `hwpforge_convert`     | Markdown → HWPX document generation                    |
| `hwpforge_inspect`     | HWPX document structure analysis                       |
| `hwpforge_to_json`     | HWPX → JSON export (full or section)                   |
| `hwpforge_from_json`   | Build an HWPX document directly from JSON              |
| `hwpforge_patch`       | Replace a section's paragraph text with edited JSON    |
| `hwpforge_outline`     | Navigation map: headings, tables, fields, bookmarks    |
| `hwpforge_diff`        | Compare two HWPX files (semantic + package channels)   |
| `hwpforge_delete_para` | Delete top-level paragraphs by index                   |
| `hwpforge_insert_para` | Insert a paragraph relative to an anchor               |
| `hwpforge_read`        | Read a targeted paragraph range, table, or field       |
| `hwpforge_fields`      | List named click-here fields (누름틀)                  |
| `hwpforge_fill`        | Fill named click-here fields by name → value           |
| `hwpforge_stamp_plan`  | Discover prose placeholder candidates for stamping     |
| `hwpforge_stamp`       | Promote placeholders to named click-here fields        |
| `hwpforge_set_cell`    | Edit table cells by logical grid address               |
| `hwpforge_validate`    | Validate HWPX structure and integrity                  |
| `hwpforge_restyle`     | Apply a different style preset to an existing document |
| `hwpforge_templates`   | List available style presets                           |
| `hwpforge_to_md`       | HWPX → Markdown conversion                             |

## Workflow Examples

### Create a document

```
hwpforge_convert(markdown: "report.md", output_path: "report.hwpx")
```

### Edit an existing document

```
# 1. Inspect structure
hwpforge_inspect(file_path: "report.hwpx")

# 2. Export section to JSON
hwpforge_to_json(file_path: "report.hwpx", section: 0, output_path: "section0.json")

# 3. Edit the paragraph text (keep the paragraph count and structure), then patch back
hwpforge_patch(base_path: "report.hwpx", section: 0, section_json_path: "section0.json", output_path: "report_edited.hwpx")
```

`hwpforge_patch` is text-only: it refuses a replacement whose semantic text slots differ in count or path. Add or remove paragraphs with `hwpforge_insert_para` / `hwpforge_delete_para`, change table cells with `hwpforge_set_cell`, and rebuild a restructured document with `hwpforge_from_json`.

## Transport

stdio (JSON-RPC 2.0). All logging goes to stderr.

## License

MIT OR Apache-2.0
