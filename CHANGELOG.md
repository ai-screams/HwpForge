# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — targeted as `0.6.0`

### Phase 12 (HWP5 drawing-object carry)

Continues the Phase 11 line: HWP5 drawing objects the decoder previously
skipped now carry through Core to HWPX instead of silently emptying their
host paragraph. No Core or HWPX API changes — these shape variants already
existed in the shared model; only the HWP5 leg was missing.

#### Added — Wave 12a (GSO ellipse / arc / curve)

- Decode `gso ` `ShapeComponentEllipse` (`0x50`) and `ShapeComponentCurve`
  (`0x53`) sub-records and project them to `Control::Ellipse`,
  `Control::Arc`, and `Control::Curve`. Previously these fell through to
  `Hwp5Control::Unknown` and were dropped, emptying the host paragraph.
- 한컴 stores arcs inside the ellipse (`0x50`) record with arc fields set —
  it does **not** emit a separate `ShapeComponentArc` (`0x51`). An arc is
  now distinguished by content and carried as `Control::Arc` →
  `<hp:ellipse hasArcPr="1">`.
- Classify ellipse/arc/curve in the audit semantic model
  (`Hwp5SemanticControlKind::{Ellipse, Arc, Curve}`) so source-side control
  counts match converted output.
- Binary layouts confirmed empirically from 한컴 truth fixtures
  (`sample-gso-{ellipse,arc,curve}`); golden tests assert end-to-end carry.
- Known limitation: arcs carry as the `Normal` arc type sized from the
  bounding box. Pie/chord arc types and exact arc-sweep endpoints are
  deferred until dedicated fixtures exist.

#### Added — Wave 12b (GSO connect line)

- Carry connectors as `Control::ConnectLine` → `<hp:connectLine>` instead of
  demoting them to a plain `<hp:line>`. 한컴 stores a connector in the **same**
  `ShapeComponentLine` (`0x4E`) sub-record as a plain line; the only
  discriminator is the `ShapeComponent` (`0x4C`) type tag `"$col"` (confirmed
  against `$rec`/`$ell`/`$cur`). A conservative guard upgrades **only** an
  exact `"$col"` match, so plain lines are never reclassified.
- Classify connectors in the audit semantic model
  (`Hwp5SemanticControlKind::ConnectLine`).
- Confirmed end to end against a natively-drawn 한컴 connector fixture
  (`sample-gso-connectline-native`: two rectangles + one connector).
- Known limitation: only a straight connector with its endpoints is carried;
  the source connector's object-link references have no `<hp:connectLine>`
  representation and are dropped.

#### Fixed — floating ellipse/arc/curve/connect-line positioning (HWPX encoder)

- `<hp:ellipse>`, `<hp:curve>`, and `<hp:connectLine>` hardcoded inline
  positioning (`numberingType="NONE"`, `textWrap="TOP_AND_BOTTOM"`,
  `vertRelTo/horzRelTo="PARA"`) instead of using the shared offset-aware
  helpers that `<hp:line>`/`<hp:rect>` already used. A **floating** shape
  (non-zero offset) was therefore mis-anchored to the paragraph and rendered
  in the wrong place in 한컴. They now route through `shape_position` /
  `shape_numbering_type` / `shape_text_wrap`, so a floating shape anchors to
  `PAPER` as `PICTURE`/`IN_FRONT_OF_TEXT` (matching 한컴) while inline shapes
  (zero offset) are unchanged. Exposed by Wave 12b's first floating connector.

#### Added — Wave 12d (equation)

- Carry equations as `Control::Equation` → `<hp:equation>` with the HancomEQN
  script preserved. The `eqed` ctrl used to fall through to
  `Hwp5Control::Unknown` and was dropped. The decoder now recognizes the
  `eqed` ctrl, parses the script from its child `HWPTAG_EQEDIT` (`0x58`)
  record (`UINT32` property, then a `UINT16` WCHAR-count length prefix + UTF-16
  script), and projects it to `Control::Equation` sized from the ctrl-header
  geometry.
- Classify equations in the audit semantic model
  (`Hwp5SemanticControlKind::Equation`).
- Confirmed end to end against a 한컴 equation fixture
  (`sample-equation-basic`: `{a + b} over {c + d}`); golden test asserts both
  `<hp:equation>` emission and verbatim `<hp:script>` carry.

#### Added — Wave 12e-Memo (memo annotation carry + body corruption fix)

- Carry memo annotations as `Control::Memo` → HWPX `<hp:fieldBegin type="MEMO">`
  with the body paragraphs in `<hp:subList>`. The HWP5 `%unk` ctrl with command
  `"MEMO/{shapeId}/{memo_id}/{instId}"` used to fall through to
  `Hwp5Control::Unknown`, so the matching `HWPTAG_MEMO_LIST` (`0x5D`) cluster's
  level-2 `ParaText` records would fall into the body-paragraph `ParaText` arm
  and **overwrite the body text** — the visible "memo content replaces body
  text" corpus bug.
- Decoder now recognises the `%unk MEMO/...` placeholder, captures the matching
  cluster region at the end of the section's last body paragraph (records at
  level 1/2: `MemoList`, `ListHeader`, content `ParaHeader`, `ParaText`,
  `CharShape`, …), and joins clusters back to placeholders by `memo_id` (not
  by document position) during `BodyTextParserState::finish`. Multi-memo
  fixtures confirmed both happy path and id-keyed matching.
- Projection adds an `ActiveField::MemoAnchor` so the anchor text inside the
  `FieldBegin %unk MEMO` / `FieldEnd` span flows into `runs` (no drop) and the
  memo `Run` is emitted at the anchor's start position.
- `layout_hint_patch` now folds memo body paragraphs into the body scope so the
  HWPX patcher does not underflow the paragraph-hint queue.
- Classify memos in the audit semantic model (`Hwp5SemanticControlKind::Memo`).
- Golden tests confirmed against two 한컴 fixtures: `sample-memo-basic` (one
  memo, body anchor + body content preservation) and `sample-memo-multiple`
  (two memos, id-keyed cluster matching).

#### Changed — Wave 12e-Memo (Core API breaking, semver-deliberate)

- `Control::Memo` no longer carries `author` / `date` — the variant is now
  `Control::Memo { content: Vec<Paragraph> }`. Neither field was actually
  populated by any wire path: the HWPX `<hp:fieldBegin type="MEMO">` only
  exposes `MemoShapeID` / `MemoType` parameters, and HWP5's `%unk MEMO/...`
  command exposes no author/date metadata. Holding the fields encouraged
  callers to pass dummy values that never round-tripped.
- `Control::memo(content)` helper drops the corresponding `author`/`date`
  parameters; call sites in `smithy-hwpx` examples (`shapes_and_references`,
  `hwpx_complete_guide_parts/section2`) and tests
  (`smithy-hwpx/src/registry_bridge.rs`,
  `smithy-hwpx/src/encoder/section.rs`) updated.
- `smithy-md` markdown encoder emits `<!-- memo: body -->` instead of
  `<!-- memo(author): body -->` (the author segment was always blank in
  practice).

#### Fixed — Wave 12f (memo anchor position)

- HWP5 stores the memo *inline* `FieldBegin` marker with `extra[0..4] = %%me`
  (`0x2525_6D65`), which is **not** the same id as the `CtrlHeader` ctrl_id
  `%unk` (`0x2575_6E6B`). Wave 12e matched only the latter, so the inline
  anchor was never recognised and the memo `Run` ended up drained at the
  end of the paragraph as a point-anchored field — 한컴 rendered it as
  `메모 대상 문장입니다.[메모 시작][필드 끝]`.
- `projection.rs::start_active_field` now recognises both ids: the inline
  marker activates `ActiveField::MemoAnchor`, which positions the memo
  `Run` at the correct anchor offset (`vis=2` in the basic fixture).

#### Changed — Wave 12g (Core API breaking, semver-deliberate)

- `Control::Memo` gains `anchor_runs: Vec<Run>`. The anchor body sits
  *inside* the same `<hp:run>` as `fieldBegin`/`fieldEnd` in 한컴
  truth fixtures; the previous Wave-12f layout placed the anchor as a
  separate Run before the memo and 한컴 mis-rendered the end marker as
  generic `[필드 끝]`.
- HWPX encoder now serializes memos as a flat
  `[fieldBegin][anchor_xml][fieldEnd]` `<hp:run>` via
  `build_memo_anchor_xml`. See
  `.docs/algorithms/2026-06-01_memo_anchor_serialization.md` for the
  anchor-body collapse heuristic (single `<hp:t>` per anchor) and why
  we accept the per-run char-shape fidelity loss.
- `Control::memo_with_anchor(content, anchor_runs)` helper added.

#### Added / Changed — Wave 12h (full `<hp:parameters>` carry)

- HWPX `<hp:parameters>` now emits the full 한컴-standard `cnt="7"`
  block (`Prop`, `Command`, `ID`, `Number`, `Author`, `MemoShapeIDRef`,
  `CreateDateTime`) plus `editable="1" dirty="1" zorder="1"`. The
  pre-12h `cnt="2"` block (`MemoShapeID` + `MemoType` only) made
  한컴 mis-classify the field — the memo body rendered correctly, but
  the end marker fell back to generic `[필드 끝]` in 조판부호 view.
- New schema struct `Hwp5MemoCommand` parses the wire's
  `"MEMO/{shape_id}/{memo_id}/{hancom_inst_a}/{hancom_inst_b}/{author}/{terminator}"`
  command string. Reusable wire-string utilities
  (`parse_ctrl_header_command_string`, `split_slash_command`) added to
  `schema/section.rs` for future `%hlk` / `%xrf` / `%bmk` work.
- New `Control::Memo { metadata: MemoMetadata, … }` field (Core API
  breaking, semver-deliberate). `MemoMetadata` carries `shape_id_ref`,
  `number`, `id`, `author`, `create_datetime`, `command` — wire
  values flow through `projection.rs` verbatim.
- `CreateDateTime` has no HWP5 wire source (verified via DOC_PROPERTIES
  `0x10`, TRACKCHANGE `0x20`, MemoShape `0x5E` dumps). Encoder fills it
  with `iso8601_utc_now()` (std-only Howard-Hinnant civil-from-days) at
  write time — matching 한컴's own behaviour on fresh saves.
- `build_memo_parameters_xml` extracted as a re-usable HWPX
  `<hp:parameters>` builder for future field types that need the same
  structure (hyperlink / cross-reference fidelity work).

#### Added / Changed — Wave 12i (dutmal carry + flat-path control filter)

- Carry dutmal (덧말) annotations as `Control::Dutmal` →
  `<hp:dutmal>` with `main_text` / `sub_text` / `posType` /
  **`option`**. The decoder now reads `option_raw` from
  `tail[8..12]` of the `tdut` ctrl payload (`Hwp5DutmalControl`); the
  HWPX encoder previously hard-coded `option="0"` and the HWPX
  decoder discarded the value, so any non-default `option=4` 한컴
  fixture lost fidelity end to end. Both legs now mirror the integer
  verbatim. Semantics of `option` are intentionally not pinned —
  the bit/enum meaning is undocumented and produces no visible
  rendering difference in our truth fixture; see
  `.docs/algorithms/2026-06-01_dutmal_carry.md`.
- New `Control::Dutmal { metadata: DutmalMetadata, … }` field
  (Core API breaking, semver-deliberate). `DutmalMetadata` carries
  `option: u32` and is `#[non_exhaustive]` so future
  `sz_ratio` / `align` / `style_id_ref` decode work is additive.

#### Fixed — Wave 12i flat-path projection `control_iter` filter

- `project_paragraph_with_images_flat` used to iterate **every**
  control in `Hwp5Paragraph.controls` (including the
  `secd` / `cold` / `%bmk` / `%hlk` / `%xrf` / `bokm` / `pgnp`
  Unknown markers that lead a first-section paragraph) when matching
  inline `\u{FFFC}` `ControlRef` positions to runs. Each FFFC
  popped the *wrong* control — the marker-header Unknowns returned
  `None` from `project_control_run` and got dropped, while the real
  inline controls leaked to the end-of-paragraph drain. The
  observable symptom on Wave 12i's two-dutmals-with-space fixture
  was the body space `<hp:t> </hp:t>` getting pulled in front of
  both dutmals (`한국어 韓字` → `한국어韓字`). The structural
  projection path was unaffected because it already separated those
  controls into `marker_headers` vs. `object_controls` queues. The
  flat path now applies the same filter so its FFFC iterator only
  sees object controls. Any first-section paragraph that combines
  `secd` / `cold` with **any** inline shape (rect, polygon,
  ellipse, image, table, equation, dutmal) is covered, not just
  dutmal. See `.docs/algorithms/2026-06-01_dutmal_carry.md`
  (companion-fix section) for the full root-cause + rationale.

#### Added / Changed — Wave 12j (compose carry + char_pr_ids fidelity)

- Carry compose (글자겹침) annotations end-to-end through the HWP5 leg
  — `Hwp5ComposeControl` schema struct, `tcps` CtrlHeader decode,
  `Hwp5Control::Compose` variant, and a `project_compose_run` that
  emits `Control::Compose` with circleType / composeType mapped from
  raw enum bytes to the OWPML attribute strings (14 `SHAPECIRCLETYPE`
  values + 2 `COMPOSETYPE` values, including the spec-typo
  `SHAPE_REVERSAL_TIRANGLE`). HWPX→Core and Core→HWPX both already
  existed prior to this wave; only HWP5→Core was missing and `tcps`
  was silently dropped to `Hwp5Control::Unknown`.
- New `Control::Compose { char_pr_ids: Vec<u32>, … }` field (Core API
  breaking, semver-deliberate). HWPX schema fixes `<hp:compose
  charPrCnt>` at 10, but the existing encoder hard-coded all 10 slots
  to `u32::MAX` ("no override" sentinel) and the existing decoder
  discarded `<hp:charPr prIDRef>` children entirely. The new field
  threads the 10 LE u32 charPr IDs verbatim through Core so a `HWPX
  → Core → HWPX` round-trip preserves which slots carry a real
  `prIDRef` override (e.g. `7`) vs. the `u32::MAX` placeholder.
  Encoder pads / truncates to 10 slots.
- Compose wire layout discriminator. The `tcps` CtrlHeader payload
  has two empirically-observed forms, discriminated by the low half
  of `properties` (`data[4..6]` as LE u16):
  - `0x0003` (unpacked) — `composeText` is fully in `data[8..]`,
    body trailer carries the 4 metadata bytes + 10 × u32 charPrs.
    27 of 28 round-tripped variants and every native 한컴 fixture
    use this layout.
  - `0x0002` (packed) — `composeText[0]` is in `properties[2..4]`,
    `composeText[1..N]` is at the start of the body; the body
    trailer layout is unchanged. Observed exclusively on the
    `CHAR + OVERLAP` combination when 한컴 saved an HWPX → HWP5.
    The parser detects this via `properties.low == 0x0002` and
    prepends the packed char to the body region before decoding.
- Any other `properties.low` value falls through to
  `Hwp5Control::Unknown`. No clamp; no guessing. See
  `.docs/algorithms/2026-06-01_compose_carry.md` for the full
  discriminator table, the shape-glyph table for `properties.high`,
  and the validation policy rationale (Codex-reviewed).
- The packed variant was discovered only through a 14 × 2 = 28
  `circleType × composeType` combinatorial fixture
  (`gen_compose_variants` example) — single-shape native fixtures
  never trigger it. The lesson is captured in
  `.docs/learnings/2026-06-01_hwp5_ctrl_header_properties_overloaded.md`:
  HWP5's CtrlHeader `properties` word is **not** a generic bitfield —
  each ctrl_id can repurpose it for ctrl-specific data, and the same
  ctrl can switch layouts within a single document.

### Phase 11 (HWP5 → HWPX silent-gap closure)

This release closes the largest batch of "HWP5 decoder has the bytes, but
projection or shared model can't carry them" gaps measured against truth
HWPX fixtures from 한컴 Office. All work is HWP5-leg only — HWPX
encode/decode paths were already verified by existing golden tests.

#### Added — Wave 0 (audit infrastructure)

- `audit-hwp5` warning taxonomy (`SilentGap`, `DroppedControl`, …) and
  `--strict` mode so silently dropped semantics are surfaced as build
  signals instead of accumulating as invisible technical debt.

#### Added — Wave 1 (character style line family + word break)

- Carry `underline` (1b) and `strikeout` (1c) line family (`DOT`, `DASH`,
  `DOT_DASH`, etc.) through `HwpxCharShape` instead of collapsing to
  `SOLID`.
- Carry `breakLatinWord = HYPHENATION` (1d) through `WordBreakType` and
  HWP5 projection so HWPX `breakLatinWord` is no longer always
  `KEEP_WORD`.
- Surface silent `shadow` decode gap as warning (1a) until carry lands.

#### Added — Wave 2 (paragraph layout fidelity)

- Carry `lineSpacingType = AtLeast` (2a).
- Verify all 6 alignment variants (2b), `indent` + `pageBreakBefore`
  (2cd), and `border` + `shading` (2e) carry against truth HWPX
  fixtures.

#### Added — Wave 3 (paragraph-level checked decode)

- HWP5 paragraph-level `paraPr.checked` decode so the third location of
  checkable-bullet truth (the per-item checked state) is no longer lost.
  Closes the legacy R1 line.

#### Added — Wave 4 (Field / Object parity)

- Wave 4a: carry HWP5 `Rect` control through Core → HWPX.
- Wave 4b: carry HWP5 footnote control through Core → HWPX.
- Wave 4c: carry HWP5 chart (OLE-backed BinData) end-to-end as
  `Control::EmbeddedChart` passthrough — emits `Chart/chartN.xml` +
  `BinData/oleN.ole` + `<hp:switch>` block. Closes the
  `DroppedControl:ole_object` measurement.
- Carry HWP5 fixed-width space and non-breaking space through Core to
  HWPX inline text.
- Carry HWP5 checkable bullet `checkedChar` + `paraHead.checkable`
  through bullet conversion.
- Tab fidelity end-to-end: carry inline `<hp:tab width / leader / type>`
  attributes through a new `RunContent::InlineText` variant; HWPX
  encoder/decoder updated symmetrically; fill-type → leader mapping
  rebuilt against openhwp truth. See debug doc
  `.docs/debug/2026-05-26_tab_fidelity_bugs.md` and
  `.docs/debug/2026-05-27_hwpx_decoder_inline_tab_attrs_lost.md`.
- Field controls: emit non-zero `fieldid` for `CROSSREF` and keep
  `fieldBegin id` within signed 32-bit range.

#### Added — Wave 5 (page-level features)

- Wave 5 gap A: carry per-ctrl `applyPageType` (`BOTH` / `ODD` / `EVEN`)
  through HWP5 `head` / `foot` ctrl property word into multiple
  `<hp:header>` / `<hp:footer>` elements. Verified against
  `sample-header-footer-odd-even` truth fixture.
- Wave 5 gap B: carry `secd` ctrl property bits (0/1/2/5/19) into
  `Section.visibility.hide_first_header / footer / page_num /
  empty_line / master_page`. Verified against
  `sample-header-footer-hide-first` truth fixture. New
  `crates/hwpforge-smithy-hwp5/examples/probe_secd.rs` reusable probe.
- Wave 5 gap C (`masterPage` carry): **deferred**. Diagnosed as
  fixture-asymmetric on macOS 한컴 (truth HWPX has `masterpage0.xml`
  but the paired HWP5 has no master-page sub-records). See debug doc
  `.docs/debug/2026-05-27_hwp5_page_features_lost.md` for full probe
  results. Resume when a PC-한컴 fixture is available.

#### Fixed — Wave 6 (corpus-driven conversion robustness)

Measured by extending the `audit-hwp5` signal source from synthetic
fixtures to the real government-document corpus and clustering the
pre-categorized conversion failures. The two real bugs recovered all
29 `hwp5_convert_failed` documents (plus one more from `탈락`); the
remaining failures are inputs that are genuinely not HWP5.

- Schemeless hyperlink URLs (e.g. `www.motie.go.kr`) are normalized to
  `http://` instead of aborting the whole conversion. The HWPX encoder
  previously rejected any URL outside the `http://` / `https://` /
  `mailto:` allowlist; explicit unsafe schemes (`javascript:`, `data:`,
  `file:`, …) are still rejected. (16 corpus docs)
- Non-leading table header rows are demoted to normal rows (with a
  warning) in HWP5 projection instead of failing Core validation with
  `NonLeadingTableHeaderRow`. Real 한글 tables sometimes restate a
  header row mid-table; the leading header block is preserved and the
  stray header row is demoted so the document still converts.
  (14 corpus docs)
- `.hwp` inputs that are actually ZIP (`PK..`, i.e. an HWPX saved with a
  `.hwp` extension) or a Hancom secured/DRM container (`SCDS..`) now
  fail with an actionable message instead of a raw CFB byte dump.

### Changed — Breaking

- **ADR-002**: `hwpforge_core::Section.header: Option<HeaderFooter>` →
  `Section.headers: Vec<HeaderFooter>` (and `footer` → `footers`). This
  changes the public `Section` shape so that multiple page-type-scoped
  headers (`ODD` / `EVEN` / `BOTH`) can coexist as required by HWPX
  wire format. Empty `Vec` means "no header". JSON dump now emits a
  list instead of an object (or omits when empty). Patch / MD encoder
  / CLI / MCP consumers updated to iterate the slot. See
  `.docs/architecture/adr/ADR-002-section-multi-header-footer-cardinality.md`.

### Added — Public API

- `hwpforge_core::RunContent::InlineText(InlineText)` variant for
  mixed-content runs that include `<hp:tab>` (and is forward-compatible
  with `<hp:lineBreak>` / `<hp:nbSpace>` / `<hp:fwSpace>`). `InlineText`
  / `InlineSegment` / `InlineTabAttr` are `#[non_exhaustive]`.
- `Section` constructors continue to be `new()` / `with_paragraphs()`;
  push headers/footers via `section.headers.push(HeaderFooter::…)`.

### Migration

- Replace `section.header = Some(hf)` with `section.headers.push(hf)`.
- Replace `section.header.as_ref()` with
  `section.headers.first()` (single-slot consumers) or iterate
  `section.headers` (general).
- `serde` shape now uses `headers` / `footers` arrays; persisted JSON
  using the old keys must be migrated.
- Match arms on `RunContent` may need a new `InlineText(_)` arm. Use
  `RunContent::carries_text()` / `plain_text()` for read-only consumers
  that don't care about inline attribute fidelity.

## [0.5.2] - 2026-05-13

### Added

- `hwpforge` CLI gains `to-md` lossy / lossless modes for
  HWPX → Markdown export choice.
- HWP5 → HWPX projection preserves field controls and checkable state
  (precursor to Phase 11 Wave 3/4 closure).

### Fixed

- Dependency hygiene updates (`sha2`, GitHub Actions pinning, suppress
  `RUSTSEC-2026-0097` non-applicable advisory).

## [0.5.1] - 2026-04-13

### Added

- HWP5 → HWPX style fidelity bridge improvements (more char/para style
  surface preserved end-to-end).
- HWPX char effects: preserve `emboss`, `engrave`, `superscript`,
  `subscript` (also covered later under Wave 1 audit).

### Fixed

- Warn on conflicting vertical-position bits instead of silently
  normalizing.
- HWP5 paragraph layout hints (linesegarray, safe table height) carried
  to HWPX so visual diff matches truth better.
- `convert-hwp5` / `audit-hwp5` / `inspect` summary share the same
  style-projection warning source.

## [0.5.0] - 2026-03-20

### Changed — Breaking

- Adopt shared `ordered` / `bullet` / `outline` list semantics across
  `core`, `blueprint`, and `smithy-hwpx`. Markdown bridge integrated.
- Add **checkable bullet** semantics (HWPX `heading(type="BULLET")` +
  `bullet.checkedChar` + `bullet.paraHead.checkable` +
  `paraPr.checked`). Markdown task lists normalize to this shared HWP
  semantic; ordered task lists intentionally drop numbering on the way
  in.
- Tighten bullet semantics: paragraph `heading_level` is no longer a
  catch-all for list semantics (see gotcha #7 in `CLAUDE.md`).

### Added

- Markdown bridge: preserve task list continuation paragraphs;
  normalize ordered task lists.
- Fixtures: reorganized under `examples/` and `tests/fixtures/`.

### Fixed

- HWPX style id bridging for registry-local style ids.
- Outline contract hardening (golden tests).

## [0.4.0] - 2026-03-20

### Changed

- Promote the workspace release line to `0.4.0` for the breaking tab semantics contract in `hwpforge-core` and `hwpforge-blueprint`.
- Add shared tab-stop semantics across the IR stack so HWPX/HWP5 codecs can preserve explicit tab definitions and paragraph tab references.

### Migration

- `hwpforge_core::TabDef` now includes explicit `stops`; downstream struct literals must initialize the new field.
- `hwpforge_blueprint::Template`, `ParaShape`, and `PartialParaShape` now carry tab definition references/collections directly.
- Consumers matching on `BlueprintErrorCode` should handle the new tab-related error codes explicitly.

## [0.3.0] - 2026-03-19

### Changed

- Promote the workspace release line to `0.3.0` for the breaking HWPX section editing contract update.
- Preserve-first section editing now requires preservation metadata on `ExportedSection` and rejects stale or legacy section exports explicitly.

## [0.2.0] - 2026-03-17

### Changed

- Adopt the `hwpforge-core` v0.2.0 public DOM contract for richer table and image semantics.
- Align the workspace release line and internal crate pins on `0.2.0`.

### Migration

- `Table`, `TableRow`, `TableCell`, and `Image` are now `#[non_exhaustive]` and should be constructed via `new`/`with_*` builders instead of struct literals.
- Table DOM now carries page-break, repeat-header, cell-spacing, border/fill, header-row, cell margin, and vertical-alignment semantics directly in `hwpforge-core`.
- Image DOM now carries placement metadata directly in `hwpforge-core`.
- Validation now exposes `CoreErrorCode::NonLeadingTableHeaderRow`; downstream code that inspects validation codes should handle it explicitly.

## [0.1.0] - 2026-03-06

### Added

- **hwpforge**: Umbrella crate with feature flags (`hwpx`, `md`, `full`)
- **hwpforge-foundation**: Primitive types (HwpUnit, Color BGR, branded Index<T>, enums, error codes)
- **hwpforge-core**: Format-independent document model with typestate validation (Draft/Validated)
  - Document, Section, Paragraph, Run, Table, Image
  - Controls: TextBox, Footnote, Endnote, Equation, Chart (18 types)
  - Shapes: Line, Ellipse, Polygon, Arc, Curve, ConnectLine
  - References: Bookmark, CrossRef, Field, Memo, IndexMark
  - Layout: Multi-column, captions, headers/footers, page numbers, master pages
  - Annotations: Dutmal, compose characters
- **hwpforge-blueprint**: YAML-based style template system
  - Template inheritance with DFS merge
  - StyleRegistry with deduplicated fonts, char shapes, para shapes
  - Built-in default template (Hancom 한컴바탕)
  - BorderFill support
- **hwpforge-smithy-hwpx**: Full HWPX codec (KS X 6101)
  - Decoder: HWPX ZIP+XML -> Core Document
  - Encoder: Core Document -> HWPX ZIP+XML
  - Lossless roundtrip for all supported content
  - HancomStyleSet support (Classic/Modern/Latest)
  - 22 default styles with per-style charPr/paraPr
  - ZIP bomb defense (50MB/500MB/10k limits)
  - OOXML chart generation (18 chart types)
  - Golden fixture tests with real Hancom 한글 files
- **hwpforge-smithy-md**: Markdown codec
  - GFM decoder (pulldown-cmark) with YAML frontmatter
  - Lossy encoder (readable GFM) and lossless encoder (HTML+YAML)
  - Full pipeline: MD -> Core -> HWPX verified in Hancom 한글

[0.1.0]: https://github.com/ai-screams/HwpForge/releases/tag/v0.1.0
