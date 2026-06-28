//! Stable document-object identity used to link cross-references to targets.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A document-object identity that links a cross-reference to its target.
///
/// Cross-reference *targets* — image, table, equation, group, text-art,
/// footnote, endnote — carry an `Option<ObjectId>` (the `inst_id` field),
/// and the *referencing* [`Control::CrossRef`](crate::Control::CrossRef)
/// carries the **same** `ObjectId` via
/// [`RefTarget::Object`](crate::control::RefTarget::Object). Sharing one id
/// type makes the target↔referencer link checkable at the type level instead
/// of relying on two unrelated integer fields coincidentally agreeing on a
/// raw value (the latent fragility this newtype replaces — see ADR-010).
///
/// `ObjectId` is format-agnostic: Core only carries *identity*. The wire
/// encoder owns id *allocation* — preserving ids imported from a source
/// document and assigning fresh ids to authored objects. Serialization is
/// [`#[serde(transparent)]`](https://serde.rs/container-attrs.html#transparent),
/// so an `ObjectId` round-trips through JSON/YAML as a bare integer.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ObjectId(u64);

impl ObjectId {
    /// Wraps a raw integer id.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the wrapped raw integer id.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for ObjectId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<ObjectId> for u64 {
    fn from(id: ObjectId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_value_round_trip() {
        assert_eq!(ObjectId::new(0).value(), 0);
        assert_eq!(ObjectId::new(u64::MAX).value(), u64::MAX);
        assert_eq!(ObjectId::new(1_108_165_575).value(), 1_108_165_575);
    }

    #[test]
    fn display_is_bare_integer() {
        assert_eq!(ObjectId::new(42).to_string(), "42");
        assert_eq!(ObjectId::new(0).to_string(), "0");
    }

    #[test]
    fn from_conversions() {
        let id: ObjectId = 7u64.into();
        assert_eq!(id, ObjectId::new(7));
        let raw: u64 = ObjectId::new(7).into();
        assert_eq!(raw, 7);
    }

    #[test]
    fn serde_is_transparent_bare_integer() {
        let id = ObjectId::new(40_257_166);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "40257166");
        let back: ObjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);

        // Round-trips inside an Option exactly like a bare integer would.
        let some: Option<ObjectId> = Some(id);
        assert_eq!(serde_json::to_string(&some).unwrap(), "40257166");
        let none: Option<ObjectId> = None;
        assert_eq!(serde_json::to_string(&none).unwrap(), "null");
    }

    #[test]
    fn equality_enables_link_check() {
        // The whole point: a target id and a referencer id are comparable.
        let target = Some(ObjectId::new(123));
        let referencer = ObjectId::new(123);
        assert_eq!(target, Some(referencer));
    }
}
