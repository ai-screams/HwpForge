//! Checked rational lengths; no intermediate floating-point rounding.
use crate::{error::Result, DocxError};

pub(super) fn length(
    value: &str,
    factor: i128,
    minimum: i32,
) -> std::result::Result<i32, &'static str> {
    let (number, numerator, denominator) = [
        ("EMU", 1, 127),
        ("pt", 100, 1),
        ("in", 7200, 1),
        ("pc", 1200, 1),
        ("mm", 36000, 127),
        ("cm", 360000, 127),
    ]
    .into_iter()
    .find_map(|(suffix, n, d)| value.strip_suffix(suffix).map(|s| (s, n, d)))
    .unwrap_or((value, factor, 1));
    let negative = number.starts_with('-');
    let number = number.strip_prefix(['-', '+']).unwrap_or(number);
    let mut digits = 0_i128;
    let mut scale = 1_i128;
    let mut dot = false;
    let mut count = 0;
    for c in number.bytes() {
        if c == b'.' && !dot {
            dot = true;
            continue;
        }
        if !c.is_ascii_digit() {
            return Err("invalid numeric syntax");
        }
        count += 1;
        digits = digits
            .checked_mul(10)
            .and_then(|v| v.checked_add(i128::from(c - b'0')))
            .ok_or("numeric overflow")?;
        if dot {
            scale = scale.checked_mul(10).ok_or("numeric overflow")?;
        }
    }
    if count == 0 {
        return Err("invalid numeric syntax");
    }
    let n = digits.checked_mul(numerator).ok_or("numeric overflow")?;
    let d = scale.checked_mul(denominator).ok_or("numeric overflow")?;
    // Reject out-of-range source values before rounding, not just their rounded result.
    if n > 100_000_000_i128.checked_mul(d).ok_or("numeric overflow")? {
        return Err("length out of range");
    }
    let signed = if negative { -n } else { n };
    if signed < i128::from(minimum).checked_mul(d).ok_or("numeric overflow")? {
        return Err("length below minimum");
    }
    let rounded = n / d + i128::from(n % d >= (d / 2 + d % 2));
    i32::try_from(if negative { -rounded } else { rounded }).map_err(|_| "numeric overflow")
}
pub(super) fn canonical_float(value: f64) -> Result<f64> {
    if !value.is_finite() || value.abs() > f64::from(i32::MAX) {
        return Err(DocxError::invalid(
            "#format",
            "line",
            "non-finite or out-of-range line spacing",
        ));
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}

/// Validate encountered page dimensions without importing page layout.
/// Page sizes below 1 HwpUnit are rejected before rounding.
pub(crate) fn structural(e: &crate::xml::Element, part: &str) -> Result<()> {
    let keys: &[&str] = match e.name.as_str() {
        "pgSz" => &["w", "h"],
        "pgMar" => &["top", "right", "bottom", "left", "header", "footer", "gutter"],
        _ => return Ok(()),
    };
    for key in keys {
        if let Some(value) = super::attr(e, key) {
            let dimension = if e.name == "pgSz" {
                super::StructuralDimension::PageSize
            } else {
                super::StructuralDimension::Margin
            };
            super::structural_length(value, dimension)
                .map_err(|reason| DocxError::invalid(part, &e.name, reason))?;
        }
    }
    Ok(())
}
