//! Finite loss categories; only the first example of each category is retained.
use crate::error::{quote, DocxError, Result};

macro_rules! warnings {
    ($($variant:ident => ($code:literal, $doc:literal)),+ $(,)?) => {
        /// Non-fatal DOCX losses, aggregated in first-occurrence order.
        ///
        /// `part` and `location` always describe the first occurrence, even when
        /// that occurrence has no known location. Decoder-produced counts start
        /// at one. Loss/action strings are finite decoder-owned classifications,
        /// never input names, IDs, or values. Format is `docx`, stage is `decode`.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum DocxWarning {
            $(#[doc = $doc]
            #[non_exhaustive]
            $variant {
                /// First package part, or unknown.
                part: Option<String>,
                /// First short logical location, or unknown.
                location: Option<String>,
                /// Bounded explanation of the first example.
                reason: String,
                /// Total occurrences, including omitted later locations.
                count: u64,
                /// Stable classification of the lost meaning.
                loss: &'static str,
                /// Stable classification of the replacement behavior.
                action: &'static str,
            }),+
        }
        impl DocxWarning {
            /// Stable, format-specific diagnostic code.
            pub fn code(&self) -> &'static str {
                match self { $(Self::$variant { .. } => $code),+ }
            }
            /// Stable loss classification used by warning aggregation.
            pub fn loss(&self) -> &'static str {
                match self { $(Self::$variant { loss, .. } => loss),+ }
            }
            /// Stable replacement action used by warning aggregation.
            pub fn action(&self) -> &'static str {
                match self { $(Self::$variant { action, .. } => action),+ }
            }
            /// First affected part; later occurrences never replace it.
            pub fn part(&self) -> Option<&str> {
                match self { $(Self::$variant { part, .. } => part.as_deref()),+ }
            }
            /// First affected location; later occurrences never replace it.
            pub fn location(&self) -> Option<&str> {
                match self { $(Self::$variant { location, .. } => location.as_deref()),+ }
            }
            /// Total occurrences in this loss group.
            pub fn count(&self) -> u64 {
                match self { $(Self::$variant { count, .. } => *count),+ }
            }
            /// Human-readable first example and, when repeated, total count.
            pub fn message(&self) -> String {
                let reason = match self { $(Self::$variant { reason, .. } => reason),+ };
                if self.count() == 1 { reason.clone() } else {
                    format!("{reason} (total {} occurrences; first example only; subsequent locations omitted)", self.count())
                }
            }
            fn count_mut(&mut self) -> &mut u64 {
                match self { $(Self::$variant { count, .. } => count),+ }
            }
        }
        #[derive(Clone, Copy)]
        pub(crate) enum Kind { $($variant),+ }
        impl Kind {
            fn code(self) -> &'static str { match self { $(Self::$variant => $code),+ } }
            fn at(self, part: Option<&str>, location: Option<impl std::fmt::Display>, reason: impl std::fmt::Display,
                loss: &'static str, action: &'static str) -> DocxWarning {
                let part = part.map(quote);
                let location = location.map(quote);
                let reason = quote(reason);
                match self { $(Self::$variant => DocxWarning::$variant { part, location, reason, count: 1, loss, action }),+ }
            }
        }
    };
}
warnings! {
    DocumentSettingsOmitted => ("DOCX_DOCUMENT_SETTINGS_OMITTED", "Document settings are not applied."),
    StructuredWrapperFlattened => ("DOCX_STRUCTURED_WRAPPER_FLATTENED", "Wrapper meaning is discarded while child content is retained."),
    FormattingOmitted => ("DOCX_FORMATTING_OMITTED", "Character, paragraph, list, or section formatting uses defaults."),
    ContentOmitted => ("DOCX_CONTENT_OMITTED", "An unsupported object or content subtree was excluded."),
    WhitespaceApproximated => ("DOCX_WHITESPACE_APPROXIMATED", "Boundary whitespace without explicit preservation was kept."),
    FieldInstructionOmitted => ("DOCX_FIELD_INSTRUCTION_OMITTED", "A field keeps only its saved result."),
    FieldResultMissing => ("DOCX_FIELD_RESULT_MISSING", "A field has no saved result and contributes empty text."),
    NestedFieldFlattened => ("DOCX_NESTED_FIELD_FLATTENED", "Nested field behavior was replaced by saved results."),
    LinkTargetOmitted => ("DOCX_LINK_TARGET_OMITTED", "Hyperlink text remains but its target was discarded."),
    ContentControlFlattened => ("DOCX_CONTENT_CONTROL_FLATTENED", "Content control body remains without control behavior."),
    AlternateContentSelected => ("DOCX_ALTERNATE_CONTENT_SELECTED", "A fallback branch was selected without feature negotiation."),
    OptionalPartOmitted => ("DOCX_OPTIONAL_PART_OMITTED", "An optional part could not be selected or is not interpreted."),
    BreakOmitted => ("DOCX_BREAK_OMITTED", "A page or column break was discarded without splitting paragraphs."),
    AttributeOmitted => ("DOCX_ATTRIBUTE_OMITTED", "An unsupported attribute was ignored."),
    ValueApproximated => ("DOCX_VALUE_APPROXIMATED", "An unsupported value used the reported replacement."),
}

pub(crate) fn record<'a, L: std::fmt::Display, R: std::fmt::Display>(
    warnings: &mut Vec<DocxWarning>,
    kind: Kind,
    loss: &'static str,
    action: &'static str,
    example: impl FnOnce() -> (Option<&'a str>, Option<L>, R),
) -> Result<()> {
    let key = (kind.code(), loss, action);
    if let Some(first) = warnings.iter_mut().find(|w| (w.code(), w.loss(), w.action()) == key) {
        *first.count_mut() = first.count().checked_add(1).ok_or_else(|| {
            DocxError::invalid(
                first.part().unwrap_or("#unknown"),
                first.location().unwrap_or("#unknown"),
                "warning count overflow (u64)",
            )
        })?;
    } else {
        let (part, location, reason) = example();
        warnings.push(kind.at(part, location, reason, loss, action));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn take_diagnostic_bytes() -> usize {
    crate::error::take_quoted_bytes()
}
