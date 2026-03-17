# Contributing to HwpForge

HwpForge는 HWP/HWPX 문서를 프로그래밍 방식으로 다루기 위한 Rust 라이브러리입니다.
버그 수정, 포맷 리서치, 테스트 보강, 문서 개선 모두 환영합니다.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Architecture Overview](#architecture-overview)
- [Making Changes](#making-changes)
- [Commit Conventions](#commit-conventions)
- [Pull Request Guide](#pull-request-guide)
- [Testing](#testing)
- [Documentation](#documentation)
- [MSRV Policy](#msrv-policy)
- [License](#license)

## Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).
We are committed to providing a friendly, safe, and welcoming environment for all contributors.

## Getting Started

### First-time contributors

Good first issues are labeled [`good first issue`](https://github.com/ai-screams/HwpForge/labels/good%20first%20issue).
These are self-contained tasks with clear scope — a great way to get familiar with the codebase.

Before starting work on anything larger, please open an issue to discuss the approach.
This prevents duplicated effort and ensures the change aligns with the project direction.

### Reporting bugs

- Check [existing issues](https://github.com/ai-screams/HwpForge/issues) first.
- Use the [Bug Report template](https://github.com/ai-screams/HwpForge/issues/new?template=bug_report.yml).
- Include a minimal reproduction, the HwpForge version, and `rustc --version` output.
- If you have a `.hwpx` file that triggers the bug, attach it — sample files are extremely valuable.

### Security vulnerabilities

**Do not open a public issue.** See [SECURITY.md](SECURITY.md) for private reporting instructions.

## Development Environment

### Prerequisites

- **Rust 1.88+** (MSRV) — `rustup show` to verify
- [cargo-nextest](https://nexte.st/) — parallel test runner (recommended)
- [pre-commit](https://pre-commit.com/) — git hook automation (optional)

### Essential commands

```bash
make ci          # Full CI pipeline: fmt + clippy + test + deny + lint
make test        # cargo nextest run (parallel, all features)
make clippy      # cargo clippy (all targets, all features, -D warnings)
make fmt-fix     # Auto-format with rustfmt
make doc         # Generate rustdoc (opens in browser)
make cov         # Coverage report with 90% gate (llvm-cov)
mdbook build     # Build the project book
```

**Always run `make ci` before pushing.** It matches CI flags exactly.
Bare `cargo clippy` or `cargo fmt --check` will miss workspace-level checks.

### Watch mode

```bash
bacon         # Auto-run clippy on file changes
bacon test    # Auto-run tests on file changes
```

## Architecture Overview

```
foundation (primitives: HwpUnit, Color, Index<T>)
    |
  core (format-independent document model, style references only)
    |
  blueprint (YAML style templates, Figma Design Token pattern)
    |
  smithy-hwpx / smithy-md (format-specific compilers)
    |
  bindings-py / bindings-cli (user interfaces)
```

**Key principle**: Structure and Style are separate (like HTML + CSS).
Core holds style _references_ (indices), Blueprint holds style _definitions_ (fonts, sizes, colors).

Before touching a crate, read its `AGENTS.md` (if present) for role definitions.

**Foundation is the root.** Changes to `hwpforge-foundation` rebuild _everything_.
Keep it minimal.

## Making Changes

### Before you start

1. Check for existing issues or PRs covering the same change.
2. For non-trivial changes, open an issue to discuss the approach first.
3. Fork the repository and create a feature branch from `main`.

### During implementation

1. **TDD** — write edge-case tests first, then implement, then refactor.
2. **Atomic commits** — one logical change per commit.
3. **Documentation** — 100% rustdoc coverage (enforced by `#![deny(missing_docs)]`).
4. **Zero warnings** — `cargo clippy -- -D warnings` must pass.

### Before submitting

1. Run `make ci` — it must pass.
2. If you changed docs, verify with `mdbook build`.
3. If you added a public API, ensure rustdoc is complete.
4. If your change affects roundtrip (decode/encode), add a golden test with a real HWPX file.

## Commit Conventions

We use [Conventional Commits](https://www.conventionalcommits.org/).
[release-plz](https://release-plz.ieni.dev/) uses these prefixes to determine release scope.
If you need to force a breaking release on a non-standard type, use `type!:` in the subject.

### Release-triggering prefixes

| Prefix     | When to use                   | SemVer impact |
| ---------- | ----------------------------- | ------------- |
| `feat`     | New feature or capability     | Minor         |
| `fix`      | Bug fix                       | Patch         |
| `perf`     | Performance improvement       | Patch         |
| `refactor` | Code restructuring (no API Δ) | Patch         |

Any conventional `type!:` commit is also treated as release-triggering so explicit breaking changes
do not get filtered out when they use a non-standard type.

### Non-release prefixes

| Prefix  | When to use                         |
| ------- | ----------------------------------- |
| `docs`  | Documentation only                  |
| `test`  | Adding or updating tests            |
| `ci`    | CI/CD configuration                 |
| `chore` | Maintenance (deps, tooling, config) |

### Breaking changes

Append `!` after the prefix: `feat!: remove deprecated method`.
Include a `BREAKING CHANGE:` footer in the commit body describing the migration path.

### Scope (optional)

Use the crate name as scope: `fix(smithy-hwpx): handle self-closing colPr tags`.

## Pull Request Guide

Use the PR template. At minimum, include:

- **What** changed
- **Why** it's needed
- **How** it was verified

### Checklist

- [ ] `make ci` passes
- [ ] Tests added or updated
- [ ] Public API changes are documented (rustdoc)
- [ ] Doc changes verified with `mdbook build`
- [ ] No MSRV regression (or documented if intentional)
- [ ] Commits follow Conventional Commits

### Impact flags

If any of these apply, call them out in the PR description:

- Breaking API change
- Format roundtrip affected
- MSRV change
- New dependency added
- Performance impact

### Review process

- All PRs require at least one approving review.
- CI must be green before merge.
- Squash merge is the default strategy.

## Testing

HwpForge uses a 3-tier testing strategy:

### 1. Golden tests (most important)

Real HWPX/HWP5 files from Hancom 한글.
Load → Save → Load → assert equality.

```bash
cargo test --test golden
```

### 2. Unit tests (edge-first)

Write boundary and invalid-input tests before happy paths:

- Boundary values: `0`, `MIN`, `MAX`
- Invalid inputs: `INFINITY`, `NAN`, empty strings
- Round-trip: `pt → HwpUnit → pt`, `RGB → BGR → RGB`
- Normal cases last

```bash
cargo test --lib
cargo test -p hwpforge-foundation
```

### 3. Property tests

`proptest` for invariants that should hold for all inputs.

### Coverage

Target: **90%+** per crate (enforced in CI).

```bash
make cov    # HTML coverage report
```

## Documentation

Documentation changes follow the same review bar as code changes.

- Markdown lint must pass (`dprint check` + `markdownlint-cli2`).
- `mdbook build` must succeed.
- Code examples should compile against the current API.
- Use `#![doc = include_str!("...")]` for long module docs when appropriate.
- All public items need rustdoc with `# Examples`, `# Errors`, and `# Panics` sections where applicable.

## MSRV Policy

HwpForge maintains an MSRV of **stable minus 4 releases** (currently Rust 1.88).

Rules:

- `rust-version` in `Cargo.toml` is the single source of truth.
- New code that compiles on latest stable but breaks on MSRV is a regression.
- If a dependency update requires raising MSRV, document the reason in the PR and update `Cargo.toml`, CI, and CHANGELOG together.
- MSRV bumps are never silent — they require explicit discussion and approval.

## License

This repository is dual-licensed under **MIT OR Apache-2.0** ([LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE)).

By submitting a contribution, you agree that your work may be distributed under either license,
at the choice of downstream users. You certify that you have the right to submit the contribution
under these terms (see the [Developer Certificate of Origin](https://developercertificate.org/)).
