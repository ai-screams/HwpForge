//! Diagnostic-preserving carriers for the facades that decode or encode
//! internally.
//!
//! # Why these exist
//!
//! Most facades in this crate take bytes, decode (or encode) them, and return
//! only the projection the caller asked for. The diagnostics the codec
//! produced on the way are dropped, so a caller that advertises a `warnings`
//! channel can only ever report an empty one.
//!
//! Every result type that would otherwise need a new field is already
//! published without `#[non_exhaustive]`, so adding one is a breaking change.
//! The additive fix is a **twin entry point** — `*_with_diagnostics` — that
//! returns the same payload wrapped in one of the carriers below. The
//! original entry point stays, unchanged in signature and behaviour, as a
//! thin wrapper that drops the warnings; that keeps existing consumers byte
//! identical while giving the operations layer a lossless path.
//!
//! There are three carriers — one per codec pass a facade can run, plus
//! [`WithCodecWarnings`] for the regenerating editors that run both. All are
//! `#[non_exhaustive]`, so a further diagnostic channel can be added later
//! without breaking the callers added today.

use crate::decoder::DecodeWarning;
use crate::encoder::EncodeWarning;

/// A projection plus the decoder warnings produced while reading it.
///
/// Returned by the `*_with_diagnostics` twins of the read-only facades
/// ([`HwpxReader`](crate::HwpxReader), [`HwpxFiller::list_fields`](crate::HwpxFiller::list_fields))
/// and by the section workflow, whose payload is produced by decoding the
/// input package.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_smithy_hwpx::HwpxReader;
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let diagnosed = HwpxReader::outline_with_diagnostics(&bytes)?;
/// for warning in &diagnosed.warnings {
///     eprintln!("decode warning: {warning}");
/// }
/// println!("{} table(s)", diagnosed.value.tables.len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WithDecodeWarnings<T> {
    /// The payload the twin's plain counterpart returns on its own.
    pub value: T,
    /// Every warning the decoder raised for this input, in decoder order.
    pub warnings: Vec<DecodeWarning>,
}

impl<T> WithDecodeWarnings<T> {
    /// Pairs a payload with the decoder warnings that accompanied it.
    #[must_use]
    pub fn new(value: T, warnings: Vec<DecodeWarning>) -> Self {
        Self { value, warnings }
    }

    /// Drops the warnings, yielding what the plain entry point returns.
    ///
    /// This is how each original facade is expressed as a thin wrapper over
    /// its twin, so the two can never disagree about the payload.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

/// A result plus the **non-semantic** encoder warnings the successful encode
/// produced.
///
/// Semantic-loss warnings are never carried here: the regenerating editors
/// ([`HwpxCellEditor`](crate::HwpxCellEditor),
/// [`HwpxStamper`](crate::stamp::HwpxStamper)) are preserve-first, so an
/// encode that reports semantic loss fails closed with no bytes at all. What
/// reaches this carrier is the remainder — diagnostics that describe the
/// output without claiming its meaning changed.
///
/// # Scope
///
/// The warnings come from the single encode that produced the returned
/// bytes. The admission gate's no-op round trip and the v2 fixed-point check
/// encode too, but those outputs are discarded verification artefacts, so
/// their diagnostics are not reported as if they described the result.
#[derive(Debug)]
#[non_exhaustive]
pub struct WithEncodeWarnings<T> {
    /// The payload the twin's plain counterpart returns on its own.
    pub value: T,
    /// Non-semantic encoder warnings from the encode that produced the
    /// output bytes, in encoder order.
    pub warnings: Vec<EncodeWarning>,
}

impl<T> WithEncodeWarnings<T> {
    /// Pairs a result with the encoder warnings that accompanied it.
    #[must_use]
    pub fn new(value: T, warnings: Vec<EncodeWarning>) -> Self {
        Self { value, warnings }
    }

    /// Drops the warnings, yielding what the plain entry point returns.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

/// A result plus **both** codec channels: what decoding the input reported,
/// and the non-semantic warnings of the encode that produced the output.
///
/// Returned by the twins of the regenerating editors that do both halves in
/// one call — today [`HwpxStamper::stamp_with_diagnostics`](crate::stamp::HwpxStamper::stamp_with_diagnostics)
/// and its v2 sibling. A facade that only decodes uses
/// [`WithDecodeWarnings`]; one whose only internal codec pass is the encode
/// uses [`WithEncodeWarnings`].
///
/// # Scope
///
/// `decode_warnings` comes from the **input** decode alone. A regenerating
/// edit decodes several other packages on the way — the admission gate's
/// no-op round trip, the v2 fixed-point check, and the output re-read that
/// builds the manifest — but each of those reads bytes that are either
/// discarded verification artefacts or derived from the input decode already
/// reported, so carrying them would restate a subset as if it were new.
///
/// `encode_warnings` keeps the contract [`WithEncodeWarnings`] documents:
/// semantic loss never appears here, because it is the fail-closed path and
/// produces no bytes at all.
#[derive(Debug)]
#[non_exhaustive]
pub struct WithCodecWarnings<T> {
    /// The payload the twin's plain counterpart returns on its own.
    pub value: T,
    /// Every warning the decoder raised for the **input**, in decoder order.
    pub decode_warnings: Vec<DecodeWarning>,
    /// Non-semantic encoder warnings from the encode that produced the
    /// output bytes, in encoder order.
    pub encode_warnings: Vec<EncodeWarning>,
}

impl<T> WithCodecWarnings<T> {
    /// Pairs a result with both codec channels.
    #[must_use]
    pub fn new(
        value: T,
        decode_warnings: Vec<DecodeWarning>,
        encode_warnings: Vec<EncodeWarning>,
    ) -> Self {
        Self { value, decode_warnings, encode_warnings }
    }

    /// Drops both channels, yielding what the plain entry point returns.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::ParagraphPath;

    fn path() -> ParagraphPath {
        ParagraphPath(vec![crate::decoder::PathSeg::Section(0)])
    }

    #[test]
    fn into_value_drops_only_the_warnings() {
        let diagnosed = WithDecodeWarnings::new(
            42_u32,
            vec![DecodeWarning::LayoutCacheDropped { path: path(), reason: "probe".to_string() }],
        );

        assert_eq!(diagnosed.warnings.len(), 1);
        assert_eq!(diagnosed.into_value(), 42);
    }

    #[test]
    fn the_codec_carrier_keeps_its_two_channels_apart() {
        let decode = DecodeWarning::LayoutCacheDropped { path: path(), reason: "in".to_string() };
        let encode = EncodeWarning::LayoutCacheDropped { path: path(), reason: "out".to_string() };

        let diagnosed = WithCodecWarnings::new("bytes", vec![decode.clone()], vec![encode.clone()]);

        assert_eq!(diagnosed.decode_warnings, vec![decode]);
        assert_eq!(diagnosed.encode_warnings, vec![encode]);
        assert_eq!(diagnosed.into_value(), "bytes");
    }

    #[test]
    fn the_encode_carrier_keeps_the_warning_order_it_was_given() {
        let first = EncodeWarning::LayoutCacheDropped { path: path(), reason: "first".to_string() };
        let second =
            EncodeWarning::LayoutCacheDropped { path: path(), reason: "second".to_string() };

        let diagnosed = WithEncodeWarnings::new((), vec![first.clone(), second.clone()]);

        assert_eq!(diagnosed.warnings, vec![first, second]);
    }
}
