//! Inter-crate version requirements.
//!
//! Every `hwpforge*` path dependency that carries a `version` requirement must
//! read exactly the workspace version as a plain caret requirement
//! (`"0.17.1"`). `"0"` let a user update one crate past a breaking minor while
//! the lock file kept an older sibling; the exact version makes cargo pull the
//! sibling forward. release-plz rewrites these requirements in the Release PR,
//! but only plain forms: a range such as `">=0.17.0, <1"` makes
//! `release-plz update` fail (see `RELEASING.md`).
//!
//! The requirements come from `cargo metadata`, so every manifest form cargo
//! accepts (quoted or dotted tables, `workspace = true` inheritance) is seen
//! as cargo resolves it.
//!
//! 이것을 실패시키는 것: release-plz 가 요구를 다시 쓰지 않은 Release PR, "0" 이나 범위 꼴로 되돌리기

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Versioned inter-crate requirements in the workspace when this test was
/// written; fewer means a dependency was dropped from the check.
const MIN_CHECKED: usize = 27;

#[test]
fn inter_crate_version_requirements_match_the_workspace_version() {
    let expected = format!("^{}", env!("CARGO_PKG_VERSION"));
    let workspace_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(&workspace_manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("cargo metadata JSON");

    let mut checked = 0usize;
    let mut violations = Vec::new();
    for package in metadata["packages"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("package name");
        // `publish = false` shows up as an empty registry list.
        let published =
            package["publish"].as_array().is_none_or(|registries| !registries.is_empty());
        for dep in package["dependencies"].as_array().expect("dependencies") {
            let dep_name = dep["name"].as_str().expect("dependency name");
            if !dep_name.starts_with("hwpforge") || dep["path"].is_null() {
                continue;
            }
            let req = dep["req"].as_str().expect("dependency req");
            // A path-only dependency (no `version`) is allowed only where nothing is published.
            if req == "*" && !published {
                continue;
            }
            checked += 1;
            if req != expected {
                let kind = dep["kind"].as_str().unwrap_or("normal");
                violations.push(format!(
                    "{name} -> {dep_name} ({kind}): requires {req:?}, expected {expected:?}"
                ));
            }
        }
    }

    assert!(
        checked >= MIN_CHECKED,
        "checked only {checked} inter-crate requirements, expected at least {MIN_CHECKED}"
    );
    assert!(
        violations.is_empty(),
        "inter-crate version requirement must be {expected:?}:\n{}",
        violations.join("\n")
    );
}
