#![no_main]
//! Fuzz the HWPX decode path on untrusted bytes.
//!
//! Exercises the ZIP/OWPML reader (`PackageReader`, header/section/metadata
//! XML parsing). The invariant under test: no input may panic or OOM — every
//! malformed `.hwpx` must surface as `Err`, never a crash.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = hwpforge_smithy_hwpx::HwpxDecoder::decode(data);
});
