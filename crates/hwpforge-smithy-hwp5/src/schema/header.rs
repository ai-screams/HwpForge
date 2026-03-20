//! HWP5 `FileHeader` and `DocInfo` schema types.
//!
//! Defines typed Rust structs for the fixed-layout `FileHeader` binary
//! block (signature, version, flags) and the variable-length `DocInfo`
//! records (font list, char properties, para properties, named styles).
//!
//! Currently a stub; full implementation is in Tasks 4 and 6.

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use std::fmt;
use std::io::{Cursor, Read};
use std::str::FromStr;

use hwpforge_foundation::{HeadingType, NumberFormatType};

use crate::error::{Hwp5Error, Hwp5Result};

/// Expected HWP5 file signature (first 17 bytes of the 32-byte signature field).
const HWP5_SIGNATURE: &[u8] = b"HWP Document File";

/// Minimum required size of the `FileHeader` stream.
const FILE_HEADER_SIZE: usize = 256;

/// Version of the HWP5 file format.
///
/// Packed in the file as a single `u32` little-endian:
/// `(major << 24) | (minor << 16) | (build << 8) | revision`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HwpVersion {
    /// Major version (e.g. 5).
    pub major: u8,
    /// Minor version (e.g. 0).
    pub minor: u8,
    /// Build version (e.g. 2).
    pub build: u8,
    /// Revision version (e.g. 5).
    pub revision: u8,
}

impl HwpVersion {
    /// Minimum supported version — anything older is rejected.
    pub const MIN_SUPPORTED: Self = Self::new(5, 0, 0, 0);

    /// Create a new [`HwpVersion`].
    pub const fn new(major: u8, minor: u8, build: u8, revision: u8) -> Self {
        Self { major, minor, build, revision }
    }

    /// Parse from the packed `u32` representation stored in the file.
    fn from_u32(v: u32) -> Self {
        Self {
            major: ((v >> 24) & 0xFF) as u8,
            minor: ((v >> 16) & 0xFF) as u8,
            build: ((v >> 8) & 0xFF) as u8,
            revision: (v & 0xFF) as u8,
        }
    }
}

impl fmt::Display for HwpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.major, self.minor, self.build, self.revision)
    }
}

/// Bitfield flags from bytes 36–39 of the `FileHeader` stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileFlags {
    /// Bit 0: body text is zlib-compressed.
    pub compressed: bool,
    /// Bit 1: document is password-protected.
    pub encrypted: bool,
    /// Bit 2: distribution document.
    pub distributed: bool,
    /// Bit 3: document contains a script.
    pub script: bool,
    /// Bit 4: DRM-protected document.
    pub drm: bool,
    /// Bit 5: XML template document.
    pub xml_template: bool,
    /// Bit 6: document has a history stream.
    pub history: bool,
    /// Bit 7: document carries a digital signature.
    pub signed: bool,
    /// Bit 8: certificate-based encryption.
    pub certificate_encrypt: bool,
    /// Bit 9: signature spare field.
    pub signature_spare: bool,
    /// Bit 10: certificate-based DRM.
    pub certificate_drm: bool,
    /// Bit 11: CCL document.
    pub ccl: bool,
}

impl FileFlags {
    /// Parse from the packed `u32` representation stored in the file.
    fn from_u32(v: u32) -> Self {
        Self {
            compressed: v & 1 == 1,
            encrypted: (v >> 1) & 1 == 1,
            distributed: (v >> 2) & 1 == 1,
            script: (v >> 3) & 1 == 1,
            drm: (v >> 4) & 1 == 1,
            xml_template: (v >> 5) & 1 == 1,
            history: (v >> 6) & 1 == 1,
            signed: (v >> 7) & 1 == 1,
            certificate_encrypt: (v >> 8) & 1 == 1,
            signature_spare: (v >> 9) & 1 == 1,
            certificate_drm: (v >> 10) & 1 == 1,
            ccl: (v >> 11) & 1 == 1,
        }
    }
}

/// Parsed representation of the 256-byte `/FileHeader` stream.
///
/// The stream layout is:
/// - Bytes 0–31: Signature (NUL-padded UTF-8, must start with `"HWP Document File"`)
/// - Bytes 32–35: Version (u32 LE)
/// - Bytes 36–39: Flags (u32 LE)
/// - Bytes 40–255: Reserved (ignored)
#[derive(Debug, Clone, Copy)]
pub struct FileHeader {
    /// Format version extracted from the header.
    pub version: HwpVersion,
    /// Feature flags extracted from the header.
    pub flags: FileFlags,
}

impl FileHeader {
    /// Parse a [`FileHeader`] from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::NotHwp5`] if the data is too short or the
    /// signature does not match. Returns [`Hwp5Error::UnsupportedVersion`]
    /// if the version predates [`HwpVersion::MIN_SUPPORTED`].
    /// Returns [`Hwp5Error::PasswordProtected`] if the encrypted flag is set.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        // 1. Length check
        if data.len() < FILE_HEADER_SIZE {
            return Err(Hwp5Error::NotHwp5 {
                detail: format!(
                    "FileHeader stream too short: {} bytes (expected {})",
                    data.len(),
                    FILE_HEADER_SIZE
                ),
            });
        }

        // 2. Signature check
        if &data[..HWP5_SIGNATURE.len()] != HWP5_SIGNATURE {
            return Err(Hwp5Error::NotHwp5 { detail: "missing or invalid HWP5 signature".into() });
        }

        // 3. Version
        let version_raw = LittleEndian::read_u32(&data[32..36]);
        let version = HwpVersion::from_u32(version_raw);

        // 4. Version gate
        if version < HwpVersion::MIN_SUPPORTED {
            return Err(Hwp5Error::UnsupportedVersion {
                major: version.major,
                minor: version.minor,
                micro: version.build,
                build: version.revision,
            });
        }

        // 5. Flags
        let flags_raw = LittleEndian::read_u32(&data[36..40]);
        let flags = FileFlags::from_u32(flags_raw);

        // 6. Encrypted gate
        if flags.encrypted {
            return Err(Hwp5Error::PasswordProtected);
        }

        Ok(Self { version, flags })
    }
}

// ---------------------------------------------------------------------------
// DocInfo stream record types
// ---------------------------------------------------------------------------

/// Parsed from the `IdMappings` record (TagId 0x11).
///
/// Tells the decoder how many of each style type to expect in the DocInfo stream.
#[derive(Debug, Clone)]
pub struct Hwp5RawIdMappings {
    /// Number of binary data entries.
    pub bin_data_count: i32,
    /// Number of Hangul font faces.
    pub hangul_font_count: i32,
    /// Number of English font faces.
    pub english_font_count: i32,
    /// Number of Hanja font faces.
    pub hanja_font_count: i32,
    /// Number of Japanese font faces.
    pub japanese_font_count: i32,
    /// Number of other-language font faces.
    pub other_font_count: i32,
    /// Number of symbol font faces.
    pub symbol_font_count: i32,
    /// Number of user-defined font faces.
    pub user_font_count: i32,
    /// Number of border/fill definitions.
    pub border_fill_count: i32,
    /// Number of character shape definitions.
    pub char_shape_count: i32,
    /// Number of tab-stop definitions.
    pub tab_def_count: i32,
    /// Number of numbering definitions.
    pub numbering_def_count: i32,
    /// Number of bullet definitions.
    pub bullet_def_count: i32,
    /// Number of paragraph shape definitions.
    pub para_shape_count: i32,
    /// Number of named styles.
    pub style_count: i32,
    /// Memo shape count — present only in v5.0.2.1+.
    pub memo_shape_count: Option<i32>,
    /// Track-change content count — present only in v5.0.3.2+.
    pub change_tracking_count: Option<i32>,
    /// Track-change author count — present only in v5.0.3.2+.
    pub change_tracking_author_count: Option<i32>,
}

impl Hwp5RawIdMappings {
    /// Parse an `IdMappings` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if the data is shorter than the
    /// minimum required 60 bytes.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const MIN_SIZE: usize = 60; // 15 × i32
        if data.len() < MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "IdMappings too short: {} bytes (expected >= {})",
                    data.len(),
                    MIN_SIZE
                ),
            });
        }
        let mut cur = Cursor::new(data);
        Ok(Self {
            bin_data_count: cur.read_i32::<LittleEndian>()?,
            hangul_font_count: cur.read_i32::<LittleEndian>()?,
            english_font_count: cur.read_i32::<LittleEndian>()?,
            hanja_font_count: cur.read_i32::<LittleEndian>()?,
            japanese_font_count: cur.read_i32::<LittleEndian>()?,
            other_font_count: cur.read_i32::<LittleEndian>()?,
            symbol_font_count: cur.read_i32::<LittleEndian>()?,
            user_font_count: cur.read_i32::<LittleEndian>()?,
            border_fill_count: cur.read_i32::<LittleEndian>()?,
            char_shape_count: cur.read_i32::<LittleEndian>()?,
            tab_def_count: cur.read_i32::<LittleEndian>()?,
            numbering_def_count: cur.read_i32::<LittleEndian>()?,
            bullet_def_count: cur.read_i32::<LittleEndian>()?,
            para_shape_count: cur.read_i32::<LittleEndian>()?,
            style_count: cur.read_i32::<LittleEndian>()?,
            memo_shape_count: if data.len() >= 64 {
                Some(cur.read_i32::<LittleEndian>()?)
            } else {
                None
            },
            change_tracking_count: if data.len() >= 68 {
                Some(cur.read_i32::<LittleEndian>()?)
            } else {
                None
            },
            change_tracking_author_count: if data.len() >= 72 {
                Some(cur.read_i32::<LittleEndian>()?)
            } else {
                None
            },
        })
    }
}

// ---------------------------------------------------------------------------

/// Parsed from a `NumberingDef` record (TagId 0x17).
///
/// HWP5 stores the semantic list identity here: a shared numbering ID with up
/// to 10 paragraph heads. Only the fields needed to preserve ordered-list
/// semantics are retained; layout-only attributes are parsed and discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawNumberingParaHead {
    /// Start number for this level.
    pub start_number: u32,
    /// Zero-based list level within the numbering definition.
    pub level: u8,
    /// Number format string normalized to HWPX-style terms.
    pub num_format: String,
    /// Display template text as stored in the HWP5 numbering definition.
    pub text: String,
    /// Checkable flag.
    pub checkable: bool,
}

/// Parsed numbering definition from DocInfo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawNumberingDef {
    /// Numbering start offset.
    pub start: u16,
    /// Paragraph-head definitions in record order.
    pub paragraph_heads: Vec<Hwp5RawNumberingParaHead>,
}

impl Hwp5RawNumberingDef {
    /// Parse a `NumberingDef` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] when the record is truncated or has
    /// unsupported version-dependent layout.
    pub fn parse(data: &[u8], version: &HwpVersion) -> Hwp5Result<Self> {
        let mut cur = Cursor::new(data);
        let mut paragraph_heads = Vec::with_capacity(10);

        for level in 0..7u8 {
            paragraph_heads.push(parse_numbering_para_head(&mut cur, level, true)?);
        }

        let start = cur.read_u16::<LittleEndian>()?;

        if *version >= HwpVersion::new(5, 0, 2, 5) {
            for head in paragraph_heads.iter_mut().take(7) {
                head.start_number = cur.read_u32::<LittleEndian>()?;
            }
        }

        if *version >= HwpVersion::new(5, 1, 0, 0) && cur.position() < data.len() as u64 {
            for level in 7..10u8 {
                paragraph_heads.push(parse_numbering_para_head(&mut cur, level, true)?);
            }

            for head in paragraph_heads.iter_mut().skip(7).take(3) {
                head.start_number = cur.read_u32::<LittleEndian>()?;
            }
        }

        if cur.position() != data.len() as u64 {
            return Err(Hwp5Error::RecordParse {
                offset: cur.position() as usize,
                detail: format!(
                    "NumberingDef parsed {} of {} bytes; trailing bytes are not supported",
                    cur.position(),
                    data.len()
                ),
            });
        }

        Ok(Self { start, paragraph_heads })
    }

    /// Convert to the shared Core numbering definition.
    pub fn to_core_numbering_def(&self, id: u32) -> hwpforge_core::NumberingDef {
        let levels = self
            .paragraph_heads
            .iter()
            .map(|head| hwpforge_core::ParaHead {
                start: head.start_number,
                level: u32::from(head.level) + 1,
                num_format: NumberFormatType::from_str(&head.num_format).unwrap_or_default(),
                text: head.text.clone(),
                checkable: head.checkable,
            })
            .collect();
        hwpforge_core::NumberingDef { id, start: u32::from(self.start), levels }
    }
}

/// Parsed bullet definition from DocInfo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawBulletDef {
    /// Bullet paragraph-head metadata.
    pub paragraph_head: Hwp5RawNumberingParaHead,
    /// Bullet glyph string.
    pub bullet_char: String,
    /// Whether this bullet uses an image marker.
    pub use_image: bool,
    /// Image bullet id when `use_image` is set.
    pub image_id: Option<u32>,
    /// Check-mark glyph, when present.
    pub check_bullet_char: Option<String>,
}

impl Hwp5RawBulletDef {
    /// Parse a bullet record from its raw payload bytes.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const MIN_SIZE: usize = 18;
        if data.len() < MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("Bullet too short: {} bytes (expected >= {MIN_SIZE})", data.len()),
            });
        }

        let mut cur = Cursor::new(data);
        let paragraph_head = parse_numbering_para_head(&mut cur, 0, false)?;
        let bullet_char = decode_utf16_code_unit(cur.read_u16::<LittleEndian>()?);
        let image_bullet_flag = cur.read_i32::<LittleEndian>()?;
        let use_image = image_bullet_flag != 0;
        let image_id = use_image.then_some(image_bullet_flag as u32);

        if use_image && (data.len() as u64).saturating_sub(cur.position()) >= 4 {
            let mut ignored = [0u8; 4];
            cur.read_exact(&mut ignored)?;
        }

        let check_bullet_char = if (data.len() as u64).saturating_sub(cur.position()) >= 2 {
            Some(decode_utf16_code_unit(cur.read_u16::<LittleEndian>()?))
        } else {
            None
        };

        Ok(Self { paragraph_head, bullet_char, use_image, image_id, check_bullet_char })
    }

    /// Convert to the shared Core bullet definition.
    pub fn to_core_bullet_def(&self, id: u32) -> hwpforge_core::BulletDef {
        hwpforge_core::BulletDef {
            id,
            bullet_char: self.bullet_char.clone(),
            use_image: self.use_image,
            para_head: hwpforge_core::ParaHead {
                start: 0,
                level: u32::from(self.paragraph_head.level) + 1,
                num_format: if self.paragraph_head.num_format.is_empty() {
                    NumberFormatType::Digit
                } else {
                    NumberFormatType::from_str(&self.paragraph_head.num_format)
                        .unwrap_or(NumberFormatType::Digit)
                },
                text: self.paragraph_head.text.clone(),
                checkable: self.paragraph_head.checkable,
            },
        }
    }
}

// ---------------------------------------------------------------------------

/// Binary data storage type from a `BinData` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5BinDataType {
    /// External linked resource.
    Link,
    /// Embedded binary blob stored under `/BinData/*`.
    Embedding,
    /// Storage-backed binary payload.
    Storage,
    /// Unrecognized type value.
    Unknown(u8),
}

impl Hwp5BinDataType {
    fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Link,
            1 => Self::Embedding,
            2 => Self::Storage,
            other => Self::Unknown(other),
        }
    }

    /// Returns `true` when this type is expected to resolve to a `/BinData/*` stream.
    pub fn has_embedded_stream(self) -> bool {
        matches!(self, Self::Embedding | Self::Storage)
    }
}

/// Per-entry compression mode from a `BinData` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5BinDataCompression {
    /// Follow the document-wide `FileHeader.flags.compressed` setting.
    Default,
    /// Always raw-DEFLATE compressed.
    Compress,
    /// Never compressed.
    NotCompress,
    /// Unrecognized compression value.
    Unknown(u8),
}

impl Hwp5BinDataCompression {
    fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Default,
            1 => Self::Compress,
            2 => Self::NotCompress,
            other => Self::Unknown(other),
        }
    }

    /// Returns `true` when the payload bytes must be DEFLATE-decoded.
    pub fn should_decompress(self, file_is_compressed: bool) -> bool {
        match self {
            Self::Default => file_is_compressed,
            Self::Compress => true,
            Self::NotCompress | Self::Unknown(_) => false,
        }
    }
}

/// Parsed from a `BinData` record (TagId 0x12).
#[derive(Debug, Clone)]
pub struct Hwp5RawBinData {
    /// Raw property bitfield.
    pub property: u16,
    /// Storage kind.
    pub data_type: Hwp5BinDataType,
    /// Per-entry compression mode.
    pub compression: Hwp5BinDataCompression,
    /// Status bits from the property field.
    pub status: u8,
    /// 1-based binary item ID used by picture controls.
    pub binary_data_id: u16,
    /// File extension used in `/BinData/BINXXXX.ext`.
    pub extension: String,
}

impl Hwp5RawBinData {
    /// Parse a `BinData` record from its raw payload bytes.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < 4 {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("BinData too short: {} bytes (expected >= 4)", data.len()),
            });
        }

        let mut cur = Cursor::new(data);
        let property = cur.read_u16::<LittleEndian>()?;
        let binary_data_id = cur.read_u16::<LittleEndian>()?;
        let extension = read_utf16le_string(&mut cur)?;
        let data_type = Hwp5BinDataType::from_bits((property & 0x000F) as u8);
        let compression = Hwp5BinDataCompression::from_bits(((property >> 4) & 0x0003) as u8);
        let status = ((property >> 8) & 0x0003) as u8;

        Ok(Self { property, data_type, compression, status, binary_data_id, extension })
    }

    /// Returns the expected `/BinData/*` filename for this entry.
    pub fn storage_name(&self) -> String {
        if self.extension.is_empty() {
            format!("BIN{:04X}", self.binary_data_id)
        } else {
            format!("BIN{:04X}.{}", self.binary_data_id, self.extension)
        }
    }
}

// ---------------------------------------------------------------------------

/// Parsed from a `FaceName` record (TagId 0x13).
///
/// Contains the font property flags and the decoded UTF-16LE face name.
#[derive(Debug, Clone)]
pub struct Hwp5RawFaceName {
    /// Font property flags byte.
    pub property: u8,
    /// Font face name decoded from UTF-16LE.
    pub face_name: String,
    /// Alternate font type — present when `property & 0x80 != 0`.
    pub alternate_font_type: Option<u8>,
    /// Alternate font name — present when `property & 0x80 != 0`.
    pub alternate_font_name: Option<String>,
    /// PANOSE classification — present when `property & 0x40 != 0`.
    pub panose1: Option<[u8; 10]>,
    /// Default font name — present when `property & 0x20 != 0`.
    pub default_font_name: Option<String>,
}

impl Hwp5RawFaceName {
    /// Parse a `FaceName` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if the data is empty or the name
    /// cannot be decoded as UTF-16LE.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.is_empty() {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: "FaceName record is empty".into(),
            });
        }
        let mut cur = Cursor::new(data);
        let property = cur.read_u8().map_err(|e| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("failed to read FaceName property byte: {e}"),
        })?;
        let face_name = read_utf16le_string(&mut cur)?;

        let (alternate_font_type, alternate_font_name) = if property & 0x80 != 0 {
            let start = cur.position() as usize;
            let font_type = cur.read_u8().map_err(|e| Hwp5Error::RecordParse {
                offset: start,
                detail: format!("failed to read alternate font type: {e}"),
            })?;
            let font_name = read_utf16le_string(&mut cur)?;
            (Some(font_type), Some(font_name))
        } else {
            (None, None)
        };

        let panose1 = if property & 0x40 != 0 {
            let start = cur.position() as usize;
            let end = start + 10;
            if data.len() < end {
                return Err(Hwp5Error::RecordParse {
                    offset: start,
                    detail: "FaceName PANOSE block is truncated".into(),
                });
            }
            let mut bytes = [0u8; 10];
            bytes.copy_from_slice(&data[start..end]);
            cur.set_position(end as u64);
            Some(bytes)
        } else {
            None
        };

        let default_font_name =
            if property & 0x20 != 0 { Some(read_utf16le_string(&mut cur)?) } else { None };

        Ok(Self {
            property,
            face_name,
            alternate_font_type,
            alternate_font_name,
            panose1,
            default_font_name,
        })
    }
}

// ---------------------------------------------------------------------------

/// Parsed from a `CharShape` record (TagId 0x15).
///
/// Minimum size is 68 bytes; optional fields appear in later format versions.
#[derive(Debug, Clone)]
pub struct Hwp5RawCharShape {
    /// Font IDs for 7 language groups (hangul, english, hanja, japanese,
    /// other, symbol, user).
    pub font_ids: [u16; 7],
    /// Font width ratios (%) per language group.
    pub font_ratios: [u8; 7],
    /// Font character spacings per language group.
    pub font_spacings: [i8; 7],
    /// Font relative sizes (%) per language group.
    pub font_rel_sizes: [u8; 7],
    /// Font vertical offsets (%) per language group.
    pub font_offsets: [i8; 7],
    /// Character height in HwpUnit.
    pub height: i32,
    /// Property bitfield (bold = bit 0, italic = bit 1, underline type = bits
    /// 2–4, etc.).
    pub property: u32,
    /// Shadow gap X.
    pub shadow_gap_x: i8,
    /// Shadow gap Y.
    pub shadow_gap_y: i8,
    /// Text color (COLORREF, BGR byte order).
    pub text_color: u32,
    /// Underline color.
    pub underline_color: u32,
    /// Shade color.
    pub shade_color: u32,
    /// Shadow color.
    pub shadow_color: u32,
    /// Border/fill ID — present only in v5.0.2.1+ (data ≥ 70 bytes).
    pub border_fill_id: Option<u16>,
    /// Strikethrough color — present only in v5.0.3.0+ (data ≥ 74 bytes).
    pub strike_color: Option<u32>,
}

impl Hwp5RawCharShape {
    /// Parse a `CharShape` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if the data is shorter than the
    /// minimum 68 bytes.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const MIN_SIZE: usize = 68;
        if data.len() < MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "CharShape too short: {} bytes (expected >= {})",
                    data.len(),
                    MIN_SIZE
                ),
            });
        }
        let mut cur = Cursor::new(data);

        let mut font_ids = [0u16; 7];
        for id in &mut font_ids {
            *id = cur.read_u16::<LittleEndian>()?;
        }
        let mut font_ratios = [0u8; 7];
        for r in &mut font_ratios {
            *r = cur.read_u8()?;
        }
        let mut font_spacings = [0i8; 7];
        for s in &mut font_spacings {
            *s = cur.read_i8()?;
        }
        let mut font_rel_sizes = [0u8; 7];
        for r in &mut font_rel_sizes {
            *r = cur.read_u8()?;
        }
        let mut font_offsets = [0i8; 7];
        for o in &mut font_offsets {
            *o = cur.read_i8()?;
        }
        // Offset now at 42
        let height = cur.read_i32::<LittleEndian>()?;
        let property = cur.read_u32::<LittleEndian>()?;
        let shadow_gap_x = cur.read_i8()?;
        let shadow_gap_y = cur.read_i8()?;
        let text_color = cur.read_u32::<LittleEndian>()?;
        let underline_color = cur.read_u32::<LittleEndian>()?;
        let shade_color = cur.read_u32::<LittleEndian>()?;
        let shadow_color = cur.read_u32::<LittleEndian>()?;
        // Offset now at 68

        let border_fill_id =
            if data.len() >= 70 { Some(cur.read_u16::<LittleEndian>()?) } else { None };
        let strike_color =
            if data.len() >= 74 { Some(cur.read_u32::<LittleEndian>()?) } else { None };

        Ok(Self {
            font_ids,
            font_ratios,
            font_spacings,
            font_rel_sizes,
            font_offsets,
            height,
            property,
            shadow_gap_x,
            shadow_gap_y,
            text_color,
            underline_color,
            shade_color,
            shadow_color,
            border_fill_id,
            strike_color,
        })
    }

    /// Returns `true` if the bold flag (bit 0) is set in `property`.
    pub fn is_bold(&self) -> bool {
        self.property & 1 != 0
    }

    /// Returns `true` if the italic flag (bit 1) is set in `property`.
    pub fn is_italic(&self) -> bool {
        self.property & 2 != 0
    }
}

// ---------------------------------------------------------------------------

/// Parsed from a `ParaShape` record (TagId 0x19).
///
/// Base size is 42 bytes; optional fields appear in later format versions.
#[derive(Debug, Clone)]
pub struct Hwp5RawParaShape {
    /// Property bitfield 1 (alignment in bits 2–4, etc.).
    pub property1: u32,
    /// Left margin in HwpUnit.
    pub left_margin: i32,
    /// Right margin in HwpUnit.
    pub right_margin: i32,
    /// First-line indent in HwpUnit.
    pub indent: i32,
    /// Space before paragraph in HwpUnit.
    pub space_before: i32,
    /// Space after paragraph in HwpUnit.
    pub space_after: i32,
    /// Line spacing value.
    pub line_spacing: i32,
    /// Tab definition ID.
    pub tab_def_id: u16,
    /// Numbering/bullet definition ID.
    pub numbering_bullet_id: u16,
    /// Border/fill definition ID.
    pub border_fill_id: u16,
    /// Left border offset.
    pub border_offset_left: i16,
    /// Right border offset.
    pub border_offset_right: i16,
    /// Top border offset.
    pub border_offset_top: i16,
    /// Bottom border offset.
    pub border_offset_bottom: i16,
    // Total base: 42 bytes
    /// Property bitfield 2 — present in v5.0.1.7+ (data ≥ 46 bytes).
    pub property2: Option<u32>,
    /// Property bitfield 3 — present in v5.0.2.5+ (data ≥ 50 bytes).
    pub property3: Option<u32>,
    /// Additional line spacing field — present in v5.0.2.5+ (data ≥ 54 bytes).
    pub line_spacing2: Option<u32>,
}

impl Hwp5RawParaShape {
    /// Returns the list family encoded in `property1` bits 23-24.
    ///
    /// This is the authoritative source of paragraph list semantics; the raw
    /// `numbering_bullet_id` is only the referenced definition slot.
    pub fn heading_kind(&self) -> HeadingType {
        match (self.property1 >> 23) & 0b11 {
            1 => HeadingType::Outline,
            2 => HeadingType::Number,
            3 => HeadingType::Bullet,
            _ => HeadingType::None,
        }
    }

    /// Returns the zero-based paragraph list level encoded in `property1`
    /// bits 25-27.
    ///
    /// HWP5 stores these bits as one-based values (`1..=7`) for outline,
    /// numbering, and bullet paragraphs. The shared IR and HWPX wire heading
    /// level are zero-based, so this helper normalizes the value.
    pub fn heading_level(&self) -> u8 {
        (((self.property1 >> 25) & 0b111) as u8).saturating_sub(1)
    }

    /// Returns the raw HWP5 list-definition slot index.
    pub fn list_ref_id(&self) -> u32 {
        u32::from(self.numbering_bullet_id)
    }

    /// Parse a `ParaShape` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if the data is shorter than the
    /// minimum 42 bytes.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const MIN_SIZE: usize = 42;
        if data.len() < MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ParaShape too short: {} bytes (expected >= {})",
                    data.len(),
                    MIN_SIZE
                ),
            });
        }
        let mut cur = Cursor::new(data);
        let property1 = cur.read_u32::<LittleEndian>()?;
        let left_margin = cur.read_i32::<LittleEndian>()?;
        let right_margin = cur.read_i32::<LittleEndian>()?;
        let indent = cur.read_i32::<LittleEndian>()?;
        let space_before = cur.read_i32::<LittleEndian>()?;
        let space_after = cur.read_i32::<LittleEndian>()?;
        let line_spacing = cur.read_i32::<LittleEndian>()?;
        let tab_def_id = cur.read_u16::<LittleEndian>()?;
        let numbering_bullet_id = cur.read_u16::<LittleEndian>()?;
        let border_fill_id = cur.read_u16::<LittleEndian>()?;
        let border_offset_left = cur.read_i16::<LittleEndian>()?;
        let border_offset_right = cur.read_i16::<LittleEndian>()?;
        let border_offset_top = cur.read_i16::<LittleEndian>()?;
        let border_offset_bottom = cur.read_i16::<LittleEndian>()?;
        // Offset now at 42
        let property2 = if data.len() >= 46 { Some(cur.read_u32::<LittleEndian>()?) } else { None };
        let property3 = if data.len() >= 50 { Some(cur.read_u32::<LittleEndian>()?) } else { None };
        let line_spacing2 =
            if data.len() >= 54 { Some(cur.read_u32::<LittleEndian>()?) } else { None };

        Ok(Self {
            property1,
            left_margin,
            right_margin,
            indent,
            space_before,
            space_after,
            line_spacing,
            tab_def_id,
            numbering_bullet_id,
            border_fill_id,
            border_offset_left,
            border_offset_right,
            border_offset_top,
            border_offset_bottom,
            property2,
            property3,
            line_spacing2,
        })
    }
}

// ---------------------------------------------------------------------------

fn parse_numbering_para_head(
    cur: &mut Cursor<&[u8]>,
    level: u8,
    numbering: bool,
) -> Hwp5Result<Hwp5RawNumberingParaHead> {
    let attribute = cur.read_u32::<LittleEndian>()?;
    let _align_bits = (attribute & 0b11) as u8;
    let _use_instance_width = (attribute >> 2) & 1 != 0;
    let _auto_indent = (attribute >> 3) & 1 != 0;
    let _text_offset_kind = ((attribute >> 4) & 0b11) as u8;
    let _width_adjust = cur.read_i16::<LittleEndian>()?;
    let _text_offset = cur.read_i16::<LittleEndian>()?;
    let _char_shape_id = cur.read_u32::<LittleEndian>()?;
    let (num_format, text) = if numbering {
        (numbering_attr_num_format(attribute).to_string(), read_utf16le_string(cur)?)
    } else {
        (String::new(), String::new())
    };

    // HWP5 numbering paragraphs do not expose a stable checkable bit in the
    // raw record layout we currently preserve here, so keep the semantic slot
    // but mark it false instead of guessing.
    Ok(Hwp5RawNumberingParaHead { start_number: 1, level, num_format, text, checkable: false })
}

fn numbering_attr_num_format(attribute: u32) -> &'static str {
    match (attribute >> 5) & 0x1F {
        0 => "DIGIT",
        1 => "CIRCLED_DIGIT",
        2 => "ROMAN_CAPITAL",
        3 => "ROMAN_SMALL",
        4 => "LATIN_CAPITAL",
        5 => "LATIN_SMALL",
        7 => "CIRCLED_LATIN_SMALL",
        8 => "HANGUL_SYLLABLE",
        9 => "CIRCLED_HANGUL_SYLLABLE",
        10 => "HANGUL_JAMO",
        11 => "HANJA_DIGIT",
        _ => "DIGIT",
    }
}

fn decode_utf16_code_unit(code_unit: u16) -> String {
    char::from_u32(u32::from(code_unit)).unwrap_or('\u{25CF}').to_string()
}

/// A single explicit HWP5 tab stop inside a `TabDef` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawTabStop {
    /// Stop position in HWPUNIT.
    pub position: u32,
    /// Alignment code (0=left, 1=right, 2=center, 3=decimal).
    pub tab_type: u8,
    /// Leader/fill code.
    pub fill_type: u8,
}

/// Parsed from a `TabDef` record (TagId 0x16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawTabDef {
    /// Raw property bitfield.
    pub property: u32,
    /// Explicit tab stops in record order.
    pub tab_stops: Vec<Hwp5RawTabStop>,
}

impl Hwp5RawTabDef {
    /// Parse a `TabDef` record from its raw payload bytes.
    ///
    /// Layout:
    /// - `u32 property`
    /// - `i32 tab_count`
    /// - repeated tab entries: `u32 position`, `u8 tab_type`, `u8 fill_type`, `u16 reserved`
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const HEADER_SIZE: usize = 8;
        const TAB_STOP_SIZE: usize = 8;

        if data.len() < HEADER_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "TabDef too short: {} bytes (expected >= {HEADER_SIZE})",
                    data.len()
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let property = cur.read_u32::<LittleEndian>()?;
        let tab_count = cur.read_i32::<LittleEndian>()?;
        if tab_count < 0 {
            return Err(Hwp5Error::RecordParse {
                offset: 4,
                detail: format!("TabDef has negative tab_count: {tab_count}"),
            });
        }

        let tab_count = tab_count as usize;
        let expected_len = HEADER_SIZE + (tab_count * TAB_STOP_SIZE);
        if data.len() < expected_len {
            return Err(Hwp5Error::RecordParse {
                offset: HEADER_SIZE,
                detail: format!(
                    "TabDef truncated: {} bytes (need {expected_len} for {tab_count} tab stops)",
                    data.len()
                ),
            });
        }

        let mut tab_stops = Vec::with_capacity(tab_count);
        for _ in 0..tab_count {
            let position = cur.read_u32::<LittleEndian>()?;
            let tab_type = cur.read_u8()?;
            let fill_type = cur.read_u8()?;
            let _reserved = cur.read_u16::<LittleEndian>()?;
            tab_stops.push(Hwp5RawTabStop { position, tab_type, fill_type });
        }

        if data.len() != expected_len {
            return Err(Hwp5Error::RecordParse {
                offset: expected_len,
                detail: format!(
                    "TabDef has {} unexpected trailing bytes after {tab_count} tab stops",
                    data.len() - expected_len
                ),
            });
        }

        Ok(Self { property, tab_stops })
    }

    /// Whether this definition enables auto tab at paragraph left end.
    pub fn auto_tab_left(&self) -> bool {
        (self.property & 0b1) != 0
    }

    /// Whether this definition enables auto tab at paragraph right end.
    pub fn auto_tab_right(&self) -> bool {
        (self.property & 0b10) != 0
    }
}

/// A `TabDef` slot preserved from DocInfo record order.
///
/// `raw_id` matches the original HWP5 tab definition id used by paragraph
/// shapes. `tab_def = None` means the slot existed in DocInfo but could not be
/// parsed, so callers must preserve the index without trusting the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5TabDefSlot {
    /// Raw 0-based tab definition id from DocInfo order.
    pub raw_id: u32,
    /// Parsed definition payload, if decoding succeeded.
    pub tab_def: Option<Hwp5RawTabDef>,
}

impl Hwp5TabDefSlot {
    /// Construct a successfully parsed tab definition slot.
    pub fn parsed(raw_id: u32, tab_def: Hwp5RawTabDef) -> Self {
        Self { raw_id, tab_def: Some(tab_def) }
    }

    /// Construct a placeholder slot for a tab definition that failed to parse.
    pub fn invalid(raw_id: u32) -> Self {
        Self { raw_id, tab_def: None }
    }
}

// ---------------------------------------------------------------------------

/// Reads a length-prefixed UTF-16LE string from a `Cursor<&[u8]>`.
///
/// Format: `u16` length (in UTF-16 code units) followed by that many u16 LE
/// values.
fn read_utf16le_string(cur: &mut Cursor<&[u8]>) -> Hwp5Result<String> {
    let start = cur.position() as usize;
    if cur.get_ref().len() < start + 2 {
        return Err(Hwp5Error::RecordParse {
            offset: start,
            detail: "missing UTF-16 string length prefix".into(),
        });
    }

    let len = LittleEndian::read_u16(&cur.get_ref()[start..start + 2]) as usize;
    let data_start = start + 2;
    let data_end = data_start + len.saturating_mul(2);
    if cur.get_ref().len() < data_end {
        return Err(Hwp5Error::RecordParse {
            offset: start,
            detail: format!("truncated UTF-16 string payload: need {} bytes", len * 2),
        });
    }

    let u16s: Vec<u16> = cur.get_ref()[data_start..data_end]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    cur.set_position(data_end as u64);
    String::from_utf16(&u16s).map_err(|_| Hwp5Error::RecordParse {
        offset: cur.position() as usize,
        detail: "invalid UTF-16LE string in record".into(),
    })
}

/// Parsed from a `Style` record (TagId 0x1A).
#[derive(Debug, Clone)]
pub struct Hwp5RawStyle {
    /// Style name in Korean (UTF-16LE).
    pub name: String,
    /// Style name in English (UTF-16LE).
    pub english_name: String,
    /// Style kind: 0 = paragraph style, 1 = character style.
    pub kind: u8,
    /// ID of the next (following) style.
    pub next_style_id: u8,
    /// Language ID (e.g. 0x0412 = Korean).
    pub lang_id: i16,
    /// Paragraph shape definition ID.
    pub para_shape_id: u16,
    /// Character shape definition ID.
    pub char_shape_id: u16,
    /// Whether the style is form-locked.
    pub lock_form: u16,
}

impl Hwp5RawStyle {
    /// Parse a `Style` record from its raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] on truncated or invalid data.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        let mut cur = Cursor::new(data);
        let name = read_utf16le_string(&mut cur)?;
        let english_name = read_utf16le_string(&mut cur)?;
        let kind = cur.read_u8()?;
        let next_style_id = cur.read_u8()?;
        let lang_id = cur.read_i16::<LittleEndian>()?;
        let para_shape_id = cur.read_u16::<LittleEndian>()?;
        let char_shape_id = cur.read_u16::<LittleEndian>()?;
        let lock_form = cur.read_u16::<LittleEndian>()?;
        Ok(Self {
            name,
            english_name,
            kind,
            next_style_id,
            lang_id,
            para_shape_id,
            char_shape_id,
            lock_form,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file_header_bytes(version: u32, flags: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 256];
        let sig = b"HWP Document File";
        buf[..sig.len()].copy_from_slice(sig);
        buf[32..36].copy_from_slice(&version.to_le_bytes());
        buf[36..40].copy_from_slice(&flags.to_le_bytes());
        buf
    }

    fn make_version(major: u8, minor: u8, build: u8, rev: u8) -> u32 {
        (major as u32) << 24 | (minor as u32) << 16 | (build as u32) << 8 | rev as u32
    }

    #[test]
    fn parse_valid_header() {
        let buf = make_file_header_bytes(make_version(5, 0, 2, 5), 0x01);
        let header = FileHeader::parse(&buf).unwrap();
        assert_eq!(header.version, HwpVersion::new(5, 0, 2, 5));
        assert!(header.flags.compressed);
        assert!(!header.flags.encrypted);
    }

    #[test]
    fn parse_uncompressed_header() {
        let buf = make_file_header_bytes(make_version(5, 0, 3, 0), 0x00);
        let header = FileHeader::parse(&buf).unwrap();
        assert!(!header.flags.compressed);
    }

    #[test]
    fn reject_invalid_signature() {
        let buf = vec![0u8; 256];
        let err = FileHeader::parse(&buf).unwrap_err();
        assert!(matches!(err, Hwp5Error::NotHwp5 { .. }));
    }

    #[test]
    fn reject_too_short() {
        let buf = vec![0u8; 100];
        let err = FileHeader::parse(&buf).unwrap_err();
        assert!(matches!(err, Hwp5Error::NotHwp5 { .. }));
    }

    #[test]
    fn reject_encrypted_document() {
        let buf = make_file_header_bytes(make_version(5, 0, 2, 5), 0x02);
        let err = FileHeader::parse(&buf).unwrap_err();
        assert!(matches!(err, Hwp5Error::PasswordProtected));
    }

    #[test]
    fn reject_unsupported_version() {
        let buf = make_file_header_bytes(make_version(4, 0, 0, 0), 0x01);
        let err = FileHeader::parse(&buf).unwrap_err();
        assert!(matches!(err, Hwp5Error::UnsupportedVersion { .. }));
    }

    #[test]
    fn version_ordering() {
        let v1 = HwpVersion::new(5, 0, 2, 5);
        let v2 = HwpVersion::new(5, 0, 3, 0);
        let v3 = HwpVersion::new(5, 1, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
        assert_eq!(v1, HwpVersion::new(5, 0, 2, 5));
    }

    #[test]
    fn version_display() {
        let v = HwpVersion::new(5, 0, 2, 5);
        assert_eq!(v.to_string(), "5.0.2.5");
    }

    #[test]
    fn flags_all_bits() {
        let buf = make_file_header_bytes(make_version(5, 0, 2, 5), 0xFFF & !0x02);
        let header = FileHeader::parse(&buf).unwrap();
        assert!(header.flags.compressed);
        assert!(!header.flags.encrypted); // we cleared bit 1
        assert!(header.flags.distributed);
        assert!(header.flags.script);
        assert!(header.flags.drm);
        assert!(header.flags.xml_template);
        assert!(header.flags.history);
        assert!(header.flags.signed);
    }

    #[test]
    fn min_supported_version() {
        assert_eq!(HwpVersion::MIN_SUPPORTED, HwpVersion::new(5, 0, 0, 0));
    }

    // -----------------------------------------------------------------------
    // DocInfo record type tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_id_mappings_basic() {
        // 60 bytes = 15 × i32
        let mut data = vec![0u8; 60];
        // hangul_font_count at offset 4
        data[4..8].copy_from_slice(&3i32.to_le_bytes());
        // char_shape_count at offset 36
        data[36..40].copy_from_slice(&5i32.to_le_bytes());
        // para_shape_count at offset 52
        data[52..56].copy_from_slice(&2i32.to_le_bytes());
        let mappings = Hwp5RawIdMappings::parse(&data).unwrap();
        assert_eq!(mappings.hangul_font_count, 3);
        assert_eq!(mappings.char_shape_count, 5);
        assert_eq!(mappings.para_shape_count, 2);
        assert!(mappings.memo_shape_count.is_none());
        assert!(mappings.change_tracking_count.is_none());
    }

    #[test]
    fn parse_id_mappings_with_memo() {
        // 72 bytes = 18 × i32 (includes memo + track-change counts)
        let mut data = vec![0u8; 72];
        data[60..64].copy_from_slice(&7i32.to_le_bytes());
        data[64..68].copy_from_slice(&8i32.to_le_bytes());
        data[68..72].copy_from_slice(&9i32.to_le_bytes());
        let mappings = Hwp5RawIdMappings::parse(&data).unwrap();
        assert_eq!(mappings.memo_shape_count, Some(7));
        assert_eq!(mappings.change_tracking_count, Some(8));
        assert_eq!(mappings.change_tracking_author_count, Some(9));
    }

    #[test]
    fn parse_id_mappings_too_short() {
        let data = vec![0u8; 10];
        assert!(matches!(
            Hwp5RawIdMappings::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn parse_face_name() {
        // property=0, then length-prefixed "바탕" in UTF-16LE.
        let mut data = vec![0u8; 1]; // property byte
        let name_utf16: Vec<u16> = "바탕".encode_utf16().collect();
        data.extend_from_slice(&(name_utf16.len() as u16).to_le_bytes());
        for &ch in &name_utf16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let face = Hwp5RawFaceName::parse(&data).unwrap();
        assert_eq!(face.face_name, "바탕");
        assert_eq!(face.property, 0);
        assert!(face.alternate_font_name.is_none());
    }

    #[test]
    fn parse_face_name_with_optional_tails() {
        let mut data = vec![0xE0u8]; // alternate + panose + default
        let base_utf16: Vec<u16> = "돋움체".encode_utf16().collect();
        data.extend_from_slice(&(base_utf16.len() as u16).to_le_bytes());
        for &ch in &base_utf16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }

        data.push(1); // alternate font type
        let alt_utf16: Vec<u16> = "DotumChe".encode_utf16().collect();
        data.extend_from_slice(&(alt_utf16.len() as u16).to_le_bytes());
        for &ch in &alt_utf16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }

        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]); // panose

        let default_utf16: Vec<u16> = "돋움".encode_utf16().collect();
        data.extend_from_slice(&(default_utf16.len() as u16).to_le_bytes());
        for &ch in &default_utf16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }

        let face = Hwp5RawFaceName::parse(&data).unwrap();
        assert_eq!(face.face_name, "돋움체");
        assert_eq!(face.alternate_font_type, Some(1));
        assert_eq!(face.alternate_font_name.as_deref(), Some("DotumChe"));
        assert_eq!(face.panose1, Some([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
        assert_eq!(face.default_font_name.as_deref(), Some("돋움"));
    }

    #[test]
    fn parse_face_name_empty_data() {
        assert!(matches!(Hwp5RawFaceName::parse(&[]).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn parse_bin_data_embedding_record() {
        let mut data = Vec::new();
        let property = 0x0011u16; // embedding + compress
        data.extend_from_slice(&property.to_le_bytes());
        data.extend_from_slice(&5u16.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        for ch in "png".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }

        let bin = Hwp5RawBinData::parse(&data).unwrap();
        assert_eq!(bin.data_type, Hwp5BinDataType::Embedding);
        assert_eq!(bin.compression, Hwp5BinDataCompression::Compress);
        assert_eq!(bin.binary_data_id, 5);
        assert_eq!(bin.extension, "png");
        assert_eq!(bin.storage_name(), "BIN0005.png");
    }

    #[test]
    fn parse_bin_data_default_compression_record() {
        let mut data = Vec::new();
        let property = 0x0101u16; // embedding + default compression + status 1
        data.extend_from_slice(&property.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        for ch in "jpg".encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }

        let bin = Hwp5RawBinData::parse(&data).unwrap();
        assert_eq!(bin.compression, Hwp5BinDataCompression::Default);
        assert_eq!(bin.status, 1);
        assert!(bin.data_type.has_embedded_stream());
        assert!(bin.compression.should_decompress(true));
        assert!(!bin.compression.should_decompress(false));
    }

    #[test]
    fn parse_char_shape_base() {
        // 68 bytes minimum
        let mut data = vec![0u8; 68];
        // height at offset 42 = 1200 (12pt)
        data[42..46].copy_from_slice(&1200i32.to_le_bytes());
        // property at offset 46: bold (bit 0)
        data[46..50].copy_from_slice(&1u32.to_le_bytes());
        // text_color at offset 50 = 0x000000 (black)
        data[50..54].copy_from_slice(&0x000000u32.to_le_bytes());
        let cs = Hwp5RawCharShape::parse(&data).unwrap();
        assert_eq!(cs.height, 1200);
        assert!(cs.is_bold());
        assert!(!cs.is_italic());
        assert!(cs.border_fill_id.is_none());
        assert!(cs.strike_color.is_none());
    }

    #[test]
    fn parse_char_shape_with_extensions() {
        // 74 bytes (full)
        let mut data = vec![0u8; 74];
        data[42..46].copy_from_slice(&1000i32.to_le_bytes());
        // border_fill_id at byte 68
        data[68..70].copy_from_slice(&5u16.to_le_bytes());
        // strike_color at byte 70
        data[70..74].copy_from_slice(&0xFF_0000u32.to_le_bytes());
        let cs = Hwp5RawCharShape::parse(&data).unwrap();
        assert_eq!(cs.border_fill_id, Some(5));
        assert_eq!(cs.strike_color, Some(0xFF_0000));
    }

    #[test]
    fn parse_char_shape_too_short() {
        let data = vec![0u8; 40];
        assert!(matches!(
            Hwp5RawCharShape::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn parse_para_shape_base() {
        let mut data = vec![0u8; 42];
        // property1: alignment=center (1 << 2)
        data[0..4].copy_from_slice(&(1u32 << 2).to_le_bytes());
        // left_margin = 500
        data[4..8].copy_from_slice(&500i32.to_le_bytes());
        let ps = Hwp5RawParaShape::parse(&data).unwrap();
        assert_eq!(ps.property1 >> 2 & 0x7, 1); // center
        assert_eq!(ps.left_margin, 500);
        assert!(ps.property2.is_none());
    }

    #[test]
    fn parse_para_shape_extended() {
        let mut data = vec![0u8; 54];
        // property2 at byte 42
        data[42..46].copy_from_slice(&0xABu32.to_le_bytes());
        // property3 at byte 46
        data[46..50].copy_from_slice(&0xCDu32.to_le_bytes());
        // line_spacing2 at byte 50
        data[50..54].copy_from_slice(&160u32.to_le_bytes());
        let ps = Hwp5RawParaShape::parse(&data).unwrap();
        assert_eq!(ps.property2, Some(0xAB));
        assert_eq!(ps.property3, Some(0xCD));
        assert_eq!(ps.line_spacing2, Some(160));
    }

    #[test]
    fn parse_para_shape_too_short() {
        let data = vec![0u8; 20];
        assert!(matches!(
            Hwp5RawParaShape::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn parse_tab_def_with_two_stops() {
        let mut data = Vec::new();
        data.extend_from_slice(&0b11u32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&4000u32.to_le_bytes());
        data.push(0);
        data.push(3);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&8000u32.to_le_bytes());
        data.push(3);
        data.push(0);
        data.extend_from_slice(&0u16.to_le_bytes());

        let tab_def = Hwp5RawTabDef::parse(&data).unwrap();
        assert!(tab_def.auto_tab_left());
        assert!(tab_def.auto_tab_right());
        assert_eq!(tab_def.tab_stops.len(), 2);
        assert_eq!(tab_def.tab_stops[0].position, 4000);
        assert_eq!(tab_def.tab_stops[0].tab_type, 0);
        assert_eq!(tab_def.tab_stops[0].fill_type, 3);
        assert_eq!(tab_def.tab_stops[1].position, 8000);
        assert_eq!(tab_def.tab_stops[1].tab_type, 3);
        assert_eq!(tab_def.tab_stops[1].fill_type, 0);
    }

    #[test]
    fn reject_tab_def_with_negative_count() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(-1i32).to_le_bytes());

        let err = Hwp5RawTabDef::parse(&data).unwrap_err();
        assert!(matches!(err, Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn reject_tab_def_with_trailing_bytes() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&4000u32.to_le_bytes());
        data.push(0);
        data.push(3);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0xAA);

        let err = Hwp5RawTabDef::parse(&data).unwrap_err();
        assert!(matches!(err, Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn parse_style() {
        let mut data = Vec::new();
        let name = "본문";
        let name_u16: Vec<u16> = name.encode_utf16().collect();
        data.extend_from_slice(&(name_u16.len() as u16).to_le_bytes());
        for &ch in &name_u16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let eng = "Body";
        let eng_u16: Vec<u16> = eng.encode_utf16().collect();
        data.extend_from_slice(&(eng_u16.len() as u16).to_le_bytes());
        for &ch in &eng_u16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        data.push(0); // kind = paragraph
        data.push(1); // next_style_id
        data.extend_from_slice(&0x0412i16.to_le_bytes()); // lang_id (Korean)
        data.extend_from_slice(&0u16.to_le_bytes()); // para_shape_id
        data.extend_from_slice(&0u16.to_le_bytes()); // char_shape_id
        data.extend_from_slice(&1u16.to_le_bytes()); // lock_form
        let style = Hwp5RawStyle::parse(&data).unwrap();
        assert_eq!(style.name, "본문");
        assert_eq!(style.english_name, "Body");
        assert_eq!(style.kind, 0);
        assert_eq!(style.next_style_id, 1);
        assert_eq!(style.lang_id, 0x0412);
        assert_eq!(style.para_shape_id, 0);
        assert_eq!(style.char_shape_id, 0);
        assert_eq!(style.lock_form, 1);
    }
}
