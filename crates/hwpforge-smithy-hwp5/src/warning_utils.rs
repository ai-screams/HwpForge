use crate::decoder::Hwp5Warning;

pub(crate) fn push_projection_fallback(
    warnings: &mut Vec<Hwp5Warning>,
    subject: &'static str,
    reason: impl Into<String>,
) {
    warnings.push(Hwp5Warning::ProjectionFallback { subject, reason: reason.into() });
}
