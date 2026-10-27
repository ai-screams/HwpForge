#![allow(dead_code)]

#[path = "process.rs"]
mod process;

use hwpforge_core::{Paragraph, RunContent};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use zip::ZipArchive;

pub(super) static COUNTER: AtomicUsize = AtomicUsize::new(0);

// ─── Helpers ───

/// Path to the built binary (set by cargo for integration tests).
pub(super) fn hwpforge_bin() -> PathBuf {
    process::hwpforge_bin()
}

/// Create a unique temp directory for each test.
pub(super) fn test_tmp() -> PathBuf {
    // nextest runs every test in its own process, so `COUNTER` restarts at 0
    // each time and the directory name used to be `hwpforge_test_0_<pid>`.
    // Directories are never removed, and the OS recycles pids, so a later
    // test process could inherit a directory that an earlier one had already
    // populated (`patch_section_index_mismatch_fails_fast` saw a stale
    // `patched.hwpx` and failed). Fold the wall clock in and start from an
    // empty directory.
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir =
        std::env::temp_dir().join(format!("hwpforge_test_{id}_{}_{nanos}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear stale temp dir");
    }
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

pub(super) fn read_hwpx_entry(path: &Path, entry: &str) -> String {
    let bytes = std::fs::read(path).expect("read hwpx");
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).expect("open hwpx zip");
    let mut file = archive.by_name(entry).expect("zip entry exists");
    let mut content = String::new();
    file.read_to_string(&mut content).expect("read zip entry as string");
    content
}

pub(super) fn replace_first_text_run(paragraphs: &mut [Paragraph], replacement: &str) -> bool {
    for paragraph in paragraphs {
        for run in &mut paragraph.runs {
            match &mut run.content {
                RunContent::Text(text) => {
                    *text = replacement.to_string();
                    return true;
                }
                // Downgrade-on-modify: same policy as the MCP patch
                // helper. See debug doc §3a-C19.
                RunContent::InlineText(_) => {
                    run.content = RunContent::Text(replacement.to_string());
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}

/// Run hwpforge with given args, return (stdout, stderr, exit_code).
pub(super) fn run(args: &[&str]) -> (String, String, i32) {
    process::run(args)
}

/// Run hwpforge with --json flag prepended.
pub(super) fn run_json(args: &[&str]) -> (serde_json::Value, String, i32) {
    let (_, value, stderr, code) = run_json_with_stdout(args);
    (value, stderr, code)
}

/// Run hwpforge with --json flag prepended and return raw stdout too.
pub(super) fn run_json_with_stdout(args: &[&str]) -> (String, serde_json::Value, String, i32) {
    let mut full_args = vec!["--json"];
    full_args.extend_from_slice(args);
    let (stdout, stderr, code) = run(&full_args);
    if code == 0 {
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("invalid JSON output: {e}\nstdout: {stdout}"));
        (stdout, value, stderr, code)
    } else {
        // Try to parse stderr as JSON error
        let err_value = serde_json::from_str(&stderr).unwrap_or(serde_json::Value::Null);
        (stdout, err_value, stderr, code)
    }
}

/// Assert a file is a valid HWPX by running inspect on it.
pub(super) fn assert_valid_hwpx(path: &Path) {
    let (_, _, code) = run(&["inspect", path.to_str().unwrap()]);
    assert_eq!(code, 0, "inspect failed on {}", path.display());

    let bytes = std::fs::read(path).expect("read hwpx");
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).expect("open hwpx zip");
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("zip entry by index");
        if !(file.name().expect("ZIP entry name").ends_with(".xml")
            || file.name().expect("ZIP entry name").ends_with(".hpf"))
        {
            continue;
        }

        let mut content = Vec::new();
        file.read_to_end(&mut content).expect("read xml-ish zip entry");
        assert!(
            !content.contains(&0),
            "xml entry {} contains NUL byte in {}",
            file.name().expect("ZIP entry name"),
            path.display()
        );
    }
}

// ═══════════════════════════════════════════════════════════════
// E3 Wave 2: 표 격자 주소 (grid addresses on JSON exports)
// ═══════════════════════════════════════════════════════════════

/// (paragraph, run) indices of the first table run in a document export.
pub(super) fn first_table_run_indices(root: &serde_json::Value) -> (usize, usize) {
    let paragraphs = root["document"]["sections"][0]["paragraphs"].as_array().expect("paragraphs");
    for (pi, paragraph) in paragraphs.iter().enumerate() {
        for (ri, run) in paragraph["runs"].as_array().expect("runs").iter().enumerate() {
            if run["content"]["Table"].is_object() {
                return (pi, ri);
            }
        }
    }
    panic!("no table run in export");
}

// ─── Bounded input reads (audit HIGH-1) ───
//
// `check_file_size`'s `metadata().len()` fast path is accurate for regular
// files but reports 0 for a FIFO — the actual guarantee is the bounded read
// underneath it (`error::read_input`/`read_bounded`). These two tests drive
// a real named pipe through the built binary: one under the cap (no
// regression on the ordinary small-file path), one past it (the new
// `INPUT_TOO_LARGE` guarantee a FIFO's lying `metadata()` used to let
// through unbounded).

#[cfg(unix)]
pub(super) fn create_fifo(path: &Path) {
    let status = Command::new("mkfifo").arg(path).status().expect("failed to run mkfifo");
    assert!(status.success(), "mkfifo failed for {}", path.display());
}
