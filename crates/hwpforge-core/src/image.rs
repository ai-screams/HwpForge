//! Image types for embedded or referenced images.
//!
//! [`Image`] represents an image reference within a document. Core stores
//! only the path and dimensions -- actual binary data lives in the Smithy
//! layer (inside the HWPX ZIP or HWP5 BinData stream).
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::image::{Image, ImageFormat};
//! use hwpforge_foundation::HwpUnit;
//!
//! let img = Image {
//!     path: "BinData/image1.png".to_string(),
//!     width: HwpUnit::from_mm(50.0).unwrap(),
//!     height: HwpUnit::from_mm(30.0).unwrap(),
//!     format: ImageFormat::Png,
//!     caption: None,
//! };
//! assert!(img.path.ends_with(".png"));
//! ```

use std::collections::HashMap;

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;

/// An image reference within the document.
///
/// Contains the path to the image resource (relative to the document
/// package root), its display dimensions, and format hint.
///
/// # No Binary Data
///
/// Core deliberately holds no image bytes. The Smithy crate resolves
/// `path` into actual binary data during encode/decode.
///
/// # Examples
///
/// ```
/// use hwpforge_core::image::{Image, ImageFormat};
/// use hwpforge_foundation::HwpUnit;
///
/// let img = Image::new(
///     "BinData/logo.jpeg",
///     HwpUnit::from_mm(80.0).unwrap(),
///     HwpUnit::from_mm(40.0).unwrap(),
///     ImageFormat::Jpeg,
/// );
/// assert_eq!(img.format, ImageFormat::Jpeg);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Image {
    /// Relative path within the document package (e.g. `"BinData/image1.png"`).
    pub path: String,
    /// Display width.
    pub width: HwpUnit,
    /// Display height.
    pub height: HwpUnit,
    /// Image format hint.
    pub format: ImageFormat,
    /// Optional image caption.
    pub caption: Option<Caption>,
}

impl Image {
    /// Creates a new image reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::image::{Image, ImageFormat};
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let img = Image::new(
    ///     "images/photo.png",
    ///     HwpUnit::from_mm(100.0).unwrap(),
    ///     HwpUnit::from_mm(75.0).unwrap(),
    ///     ImageFormat::Png,
    /// );
    /// assert_eq!(img.path, "images/photo.png");
    /// ```
    pub fn new(
        path: impl Into<String>,
        width: HwpUnit,
        height: HwpUnit,
        format: ImageFormat,
    ) -> Self {
        Self { path: path.into(), width, height, format, caption: None }
    }
}

impl std::fmt::Display for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image({}, {:.1}mm x {:.1}mm)",
            self.format,
            self.width.to_mm(),
            self.height.to_mm()
        )
    }
}

/// Supported image formats.
///
/// Marked `#[non_exhaustive]` so new formats can be added in future
/// phases without a breaking change.
///
/// # Examples
///
/// ```
/// use hwpforge_core::image::ImageFormat;
///
/// let fmt = ImageFormat::Png;
/// assert_eq!(fmt.to_string(), "PNG");
///
/// let unknown = ImageFormat::Unknown("SVG".to_string());
/// assert_eq!(unknown.to_string(), "SVG");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// Graphics Interchange Format.
    Gif,
    /// Windows Bitmap.
    Bmp,
    /// Windows Metafile.
    Wmf,
    /// Enhanced Metafile.
    Emf,
    /// Unrecognized format with its extension or MIME type.
    Unknown(String),
}

impl std::fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Png => write!(f, "PNG"),
            Self::Jpeg => write!(f, "JPEG"),
            Self::Gif => write!(f, "GIF"),
            Self::Bmp => write!(f, "BMP"),
            Self::Wmf => write!(f, "WMF"),
            Self::Emf => write!(f, "EMF"),
            Self::Unknown(s) => write!(f, "{s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// ImageStore
// ---------------------------------------------------------------------------

/// Storage for binary image data keyed by path.
///
/// Maps image paths (e.g. `"image1.jpg"`) to their binary content.
/// Used by the encoder to embed images into HWPX archives and by the
/// decoder to extract them.
///
/// # Examples
///
/// ```
/// use hwpforge_core::image::ImageStore;
///
/// let mut store = ImageStore::new();
/// store.insert("logo.png", vec![0x89, 0x50, 0x4E, 0x47]);
/// assert_eq!(store.len(), 1);
/// assert!(store.get("logo.png").is_some());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageStore {
    images: HashMap<String, Vec<u8>>,
}

impl ImageStore {
    /// Creates an empty image store.
    pub fn new() -> Self {
        Self { images: HashMap::new() }
    }

    /// Inserts an image with the given key and binary data.
    ///
    /// If the key already exists, the data is replaced.
    pub fn insert(&mut self, key: impl Into<String>, data: Vec<u8>) {
        self.images.insert(key.into(), data);
    }

    /// Returns the binary data for the given key, if present.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.images.get(key).map(|v| v.as_slice())
    }

    /// Returns the number of stored images.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Returns `true` if the store contains no images.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Iterates over all `(key, data)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.images.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

impl FromIterator<(String, Vec<u8>)> for ImageStore {
    fn from_iter<I: IntoIterator<Item = (String, Vec<u8>)>>(iter: I) -> Self {
        Self { images: iter.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_image() -> Image {
        Image::new(
            "BinData/image1.png",
            HwpUnit::from_mm(50.0).unwrap(),
            HwpUnit::from_mm(30.0).unwrap(),
            ImageFormat::Png,
        )
    }

    #[test]
    fn new_constructor() {
        let img = sample_image();
        assert_eq!(img.path, "BinData/image1.png");
        assert_eq!(img.format, ImageFormat::Png);
    }

    #[test]
    fn struct_literal_construction() {
        let img = Image {
            path: "test.jpeg".to_string(),
            width: HwpUnit::from_mm(10.0).unwrap(),
            height: HwpUnit::from_mm(10.0).unwrap(),
            format: ImageFormat::Jpeg,
            caption: None,
        };
        assert_eq!(img.format, ImageFormat::Jpeg);
    }

    #[test]
    fn display_format() {
        let img = sample_image();
        let s = img.to_string();
        assert!(s.contains("PNG"), "display: {s}");
        assert!(s.contains("50.0"), "display: {s}");
        assert!(s.contains("30.0"), "display: {s}");
    }

    #[test]
    fn image_format_display() {
        assert_eq!(ImageFormat::Png.to_string(), "PNG");
        assert_eq!(ImageFormat::Jpeg.to_string(), "JPEG");
        assert_eq!(ImageFormat::Gif.to_string(), "GIF");
        assert_eq!(ImageFormat::Bmp.to_string(), "BMP");
        assert_eq!(ImageFormat::Wmf.to_string(), "WMF");
        assert_eq!(ImageFormat::Emf.to_string(), "EMF");
        assert_eq!(ImageFormat::Unknown("TIFF".to_string()).to_string(), "TIFF");
    }

    #[test]
    fn equality() {
        let a = sample_image();
        let b = sample_image();
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_on_different_paths() {
        let a = sample_image();
        let mut b = sample_image();
        b.path = "other.png".to_string();
        assert_ne!(a, b);
    }

    #[test]
    fn clone_independence() {
        let img = sample_image();
        let mut cloned = img.clone();
        cloned.path = "modified.png".to_string();
        assert_eq!(img.path, "BinData/image1.png");
    }

    #[test]
    fn serde_roundtrip() {
        let img = sample_image();
        let json = serde_json::to_string(&img).unwrap();
        let back: Image = serde_json::from_str(&json).unwrap();
        assert_eq!(img, back);
    }

    #[test]
    fn serde_unknown_format_roundtrip() {
        let img = Image::new(
            "test.svg",
            HwpUnit::from_mm(10.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
            ImageFormat::Unknown("SVG".to_string()),
        );
        let json = serde_json::to_string(&img).unwrap();
        let back: Image = serde_json::from_str(&json).unwrap();
        assert_eq!(img, back);
    }

    #[test]
    fn image_format_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ImageFormat::Png);
        set.insert(ImageFormat::Jpeg);
        set.insert(ImageFormat::Png);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn from_string_path() {
        let path = String::from("dynamic/path.bmp");
        let img = Image::new(path, HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Bmp);
        assert_eq!(img.path, "dynamic/path.bmp");
    }

    // -----------------------------------------------------------------------
    // ImageStore tests
    // -----------------------------------------------------------------------

    #[test]
    fn image_store_new_is_empty() {
        let store = ImageStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn image_store_insert_and_get() {
        let mut store = ImageStore::new();
        store.insert("logo.png", vec![0x89, 0x50, 0x4E, 0x47]);
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
        assert_eq!(store.get("logo.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));
    }

    #[test]
    fn image_store_get_missing() {
        let store = ImageStore::new();
        assert!(store.get("nonexistent.png").is_none());
    }

    #[test]
    fn image_store_insert_replaces() {
        let mut store = ImageStore::new();
        store.insert("img.png", vec![1, 2, 3]);
        store.insert("img.png", vec![4, 5, 6]);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("img.png"), Some(&[4, 5, 6][..]));
    }

    #[test]
    fn image_store_multiple_images() {
        let mut store = ImageStore::new();
        store.insert("a.png", vec![1]);
        store.insert("b.jpg", vec![2]);
        store.insert("c.gif", vec![3]);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn image_store_iter() {
        let mut store = ImageStore::new();
        store.insert("a.png", vec![1]);
        store.insert("b.jpg", vec![2]);
        let pairs: Vec<_> = store.iter().collect();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn image_store_from_iterator() {
        let items = vec![("a.png".to_string(), vec![1, 2]), ("b.jpg".to_string(), vec![3, 4])];
        let store: ImageStore = items.into_iter().collect();
        assert_eq!(store.len(), 2);
        assert_eq!(store.get("a.png"), Some(&[1, 2][..]));
    }

    #[test]
    fn image_store_default() {
        let store = ImageStore::default();
        assert!(store.is_empty());
    }

    #[test]
    fn image_store_clone_independence() {
        let mut store = ImageStore::new();
        store.insert("img.png", vec![1, 2, 3]);
        let mut cloned = store.clone();
        cloned.insert("other.png", vec![4, 5]);
        assert_eq!(store.len(), 1);
        assert_eq!(cloned.len(), 2);
    }

    #[test]
    fn image_store_equality() {
        let mut a = ImageStore::new();
        a.insert("img.png", vec![1, 2, 3]);
        let mut b = ImageStore::new();
        b.insert("img.png", vec![1, 2, 3]);
        assert_eq!(a, b);
    }

    #[test]
    fn image_store_serde_roundtrip() {
        let mut store = ImageStore::new();
        store.insert("logo.png", vec![0x89, 0x50]);
        let json = serde_json::to_string(&store).unwrap();
        let back: ImageStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store, back);
    }

    #[test]
    fn image_store_string_key() {
        let mut store = ImageStore::new();
        let key = String::from("dynamic/path.png");
        store.insert(key, vec![42]);
        assert!(store.get("dynamic/path.png").is_some());
    }
}
