#![allow(dead_code)]

/// A package whose decode raises `LAYOUT_CACHE_DROPPED` and which is still
/// admissible.
///
/// It is this codec's own output with the last paragraph's
/// `<hp:linesegarray>` pointing past the end of that paragraph's text — the
/// shape a third-party edit leaves behind — so the decoder refuses to
/// promote a guessed coordinate and reports the drop instead. Being our own
/// output is what keeps it admissible: the two native documents in the tree
/// that warn both fail the gate on uncarried ZIP entries.
///
/// The same fixture backs the decode-warning tests of `fill`, `insert_para`,
/// `delete_para` and `stamp`, so the four surfaces cannot disagree about
/// what "a document that warns" means.
pub(crate) fn warning_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/");
    std::fs::read(format!("{path}stale-line-cache.hwpx")).expect("stale-line-cache.hwpx")
}

pub(crate) fn hwpx_fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub(crate) fn repo_fixture(relative: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/");
    std::fs::read(format!("{path}{relative}")).unwrap_or_else(|e| panic!("fixture {relative}: {e}"))
}
