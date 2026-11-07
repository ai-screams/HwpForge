//! Context-bearing errors at the DOCX input boundary.

/// Result of decoding a DOCX package into Core.
pub type DocxResult<T> = std::result::Result<T, DocxError>;
pub(crate) use DocxResult as Result;

// 이것을 실패시키는 것: 각 오류 variant의 non_exhaustive를 제거하면 해당 compile_fail 계약이 실패.
/// Fatal package, XML, resource, or semantic input failure.
///
/// Variant fields may grow; downstream matches must include `..`.
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::MissingPart { part } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::AmbiguousPart { part } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::OpaquePart { part } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::MissingContentType { part } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::ContentTypeMismatch { part, actual, expected } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::UnsupportedEncoding { part, encoding } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::Invalid { part, element, detail } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::Limit { part, element, resource, observed, limit } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::Package { part, source } = error {}
/// }
/// ```
///
/// ```compile_fail,E0638
/// use hwpforge_smithy_docx::DocxError;
/// fn inspect(error: DocxError) {
///     if let DocxError::TrackedChange { part, location, element } = error {}
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DocxError {
    /// A required part is absent.
    #[error("DOCX part '{part}': part entries observed 0, required 1")]
    #[non_exhaustive]
    MissingPart {
        /// First affected package part, bounded for diagnostics.
        part: String,
    },
    /// Case-insensitive lookup found multiple entries.
    #[error("DOCX part '{part}': ambiguous ASCII case-insensitive entry lookup")]
    #[non_exhaustive]
    AmbiguousPart {
        /// First affected package part, bounded for diagnostics.
        part: String,
    },
    /// An opaque entry cannot be selected.
    #[error("DOCX part '{part}': opaque part selected (observed 1, limit 0)")]
    #[non_exhaustive]
    OpaquePart {
        /// First affected package part, bounded for diagnostics.
        part: String,
    },
    /// No content type is declared for the selected part.
    #[error("DOCX part '{part}': content type definitions observed 0, required 1")]
    #[non_exhaustive]
    MissingContentType {
        /// First affected package part, bounded for diagnostics.
        part: String,
    },
    /// The declared MIME type disagrees with the relationship.
    #[error("DOCX part '{part}': content type '{actual}' does not match '{expected}'")]
    #[non_exhaustive]
    ContentTypeMismatch {
        /// First affected package part, bounded for diagnostics.
        part: String,
        /// Observed MIME type.
        actual: String,
        /// Required MIME classification.
        expected: &'static str,
    },
    /// Only UTF-8 XML is accepted.
    #[error("DOCX part '{part}': {encoding} XML part unsupported; expected UTF-8")]
    #[non_exhaustive]
    UnsupportedEncoding {
        /// First affected package part, bounded for diagnostics.
        part: String,
        /// Observed encoding label.
        encoding: String,
    },
    /// The input violates the accepted syntax or semantic contract.
    #[error("invalid DOCX part '{part}' at <{element}>: {detail}")]
    #[non_exhaustive]
    Invalid {
        /// First affected package part, bounded for diagnostics.
        part: String,
        /// Element or short logical location.
        element: String,
        /// Reason the input cannot be decoded.
        detail: String,
    },
    /// A resource limit would be exceeded.
    #[error("DOCX part '{part}' at <{element}>: {resource} observed {observed}, limit {limit}")]
    #[non_exhaustive]
    Limit {
        /// First affected package part, bounded for diagnostics.
        part: String,
        /// Element or short logical location.
        element: String,
        /// Resource being counted.
        resource: &'static str,
        /// Observed or requested quantity.
        observed: usize,
        /// Maximum admitted quantity.
        limit: usize,
    },
    /// Core could not open or read the bounded ZIP package.
    #[error("cannot read DOCX part '{part}': {}", quote(.source))]
    #[non_exhaustive]
    Package {
        /// First affected package part, bounded for diagnostics.
        part: String,
        /// Original Core package error; always fatal.
        #[source]
        source: hwpforge_core::CoreError,
    },
    /// Tracked changes prevent any document from being returned.
    #[error("DOCX part '{part}' at {location}: tracked change '{element}'; accept all changes in Word or LibreOffice and retry")]
    #[non_exhaustive]
    TrackedChange {
        /// First affected part.
        part: String,
        /// Short location of the first tracked change.
        location: String,
        /// WordprocessingML change element.
        element: String,
    },
}

impl DocxError {
    pub(crate) fn invalid(
        part: &str,
        element: impl std::fmt::Display,
        detail: impl Into<String>,
    ) -> Self {
        Self::Invalid { part: quote(part), element: quote(element), detail: detail.into() }
    }
}

/// Bound each diagnostic input before copying, including streamed parser errors.
pub(crate) fn quote(value: impl std::fmt::Display) -> String {
    use std::fmt::Write;
    struct Quoted {
        text: String,
        remaining: usize,
    }
    impl std::fmt::Write for Quoted {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            let end = value.char_indices().nth(self.remaining).map(|(i, _)| i);
            match end {
                Some(end) => {
                    self.text.push_str(&value[..end]);
                    self.text.push('…');
                    Err(std::fmt::Error)
                }
                None => {
                    self.remaining -= value.chars().count();
                    self.text.push_str(value);
                    Ok(())
                }
            }
        }
    }
    let mut output = Quoted { text: String::new(), remaining: 512 };
    let _ = write!(output, "{value}");
    #[cfg(test)]
    QUOTED_BYTES.with(|bytes| bytes.set(bytes.get() + output.text.len()));
    output.text
}

#[cfg(test)]
thread_local! {
    static QUOTED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn take_quoted_bytes() -> usize {
    QUOTED_BYTES.with(|bytes| bytes.replace(0))
}
