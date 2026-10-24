//! The 1 MiB minimum stack documented on `HwpxDecoder::decode` for optimized
//! builds, checked on the public path with real packages.
//!
//! This lives in an integration test, not in the library's unit tests, so it
//! measures the library as it ships. The unit-test binary compiles the
//! `#[cfg(test)]` code together with the decoder, and with
//! `codegen-units = 1` a unit test that calls the deserializer on an inner
//! type changes how the recursive deserialize chain is inlined: there the
//! same inputs needed about twice the stack (1,329 KiB against 631 KiB for
//! B 44 on macOS arm64), while the shipped decoder did not change.
//!
//! Only optimized builds are covered (an unoptimized build needs more), so
//! the whole file is compiled out of debug builds and CI runs it with
//! `--cargo-profile release` in `Verify › Python`. An overflow aborts the
//! test process, so a failure shows as SIGABRT.
#![cfg(not(debug_assertions))]

#[path = "support/depth_package.rs"]
mod support;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxError};
use support::{nested_section, package, Place, DEEPEST};

/// The minimum stack `HwpxDecoder::decode` documents for optimized builds.
const DOCUMENTED_MIN_STACK: usize = 1 << 20;

/// Runs the public `HwpxDecoder::decode` on a thread with `stack` bytes;
/// returns the decoded sections' debug text.
fn decode_package_on_stack(bytes: Vec<u8>, stack: usize) -> Result<String, HwpxError> {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || HwpxDecoder::decode(&bytes).map(|d| format!("{:?}", d.document.sections())))
        .expect("spawn decode thread")
        .join()
        .expect("decode thread panicked")
}

/// B 44 is the deepest text-box input the recursion budget still admits and
/// the input that needs the most stack; B 100 is past the budget.
// 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 10000 으로 올리기 — B 100 이
// 한도에 막히지 않고 1 MiB 스택을 넘쳐 테스트 프로세스가 abort 한다.
// 스택을 512 KiB 로 줄이기 — B 44 가 abort 한다.
// 이것을 실패시키는 것: subList 가드 문구에서 depth + 1 → depth.
#[test]
fn xml_limit_release_decodes_on_documented_1_mib_stack() {
    let decode = |pattern: &str, levels: usize| {
        decode_package_on_stack(
            package(nested_section(pattern, levels), Place::Body),
            DOCUMENTED_MIN_STACK,
        )
    };
    for (pattern, levels) in [("T", 32), ("B", 32), ("TB", 32)] {
        let debug = decode(pattern, levels).expect("32 levels decode on 1 MiB");
        assert!(debug.contains(DEEPEST), "{pattern} x{levels}: deepest text lost");
    }
    assert!(matches!(
        decode("B", 44),
        Err(HwpxError::InvalidStructure { detail }) if detail == "sublist nesting depth 33 exceeds limit of 32 in Contents/section0.xml"
    ));
    assert!(matches!(
        decode("B", 100),
        Err(HwpxError::XmlParse { detail, .. }) if detail.contains("recursion limit of 224")
    ));
}
