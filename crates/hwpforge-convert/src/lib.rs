//! Cross-format conversion orchestrator for HwpForge.
//!
//! This crate sits *above* the format-specific Smithy crates and wires them
//! together through the neutral Core IR: `decode(format A) -> Core Document ->
//! encode(format B)`. Keeping orchestration here lets each Smithy crate depend
//! only on Core (peer-equality) and opens a path for additional output formats
//! without modifying the decoders.
#![deny(missing_docs)]
