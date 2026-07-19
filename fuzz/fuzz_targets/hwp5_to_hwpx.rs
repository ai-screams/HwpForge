#![no_main]
//! Fuzz the full HWP5 → HWPX conversion pipeline on untrusted bytes.
//!
//! Exercises decode + projection + HWPX encode end to end. The invariant
//! under test: no input may panic or OOM — every malformed file must surface
//! as `Err`, never a crash.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = hwpforge_convert::hwp5_to_hwpx_bytes(data);
});
