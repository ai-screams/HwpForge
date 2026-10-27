# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository. Contributor workflow (PR, release) lives in `CONTRIBUTING.md` and `RELEASING.md`.

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

## Architecture (Forge Metaphor)

The codebase follows a **blacksmith workshop** metaphor with clear separation of concerns (canonical names: `branding/BRANDING.md`):

```
Foundation = Ingot (primitives)
  → Core = Anvil (pure document structure, no style definitions; the hub every format passes through)
  → Blueprint = Blueprint (YAML style templates, centralized like Figma Design Tokens)
  → Smithy = Hearth (format-specific compilers: HWPX, HWP5, Markdown, PDF)
  → Bindings: CLI = Hammer 🔨 · MCP = Vise 🗜️ · Python = Tongs 🐍
```

**Key Principle**: **Structure and Style are separate** (like HTML + CSS).

- Core contains document structure with style **references** (IDs only)
- Blueprint contains style **definitions** (fonts, sizes, colors)
- Smithy compilers fuse Core + Blueprint → final format

This enables:

- One YAML template applied to multiple documents
- Format-agnostic document manipulation
- Easy addition of new formats — smithy-pdf is a real, shipped example (0.12.1+); smithy-odt remains a hypothetical one

## Crate Map

`crates/hwpforge-foundation` (Ingot) · `hwpforge-core` (Anvil) · `hwpforge-blueprint` · `hwpforge-smithy-hwpx` · `hwpforge-smithy-hwp5` · `hwpforge-smithy-md` · `hwpforge-smithy-pdf` · `hwpforge-convert` (cross-format operations) · `hwpforge` (facade) · `hwpforge-bindings-cli` · `hwpforge-bindings-mcp` · `hwpforge-bindings-py`.

## Tests

- Shared integration-test helpers live in `crates/<crate>/tests/support/<concern>.rs`, included with `#[path = "support/<concern>.rs"] mod <concern>;`. Reuse them before adding a helper.
- CLI integration tests are one binary split by command: `crates/hwpforge-bindings-cli/tests/cli_integration/<command>.rs`.
- Generated `.hwpx` inputs must be reproducible from `crates/*/examples` (see `crates/hwpforge-smithy-hwpx/tests/generated_inputs.rs`).

---

## Development Commands

```bash
make ci               # required before push — alias of ci-fast (fixture-policy, fmt, clippy, test, deny, lint-md; same flags as CI)
make ci-full          # the above + coverage + MSRV (before a release or a large change)
make py-all           # Python bindings (lint, types, Rust/Python tests, coverage)
make site-check       # docs site build + post-processing (same path as CI Docs Build; needs mdBook 0.4)
mdbook build          # when docs change — mdBook 0.4 line required (`make install-tools`)
cargo nextest run -p <crate>          # one crate (a cold full-workspace run takes 15+ min)
cargo clippy -p <crate> --all-targets -- -D warnings   # pre-check the crates you touched
bacon / bacon test    # watch mode
```

`make ci` is the six fast lanes, not all of CI. CI also runs Coverage, MSRV, Docs Build, Python (including release-only tests) and Workflow Lint.

---

## Working Principles

- **Warning-first for unknowns**: if source truth is missing or a value is unsupported, emit a warning or validation signal first.
- **No fake support**: do not silently normalize unknown semantics into arbitrary defaults just to keep output green.
- **Unhandled enum ≠ bug**: an unmatched enum arm is a real gap only if that value actually exists in the reference enum (hwpxlib/libhwp) or in a document saved by Hancom Office. Verify existence first — otherwise `_ => default` is correct and a guessed mapping is fake support. (See `HWP5_WIRE_SPEC.md §22`: numbering code 11 and image fill mode were real bugs; 가나다 numbering and diagonal types 1/4/5 were false positives.)
- **Shared-model first**: if HWP5 discovers a semantic that Core/HWPX cannot carry, extend the shared representation first and wire HWP5 after.
- **Semver-first for public API**: if a design touches public structs, enums, or externally constructible types, surface the breakage before implementation and get approval first.
- **Editing surfaces fail closed on semantic loss**: preserve-first editors (stamper, cell-edit) reject with their existing error type when encoding emits a semantic warning (`NoteHeadSkipped` etc.). Admission compares Core only and cannot see wire damage, so never return success silently.

Always: **TDD edge-first** · **atomic conventional commits** (breaking = `type(scope)!:`) · **100% rustdoc** (`#![deny(missing_docs)]`) · **zero clippy warnings**.

## Repository Rules

1. **Color is BGR** — use `Color::from_rgb()` only (`from_raw(0xFF0000)` is blue).
2. **No manual version/tag/publish** — release-plz owns workspace releases (`v*`); the only hand-made tag is the emergency Python `py-v*` tag in `RELEASING.md` §9.3. Merge to main only through the merge queue (squash).
3. **`Cargo.lock` and `uv.lock` are not tracked** — they are in `.gitignore`, so `git add` refuses them. CI resolves the latest dependencies.
4. **Do not edit sources while tests run** (it triggers a rebuild). nextest filters are **substrings** (`'a|b'` does not work).
5. PR titles are **English conventional commits** (squash makes the PR title the main commit and the release-plz input; breaking = `type!:`).
6. **Do not commit documents saved by other apps** — `scripts/check_fixture_policy.py` rejects `.hwp`, `.pdf` and `.hwpx` without the HwpForge encoder signature. Test inputs must be reproducible from examples or code in the repo.
7. **Test temp paths use a drop guard** — use `tempfile::TempDir` or the CLI `test_tmp()` and bind the value to a variable (`test_tmp().join(..)` deletes the directory at the end of that statement). A directory made with `std::env::temp_dir().join(..)` and never removed piles up by hundreds per nextest run.
8. **Tracked files carry no work-phase labels** — comments, doc text, messages, test names and file names must not cite work-phase names (Wave/W/Phase/E/S numbers such as `W4a`), review rounds, or documents outside the repo; state the reason directly and name things by what they do. Public issue/PR numbers and `HWP5_WIRE_SPEC.md §N` are fine.
