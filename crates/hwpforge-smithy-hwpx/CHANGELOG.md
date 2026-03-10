# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.4...hwpforge-smithy-hwpx-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.3...hwpforge-smithy-hwpx-v0.1.4) - 2026-03-09

### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication


### Documentation

- change Anvil emoji from ⚒️ to ⚙️ for better semantic match

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- update README with MCP server section, badges, and project stats


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.2...hwpforge-smithy-hwpx-v0.1.3) - 2026-03-09

### Added

- *(examples)* add hwpx_complete_guide to JSON round-trip

- *(examples)* add HWPX↔JSON round-trip examples

- *(examples)* reorganize examples and add 16 HWPX showcase files

- *(smithy-hwpx)* add gradient fill support for shapes


### Fixed

- *(smithy-hwpx)* fix JSON round-trip crash and improve codec fidelity

- *(encoder)* use DrawingML namespace for chart title

- *(encoder)* add pattern fill (hatchStyle) support and fix BACK_SLASH/SLASH swap

- *(encoder)* fix rotation encoding to match 한글 convention

- *(encoder)* encode flip in rotMatrix instead of scaMatrix

- *(encoder)* apply scaMatrix + transMatrix for shape flip rendering

- *(encoder)* add unique id to fieldBegin and fix table cellAddr for merged cells


## [0.1.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.1...hwpforge-smithy-hwpx-v0.1.2) - 2026-03-08

### Added

- *(cli)* implement Phase 6 AI-first CLI with 7 commands


### Documentation

- *(readme)* add CLI quick start section and update project stats


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.0...hwpforge-smithy-hwpx-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)

