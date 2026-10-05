//! Children of a [`Control::Group`](super::Control::Group).

use std::fmt;

use schemars::JsonSchema;
use serde::de::{self, DeserializeSeed, IntoDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::Control;
use crate::image::Image;

/// One child of a [`Control::Group`](super::Control::Group) (묶음 개체), in
/// z-order.
///
/// A picture is an [`Image`], as in a run; every other child is a drawing
/// shape [`Control`] (a nested group included).
///
/// In JSON a picture is `{"Picture": {…}}` and a shape is the shape's own
/// [`Control`] form (`{"Rect": {…}}`), so group JSON written before
/// pictures could be group children reads unchanged.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::{Control, GroupChild};
/// use hwpforge_core::image::{Image, ImageFormat};
/// use hwpforge_foundation::HwpUnit;
///
/// let w = HwpUnit::new(1000).unwrap();
/// let children = vec![
///     GroupChild::Picture(Image::new("BinData/image1.png", w, w, ImageFormat::Png)),
///     GroupChild::Shape(Control::rect(w, w).unwrap()),
/// ];
/// let json = serde_json::to_string(&children).unwrap();
/// assert!(json.starts_with(r#"[{"Picture":{"#));
/// assert!(json.contains(r#"{"Rect":{"#));
/// let back: Vec<GroupChild> = serde_json::from_str(&json).unwrap();
/// assert_eq!(back, children);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[non_exhaustive]
pub enum GroupChild {
    /// A picture placed in the group. Its `placement` offset is relative to
    /// the group's top-left corner, like a shape child's.
    Picture(Image),
    /// A drawing shape, or a nested [`Control::Group`](super::Control::Group).
    #[serde(untagged)]
    Shape(Control),
}

/// Reads a group child: a map whose single key is `"Picture"` is a picture;
/// any other key is handed to [`Control`]'s own reader, so its errors keep
/// the original variant and field names.
impl<'de> Deserialize<'de> for GroupChild {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(GroupChildVisitor)
    }
}

struct GroupChildVisitor;

impl<'de> Visitor<'de> for GroupChildVisitor {
    type Value = GroupChild;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a group child: {\"Picture\": {...}} or a shape control")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<GroupChild, M::Error> {
        let Some(key) = map.next_key::<String>()? else {
            return Err(de::Error::invalid_length(0, &self));
        };
        let child = if key == "Picture" {
            GroupChild::Picture(map.next_value()?)
        } else {
            GroupChild::Shape(Control::deserialize(KeyedEnum { key, map: &mut map })?)
        };
        if map.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(2, &self));
        }
        Ok(child)
    }
}

/// An externally tagged enum whose tag has already been read from `map`.
struct KeyedEnum<'a, M> {
    key: String,
    map: &'a mut M,
}

impl<'de, M: MapAccess<'de>> Deserializer<'de> for KeyedEnum<'_, M> {
    type Error = M::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, M::Error> {
        visitor.visit_enum(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de, M: MapAccess<'de>> de::EnumAccess<'de> for KeyedEnum<'_, M> {
    type Error = M::Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), M::Error> {
        let key: de::value::StringDeserializer<M::Error> = self.key.clone().into_deserializer();
        Ok((seed.deserialize(key)?, self))
    }
}

impl<'de, M: MapAccess<'de>> de::VariantAccess<'de> for KeyedEnum<'_, M> {
    type Error = M::Error;

    fn unit_variant(self) -> Result<(), M::Error> {
        self.map.next_value::<()>()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, M::Error> {
        self.map.next_value_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, M::Error> {
        self.map.next_value_seed(Shaped::Tuple(len, visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, M::Error> {
        self.map.next_value_seed(Shaped::Struct(fields, visitor))
    }
}

/// The value of a tuple or struct variant, read with the variant's visitor.
enum Shaped<V> {
    Tuple(usize, V),
    Struct(&'static [&'static str], V),
}

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for Shaped<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        match self {
            Self::Tuple(len, visitor) => deserializer.deserialize_tuple(len, visitor),
            Self::Struct(fields, visitor) => deserializer.deserialize_struct("", fields, visitor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageFormat;
    use hwpforge_foundation::HwpUnit;

    fn rect_json() -> String {
        let w = HwpUnit::new(1000).unwrap();
        serde_json::to_string(&Control::rect(w, w).unwrap()).unwrap()
    }

    /// Group JSON written before pictures could be children is a list of
    /// plain controls; it reads as shape children.
    /// 이것을 실패시키는 것: `Picture` 가 아닌 키를 `Control` 로 넘기지 않는 변이.
    #[test]
    fn a_plain_control_reads_as_a_shape_child() {
        let child: GroupChild = serde_json::from_str(&rect_json()).unwrap();
        assert!(matches!(child, GroupChild::Shape(Control::Rect { .. })), "{child:?}");
    }

    /// 이것을 실패시키는 것: `"Picture"` 키 판정을 지워 그림을 `Control` 로 넘기는 변이.
    #[test]
    fn a_picture_round_trips_under_its_own_key() {
        let w = HwpUnit::new(1000).unwrap();
        let child = GroupChild::Picture(Image::new("BinData/image1.png", w, w, ImageFormat::Png));
        let json = serde_json::to_string(&child).unwrap();
        assert!(json.starts_with(r#"{"Picture":{"path":"BinData/image1.png""#), "{json}");
        assert_eq!(serde_json::from_str::<GroupChild>(&json).unwrap(), child);
    }

    /// A broken shape keeps the error the shape's own reader gives (the
    /// missing field by name), not a generic "no variant matched".
    /// 이것을 실패시키는 것: 수동 Deserialize 를 `#[serde(untagged)]` derive 로 바꾸는 변이.
    #[test]
    fn a_broken_shape_reports_the_missing_field() {
        let err = serde_json::from_str::<GroupChild>(r#"{"Rect":{"width":10}}"#).unwrap_err();
        assert!(err.to_string().contains("missing field `height`"), "{err}");
        let err = serde_json::from_str::<GroupChild>(r#"{"Recct":{}}"#).unwrap_err();
        assert!(err.to_string().contains("unknown variant `Recct`"), "{err}");
    }

    /// 이것을 실패시키는 것: 두 번째 키 검사 삭제.
    #[test]
    fn a_second_key_is_rejected() {
        let w = HwpUnit::new(1000).unwrap();
        let pic = serde_json::to_string(&Image::new("a.png", w, w, ImageFormat::Png)).unwrap();
        let two = format!(r#"{{"Picture":{pic},"Rect":{}}}"#, rect_json());
        let err = serde_json::from_str::<GroupChild>(&two).unwrap_err();
        assert!(err.to_string().contains("invalid length 2"), "{err}");
    }
}
