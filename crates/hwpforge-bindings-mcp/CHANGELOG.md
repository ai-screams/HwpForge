# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.2.1...hwpforge-bindings-mcp-v0.3.0) - 2026-03-18

### Fixed

- *(bindings)* align section edit validation and warnings

- *(hwpx)* harden and unify preserving section workflows

- *(hwpx)* harden preserving section patch fidelity


### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.7...hwpforge-bindings-mcp-v0.2.0) - 2026-03-17

### Changed

- Align the MCP binding crate with the workspace-wide `0.2.0` release line.
- Adopt the `hwpforge-core` `0.2.0` table and image construction contract in the shipped command surface.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.6...hwpforge-bindings-mcp-v0.1.7) - 2026-03-12

### Added

- *(cli/mcp)* add to-md command for HWPX→Markdown conversion


### Documentation

- add metadata extraction guide and fix MCP inspect metadata gap


## [0.1.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.5...hwpforge-bindings-mcp-v0.1.6) - 2026-03-10

### Added

- *(mcp)* Phase 7c MCP Extended — 3 tools + 4 resources + 3 prompts


### Changed

- *(mcp)* extract shared I/O helpers and eliminate TOCTOU race condition


### Fixed

- *(mcp)* address quality review — font contract docs, extension guard, range format

- *(mcp)* apply PR review fixes — TOCTOU comment, tests, error handling

- *(mcp)* fix restyle index mismatch and convert font override bugs


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.3...hwpforge-bindings-mcp-v0.1.4) - 2026-03-09

### Added

- *(mcp)* implement 5 MCP tools (convert, inspect, to_json, patch, templates)

- *(mcp)* add hwpforge-bindings-mcp crate skeleton with rmcp


### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication


### Documentation

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- *(mcp)* add README with installation and platform setup guides


### Fixed

- *(mcp)* add workspace metadata and dep versions for crates.io publish

- *(mcp)* add missing #[tool_handler] macro for MCP tool discovery

- *(mcp)* fix duplicate step comments and CLI-style hint text

- *(mcp)* harden security and correctness from audit

- *(mcp)* address code review findings (P1-P3)
