//! Context-bearing errors at the DOCX input boundary.

pub(crate) type Result<T> = std::result::Result<T, DocxError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DocxError {
    #[error("DOCX part '{part}': part entries observed 0, required 1")]
    MissingPart { part: String },
    #[error("DOCX part '{part}': ambiguous ASCII case-insensitive entry lookup")]
    AmbiguousPart { part: String },
    #[error("DOCX part '{part}': opaque part selected (observed 1, limit 0)")]
    OpaquePart { part: String },
    #[error("DOCX part '{part}': content type definitions observed 0, required 1")]
    MissingContentType { part: String },
    #[error("DOCX part '{part}': content type '{actual}' does not match '{expected}'")]
    ContentTypeMismatch { part: String, actual: String, expected: &'static str },
    #[error("DOCX part '{part}': {encoding} XML part unsupported; expected UTF-8")]
    UnsupportedEncoding { part: String, encoding: String },
    #[error("invalid DOCX part '{part}' at <{element}>: {detail}")]
    Invalid { part: String, element: String, detail: String },
    #[error("DOCX part '{part}' at <{element}>: {resource} observed {observed}, limit {limit}")]
    Limit { part: String, element: String, resource: &'static str, observed: usize, limit: usize },
    #[error("cannot read DOCX part '{part}': {}", quote(.source))]
    Package {
        part: String,
        #[source]
        source: hwpforge_core::CoreError,
    },
}

impl DocxError {
    pub(crate) fn invalid(part: &str, element: &str, detail: impl Into<String>) -> Self {
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
    output.text
}
