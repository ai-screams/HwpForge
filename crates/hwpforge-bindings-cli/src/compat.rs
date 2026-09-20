//! Compatibility layer between `hwpforge::ops` / `hwpforge_convert::ops` and
//! the CLI's frozen error contract (`code`, `hint`, exit code, `--json`
//! schema — `docs/planning` W3 `common.md` §"Contract that must NOT
//! change").
//!
//! `hwpforge::ops` classifies every failure as an [`OpsCode`] wire string
//! (`hwpforge_foundation::diagnostics::OpsCode`) — a table the CLI's own
//! vocabulary seeded (most codes are identical strings). [`cli_error`] and
//! [`convert_error`] translate an `ops`/`hwpforge_convert::ops` result back
//! onto the CLI's pre-migration spelling so replacing a command's direct
//! `smithy-*` calls with an `ops` call is not a wire-format break.
//!
//! # What is frozen and what is not
//!
//! The **code**, **hint** and **exit code** this module emits are the
//! frozen contract. `tests/data/legacy_codes.txt` is the hand-audited
//! snapshot of every `(command, code, exit, hint)` shape
//! `src/commands/*.rs`/`src/error.rs` produced before this migration, and
//! this module's `#[cfg(test)]` inventory (`mod tests`) checks every row
//! [`TABLE`] claims against it. The **message** is not frozen (`common.md`
//! W3 brief §"Contract that must NOT change" point 3 lists message-body
//! changes separately) — [`cli_error`]/[`convert_error`] default to the
//! wrapped error's own `Display`, except the semantic-loss reconstructions
//! below, which pin the exact legacy wording because a downstream test
//! (`hwpforge-smithy-hwpx`'s R1 F4 regression pins, referenced from
//! `set_cell.rs`/`stamp.rs`) already depends on it byte-for-byte.
//!
//! # Design: no silent fallback
//!
//! Every row in [`TABLE`] is explicit — [`Hint::None`] is a *frozen* legacy
//! "no hint at all" value, never "unset, ask `err.hint()`". A W6b audit
//! comparison script found ~20 `(command, code)` pairs where the code has
//! an `hwpforge::ops::hint_for` entry but the legacy call site genuinely
//! had no hint (every non-`Inspect` `DecodeFailed` row, both structural
//! `InputNotRoundtripSafe`/`InputEntriesNotCarried` pairs, `Read`'s and
//! `Stamp`'s own `TableGridInvalid`, …) — a blanket "`None` asks `ops`"
//! rule would silently *add* a hint to every one of them. `hwpforge::ops`'s
//! own `hint_for` table disagrees with the legacy CLI in both directions
//! anyway — it adds hints commands like `fields`/`outline`/`diff` never
//! had, and it phrases `PRESET_NOT_FOUND` in Korean where `templates`'s
//! legacy hint is English — so [`Hint::Literal`] rows (kept verbatim
//! because `hint_for` disagrees or has no entry for that code) never fall
//! through either.
//!
//! The same comparison also found 13 rows whose legacy literal is
//! byte-identical to `hint_for`'s own text for that code (`Inspect`'s
//! `DecodeFailed`, every `Fill`/`SetCell` hint below that also appears in
//! `hint_for`, five of `Stamp`'s). Those are [`Hint::FromOps`]: instead of
//! spelling the same string a third time, [`resolved_hint`] asks
//! `err.hint()` for them — same output, one fewer place the wording can
//! drift. The inventory's fidelity test ([`tests::every_table_row_matches_a_real_snapshot_shape`])
//! is what makes this safe: it resolves every row (including
//! `Hint::FromOps` ones) through [`resolved_hint`] and checks the result
//! against the audited snapshot, so a future `hint_for` wording change
//! that would silently alter one of these 13 rows' CLI output fails there.
//!
//! The one place this module falls through unconditionally
//! (`code.as_str()` + `err.hint()`) is for an `OpsCode` that has **no
//! legacy row at all** — a new code `ops` can reach that the pre-migration
//! CLI never emitted (for example `convert`'s
//! `ASSET_PLAN_MISMATCH`/`ASSET_IDENTITY_CONFLICT`, which the old
//! hand-rolled image-embed path never distinguished as a hard error). That
//! is new coverage, not a divergence to hide — and it is the same
//! `err.hint()` call `Hint::FromOps` makes, just with no `TABLE` row to
//! carry a legacy code string either.
//!
//! A handful of frontend-neutral candidates this comparison surfaced
//! (`ToJson`/`JsonSerializeFailed`'s "Check for NaN/Infinity values in
//! chart data", `Patch`'s `PatchFailed` hint) were deliberately **not**
//! moved into `hint_for`: `hwpforge::ops` is shared with the Python
//! bindings, which this lane did not audit, and adding a `hint_for` entry
//! changes what `OpsError::hint()` returns for every caller, not just this
//! one. Left as `Hint::Literal` pending a lane that can verify the
//! Python-bindings ripple.
//!
//! # Exit codes vary by command, not by code
//!
//! `set-cell`'s `exit_cell_edit_error` hard-codes exit 1 for *every* code it
//! emits, including its own `SET_CELL_CODEC_FAILED` (which would be a
//! codec/exit-2 class error everywhere else). `structural.rs`'s
//! `write_output` uses exit 2 for `FILE_WRITE_FAILED` while every other
//! command uses exit 1. `to-pdf`'s `INVALID_DISCOVERY` is exit 2 despite
//! being an argument rejection (the class that is exit 1 everywhere else).
//! A `default_exit(code)` heuristic gets these wrong silently; [`Row::exit`]
//! is therefore the *only* source of truth, looked up per `(command, code)`
//! by [`exit_code`], never derived from the code alone.
//!
//! # `convert_error` needs the command (deviation from the W3 brief)
//!
//! `lane-compat.md` specifies `pub fn convert_error(err:
//! hwpforge_convert::ops::ConvertOpsError) -> CliError`, but that signature
//! cannot hold the contract: `convert-hwp5` and `to-pdf` disagree on the
//! same [`ConvertOpsError`] variants they can both reach.
//! [`ConvertOpsError::Decode`] is `DECODE_FAILED` (`ops`'s own code) for
//! `to-pdf`'s own decode of an HWPX package, but the legacy CLI called it
//! `HWPX_DECODE_FAILED` there — a code `convert-hwp5` cannot even
//! reach (it never decodes HWPX). Both commands **coarsen** the HWP5
//! decode/convert split the underlying `ops` call can report: the legacy
//! CLI's single `hwp5_to_hwpx_with_options` (`convert-hwp5`) /
//! `hwp5_to_hwpx_bytes_with_options` (`to-pdf`) call site mapped every
//! failure it returned — decode-stage included — to one code
//! (`HWP5_CONVERT_FAILED`), so both [`OpsCode::Hwp5DecodeFailed`] and
//! [`OpsCode::Hwp5ConvertFailed`] collapse to that one string for *both*
//! [`Command::ConvertHwp5`] and [`Command::ToPdf`] (see the `TABLE` rows
//! below; W3 remediation finding 7 — an earlier lane had kept `convert-hwp5`'s
//! `Hwp5DecodeFailed` on its own `HWP5_DECODE_FAILED` code, which only the
//! CLI-local `inspect_hwp5_file` pre-check ever legitimately returns —
//! `commands/convert_hwp5.rs` constructs that one directly and never
//! reaches this table). Their `Hwp5ConvertFailed` hints still differ
//! though (`convert-hwp5`'s legacy hint names the *output path*, `to-pdf`'s
//! call site had no hint at all). This module therefore takes `cmd:
//! Command` on [`convert_error`], and the report for this lane calls the
//! deviation out explicitly rather than silently keeping the brief's
//! narrower signature and losing the per-command split.
//!
//! # Table vs. special cases
//!
//! Most rows are static: a `(Command, OpsCode)` pair determines a legacy
//! code, hint and exit. A few legacy hints embed per-call data (an
//! available-fields list, a section count, a table's cell count, a
//! conditional anchor coordinate) that no flat row can hold; those are
//! matched on the *wrapped* library error directly, before the table is
//! consulted, in [`cli_error`]. Two codes are **dual-source** — the same
//! `OpsCode` is produced by both a text-spec rejection (`StampError`,
//! inside `StamperError::Stamp`) and a cell-spec rejection
//! (`CellStampError`, inside `StamperError::CellStamp`) — and the legacy
//! hint differs by origin; those are matched the same way.
//!
//! # Semantic loss keeps its pre-`ops` legacy shape
//!
//! `set_cell` and `stamp` never reported `ENCODE_SEMANTIC_LOSS`: before
//! `CellEditError`/`StamperError` grew a typed `SemanticLoss` variant, both
//! commands' `Codec`/generic arm printed the first semantic warning's
//! `Display` verbatim (source comments in both `set_cell.rs`
//! (`exit_cell_edit_error`) and `stamp.rs` (`exit_stamper_error`) mark this
//! "R1 F4 — output contract stays the same"). [`OpsError::EncodeSemanticLoss`]
//! is matched **before** the table for both commands and reconstructs that
//! exact legacy code/exit (`SET_CELL_CODEC_FAILED`/1,
//! `STAMP_CODEC_FAILED`/2) from the first carried warning.
//!
//! # `ops` gaps this lane found (reported, not worked around)
//!
//! - **`inspect` `ANALYSIS_FAILED`** — the legacy CLI's `summarize_hwpx_document`
//!   deep-count analysis (`crate::analysis::deep_counts`) could fail with
//!   `ANALYSIS_FAILED`/exit 2; `hwpforge::ops::inspect`'s own `# Errors` doc
//!   names only `DECODE_FAILED`. Whichever lane wires `inspect` either keeps
//!   calling `summarize_hwpx_document` alongside `ops::inspect` for this one
//!   failure mode, or accepts the behaviour loss — this module cannot
//!   synthesize a code `ops` never returns.
//! - **`from-json` `JSON_PARSE_FAILED` hint — resolved at the call site, not
//!   here.** The legacy CLI had two call sites sharing this code: a raw
//!   `serde_json::from_str` failure (no hint) and a schema-mismatch
//!   `Deserialize` failure (hint: "Ensure the JSON matches the HwpForge
//!   document schema…"). `ops::from_json` wraps both in the same
//!   [`OpsError::Json`] variant (`serde_json::Error`), with no way to tell
//!   them apart from inside `ops` or from this table alone. `TABLE` keeps
//!   the no-hint shape; `from_json.rs` restores the classification instead
//!   by pre-checking the raw JSON syntax before calling `ops::from_json` —
//!   a syntax failure exits right there (byte-identical), so any
//!   `JSON_PARSE_FAILED` this module still returns for `Command::FromJson`
//!   is necessarily the schema case, and `from_json.rs` attaches the legacy
//!   hint to it before exiting.
//! - **`to-json` `SECTION_INDEX_MISMATCH`/`PATCH_FAILED`/`SECTION_WORKFLOW_FAILED`
//!   — not a gap, a finding.** The legacy `to_json.rs`'s
//!   `exit_section_workflow_error` shares its match arms verbatim with
//!   `patch.rs`'s copy, so it has arms for all four [`SectionWorkflowError`]
//!   cases even though `ops::exchange::export_section`'s own `# Errors` doc
//!   names only two reachable ones (`DECODE_FAILED`, `SECTION_OUT_OF_RANGE`)
//!   — `--section` export never patches, so it can never disagree with a
//!   JSON's own `section_index` or fail preservation. These three codes were
//!   already dead in the pre-migration CLI; `TABLE` carries no row for them
//!   (`tests/data/legacy_codes.txt`'s inventory test's `LEGACY_UNREACHABLE_VIA_OPS`
//!   list accounts for the snapshot rows instead).
//! - **`patch`/`delete-para`/`insert-para`/`set-cell`/`stamp`'s
//!   default-arm codes collapse to `UpstreamUnmapped`.** `patch`'s
//!   `SECTION_WORKFLOW_FAILED`, `delete-para`/`insert-para`'s
//!   `STRUCTURAL_EDIT_FAILED`, and `set-cell`/`stamp`'s
//!   `SET_CELL_FAILED`/`STAMP_FAILED` are each the legacy default arm for an
//!   unrecognized upstream variant. `hwpforge::ops`'s own classifiers
//!   (`section_workflow_code`, `structural_code`, `cell_edit_code`,
//!   `stamper_code` in `crates/hwpforge/src/ops/mod.rs`) fold *their* own
//!   unrecognized-variant fallback into the single generic
//!   [`OpsCode::UpstreamUnmapped`], losing which command's default-arm
//!   string it should become. Every upstream error variant these commands
//!   can reach *today* has its own dedicated code (unlike `to-json` above,
//!   `patch`'s doc genuinely lists `SectionWorkflowError::PreservingPatch`
//!   as reachable — only a *fifth*, currently nonexistent variant would hit
//!   this), so `UpstreamUnmapped` is unreachable in practice for all five;
//!   a genuinely new upstream variant later would need this module to match
//!   on the *wrapped* error type, not just `OpsCode`, to fully restore the
//!   old per-command default-arm string. `TABLE` still carries a
//!   best-effort row per command for the day that matters.

use hwpforge::ops::OpsError;
use hwpforge_convert::ops::ConvertOpsError;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwpx::stamp::{CellStampError, StampError, StamperError};
use hwpforge_smithy_hwpx::{CellEditError, FillError, SectionWorkflowError};

use crate::error::{CliError, ErrorCause};

/// Which of the 20 migratable `hwpforge` subcommands is reporting the
/// error. `audit-hwp5` and `census-hwp5` are analysis commands with no
/// `ops` counterpart (`common.md` W3 brief) and are not represented here.
/// [`Command::Validate`] is a 21st, later addition — not one of the 20
/// migratable commands (there was no legacy `validate` to migrate; see its
/// own module docs) — kept in this enum anyway so [`cli_error`]/
/// [`exit_code`] cover it uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// `convert-hwp5` (`hwpforge_convert::ops::convert_hwp5`).
    ConvertHwp5,
    /// `to-pdf` (`hwpforge_convert::ops::to_pdf`).
    ToPdf,
    /// `convert` (Markdown → HWPX, `ops::convert_md`).
    Convert,
    /// `inspect`.
    Inspect,
    /// `to-json` (whole-document and `--section` export).
    ToJson,
    /// `from-json`.
    FromJson,
    /// `outline`.
    Outline,
    /// `diff`.
    Diff,
    /// `delete-para`.
    DeletePara,
    /// `insert-para`.
    InsertPara,
    /// `read`.
    Read,
    /// `fields`.
    Fields,
    /// `fill`.
    Fill,
    /// `set-cell`.
    SetCell,
    /// `stamp-plan`.
    StampPlan,
    /// `stamp`.
    Stamp,
    /// `patch`.
    Patch,
    /// `templates show` (`templates list` never fails — `ops::templates` is
    /// infallible).
    Templates,
    /// `schema`. Never constructed at runtime — the schema catalogue is a
    /// CLI artefact that stays local (module docs) — but kept so the snapshot
    /// inventory's `cmd_name` stays exhaustive over every subcommand.
    #[allow(dead_code, reason = "schema stays CLI-local; variant keeps the inventory exhaustive")]
    Schema,
    /// `to-md`.
    ToMd,
    /// `validate`. New command, no legacy precedent (module docs).
    Validate,
}

/// A row's frozen hint status (module docs' "Design: no silent fallback").
#[derive(Debug, Clone, Copy)]
enum Hint {
    /// Frozen: the legacy call site had no `.with_hint(...)` at all. Never
    /// asks `ops` for one, even where `hint_for` has an entry for this
    /// code — that would silently add a hint the legacy contract never had.
    None,
    /// Frozen: the legacy literal, kept verbatim because `hint_for` either
    /// has no entry for this code or its wording genuinely differs.
    Literal(&'static str),
    /// The legacy literal was byte-identical to `hint_for`'s own text for
    /// this code — resolved dynamically through [`resolved_hint`] instead
    /// of duplicating the string.
    FromOps,
}

/// One static compatibility-table row: for this `(cmd, code)` pair, emit
/// `legacy` as the code, `hint` (see [`Hint`] — frozen, never a silent
/// fallback) and `exit` as the process exit code.
struct Row {
    cmd: Command,
    code: OpsCode,
    legacy: &'static str,
    hint: Hint,
    exit: i32,
}

macro_rules! row {
    ($cmd:ident, $code:ident, $legacy:literal, $exit:literal) => {
        Row {
            cmd: Command::$cmd,
            code: OpsCode::$code,
            legacy: $legacy,
            hint: Hint::None,
            exit: $exit,
        }
    };
    ($cmd:ident, $code:ident, $legacy:literal, $exit:literal, ops) => {
        Row {
            cmd: Command::$cmd,
            code: OpsCode::$code,
            legacy: $legacy,
            hint: Hint::FromOps,
            exit: $exit,
        }
    };
    ($cmd:ident, $code:ident, $legacy:literal, $exit:literal, $hint:literal) => {
        Row {
            cmd: Command::$cmd,
            code: OpsCode::$code,
            legacy: $legacy,
            hint: Hint::Literal($hint),
            exit: $exit,
        }
    };
}

/// The hint [`cli_error`]/[`convert_error`] use for a resolved `hint`,
/// asking `ops` for [`Hint::FromOps`] through the exact same [`OpsError::hint`]/
/// [`ConvertOpsError::hint`] lookup production code already has to make
/// (`ops_hint` is that call's result) — never a second, hand-copied
/// `hint_for` table. The inventory fidelity test uses this too, so a
/// [`Hint::FromOps`] row is checked against the string production would
/// actually emit, not a hand-typed one.
#[must_use]
fn resolved_hint(hint: Hint, ops_hint: Option<&'static str>) -> Option<&'static str> {
    match hint {
        Hint::None => None,
        Hint::Literal(text) => Some(text),
        Hint::FromOps => ops_hint,
    }
}

/// The compatibility table. See the module docs for why every reachable
/// `(cmd, code)` pair gets an explicit row (no fallback for legacy-precedent
/// codes) and why `exit` cannot be derived from `code` alone.
#[rustfmt::skip]
const TABLE: &[Row] = &[
    // ── ConvertHwp5 (hwpforge_convert::ops::convert_hwp5) ────────────
    // `Hwp5DecodeFailed` here is the `convert_hwp5()` ops call's own decode
    // failure (e.g. a corrupt stream discovered mid-conversion), NOT the
    // CLI-local `inspect_hwp5_file` pre-check in `commands/convert_hwp5.rs`
    // (that one hardcodes `HWP5_DECODE_FAILED` directly and never reaches
    // this table). Legacy's single `hwp5_to_hwpx_with_options` call site
    // mapped every failure from it — decode-stage included — to
    // `HWP5_CONVERT_FAILED` (W3 remediation finding 7), matching `to-pdf`'s
    // own coarsening below.
    row!(ConvertHwp5, Hwp5DecodeFailed, "HWP5_CONVERT_FAILED", 2, "Check that the source is a supported HWP5 document and the output path is writable"),
    row!(ConvertHwp5, Hwp5ConvertFailed, "HWP5_CONVERT_FAILED", 2, "Check that the source is a supported HWP5 document and the output path is writable"),

    // ── ToPdf (hwpforge_convert::ops::to_pdf) ─────────────────────────
    // NOTE: legacy to-pdf mapped every HWP5-conversion failure (decode AND
    // convert stage) to one HWP5_CONVERT_FAILED — coarser than convert-hwp5.
    // Both ops codes collapse to that one string here (module docs).
    row!(ToPdf, Hwp5DecodeFailed, "HWP5_CONVERT_FAILED", 2),
    row!(ToPdf, Hwp5ConvertFailed, "HWP5_CONVERT_FAILED", 2),
    row!(ToPdf, DecodeFailed, "HWPX_DECODE_FAILED", 2),
    row!(ToPdf, ValidationFailed, "VALIDATION_FAILED", 2),
    row!(ToPdf, UnrecognizedFormat, "UNRECOGNIZED_FORMAT", 2, "to-pdf detects the format by content — the extension is only a hint"),
    row!(ToPdf, PdfRenderFailed, "PDF_RENDER_FAILED", 2, "cacheless documents need a Hancom re-save; font errors may need --font-dir/--discovery or --degraded"),
    row!(ToPdf, InvalidDiscovery, "INVALID_DISCOVERY", 2),

    // ── Convert (Markdown → HWPX, ops::convert_md) ────────────────────
    row!(Convert, PresetNotFound, "UNKNOWN_PRESET", 1, "Available presets: default"),
    row!(Convert, MdDecodeFailed, "MD_DECODE_FAILED", 2),
    row!(Convert, StyleStoreFailed, "STYLE_STORE_FAILED", 2),
    row!(Convert, StyleRebindFailed, "STYLE_REBIND_FAILED", 2),
    row!(Convert, ValidationFailed, "VALIDATION_FAILED", 2),
    row!(Convert, EncodeFailed, "ENCODE_FAILED", 2),

    // ── Inspect (ops::inspect) ─────────────────────────────────────────
    row!(Inspect, DecodeFailed, "DECODE_FAILED", 2, ops),
    // ANALYSIS_FAILED: ops gap, see module docs — no row, no ops code exists.

    // ── ToJson (ops::to_json / ops::export_section) ───────────────────
    row!(ToJson, DecodeFailed, "DECODE_FAILED", 2),
    row!(ToJson, JsonSerializeFailed, "JSON_SERIALIZE_FAILED", 2, "Check for NaN/Infinity values in chart data"),
    row!(ToJson, GridAddrProjectionFailed, "GRID_ADDR_PROJECTION_FAILED", 2),
    // SectionOutOfRange: dynamic hint, see cli_error.
    // No rows for SectionIndexMismatch/PatchFailed/the UpstreamUnmapped
    // catch-all ("SECTION_INDEX_MISMATCH"/"PATCH_FAILED"/"SECTION_WORKFLOW_FAILED"):
    // `ops::exchange::export_section`'s own `# Errors` doc names only two
    // reachable `SectionWorkflow` codes, `DECODE_FAILED` and
    // `SECTION_OUT_OF_RANGE` — the legacy `to_json.rs`'s
    // `exit_section_workflow_error` had these three extra arms only because
    // it shares its match verbatim with `patch.rs`'s copy (which genuinely
    // reaches all four), never because `--section` export could produce
    // them. Legacy dead code; see `tests/data/legacy_codes.txt` (both rows
    // are audited there and marked `CLI_LOCAL` in the inventory below for
    // exactly this reason, not because a frontend constructs them).

    // ── FromJson (ops::from_json) ──────────────────────────────────────
    // JSON_PARSE_FAILED: ops gap (dual legacy hint, one OpsError::Json
    // variant) — see module docs. Keeps the no-hint shape.
    row!(FromJson, JsonParseFailed, "JSON_PARSE_FAILED", 2),
    row!(FromJson, GridAddrInvalid, "GRID_ADDR_INVALID", 2, "Grid addresses come from to-json output; after structural edits, drop the stale addr fields (or re-export) and retry"),
    row!(FromJson, ValidationFailed, "VALIDATION_FAILED", 2),
    row!(FromJson, DecodeFailed, "DECODE_FAILED", 2),
    row!(FromJson, EncodeFailed, "ENCODE_FAILED", 2),

    // ── Outline (ops::read::outline) ───────────────────────────────────
    row!(Outline, DecodeFailed, "DECODE_FAILED", 2),

    // ── Diff (ops::diff) ────────────────────────────────────────────────
    row!(Diff, DecodeFailed, "DECODE_FAILED", 2),

    // ── DeletePara / InsertPara (ops::edit::{delete_para,insert_para}) ──
    row!(DeletePara, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(InsertPara, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(DeletePara, DeleteNoTarget, "DELETE_NO_TARGET", 1),
    row!(DeletePara, DuplicateTarget, "DUPLICATE_TARGET", 1),
    row!(DeletePara, ReferenceStranded, "REFERENCE_STRANDED", 1),
    row!(DeletePara, HardBreakLoss, "HARD_BREAK_LOSS", 1),
    row!(DeletePara, EmptySection, "EMPTY_SECTION", 1),
    // InsertTextRequired ("INSERT_TEXT_REQUIRED"): clap's
    // `#[arg(long = "text", required = true)]` already refuses an empty
    // `--text` list before `run_insert` is ever called, so the legacy CLI
    // never had a call site for this case — `ops::edit::insert_para`'s own
    // `# Errors` doc names it (`OpsError::Rejected`/`OpsCode::InsertTextRequired`)
    // for callers with no such clap guard (e.g. MCP). Given a row anyway
    // (audit finding — an unrouted new code silently fell through to
    // `exit_code`'s fallback, exit 2, the codec-failure class, though this
    // is an argument rejection): exit 1, the argument-error class every
    // other `insert-para`/`delete-para` row in this table uses. New code,
    // no legacy precedent to preserve (`tests/data/legacy_codes.txt`'s own
    // note on this pair).
    row!(InsertPara, InsertTextRequired, "INSERT_TEXT_REQUIRED", 1),
    row!(InsertPara, MultiParagraphText, "MULTI_PARAGRAPH_TEXT", 1),
    row!(InsertPara, InsertBeforeSectionProperties, "INSERT_BEFORE_SECTION_PROPERTIES", 1),
    // Shared between both commands (structural.rs's exit_structural_error
    // handles both delete_paragraphs and insert_paragraphs results).
    row!(DeletePara, SectionOutOfRange, "SECTION_OUT_OF_RANGE", 1),
    row!(InsertPara, SectionOutOfRange, "SECTION_OUT_OF_RANGE", 1),
    row!(DeletePara, ParagraphOutOfRange, "PARAGRAPH_OUT_OF_RANGE", 1),
    row!(InsertPara, ParagraphOutOfRange, "PARAGRAPH_OUT_OF_RANGE", 1),
    row!(DeletePara, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", 1),
    row!(InsertPara, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", 1),
    row!(DeletePara, InputEntriesNotCarried, "UNCARRIED_ZIP_ENTRIES", 1),
    row!(InsertPara, InputEntriesNotCarried, "UNCARRIED_ZIP_ENTRIES", 1),
    row!(DeletePara, SectionPropertiesParagraph, "SECTION_PROPERTIES_PARAGRAPH", 1),
    row!(InsertPara, SectionPropertiesParagraph, "SECTION_PROPERTIES_PARAGRAPH", 1),
    row!(DeletePara, SpanCountMismatch, "SPAN_COUNT_MISMATCH", 1),
    row!(InsertPara, SpanCountMismatch, "SPAN_COUNT_MISMATCH", 1),
    row!(DeletePara, SelfVerifyFailed, "SELF_VERIFY_FAILED", 1),
    row!(InsertPara, SelfVerifyFailed, "SELF_VERIFY_FAILED", 1),
    row!(DeletePara, StructuralCodec, "STRUCTURAL_CODEC", 2),
    row!(InsertPara, StructuralCodec, "STRUCTURAL_CODEC", 2),
    row!(DeletePara, UpstreamUnmapped, "STRUCTURAL_EDIT_FAILED", 1),
    row!(InsertPara, UpstreamUnmapped, "STRUCTURAL_EDIT_FAILED", 1),

    // ── Read (ops::read::read) ────────────────────────────────────────
    row!(Read, ReadTargetRequired, "READ_TARGET_REQUIRED", 1),
    row!(Read, ReadParasWithoutSection, "READ_PARAS_WITHOUT_SECTION", 1),
    row!(Read, ReadParasInvalid, "READ_PARAS_INVALID", 1),
    row!(Read, DecodeFailed, "DECODE_FAILED", 2),
    row!(Read, ReadSectionOutOfRange, "READ_SECTION_OUT_OF_RANGE", 1),
    row!(Read, ReadParaRangeInvalid, "READ_PARA_RANGE_INVALID", 1),
    row!(Read, ReadTableOutOfRange, "READ_TABLE_OUT_OF_RANGE", 1),
    row!(Read, TableGridInvalid, "TABLE_GRID_INVALID", 1),
    row!(Read, ReadFieldNotFound, "READ_FIELD_NOT_FOUND", 1),

    // ── Fields (ops::read::fields) ─────────────────────────────────────
    row!(Fields, DecodeFailed, "DECODE_FAILED", 2),

    // ── Fill (ops::edit::fill) ─────────────────────────────────────────
    row!(Fill, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(Fill, NoValues, "NO_VALUES", 1, "예: hwpforge fill doc.hwpx --set 과제명=\"AI 문서 자동화\" -o out.hwpx"),
    row!(Fill, EmptyFieldValue, "EMPTY_FIELD_VALUE", 1, ops),
    // FieldNotFound: dynamic hint, see cli_error.
    row!(Fill, FieldNameAmbiguous, "FIELD_NAME_AMBIGUOUS", 1, ops),
    row!(Fill, FieldNotFillable, "FIELD_NOT_FILLABLE", 1, ops),
    row!(Fill, FillFailed, "FILL_FAILED", 2),

    // ── SetCell (ops::edit::set_cell) ──────────────────────────────────
    // NOTE: legacy set-cell hard-codes exit 1 for every code, including its
    // own codec failure — the only command that does this (module docs).
    // DecodeFailed is the one exception: it is the shared decode stage,
    // not one of set-cell's own error variants, and had no row at all
    // before this one — its exit was always 2 through `exit_code`'s
    // fallback (pinned unchanged by `structural_commands_decode_failed_
    // output_is_unchanged_by_their_new_table_rows`).
    row!(SetCell, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(SetCell, InvalidSetCellArgs, "INVALID_SET_CELL_ARGS", 1),
    row!(SetCell, InvalidSetCellMap, "INVALID_SET_CELL_MAP", 1),
    // TableNotFound: always has a dynamic (table-count) hint, no flat shape — see cli_error.
    row!(SetCell, TableGridInvalid, "TABLE_GRID_INVALID", 1, ops),
    row!(SetCell, CellNotFound, "CELL_NOT_FOUND", 1),
    row!(SetCell, CellLabelAmbiguous, "CELL_LABEL_AMBIGUOUS", 1, "라벨이 여러 셀과 일치합니다 — --at 좌표로 직접 지정하세요"),
    row!(SetCell, CellHasNonTextContent, "CELL_HAS_NON_TEXT_CONTENT", 1, ops),
    row!(SetCell, CellTargetDuplicate, "CELL_TARGET_DUPLICATE", 1),
    row!(SetCell, CellTargetConflict, "CELL_TARGET_CONFLICT", 1, ops),
    row!(SetCell, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", 1, ops),
    row!(SetCell, InputEntriesNotCarried, "INPUT_ENTRIES_NOT_CARRIED", 1),
    row!(SetCell, SetCellCodecFailed, "SET_CELL_CODEC_FAILED", 1),
    row!(SetCell, UpstreamUnmapped, "SET_CELL_FAILED", 1),
    // EncodeSemanticLoss: reconstructed to SET_CELL_CODEC_FAILED/1, see cli_error.

    // ── StampPlan (ops::stamp::stamp_plan — narrow, see legacy_codes.txt) ─
    row!(StampPlan, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(StampPlan, StampCodecFailed, "STAMP_CODEC_FAILED", 2),

    // ── Stamp (ops::stamp::stamp) ───────────────────────────────────────
    row!(Stamp, DecodeFailed, "DECODE_FAILED", 2, ops),
    row!(Stamp, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", 1, "이 입력은 무손실 재인코드가 증명되지 않아 스탬핑을 거부합니다 (fail-closed). 코덱 갭 수정 또는 E4 preserve-first 경로가 필요합니다"),
    row!(Stamp, InputEntriesNotCarried, "INPUT_ENTRIES_NOT_CARRIED", 1, ops),
    row!(Stamp, StampManifestInvariant, "STAMP_MANIFEST_INVARIANT", 2),
    row!(Stamp, StampCodecFailed, "STAMP_CODEC_FAILED", 2),
    row!(Stamp, StampSourceHashMismatch, "STAMP_SOURCE_HASH_MISMATCH", 1, "문서가 변경됐습니다 — `stamp-plan` 을 다시 실행해 맵의 source_sha256 을 갱신하세요"),
    row!(Stamp, StampDeltaMismatch, "STAMP_DELTA_MISMATCH", 2, ops),
    // `stamper_code`'s default arm folds every unrecognized `StamperError`
    // variant into `OpsCode::UpstreamUnmapped` (module docs) — no
    // classifier in `crates/hwpforge/src/ops/mod.rs` ever produces
    // `OpsCode::StampFailed` (grepped: zero hits), so this row is keyed on
    // `UpstreamUnmapped`, not `StampFailed` (W3 remediation fix).
    row!(Stamp, UpstreamUnmapped, "STAMP_FAILED", 2),
    row!(Stamp, TableNotFound, "TABLE_NOT_FOUND", 1),
    row!(Stamp, TableGridInvalid, "TABLE_GRID_INVALID", 1),
    // StampCellNotAnchor: dynamic hint, see cli_error.
    row!(Stamp, StampCellNotEmpty, "STAMP_CELL_NOT_EMPTY", 1, ops),
    row!(Stamp, StampLabelDrift, "STAMP_LABEL_DRIFT", 1, ops),
    row!(Stamp, StampCellNotCandidate, "STAMP_CELL_NOT_CANDIDATE", 1),
    row!(Stamp, StampCellTargetDuplicate, "STAMP_CELL_TARGET_DUPLICATE", 1),
    row!(Stamp, StampNameEmpty, "STAMP_NAME_EMPTY", 1),
    row!(Stamp, StampHintBlank, "STAMP_HINT_BLANK", 1, ops),
    // StampNameDuplicate/StampNameCollision/StampCandidateUncovered:
    // dual-source (text-spec vs cell-spec wording), see cli_error.
    row!(Stamp, StampSpecStale, "STAMP_SPEC_STALE", 1, "문서가 변경됐거나 span 이 어긋났습니다 — `stamp-plan` 을 다시 실행해 맵을 갱신하세요"),
    row!(Stamp, StampMarkerMismatch, "STAMP_MARKER_MISMATCH", 1),
    row!(Stamp, StampSpecDuplicate, "STAMP_SPEC_DUPLICATE", 1),
    // EncodeSemanticLoss: reconstructed to STAMP_CODEC_FAILED/2, see cli_error.

    // ── Patch (ops::patch) ────────────────────────────────────────────
    row!(Patch, JsonParseFailed, "JSON_PARSE_FAILED", 2),
    row!(Patch, GridAddrInvalid, "GRID_ADDR_INVALID", 2, "Grid addresses come from to-json output; after structural edits, drop the stale addr fields (or re-export) and retry"),
    row!(Patch, DecodeFailed, "DECODE_FAILED", 2),
    // SectionOutOfRange / SectionIndexMismatch: dynamic hint, see cli_error.
    row!(Patch, PatchFailed, "PATCH_FAILED", 2, "Re-export the target section with this version of hwpforge so the JSON contains preservation metadata. Structural or style changes still require a broader rebuild workflow."),
    row!(Patch, UpstreamUnmapped, "SECTION_WORKFLOW_FAILED", 2, "Update hwpforge so the CLI understands the newer section workflow error."),

    // ── Templates (ops::templates + OpsError::PresetNotFound) ─────────
    row!(Templates, PresetNotFound, "PRESET_NOT_FOUND", 1, "Run 'hwpforge templates list' to see available presets"),

    // ── Schema — stays CLI-local: `ops::schema` needs hwpforge's
    // `schemars` feature, not requested by this lane's Cargo.toml change
    // (common.md/lane-compat.md specify `features = ["ops"]` only). No
    // TABLE rows; flagged in the W3 report for whichever lane wires `schema`.

    // ── ToMd (ops::markdown::to_md) ─────────────────────────────────────
    row!(ToMd, DecodeFailed, "DECODE_FAILED", 2),
    // NOTE: code differs — legacy to-md used VALIDATE_FAILED, not ops's
    // VALIDATION_FAILED.
    row!(ToMd, ValidationFailed, "VALIDATE_FAILED", 2),
    row!(ToMd, EncodeFailed, "ENCODE_FAILED", 2),

    // ── Validate (ops::validate) — new command, no legacy precedent ───
    // `ops::validate`'s own `# Errors` doc names only `OpsError::Decode`;
    // a failed `Document::validate` check is `ValidateOutput::ok == false`,
    // not an `OpsError`, so it never reaches this table (see validate.rs's
    // module docs for the exit-1 report-semantics path instead).
    row!(Validate, DecodeFailed, "DECODE_FAILED", 2),
];

/// Maps an [`OpsError`] from calling `cmd`'s underlying `ops` function onto
/// the CLI's frozen `(code, hint)` contract. Call [`exit_code`] with the
/// same `cmd` and the returned error's `code` for the process exit code.
///
/// The message is **not** frozen (see module docs): it defaults to
/// `err.to_string()`, except the semantic-loss and dual-source
/// reconstructions below, which pin exact legacy wording a downstream test
/// depends on byte-for-byte.
#[must_use]
pub fn cli_error(cmd: Command, err: OpsError) -> CliError {
    // Semantic-loss refusals: set-cell and stamp never reported
    // ENCODE_SEMANTIC_LOSS (module docs). Reconstructs the byte-identical
    // legacy message from the *first* carried semantic-loss warning, the
    // same convention `hwpforge-smithy-hwpx`'s own R1 F4 regression tests
    // (`cell_edit.rs`, `stamp/stamper.rs`) pin.
    if let OpsError::EncodeSemanticLoss { warnings, .. } = &err {
        if let Some(info) = semantic_loss_error(cmd, warnings) {
            return info;
        }
    }

    // Hints that embed per-call data no flat table row can hold, matched on
    // the wrapped library error (so no `cmd` disambiguation is needed for
    // most of these — each error type is reachable from exactly one
    // command). `Fill`/`ToJson`/`Patch` disambiguate by `cmd` only where the
    // same wrapped error type is reachable from more than one command.
    match &err {
        OpsError::Fill(FillError::UnknownField { available, .. }) if cmd == Command::Fill => {
            return CliError::new("FIELD_NOT_FOUND", err.to_string()).with_hint(format!(
                "사용 가능한 필드: [{}] — `hwpforge fields <file>` 로 확인",
                available.join(", ")
            ));
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange { sections, .. })
            if cmd == Command::ToJson || cmd == Command::Patch =>
        {
            return CliError::new("SECTION_OUT_OF_RANGE", err.to_string())
                .with_hint(format!("Valid range: 0..{}", sections.saturating_sub(1)));
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
            requested,
            actual,
        }) if cmd == Command::Patch => {
            return CliError::new("SECTION_INDEX_MISMATCH", err.to_string()).with_hint(format!(
                "Use --section {actual} to match the JSON, or re-export section {requested} with this version of hwpforge."
            ));
        }
        // No `Command::ToJson` arm for `SectionIndexMismatch`: unreachable
        // through `ops::exchange::export_section` (module docs / `TABLE`'s
        // ToJson section) — falls through to the generic fallback below,
        // which already reproduces the legacy code+no-hint shape exactly
        // (`code.as_str()` is `"SECTION_INDEX_MISMATCH"`, `err.hint()` is
        // `None`) if this is ever somehow reached.
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NotAnAnchor {
            anchor, ..
        })) => {
            let mut e = CliError::new("STAMP_CELL_NOT_ANCHOR", err.to_string());
            if let Some(anchor) = anchor {
                e = e.with_hint(format!(
                    "이 좌표는 병합 피복 위치입니다 — anchor ({},{}) 를 지정하세요",
                    anchor.row, anchor.col
                ));
            }
            return e;
        }
        // Dual-source: same OpsCode, different legacy wording depending on
        // whether the rejection came from the text-spec path (StampError,
        // inside StamperError::Stamp) or the cell-spec path
        // (CellStampError, inside StamperError::CellStamp).
        OpsError::Stamper(StamperError::Stamp(StampError::DuplicateName { .. })) => {
            return CliError::new("STAMP_NAME_DUPLICATE", err.to_string());
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::DuplicateName { .. })) => {
            return CliError::new("STAMP_NAME_DUPLICATE", err.to_string());
        }
        OpsError::Stamper(StamperError::Stamp(StampError::NameCollision { .. })) => {
            return CliError::new("STAMP_NAME_COLLISION", err.to_string())
                .with_hint("기존 누름틀과 이름이 겹칩니다 — `fields` 로 기존 이름을 확인하세요");
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NameCollision { .. })) => {
            return CliError::new("STAMP_NAME_COLLISION", err.to_string());
        }
        OpsError::Stamper(StamperError::Stamp(StampError::UncoveredCandidate { .. })) => {
            return CliError::new("STAMP_CANDIDATE_UNCOVERED", err.to_string()).with_hint(
                "모든 무가드 후보는 이름을 붙이거나 ignore 로 명시해야 합니다 — `stamp-plan` 출력을 빠짐없이 분류하세요",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::UncoveredCandidate {
            ..
        })) => {
            return CliError::new("STAMP_CANDIDATE_UNCOVERED", err.to_string())
                .with_hint("모든 무가드 셀 후보는 이름을 붙이거나 ignore 로 명시해야 합니다");
        }
        OpsError::CellEdit(CellEditError::TableNotFound { tables, .. })
            if cmd == Command::SetCell =>
        {
            return CliError::new("TABLE_NOT_FOUND", err.to_string()).with_hint(format!(
                "문서에 표가 {tables}개 있습니다 (to-json export 순서 기준 0-base)"
            ));
        }
        _ => {}
    }

    let code = err.code();
    let message = err.to_string();
    let (legacy, hint) = match TABLE.iter().find(|row| row.cmd == cmd && row.code == code) {
        Some(row) => (row.legacy, resolved_hint(row.hint, err.hint())),
        // New OpsCode with no legacy precedent for this command (module
        // docs) — `code.as_str()` and `err.hint()` are `ops`'s own,
        // introduced by this migration, not a divergence to hide. The same
        // `err.hint()` a `Hint::FromOps` row would resolve to, just with no
        // legacy code string to carry either.
        None => (code.as_str(), err.hint()),
    };
    match hint {
        Some(hint) => CliError::new(legacy, message).with_hint(hint),
        None => CliError::new(legacy, message),
    }
}

/// Reconstructs the byte-identical legacy semantic-loss refusal for
/// `set-cell` and `stamp`. Returns `None` for any other command: neither
/// currently reaches `EncodeSemanticLoss` through the `ops` functions they
/// call.
fn semantic_loss_error(cmd: Command, warnings: &[WarningInfo]) -> Option<CliError> {
    match cmd {
        Command::SetCell => {
            let message = match warnings.first() {
                Some(w) => format!(
                    "codec failure: encode produced a semantic-loss warning (fail-closed): {}",
                    w.message
                ),
                None => {
                    "codec failure: encode produced a semantic-loss warning (fail-closed)".into()
                }
            };
            Some(CliError::new("SET_CELL_CODEC_FAILED", message))
        }
        Command::Stamp => {
            let message = match warnings.first() {
                Some(w) => {
                    format!("encode produced a semantic-loss warning (fail-closed): {}", w.message)
                }
                None => "encode produced a semantic-loss warning (fail-closed)".into(),
            };
            Some(CliError::new("STAMP_CODEC_FAILED", message))
        }
        _ => None,
    }
}

/// Maps a [`ConvertOpsError`] from calling `cmd`'s
/// `hwpforge_convert::ops` function (`convert_hwp5` or `to_pdf`) onto the
/// CLI's frozen contract, carrying `cause` (`to-pdf`'s `PDF_RENDER_FAILED`
/// detail) from [`ConvertOpsError::cause_info`].
///
/// Takes `cmd` — a deviation from `lane-compat.md`'s `convert_error(err) ->
/// CliError` signature; see the module docs for why the two callers cannot
/// share one code/hint/exit mapping.
#[must_use]
pub fn convert_error(cmd: Command, err: ConvertOpsError) -> CliError {
    let code = err.code();
    let message = err.to_string();
    let (legacy, hint) = match TABLE.iter().find(|row| row.cmd == cmd && row.code == code) {
        Some(row) => (row.legacy, resolved_hint(row.hint, err.hint())),
        None => (code.as_str(), err.hint()),
    };
    let base = match hint {
        Some(hint) => CliError::new(legacy, message).with_hint(hint),
        None => CliError::new(legacy, message),
    };
    match err.cause_info() {
        Some(cause) => base.with_cause(ErrorCause {
            stage: cause.stage,
            code: cause.code.as_str(),
            kind: cause.kind,
            location: cause.location,
        }),
        None => base,
    }
}

/// Exit codes for the `(command, legacy)` pairs [`cli_error`] resolves
/// *before* consulting `TABLE` — the dynamic-hint and dual-source arms whose
/// hint embeds per-call data (or differs by wrapped-error origin) and so has
/// no flat `TABLE` row. `TABLE` carries the exit code together with the hint;
/// these pairs carry theirs here, straight from the audited snapshot's exit
/// column (`tests/data/legacy_codes.txt`). Without this table [`exit_code`]
/// fell through to its fallback (2) for every one of them while the legacy
/// exit was 1 (W3 lane finding — four commands had to override it locally).
/// The inventory tests pin this table to the snapshot both ways: every
/// `DYNAMIC`/`DUAL_SOURCE` pair has a row here, every row here is such a
/// pair, and each exit matches the snapshot.
const DYNAMIC_EXIT: &[(Command, &str, i32)] = &[
    (Command::Fill, "FIELD_NOT_FOUND", 1),
    (Command::ToJson, "SECTION_OUT_OF_RANGE", 1),
    (Command::Patch, "SECTION_OUT_OF_RANGE", 1),
    (Command::Patch, "SECTION_INDEX_MISMATCH", 2),
    (Command::Stamp, "STAMP_CELL_NOT_ANCHOR", 1),
    (Command::Stamp, "STAMP_NAME_DUPLICATE", 1),
    (Command::Stamp, "STAMP_NAME_COLLISION", 1),
    (Command::Stamp, "STAMP_CANDIDATE_UNCOVERED", 1),
    (Command::SetCell, "TABLE_NOT_FOUND", 1),
];

/// The process exit code for `cmd`'s error `err` (its `code` field, as
/// returned by [`cli_error`]/[`convert_error`]). Exit codes vary by command,
/// not by code alone (module docs) — this is a `TABLE` lookup (then the
/// [`DYNAMIC_EXIT`] lookup for the pairs `TABLE` cannot hold), not a
/// heuristic. Falls back to 2 (the majority "codec/validation failure"
/// class) for a code with no legacy row, matching the same "new code, no
/// divergence to preserve" reasoning as [`cli_error`]'s fallback.
#[must_use]
pub fn exit_code(cmd: Command, err: &CliError) -> i32 {
    TABLE
        .iter()
        .find(|row| row.cmd == cmd && row.legacy == err.code)
        .map(|row| row.exit)
        .or_else(|| {
            DYNAMIC_EXIT.iter().find(|(c, legacy, _)| *c == cmd && *legacy == err.code).map(|r| r.2)
        })
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    //! Inventory: every row [`TABLE`] claims is checked against
    //! `tests/data/legacy_codes.txt` (the hand-audited snapshot) two ways —
    //! **coverage** (`every_snapshot_row_is_accounted_for`: every snapshot
    //! `(command, code)` pair is `TABLE`, a `DYNAMIC` special case, or a
    //! `CLI_LOCAL` code with no `OpsCode` equivalent) and **fidelity**
    //! (`every_table_row_matches_a_real_snapshot_shape`: every `TABLE` row's
    //! `hint` and `exit` match — byte-for-byte for `hint`, exactly for
    //! `exit` — a real call-site shape the snapshot records, built
    //! programmatically from the snapshot's own hint/exit columns rather
    //! than re-typed by hand into this test). A prior lane's snapshot
    //! recorded only code literals and shipped three wrong hints
    //! undetected (W2 adversarial review) — this is what closes that gap.

    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    const SNAPSHOT: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/legacy_codes.txt"));
    /// Codes/commands with no pre-migration call site to audit
    /// (`legacy_codes.txt`'s own header note) — `validate` and
    /// `insert-para INSERT_TEXT_REQUIRED` today. Read alongside `SNAPSHOT`;
    /// kept a separate file (not a separate table) so both stay audited
    /// snapshots, not compat.rs's own claims re-typed under a new name.
    const SNAPSHOT_NEW: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/new_codes.txt"));

    /// One `tests/data/legacy_codes.txt` data row, hint parsed into
    /// [`SnapshotHint`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SnapshotRow {
        cmd: &'static str,
        code: &'static str,
        exit: i32,
        hint: SnapshotHint,
    }

    /// The snapshot's `hint` column, parsed per the format `tests/data/
    /// legacy_codes.txt`'s header documents.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum SnapshotHint {
        /// `-` — no `.with_hint(...)` at this call site.
        None,
        /// `dynamic:<what>` — a per-call value no flat row can hold.
        Dynamic,
        /// The exact `.with_hint("...")` literal.
        Literal(&'static str),
    }

    /// Parses every non-comment, non-blank line of `tests/data/legacy_codes.txt`
    /// *and* `tests/data/new_codes.txt` (module docs — the migration split
    /// "pre-migration" and "no pre-migration call site" rows across the two
    /// files; a `(command, code)` pair audited in one must not also appear
    /// in the other, see `legacy_and_new_snapshots_stay_disjoint`). Comment
    /// lines (including the `[compound: …]`/`[wildcard: …]` annotations
    /// directly above some rows) and blank lines are skipped; everything
    /// else must be a 4-column `command\tcode\texit\thint` row.
    fn snapshot_rows() -> Vec<SnapshotRow> {
        [SNAPSHOT, SNAPSHOT_NEW]
            .into_iter()
            .flat_map(str::lines)
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .map(|line| {
                let mut parts = line.splitn(4, '\t');
                let cmd = parts.next().expect("command column");
                let code = parts.next().expect("code column");
                let exit: i32 = parts
                    .next()
                    .expect("exit column")
                    .parse()
                    .unwrap_or_else(|e| panic!("bad exit column in {line:?}: {e}"));
                let hint_field = parts.next().expect("hint column");
                assert!(parts.next().is_none(), "unexpected 5th column: {line:?}");
                let hint = if hint_field == "-" {
                    SnapshotHint::None
                } else if let Some(_desc) = hint_field.strip_prefix("dynamic:") {
                    SnapshotHint::Dynamic
                } else {
                    SnapshotHint::Literal(hint_field)
                };
                SnapshotRow { cmd, code, exit, hint }
            })
            .collect()
    }

    fn snapshot_pairs() -> BTreeSet<(&'static str, &'static str)> {
        snapshot_rows().into_iter().map(|r| (r.cmd, r.code)).collect()
    }

    fn cmd_name(cmd: Command) -> &'static str {
        match cmd {
            Command::ConvertHwp5 => "convert-hwp5",
            Command::ToPdf => "to-pdf",
            Command::Convert => "convert",
            Command::Inspect => "inspect",
            Command::ToJson => "to-json",
            Command::FromJson => "from-json",
            Command::Outline => "outline",
            Command::Diff => "diff",
            Command::DeletePara => "delete-para",
            Command::InsertPara => "insert-para",
            Command::Read => "read",
            Command::Fields => "fields",
            Command::Fill => "fill",
            Command::SetCell => "set-cell",
            Command::StampPlan => "stamp-plan",
            Command::Stamp => "stamp",
            Command::Patch => "patch",
            Command::Templates => "templates",
            Command::Schema => "schema",
            Command::ToMd => "to-md",
            Command::Validate => "validate",
        }
    }

    /// `(command, legacy)` pairs whose snapshot hint is `dynamic:` — the
    /// ones no flat `TABLE` row can hold verbatim, matched on the wrapped
    /// library error in `cli_error` instead. `set-cell SET_CELL_CODEC_FAILED`
    /// and `stamp STAMP_CODEC_FAILED` are deliberately **not** here: their
    /// `EncodeSemanticLoss` origin is a special case too
    /// (`semantic_loss_error`), but its `(code, exit)` shape is identical to
    /// the `Codec(_)`-origin `TABLE` row already covering that pair, and its
    /// snapshot hint is `-`, not `dynamic:` (no per-call value to embed) —
    /// so `TABLE` alone already accounts for it in (b)/(c).
    const DYNAMIC: &[(&str, &str)] = &[
        ("fill", "FIELD_NOT_FOUND"),
        ("to-json", "SECTION_OUT_OF_RANGE"),
        ("patch", "SECTION_OUT_OF_RANGE"),
        ("patch", "SECTION_INDEX_MISMATCH"),
        ("stamp", "STAMP_CELL_NOT_ANCHOR"),
        ("set-cell", "TABLE_NOT_FOUND"),
    ];

    /// `(command, legacy)` pairs that *have* an `OpsCode` in the shared
    /// vocabulary but are unreachable through the specific `ops` function
    /// this command calls, per that function's own `# Errors` doc — not
    /// "no `OpsCode` equivalent exists" (that is `CLI_LOCAL`), but "this
    /// command's legacy `exit_section_workflow_error` shared its match
    /// verbatim with `patch.rs`'s copy, which genuinely reaches these, while
    /// `to-json`'s own `--section` export never could." `TABLE` correctly
    /// carries no row for these; kept distinct from `CLI_LOCAL` so a future
    /// reader does not conclude the CLI constructs them itself.
    const LEGACY_UNREACHABLE_VIA_OPS: &[(&str, &str)] = &[
        ("to-json", "SECTION_INDEX_MISMATCH"),
        ("to-json", "PATCH_FAILED"),
        ("to-json", "SECTION_WORKFLOW_FAILED"),
    ];

    /// `(command, legacy)` pairs that are **dual-source** — the same
    /// `OpsCode` reachable through two different wrapped-error origins with
    /// different legacy wording (text-spec `StampError` vs. cell-spec
    /// `CellStampError`), matched in `cli_error` before `TABLE` the same way
    /// `DYNAMIC` pairs are, but not embedding a per-call *value* the way
    /// `dynamic:` rows do — each origin's hint is its own fixed literal. Kept
    /// separate from `DYNAMIC` so (d) does not demand a `dynamic:` marker
    /// these rows' snapshot hints correctly do not have.
    const DUAL_SOURCE: &[(&str, &str)] = &[
        ("stamp", "STAMP_NAME_DUPLICATE"),
        ("stamp", "STAMP_NAME_COLLISION"),
        ("stamp", "STAMP_CANDIDATE_UNCOVERED"),
    ];

    /// `(command, legacy)` pairs with no `OpsCode` equivalent — pure local
    /// I/O, CLI-only argument parsing (`--set`'s `NAME=VALUE` shape, a
    /// `--map` JSON file's own parse), or a pretty-printing stage `ops`
    /// never performs. These stay direct `CliError::new(...)` construction
    /// in the command files, never routed through `cli_error`/`convert_error`.
    #[rustfmt::skip]
    const CLI_LOCAL: &[(&str, &str)] = &[
        ("audit-hwp5", "FILE_READ_FAILED"), ("audit-hwp5", "HWP5_DECODE_FAILED"),
        ("audit-hwp5", "HWP5_SEMANTIC_FAILED"), ("audit-hwp5", "HWPX_ANALYSIS_FAILED"),
        ("audit-hwp5", "HWPX_DECODE_FAILED"),
        ("census-hwp5", "FILE_READ_FAILED"), ("census-hwp5", "FILE_WRITE_FAILED"),
        ("census-hwp5", "HWP5_CENSUS_FAILED"), ("census-hwp5", "HWPX_CENSUS_FAILED"),
        ("convert-hwp5", "FILE_WRITE_FAILED"),
        // The CLI-local `inspect_hwp5_file` pre-check in
        // `commands/convert_hwp5.rs` — the `convert_hwp5()` ops call's own
        // `Hwp5DecodeFailed` collapses into `HWP5_CONVERT_FAILED` in `TABLE`
        // instead (W3 remediation finding 7 — legacy's single
        // `hwp5_to_hwpx_with_options` call site never distinguished decode
        // from convert stage).
        ("convert-hwp5", "HWP5_DECODE_FAILED"),
        ("to-pdf", "FILE_READ_FAILED"), ("to-pdf", "FILE_WRITE_FAILED"),
        ("convert", "FILE_READ_FAILED"), ("convert", "FILE_WRITE_FAILED"),
        ("convert", "INPUT_TOO_LARGE"), ("convert", "STDIN_READ_FAILED"),
        ("inspect", "FILE_READ_FAILED"), ("inspect", "ANALYSIS_FAILED"),
        ("to-json", "FILE_READ_FAILED"), ("to-json", "FILE_WRITE_FAILED"),
        ("to-json", "INVALID_EXTENSION"), ("to-json", "JSON_SERIALIZE_FAILED"),
        ("from-json", "FILE_READ_FAILED"), ("from-json", "FILE_WRITE_FAILED"),
        ("outline", "FILE_READ_FAILED"),
        ("diff", "FILE_READ_FAILED"), ("diff", "FILE_WRITE_FAILED"),
        ("delete-para", "FILE_READ_FAILED"), ("delete-para", "FILE_WRITE_FAILED"),
        ("insert-para", "FILE_READ_FAILED"), ("insert-para", "FILE_WRITE_FAILED"),
        ("read", "FILE_READ_FAILED"),
        ("fields", "FILE_READ_FAILED"),
        ("fill", "FILE_READ_FAILED"), ("fill", "FILE_WRITE_FAILED"),
        ("fill", "INVALID_SET"), ("fill", "DUPLICATE_SET"),
        ("set-cell", "FILE_READ_FAILED"), ("set-cell", "FILE_WRITE_FAILED"),
        ("set-cell", "INVALID_SET_CELL_MAP"),
        ("stamp-plan", "FILE_READ_FAILED"),
        ("stamp", "FILE_READ_FAILED"), ("stamp", "FILE_WRITE_FAILED"),
        ("stamp", "INVALID_STAMP_MAP"), ("stamp", "MANIFEST_PATH_CONFLICT"),
        ("patch", "FILE_READ_FAILED"), ("patch", "FILE_WRITE_FAILED"),
        ("templates", "PRESET_NOT_FOUND"),
        ("schema", "UNKNOWN_SCHEMA_TYPE"),
        ("to-md", "DIR_CREATE_FAILED"), ("to-md", "FILE_WRITE_FAILED"),
        ("validate", "FILE_READ_FAILED"), ("validate", "JSON_SERIALIZE_FAILED"),
        ("shared", "INPUT_TOO_LARGE"),
    ];

    /// (a) Every `TABLE` row's `(cmd, legacy)` pair is genuinely in the
    /// audited snapshot for that command.
    #[test]
    fn every_table_row_is_in_the_audited_snapshot() {
        let snapshot = snapshot_pairs();
        for row in TABLE {
            let name = cmd_name(row.cmd);
            assert!(
                snapshot.contains(&(name, row.legacy)),
                "TABLE has ({name}, {}) but tests/data/legacy_codes.txt does not — \
                 either the row is wrong or the snapshot needs re-auditing",
                row.legacy
            );
        }
    }

    /// (b) Every snapshot row is `TABLE`, `DYNAMIC`, `DUAL_SOURCE`, or
    /// `CLI_LOCAL` — not two of the four (a pair in both `TABLE` and
    /// `CLI_LOCAL` would be two conflicting answers for the same code).
    #[test]
    fn every_snapshot_row_is_accounted_for() {
        let table: BTreeSet<(&str, &str)> =
            TABLE.iter().map(|row| (cmd_name(row.cmd), row.legacy)).collect();
        let dynamic: BTreeSet<(&str, &str)> = DYNAMIC.iter().copied().collect();
        let dual_source: BTreeSet<(&str, &str)> = DUAL_SOURCE.iter().copied().collect();
        let unreachable: BTreeSet<(&str, &str)> =
            LEGACY_UNREACHABLE_VIA_OPS.iter().copied().collect();
        let local: BTreeSet<(&str, &str)> = CLI_LOCAL.iter().copied().collect();
        let buckets = [&table, &dynamic, &dual_source, &unreachable, &local];

        for pair in snapshot_pairs() {
            assert!(
                buckets.iter().any(|b| b.contains(&pair)),
                "{pair:?} is in the snapshot but TABLE/DYNAMIC/DUAL_SOURCE/\
                 LEGACY_UNREACHABLE_VIA_OPS/CLI_LOCAL none account for it"
            );
        }
        // Pairwise disjoint among DYNAMIC/DUAL_SOURCE/LEGACY_UNREACHABLE_VIA_OPS/
        // CLI_LOCAL — two of *these* both claiming a pair would be two
        // conflicting answers for the same code. `TABLE` is deliberately
        // excluded from this check: a `(command, code)` pair can genuinely
        // have both an ops-originated call-site shape (in `TABLE`) and a
        // separate CLI-local one (in `CLI_LOCAL`) — e.g. `set-cell
        // INVALID_SET_CELL_MAP` (ops's own empty-batch rejection vs. the
        // frontend's own `--map` file JSON-parse failure) and `to-json
        // JSON_SERIALIZE_FAILED` (`ops::to_json`'s `serde_json::to_value`
        // failure vs. the frontend's own `to_string_pretty` pretty-print
        // stage `ops` never performs) — both real, both correctly routed to
        // different code paths, neither a drift bug.
        let no_table = [&dynamic, &dual_source, &unreachable, &local];
        for i in 0..no_table.len() {
            for j in (i + 1)..no_table.len() {
                for pair in no_table[i] {
                    assert!(
                        !no_table[j].contains(pair),
                        "{pair:?} is in more than one of DYNAMIC/DUAL_SOURCE/\
                         LEGACY_UNREACHABLE_VIA_OPS/CLI_LOCAL (indices {i} and {j})"
                    );
                }
            }
        }
    }

    /// (c) Fidelity: every `TABLE` row's `hint` and `exit` match — hint
    /// byte-for-byte, exit exactly — at least one real snapshot row for the
    /// same `(command, code)`. Built from the snapshot's own parsed columns,
    /// not by re-typing the expected hint text a second time into this test
    /// (the W2-review failure mode this addendum closes).
    #[test]
    fn every_table_row_matches_a_real_snapshot_shape() {
        let mut by_pair: BTreeMap<(&str, &str), Vec<SnapshotRow>> = BTreeMap::new();
        for row in snapshot_rows() {
            by_pair.entry((row.cmd, row.code)).or_default().push(row);
        }

        for row in TABLE {
            let name = cmd_name(row.cmd);
            let candidates = by_pair
                .get(&(name, row.legacy))
                .unwrap_or_else(|| panic!("({name}, {}) has no snapshot rows at all", row.legacy));
            // `Hint::FromOps` rows resolve through the exact same
            // `OpsError::hint()` lookup `cli_error` uses — `OpsError::Rejected`
            // is `OpsError`'s own public escape hatch for pinning an
            // arbitrary code (`hint()` only ever consults `self.code()`), so
            // this needs no per-code error constructor.
            let ops_hint = OpsError::Rejected { code: row.code, reason: String::new() }.hint();
            let resolved = resolved_hint(row.hint, ops_hint);
            let matches = candidates.iter().any(|c| {
                c.exit == row.exit
                    && match (&c.hint, resolved) {
                        (SnapshotHint::None, None) => true,
                        (SnapshotHint::Literal(text), Some(hint)) => *text == hint,
                        _ => false,
                    }
            });
            assert!(
                matches,
                "TABLE row ({name}, {}, exit={}, hint={:?}) matches none of the snapshot's \
                 shapes for that (command, code): {candidates:?}",
                row.legacy, row.exit, resolved
            );
        }
    }

    /// (e) `DYNAMIC_EXIT` is exactly the `DYNAMIC` ∪ `DUAL_SOURCE` pairs, and
    /// each exit matches at least one snapshot row for that pair (the same
    /// "real call-site shape" rule (c) applies to `TABLE`). A pair in
    /// `DYNAMIC`/`DUAL_SOURCE` without a row here would silently take
    /// `exit_code`'s fallback (2) — the W3 lane finding this table closes.
    #[test]
    fn dynamic_exit_rows_cover_every_special_case_pair_with_the_snapshot_exit() {
        let rows = snapshot_rows();
        let special: BTreeSet<(&str, &str)> =
            DYNAMIC.iter().chain(DUAL_SOURCE.iter()).copied().collect();
        let table: BTreeSet<(&str, &str)> =
            DYNAMIC_EXIT.iter().map(|(c, legacy, _)| (cmd_name(*c), *legacy)).collect();
        assert_eq!(
            table, special,
            "DYNAMIC_EXIT must list exactly the DYNAMIC ∪ DUAL_SOURCE pairs"
        );
        for (cmd, legacy, exit) in DYNAMIC_EXIT {
            let name = cmd_name(*cmd);
            assert!(
                rows.iter().any(|r| r.cmd == name && r.code == *legacy && r.exit == *exit),
                "DYNAMIC_EXIT row ({name}, {legacy}, exit={exit}) matches no snapshot row"
            );
        }
    }

    /// (d) Every snapshot row marked `dynamic:` in its hint column is in the
    /// `DYNAMIC` list (and vice versa is covered by (b)'s
    /// `every_snapshot_row_is_accounted_for`) — catches a row whose hint
    /// category and the `cli_error` special-case list have drifted apart.
    #[test]
    fn every_dynamic_snapshot_row_has_a_special_case() {
        for row in snapshot_rows() {
            if row.hint == SnapshotHint::Dynamic {
                assert!(
                    DYNAMIC.contains(&(row.cmd, row.code)),
                    "({}, {}) is marked dynamic: in the snapshot but DYNAMIC does not list it",
                    row.cmd,
                    row.code
                );
            }
        }
    }

    /// `SNAPSHOT` (pre-migration) and `SNAPSHOT_NEW` (no pre-migration call
    /// site) must not both audit the same `(command, code)` pair — that
    /// would be two conflicting "audited" answers for one call site (both
    /// files' own header notes commit to this).
    #[test]
    fn legacy_and_new_snapshots_stay_disjoint() {
        fn pairs(text: &str) -> BTreeSet<(&str, &str)> {
            text.lines()
                .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .map(|line| {
                    let mut parts = line.splitn(4, '\t');
                    (parts.next().expect("command column"), parts.next().expect("code column"))
                })
                .collect()
        }
        let legacy = pairs(SNAPSHOT);
        let new = pairs(SNAPSHOT_NEW);
        for pair in &new {
            assert!(
                !legacy.contains(pair),
                "{pair:?} is audited in both legacy_codes.txt and new_codes.txt"
            );
        }
    }

    /// Cross-frontend agreement (audit finding: "two snapshots never
    /// compared" — and a follow-up audit finding on *this very test*: an
    /// earlier version compared against ONE global MCP vocabulary merged
    /// across every tool, so a CLI row could "agree" by matching some
    /// *other* tool's string for the same code. `insert-para`/`delete-para`
    /// `SECTION_OUT_OF_RANGE` was exactly such a false agreement: that
    /// string is genuinely in MCP's vocabulary — for `patch`/`to_json`, not
    /// for `insert_para`/`delete_para`, which emit `INDEX_OUT_OF_RANGE`
    /// instead — so the merged set hid a real divergence). This version
    /// compares each row against only the MCP *tool*
    /// [`mcp_tool_name`] maps its `Command` to, via a per-tool vocabulary
    /// map, not a flat union.
    ///
    /// For every `TABLE` row, checks whether CLI's legacy wire string for
    /// that `(command, code)` also exists in the *mapped tool's own* rows of
    /// MCP's frozen vocabulary (`hwpforge-bindings-mcp/tests/data/
    /// legacy_codes.txt` and `new_codes.txt`, included as plain data — this
    /// crate cannot depend on that one, and the MCP snapshot records legacy
    /// *strings*, not `OpsCode` variants, so this proves "CLI's wire string
    /// for this code exists in MCP's vocabulary for the same tool", not
    /// "MCP's own `ops` classifier reaches the same `OpsCode` here" — that
    /// would need MCP's `compat.rs`, out of this crate's reach). MCP's
    /// shared `output` tool (file I/O reachable from nearly every MCP tool)
    /// is deliberately not unioned into every tool's vocabulary here — this
    /// test only walks `TABLE`, which never carries a CLI-local I/O code
    /// (those are `CLI_LOCAL`'s, compared to MCP's `output` tool nowhere in
    /// this crate), so there is nothing in `TABLE` such a union would ever
    /// legitimately match; adding it back would silently reopen the same
    /// cross-tool leakage this fix closes.
    ///
    /// [`Command::ConvertHwp5`]/[`Command::ToPdf`]/[`Command::Schema`] have
    /// no mapped tool at all (`mcp_tool_name` returns `None`): MCP has no
    /// tool for HWP5→HWPX/PDF conversion or the schema catalogue, so there
    /// is nothing in its vocabulary to compare those rows against — not a
    /// divergence, an absent operation. Every code the comparison script
    /// found unmatched for a command MCP *does* have a tool for is
    /// `KNOWN_CODE_DIVERGENCE`, keyed by `(Command, OpsCode)` rather than
    /// bare `OpsCode` — the `SectionOutOfRange` case above is exactly why a
    /// bare-`OpsCode` key cannot express this: `patch`/`to-json`'s
    /// `SECTION_OUT_OF_RANGE` genuinely agrees with MCP, only
    /// `insert-para`/`delete-para`'s does not — with a one-line reason each
    /// (seeded from that script's output — see the W6b report). On a
    /// mismatch, every offending row is collected and reported together in
    /// one panic, not just the first — the same reason `assert_eq!` beats a
    /// loop of single asserts for a maintainer chasing this down later.
    #[test]
    fn cli_legacy_codes_agree_with_mcp_vocabulary_or_are_a_known_divergence() {
        const MCP_LEGACY: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hwpforge-bindings-mcp/tests/data/legacy_codes.txt"
        ));
        const MCP_NEW: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hwpforge-bindings-mcp/tests/data/new_codes.txt"
        ));
        let mut mcp_codes_by_tool: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for line in [MCP_LEGACY, MCP_NEW]
            .into_iter()
            .flat_map(str::lines)
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        {
            let mut columns = line.split('\t');
            let tool = columns.next().expect("tool column");
            let code = columns.next().expect("code column");
            mcp_codes_by_tool.entry(tool).or_default().insert(code);
        }

        /// Maps a CLI [`Command`] to the MCP tool name MCP's own snapshot
        /// files spell it with (hyphens become underscores — the same
        /// transform [`cmd_name`] would need reversed). `None` means MCP
        /// has no tool for this command at all — see this test's doc for
        /// why those three are excluded rather than compared against
        /// nothing.
        fn mcp_tool_name(cmd: Command) -> Option<&'static str> {
            match cmd {
                Command::ConvertHwp5 | Command::ToPdf | Command::Schema => None,
                Command::Convert => Some("convert"),
                Command::Inspect => Some("inspect"),
                Command::ToJson => Some("to_json"),
                Command::FromJson => Some("from_json"),
                Command::Outline => Some("outline"),
                Command::Diff => Some("diff"),
                Command::DeletePara => Some("delete_para"),
                Command::InsertPara => Some("insert_para"),
                Command::Read => Some("read"),
                Command::Fields => Some("fields"),
                Command::Fill => Some("fill"),
                Command::SetCell => Some("set_cell"),
                Command::StampPlan => Some("stamp_plan"),
                Command::Stamp => Some("stamp"),
                Command::Patch => Some("patch"),
                Command::Templates => Some("templates"),
                Command::ToMd => Some("to_md"),
                Command::Validate => Some("validate"),
            }
        }

        /// Every [`Command`] variant, exhaustive by hand the same way
        /// [`cmd_name`] is — used only to drive the typo guard below over
        /// every mapped tool name at once, not to compare `TABLE` rows.
        const ALL_COMMANDS: &[Command] = &[
            Command::ConvertHwp5,
            Command::ToPdf,
            Command::Convert,
            Command::Inspect,
            Command::ToJson,
            Command::FromJson,
            Command::Outline,
            Command::Diff,
            Command::DeletePara,
            Command::InsertPara,
            Command::Read,
            Command::Fields,
            Command::Fill,
            Command::SetCell,
            Command::StampPlan,
            Command::Stamp,
            Command::Patch,
            Command::Templates,
            Command::Schema,
            Command::ToMd,
            Command::Validate,
        ];

        // Guards the mapping above against a typo (e.g. `"tojson"` for
        // `"to_json"`): such a typo would make every row for that command
        // "divergent" and get silently absorbed by growing
        // `KNOWN_CODE_DIVERGENCE` instead of being caught here.
        for cmd in ALL_COMMANDS {
            if let Some(tool) = mcp_tool_name(*cmd) {
                assert!(
                    mcp_codes_by_tool.contains_key(tool),
                    "mcp_tool_name({cmd:?}) = {tool:?}, but no tool by that name exists in \
                     MCP's snapshot files — check for a spelling mismatch"
                );
            }
        }

        /// `(command, code)` pairs where CLI's legacy wire string is not in
        /// that command's mapped MCP tool's vocabulary at all (each
        /// frontend spelled its own error independently, pre-`ops`) — see
        /// the brief's own two examples: `convert`'s `UNKNOWN_PRESET` (MCP's
        /// `convert` uses `PRESET_NOT_FOUND`, matching every *other*
        /// command/tool) and `from-json`'s `JSON_PARSE_FAILED` (MCP's
        /// `from_json` uses `JSON_PARSE_ERROR`).
        const KNOWN_CODE_DIVERGENCE: &[(Command, OpsCode)] = &[
            // CLI: DECODE_FAILED, every command below. MCP: DECODE_ERROR,
            // always (except `validate`'s own dedicated arm, which is why
            // `Validate` is *not* listed here — it genuinely agrees).
            (Command::Inspect, OpsCode::DecodeFailed),
            (Command::ToJson, OpsCode::DecodeFailed),
            (Command::FromJson, OpsCode::DecodeFailed),
            (Command::Outline, OpsCode::DecodeFailed),
            (Command::Diff, OpsCode::DecodeFailed),
            (Command::DeletePara, OpsCode::DecodeFailed),
            (Command::InsertPara, OpsCode::DecodeFailed),
            (Command::Read, OpsCode::DecodeFailed),
            (Command::Fields, OpsCode::DecodeFailed),
            (Command::Fill, OpsCode::DecodeFailed),
            (Command::SetCell, OpsCode::DecodeFailed),
            (Command::StampPlan, OpsCode::DecodeFailed),
            (Command::Stamp, OpsCode::DecodeFailed),
            (Command::Patch, OpsCode::DecodeFailed),
            (Command::ToMd, OpsCode::DecodeFailed),
            // CLI: ENCODE_FAILED. MCP: ENCODE_ERROR.
            (Command::Convert, OpsCode::EncodeFailed),
            (Command::FromJson, OpsCode::EncodeFailed),
            (Command::ToMd, OpsCode::EncodeFailed),
            // CLI: FILL_FAILED. MCP: FILL_ERROR.
            (Command::Fill, OpsCode::FillFailed),
            // delete-para/insert-para kept the pre-ops UNCARRIED_ZIP_ENTRIES/
            // wildcard STRUCTURAL_EDIT_FAILED strings for this code;
            // set-cell/stamp already agree with MCP on INPUT_ENTRIES_NOT_CARRIED.
            (Command::DeletePara, OpsCode::InputEntriesNotCarried),
            (Command::InsertPara, OpsCode::InputEntriesNotCarried),
            // CLI keeps its own INSERT_BEFORE_SECTION_PROPERTIES; MCP folds
            // it into the shared SECTION_PROPERTIES_PARAGRAPH wildcard arm
            // (R1 fix, MCP compat.rs docs).
            (Command::InsertPara, OpsCode::InsertBeforeSectionProperties),
            // CLI-only: unreachable through MCP's typed `CellSpec` list (MCP
            // compat.rs module docs) — no MCP vocabulary entry to compare.
            (Command::SetCell, OpsCode::InvalidSetCellArgs),
            // CLI: JSON_PARSE_FAILED. MCP: JSON_PARSE_ERROR.
            (Command::FromJson, OpsCode::JsonParseFailed),
            (Command::Patch, OpsCode::JsonParseFailed),
            // CLI: JSON_SERIALIZE_FAILED. MCP: SERIALIZE_ERROR (predates
            // `ops`, also covers MCP's own local pretty-print stage).
            (Command::ToJson, OpsCode::JsonSerializeFailed),
            // CLI: MD_DECODE_FAILED. MCP: MD_DECODE_ERROR.
            (Command::Convert, OpsCode::MdDecodeFailed),
            // CLI keeps distinct PARAGRAPH_OUT_OF_RANGE; MCP folds
            // paragraph/section-out-of-range into one shared INDEX_OUT_OF_RANGE.
            (Command::DeletePara, OpsCode::ParagraphOutOfRange),
            (Command::InsertPara, OpsCode::ParagraphOutOfRange),
            // CLI: PATCH_FAILED. MCP: PATCH_ERROR.
            (Command::Patch, OpsCode::PatchFailed),
            // `convert`'s legacy string is UNKNOWN_PRESET, not the
            // PRESET_NOT_FOUND every other command/tool (including MCP's
            // `convert`) uses — CLI's own pre-migration inconsistency, not a
            // cross-frontend one (brief's own example).
            (Command::Convert, OpsCode::PresetNotFound),
            // CLI keeps distinct SPAN_COUNT_MISMATCH; MCP folds it into the
            // wildcard STRUCTURAL_EDIT_FAILED.
            (Command::DeletePara, OpsCode::SpanCountMismatch),
            (Command::InsertPara, OpsCode::SpanCountMismatch),
            // insert-para/delete-para keep their own legacy SECTION_OUT_OF_
            // RANGE; MCP's `insert_para`/`delete_para` use INDEX_OUT_OF_RANGE
            // instead (`patch`/`to-json` genuinely agree with MCP on
            // SECTION_OUT_OF_RANGE, so this is command-specific, not a
            // blanket OpsCode divergence — the case this test's own doc
            // names as the reason the key is `(Command, OpsCode)`, not
            // `OpsCode` alone).
            (Command::DeletePara, OpsCode::SectionOutOfRange),
            (Command::InsertPara, OpsCode::SectionOutOfRange),
            // CLI: STYLE_REBIND_FAILED. MCP: STYLE_REBIND_ERROR.
            (Command::Convert, OpsCode::StyleRebindFailed),
            // CLI: STYLE_STORE_FAILED. MCP: STYLE_STORE_ERROR.
            (Command::Convert, OpsCode::StyleStoreFailed),
            // `patch` keeps its own CLI-only default-arm string,
            // SECTION_WORKFLOW_FAILED — by design, module docs' "Table vs.
            // special cases".
            (Command::Patch, OpsCode::UpstreamUnmapped),
            // CLI: VALIDATION_FAILED (VALIDATE_FAILED for to-md). MCP:
            // VALIDATION_ERROR.
            (Command::Convert, OpsCode::ValidationFailed),
            (Command::FromJson, OpsCode::ValidationFailed),
            (Command::ToMd, OpsCode::ValidationFailed),
        ];

        let mismatches: Vec<String> = TABLE
            .iter()
            .filter_map(|row| {
                let tool = mcp_tool_name(row.cmd)?;
                let agrees =
                    mcp_codes_by_tool.get(tool).is_some_and(|codes| codes.contains(row.legacy));
                if agrees || KNOWN_CODE_DIVERGENCE.contains(&(row.cmd, row.code)) {
                    return None;
                }
                Some(format!(
                    "{:?} (CLI legacy {:?}, command {:?}, mapped MCP tool {tool:?}) has no \
                     matching string in that tool's MCP vocabulary and is not in \
                     KNOWN_CODE_DIVERGENCE",
                    row.code, row.legacy, row.cmd
                ))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "{} mismatch(es):\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );

        // Reverse check (review finding C4): the loop above only walks
        // `TABLE` looking for rows `KNOWN_CODE_DIVERGENCE` can excuse — it
        // never asks whether an excuse is still needed. Two ways an entry
        // can rot: it stops matching any `TABLE` row at all (the row was
        // renamed or removed), or its row's legacy string starts agreeing
        // with MCP's vocabulary again (a later fix closed the gap) and the
        // exception is now dead weight nobody would notice removing.
        let mut dead = Vec::new();
        let mut stale = Vec::new();
        for &(cmd, code) in KNOWN_CODE_DIVERGENCE {
            let Some(row) = TABLE.iter().find(|row| row.cmd == cmd && row.code == code) else {
                dead.push(format!(
                    "{cmd:?}/{code:?} is in KNOWN_CODE_DIVERGENCE but matches no TABLE row"
                ));
                continue;
            };
            // Every command in the list above maps to an MCP tool (unlike
            // `ConvertHwp5`/`ToPdf`/`Schema`, which never appear here), so
            // this is reusing the same agreement formula the forward check
            // runs, not a new one.
            let Some(tool) = mcp_tool_name(cmd) else { continue };
            let agrees =
                mcp_codes_by_tool.get(tool).is_some_and(|codes| codes.contains(row.legacy));
            if agrees {
                stale.push(format!(
                    "{cmd:?}/{code:?} (CLI legacy {:?}) now agrees with MCP tool {tool:?} — \
                     this exception is no longer needed — remove it from \
                     KNOWN_CODE_DIVERGENCE",
                    row.legacy
                ));
            }
        }
        assert!(
            dead.is_empty() && stale.is_empty(),
            "{} dead KNOWN_CODE_DIVERGENCE entrie(s):\n{}\n{} stale KNOWN_CODE_DIVERGENCE \
             entrie(s):\n{}",
            dead.len(),
            dead.join("\n"),
            stale.len(),
            stale.join("\n")
        );
    }

    // ── round-trip checks: construct a representative OpsError/ConvertOpsError
    // per code, confirm cli_error's/convert_error's output matches. ────────

    fn assert_code(cmd: Command, err: OpsError, expected_code: &str, expected_exit: i32) {
        let got = cli_error(cmd, err);
        assert_eq!(got.code, expected_code, "{cmd:?}: {got:?}");
        assert_eq!(exit_code(cmd, &got), expected_exit, "{cmd:?}: {got:?}");
    }

    #[test]
    fn from_ops_rows_keep_emitting_the_same_hint_text_as_before_the_refactor() {
        // Regression guard for the `Hint::FromOps` collapse (module docs):
        // these rows used to carry their own literal; now they resolve
        // through `err.hint()` (== `hint_for` for this code). The generic
        // fidelity test (c) already proves every `TABLE` row byte-matches
        // the snapshot, but this pins two representative rows end-to-end
        // through `cli_error` itself, not just the table lookup.
        let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("bad".into()));
        let got = cli_error(Command::Inspect, err);
        assert_eq!(got.code, "DECODE_FAILED");
        assert_eq!(got.hint.as_deref(), Some("Check that the file is a valid HWPX document"));

        let err = OpsError::Fill(FillError::EmptyValue { name: "x".into() });
        let got = cli_error(Command::Fill, err);
        assert_eq!(got.code, "EMPTY_FIELD_VALUE");
        assert_eq!(
            got.hint.as_deref(),
            Some("빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요")
        );
    }

    #[test]
    fn preset_not_found_code_differs_by_command() {
        // Same OpsCode, two different legacy strings (module docs: TABLE
        // vocabulary is per-command, not global).
        assert_code(
            Command::Convert,
            OpsError::PresetNotFound { name: "gov".into() },
            "UNKNOWN_PRESET",
            1,
        );
        assert_code(
            Command::Templates,
            OpsError::PresetNotFound { name: "gov".into() },
            "PRESET_NOT_FOUND",
            1,
        );
    }

    #[test]
    fn to_md_validation_failed_keeps_its_legacy_spelling() {
        use hwpforge_core::CoreError;
        let err =
            OpsError::Core(CoreError::InvalidStructure { context: "x".into(), reason: "y".into() });
        assert_code(Command::ToMd, err, "VALIDATE_FAILED", 2);
    }

    #[test]
    fn set_cell_semantic_loss_reconstructs_the_legacy_codec_message() {
        let err = OpsError::EncodeSemanticLoss {
            warnings: vec![WarningInfo::new("NOTE_HEAD_SKIPPED", "titleMark dropped")],
            others: Vec::new(),
        };
        let got = cli_error(Command::SetCell, err);
        assert_eq!(got.code, "SET_CELL_CODEC_FAILED");
        assert_eq!(
            got.message,
            "codec failure: encode produced a semantic-loss warning (fail-closed): titleMark dropped"
        );
        assert_eq!(exit_code(Command::SetCell, &got), 1);
    }

    #[test]
    fn stamp_semantic_loss_reconstructs_the_legacy_message_and_exit_2() {
        let err = OpsError::EncodeSemanticLoss {
            warnings: vec![WarningInfo::new("NOTE_HEAD_SKIPPED", "titleMark dropped")],
            others: Vec::new(),
        };
        let got = cli_error(Command::Stamp, err);
        assert_eq!(got.code, "STAMP_CODEC_FAILED");
        assert_eq!(
            got.message,
            "encode produced a semantic-loss warning (fail-closed): titleMark dropped"
        );
        assert_eq!(exit_code(Command::Stamp, &got), 2);
    }

    #[test]
    fn stamp_upstream_unmapped_keeps_the_legacy_stamp_failed_spelling() {
        // `stamper_code`'s default arm (`crates/hwpforge/src/ops/mod.rs`)
        // folds an unrecognized `StamperError` variant into
        // `OpsCode::UpstreamUnmapped` — no classifier ever produces
        // `OpsCode::StampFailed` (W3 remediation finding). `Rejected` is
        // `OpsError`'s public escape hatch for pinning an arbitrary code
        // without needing a real unmatched upstream variant (same
        // construction the crate's own `mod.rs` tests use for
        // `OpsCode::NoValues`).
        let err = OpsError::Rejected {
            code: OpsCode::UpstreamUnmapped,
            reason: "unrecognized upstream stamper variant".into(),
        };
        assert_code(Command::Stamp, err, "STAMP_FAILED", 2);
    }

    #[test]
    fn fill_field_not_found_carries_the_available_list_hint() {
        let err = OpsError::Fill(FillError::UnknownField {
            name: "x".into(),
            available: vec!["a".into(), "b".into()],
        });
        let got = cli_error(Command::Fill, err);
        assert_eq!(got.code, "FIELD_NOT_FOUND");
        assert!(got.hint.as_deref().unwrap_or_default().contains("[a, b]"), "{got:?}");
    }

    #[test]
    fn section_index_mismatch_hint_differs_between_to_json_and_patch() {
        let err = || {
            OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
                requested: 1,
                actual: 2,
            })
        };
        let to_json = cli_error(Command::ToJson, err());
        assert_eq!(to_json.code, "SECTION_INDEX_MISMATCH");
        assert!(to_json.hint.is_none(), "{to_json:?}");

        let patch = cli_error(Command::Patch, err());
        assert_eq!(patch.code, "SECTION_INDEX_MISMATCH");
        assert!(patch.hint.as_deref().unwrap_or_default().contains("--section 2"), "{patch:?}");
    }

    #[test]
    fn to_pdf_coarsens_hwp5_decode_and_convert_to_one_legacy_code() {
        use hwpforge_convert::ConvertError;
        use hwpforge_smithy_hwp5::Hwp5Error;

        let decode = ConvertOpsError::Convert(ConvertError::Decode(Hwp5Error::NotHwp5 {
            detail: "x".into(),
        }));
        let convert_error_out = convert_error(Command::ToPdf, decode);
        assert_eq!(convert_error_out.code, "HWP5_CONVERT_FAILED");
        assert_eq!(exit_code(Command::ToPdf, &convert_error_out), 2);
    }

    #[test]
    fn convert_hwp5_also_coarsens_a_decode_failure_from_the_ops_call() {
        // W3 remediation finding 7: legacy's single `hwp5_to_hwpx_with_options`
        // call site mapped every failure — decode-stage included — to
        // `HWP5_CONVERT_FAILED`. Only the CLI-local `inspect_hwp5_file`
        // pre-check (`commands/convert_hwp5.rs`, never routed through this
        // table) returns `HWP5_DECODE_FAILED`.
        use hwpforge_convert::ConvertError;
        use hwpforge_smithy_hwp5::Hwp5Error;

        let decode = ConvertOpsError::Convert(ConvertError::Decode(Hwp5Error::NotHwp5 {
            detail: "x".into(),
        }));
        let got = convert_error(Command::ConvertHwp5, decode);
        assert_eq!(got.code, "HWP5_CONVERT_FAILED");
        assert_eq!(
            got.hint.as_deref(),
            Some("Check that the source is a supported HWP5 document and the output path is writable")
        );
        assert_eq!(exit_code(Command::ConvertHwp5, &got), 2);
    }

    #[test]
    fn to_pdf_unrecognized_format_uses_its_own_legacy_hint() {
        let got = convert_error(Command::ToPdf, ConvertOpsError::UnrecognizedFormat);
        assert_eq!(got.code, "UNRECOGNIZED_FORMAT");
        assert_eq!(
            got.hint.as_deref(),
            Some("to-pdf detects the format by content — the extension is only a hint")
        );
    }

    #[test]
    fn validate_decode_failed_has_no_hint_and_exits_2() {
        // `ops::validate`'s only `OpsError` variant (module docs on
        // `Command::Validate`'s TABLE row) — a failed `Document::validate`
        // check never reaches `cli_error` at all (it is `ValidateOutput::ok
        // == false`, handled entirely in `validate.rs`, not this table).
        let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("not a zip file".into()));
        assert_code(Command::Validate, err, "DECODE_FAILED", 2);
    }

    #[test]
    fn structural_commands_decode_failed_output_is_unchanged_by_their_new_table_rows() {
        // Review finding C4 (reverse-check follow-up): `DeletePara`,
        // `InsertPara`, `Fill`, `SetCell`, `StampPlan` and `Stamp` each
        // decode an existing HWPX package, so `DecodeFailed` is genuinely
        // reachable for every one of them — but none had a `TABLE` row,
        // so each fell through `cli_error`'s `None` branch
        // (`(code.as_str(), err.hint())`). That branch always asks
        // `err.hint()`, regardless of the `Hint::None` convention every
        // sibling command with an *explicit* `DecodeFailed` row chose
        // (module docs: "every non-`Inspect` `DecodeFailed` row … had no
        // hint at all") — so today's real output for these six already
        // carries `hint_for(DecodeFailed)`'s text, not `None`. This pins
        // that exact triple so promoting the six to real `TABLE` rows
        // (`Hint::FromOps`, matching `Inspect`'s own row) cannot silently
        // change it.
        for cmd in [
            Command::DeletePara,
            Command::InsertPara,
            Command::Fill,
            Command::SetCell,
            Command::StampPlan,
            Command::Stamp,
        ] {
            let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("bad".into()));
            let got = cli_error(cmd, err);
            assert_eq!(got.code, "DECODE_FAILED", "{cmd:?}");
            assert_eq!(
                got.hint.as_deref(),
                Some("Check that the file is a valid HWPX document"),
                "{cmd:?}"
            );
            assert_eq!(exit_code(cmd, &got), 2, "{cmd:?}");
        }
    }
}
