//! XML schema types (DTOs) for HWPX format.
//!
//! These types map directly to HWPX XML elements and exist only as an
//! internal deserialization boundary. They are **not** re-exported from
//! the crate's public API.
//!
//! The `Hx` prefix distinguishes these types from their Core
//! counterparts (e.g. `HxParagraph` vs `Paragraph`).

pub(crate) mod header;
pub(crate) mod section;
pub(crate) mod shapes;

/// Deserializes an `i32` that may be stored as unsigned in HWPX XML.
///
/// Hancom uses `4294967295` (`u32::MAX`) as a sentinel for "inherit/auto",
/// which is `-1` in two's complement. Standard `i32` deserialization rejects
/// values above `i32::MAX`, so we fall back to `u32` parsing with wrapping.
pub(crate) fn deser_i32_or_u32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = i32;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("i32 or u32-wrapping integer")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i32, E> {
            v.parse::<i32>().or_else(|_| v.parse::<u32>().map(|n| n as i32).map_err(E::custom))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i32, E> {
            Ok(v as i32)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i32, E> {
            Ok(v as i32)
        }
    }
    d.deserialize_any(V)
}
