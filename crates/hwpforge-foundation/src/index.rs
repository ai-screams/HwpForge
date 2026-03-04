//! Branded (phantom-typed) index types for type-safe collection access.
//!
//! [`Index<T>`] wraps a `usize` with a phantom type parameter `T` so that
//! indices into different collections cannot be accidentally mixed at
//! compile time.
//!
//! # Why No `Default`?
//!
//! Index 0 is valid data (the first element), not a sentinel value.
//! Providing `Default` would invite bugs where an "uninitialized" index
//! silently points at element 0.
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::CharShapeIndex;
//!
//! let idx = CharShapeIndex::new(3);
//! assert_eq!(idx.get(), 3);
//!
//! // Bounds checking
//! assert!(idx.checked_get(10).is_ok());
//! assert!(idx.checked_get(2).is_err());
//! ```

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{FoundationError, FoundationResult};

/// A branded index into a typed collection.
///
/// The phantom type `T` prevents mixing indices of different domains
/// (e.g. you cannot use a `CharShapeIndex` where a `ParaShapeIndex`
/// is expected).
///
/// Serializes as a plain `usize`, not as a struct.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
///
/// let cs = CharShapeIndex::new(5);
/// let ps = ParaShapeIndex::new(5);
///
/// // cs and ps have the same numeric value but are different types.
/// assert_eq!(cs.get(), ps.get());
/// // cs == ps; // Would not compile -- different phantom types!
/// ```
pub struct Index<T> {
    value: usize,
    _phantom: PhantomData<T>,
}

// Compile-time size guarantee: usize + ZST = usize
const _: () = assert!(std::mem::size_of::<Index<()>>() == std::mem::size_of::<usize>());

// Manual trait impls because derive would require T: Trait bounds we don't want.

impl<T> Clone for Index<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Index<T> {}

impl<T> PartialEq for Index<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for Index<T> {}

impl<T> PartialOrd for Index<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Index<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T> Hash for Index<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<T> Index<T> {
    /// Creates a new index with the given value.
    ///
    /// No bounds checking is performed here; use [`checked_get`](Self::checked_get)
    /// when accessing a collection.
    pub const fn new(value: usize) -> Self {
        Self { value, _phantom: PhantomData }
    }

    /// Returns the raw `usize` value.
    ///
    /// # Note
    ///
    /// The caller is responsible for ensuring this index is within
    /// the bounds of the target collection. Prefer [`checked_get`](Self::checked_get)
    /// for safe access.
    pub const fn get(self) -> usize {
        self.value
    }

    /// Returns the raw value after verifying it is less than `max`.
    ///
    /// # Errors
    ///
    /// Returns [`FoundationError::IndexOutOfBounds`] when
    /// `self.value >= max`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_foundation::FontIndex;
    ///
    /// let idx = FontIndex::new(3);
    /// assert_eq!(idx.checked_get(10).unwrap(), 3);
    /// assert!(idx.checked_get(2).is_err());
    /// ```
    pub fn checked_get(self, max: usize) -> FoundationResult<usize> {
        if self.value >= max {
            return Err(FoundationError::IndexOutOfBounds {
                index: self.value,
                max,
                type_name: std::any::type_name::<T>(),
            });
        }
        Ok(self.value)
    }
}

impl<T> fmt::Debug for Index<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Extract the short type name from the full path
        let full = std::any::type_name::<T>();
        let short = full.rsplit("::").next().unwrap_or(full);
        write!(f, "Index<{short}>({})", self.value)
    }
}

impl<T> fmt::Display for Index<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full = std::any::type_name::<T>();
        let short = full.rsplit("::").next().unwrap_or(full);
        write!(f, "{short}[{}]", self.value)
    }
}

// Serialize as plain usize (not as a struct with phantom)
impl<T> Serialize for Index<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Index<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = usize::deserialize(deserializer)?;
        Ok(Self::new(value))
    }
}

impl<T> schemars::JsonSchema for Index<T> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Index".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<usize>()
    }
}

// ---------------------------------------------------------------------------
// Marker types and type aliases
// ---------------------------------------------------------------------------

/// Phantom marker for character shape indices.
pub struct CharShapeMarker;
/// Phantom marker for paragraph shape indices.
pub struct ParaShapeMarker;
/// Phantom marker for font indices.
pub struct FontMarker;
/// Phantom marker for border/fill indices.
pub struct BorderFillMarker;
/// Phantom marker for style indices.
pub struct StyleMarker;
/// Phantom marker for numbering definition indices.
pub struct NumberingMarker;
/// Phantom marker for tab property indices.
pub struct TabMarker;

/// Index into a character shape collection.
pub type CharShapeIndex = Index<CharShapeMarker>;
/// Index into a paragraph shape collection.
pub type ParaShapeIndex = Index<ParaShapeMarker>;
/// Index into a font collection.
pub type FontIndex = Index<FontMarker>;
/// Index into a border/fill collection.
pub type BorderFillIndex = Index<BorderFillMarker>;
/// Index into a style collection.
pub type StyleIndex = Index<StyleMarker>;
/// Index into the numbering definition list.
pub type NumberingIndex = Index<NumberingMarker>;
/// Index into the tab properties list.
pub type TabIndex = Index<TabMarker>;

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // Index<T> edge cases (10+)
    // ===================================================================

    // Edge Case 1: Index 0 is valid
    #[test]
    fn index_zero_is_valid() {
        let idx = CharShapeIndex::new(0);
        assert_eq!(idx.get(), 0);
        assert!(idx.checked_get(1).is_ok());
    }

    // Edge Case 2: In-range checked_get
    #[test]
    fn index_in_range() {
        let idx = CharShapeIndex::new(5);
        assert_eq!(idx.checked_get(10).unwrap(), 5);
    }

    // Edge Case 3: Out-of-range checked_get
    #[test]
    fn index_out_of_range() {
        let idx = CharShapeIndex::new(10);
        let err = idx.checked_get(5).unwrap_err();
        match err {
            FoundationError::IndexOutOfBounds { index, max, type_name } => {
                assert_eq!(index, 10);
                assert_eq!(max, 5);
                assert!(type_name.contains("CharShape"), "type_name: {type_name}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    // Edge Case 4: checked_get at exact boundary -> error (>= max)
    #[test]
    fn index_at_exact_boundary_is_error() {
        let idx = CharShapeIndex::new(5);
        assert!(idx.checked_get(5).is_err());
    }

    // Edge Case 5: checked_get just below boundary -> ok
    #[test]
    fn index_just_below_boundary() {
        let idx = CharShapeIndex::new(4);
        assert_eq!(idx.checked_get(5).unwrap(), 4);
    }

    // Edge Case 6: usize::MAX
    #[test]
    fn index_usize_max() {
        let idx = CharShapeIndex::new(usize::MAX);
        assert_eq!(idx.get(), usize::MAX);
        assert!(idx.checked_get(usize::MAX).is_err()); // >= max
    }

    // Edge Case 7: Type safety (different phantom types are distinct)
    #[test]
    fn index_type_safety() {
        fn accept_char_shape(_: CharShapeIndex) {}
        fn accept_para_shape(_: ParaShapeIndex) {}

        let cs = CharShapeIndex::new(0);
        let ps = ParaShapeIndex::new(0);

        accept_char_shape(cs);
        accept_para_shape(ps);
        // accept_char_shape(ps); // Would not compile!
    }

    // Edge Case 8: PartialEq, Eq
    #[test]
    fn index_equality() {
        let a = CharShapeIndex::new(5);
        let b = CharShapeIndex::new(5);
        let c = CharShapeIndex::new(6);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // Edge Case 9: Hash (can be used as HashMap key)
    #[test]
    fn index_hash() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(FontIndex::new(0), "Batang");
        map.insert(FontIndex::new(1), "Dotum");
        assert_eq!(map[&FontIndex::new(0)], "Batang");
    }

    // Edge Case 10: Ord
    #[test]
    fn index_ord() {
        let a = CharShapeIndex::new(3);
        let b = CharShapeIndex::new(7);
        assert!(a < b);
    }

    // Edge Case 11: Display format
    #[test]
    fn index_display() {
        let idx = CharShapeIndex::new(3);
        let s = idx.to_string();
        assert!(s.contains("CharShape"), "display: {s}");
        assert!(s.contains("[3]"), "display: {s}");
    }

    // Edge Case 12: Debug format
    #[test]
    fn index_debug() {
        let idx = FontIndex::new(42);
        let s = format!("{idx:?}");
        assert!(s.contains("Font"), "debug: {s}");
        assert!(s.contains("42"), "debug: {s}");
    }

    // Edge Case 13: Serialize as plain usize
    #[test]
    fn index_serde_as_usize() {
        let idx = CharShapeIndex::new(7);
        let json = serde_json::to_string(&idx).unwrap();
        assert_eq!(json, "7");
        let back: CharShapeIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back, idx);
    }

    // Edge Case 14: Copy semantics
    #[test]
    fn index_is_copy() {
        let a = CharShapeIndex::new(1);
        let b = a; // Copy
        assert_eq!(a, b); // both still usable
    }

    // Edge Case 15: checked_get with max=0 -> always error
    #[test]
    fn index_checked_get_empty_collection() {
        let idx = CharShapeIndex::new(0);
        assert!(idx.checked_get(0).is_err());
    }

    // ===================================================================
    // proptest
    // ===================================================================

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_index_in_bounds(idx in 0usize..1000, max in 1usize..2000) {
            let index = CharShapeIndex::new(idx);
            if idx < max {
                prop_assert_eq!(index.checked_get(max).unwrap(), idx);
            } else {
                prop_assert!(index.checked_get(max).is_err());
            }
        }

        #[test]
        fn prop_index_serde_roundtrip(val in 0usize..100_000) {
            let idx = FontIndex::new(val);
            let json = serde_json::to_string(&idx).unwrap();
            let back: FontIndex = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(idx, back);
        }
    }
}
