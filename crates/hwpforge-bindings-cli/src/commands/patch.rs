//! Patch a section in an existing HWPX file.

use std::path::PathBuf;

use hwpforge::ops::exchange::{patch as ops_patch, PatchOptions};
use hwpforge::ops::OpsError;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the patch command.
pub fn run(
    base: &PathBuf,
    section_idx: usize,
    section_json: &PathBuf,
    output: &PathBuf,
    json_mode: bool,
) {
    // Read base HWPX
    check_file_size(base, json_mode);
    let base_bytes = match std::fs::read(base) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", base.display()))
                .exit(json_mode, 1);
        }
    };
    // Read section JSON
    check_file_size(section_json, json_mode);
    let json_str = match std::fs::read_to_string(section_json) {
        Ok(s) => s,
        Err(e) => {
            CliError::new(
                "FILE_READ_FAILED",
                format!("Cannot read '{}': {e}", section_json.display()),
            )
            .exit(json_mode, 1);
        }
    };

    // `ops::exchange::patch` reproduces the whole legacy pipeline itself:
    // parse the patch JSON, deserialize it as an `ExportedSection`, verify
    // its grid addresses against it, then apply the preserving patch.
    let opts = PatchOptions::default().with_section(section_idx).with_patch(json_str);
    let outcome = match ops_patch(&base_bytes, &opts) {
        Ok(o) => o,
        Err(e) => exit_ops_error(Command::Patch, e, json_mode),
    };

    if let Err(e) = std::fs::write(output, &outcome.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "patched_section": outcome.section,
        "sections": outcome.sections,
        "size_bytes": outcome.bytes.len(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Patched section {} -> {} ({} bytes)",
            outcome.section,
            output.display(),
            outcome.bytes.len()
        );
    }
}

/// Maps an `ops::exchange::patch` failure onto the frozen contract and exits.
fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    let ce = compat::cli_error(cmd, err);
    let exit = compat::exit_code(cmd, &ce);
    ce.exit(json_mode, exit);
}
