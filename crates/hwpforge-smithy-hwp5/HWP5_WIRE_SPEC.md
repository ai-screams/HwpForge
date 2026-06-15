<!-- Parent: AGENTS.md -->

# HWP5 Wire Spec (HwpForge-internal)

> Code-grounded snapshot: 2026-06-11 (Wave 12q + #86 hardening)
>
> Scope: only what HwpForge's HWP5 decoder/projection currently reads and writes.
> Not a substitute for the official KS X 6101 / Hancom HWP 5.0 reference —
> this document records the **deltas, gotchas, and field-layout discoveries**
> that the Wave 12 series produced through hex-inspection of native 한컴 fixtures.

---

## Purpose

The published HWP 5.0 binary spec is incomplete in several places that matter
for round-trip fidelity:

- ParaHeader `[18..22]` carries an `instance_id` that the spec omits (Wave 12p).
- CtrlHeader payload offset for instance ID is **family-dependent** (fn/en at 16,
  gso/tbl/eqed at 36) — undocumented (Wave 12p Step 5).
- Outline heading level bits cap at 3-bit ordinal (0..=6); levels 7~9 live in
  `Style` record name "개요 N" (Wave 12q).
- ParaShape `property1` outline bits 25-27 are **zero-based** (Wave 12p #121).
- Auto-field SUMMERY `editable` attribute is per-FieldType, not universally `1`
  (Wave 12p #124).
- HWPTAG_BEGIN offset (+16) for BodyText tags is in the spec but the most common
  parser-side mistake (gotcha #1).

This file captures those discoveries with their **source file + line** so future
contributors can verify rather than rediscover.

---

## 1. Container — OLE2 CFB + DEFLATE

- `.hwp` files are OLE2 Compound File Binary (Microsoft CFB).
- Streams of interest:
  - `FileHeader` (256 bytes, never compressed)
  - `DocInfo`, `BodyText/Section{N}`, `Scripts/...`, `DocOptions/...`,
    `BinData/...`, `\x05HwpSummaryInformation` (PropertySet, not record stream)
- Stream compression: **DEFLATE**, no zlib header (raw deflate). Fall back
  to zlib if raw deflate fails (Hancom occasionally emits zlib-wrapped streams).
- Security defaults applied at unpack time: 500MB output cap, 100× max
  compression ratio (decompression bomb guard).

Implementation: `decoder/package.rs`.

---

## 2. Record Encoding (TLV)

All record streams except `FileHeader` and `\x05HwpSummaryInformation` are a
sequence of records:

```
+-- 4-byte packed word (LE) -------------------+--- optional 4-byte size --+
| tag_id (10 bits) | level (10) | size (12)    |  size_ext (if size==0xFFF) |
+----------------------------------------------+----------------------------+
| data (size bytes)                                                         |
+----------------------------------------------+----------------------------+
```

- `tag_id`: 10 bits → max 0x3FF (1023)
- `level`: 10 bits → nesting depth, 0 = top-level
- `size`: 12 bits → if `0xFFF`, the next 4 bytes are the actual size (extended)

Implementation: `schema/record.rs::RecordHeader::parse`.

---

## 3. Tag ID +16 Offset (Gotcha #1)

The KS X 6101 spec documents BodyText tags as `HWPTAG_BEGIN + offset`, where
`HWPTAG_BEGIN = 0x10` (16 decimal). For example:

| Spec name            | Documented value    | Actual byte value |
| -------------------- | ------------------- | ----------------- |
| `HWPTAG_PARA_HEADER` | `HWPTAG_BEGIN + 50` | `0x42` (66)       |
| `HWPTAG_PARA_TEXT`   | `HWPTAG_BEGIN + 51` | `0x43` (67)       |
| `HWPTAG_CTRL_HEADER` | `HWPTAG_BEGIN + 55` | `0x47` (71)       |
| `HWPTAG_MEMO_LIST`   | `HWPTAG_BEGIN + 77` | `0x5D` (93)       |

Our `TagId` enum stores **actual byte values**, not the spec offsets.
This is the most common parser bug — checking `tag_id == 50` instead of
`tag_id == 0x42` silently misses every paragraph in the file.

Implementation: `schema/record.rs::TagId` (comment in source preserves the
`+16 from documented offsets` note).

---

## 4. Tag ID Ranges

| Range        | Stream   | Notes                                      |
| ------------ | -------- | ------------------------------------------ |
| `0x10..0x3F` | DocInfo  | `is_doc_info()`                            |
| `0x42..0x73` | BodyText | `is_body_text()` (note gap at `0x40,0x41`) |

DocInfo and BodyText tags **share the same 10-bit namespace** but never
collide because their value ranges are disjoint.

---

## 5. ParaHeader (`0x42`) — 22-byte Base + 2-byte Optional

```
[0..4]   char_count       u32 LE   # UTF-16 code units
[4..8]   control_mask     u32 LE
[8..10]  para_shape_id    u16 LE
[10]     style_id         u8
[11]     page_break /
         divide_sort      u8
[12..14] char_shape_count u16 LE
[14..16] range_tag_count  u16 LE
[16..18] line_seg_count   u16 LE
[18..22] instance_id      u32 LE   # ← Wave 12p discovery
[22..24] is_merged_by_track u16 LE # v5.0.3.2+ only (optional)
```

### Wave 12p (Step 1) — `instance_id` at `[18..22]`

The spec docs treat `[18..22]` as reserved. **Native 한컴 fixtures carry
a unique per-paragraph `instance_id` here** — used by HWPX cross-ref
`Command` of the form `?#<id>` to look up an Outline-target paragraph.

Pattern observed in native files: `instance_id = (0x42_xx << 16) | counter`,
where the high-16 byte is a session prefix and the low-16 is a monotonic
counter. HwpForge preserves the raw u32 value rather than trying to
reconstruct the prefix scheme.

Source: `schema/section.rs::Hwp5ParaHeader::parse`.

---

## 6. CtrlHeader (`0x47`) — Family-Aware Payload

The first 4 bytes of every CtrlHeader payload are the `ctrl_id` (LE u32 in
the stream, but conventionally read as BE-ASCII so e.g. `0x7462_6C20` reads
as `"tbl "`).

After the `ctrl_id`, the payload layout **depends on the family**:

### 6.1 Family-aware `instance_id` offset (Wave 12p Step 5)

| `ctrl_id`              | Family                | `instance_id` offset |
| ---------------------- | --------------------- | -------------------- |
| `0x666E_2020` (`fn`)   | Footnote              | `data[16..20]`       |
| `0x656E_2020` (`en`)   | Endnote               | `data[16..20]`       |
| `0x6773_6F20` (`gso`)  | Drawing shape         | `data[36..40]`       |
| `0x7462_6C20` (`tbl`)  | Table                 | `data[36..40]`       |
| `0x6571_6564` (`eqed`) | Equation              | `data[36..40]`       |
| (others)               | head/foot/secd/cold/… | absent (use `0`)     |

Source: `decoder/section.rs::extract_ctrl_header_instance_id`.

**This was not in any reference** — we discovered it by hex-searching
BodyText for HWPX `target_id` values, then dumping `±32` bytes around
each match (`HWPFORGE_DEBUG_TRAILER=1` probe).

### 6.2 Practical implication

HWPX `<hp:footNote instId="...">` / `<hp:pic id="...">` etc. round-trip
correctly only if you read the instance_id from the family-correct
offset. Reading offset 36 for a footnote returns garbage.

---

## 7. ParaShape `property1` Bit Layout

HWP5 packs paragraph properties into a single u32. The bits HwpForge
currently reads:

| Bits  | Meaning                                            | Notes                                |
| ----- | -------------------------------------------------- | ------------------------------------ |
| 2-4   | alignment (LEFT/CENTER/RIGHT/JUSTIFY/…)            | 3-bit enum                           |
| 23-24 | list family (`numbered`/`bullet`/`outline`/`none`) | 2-bit                                |
| 25-27 | outline level                                      | **3-bit ZERO-BASED ordinal (cap 6)** |

### Wave 12p #121 — Outline level off-by-1

`Hwp5RawParaShape::heading_level()` originally applied `saturating_sub(1)`,
treating bits 25-27 as 1-based. **Native fixtures prove bits 25-27 are
zero-based**: an Outline level-1 paragraph stores `0b000`, level-7 stores
`0b110`. The correct extraction is:

```rust
pub fn heading_level(&self) -> u8 {
    ((self.property1 >> 25) & 0b111) as u8  // 0..=6
}
```

### Wave 12q (#122) — Outline levels 7~9 via Style "개요 N"

3 bits can only express ordinals 0..=6 — so HWP5 cannot store levels 7,
8, or 9 in `property1` directly. **Native 한컴 represents level 7~9 in
the `Style` record's Korean name "개요 N" (or English "Outline N")**,
where `N-1` is the HWPX level.

Implementation: `style_store.rs::apply_outline_style_level_overrides` runs
silently after the base ParaShape conversion. Pattern match against
`"개요 N"` / `"Outline N"` style names; if `heading_type == Outline`,
override the paraPr's `heading_level` to `N - 1`. Codex(architect) §5
"순차 ID 신앙 금지" 호환: paraPr id 자체에 의존하지 않고
`heading_type == Outline` 인 paraPr 만 override.

HWPX-side: no `hp10` namespace switch wrapping needed — 한컴 reads
levels 7/8/9 directly without `hp10` (visually verified on native
`sample-outline-9levels.hwp` with 10 levels).

---

## 8. Inline Control Characters in `ParaText`

`ParaText` (`0x43`) is a flat UTF-16LE stream. Code points `0x00..0x1F`
are control markers — most carry **14 bytes (7 u16) of inline `extra`
metadata** immediately following the marker code point.

| Marker       | Meaning                                  | `extra` |
| ------------ | ---------------------------------------- | ------- |
| `0x02`       | Section/column definition boundary       | 14 B    |
| `0x03`       | Field begin                              | 14 B    |
| `0x04`       | Field end                                | 14 B    |
| `0x09`       | Tab (carries width/leader/type — Wave 4) | 14 B    |
| `0x0A`       | Soft line break                          | none    |
| `0x0B`       | Object/control ref (most common)         | 14 B    |
| `0x0C`       | Extended control ref                     | 14 B    |
| `0x0D`       | Paragraph break                          | none    |
| `0x0E..0x15` | Various extended control markers         | 14 B    |
| `0x16`       | IndexMark inline marker (Wave 12k)       | 14 B    |
| `0x18`       | (other)                                  | varies  |

The `extra[0..4]` slot inside a 14-byte block usually carries an LE-stored
ctrl_id. Read it as **BE-ASCII** to identify the marker family:

```rust
fn ctrl_id_from_inline_extra_bytes(extra: &[u8; 14]) -> u32 {
    u32::from_be_bytes([extra[3], extra[2], extra[1], extra[0]])
}
```

Source: `schema/section.rs::ctrl_id_from_inline_extra_bytes`.

---

## 9. CTRL_ID Magic Constants (Wave 12 series)

`ctrl_id` is a BE-ASCII 4-byte tag — a **type discriminator**, not an
instance ID. The schema crate stores them as `u32` (BE-ASCII numeric value).

| Constant                  | Hex           | ASCII  | Wave     |
| ------------------------- | ------------- | ------ | -------- |
| `CTRL_ID_TABLE`           | `0x7462_6C20` | `tbl`  | Phase 10 |
| `CTRL_ID_HEADER`          | `0x6865_6164` | `head` | Wave 5   |
| `CTRL_ID_FOOTER`          | `0x666F_6F74` | `foot` | Wave 5   |
| `CTRL_ID_SECD`            | `0x7365_6364` | `secd` | Wave 5   |
| `CTRL_ID_FOOTNOTE`        | `0x666E_2020` | `fn`   | Wave 4   |
| `CTRL_ID_ENDNOTE`         | `0x656E_2020` | `en`   | Wave 4   |
| `CTRL_ID_GSO`             | `0x6773_6F20` | `gso`  | Wave 12a |
| `CTRL_ID_EQED`            | `0x6571_6564` | `eqed` | Wave 12d |
| `CTRL_ID_MEMO`            | `0x2575_6E6B` | `%unk` | Wave 12e |
| `CTRL_ID_DUTMAL`          | `0x7464_7574` | `tdut` | Wave 12i |
| `CTRL_ID_COMPOSE`         | `0x7463_7073` | `tcps` | Wave 12j |
| `CTRL_ID_INDEXMARK`       | `0x6964_786D` | `idxm` | Wave 12k |
| `CTRL_ID_CLICK_HERE`      | `0x2563_6C6B` | `%clk` | Wave 12l |
| `CTRL_ID_FIELD_SUMMERY`   | `0x2573_6D72` | `%smr` | Wave 12n |
| `CTRL_ID_FIELD_DATE_CODE` | `0x2564_7465` | `%dte` | Wave 12n |
| `CTRL_ID_FIELD_PATH`      | `0x2570_6174` | `%pat` | Wave 12n |
| `CTRL_ID_FIELD_CROSSREF`  | `0x2578_7266` | `%xrf` | Wave 12m |
| `CTRL_ID_ATNO`            | `0x6174_6E6F` | `atno` | Wave 12n |
| `CTRL_ID_HYPERLINK`       | `0x2568_6C6B` | `%hlk` | early    |
| `CTRL_ID_BOOKMARK_SPAN`   | `0x2562_6D6B` | `%bmk` | early    |
| `CTRL_ID_BOOKMARK_POINT`  | `0x626F_6B6D` | `bokm` | early    |
| `CTRL_ID_COLUMN_DEF`      | `0x636F_6C64` | `cold` | early    |
| `CTRL_ID_PAGE_NUMBER`     | `0x7067_6E70` | `pgnp` | early    |
| `CTRL_ID_MEMO_INLINE`     | `0x2525_6D65` | `%%me` | Wave 12e |

**Note**: these constants live in `crate::ctrl_ids` (single source of
truth as of task #94). Names are canonicalised to wire-name-first
(`SECD`, `ATNO`) or `FIELD_*` family for `%`-class auto fields; the
Step B1 backward-compat aliases (`SECTION_DEF`, `CROSSREF`,
`FIELD_INLINE_PAGE`, `INDEXMARK_INLINE`, `INLINE_AUTONUM`) were removed
in Step B2.

Naming conventions observed:

- `%xxx` (`0x25...`): "field" / "command" family (date, path, summary, click-here,
  crossref, hyperlink, bookmark-span, memo)
- 4-letter all-ASCII (`tbl`, `head`, `gso`, `eqed`, …): structural object
- `t` prefix (`tdut`, `tcps`): typography family
- `%unk` (`0x2575_6E6B`): Memo with a sentinel-looking name, **not** unknown.

---

## 10. GSO (`gso`) Shape Sub-records

A `gso` CtrlHeader is followed by **nested sub-records** at deeper `level`s.
The schema parses these by family:

| Sub-tag | Hex                        | Family                                                                              | Wave     |
| ------- | -------------------------- | ----------------------------------------------------------------------------------- | -------- |
| `0x4C`  | LIST_HEADER → `ListHeader` | Shape component header                                                              | Phase 10 |
| `0x4E`  | `ShapeComponentLine`       | line / connect-line discriminated by leading 4-byte type tag (`$col` = ConnectLine) | Wave 12b |
| `0x4F`  | `ShapeComponentRect`       | rectangle                                                                           | Phase 10 |
| `0x50`  | `ShapeComponentEllipse`    | ellipse + arc (arc inferred via `hasArcPr=1`)                                       | Wave 12a |
| `0x51`  | `ShapeComponentArc`        | (rarely emitted directly; usually inferred from `0x50`)                             | Wave 12a |
| `0x52`  | `ShapeComponentPolygon`    | polygon (close with first vertex repeated)                                          | Phase 10 |
| `0x53`  | `ShapeComponentCurve`      | curve / spline                                                                      | Wave 12a |
| `0x54`  | `ShapeComponentOle`        | OLE container (Chart, embed)                                                        | Wave 4c  |
| `0x55`  | `ShapePicture`             | picture / image                                                                     | early    |
| `0x56`  | `ShapeContainer`           | group container                                                                     | early    |
| `0x5A`  | `ShapeTextArt`             | TextArt (글맵시) — see §10.1                                                        | TextArt  |

ConnectLine vs Line (both `0x4E`): differentiated by the leading 4-byte
type tag on the parent `ShapeComponent` (`0x4C`) — `"$col"` discriminator
for ConnectLine.

> **`0x56 ShapeContainer` is a dead `TagId` entry** — 한컴 encodes a group as a
> `ShapeComponent` (`0x4C`) with the `"$con"` type tag, NOT `0x56`.
> **`0x5A ShapeTextArt` IS used** (TextArt), nested under a `ShapeComponent`
> (`0x4C`) carrying the `"$tat"` type tag — see §10.1.

### 10.1 TextArt (`<hp:textart>`, comp_type `"$tat"` + `0x5A`)

TextArt (글맵시 — warped decorative text, like WordArt) is a `gso` shape whose
`ShapeComponent` (`0x4C`) carries comp_type `"$tat"` (raw bytes
`74 61 74 24`) wrapping a `ShapeTextArt` (`0x5A`) sub-record:

```
gso CtrlHeader (0x47 'gso ')               ← offset / size geometry
  └─ ShapeComponent (0x4C, comp_type "$tat")  ← shape-common block
       └─ ShapeTextArt (0x5A)                  ← textart payload
```

**`0x5A` payload layout** (little-endian; parsed by `Hwp5ShapeTextArt::parse`):

| bytes     | field                                                  |
| --------- | ------------------------------------------------------ |
| `[0..32]` | `pt0..pt3` — four `(i32 x, i32 y)` corner points       |
| BSTR      | `text` (u16 unit-count prefix + UTF-16LE body)         |
| BSTR      | `font_name`                                            |
| BSTR      | `font_style` (e.g. `"보통"`)                           |
| `u32`     | `font_type` (`1` = TTF)                                |
| `u32`     | `text_shape` (`0..=54` enum — see table below)         |
| `u32`     | `line_spacing` (percent)                               |
| `u32`     | `char_spacing` (percent)                               |
| `u32`     | `align` (`0` = LEFT)                                   |
| `[+20]`   | shadow (type / offsetX / offsetY) + reserved (ignored) |

The HWPX `<hp:textartPr scaMatrix>` (the visual squish) is **not** read from
the wire — it is computed as `curSz / orgSz` (`width/14173`, `height/14173`),
matching native byte-for-byte.

**`text_shape` enum** (HWP5 integer = grid reading-order position; HWPX string
read from native `.hwpx`): `0` PARALLELOGRAM, `1` INVERTED_PARALLELOGRAM,
`2` INVERTED_UPWARD_CASCADE, `3` INVERTED_DOWNWARD_CASCADE, `4` UPWARD_CASCADE,
`5` DOWNWARD_CASCADE, `6` REDUCE_RIGHT, `7` REDUCE_LEFT, `8` ISOSCELES_TRAPEZOID,
`9` INVERTED_ISOSCELES_TRAPEZOID, `10` TOP_RIBBON_RECTANGLE,
`11` BOTTOM_RIBBON_RECTANGLE, `12` CHEVRON_DOWN, `13` CHEVRON, `14` BOW_TIE,
`15` HEXAGON, `16` WAVE1, `17` WAVE2, `18` WAVE3, `19` WAVE4,
`20` LEFT_TILT_CYLINDER, `21` RIGHT_TILT_CYLINDER, `22` BOTTOM_WIDE_CYLINDER,
`23` TOP_WIDE_CYLINDER, `24` THIN_CURVE_UP1, `25` THIN_CURVE_UP2,
`26` THIN_CURVE_DOWN1, `27` THIN_CURVE_DOWN2, `28` INVERSED_FINGERNAIL,
`29` FINGERNAIL, `30` GINKO_LEAF1, `31` GINKO_LEAF2, `32` INFLATE_RIGHT,
`33` INFLATE_LEFT, `34` INFLATE_UP_CONVEX, `35` INFLATE_BOTTOM_CONVEX,
`36` DEFLATE_TOP, `37` DEFLATE_BOTTOM, `38` DEFLATE, `39` INFLATE,
`40` INFLATE_TOP, `41` INFLATE_BOTTOM, `42` RECTANGLE, `43` LEFT_CYLINDER,
`44` CYLINDER, `45` RIGHT_CYLINDER, `46` CIRCLE, `47` CURVE_DOWN, `48` ARCH_UP,
`49` ARCH_DOWN, `50` SINGLE_LINE_CIRCLE1, `51` SINGLE_LINE_CIRCLE2,
`52` TRIPLE_LINE_CIRCLE1, `53` TRIPLE_LINE_CIRCLE2, `54` DOUBLE_LINE_CIRCLE.
(Source: `TEXTART_SHAPE_NAMES` in `schema/section.rs`, derived by pairing a
native 56-textart fixture's wire integers with the 한컴-emitted HWPX strings.)

### Group / 묶음 객체 (`<hp:container>`, Wave A flat / Wave B nested)

A group is NOT a distinct tag. It is a `ShapeComponent` (`0x4C`) whose
4-byte comp_type discriminator is `"$con"` (same mechanism as `"$col"` /
`"$rec"` / `"$ell"`). Its **child shapes are deeper-level `ShapeComponent`
records**, each carrying its own shape sub-record (`0x4F`/`0x50`/…) + optional
`LIST_HEADER` + `PARA_HEADER` drawText. Children may themselves be `"$con"`
(nested group, `$con`-in-`$con`) — **Wave B** recurses these to arbitrary
depth (up to `GSO_GROUP_MAX_DEPTH`, beyond which a nested group degrades to
`Unknown` with a warning).

The decoder routes this with a scope stack (`GsoGroupBuilder` /
`GsoChildBuilder`, modeled on `table_stack`), because the gso scope was a
flat single-shape state machine (`classify_gso_control` requires
`payload_count == 1`). The live child is a `GsoActiveChild` enum:
`Leaf(GsoChildBuilder)` for a flat shape, or `Nested(Box<GsoGroupBuilder>)`
for a nested `$con` (boxed to break the `GsoGroupBuilder → GsoActiveChild →
GsoGroupBuilder` type cycle). A nested child's records recurse into its own
builder; the parent's depth-based close rule is identical for both arms.

Recursion threads through all four layers: projection
(`project_group_child` → `project_group_run` on a nested group),
layout-hint collection (`collect_group_child_layout_hints` recurses so the
inner group's drawText `<hp:p>` get hints — otherwise the section patcher
underflows), HWPX encoder (`encode_group_child_xml` early-returns into
`encode_group_to_xml`, with `groupLevel` incremented +1 per nesting), and
HWPX decoder (`HxContainer.containers: Vec<HxContainer>` +
`decode_container` recursion under a `MAX_NESTING_DEPTH` = 32 guard).

**Child geometry layout** (group-relative, from the child's own
`ShapeComponent` common header — NOT the gso CtrlHeader, which children lack):

| bytes      | field                  |
| ---------- | ---------------------- |
| `[0..4]`   | comp_type (`$rec`/…)   |
| `[4..8]`   | x offset (i32 LE)      |
| `[8..12]`  | y offset (i32 LE)      |
| `[12..16]` | grouping/version flags |
| `[16..20]` | width (u32, orgSz)     |
| `[20..24]` | height (u32, orgSz)    |

Source: `Hwp5ShapeComponentGeometry::parse_from_shape_component`. Derived by
correlating native `sample-gso-group.hwp` child bytes with the 한컴-emitted
`<hp:offset>`/`<hp:orgSz>` in the `.hwpx` pair.

**HWPX positioning gotcha**: 한컴 places a `<hp:container>` child by its
`<hc:transMatrix>` **translation** (`e3` = x, `e6` = y), NOT by `<hp:offset>`
(native carries both with the same value; an identity matrix renders every
child at the group origin → overlap). Container children also omit the
top-level `<hp:sz>`/`<hp:pos>` and emit `<hp:curSz>` `0×0` (size from
`<hp:orgSz>`). The container itself owns the single `<hp:sz>`/`<hp:pos>`.

---

## 11. Equation (`eqed`) + `EQEDIT` (`0x58`)

`eqed` CtrlHeader emits the equation's shape metadata. The script (LaTeX-like
HWP equation syntax) lives in a **separate** `HWPTAG_EQEDIT` (`0x58`) record
that follows the eqed CtrlHeader at the same level.

Pairing logic: the decoder buffers a `pending_eqed` state when `eqed` is
seen, then attaches the next `EQEDIT` record's script payload. The same
pattern is used for `%unk` MEMO (paired with `MEMO_LIST` 0x5D, Wave 12e)
and `%clk` ClickHere (paired with `CtrlData` 0x57 at `lvl=2`, Wave 12l).

---

## 12. Memo Cluster (`%unk` + `HWPTAG_MEMO_LIST` 0x5D)

The Memo control uses ctrl_id `0x2575_6E6B` (`%unk`) — **literally the
ASCII string "unk"** with `%` prefix. Despite the name, this is the
canonical Memo discriminator, not a fallback for unknown ctrls.

Anchor positioning (Wave 12f): inline marker is `CTRL_ID_MEMO_INLINE`
(`0x2525_6D65`, "%%me"), distinct from the CtrlHeader id. The inline
marker positions the memo anchor; the CtrlHeader+MemoList carry the
content.

7 parameters carried as `<hp:parameters>` (Wave 12h):
`editable`, `dirty`, `zorder`, `field_id`, `begin_id_ref`, `name`,
`meta_tag`.

---

## 13. BSTR (Length-Prefixed UTF-16) Field Commands

ClickHere/Auto fields encode their command + name as **back-to-back
length-prefixed UTF-16LE strings** sometimes called "split-leader BSTR":

```
[len_u16:2][utf16_units:2*len][len_u16:2][utf16_units:2*len]
```

- First BSTR = `command` (e.g. `Clickhere:set:N:`, `Format=`, …)
- Second BSTR = `name` (display label / hint)

ClickHere (`%clk`, Wave 12l) additionally pairs with a sub-record
`0x57` (`CtrlData`) at `lvl=2` for trailing metadata. The same BSTR
encoding is used by Compose (`tcps`), Dutmal (`tdut`), and IndexMark
(`idxm`) — refactor task #95 (split-leader BSTR helper).

### Dutmal (`tdut`) tail words (task #73)

After the split-leader `main_text` + length-prefixed `sub_text`, the
`tdut` payload carries five u32 LE tail words. Offsets pinned by the
one-knob-per-paragraph `sample-dutmal-variants.hwp` fixture
(`probe_dutmal_tail` diff vs baseline):

| tail offset | field      | values observed                                             |
| ----------- | ---------- | ----------------------------------------------------------- |
| `[0..4]`    | `pos_type` | 0=TOP, 1=BOTTOM (2/3 = RIGHT/LEFT per projection mapping)   |
| `[4..8]`    | `sz_ratio` | percent; 0=auto (한컴 renders auto ≈50%)                    |
| `[8..12]`   | `option`   | mirrored verbatim (semantics unpinned)                      |
| `[12..16]`  | reserved   | constant 0 (styleIDRef candidate — unattributed)            |
| `[16..20]`  | `align`    | **1=LEFT, 2=RIGHT, 3=CENTER** — note CENTER is `3`, not `0` |

Unknown align codes project to CENTER with a `ProjectionFallback`
warning. Source: `schema/section.rs::Hwp5DutmalControl`.

### Compose (`tcps`) layouts + decoration glyph (task #74)

The full 14-circleType × 2-composeType matrix
(`sample-compose-all-shapes.hwp`, `probe_compose_variants`) confirmed
exactly **two** layouts discriminated by `properties.low`:

- `0x0003` (unpacked) — text fully in body; `properties.high` holds a
  decoration glyph that is a pure 1:1 function of the body-trailer
  `circleType`: CHAR→U+3000, ◯●□■△▲☼◇◆▢♲♺♻ for the 13 shape kinds
  in OWPML enum order. No independent information → decoder ignores it.
- `0x0002` (packed) — `composeText[0]` in `properties.high`; observed
  on exactly one matrix cell (`CHAR + OVERLAP`).

No third layout exists, so an unknown discriminator is genuinely
malformed. Source: `schema/section.rs::Hwp5ComposeControl`.

### IndexMark (`idxm`) surrogate split + trailer (task #75)

The split-leader packing **does split surrogate pairs**: an
emoji-leading primary stores the high surrogate alone in
`properties.high` and the low surrogate as the first body unit
(`sample-indexmark-surrogate.hwp`, `probe_indexmark_surrogate`:
`emoji-first` → header `U+D83D`, body starts `00 DE`).
`parse_split_leader_utf16` collects all units before UTF-16
validation, so the pair reassembles correctly — verified end-to-end
(all five variants round-trip with emoji intact, zero warnings).

The trailing 4 bytes remain constant sentinels on every observation
(`0x00000000` on HWPX→HWP5 round-trip saves, `0xFFFFFFFF` on
hand-authored native) — no varied information, no HWPX carrier
element, and no HWP5 re-encoder to feed; raw carry stays deferred
until a re-encoder exists (YAGNI, now backed by observation).

### Allocation caps (defensive, Wave 12 + #86)

Length prefixes are `u16` (0..65535). Without a cap, a malicious file
could trigger 128KB allocations per BSTR. HwpForge enforces:

| Constant                      | Cap (u16 units) | Effective bytes |
| ----------------------------- | --------------- | --------------- |
| `MAX_CLICKHERE_COMMAND_UNITS` | 32 * 1024       | 64 KB           |
| `MAX_CLICKHERE_NAME_UNITS`    | 2 * 1024        | 4 KB            |
| `MAX_SUMMERY_COMMAND_UNITS`   | 1024            | 2 KB            |
| `MAX_DATECODE_COMMAND_UNITS`  | 1024            | 2 KB            |
| `MAX_DUTMAL_TEXT_UNITS`       | 1024            | 2 KB            |
| `MAX_INDEXMARK_KEY_UNITS`     | 1024            | 2 KB            |

Checks run **before** `Vec::with_capacity` to prevent OOM-by-allocation,
not just OOM-by-fill. See `schema/section.rs` for each cap site.

---

## 14. Cross-Reference Command Wire (`%xrf`)

CrossRef Command is an 8-parameter Hancom-canonical format, **not** the
5-parameter form sometimes shown in spec excerpts. Layout:

```
?<target>;N1;N2;N3;N4;<Fiexde>;<Prop>;<Command>;
```

- `target` — semantic object key (e.g. `pageNum`, `figureNum`, bookmark name)
- `N1..N4` — Hancom internal counters / target IDs
- `Fiexde`, `Prop`, `Command` — Hancom-specific control fields

Wave 12m Phase 2 elevated CrossRef from a lossy surrogate to a typed
`Hwp5CrossRefControl`. See ADR-004 for the wire-format archaeology.

### RefContentType semantic note

`ContentType` is **interpreted relative to `RefType`**:

| `RefType` | `ContentType=Contents` means |
| --------- | ---------------------------- |
| Bookmark  | bookmark **name** (display)  |
| Figure    | caption **body text**        |
| Table     | caption **body text**        |
| Footnote  | footnote **body text**       |

Do **not** invent a flat `BookmarkName` enum or treat `Contents` as a
universal "body" semantic — it's overloaded by `RefType`.

---

## 15. Style Record (`0x1A`) — Outline Name Encoding

The Style record carries a UTF-16LE name (and English/Korean variants).
Hancom encodes outline-level intent in the **Korean** name:

| Style name pattern    | HWPX level | Notes                   |
| --------------------- | ---------- | ----------------------- |
| `개요 1`              | 0          | "Outline 1"             |
| `개요 2`              | 1          |                         |
| ...                   | ...        |                         |
| `개요 9`              | 8          |                         |
| `개요 10`             | 9          | (10 levels total)       |
| `Outline 1` (English) | 0          | English fallback (rare) |

Levels 1~7 are also expressible via ParaShape bits 25-27 (zero-based);
levels 8~10 are **only** expressible via Style name. Hancom is consistent
about this — never mixes the two for the same paragraph.

---

## 16. SummaryInformation (`\x05HwpSummaryInformation`)

Standard Microsoft Office **PropertySet** stream — not the HWP5 TLV
record format. Layout:

```
[BOM_LE: 2 bytes 0xFFFE]
[version: 2 bytes]
[os_marker: 4 bytes]
[clsid: 16 bytes]
[section_count: 4 bytes]   # HwpForge accepts only 1 (cap)
[section entries...]
[property table + values]
```

VT types HwpForge handles:

| VT            | Hex      | Meaning                                   |
| ------------- | -------- | ----------------------------------------- |
| `VT_LPSTR`    | `0x001E` | length-prefixed ASCII                     |
| `VT_LPWSTR`   | `0x001F` | length-prefixed UTF-16LE                  |
| `VT_FILETIME` | `0x0040` | Windows FILETIME (100ns since 1601-01-01) |

### Hancom custom PIDs

Standard PropertySet PIDs (Title=2, Author=4, Keywords=5, Comments=6,
LastSavedBy=8, CreatedTime=12, LastSavedTime=13) are joined by Hancom
custom PIDs:

| PID    | Meaning               |
| ------ | --------------------- |
| `0x14` | Hancom custom field 1 |
| `0x15` | Hancom custom field 2 |

FILETIME → ISO 8601 conversion is hand-rolled (no `chrono` dependency
in foundation).

### Defenses (Wave 12o)

- Section count cap (>1 rejected) — defeats classic PropertySet payload-cycle DoS
- Property offset monotonicity check — defeats cyclic-reference DoS
- UTF-16LE BOM strip (Hancom emits BOM in `VT_LPWSTR` payloads sometimes)
- `sec_start + 8` bounds check uses `checked_add` (32-bit/wasm panic guard,
  Wave 12o-fixup Top-1)

Source: `schema/summary_info.rs`.

---

## 17. SUMMERY Field `editable` (Wave 12n / #124)

The HWPX `<hp:fieldBegin>` element has an `editable` attribute. Earlier
HwpForge emitted `"1"` universally; **native 한컴 emits per-FieldType**:

| FieldType      | `editable` |
| -------------- | ---------- |
| `Author`       | `"0"`      |
| `Title`        | `"0"`      |
| `LastSavedBy`  | `"1"`      |
| `CreatedTime`  | `"1"`      |
| `ModifiedTime` | `"1"`      |
| `ClickHere`    | `"1"`      |

Implementation: `foundation/enums.rs::FieldType::hwpx_editable()`.

Note: SUMMERY (a typo in the original spec — meant "SUMMARY") is the
HWPX `type=` attribute for these document-metadata fields. We preserve
the misspelling for byte-identical compatibility.

### Field body cached value (#120/#136)

SUMMERY/PATH fields carry their **resolved value** in the body between
`<hp:fieldBegin>` and `<hp:fieldEnd>` (e.g. `<hp:t>hanyul</hp:t>`, the
locale-formatted date, the absolute path). An **empty** body triggers
한컴's "낮은 보안 수준 복구" warning on open + leaves a blank placeholder
until the user saves.

The value is present in the HWP5 source `BodyText/Section0` ParaText, in
the FieldBegin..FieldEnd span. Projection accumulates it into
`ActiveField::{SummeryField,DateCodeField,PathField}.display_text`
(capped at `MAX_FIELD_DISPLAY_TEXT_UNITS = 4096` — body text bypasses the
BSTR command caps), carries it on the matching `Control` variant, and the
HWPX encoder emits it as the body `<hp:t>`.

> **Supersedes Wave 12n Step 6.6.** That step emitted an _empty_ body
> after observing a _synthesized_ ISO date (`2026-06-06`) — locale-
> mismatched against 한컴's `2026년 6월 …` — get rejected as corrupted
> content. But an empty body **also** triggers the warning (#120 stayed
> open). Byte-diff + 한컴 실측 (2026-06-13) proved native 한컴 carries the
> verbatim locale value and opens cleanly; carrying the HWP5 source's own
> cached render (NOT a synthesized value) reproduces that. A trailing
> `<hp:t/>` _after_ `fieldEnd` is a separate, still-required element.

한컴 recomputes editable fields (`editable="1"`: LastSavedBy/CreatedTime/
ModifiedTime/PATH) on save, so the carried value is the cached render at
HWP5 save time — exactly what native 한컴 HWPX itself stores.

---

## 18. `linesegarray` Synthesis (Wave 12p #123)

HWPX expects every paragraph to have a `<hp:linesegarray>` element.
HWP5 paragraphs that lack a `ParaLineSeg` (`0x45`) sub-record (common
for empty / page-break paragraphs) must still emit a default lineseg
or 한컴 raises "낮은 보안 수준 복구" warning at open time.

HwpForge fills missing linesegs with a synthetic default:

```
vertsize    = 1000
textheight  = 1000
baseline    = 850
spacing     = 600
horzsize    = 42520
flags       = 393216    (0x6_0000)
```

These values match what 한컴 itself emits for empty paragraphs.

Source: `layout_hint_patch.rs::write_linesegarray`.

---

## 19. Reference Map (where the wire knowledge lives)

| Topic                            | File / Symbol                                                        |
| -------------------------------- | -------------------------------------------------------------------- |
| Record header / TagId            | `schema/record.rs`                                                   |
| ParaHeader + `instance_id`       | `schema/section.rs::Hwp5ParaHeader`                                  |
| CtrlHeader family-aware offsets  | `decoder/section.rs::extract_ctrl_header_instance_id`                |
| Inline `0x02..0x16` markers      | `schema/section.rs::Hwp5ParaText::parse`                             |
| CTRL_ID constants                | `decoder/section.rs`, `projection.rs`, `schema/section.rs`           |
| GSO sub-record parsing           | `decoder/section.rs::NestedSubtreeContext`                           |
| Eqed/EqEdit pairing              | `decoder/section.rs` (`pending_eqed` state)                          |
| Memo cluster                     | `decoder/section.rs` (`Hwp5MemoControl`) + `projection.rs`           |
| ClickHere BSTR + allocation caps | `schema/section.rs::Hwp5ClickHereControl`                            |
| SUMMERY auto fields              | `schema/section.rs::Hwp5FieldSummeryControl` + `Hwp5DateCodeControl` |
| Dutmal / Compose / IndexMark     | `schema/section.rs` (split-leader BSTR — task #95)                   |
| CrossRef structured              | `schema/section.rs::Hwp5CrossRefControl` + ADR-004                   |
| Style outline name override      | `style_store.rs::apply_outline_style_level_overrides`                |
| SummaryInformation PropertySet   | `schema/summary_info.rs`                                             |
| Linesegarray synthesis           | `layout_hint_patch.rs::write_linesegarray`                           |

---

## 20. What This Document Is Not

- Not a complete HWP5 spec — many records (Bullet detail, Numbering inner
  layout, TabDef, BinData inner format, …) are read by HwpForge but not
  recorded here. Add when a Wave produces new wire-level discovery.
- Not a substitute for reading the source. When in doubt, the
  `parse()` methods are the truth.
- Not a roadmap. New discoveries land in `CHANGELOG.md` first.

---

## 21. Wave 12 Series Provenance

| Wave    | Discovery                                                                                                         |
| ------- | ----------------------------------------------------------------------------------------------------------------- |
| 12a     | GSO Ellipse/Arc/Curve sub-records (0x50/0x53)                                                                     |
| 12b     | ConnectLine `$col` discriminator on `ShapeComponentLine`                                                          |
| 12d     | `eqed` + `EQEDIT` (0x58) script pairing                                                                           |
| 12e     | Memo `%unk` + `HWPTAG_MEMO_LIST` (0x5D) cluster joining                                                           |
| 12f     | Memo inline marker `%%me` (0x2525_6D65) vs CtrlHeader `%unk`                                                      |
| 12h     | Memo 7-parameter `<hp:parameters>` carry                                                                          |
| 12i     | Dutmal `tdut` split-leader BSTR + `option_raw`                                                                    |
| #73     | Dutmal tail words pinned — `sz_ratio` tail[4..8], `align` tail[16..20] (CENTER=3)                                 |
| #74     | Compose full-matrix probe — glyph = f(circleType) 1:1, packed layout only on CHAR+OVERLAP, no third variant       |
| #75     | IndexMark split-leader splits surrogate pairs (high in header, low leads body); trailer stays a constant sentinel |
| 12j     | Compose `tcps` packed variant + `char_pr_ids` fidelity                                                            |
| 12k     | IndexMark `idxm` `0x16` inline marker + sub-record `0x57` pairing                                                 |
| 12l     | ClickHere `%clk` BSTR + `0x57 lvl=2` (`CtrlData`) sub-record pairing                                              |
| 12m     | CrossRef `%xrf` 8-parameter Hancom-canonical wire + ADR-004                                                       |
| 12n     | Auto fields `%smr`/`%dte`/`%pat`/`atno` discriminated by ctrl_id                                                  |
| 12o     | SummaryInformation OLE2 PropertySet (BOM, VT types, Hancom PIDs)                                                  |
| 12p     | ParaHeader `[18..22]` instance_id + family-aware CtrlHeader offsets                                               |
| 12p#121 | ParaShape bits 25-27 are zero-based ordinal                                                                       |
| 12p#123 | Default `linesegarray` synthesis for paragraphs lacking 0x45                                                      |
| 12p#124 | SUMMERY `editable` per-FieldType                                                                                  |
| 12q     | Style "개요 N" override for outline levels 7~9                                                                    |

---

## 22. Update Protocol

When a new wave discovers wire-level behavior:

1. Land the fix with code + tests.
2. Add a row to the relevant section here, **with the source file/symbol path**.
3. Update the Wave Provenance table.
4. Mention this file in the CHANGELOG entry: `(see HWP5_WIRE_SPEC.md §N)`.

The goal is that future me / future contributors can grep this file before
re-running probes.
