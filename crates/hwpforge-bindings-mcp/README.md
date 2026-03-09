# hwpforge-mcp

MCP (Model Context Protocol) server for HwpForge — enables AI agents to create and edit Korean HWPX documents.

## Installation

```bash
cargo install hwpforge-mcp
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

| Tool                 | Description                          |
| -------------------- | ------------------------------------ |
| `hwpforge_convert`   | Markdown → HWPX document generation  |
| `hwpforge_inspect`   | HWPX document structure analysis     |
| `hwpforge_to_json`   | HWPX → JSON export (full or section) |
| `hwpforge_patch`     | Replace a section with edited JSON   |
| `hwpforge_templates` | List available style presets         |

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

# 3. Edit the JSON, then patch back
hwpforge_patch(base_path: "report.hwpx", section: 0, section_json_path: "section0.json", output_path: "report_edited.hwpx")
```

## Transport

stdio (JSON-RPC 2.0). All logging goes to stderr.

## License

MIT OR Apache-2.0
