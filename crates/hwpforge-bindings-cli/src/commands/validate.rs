//! `validate` subcommand: check that an HWPX document satisfies Core's
//! structural invariants (`Document::validate`) without editing it.
//!
//! MCP already exposes this over `hwpforge_validate`
//! (`crates/hwpforge-bindings-mcp/src/tools/validate.rs`) and Python over
//! `Document.validate()`, both through [`ops::validate`] — this command was
//! the one purpose-fit gap (W5b finding). It is a new command, not part of
//! the W3/W4 `ops` migration `compat.rs`'s module docs describe, so its
//! `(code, hint, exit)` shape has no legacy CLI to reproduce; it is frozen
//! from this commit forward instead (`tests/data/new_codes.txt`'s header).

use std::path::Path;

use serde::Serialize;

use hwpforge::ops::{self, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// The `validate --json` result.
///
/// Field names deliberately diverge from [`hwpforge::ops::ValidateReport`]:
/// that type's `ok` is the shared `ops` report vocabulary (every `ops::*`
/// output that carries a verdict alongside data uses it), but this command
/// exists to answer exactly one question — "is this document valid" — so
/// the wire shape says `valid`, matching the MCP tool's
/// `ValidateData::valid` (`tools/validate.rs`) rather than the `ops`-wide
/// spelling. There is no separate `data` envelope key (every sibling
/// command's `--json` output is a flat `{"status": …, <fields>}` object —
/// see `inspect.rs`'s `InspectResult`, `fields.rs`, `diff.rs`; none of them
/// nest a `data` field, so this command does not invent one either).
#[derive(Serialize)]
struct ValidateResult {
    status: &'static str,
    valid: bool,
    sections: usize,
    paragraphs: usize,
    /// The validation errors; empty when `valid` is true. Always serialized
    /// (no `skip_serializing_if`) — this is a new command, so the
    /// fixed-key-set convention applies (`error.rs` module docs, "`warnings`
    /// / `errors` omission convention"), not the omit-if-empty shape
    /// existing commands keep for 0.16.5 wire compatibility.
    ///
    /// `errors[].code` is `ops::style::validate`'s own `"VALIDATION_FAILED"`
    /// (its rustdoc: "the code the caller asked for"), NOT `to-md`'s legacy
    /// `"VALIDATE_FAILED"` spelling for the same underlying
    /// `Document::validate` failure (`compat.rs`'s `TABLE` row for
    /// `Command::ToMd`). `to-md` reconstructs a frozen pre-migration error
    /// code for its own pipeline failure; this command has no legacy
    /// spelling to preserve and passes `ops`'s report field through
    /// verbatim.
    errors: Vec<WarningInfo>,
    /// Decode warnings raised on the way in — present whether the document
    /// validated or not (same fixed-key-set convention as `errors` above).
    warnings: Vec<WarningInfo>,
}

/// Run the validate command.
///
/// # Exit codes are report semantics, not error semantics
///
/// This command can report exactly two kinds of "not fine": the bytes could
/// not even be read as HWPX, or they decoded into a document that fails
/// `Document::validate`. Only the first is an [`ops::OpsError`] — the
/// second is [`ops::ValidateOutput`]'s own `ok: false` verdict, which
/// `ops::validate`'s docs frame explicitly as an answer, not a failure
/// ("a failed check is not an error"). The MCP `hwpforge_validate` tool
/// already treats it that way, returning `Ok(ValidateData { valid: false,
/// .. })` rather than a `ToolErrorInfo` (`tools/validate.rs`).
///
/// This command mirrors that rather than reconstructing an error envelope
/// the way `to-md`/`convert`/`from-json`/`to-pdf` do for the same
/// `Document::validate` failure inside THEIR own pipelines (where an
/// invalid document really does mean "cannot produce the requested
/// output"). The alternative — folding an invalid-but-decoded document into
/// `status: "error"` alongside a genuine decode failure — would leave the
/// two distinguishable only by `code` inside one shared shape. Splitting
/// them at the exit-code and envelope level instead means a script can
/// branch on exit code alone, and in text mode the two also land on
/// different streams (the summary always to stdout; decode failure prints
/// `Error: …` to stderr via [`CliError::exit`], same as every other
/// command).
///
/// Every exit code is its own class, not shared with a different kind of
/// failure (audit finding — exit 1 used to cover both a file/argument
/// error and a decoded-but-invalid document, so a script could not branch
/// on exit code alone the way this doc claimed):
///
/// - decoded and valid → exit **0**, `{"status":"ok","valid":true,…}`.
/// - file unreadable → exit **1**, `FILE_READ_FAILED`, constructed directly
///   like every sibling read command (`fields.rs`, `inspect.rs`, …) —
///   `compat.rs`'s `CLI_LOCAL` table carries this pair, not `TABLE`. Every
///   other sibling command's argument/file-I/O errors are exit 1 too, so
///   `validate` keeps that class rather than inventing its own.
/// - undecodable bytes → exit **2**, `DECODE_FAILED`, via
///   [`compat::cli_error`] like every other read command. Unlike the MCP
///   tool (which folds this into `valid: false` — a pre-migration contract
///   this command has no obligation to repeat), a decode failure here is a
///   genuine [`ops::OpsError`] and is not folded into `valid: false`: "the
///   bytes could not be read" and "the document read fine but is unsound"
///   are different questions, and only `ops`'s own error variant should
///   decide which one applies.
/// - decoded but invalid → exit **3**, `{"status":"ok","valid":false,…}` on
///   stdout — not routed through [`CliError`] at all. Its own class,
///   distinct from both the file/argument-error class (1) and the
///   codec/decode-failure class (2) every sibling command uses those for.
///
/// Warnings from a successful decode (`ValidateOutput::warnings`) print as
/// `[validate] <message>` on stderr in text mode only — the `--json` output
/// already carries them in the `warnings` array, so a second stderr copy
/// there would be redundant, unlike `convert`/`from-json`'s unconditional
/// `[cmd]` lines (those commands' final JSON carries no `warnings` field at
/// all, so stderr is their only channel in either mode).
pub fn run(file: &Path, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = crate::error::read_input(file, json_mode);

    let out = match ops::validate(&bytes) {
        Ok(o) => o,
        Err(e) => compat::exit_ops_error(Command::Validate, e, json_mode),
    };

    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    if !json_mode {
        for w in &warnings {
            eprintln!("[validate] {}", w.message);
        }
    }

    let result = ValidateResult {
        status: "ok",
        valid: out.ok,
        sections: out.sections,
        paragraphs: out.paragraphs,
        errors: out.errors.clone(),
        warnings,
    };

    if json_mode {
        // `to_string`, not `.unwrap()` (audit finding): `ValidateResult`'s
        // fields are all plain data (`bool`/`usize`/`WarningInfo`s built
        // from `String`s already round-tripped through JSON once by the
        // decoder), so this cannot fail today — but a panic path here would
        // still crash instead of reporting, the one thing every other exit
        // path in this command avoids. `JSON_SERIALIZE_FAILED`/exit 2
        // mirrors `to-json`'s own `render_pretty` (`to_json.rs`) for the
        // same "the result value refused to serialize" condition; unlike
        // that command's rows, this one has no pre-migration legacy to
        // audit (module docs) — new code, not a divergence.
        match serde_json::to_string(&result) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                CliError::new(
                    "JSON_SERIALIZE_FAILED",
                    format!("Failed to serialize validate result: {e}"),
                )
                .exit(json_mode, 2);
            }
        }
    } else {
        println!("Document: {}", file.display());
        println!("  Valid: {}", result.valid);
        println!("  Sections: {}", result.sections);
        println!("  Paragraphs: {}", result.paragraphs);
        if !result.errors.is_empty() {
            println!("  Errors:");
            for e in &result.errors {
                println!("    [{}] {}", e.code, e.message);
            }
        }
    }

    if !out.ok {
        // Exit 3 — its own class, not shared with exit 1's file/argument-error
        // class (this function's own "Exit codes are report semantics, not
        // error semantics" doc above).
        std::process::exit(3);
    }
}
