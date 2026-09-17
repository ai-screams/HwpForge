//! Snapshot of the stable `OpsCode` wire strings.
//!
//! The strings are a public contract: CLI JSON errors, MCP tool errors and
//! `HwpForgeError.code` in Python all expose them. Adding a code is
//! additive; renaming or removing one is a breaking change, and this
//! snapshot is where that shows up.
//!
//! Regenerate after an intentional change:
//! `UPDATE_INVENTORY=1 cargo nextest run -p hwpforge --all-features`.
#![cfg(feature = "ops-hwpx")]

use hwpforge::foundation::diagnostics::OpsCode;

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ops_codes.txt");

fn rendered() -> String {
    let mut wires: Vec<&'static str> = OpsCode::ALL.iter().map(|c| c.as_str()).collect();
    wires.sort_unstable();
    wires.join("\n") + "\n"
}

#[test]
fn wire_strings_match_the_tracked_snapshot() {
    let rendered = rendered();

    if std::env::var_os("UPDATE_INVENTORY").is_some() {
        std::fs::write(TRACKED, &rendered).expect("write snapshot");
        return;
    }

    let tracked = std::fs::read_to_string(TRACKED).expect("tracked snapshot missing");
    if tracked != rendered {
        let old: Vec<&str> = tracked.lines().collect();
        let new: Vec<&str> = rendered.lines().collect();
        let added: Vec<&&str> = new.iter().filter(|w| !old.contains(w)).collect();
        let removed: Vec<&&str> = old.iter().filter(|w| !new.contains(w)).collect();
        panic!(
            "OpsCode wire strings changed — added {added:?}, removed {removed:?}. \
             Removing or renaming one breaks the CLI, MCP and Python contract; \
             if the change is intended, rerun with UPDATE_INVENTORY=1."
        );
    }
}

#[test]
fn every_code_round_trips_and_is_unique() {
    let wires: Vec<&'static str> = OpsCode::ALL.iter().map(|c| c.as_str()).collect();
    let mut sorted = wires.clone();
    sorted.sort_unstable();
    sorted.dedup();

    assert_eq!(sorted.len(), wires.len(), "duplicate wire string");
    for code in OpsCode::ALL {
        assert_eq!(OpsCode::lookup(code.as_str()), Some(*code));
    }
}
