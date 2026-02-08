//! Custom serde helpers for human-friendly YAML values.
//!
//! Provides parsing and serialization for:
//! - Dimensions: `"16pt"`, `"20mm"`, `"1in"` → [`HwpUnit`]
//! - Percentages: `"160%"` → `f64`
//! - Colors: `"#RRGGBB"` → [`Color`]

use hwpforge_foundation::{Color, HwpUnit};
use serde::Deserialize;

use crate::error::BlueprintError;

// ---------------------------------------------------------------------------
// Serde bridge functions (used via #[serde(serialize_with/deserialize_with)])
//
// These are pub(crate) so that style.rs and template.rs can reference them
// in serde attributes as `crate::serde_helpers::ser_dim`, etc.
// ---------------------------------------------------------------------------

/// Serializes an `HwpUnit` as a dimension string (e.g. `"16pt"`).
pub(crate) fn ser_dim<S: serde::Serializer>(unit: &HwpUnit, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format_dimension_pt(*unit))
}

/// Deserializes a dimension string into `HwpUnit`.
pub(crate) fn de_dim<'de, D: serde::Deserializer<'de>>(d: D) -> Result<HwpUnit, D::Error> {
    let v = String::deserialize(d)?;
    parse_dimension(&v).map_err(serde::de::Error::custom)
}

/// Serializes an `Option<HwpUnit>` as a dimension string or null.
pub(crate) fn ser_dim_opt<S: serde::Serializer>(
    u: &Option<HwpUnit>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match u {
        Some(v) => s.serialize_str(&format_dimension_pt(*v)),
        None => s.serialize_none(),
    }
}

/// Deserializes an optional dimension string into `Option<HwpUnit>`.
pub(crate) fn de_dim_opt<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<HwpUnit>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        Some(v) => parse_dimension(&v).map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Serializes an `Option<f64>` as a percentage string or null.
pub(crate) fn ser_pct_opt<S: serde::Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(val) => s.serialize_str(&format_percentage(*val)),
        None => s.serialize_none(),
    }
}

/// Deserializes an optional percentage string into `Option<f64>`.
pub(crate) fn de_pct_opt<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        Some(v) => parse_percentage(&v).map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Serializes a `Color` as a `#RRGGBB` string.
pub(crate) fn ser_color<S: serde::Serializer>(c: &Color, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format_color(*c))
}

/// Deserializes a `#RRGGBB` string into `Color`.
pub(crate) fn de_color<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
    let v = String::deserialize(d)?;
    parse_color(&v).map_err(serde::de::Error::custom)
}

/// Serializes an `Option<Color>` as a `#RRGGBB` string or null.
pub(crate) fn ser_color_opt<S: serde::Serializer>(
    c: &Option<Color>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match c {
        Some(v) => s.serialize_str(&format_color(*v)),
        None => s.serialize_none(),
    }
}

/// Deserializes an optional `#RRGGBB` string into `Option<Color>`.
pub(crate) fn de_color_opt<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Color>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        Some(v) => parse_color(&v).map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Dimension parsing: "16pt", "20mm", "1in" → HwpUnit
// ---------------------------------------------------------------------------

/// Parses a dimension string into [`HwpUnit`].
///
/// Supported suffixes (case-insensitive):
/// - `pt` — points (1pt = 100 HwpUnit)
/// - `mm` — millimeters (1mm ≈ 2.835pt)
/// - `in` — inches (1in = 72pt)
///
/// Also accepts a plain number as raw HwpUnit value.
pub fn parse_dimension(s: &str) -> Result<HwpUnit, BlueprintError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(BlueprintError::InvalidDimension { value: s.to_string() });
    }

    let lower = s.to_ascii_lowercase();

    if let Some(num_str) = lower.strip_suffix("pt") {
        let pt: f64 = num_str
            .trim()
            .parse()
            .map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })?;
        HwpUnit::from_pt(pt).map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })
    } else if let Some(num_str) = lower.strip_suffix("mm") {
        let mm: f64 = num_str
            .trim()
            .parse()
            .map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })?;
        HwpUnit::from_mm(mm).map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })
    } else if let Some(num_str) = lower.strip_suffix("in") {
        let inches: f64 = num_str
            .trim()
            .parse()
            .map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })?;
        HwpUnit::from_inch(inches)
            .map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })
    } else {
        // Try raw integer (HwpUnit value)
        let raw: i32 =
            s.parse().map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })?;
        HwpUnit::new(raw).map_err(|_| BlueprintError::InvalidDimension { value: s.to_string() })
    }
}

/// Formats an [`HwpUnit`] back to a pt string for YAML serialization.
///
/// Uses 2 decimal places for non-integer values to preserve full precision
/// (1 HwpUnit = 0.01pt).
pub fn format_dimension_pt(unit: HwpUnit) -> String {
    let pt = unit.to_pt();
    if (pt - pt.round()).abs() < f64::EPSILON {
        format!("{}pt", pt as i64)
    } else {
        format!("{pt:.2}pt")
    }
}

// ---------------------------------------------------------------------------
// Percentage parsing: "160%" → f64
// ---------------------------------------------------------------------------

/// Parses a percentage string into `f64`.
///
/// Rejects negative values. Examples: `"160%"` → `160.0`, `"100%"` → `100.0`
pub fn parse_percentage(s: &str) -> Result<f64, BlueprintError> {
    let s = s.trim();
    let num_str = s
        .strip_suffix('%')
        .ok_or_else(|| BlueprintError::InvalidPercentage { value: s.to_string() })?;
    let value: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| BlueprintError::InvalidPercentage { value: s.to_string() })?;
    if value < 0.0 {
        return Err(BlueprintError::InvalidPercentage { value: s.to_string() });
    }
    Ok(value)
}

/// Formats a percentage value back to string.
pub fn format_percentage(value: f64) -> String {
    if (value - value.round()).abs() < f64::EPSILON {
        format!("{}%", value as i64)
    } else {
        format!("{value:.1}%")
    }
}

// ---------------------------------------------------------------------------
// Color parsing: "#RRGGBB" → Color
// ---------------------------------------------------------------------------

/// Parses a color string in `#RRGGBB` format into [`Color`].
pub fn parse_color(s: &str) -> Result<Color, BlueprintError> {
    let s = s.trim();
    let hex =
        s.strip_prefix('#').ok_or_else(|| BlueprintError::InvalidColor { value: s.to_string() })?;

    if hex.len() != 6 {
        return Err(BlueprintError::InvalidColor { value: s.to_string() });
    }

    let r = u8::from_str_radix(&hex[0..2], 16)
        .map_err(|_| BlueprintError::InvalidColor { value: s.to_string() })?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|_| BlueprintError::InvalidColor { value: s.to_string() })?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .map_err(|_| BlueprintError::InvalidColor { value: s.to_string() })?;

    Ok(Color::from_rgb(r, g, b))
}

/// Formats a [`Color`] as `#RRGGBB`.
pub fn format_color(color: Color) -> String {
    let (r, g, b) = color.to_rgb();
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -----------------------------------------------------------------------
    // Dimension parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_dimension_pt() {
        let unit = parse_dimension("16pt").unwrap();
        assert_eq!(unit, HwpUnit::from_pt(16.0).unwrap());
    }

    #[test]
    fn parse_dimension_pt_fractional() {
        let unit = parse_dimension("10.5pt").unwrap();
        assert_eq!(unit, HwpUnit::from_pt(10.5).unwrap());
    }

    #[test]
    fn parse_dimension_mm() {
        let unit = parse_dimension("20mm").unwrap();
        assert_eq!(unit, HwpUnit::from_mm(20.0).unwrap());
    }

    #[test]
    fn parse_dimension_inch() {
        let unit = parse_dimension("1in").unwrap();
        assert_eq!(unit, HwpUnit::from_inch(1.0).unwrap());
    }

    #[test]
    fn parse_dimension_case_insensitive() {
        assert_eq!(parse_dimension("16PT").unwrap(), parse_dimension("16pt").unwrap());
        assert_eq!(parse_dimension("20MM").unwrap(), parse_dimension("20mm").unwrap());
        assert_eq!(parse_dimension("1IN").unwrap(), parse_dimension("1in").unwrap());
    }

    #[test]
    fn parse_dimension_raw_integer() {
        let unit = parse_dimension("1600").unwrap();
        assert_eq!(unit, HwpUnit::new(1600).unwrap());
    }

    #[test]
    fn parse_dimension_zero() {
        let unit = parse_dimension("0pt").unwrap();
        assert_eq!(unit, HwpUnit::ZERO);
    }

    #[test]
    fn parse_dimension_whitespace_trimmed() {
        let unit = parse_dimension("  16pt  ").unwrap();
        assert_eq!(unit, HwpUnit::from_pt(16.0).unwrap());
    }

    #[test]
    fn parse_dimension_empty_error() {
        assert!(parse_dimension("").is_err());
        assert!(parse_dimension("   ").is_err());
    }

    #[test]
    fn parse_dimension_no_unit_no_number() {
        assert!(parse_dimension("pt").is_err());
        assert!(parse_dimension("mm").is_err());
        assert!(parse_dimension("abc").is_err());
    }

    #[test]
    fn parse_dimension_invalid_unit() {
        assert!(parse_dimension("16px").is_err());
        assert!(parse_dimension("16em").is_err());
    }

    #[test]
    fn parse_dimension_negative() {
        let unit = parse_dimension("-5pt").unwrap();
        assert_eq!(unit, HwpUnit::from_pt(-5.0).unwrap());
    }

    // -----------------------------------------------------------------------
    // Dimension formatting (roundtrip)
    // -----------------------------------------------------------------------

    #[test]
    fn format_dimension_whole_number() {
        assert_eq!(format_dimension_pt(HwpUnit::from_pt(16.0).unwrap()), "16pt");
    }

    #[test]
    fn format_dimension_zero() {
        assert_eq!(format_dimension_pt(HwpUnit::ZERO), "0pt");
    }

    // -----------------------------------------------------------------------
    // Percentage parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_percentage_normal() {
        assert_eq!(parse_percentage("160%").unwrap(), 160.0);
    }

    #[test]
    fn parse_percentage_hundred() {
        assert_eq!(parse_percentage("100%").unwrap(), 100.0);
    }

    #[test]
    fn parse_percentage_fractional() {
        assert_eq!(parse_percentage("150.5%").unwrap(), 150.5);
    }

    #[test]
    fn parse_percentage_zero() {
        assert_eq!(parse_percentage("0%").unwrap(), 0.0);
    }

    #[test]
    fn parse_percentage_no_percent_sign() {
        assert!(parse_percentage("160").is_err());
    }

    #[test]
    fn parse_percentage_empty() {
        assert!(parse_percentage("").is_err());
        assert!(parse_percentage("%").is_err());
    }

    #[test]
    fn parse_percentage_invalid() {
        assert!(parse_percentage("abc%").is_err());
    }

    #[test]
    fn parse_percentage_negative_rejected() {
        assert!(parse_percentage("-10%").is_err());
        assert!(parse_percentage("-0.1%").is_err());
    }

    #[test]
    fn format_percentage_whole() {
        assert_eq!(format_percentage(160.0), "160%");
    }

    #[test]
    fn format_percentage_fractional() {
        assert_eq!(format_percentage(150.5), "150.5%");
    }

    // -----------------------------------------------------------------------
    // Color parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_color_black() {
        let c = parse_color("#000000").unwrap();
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn parse_color_white() {
        let c = parse_color("#FFFFFF").unwrap();
        assert_eq!(c, Color::WHITE);
    }

    #[test]
    fn parse_color_red() {
        let c = parse_color("#FF0000").unwrap();
        assert_eq!(c, Color::RED);
    }

    #[test]
    fn parse_color_lowercase() {
        let c = parse_color("#ff0000").unwrap();
        assert_eq!(c, Color::RED);
    }

    #[test]
    fn parse_color_mixed_case() {
        let c = parse_color("#Ff0000").unwrap();
        assert_eq!(c, Color::RED);
    }

    #[test]
    fn parse_color_custom() {
        let c = parse_color("#003366").unwrap();
        let (r, g, b) = c.to_rgb();
        assert_eq!((r, g, b), (0x00, 0x33, 0x66));
    }

    #[test]
    fn parse_color_no_hash() {
        assert!(parse_color("FF0000").is_err());
    }

    #[test]
    fn parse_color_short_form() {
        assert!(parse_color("#FFF").is_err());
    }

    #[test]
    fn parse_color_too_long() {
        assert!(parse_color("#FF00FF00").is_err());
    }

    #[test]
    fn parse_color_invalid_hex() {
        assert!(parse_color("#GGHHII").is_err());
    }

    #[test]
    fn parse_color_empty() {
        assert!(parse_color("").is_err());
        assert!(parse_color("#").is_err());
    }

    #[test]
    fn format_color_roundtrip() {
        let original = "#003366";
        let color = parse_color(original).unwrap();
        assert_eq!(format_color(color), original);
    }

    #[test]
    fn format_color_red() {
        assert_eq!(format_color(Color::RED), "#FF0000");
    }
}
