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
            v.parse::<i32>().or_else(|_| {
                v.parse::<u32>().map(|n| i32::from_ne_bytes(n.to_ne_bytes())).map_err(E::custom)
            })
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i32, E> {
            self.visit_str(&v.to_string())
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i32, E> {
            self.visit_str(&v.to_string())
        }
    }
    d.deserialize_any(V)
}

/// Serializes an `i32` read by [`deser_i32_or_u32`] back as the unsigned
/// value Hancom wrote, for attributes Hancom only ever writes unsigned:
/// margins (`xs:nonNegativeInteger`) and object offsets and sizes, which
/// Hancom writes as the unsigned wrap even where the schema allows a
/// negative integer.
///
/// `-1` is written as `4294967295`, never `-1`. 이것을 실패시키는 것: i32
/// 그대로 쓰기 (`dormant_unset_cell_margin_round_trips_as_unsigned`,
/// `tests/wrapped_offsets.rs`).
pub(crate) fn ser_i32_as_u32<S: serde::Serializer>(v: &i32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u32(*v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::{Error, I64Deserializer, U64Deserializer};

    #[test]
    fn fix2_raw_integer_deserializer_checks_width_before_bit_reinterpretation() {
        // 이것을 실패시키는 것: i64/u64 visitor의 무검사 as i32 캐스트 복귀.
        for raw in [u64::from(u32::MAX) + 1, u64::MAX] {
            assert!(deser_i32_or_u32(U64Deserializer::<Error>::new(raw)).is_err());
        }
        for raw in [i64::from(i32::MIN) - 1, i64::from(u32::MAX) + 1] {
            assert!(deser_i32_or_u32(I64Deserializer::<Error>::new(raw)).is_err());
        }
        assert_eq!(
            deser_i32_or_u32(U64Deserializer::<Error>::new(u64::from(u32::MAX))).unwrap(),
            -1
        );
        assert_eq!(deser_i32_or_u32(I64Deserializer::<Error>::new(-1)).unwrap(), -1);
    }
}
