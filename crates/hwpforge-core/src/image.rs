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

use std::collections::HashMap;

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::object_id::ObjectId;
use crate::placement::ObjectPlacement;

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
    pub placement: Option<ObjectPlacement>,
    /// Wave 12p Step 2a: instance ID for cross-ref target lookup. HWP5
    /// 변환 시 GSO CtrlHeader trailer 의 instance ID 가 채워지고, HWPX
    /// encoder 가 `<hp:pic id="...">` attribute 로 emit. `None` 이면
    /// encoder 가 fallback 값 (예: sequential counter) 을 사용해도 됨.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<ObjectId>,
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
        Self {
            path: path.into(),
            width,
            height,
            format,
            caption: None,
            placement: None,
            inst_id: None,
        }
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
        Self { path, width, height, format, caption: None, placement: None, inst_id: None }
    }

    /// Attaches a caption to the image.
    #[must_use]
    pub fn with_caption(mut self, caption: Caption) -> Self {
        self.caption = Some(caption);
        self
    }

    /// Attaches placement metadata while preserving the existing constructor API.
    #[must_use]
    pub fn with_placement(mut self, placement: ObjectPlacement) -> Self {
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

    /// Sniffs an [`ImageFormat`] from the leading magic bytes.
    ///
    /// Extension-derived formats ([`Self::from_extension`]) are diagnostic
    /// hints only — file names cannot be trusted. Byte sniffing is the
    /// ground truth for admission decisions (e.g. the Markdown → HWPX image
    /// embed loader only packages bytes whose magic identifies a format
    /// HWPX `BinData` natively carries).
    ///
    /// Returns `None` for empty, truncated, or unrecognized bytes — callers
    /// must not guess from content. Magic table (shares the render-side
    /// sniffer's rules in `smithy-pdf`, with two deliberate divergences for
    /// ingestion, W6 §12b·§12-r2):
    ///
    /// - PNG `89 50 4E 47 0D 0A 1A 0A` · JPEG `FF D8 FF` · GIF `GIF87a`/`GIF89a`
    /// - BMP `BM` **plus structural header checks** — `bfOffBits`
    ///   (offset 10) within `14..=len` and a known DIB header size
    ///   (offset 14 ∈ {12, 40, 52, 56, 64, 108, 124}). The 2-byte magic
    ///   alone admits arbitrary bytes starting with `BM` into packages
    ///   (ingestion is stricter than the render sniffer, which only gates
    ///   an error path).
    /// - EMF record type `01 00 00 00` + `" EMF"` signature at offset 40
    /// - WMF **placeable only** (`D7 CD C6 9A`) — standard WMF's magic is
    ///   too weak and would misfire, so it stays unrecognized
    /// - WebP is **intentionally absent** (render sniffer knows it): HWPX
    ///   `BinData` carry of WebP is unverified against Hancom, so ingestion
    ///   refuses it until a native fixture proves it (§12-r2).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::image::ImageFormat;
    ///
    /// let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
    /// assert_eq!(ImageFormat::sniff(&png), Some(ImageFormat::Png));
    /// assert_eq!(ImageFormat::sniff(b"GIF89a\x00"), Some(ImageFormat::Gif));
    /// assert_eq!(ImageFormat::sniff(b"not an image"), None);
    /// assert_eq!(ImageFormat::sniff(&[]), None);
    /// ```
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
            return Some(Self::Png);
        }
        if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
            return Some(Self::Jpeg);
        }
        if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
            return Some(Self::Gif);
        }
        if bytes.len() >= 18 && &bytes[..2] == b"BM" {
            let off_bits = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
            let dib_size = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
            let off_ok = off_bits >= 14 && (off_bits as usize) <= bytes.len();
            let dib_ok = matches!(dib_size, 12 | 40 | 52 | 56 | 64 | 108 | 124);
            if off_ok && dib_ok {
                return Some(Self::Bmp);
            }
            return None;
        }
        if bytes.len() >= 44 && bytes[..4] == [0x01, 0, 0, 0] && &bytes[40..44] == b" EMF" {
            return Some(Self::Emf);
        }
        if bytes.len() >= 4 && bytes[..4] == [0xD7, 0xCD, 0xC6, 0x9A] {
            return Some(Self::Wmf);
        }
        None
    }

    /// Canonical file extension for a sniffed format (used to build
    /// synthetic package keys like `image1.png`).
    ///
    /// Returns `None` for [`Self::Unknown`] — unknown formats never get a
    /// synthetic key (they are not admitted into packages).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::image::ImageFormat;
    ///
    /// assert_eq!(ImageFormat::Jpeg.canonical_extension(), Some("jpg"));
    /// assert_eq!(ImageFormat::Unknown("svg".into()).canonical_extension(), None);
    /// ```
    #[must_use]
    pub fn canonical_extension(&self) -> Option<&'static str> {
        match self {
            Self::Png => Some("png"),
            Self::Jpeg => Some("jpg"),
            Self::Gif => Some("gif"),
            Self::Bmp => Some("bmp"),
            Self::Wmf => Some("wmf"),
            Self::Emf => Some("emf"),
            Self::Unknown(_) => None,
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
        use crate::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
        let img = sample_image().with_placement(ObjectPlacement {
            text_wrap: ObjectTextWrap::Square,
            text_flow: ObjectTextFlow::RightOnly,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: true,
            vert_rel_to: ObjectRelativeTo::Paper,
            horz_rel_to: ObjectRelativeTo::Page,
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
    // ImageFormat::sniff tests (W6 §12b — magic 판별, 추측 금지)
    // -----------------------------------------------------------------------

    #[test]
    fn sniff_known_magics() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert_eq!(ImageFormat::sniff(&png), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::sniff(b"GIF87a\x00"), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::sniff(b"GIF89a\x00"), Some(ImageFormat::Gif));
        // BMP: 유효 구조 헤더 (BITMAPCOREHEADER — bfOffBits=26, DIB=12).
        let mut bmp = Vec::from(&b"BM"[..]);
        bmp.extend_from_slice(&[0u8; 8]); // size(4)+reserved(4)
        bmp.extend_from_slice(&26u32.to_le_bytes()); // bfOffBits
        bmp.extend_from_slice(&12u32.to_le_bytes()); // DIB header size
        bmp.extend_from_slice(&[0u8; 8]); // 나머지 코어 헤더
        assert_eq!(ImageFormat::sniff(&bmp), Some(ImageFormat::Bmp));
        let mut emf = vec![0x01, 0, 0, 0];
        emf.extend_from_slice(&[0u8; 36]);
        emf.extend_from_slice(b" EMF");
        assert_eq!(ImageFormat::sniff(&emf), Some(ImageFormat::Emf));
        assert_eq!(ImageFormat::sniff(&[0xD7, 0xCD, 0xC6, 0x9A, 0, 0]), Some(ImageFormat::Wmf));
    }

    #[test]
    fn sniff_truncated_and_weak_magics_are_none() {
        assert_eq!(ImageFormat::sniff(&[]), None);
        // BMP 2바이트 magic 단독은 약함 — 14바이트 헤더 미달 = None.
        assert_eq!(ImageFormat::sniff(b"BM"), None);
        // PNG/JPEG/GIF 절단 prefix.
        assert_eq!(ImageFormat::sniff(&[0x89, b'P']), None);
        assert_eq!(ImageFormat::sniff(&[0xFF, 0xD8]), None);
        assert_eq!(ImageFormat::sniff(b"GIF8"), None);
        // EMF: 레코드 타입만으론 부족 (오프셋 40 시그니처 필요).
        assert_eq!(ImageFormat::sniff(&[0x01, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn sniff_bmp_requires_structural_header() {
        // 2바이트 magic 만으론 임의 바이트 반입 가능 (독립 리뷰 M1) —
        // bfOffBits·DIB 크기 구조 검사로 차단.
        assert_eq!(ImageFormat::sniff(b"BMsecret-credential-material-here-0123456789"), None);
        // 구 14바이트 규칙이 수용하던 제로 헤더도 거부 (DIB 0 미지).
        let mut zeros = Vec::from(&b"BM"[..]);
        zeros.extend_from_slice(&[0u8; 20]);
        assert_eq!(ImageFormat::sniff(&zeros), None);
        // bfOffBits 가 파일 길이를 초과하면 거부.
        let mut oob = Vec::from(&b"BM"[..]);
        oob.extend_from_slice(&[0u8; 8]);
        oob.extend_from_slice(&999u32.to_le_bytes());
        oob.extend_from_slice(&40u32.to_le_bytes());
        assert_eq!(ImageFormat::sniff(&oob), None);
    }

    #[test]
    fn sniff_never_guesses_from_content() {
        assert_eq!(ImageFormat::sniff(b"<svg xmlns=\"http\""), None);
        assert_eq!(ImageFormat::sniff(&[0u8; 64]), None);
        // RIFF 컨테이너(WAV 등) = None — WebP 포함 미지원 (HWPX BinData
        // 캐리 대상 아님).
        let mut wav = Vec::from(&b"RIFF"[..]);
        wav.extend_from_slice(&[0x10, 0, 0, 0]);
        wav.extend_from_slice(b"WAVEfmt ");
        assert_eq!(ImageFormat::sniff(&wav), None);
    }

    #[test]
    fn canonical_extension_covers_all_known() {
        assert_eq!(ImageFormat::Png.canonical_extension(), Some("png"));
        assert_eq!(ImageFormat::Jpeg.canonical_extension(), Some("jpg"));
        assert_eq!(ImageFormat::Gif.canonical_extension(), Some("gif"));
        assert_eq!(ImageFormat::Bmp.canonical_extension(), Some("bmp"));
        assert_eq!(ImageFormat::Wmf.canonical_extension(), Some("wmf"));
        assert_eq!(ImageFormat::Emf.canonical_extension(), Some("emf"));
        assert_eq!(ImageFormat::Unknown("svg".into()).canonical_extension(), None);
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
