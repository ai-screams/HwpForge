# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. AI agents generate documents via Markdown + YAML style templates, then edit them through JSON round-trip workflows.

**Stats**: ~50,700 LOC, 1,592 tests (nextest), 103 .rs files, 9 crates, 92.65% coverage

---

## Architecture

```
Foundation (primitives: HwpUnit, Color, Index<T>)
  -> Core (pure document structure, style references only)
  -> Blueprint (YAML style templates, Figma Design Token pattern)
  -> Smithy (format compilers: HWPX, HWP5, Markdown)
  -> Bindings (CLI, Python)
```

**Key Principle**: Structure and Style are separate (like HTML + CSS).

- Core = style **references** (IDs only)
- Blueprint = style **definitions** (fonts, sizes, colors)
- Smithy = Core + Blueprint -> final format

**Dependency graph** (modifying foundation rebuilds everything — keep it minimal):

```
foundation -> core -> blueprint -> smithy-hwpx, smithy-md -> bindings-cli, bindings-py
```

---

## Development Commands

```bash
make ci          # Full CI: fmt + clippy + test + deny + lint (ALWAYS run before push)
make test        # cargo-nextest (parallel)
make clippy      # All crates, all targets, all features, -D warnings
make fmt-fix     # Auto-fix formatting
make doc         # Generate rustdoc
make cov         # Coverage report (90% gate)
bacon            # Watch mode: auto-run clippy on save
```

**CI rule**: ALWAYS `make ci` before push. Never use bare `cargo clippy` or `cargo fmt --check` — they miss flags.

---

## Design Patterns

### 1. Color is BGR (NOT RGB!)

```rust
Color::from_rgb(255, 0, 0)  // red -> 0x0000FF internally (BGR)
// NEVER: Color::from_raw(0xFF0000) — this is BLUE in HWP!
```

### 2. HwpUnit Integer-Based Units

```rust
HwpUnit::from_pt(12.0)  // 12pt -> HwpUnit(1200). 1pt=100, 1mm~=283.
```

### 3. Branded Index Types

```rust
CharShapeIndex::new(0)   // OK
let idx: ParaShapeIndex = CharShapeIndex::new(0);  // Compile error! Cannot mix.
```

### 4. Typestate Pattern (Core)

```rust
let doc = Document::<Draft>::new();
let validated = doc.validate()?;  // Draft -> Validated at compile time
```

### 5. Two-Type Pattern (Blueprint)

```rust
// PartialCharShape: all fields Option (for YAML merge)
// CharShape: all fields required (after resolution)
let resolved: CharShape = partial.resolve("style_name")?;
```

### 6. StyleRegistry Pipeline

```rust
let template = Template::from_yaml(yaml_str)?;
let resolved = resolve_template(&template, &provider)?;
let registry = StyleRegistry::from_template(&resolved)?;
```

---

## Testing Strategy

**3-Tier Approach:**

1. **Golden Tests**: Real HWPX files from Hancom. Load -> Save -> Load -> assert equality
2. **Unit Tests**: Edge cases first (TDD). Boundary, invalid, normal — in that order
3. **Property Tests**: `proptest` for round-trip invariants

**TDD order for new types:** 0/MIN/MAX boundaries -> overflow/underflow -> invalid inputs -> round-trip -> normal cases

**CLI tests** (79 integration tests): Content verification against real HWPX fixtures, all flag combinations, edge cases (binary garbage, empty file), end-to-end pipeline tests.

Target: **95%+ coverage** per crate. Current: 92.65%.

---

## Gotchas & Common Mistakes

### HWPX Landscape (spec reversed!)

- `WIDELY` = portrait, `NARROWLY` = landscape (opposite of KS X 6101 spec)
- width/height always portrait-based (A4 = 210x297). Use `PageSettings.landscape: bool`
- **NEVER** infer orientation from width > height comparison

### Geometry Namespace: ALL use `hc:` (NOT `hp:`)

```xml
<hc:startPt x="0" y="0"/>   <!-- line endpoints -->
<hc:pt x="0" y="0"/>        <!-- polygon vertices -->
<hc:pt0 x="0" y="0"/>       <!-- textbox corners pt0-pt3 -->
```

Using `hp:` causes parse errors or "파일을 읽거나 저장하는데 오류".

### TextBox = hp:rect + hp:drawText (NOT a control element)

```xml
<hp:rect ...><hp:drawText>...</hp:drawText></hp:rect>
```

Key rules: element order matters (shape-common -> drawText -> caption -> hc:pt0-3 -> sz -> pos -> outMargin -> shapeComment), lastWidth = full width (no margin deduction), shadow alpha = 178, `<hp:shapeComment>` mandatory, shape run needs empty `<hp:t/>` marker.

### Chart Encoding Rules

1. **No manifest**: Chart/*.xml in ZIP only, NOT in content.hpf (crashes)
2. **`<c:f>` required**: Dummy formula needed for data display (`Sheet1!$A$2:$A$5`)
3. **`<c:tx>` direct value only**: `<c:tx><c:v>Name</c:v></c:tx>` (NOT strRef — crashes)
4. **`dropcapstyle="None"` required** on `<hp:chart>`, `horzRelTo="COLUMN"`
5. **VHLC/VOHLC**: 4-axis combo layout (each chart type needs own catAx+valAx pair)

### Field Encoding

- **Hyperlink**: `fieldBegin type="HYPERLINK"` + `fieldEnd` pair (no `<hp:hyperlink>` element)
- **Date/Time/Summary**: `type="SUMMERY"` (Hancom typo maintained 14+ years), `Prop=8`, `fieldid=628321650`
- **CLICK_HERE**: `Prop=9`, `fieldid=627272811`, `editable="1"`
- **Page number in body**: `<hp:autoNum numType="PAGE">` (NOT fieldBegin)
- **Page number in header/footer**: `<hp:pageNum>` in secPr

### Footnote/Endnote: Must be inline Run

```rust
// WRONG: separate paragraph -> footnote number on its own line
// CORRECT: same paragraph, as a Run
Paragraph::with_runs(vec![
    Run::text("Body text.", cs),
    Run::control(Control::footnote(notes), cs),
], ps);
```

### Style System

- **breakNonLatinWord = KEEP_WORD** (not BREAK_WORD — causes character-level spacing in justified text)
- **Modern style set**: 개요 8/9/10 use paraPr groups 18/16/17 (NOT sequential)
- **User paraShapes start at index 20** in Modern (after 20 defaults)
- **DropCapStyle**: PascalCase (DoubleLine, not DOUBLE_LINE), shape-level attribute

### Multiple switches per paraPr

Schema uses `Vec<HxSwitch>`, NOT `Option<HxSwitch>`. Real files have 2+ switches per `<hh:paraPr>`.

### Equation: NO shape common block

Unlike shapes (line/ellipse/polygon), equation has no offset/orgSz/curSz/flip/rotation/lineShape/fillBrush/shadow. Uses `flowWithText="1"`, `outMargin` left/right=56.

### Self-closing colPr

```rust
// WRONG: xml.find("</hp:colPr>")  — misses self-closing tags
// CORRECT: xml.find("<hp:colPr")   — matches both forms
```

### Polygon vertex closure

First vertex must be repeated at end to close the path. Hancom does NOT auto-close.

### ArrowType: Use EMPTY_ forms only

Hancom ignores `FILLED_DIAMOND/CIRCLE/BOX`. Use `EMPTY_*` + `headfill="1"` for filled.

### MasterPage XML

1. Root: `<masterPage>` (no prefix, NOT `<hm:masterPage>`)
2. 15 xmlns declarations required (same as header/section)
3. `<hp:subList>` (NOT `<hm:subList>`)

### HWP5 TagID Offset

Section records have +16 offset from spec: `PARA_HEADER` = 0x42 (66), not 0x32 (50).

### Dependency Versions

- **schemars 1.x**: `schema_name()` returns `Cow<'static, str>` (not `String`)
- **quick-xml 0.39**: `unescape()` removed (use `decoder().decode()`), handle `Event::GeneralRef`

### page_break encoding

`page_break: u32::from(para.page_break)` in `build_paragraph()` — not hardcoded 0.

---

## Phase Status

### v1.0

| Phase      | Crate                           | Status | Tests | LOC    |
| ---------- | ------------------------------- | ------ | ----- | ------ |
| 0          | foundation                      | Done   | 224   | 4,432  |
| 1          | core                            | Done   | 331   | 5,554  |
| 2          | blueprint                       | Done   | 200   | 4,647  |
| 3          | smithy-hwpx decoder             | Done   | 110   | 3,666  |
| 4          | smithy-hwpx encoder             | Done   | 226   | 10,349 |
| 4.5 W1-6   | image/header/footer/shape/chart | Done   | —     | —      |
| 5          | smithy-md                       | Done   | 73    | 3,757  |
| W7-14      | style/paragraph/layout/shape    | Done   | —     | ~5,250 |
| 6 (CLI)    | bindings-cli (79 integ tests)   | Done   | 80    | 1,035  |
| 6 (Python) | bindings-py (PyO3)              | Ready  | —     | —      |
| 7          | MCP integration                 | Ready  | —     | —      |
| 8          | Testing + Release v1.0          | Ready  | —     | —      |

### v2.0

| Phase | Crate                                      | Status |
| ----- | ------------------------------------------ | ------ |
| 9     | HWPX Full (OLE/양식컨트롤/변경추적/책갈피) | Ready  |
| 10    | smithy-hwp5 (HWP5 읽기)                    | Ready  |

---

## Development Workflow

**Before starting new work:**

1. Read `crates/hwpforge-{crate}/AGENTS.md` if it exists
2. Read `.docs/planning/` for relevant design docs

**During implementation:**

1. TDD: edge cases first
2. Atomic commits (one logical change per commit)
3. 100% rustdoc (`#![deny(missing_docs)]`)
4. Zero clippy warnings

**After implementation:**

1. Run `make ci`
2. Verify coverage >= 90%

---

## Key References

- **KS X 6101 spec**: openhwp/docs/hwpx/ (9,054 lines of spec in markdown)
- **HWP5 format**: `.docs/research/ANALYSIS_hwpers.md`, HWP_5_0_FORMAT_COMPLETE_GUIDE.md
- **API design**: Follow foundation patterns (Newtype, Branded Index, ErrorCode). Structure (Core) vs Style (Blueprint).
