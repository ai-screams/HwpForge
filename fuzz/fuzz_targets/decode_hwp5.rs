#![no_main]
//! Fuzz the HWP5 decode path on untrusted bytes.
//!
//! Exercises CFB open + `Record::parse_stream` + section/DocInfo/BinData
//! parsing + `\x05HwpSummaryInformation` PropertySet decoding transitively.
//! The invariant under test: no input may panic or OOM — every malformed
//! file must surface as `Err`, never a crash.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = hwpforge_smithy_hwp5::decode_hwp5_with_images(data);
});
