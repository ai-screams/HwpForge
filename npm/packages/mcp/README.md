# @hwpforge/mcp

> Anvil — HwpForge MCP Server for AI-native Korean HWP/HWPX document tools

## Install

```bash
npx -y @hwpforge/mcp
```

## What is HwpForge?

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats. This package provides the MCP (Model Context Protocol) server that enables AI agents to generate, inspect, and edit Korean documents.

## MCP Tools

| Tool                 | Description                     |
| -------------------- | ------------------------------- |
| `hwpforge_convert`   | Markdown to HWPX conversion     |
| `hwpforge_inspect`   | Inspect HWPX document structure |
| `hwpforge_to_json`   | Export HWPX to JSON for editing |
| `hwpforge_patch`     | Apply JSON patches to HWPX      |
| `hwpforge_templates` | List available style templates  |

## Configuration

### Claude Code

```bash
claude mcp add hwpforge -- npx -y @hwpforge/mcp
```

### Claude Desktop / Cursor / Windsurf

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

## Supported Platforms

| Platform            | Package                      |
| ------------------- | ---------------------------- |
| macOS Apple Silicon | `@hwpforge/mcp-darwin-arm64` |
| macOS Intel         | `@hwpforge/mcp-darwin-x64`   |
| Linux x64           | `@hwpforge/mcp-linux-x64`    |
| Linux ARM64         | `@hwpforge/mcp-linux-arm64`  |
| Windows x64         | `@hwpforge/mcp-win32-x64`    |

## Alternative Installation

If npm is not available, install via Cargo:

```bash
cargo install hwpforge-bindings-mcp
```

## Links

- [GitHub](https://github.com/ai-screams/HwpForge)
- [Documentation](https://ai-screams.github.io/HwpForge/)
- [crates.io](https://crates.io/crates/hwpforge-bindings-mcp)

## License

MIT OR Apache-2.0
