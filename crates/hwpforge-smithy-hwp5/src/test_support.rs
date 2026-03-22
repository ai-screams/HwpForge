//! Test-only helpers for resolving shared workspace fixtures after fixture reorganization.

use std::path::{Path, PathBuf};

/// Returns the root shared fixture directory under `tests/fixtures`.
pub(crate) fn workspace_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Resolves a shared fixture path by relative path first, then by unique basename search.
pub(crate) fn workspace_fixture_path(name: &str) -> PathBuf {
    let root = workspace_fixture_root();
    let direct = root.join(name);
    if direct.exists() {
        return direct;
    }

    let file_name = Path::new(name)
        .file_name()
        .unwrap_or_else(|| panic!("fixture name must include a file name: {name}"));

    let mut stack = vec![root.clone()];
    let mut matches = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|err| {
            panic!("failed to read fixture directory {}: {err}", dir.display())
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|err| {
                panic!("failed to read fixture entry under {}: {err}", dir.display())
            });
            let path = entry.path();
            let file_type = entry.file_type().unwrap_or_else(|err| {
                panic!("failed to stat fixture entry {}: {err}", path.display())
            });
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name() == Some(file_name) {
                matches.push(path);
            }
        }
    }

    match matches.len() {
        0 => direct,
        1 => matches.pop().expect("one match implies pop succeeds"),
        _ => {
            matches.sort();
            let paths = matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            panic!("fixture name is ambiguous: {name} -> [{paths}]");
        }
    }
}
