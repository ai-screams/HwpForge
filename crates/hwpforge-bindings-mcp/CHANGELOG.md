# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

