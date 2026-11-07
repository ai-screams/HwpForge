//! Admission before an input identifier first participates in a key lookup.
use crate::{
    error::{quote, Result},
    DocxError,
};
pub(crate) fn check(value: &str, part: &str, at: &str) -> Result<()> {
    const MAX_IDENTIFIER_BYTES: usize = 4096;
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(DocxError::Limit {
            part: quote(part),
            element: quote(at),
            resource: "identifier bytes",
            observed: value.len(),
            limit: MAX_IDENTIFIER_BYTES,
        });
    }
    Ok(())
}
