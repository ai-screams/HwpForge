# HwpForge

> **한글 문서(HWP/HWPX)를 프로그래밍으로 제어하는 Rust 라이브러리**
>
> LLM-first design | Rust Core + Python Wrapper | YAML Style Templates

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

---

## 🚧 Status: Under Development (v0.1.0)

프로젝트 세팅이 완료되었습니다. v1.0 구현 진행 중.

**v1.0 목표** (5개월):
- HWPX T1~T3 Full (텍스트, 표, 이미지) — 읽기/쓰기
- HWP5 T1~T2 Basic (텍스트, 표) — 읽기 전용
- Style Template System (YAML)
- Markdown ↔ HWPX 양방향 변환
- MCP Server (Claude Code 통합)

---

## Architecture

\`\`\`
Layer 3: LLM Interface (MCP Server, Python API, CLI)
Layer 2: Markdown Bridge (MD ↔ DOM)
Layer 1: Style Template System (YAML)
Layer 0: Core Engine (HWPX R/W, HWP5 Reader, IR)
\`\`\`

---

## Development

### Prerequisites

- Rust 1.75+
- Python 3.8+ (for Python bindings)
- pre-commit (optional, recommended)

### Setup

\`\`\`bash
# Install development tools
make install-tools

# Build
cargo build

# Test
make test

# Lint
make clippy

# Format
make fmt

# Watch mode (bacon)
bacon
\`\`\`

### Project Structure

\`\`\`
HwpForge/
├── crates/
│   ├── hwpforge-foundation/       # 🔩 Primitives (HwpUnit, Color, ID)
│   ├── hwpforge-core/             # 🔨 Pure document structure (style refs only)
│   ├── hwpforge-blueprint/        # 📐 Style templates (YAML, like Figma tokens)
│   ├── hwpforge-smithy-hwpx/      # 🔥 HWPX smithy (encoder/decoder)
│   ├── hwpforge-smithy-hwp5/      # 🔥 HWP5 smithy (decoder)
│   ├── hwpforge-smithy-md/        # 🔥 Markdown smithy (encoder/decoder)
│   ├── hwpforge-bindings-py/      # 🐍 PyO3 Python bindings
│   └── hwpforge-bindings-cli/     # ⚒️  CLI tool
├── python/hwpforge/               # Python package + MCP Server
├── templates/                     # Built-in blueprints (style templates)
└── tests/fixtures/                # Test files
\`\`\`

**Forge Philosophy**:
- **Foundation** = Raw materials (primitives)
- **Core** = Anvil (pure structure, no style)
- **Blueprint** = Design patterns (style definitions, reusable)
- **Smithy** = Workshops (format-specific forges)
- **Bindings** = Tools (interfaces for users)

Like HTML+CSS or React+Theme, enabling style reuse across documents.

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

---

## Acknowledgments

본 제품은 한글과컴퓨터의 한/글 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.
