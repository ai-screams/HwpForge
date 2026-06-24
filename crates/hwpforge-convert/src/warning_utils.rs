//! Shared helpers for emitting HWP5 conversion warnings.

use hwpforge_smithy_hwp5::Hwp5Warning;

/// Push a [`Hwp5Warning::ProjectionFallback`] with the given subject and reason.
pub(crate) fn push_projection_fallback(
    warnings: &mut Vec<Hwp5Warning>,
    subject: &'static str,
    reason: impl Into<String>,
) {
    warnings.push(Hwp5Warning::ProjectionFallback { subject, reason: reason.into() });
}
