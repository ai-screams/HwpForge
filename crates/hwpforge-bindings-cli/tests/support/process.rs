#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

pub(crate) fn hwpforge_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hwpforge"))
}

/// Runs the built `hwpforge` binary, returning `(stdout, stderr, exit_code)`.
pub(crate) fn run(args: &[&str]) -> (String, String, i32) {
    let output =
        Command::new(hwpforge_bin()).args(args).output().expect("failed to execute hwpforge");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}
