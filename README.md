# HwpForge

> **한글 문서(HWP/HWPX)를 프로그래밍으로 제어하는 Rust 라이브러리**
>
> LLM-first design | Rust Core + Python Wrapper | YAML Style Templates

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

---

## Status: Under Development (v0.1.0)

**Phase 0-5 + 4.5 Wave 1-6 완료** (Foundation → Core → Blueprint → HWPX → Markdown)

- Foundation: HwpUnit, Color (BGR), Index<T>, ErrorCode (224 tests)
- Core: Document<Draft/Validated>, Paragraph, Table, Image, Shapes, Equation, Chart (364 tests)
- Blueprint: YAML Template System, StyleRegistry, Font Dedup (203 tests)
- Smithy-HWPX: Full Encoder/Decoder with roundtrip, 9 golden fixtures (253 tests)
- Smithy-MD: GFM + lossless HTML+YAML dual-mode (74 tests)

**Stats**: ~37,052 LOC | 988 tests | 0 clippy warnings | 90%+ coverage

**Next**: Phase 6 (Python/CLI bindings), Phase 7 (MCP), Phase 8 (v1.0 release)

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

- Rust 1.93+ (pinned development toolchain)
- Rust 1.88 (MSRV — minimum supported version)
- (Optional) Python 3.8+ for bindings
- (Optional) pre-commit for hooks

### Build & Test

```bash
# Build
cargo build --workspace

# Test (using cargo-nextest)
cargo nextest run --workspace --all-features

# Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Coverage (90% gate)
cargo llvm-cov nextest --workspace --all-features --fail-under-lines 90

# Watch mode
bacon

# Full local CI
make ci-fast
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

## CI/CD

### Pipeline Architecture (Fan-out Gate)

```
Tier 1 — Gate     fmt ──┐
                  clippy┤
                        │
Tier 2 — Verify   test ◄┤  (all events)
                  cov  ◄┤  (PR / merge_group / full-suite)
                  deny ◄┤
                  docs ◄┤
                  msrv ◄┘
                        │
Tier 3 — Platform cross ◄── test  (Windows + macOS)
```

### Trigger Matrix

| Event                             | Jobs                                      |
| --------------------------------- | ----------------------------------------- |
| `push` to main                    | merge-smoke (nextest + deny)              |
| `pull_request` / `merge_group`    | Gate → full Tier 2 + Tier 3               |
| `schedule` (weekly Mon 03:00 UTC) | toolchain canary (beta + nightly)         |
| `workflow_dispatch`               | Gate + test (optional: full suite)        |
| Tag `v*.*.*`                      | Release: full CI → build → GitHub Release |

### Security

- **PR**: `cargo-deny` (licenses + advisories + bans)
- **Weekly** (Mon 03:30 UTC): advisory-only scan
- **Dependabot**: weekly Cargo + Actions updates

### Version Strategy

| Tier   | Version        | Purpose                                  |
| ------ | -------------- | ---------------------------------------- |
| MSRV   | 1.88           | Minimum supported — `cargo +1.88 check`  |
| Stable | 1.93           | Pinned development toolchain             |
| Canary | beta / nightly | Weekly monitoring (nightly non-blocking) |

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
