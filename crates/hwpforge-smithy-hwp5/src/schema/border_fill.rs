//! HWP5 `BorderFill` DocInfo record schema.
//!
//! HWP5 stores table/cell border and fill definitions in `DocInfo`
//! `BorderFill` records. The section/table records reference these definitions
//! via 1-based `border_fill_id`s, so losing the concrete definitions makes
//! emitted HWPX visually wrong even when ids still match.

use std::io::{Cursor, Read};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{Hwp5Error, Hwp5Result};

/// Parsed from a single 6-byte border line slot inside a `BorderFill` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hwp5RawBorderLine {
    /// Raw border kind code.
    pub kind: Hwp5BorderLineKind,
    /// Raw border width code.
    pub width: u8,
    /// Raw HWP `COLORREF` value (`0xAABBGGRR` / `0x00BBGGRR`).
    pub color: u32,
}

impl Hwp5RawBorderLine {
    fn parse(cur: &mut Cursor<&[u8]>) -> Hwp5Result<Self> {
        Ok(Self {
            kind: Hwp5BorderLineKind::from_raw(cur.read_u8()?),
            width: cur.read_u8()?,
            color: cur.read_u32::<LittleEndian>()?,
        })
    }
}

/// Raw HWP5 border line kind code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5BorderLineKind {
    /// No line.
    None,
    /// Solid line.
    Solid,
    /// Dashed line.
    Dash,
    /// Dotted line.
    Dot,
    /// Dash-dot line.
    DashDot,
    /// Dash-dot-dot line.
    DashDotDot,
    /// Long dash line.
    LongDash,
    /// Circle-dot line.
    Circle,
    /// Double slim line.
    DoubleSlim,
    /// Slim-thick double line.
    SlimThick,
    /// Thick-slim double line.
    ThickSlim,
    /// Slim-thick-slim triple line.
    SlimThickSlim,
    /// Wave line.
    Wave,
    /// Double wave line.
    DoubleWave,
    /// Thick 3D line.
    Thick3d,
    /// Thick 3D reverse-lighting line.
    Thick3dReverseLighting,
    /// Solid 3D line.
    Solid3d,
    /// Solid 3D reverse-lighting line.
    Solid3dReverseLighting,
    /// Unknown raw code.
    Unknown(u8),
}

impl Hwp5BorderLineKind {
    /// Maps the raw HWP5 border-line code to a kind.
    ///
    /// The codes follow 한글's real encoding, which differs from spec table 25
    /// (the spec omits the `0 = 없음` slot and lists `점선`/`파선` in a
    /// different order). Verified empirically against 한글 fixtures:
    /// `0 → 없음`, `1 → 실선`, `2 → 점선(Dot)`, `8 → 이중선(DoubleSlim)`.
    /// In particular codes 2 and 3 are `Dot`/`Dash` (not `Dash`/`Dot`).
    fn from_raw(raw: u8) -> Self {
        match raw {
            // 0/1/2 and 8 are fixture-confirmed (None/Solid/Dot/DoubleSlim).
            0 => Self::None,
            1 => Self::Solid,
            2 => Self::Dot,
            // 3..=7 are inferred from the contiguous run between the confirmed
            // endpoints 2 (Dot) and 8 (DoubleSlim); their relative order is not
            // yet fixture-verified.
            3 => Self::Dash,
            4 => Self::DashDot,
            5 => Self::DashDotDot,
            6 => Self::LongDash,
            7 => Self::Circle,
            8 => Self::DoubleSlim,
            9 => Self::SlimThick,
            10 => Self::ThickSlim,
            11 => Self::SlimThickSlim,
            12 => Self::Wave,
            13 => Self::DoubleWave,
            14 => Self::Thick3d,
            15 => Self::Thick3dReverseLighting,
            16 => Self::Solid3d,
            17 => Self::Solid3dReverseLighting,
            value => Self::Unknown(value),
        }
    }
}

/// Fill payload parsed from an HWP5 `BorderFill`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hwp5RawBorderFillFill {
    /// No fill.
    None,
    /// Solid/pattern color fill.
    Color(Hwp5RawColorFill),
    /// Gradation fill.
    Gradation(Hwp5RawGradationFill),
    /// Image fill.
    Image(Hwp5RawImageFill),
    /// Unknown fill kind.
    Unknown {
        /// Raw fill kind value.
        kind: u32,
        /// Unparsed payload bytes after the fill-kind tag.
        raw_data: Vec<u8>,
    },
}

/// Parsed solid/pattern color fill payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawColorFill {
    /// Background color.
    pub background_color: u32,
    /// Pattern color.
    pub pattern_color: u32,
    /// Raw pattern kind.
    pub pattern_kind: Hwp5FillPatternKind,
    /// Alpha transparency.
    pub alpha: u8,
    /// Extra bytes declared by the record.
    pub extra_data: Vec<u8>,
}

/// Parsed image fill payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawImageFill {
    /// Raw image fill mode.
    pub mode: Hwp5FillImageMode,
    /// Brightness adjustment.
    pub brightness: i8,
    /// Contrast adjustment.
    pub contrast: i8,
    /// Image effect.
    pub effect: Hwp5FillImageEffect,
    /// Referenced `BinData` item ID.
    pub bindata_id: u16,
    /// Trailing bytes not yet modeled.
    pub extra_data: Vec<u8>,
}

/// Parsed gradation fill payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawGradationFill {
    /// Raw gradation type.
    pub gradation_type: Hwp5GradationType,
    /// Angle/shear in degrees.
    pub angle: u32,
    /// Center X percentage.
    pub center_x: i32,
    /// Center Y percentage.
    pub center_y: i32,
    /// Blur percentage (0-100).
    pub blur: u32,
    /// Ordered color stops.
    pub colors: Vec<u32>,
    /// Extra shape value emitted after gradation payload in some files.
    pub shape: Option<u32>,
    /// Extra blur-center byte emitted after gradation payload in some files.
    pub blur_center: Option<u8>,
    /// Trailing bytes not yet modeled.
    pub extra_data: Vec<u8>,
}

/// Raw HWP5 fill pattern kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5FillPatternKind {
    /// No hatch pattern.
    None,
    /// Horizontal hatch.
    Horizontal,
    /// Vertical hatch.
    Vertical,
    /// Back-slash hatch.
    BackSlash,
    /// Slash hatch.
    Slash,
    /// Cross hatch.
    Cross,
    /// Diagonal cross hatch.
    CrossDiagonal,
    /// Unknown raw kind.
    Unknown(i32),
}

impl Hwp5FillPatternKind {
    /// The Core hatch for this kind; `None` for no hatch and an unknown kind.
    /// Kind 2 is Hancom's `SLASH` (x6 F1) — [`PatternType`]'s display swaps
    /// the two slashes the same way.
    ///
    /// [`PatternType`]: hwpforge_foundation::PatternType
    pub(crate) fn pattern_type(self) -> Option<hwpforge_foundation::PatternType> {
        use hwpforge_foundation::PatternType;
        match self {
            Self::None | Self::Unknown(_) => None,
            Self::Horizontal => Some(PatternType::Horizontal),
            Self::Vertical => Some(PatternType::Vertical),
            Self::BackSlash => Some(PatternType::BackSlash),
            Self::Slash => Some(PatternType::Slash),
            Self::Cross => Some(PatternType::Cross),
            Self::CrossDiagonal => Some(PatternType::CrossDiagonal),
        }
    }

    fn from_raw(raw: i32) -> Self {
        match raw {
            -1 => Self::None,
            0 => Self::Horizontal,
            1 => Self::Vertical,
            2 => Self::BackSlash,
            3 => Self::Slash,
            4 => Self::Cross,
            5 => Self::CrossDiagonal,
            value => Self::Unknown(value),
        }
    }
}

/// Raw HWP5 image fill mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5FillImageMode {
    /// Tile image across the whole area.
    TileAll,
    /// Tile image horizontally at the top.
    TileHorizontalTop,
    /// Tile image horizontally at the bottom.
    TileHorizontalBottom,
    /// Tile image vertically on the left.
    TileVerticalLeft,
    /// Tile image vertically on the right.
    TileVerticalRight,
    /// Resize image to fit.
    Resize,
    /// Center image.
    Center,
    /// Center image at top.
    CenterTop,
    /// Center image at bottom.
    CenterBottom,
    /// Left middle.
    LeftMiddle,
    /// Left top.
    LeftTop,
    /// Left bottom.
    LeftBottom,
    /// Right middle.
    RightMiddle,
    /// Right top.
    RightTop,
    /// Right bottom.
    RightBottom,
    /// Scale image proportionally (companion HWPX `ZOOM`).
    Zoom,
    /// Unknown raw value.
    Unknown(u8),
}

impl Hwp5FillImageMode {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::TileAll,
            1 => Self::TileHorizontalTop,
            2 => Self::TileHorizontalBottom,
            3 => Self::TileVerticalLeft,
            4 => Self::TileVerticalRight,
            5 => Self::Resize,
            6 => Self::Center,
            7 => Self::CenterTop,
            8 => Self::CenterBottom,
            9 => Self::LeftMiddle,
            10 => Self::LeftTop,
            11 => Self::LeftBottom,
            12 => Self::RightMiddle,
            13 => Self::RightTop,
            14 => Self::RightBottom,
            15 => Self::Zoom,
            value => Self::Unknown(value),
        }
    }
}

/// Raw HWP5 image fill effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5FillImageEffect {
    /// Normal image.
    RealPic,
    /// Grayscale effect.
    GrayScale,
    /// Black/white effect.
    BlackWhite,
    /// 8x8 pattern effect.
    Pattern8x8,
    /// Unknown raw value.
    Unknown(u8),
}

impl Hwp5FillImageEffect {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::RealPic,
            1 => Self::GrayScale,
            2 => Self::BlackWhite,
            3 => Self::Pattern8x8,
            value => Self::Unknown(value),
        }
    }
}

/// Raw HWP5 gradation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hwp5GradationType {
    /// Linear gradation.
    Linear,
    /// Circular/radial gradation.
    Circular,
    /// Conical gradation.
    Conical,
    /// Rectangular/square gradation.
    Rectangular,
    /// Unknown raw value.
    Unknown(u8),
}

impl Hwp5GradationType {
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Linear,
            2 => Self::Circular,
            3 => Self::Conical,
            4 => Self::Rectangular,
            value => Self::Unknown(value),
        }
    }
}

/// Parsed from a `BorderFill` record (TagId 0x14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5RawBorderFill {
    /// Raw property flags.
    pub property: u16,
    /// 3D effect flag.
    pub three_d: bool,
    /// Shadow effect flag.
    pub shadow: bool,
    /// Raw slash diagonal shape bits.
    pub slash_diagonal_shape: u8,
    /// Raw back-slash diagonal shape bits.
    pub back_slash_diagonal_shape: u8,
    /// Whether a center line is enabled.
    pub center_line: bool,
    /// Left border.
    pub left: Hwp5RawBorderLine,
    /// Right border.
    pub right: Hwp5RawBorderLine,
    /// Top border.
    pub top: Hwp5RawBorderLine,
    /// Bottom border.
    pub bottom: Hwp5RawBorderLine,
    /// Diagonal border.
    pub diagonal: Hwp5RawBorderLine,
    /// Parsed fill payload.
    pub fill: Hwp5RawBorderFillFill,
}

impl Hwp5RawBorderFill {
    /// Parse a `BorderFill` record from raw payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] on truncated data.
    pub fn parse(data: &[u8]) -> Hwp5Result<Self> {
        const FIXED_PREFIX_SIZE: usize = 36; // property + 5 border lines + fill kind
        if data.len() < FIXED_PREFIX_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "BorderFill too short: {} bytes (expected >= {})",
                    data.len(),
                    FIXED_PREFIX_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let property = cur.read_u16::<LittleEndian>()?;
        let left = Hwp5RawBorderLine::parse(&mut cur)?;
        let right = Hwp5RawBorderLine::parse(&mut cur)?;
        let top = Hwp5RawBorderLine::parse(&mut cur)?;
        let bottom = Hwp5RawBorderLine::parse(&mut cur)?;
        let diagonal = Hwp5RawBorderLine::parse(&mut cur)?;
        let fill_kind = cur.read_u32::<LittleEndian>()?;
        let fill =
            parse_fill_payload(fill_kind, &mut cur).map_err(|err| Hwp5Error::RecordParse {
                offset: cur.position() as usize,
                detail: format!("BorderFill fill_kind=0x{fill_kind:08X}: {err}"),
            })?;

        Ok(Self {
            property,
            three_d: property & 0x0001 != 0,
            shadow: property & 0x0002 != 0,
            slash_diagonal_shape: ((property >> 2) & 0b111) as u8,
            back_slash_diagonal_shape: ((property >> 5) & 0b111) as u8,
            center_line: property & (1 << 13) != 0,
            left,
            right,
            top,
            bottom,
            diagonal,
            fill,
        })
    }
}

/// Bytes left after the cursor.
fn remaining(cur: &Cursor<&[u8]>) -> u64 {
    (cur.get_ref().len() as u64).saturating_sub(cur.position())
}

/// `count` items of `size` bytes, read from what is left — refused before
/// anything is allocated when the record cannot hold them (a damaged length
/// word would otherwise ask for gigabytes).
fn checked_len(cur: &Cursor<&[u8]>, count: u64, size: u64, what: &str) -> Hwp5Result<usize> {
    let bytes = count.saturating_mul(size);
    if bytes > remaining(cur) {
        return Err(Hwp5Error::RecordParse {
            offset: cur.position() as usize,
            detail: format!(
                "fill {what}: {count} items do not fit the {} bytes left",
                remaining(cur)
            ),
        });
    }
    Ok(count as usize)
}

/// Parses a fill block (kind word already read): `DocInfo` border fills and
/// shape components (after their line information) share this layout.
///
/// Every length word is checked against the bytes left before it sizes an
/// allocation. 이것을 실패시키는 것: 이 검사를 지우기
/// (`fill_lengths_beyond_the_record_are_refused_before_allocating`).
pub(crate) fn parse_fill_payload(
    fill_kind: u32,
    cur: &mut Cursor<&[u8]>,
) -> Hwp5Result<Hwp5RawBorderFillFill> {
    match fill_kind {
        0x0000_0000 => {
            let extra_len = cur.read_u32::<LittleEndian>()?;
            let extra_len = checked_len(cur, u64::from(extra_len), 1, "extra data")?;
            let mut ignored_extra_data = vec![0u8; extra_len];
            cur.read_exact(&mut ignored_extra_data)?;
            Ok(Hwp5RawBorderFillFill::None)
        }
        0x0000_0001 => {
            let background_color = cur.read_u32::<LittleEndian>()?;
            let pattern_color = cur.read_u32::<LittleEndian>()?;
            let pattern_kind = Hwp5FillPatternKind::from_raw(cur.read_i32::<LittleEndian>()?);
            // The alpha-less form ends with exactly one zero extra-length
            // word (DocInfo: 52 bytes; also written by shape components).
            // Do not infer a version cutoff or accept arbitrary EOF: a short
            // modern trailer with nonzero alpha/length must remain an error.
            let alpha = if cur.get_ref().get(cur.position() as usize..) == Some(&[0; 4][..]) {
                0
            } else {
                cur.read_u8()?
            };
            let extra_len = cur.read_u32::<LittleEndian>()?;
            let extra_len = checked_len(cur, u64::from(extra_len), 1, "extra data")?;
            let mut extra_data = vec![0u8; extra_len];
            cur.read_exact(&mut extra_data)?;
            Ok(Hwp5RawBorderFillFill::Color(Hwp5RawColorFill {
                background_color,
                pattern_color,
                pattern_kind,
                alpha,
                extra_data,
            }))
        }
        0x0000_0002 => Ok(Hwp5RawBorderFillFill::Image(parse_image_fill_payload(cur)?)),
        0x0000_0004 => Ok(Hwp5RawBorderFillFill::Gradation(parse_gradation_fill_payload(cur)?)),
        kind => {
            let mut raw_data = Vec::new();
            cur.read_to_end(&mut raw_data)?;
            Ok(Hwp5RawBorderFillFill::Unknown { kind, raw_data })
        }
    }
}

fn parse_image_fill_payload(cur: &mut Cursor<&[u8]>) -> Hwp5Result<Hwp5RawImageFill> {
    if (cur.get_ref().len() as u64).saturating_sub(cur.position()) < 6 {
        return Err(Hwp5Error::RecordParse {
            offset: cur.position() as usize,
            detail: "BorderFill image payload too short".to_string(),
        });
    }

    let mode = Hwp5FillImageMode::from_raw(cur.read_u8()?);
    let brightness = cur.read_i8()?;
    let contrast = cur.read_i8()?;
    let effect = Hwp5FillImageEffect::from_raw(cur.read_u8()?);
    let bindata_id = cur.read_u16::<LittleEndian>()?;
    let mut extra_data = Vec::new();
    cur.read_to_end(&mut extra_data)?;

    Ok(Hwp5RawImageFill { mode, brightness, contrast, effect, bindata_id, extra_data })
}

fn parse_gradation_fill_payload(cur: &mut Cursor<&[u8]>) -> Hwp5Result<Hwp5RawGradationFill> {
    if (cur.get_ref().len() as u64).saturating_sub(cur.position()) < 21 {
        return Err(Hwp5Error::RecordParse {
            offset: cur.position() as usize,
            detail: "BorderFill gradation payload too short".to_string(),
        });
    }

    let gradation_type = Hwp5GradationType::from_raw(cur.read_u8()?);
    let angle = cur.read_u32::<LittleEndian>()?;
    let center_x = cur.read_i32::<LittleEndian>()?;
    let center_y = cur.read_i32::<LittleEndian>()?;
    let blur = cur.read_u32::<LittleEndian>()?;
    let color_count = cur.read_u32::<LittleEndian>()?;
    let color_count = checked_len(cur, u64::from(color_count), 4, "gradation colours")?;
    let mut colors = Vec::with_capacity(color_count);
    for _ in 0..color_count {
        colors.push(cur.read_u32::<LittleEndian>()?);
    }

    let remaining = (cur.get_ref().len() as u64).saturating_sub(cur.position());
    let shape = if remaining >= 4 { Some(cur.read_u32::<LittleEndian>()?) } else { None };
    let remaining = (cur.get_ref().len() as u64).saturating_sub(cur.position());
    let blur_center = if remaining >= 1 { Some(cur.read_u8()?) } else { None };
    let mut extra_data = Vec::new();
    cur.read_to_end(&mut extra_data)?;

    Ok(Hwp5RawGradationFill {
        gradation_type,
        angle,
        center_x,
        center_y,
        blur,
        colors,
        shape,
        blur_center,
        extra_data,
    })
}

/// Synthetic no-fill, short color and two-stop gradient records with distinct line slots.
#[cfg(test)]
pub(crate) fn synthetic_border_fills() -> Vec<Vec<u8>> {
    let mut prefix = 0x2003u16.to_le_bytes().to_vec();
    for slot in 0..5u8 {
        prefix.extend([slot, slot + 2]);
        prefix.extend((0x0012_3456 + u32::from(slot)).to_le_bytes());
    }
    let mut none = prefix.clone();
    none.extend(0u32.to_le_bytes());
    none.extend(0u32.to_le_bytes());
    let mut color = prefix.clone();
    for word in [1u32, 0x0012_ABCD, 0x0056_789A, u32::MAX, 0] {
        color.extend(word.to_le_bytes());
    }
    let mut gradient = prefix;
    gradient.extend(4u32.to_le_bytes());
    gradient.push(1);
    for word in [45u32, (-20i32) as u32, 30, 60, 2, 0x0011_2233, 0x0044_5566, 1] {
        gradient.extend(word.to_le_bytes());
    }
    gradient.push(50);
    vec![none, color, gradient]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_fill_synthetic_wire_values_survive_all_variants() {
        // 이것을 실패시키는 것: 짧은 color의 alpha 강제 읽기 또는 gradient blur_center 누락.
        let records = synthetic_border_fills();
        for data in &records {
            let word = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
            let fill = Hwp5RawBorderFill::parse(data).unwrap();
            assert_eq!(fill.property, 0x2003);
            for (slot, line) in
                [fill.left, fill.right, fill.top, fill.bottom, fill.diagonal].iter().enumerate()
            {
                let at = 2 + slot * 6;
                assert_eq!(line.kind, Hwp5BorderLineKind::from_raw(data[at]));
                assert_eq!(line.width, data[at + 1]);
                assert_eq!(line.color, word(at + 2));
            }
            match fill.fill {
                Hwp5RawBorderFillFill::Color(color) => {
                    assert_eq!(data.len(), 52);
                    assert_eq!(color.background_color, 0x0012_ABCD);
                    assert_eq!(color.pattern_color, 0x0056_789A);
                    assert_eq!(color.pattern_kind, Hwp5FillPatternKind::None);
                    assert_eq!(color.alpha, 0);
                    assert!(color.extra_data.is_empty());
                }
                Hwp5RawBorderFillFill::Gradation(gradient) => {
                    assert_eq!(data.len(), 70);
                    assert_eq!(gradient.colors, [0x0011_2233, 0x0044_5566]);
                    assert_eq!(gradient.angle, 45);
                    assert_eq!(gradient.center_x, -20);
                    assert_eq!(gradient.center_y, 30);
                    assert_eq!(gradient.blur, 60);
                    assert_eq!(gradient.shape, Some(1));
                    assert_eq!(gradient.blur_center, Some(50));
                    assert!(gradient.extra_data.is_empty());
                }
                Hwp5RawBorderFillFill::None => assert_eq!(data.len(), 40),
                other => panic!("unexpected synthetic fill: {other:?}"),
            }
        }
    }

    #[test]
    fn border_fill_color_alpha_and_length_boundaries() {
        // 이것을 실패시키는 것: 짧은 color에도 alpha 강제 읽기, 네 zero 조건 삭제,
        // 현대 alpha 무시, 선언한 extra 길이 검사 제거.
        let legacy = synthetic_border_fills()[1].clone();
        for len in 0..52 {
            assert!(Hwp5RawBorderFill::parse(&legacy[..len]).is_err(), "length {len}");
        }
        for at in 48..52 {
            let mut damaged = legacy.clone();
            damaged[at] = 1;
            assert!(Hwp5RawBorderFill::parse(&damaged).is_err(), "nonzero byte {at}");
        }
        for alpha in [0, 1, 128, 255] {
            let mut modern = legacy[..48].to_vec();
            modern.push(alpha);
            modern.extend_from_slice(&0u32.to_le_bytes());
            let Hwp5RawBorderFillFill::Color(color) =
                Hwp5RawBorderFill::parse(&modern).unwrap().fill
            else {
                panic!("color expected")
            };
            assert_eq!(color.alpha, alpha);
            assert_eq!(color.background_color, 0x0012_ABCD);
            assert!(color.extra_data.is_empty());
            // With alpha=0, a 52-byte prefix is byte-identical to the valid
            // legacy form; length/flags cannot distinguish its provenance.
            assert_eq!(Hwp5RawBorderFill::parse(&modern[..52]).is_ok(), alpha == 0);
            modern[49..53].copy_from_slice(&2u32.to_le_bytes());
            assert!(Hwp5RawBorderFill::parse(&modern).is_err());
            modern.push(0xAB);
            assert!(Hwp5RawBorderFill::parse(&modern).is_err());
            modern.push(0xCD);
            let Hwp5RawBorderFillFill::Color(color) =
                Hwp5RawBorderFill::parse(&modern).unwrap().fill
            else {
                panic!("color expected")
            };
            assert_eq!(color.alpha, alpha);
            assert_eq!(color.extra_data, [0xAB, 0xCD]);
        }
    }

    #[test]
    fn border_fill_gradient_and_image_layout_boundaries() {
        // 이것을 실패시키는 것: color 전용 짧은 형식을 다른 플래그에 적용,
        // 이미지 ID/효과 위치 변경, gradient blur_center 변경 또는 색 개수 길이 검사 제거.
        let gradient = synthetic_border_fills()[2].clone();
        assert_eq!(gradient.len(), 70);
        for len in 36..65 {
            assert!(Hwp5RawBorderFill::parse(&gradient[..len]).is_err(), "gradient {len}");
        }
        for alpha_tail in [vec![], vec![0], vec![128]] {
            let mut data = gradient.clone();
            data.extend_from_slice(&alpha_tail);
            let Hwp5RawBorderFillFill::Gradation(fill) =
                Hwp5RawBorderFill::parse(&data).unwrap().fill
            else {
                panic!("gradation expected")
            };
            assert_eq!(fill.shape, Some(1));
            assert_eq!(fill.blur_center, Some(50));
            assert_eq!(fill.extra_data, alpha_tail);
        }
        let mut image = vec![0; 32];
        image.extend_from_slice(&2u32.to_le_bytes());
        image.extend_from_slice(&[5, 0xF6, 20, 1, 0x34, 0x12]);
        for len in 36..42 {
            assert!(Hwp5RawBorderFill::parse(&image[..len]).is_err(), "image {len}");
        }
        image.extend_from_slice(&[0; 5]);
        let Hwp5RawBorderFillFill::Image(fill) = Hwp5RawBorderFill::parse(&image).unwrap().fill
        else {
            panic!("image expected")
        };
        assert_eq!(fill.mode, Hwp5FillImageMode::Resize);
        assert_eq!(fill.brightness, -10);
        assert_eq!(fill.contrast, 20);
        assert_eq!(fill.effect, Hwp5FillImageEffect::GrayScale);
        assert_eq!(fill.bindata_id, 0x1234);
        assert_eq!(fill.extra_data, [0; 5]);
    }

    /// A fill whose length word claims more than the record holds is
    /// refused before anything is allocated: an empty fill's or a colour
    /// fill's extra-data length, a gradation's colour count — each
    /// `u32::MAX` in a few-byte record.
    ///
    /// 이것을 실패시키는 것: 세 길이 검사 중 하나를 지우기(수 GB 할당 요청 —
    /// 테스트가 메모리 부족으로 죽거나 `read_exact` 전에 멈춘다).
    #[test]
    fn fill_lengths_beyond_the_record_are_refused_before_allocating() {
        let huge = u32::MAX.to_le_bytes();
        let none = huge.to_vec();
        let mut colour = vec![0u8; 13];
        colour.extend_from_slice(&huge);
        let mut gradation = vec![0u8; 17];
        gradation.extend_from_slice(&huge);
        for (kind, data) in [(0u32, none), (1, colour), (4, gradation)] {
            let err = parse_fill_payload(kind, &mut Cursor::new(&data[..])).unwrap_err();
            assert!(err.to_string().contains("do not fit"), "kind {kind}: {err}");
        }
    }

    fn border_line(kind: u8, width: u8, color: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(kind);
        data.push(width);
        data.extend_from_slice(&color.to_le_bytes());
        data
    }

    #[test]
    fn parse_border_fill_color_fill() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x2003u16.to_le_bytes()); // 3D + shadow + centerLine
        data.extend_from_slice(&border_line(1, 1, 0x00000000)); // solid
        data.extend_from_slice(&border_line(1, 10, 0x00000000)); // solid
        data.extend_from_slice(&border_line(1, 13, 0x00000000)); // solid
        data.extend_from_slice(&border_line(0, 0, 0x00000000)); // none
        data.extend_from_slice(&border_line(0, 0, 0x00000000)); // none
        data.extend_from_slice(&0x0000_0001u32.to_le_bytes()); // color fill
        data.extend_from_slice(&0x00A756CAu32.to_le_bytes()); // BGR for #CA56A7
        data.extend_from_slice(&0xC0FF_FFFFu32.to_le_bytes());
        data.extend_from_slice(&(-1i32).to_le_bytes()); // no hatch
        data.push(0); // alpha
        data.extend_from_slice(&0u32.to_le_bytes()); // extra len

        let fill = Hwp5RawBorderFill::parse(&data).unwrap();
        assert_eq!(fill.property, 0x2003);
        assert!(fill.three_d);
        assert!(fill.shadow);
        assert!(fill.center_line);
        assert_eq!(fill.left.kind, Hwp5BorderLineKind::Solid);
        assert_eq!(fill.right.kind, Hwp5BorderLineKind::Solid);
        assert_eq!(fill.top.kind, Hwp5BorderLineKind::Solid);
        assert_eq!(fill.bottom.kind, Hwp5BorderLineKind::None);
        assert_eq!(fill.diagonal.kind, Hwp5BorderLineKind::None);
        assert_eq!(fill.right.width, 10);
        assert_eq!(fill.top.width, 13);
        match fill.fill {
            Hwp5RawBorderFillFill::Color(color) => {
                assert_eq!(color.background_color, 0x00A756CA);
                assert_eq!(color.pattern_color, 0xC0FF_FFFF);
                assert_eq!(color.pattern_kind, Hwp5FillPatternKind::None);
                assert_eq!(color.alpha, 0);
                assert!(color.extra_data.is_empty());
            }
            other => panic!("expected color fill, got {other:?}"),
        }
    }

    #[test]
    fn parse_border_fill_none_fill() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes());
        for _ in 0..5 {
            data.extend_from_slice(&border_line(0, 0, 0x00000000));
        }
        data.extend_from_slice(&0x0000_0000u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        let fill = Hwp5RawBorderFill::parse(&data).unwrap();
        assert!(matches!(fill.fill, Hwp5RawBorderFillFill::None));
    }

    #[test]
    fn parse_border_fill_too_short() {
        assert!(matches!(
            Hwp5RawBorderFill::parse(&[0u8; 35]).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }
}
