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
//! let img = Image::new(
//!     "BinData/image1.png",
//!     HwpUnit::from_mm(50.0).unwrap(),
//!     HwpUnit::from_mm(30.0).unwrap(),
//!     ImageFormat::Png,
//! );
//! assert!(img.path.ends_with(".png"));
//! ```

use std::borrow::Cow;
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
#[non_exhaustive]
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
    /// Optional placement/presentation metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<ImagePlacement>,
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
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        width: HwpUnit,
        height: HwpUnit,
        format: ImageFormat,
    ) -> Self {
        Self { path: path.into(), width, height, format, caption: None, placement: None }
    }

    /// Creates an image reference by inferring the format from the file extension.
    ///
    /// The extension is case-insensitive. Unrecognized extensions produce
    /// [`ImageFormat::Unknown`] containing the lowercase extension string.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::image::{Image, ImageFormat};
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let w = HwpUnit::from_mm(100.0).unwrap();
    /// let h = HwpUnit::from_mm(75.0).unwrap();
    ///
    /// let img = Image::from_path("photos/hero.png", w, h);
    /// assert_eq!(img.format, ImageFormat::Png);
    ///
    /// let img_jpg = Image::from_path("scan.JPG", w, h);
    /// assert_eq!(img_jpg.format, ImageFormat::Jpeg);
    ///
    /// let img_unknown = Image::from_path("diagram.svg", w, h);
    /// assert_eq!(img_unknown.format, ImageFormat::Unknown("svg".to_string()));
    /// ```
    #[must_use]
    pub fn from_path(path: impl Into<String>, width: HwpUnit, height: HwpUnit) -> Self {
        let path: String = path.into();
        let format = ImageFormat::from_extension(&path);
        Self { path, width, height, format, caption: None, placement: None }
    }

    /// Attaches a caption to the image.
    #[must_use]
    pub fn with_caption(mut self, caption: Caption) -> Self {
        self.caption = Some(caption);
        self
    }

    /// Attaches placement metadata while preserving the existing constructor API.
    #[must_use]
    pub fn with_placement(mut self, placement: ImagePlacement) -> Self {
        self.placement = Some(placement);
        self
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

/// Optional object-placement metadata for images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImagePlacement {
    /// Text wrapping mode around the image object.
    pub text_wrap: ImageTextWrap,
    /// Side flow policy around the wrapped object.
    pub text_flow: ImageTextFlow,
    /// Whether the object behaves like an inline character.
    pub treat_as_char: bool,
    /// Whether surrounding text should flow with the object.
    pub flow_with_text: bool,
    /// Whether overlapping other objects is allowed.
    pub allow_overlap: bool,
    /// Vertical anchor reference for `vert_offset`.
    pub vert_rel_to: ImageRelativeTo,
    /// Horizontal anchor reference for `horz_offset`.
    pub horz_rel_to: ImageRelativeTo,
    /// Vertical offset from `vert_rel_to`.
    pub vert_offset: HwpUnit,
    /// Horizontal offset from `horz_rel_to`.
    pub horz_offset: HwpUnit,
}

impl ImagePlacement {
    /// Legacy inline defaults used by the pre-placement HWPX image path.
    pub fn legacy_inline_defaults() -> Self {
        Self {
            text_wrap: ImageTextWrap::TopAndBottom,
            text_flow: ImageTextFlow::BothSides,
            treat_as_char: true,
            flow_with_text: false,
            allow_overlap: false,
            vert_rel_to: ImageRelativeTo::Para,
            horz_rel_to: ImageRelativeTo::Para,
            vert_offset: HwpUnit::ZERO,
            horz_offset: HwpUnit::ZERO,
        }
    }
}

/// Text wrapping mode for placed images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ImageTextWrap {
    /// Place text above and below the object.
    TopAndBottom,
    /// Wrap text on the object's sides.
    Square,
    /// Place the object behind text.
    BehindText,
    /// Place the object in front of text.
    InFrontOfText,
    /// Tight text wrapping around the object.
    Tight,
    /// Through-style wrapping.
    Through,
    /// Any wrap value not modeled explicitly.
    Other(String),
}

impl ImageTextWrap {
    /// Converts a raw HWPX wrap string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "TOP_AND_BOTTOM" => Self::TopAndBottom,
            "SQUARE" => Self::Square,
            "BEHIND_TEXT" => Self::BehindText,
            "IN_FRONT_OF_TEXT" => Self::InFrontOfText,
            "TIGHT" => Self::Tight,
            "THROUGH" => Self::Through,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this wrap mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::TopAndBottom => Cow::Borrowed("TOP_AND_BOTTOM"),
            Self::Square => Cow::Borrowed("SQUARE"),
            Self::BehindText => Cow::Borrowed("BEHIND_TEXT"),
            Self::InFrontOfText => Cow::Borrowed("IN_FRONT_OF_TEXT"),
            Self::Tight => Cow::Borrowed("TIGHT"),
            Self::Through => Cow::Borrowed("THROUGH"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
    }
}

/// Text flow mode for placed images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ImageTextFlow {
    /// Text can flow on both sides.
    BothSides,
    /// Text can flow only on the left side.
    LeftOnly,
    /// Text can flow only on the right side.
    RightOnly,
    /// Use the side with the larger available space.
    LargestOnly,
    /// Any flow value not modeled explicitly.
    Other(String),
}

impl ImageTextFlow {
    /// Converts a raw HWPX flow string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "BOTH_SIDES" => Self::BothSides,
            "LEFT_ONLY" => Self::LeftOnly,
            "RIGHT_ONLY" => Self::RightOnly,
            "LARGEST_ONLY" => Self::LargestOnly,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this flow mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::BothSides => Cow::Borrowed("BOTH_SIDES"),
            Self::LeftOnly => Cow::Borrowed("LEFT_ONLY"),
            Self::RightOnly => Cow::Borrowed("RIGHT_ONLY"),
            Self::LargestOnly => Cow::Borrowed("LARGEST_ONLY"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
    }
}

/// Anchor target for image placement offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ImageRelativeTo {
    /// Anchor offsets to the paper.
    Paper,
    /// Anchor offsets to the page.
    Page,
    /// Anchor offsets to the paragraph.
    Para,
    /// Anchor offsets to the column.
    Column,
    /// Anchor offsets to the character box.
    Character,
    /// Anchor offsets to the line box.
    Line,
    /// Any anchor value not modeled explicitly.
    Other(String),
}

impl ImageRelativeTo {
    /// Converts a raw HWPX anchor string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "PAPER" => Self::Paper,
            "PAGE" => Self::Page,
            "PARA" => Self::Para,
            "COLUMN" => Self::Column,
            "CHAR" => Self::Character,
            "LINE" => Self::Line,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this anchor mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::Paper => Cow::Borrowed("PAPER"),
            Self::Page => Cow::Borrowed("PAGE"),
            Self::Para => Cow::Borrowed("PARA"),
            Self::Column => Cow::Borrowed("COLUMN"),
            Self::Character => Cow::Borrowed("CHAR"),
            Self::Line => Cow::Borrowed("LINE"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
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
/// assert_eq!(unknown.to_string(), "svg");
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

impl ImageFormat {
    /// Infers an [`ImageFormat`] from a file path's extension.
    ///
    /// The extension is extracted from everything after the last `'.'` in the
    /// path string and matched case-insensitively. If no dot is found, or the
    /// extension is not recognized, [`ImageFormat::Unknown`] is returned
    /// containing the lowercase extension (or an empty string when absent).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::image::ImageFormat;
    ///
    /// assert_eq!(ImageFormat::from_extension("photo.png"),  ImageFormat::Png);
    /// assert_eq!(ImageFormat::from_extension("image.JPG"),  ImageFormat::Jpeg);
    /// assert_eq!(ImageFormat::from_extension("file.jpeg"), ImageFormat::Jpeg);
    /// assert_eq!(ImageFormat::from_extension("doc.gif"),   ImageFormat::Gif);
    /// assert_eq!(ImageFormat::from_extension("img.bmp"),   ImageFormat::Bmp);
    /// assert_eq!(ImageFormat::from_extension("chart.wmf"), ImageFormat::Wmf);
    /// assert_eq!(ImageFormat::from_extension("dia.emf"),   ImageFormat::Emf);
    /// assert_eq!(
    ///     ImageFormat::from_extension("file.xyz"),
    ///     ImageFormat::Unknown("xyz".to_string()),
    /// );
    /// assert_eq!(
    ///     ImageFormat::from_extension("noext"),
    ///     ImageFormat::Unknown(String::new()),
    /// );
    /// assert_eq!(ImageFormat::from_extension("multi.dot.png"), ImageFormat::Png);
    /// ```
    pub fn from_extension(path: &str) -> Self {
        // Only treat the suffix as an extension if a dot is actually present.
        let ext_lower = path.rfind('.').map(|i| path[i + 1..].to_ascii_lowercase());
        match ext_lower.as_deref() {
            Some("png") => Self::Png,
            Some("jpg" | "jpeg") => Self::Jpeg,
            Some("gif") => Self::Gif,
            Some("bmp") => Self::Bmp,
            Some("wmf") => Self::Wmf,
            Some("emf") => Self::Emf,
            Some(ext) => Self::Unknown(ext.to_string()),
            None => Self::Unknown(String::new()),
        }
    }
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
            Self::Unknown(s) => {
                let lower = s.to_ascii_lowercase();
                write!(f, "{lower}")
            }
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
    fn from_path_constructor() {
        let img = Image::from_path(
            "test.jpeg",
            HwpUnit::from_mm(10.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
        );
        assert_eq!(img.format, ImageFormat::Jpeg);
    }

    #[test]
    fn builder_attaches_caption() {
        let img = sample_image().with_caption(Caption::default());
        assert!(img.caption.is_some());
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
        assert_eq!(ImageFormat::Unknown("TIFF".to_string()).to_string(), "tiff");
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
    fn placement_roundtrip() {
        let img = sample_image().with_placement(ImagePlacement {
            text_wrap: ImageTextWrap::Square,
            text_flow: ImageTextFlow::RightOnly,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: true,
            vert_rel_to: ImageRelativeTo::Paper,
            horz_rel_to: ImageRelativeTo::Page,
            vert_offset: HwpUnit::new(1200).unwrap(),
            horz_offset: HwpUnit::new(3400).unwrap(),
        });
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

    // -----------------------------------------------------------------------
    // ImageFormat::from_extension tests
    // -----------------------------------------------------------------------

    #[test]
    fn from_extension_png() {
        assert_eq!(ImageFormat::from_extension("photo.png"), ImageFormat::Png);
    }

    #[test]
    fn from_extension_jpg_uppercase() {
        assert_eq!(ImageFormat::from_extension("image.JPG"), ImageFormat::Jpeg);
    }

    #[test]
    fn from_extension_jpeg() {
        assert_eq!(ImageFormat::from_extension("file.jpeg"), ImageFormat::Jpeg);
    }

    #[test]
    fn from_extension_gif() {
        assert_eq!(ImageFormat::from_extension("doc.gif"), ImageFormat::Gif);
    }

    #[test]
    fn from_extension_bmp() {
        assert_eq!(ImageFormat::from_extension("img.bmp"), ImageFormat::Bmp);
    }

    #[test]
    fn from_extension_wmf() {
        assert_eq!(ImageFormat::from_extension("chart.wmf"), ImageFormat::Wmf);
    }

    #[test]
    fn from_extension_emf() {
        assert_eq!(ImageFormat::from_extension("dia.emf"), ImageFormat::Emf);
    }

    #[test]
    fn from_extension_unknown() {
        assert_eq!(
            ImageFormat::from_extension("file.xyz"),
            ImageFormat::Unknown("xyz".to_string()),
        );
    }

    #[test]
    fn from_extension_no_extension() {
        assert_eq!(ImageFormat::from_extension("noext"), ImageFormat::Unknown(String::new()));
    }

    #[test]
    fn from_extension_multi_dot() {
        assert_eq!(ImageFormat::from_extension("multi.dot.png"), ImageFormat::Png);
    }

    // -----------------------------------------------------------------------
    // Image::from_path tests
    // -----------------------------------------------------------------------

    #[test]
    fn from_path_infers_format() {
        let w = HwpUnit::from_mm(100.0).unwrap();
        let h = HwpUnit::from_mm(75.0).unwrap();

        let img = Image::from_path("photos/hero.png", w, h);
        assert_eq!(img.format, ImageFormat::Png);
        assert_eq!(img.path, "photos/hero.png");
        assert_eq!(img.width, w);
        assert_eq!(img.height, h);
        assert!(img.caption.is_none());
    }

    #[test]
    fn from_path_jpeg_uppercase() {
        let w = HwpUnit::ZERO;
        let h = HwpUnit::ZERO;
        let img = Image::from_path("scan.JPG", w, h);
        assert_eq!(img.format, ImageFormat::Jpeg);
    }

    #[test]
    fn from_path_unknown_extension() {
        let w = HwpUnit::ZERO;
        let h = HwpUnit::ZERO;
        let img = Image::from_path("diagram.svg", w, h);
        assert_eq!(img.format, ImageFormat::Unknown("svg".to_string()));
    }

    #[test]
    fn from_path_string_owned() {
        let w = HwpUnit::ZERO;
        let h = HwpUnit::ZERO;
        let path = String::from("owned/path.bmp");
        let img = Image::from_path(path, w, h);
        assert_eq!(img.format, ImageFormat::Bmp);
        assert_eq!(img.path, "owned/path.bmp");
    }

    #[test]
    fn unknown_format_display_normalizes_to_lowercase() {
        assert_eq!(ImageFormat::Unknown("SVG".to_string()).to_string(), "svg");
        assert_eq!(ImageFormat::Unknown("Tiff".to_string()).to_string(), "tiff");
        assert_eq!(ImageFormat::Unknown("webp".to_string()).to_string(), "webp");
    }

    #[test]
    fn unknown_format_casing_inequality() {
        // Unknown preserves the stored string for equality, even though display normalizes
        let upper = ImageFormat::Unknown("SVG".to_string());
        let lower = ImageFormat::Unknown("svg".to_string());
        assert_ne!(upper, lower, "Different casing in Unknown produces inequality");
        // But display output is identical
        assert_eq!(upper.to_string(), lower.to_string());
    }
}
