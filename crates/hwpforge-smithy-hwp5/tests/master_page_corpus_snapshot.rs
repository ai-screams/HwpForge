//! Opt-in Core evidence for before/after corpus comparison. No local fixture paths.
//! Run with HWP5_SNAPSHOT_LIST and HWP5_SNAPSHOT_OUT and `--ignored`.
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires an explicit corpus list and output directory"]
fn master_page_core_snapshot() {
    let list =
        fs::read_to_string(std::env::var("HWP5_SNAPSHOT_LIST").expect("corpus list")).unwrap();
    let out = PathBuf::from(std::env::var("HWP5_SNAPSHOT_OUT").expect("snapshot directory"));
    fs::create_dir_all(&out).unwrap();
    for (index, path) in list.lines().enumerate() {
        let bytes = fs::read(path).unwrap();
        if bytes.starts_with(b"PK") {
            continue;
        }
        let decoded = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
        let semantic = hwpforge_smithy_hwp5::build_hwp5_semantic(&bytes).unwrap();
        fs::write(
            out.join(format!("{index}.json")),
            serde_json::to_vec(&decoded.document).unwrap(),
        )
        .unwrap();
        fs::write(
            out.join(format!("{index}-semantic.json")),
            serde_json::to_vec(&semantic).unwrap(),
        )
        .unwrap();
    }
}
