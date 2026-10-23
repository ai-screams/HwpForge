//! Character shadow: kind, color and offsets as one value.
//!
//! A character shadow (글자 그림자) is described the same way by HWP5
//! (`CHAR_SHAPE` property bits 11-12, gap X/Y as INT8 at +50/+51, color as a
//! COLORREF at +64) and HWPX (`<hh:shadow type color offsetX offsetY>`).
//! [`CharShadow`] holds those four values together so that the default,
//! the valid ranges, color validation and serde live in one place, and both
//! the Blueprint style layer and the HWPX style store use the same type.
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::{CharShadow, Color, ShadowOffset, ShadowType};
//!
//! let shadow = CharShadow::new(ShadowType::Drop)
//!     .with_color(Some(Color::from_rgb(0x12, 0x34, 0x56)))
//!     .unwrap()
//!     .with_offset(ShadowOffset::new(-3).unwrap(), ShadowOffset::new(4).unwrap());
//! assert!(shadow.is_visible());
//! assert_eq!(shadow.offset_x().get(), -3);
//! ```

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::color::Color;
use crate::enums::ShadowType;
use crate::error::FoundationError;

/// A character shadow offset, in percent of the character size.
///
/// Invariant: `-100 <= value <= 100` (HWP5 INT8 "−100%–100%", OWPML XSD
/// `minInclusive`/`maxInclusive`).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ShadowOffset;
///
/// assert_eq!(ShadowOffset::new(-100).unwrap(), ShadowOffset::MIN);
/// assert!(ShadowOffset::new(101).is_err());
/// assert_eq!(ShadowOffset::saturating_from_i32(300), (ShadowOffset::MAX, true));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShadowOffset(i8);

impl ShadowOffset {
    /// The smallest offset, −100 %.
    pub const MIN: Self = Self(-100);
    /// The largest offset, 100 %.
    pub const MAX: Self = Self(100);

    /// Creates an offset.
    ///
    /// # Errors
    ///
    /// [`FoundationError::InvalidField`] when `v` is outside `-100..=100`.
    pub fn new(v: i8) -> Result<Self, FoundationError> {
        if !(Self::MIN.0..=Self::MAX.0).contains(&v) {
            return Err(FoundationError::InvalidField {
                field: "shadow offset".to_string(),
                reason: format!("{v} is outside -100..=100"),
            });
        }
        Ok(Self(v))
    }

    /// Clamps a wire value into range before narrowing it, and reports
    /// whether it had to clamp.
    pub const fn saturating_from_i32(v: i32) -> (Self, bool) {
        if v < Self::MIN.0 as i32 {
            (Self::MIN, true)
        } else if v > Self::MAX.0 as i32 {
            (Self::MAX, true)
        } else {
            (Self(v as i8), false)
        }
    }

    /// The offset in percent.
    pub const fn get(self) -> i8 {
        self.0
    }
}

impl fmt::Display for ShadowOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ShadowOffset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i8(self.0)
    }
}

impl<'de> Deserialize<'de> for ShadowOffset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = i64::deserialize(deserializer)?;
        i8::try_from(raw)
            .ok()
            .and_then(|v| Self::new(v).ok())
            .ok_or_else(|| de::Error::custom(format!("shadow offset {raw} is outside -100..=100")))
    }
}

impl schemars::JsonSchema for ShadowOffset {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ShadowOffset".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "integer", "minimum": -100, "maximum": 100 })
    }
}

/// A character shadow: kind, color and offsets.
///
/// The fields are private so that the color (24-bit RGB, upper byte 0) and
/// offset ranges always hold. `color` is `None` for "no color" (HWP5
/// `0xFFFFFFFF`, HWPX `color="none"`).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{CharShadow, Color, ShadowType};
///
/// let d = CharShadow::DEFAULT;
/// assert_eq!(d.kind(), ShadowType::None);
/// assert_eq!(d.color(), Some(Color::from_rgb(0xC0, 0xC0, 0xC0)));
/// assert_eq!((d.offset_x().get(), d.offset_y().get()), (10, 10));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CharShadow {
    kind: ShadowType,
    color: Option<Color>,
    offset_x: ShadowOffset,
    offset_y: ShadowOffset,
}

impl CharShadow {
    /// The new-document default, measured on a new Hancom (macOS 12.30)
    /// document: `NONE #C0C0C0 10 10`. Turning the shadow on without other
    /// changes keeps the color and offsets.
    pub const DEFAULT: Self =
        Self::new_unchecked(ShadowType::None, Some(Color::from_rgb(0xC0, 0xC0, 0xC0)), 10, 10);

    const fn new_unchecked(kind: ShadowType, color: Option<Color>, x: i8, y: i8) -> Self {
        Self { kind, color, offset_x: ShadowOffset(x), offset_y: ShadowOffset(y) }
    }

    /// `kind` with the default color and offsets.
    pub const fn new(kind: ShadowType) -> Self {
        Self::DEFAULT.with_kind(kind)
    }

    /// Replaces the kind.
    pub const fn with_kind(mut self, kind: ShadowType) -> Self {
        self.kind = kind;
        self
    }

    /// Replaces the color (`None` = no color).
    ///
    /// # Errors
    ///
    /// [`FoundationError::InvalidColor`] when the upper byte of the color is
    /// not zero: neither wire can hold it.
    pub fn with_color(mut self, color: Option<Color>) -> Result<Self, FoundationError> {
        if let Some(c) = color {
            if c.to_raw() >> 24 != 0 {
                return Err(FoundationError::InvalidColor {
                    component: "shadow color upper byte".to_string(),
                    value: format!("0x{:08X}", c.to_raw()),
                });
            }
        }
        self.color = color;
        Ok(self)
    }

    /// Replaces both offsets.
    pub const fn with_offset(mut self, x: ShadowOffset, y: ShadowOffset) -> Self {
        self.offset_x = x;
        self.offset_y = y;
        self
    }

    /// The shadow kind.
    pub const fn kind(&self) -> ShadowType {
        self.kind
    }

    /// The shadow color, `None` for no color.
    pub const fn color(&self) -> Option<Color> {
        self.color
    }

    /// The horizontal offset.
    pub const fn offset_x(&self) -> ShadowOffset {
        self.offset_x
    }

    /// The vertical offset.
    pub const fn offset_y(&self) -> ShadowOffset {
        self.offset_y
    }

    /// `true` unless the kind is [`ShadowType::None`].
    pub const fn is_visible(&self) -> bool {
        !matches!(self.kind, ShadowType::None)
    }
}

impl Default for CharShadow {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `"#RRGGBB"` or `"none"`.
fn color_to_str(color: Option<Color>) -> String {
    color.map_or_else(|| "none".to_string(), |c| c.to_hex_rgb())
}

/// Parses `"#RRGGBB"` or `"none"` (any case).
fn color_from_str(s: &str) -> Result<Option<Color>, FoundationError> {
    if s.eq_ignore_ascii_case("none") {
        Ok(None)
    } else {
        Color::from_hex_rgb(s).map(Some)
    }
}

impl Serialize for CharShadow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("color", &color_to_str(self.color))?;
        map.serialize_entry("offset_x", &self.offset_x)?;
        map.serialize_entry("offset_y", &self.offset_y)?;
        map.end()
    }
}

/// Reads a shadow kind written as a string (`"Drop"`, `"drop"`, …).
fn kind_from_str<E: de::Error>(s: &str) -> Result<ShadowType, E> {
    s.parse::<ShadowType>().map_err(E::custom)
}

impl<'de> Deserialize<'de> for CharShadow {
    /// Accepts the map written by [`Serialize`] (missing fields take the
    /// default's) or a bare kind string, the shape older releases wrote.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CharShadow;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a shadow kind string or a map {kind, color, offset_x, offset_y}")
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<CharShadow, E> {
                Ok(CharShadow::new(kind_from_str(s)?))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CharShadow, A::Error> {
                let mut out = CharShadow::DEFAULT;
                let (mut x, mut y) = (out.offset_x, out.offset_y);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "kind" => out.kind = kind_from_str(&map.next_value::<String>()?)?,
                        "color" => {
                            let s = map.next_value::<String>()?;
                            out.color = color_from_str(&s).map_err(de::Error::custom)?;
                        }
                        "offset_x" => x = map.next_value()?,
                        "offset_y" => y = map.next_value()?,
                        other => {
                            return Err(de::Error::unknown_field(
                                other,
                                &["kind", "color", "offset_x", "offset_y"],
                            ))
                        }
                    }
                }
                Ok(out.with_offset(x, y))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// The JSON schema of a shadow kind string.
fn shadow_kind_schema() -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "enum": ["None", "Drop", "Continuous", "none", "drop", "continuous"]
    })
}

/// The JSON schema of a shadow color string.
fn shadow_color_schema() -> schemars::Schema {
    schemars::json_schema!({
        "anyOf": [
            { "type": "string", "pattern": "^#[0-9A-Fa-f]{6}$" },
            { "const": "none" }
        ]
    })
}

impl schemars::JsonSchema for CharShadow {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CharShadow".into()
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [
                shadow_kind_schema(),
                {
                    "type": "object",
                    "properties": {
                        "kind": shadow_kind_schema(),
                        "color": shadow_color_schema(),
                        "offset_x": { "type": "integer", "minimum": -100, "maximum": 100 },
                        "offset_y": { "type": "integer", "minimum": -100, "maximum": 100 }
                    },
                    "additionalProperties": false
                }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off(v: i8) -> ShadowOffset {
        ShadowOffset::new(v).unwrap()
    }

    /// 이것을 실패시키는 것: 범위를 좁히는 것(−100·100 거부),
    /// 범위 검사를 지우는 것(−101·101·−128·127 통과).
    #[test]
    fn offset_new_accepts_exactly_minus_100_to_100() {
        assert_eq!(ShadowOffset::new(-100).unwrap().get(), -100);
        assert_eq!(ShadowOffset::new(100).unwrap().get(), 100);
        for bad in [-101, 101, -128, 127] {
            let err = ShadowOffset::new(bad).unwrap_err();
            assert!(matches!(err, FoundationError::InvalidField { .. }), "{bad}: {err:?}");
        }
    }

    /// 이것을 실패시키는 것: 자르기 전에 `as i8` 로 좁히는 것(300 → 44, −150 → 106),
    /// 잘랐는지 표시를 늘 true 또는 false 로 두는 것.
    #[test]
    fn saturating_from_i32_clamps_before_narrowing() {
        assert_eq!(ShadowOffset::saturating_from_i32(300), (ShadowOffset::MAX, true));
        assert_eq!(ShadowOffset::saturating_from_i32(-150), (ShadowOffset::MIN, true));
        assert_eq!(ShadowOffset::saturating_from_i32(-129), (ShadowOffset::MIN, true));
        assert_eq!(ShadowOffset::saturating_from_i32(i32::MAX), (ShadowOffset::MAX, true));
        assert_eq!(ShadowOffset::saturating_from_i32(5), (off(5), false));
        assert_eq!(ShadowOffset::saturating_from_i32(-100), (ShadowOffset::MIN, false));
    }

    /// 이것을 실패시키는 것: 역직렬화가 `new` 를 거치지 않고 값을 바로 담는 것.
    #[test]
    fn offset_deserialize_checks_range() {
        assert!(serde_json::from_str::<ShadowOffset>("101").is_err());
        assert!(serde_json::from_str::<ShadowOffset>("300").is_err());
        assert_eq!(serde_json::from_str::<ShadowOffset>("-3").unwrap(), off(-3));
    }

    /// 이것을 실패시키는 것: `with_color` 의 상위 바이트 검사를 지우는 것.
    #[test]
    fn with_color_rejects_a_nonzero_upper_byte() {
        let err = CharShadow::DEFAULT.with_color(Some(Color::from_raw(0x0103_0201))).unwrap_err();
        assert!(matches!(err, FoundationError::InvalidColor { .. }), "{err:?}");
        assert_eq!(CharShadow::DEFAULT.with_color(None).unwrap().color(), None);
    }

    /// 이것을 실패시키는 것: 직렬화가 RGB/BGR 을 뒤바꾸는 것(`#563412`).
    #[test]
    fn serialize_writes_rgb_hex() {
        let s = CharShadow::new(ShadowType::Continuous)
            .with_color(Some(Color::from_rgb(0x12, 0x34, 0x56)))
            .unwrap()
            .with_offset(off(-3), off(4));
        let json = serde_json::to_value(s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "Continuous", "color": "#123456", "offset_x": -3, "offset_y": 4})
        );
        assert_eq!(serde_json::from_value::<CharShadow>(json).unwrap(), s);
        let none = CharShadow::DEFAULT.with_color(None).unwrap();
        assert_eq!(serde_json::to_value(none).unwrap()["color"], "none");
    }

    /// 이것을 실패시키는 것: `visit_str` 를 지우는 것(옛 `"Drop"` 이 읽히지 않음),
    /// 빠진 칸을 DEFAULT 가 아닌 값으로 채우는 것, 모르는 키를 허용하는 것,
    /// `color: none` 을 색으로 읽는 것, `color: null` 을 허용하는 것.
    #[test]
    fn deserialize_shorthand_partial_map_and_errors() {
        let drop: CharShadow = serde_json::from_str("\"Drop\"").unwrap();
        assert_eq!(drop, CharShadow::new(ShadowType::Drop));
        assert_eq!(drop.color(), CharShadow::DEFAULT.color());
        let lower: CharShadow = serde_json::from_str("\"continuous\"").unwrap();
        assert_eq!(lower.kind(), ShadowType::Continuous);

        let part: CharShadow = serde_json::from_str(r#"{"kind": "Drop", "offset_x": 5}"#).unwrap();
        assert_eq!(part, CharShadow::new(ShadowType::Drop).with_offset(off(5), off(10)));

        let none: CharShadow = serde_json::from_str(r#"{"color": "none"}"#).unwrap();
        assert_eq!(none.color(), None);
        assert_eq!(none.kind(), ShadowType::None);

        assert!(serde_json::from_str::<CharShadow>(r#"{"alpha": 1}"#).is_err());
        assert!(serde_json::from_str::<CharShadow>(r#"{"color": null}"#).is_err());
        assert!(serde_json::from_str::<CharShadow>(r##"{"color": "#12345"}"##).is_err());
        assert!(serde_json::from_str::<CharShadow>(r#"{"offset_y": -101}"#).is_err());
        assert!(serde_json::from_str::<CharShadow>("\"Sparkle\"").is_err());
    }

    /// 이것을 실패시키는 것: `Default` 를 상수와 따로 정의하는 것, 기본값을 옛
    /// `#B2B2B2` 로 두는 것.
    #[test]
    fn default_is_the_hancom_new_document_value() {
        assert_eq!(CharShadow::default(), CharShadow::DEFAULT);
        let d = CharShadow::DEFAULT;
        assert_eq!(d.kind(), ShadowType::None);
        assert_eq!(d.color().map(|c| c.to_hex_rgb()), Some("#C0C0C0".to_string()));
        assert_eq!((d.offset_x(), d.offset_y()), (off(10), off(10)));
        assert!(!d.is_visible());
        assert!(CharShadow::new(ShadowType::Continuous).is_visible());
    }

    /// 이것을 실패시키는 것: 스키마가 색 none·간격 범위·모르는 키를 표현하지 않는 것.
    #[test]
    fn schema_describes_both_shapes() {
        let schema = serde_json::to_value(schemars::schema_for!(CharShadow)).unwrap();
        let text = schema.to_string();
        assert!(text.contains("\"additionalProperties\":false"), "{text}");
        assert!(text.contains("\"minimum\":-100"), "{text}");
        assert!(text.contains("\"const\":\"none\""), "{text}");
        assert!(text.contains("\"Continuous\""), "{text}");
    }
}
