# HwpForge

> **한글 문서(HWP/HWPX)를 프로그래밍으로 제어하는 Rust 라이브러리**
>
> LLM-first design | Rust Core + Python Wrapper | YAML Style Templates

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

---

## 🚧 Status: Under Development (v0.1.0)

**Phase 0-4 완료** (Foundation → Core → Blueprint → HWPX Codec)

- ✅ Foundation: HwpUnit, Color (BGR), Index<T>, ErrorCode
- ✅ Core: Document<Draft/Validated>, Paragraph, Run, Table
- ✅ Blueprint: YAML Template System, StyleRegistry
- ✅ Smithy-HWPX: Full Encoder/Decoder with roundtrip (5 golden tests)

**Stats**: ~18,600 LOC, 767 tests, 0 clippy warnings

**Next**: Phase 5 (smithy-md), Phase 6 (bindings), Phase 7 (MCP), Phase 8 (v1.0 release)

---

## Architecture

```
Foundation (🔩 primitives)
  → Core (🔨 pure structure, style refs only)
  → Blueprint (📐 YAML templates, centralized)
  → Smithy (🔥 format compilers: HWPX, HWP5, MD)
  → Bindings (🐍⚒️ Python/CLI interfaces)
```

**Key Principle**: Structure and Style are separate (like HTML + CSS).

---

## Quick Start

### Prerequisites

- Rust 1.93+ (default development toolchain)
- Rust 1.75 (MSRV validation target in CI)
- (Optional) Python 3.8+ for bindings
- (Optional) pre-commit for hooks

### Build & Test

```bash
# Build
cargo build

# Test
cargo test --all-features

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt --all -- --check

# Watch mode
bacon
```

### Project Structure

```
HwpForge/
├── crates/
│   ├── hwpforge-foundation/       # 🔩 Primitives (HwpUnit, Color, Index<T>)
│   ├── hwpforge-core/             # 🔨 Document structure (style refs only)
│   ├── hwpforge-blueprint/        # 📐 YAML templates (Figma-like)
│   ├── hwpforge-smithy-hwpx/      # 🔥 HWPX codec (ZIP+XML ↔ Core)
│   ├── hwpforge-smithy-hwp5/      # 🔥 HWP5 decoder (binary ↔ Core)
│   ├── hwpforge-smithy-md/        # 🔥 Markdown codec (MD ↔ Core)
│   ├── hwpforge-bindings-py/      # 🐍 PyO3 Python bindings
│   └── hwpforge-bindings-cli/     # ⚒️  CLI tool
└── .docs/                         # Internal docs (git-excluded)
```

---

## CI/CD Policy

- `push` (all branches): fast checks (`fmt`, `clippy`, `nextest`, `cargo-deny`, docs lint)
- `pull_request` to `main`: full checks (+ coverage 90%, MSRV 1.75, macOS/Windows build check)
- `push` to `main` (merge): full checks (same as PR gate)
- `push` tag `v*.*.*`: release pipeline (verify + GitHub Release publish)
- `schedule` (weekly): canary checks on Rust `beta`/`nightly`

Version strategy is contract-based, not "all versions":

- must pass: `1.75 (MSRV)` + pinned stable toolchain (`1.93` now)
- monitored: `beta`/`nightly` weekly canary (nightly is non-blocking)

---

## Development Philosophy

**Forge Metaphor**:

- Foundation = Raw materials
- Core = Anvil (pure structure)
- Blueprint = Design patterns (reusable styles)
- Smithy = Format-specific workshops
- Bindings = User tools

**TDD**: Edge cases first, normal cases last.

**Quality**: 90%+ coverage per crate, 0 warnings, 100% rustdoc.

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

---

## Acknowledgments

본 제품은 한글과컴퓨터의 한/글 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.
