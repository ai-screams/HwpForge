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
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::Solid,
            2 => Self::Dash,
            3 => Self::Dot,
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
        let fill = parse_fill_payload(fill_kind, &mut cur)?;

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

fn parse_fill_payload(
    fill_kind: u32,
    cur: &mut Cursor<&[u8]>,
) -> Hwp5Result<Hwp5RawBorderFillFill> {
    match fill_kind {
        0x0000_0000 => {
            let extra_len = cur.read_u32::<LittleEndian>()? as usize;
            let mut ignored_extra_data = vec![0u8; extra_len];
            cur.read_exact(&mut ignored_extra_data)?;
            Ok(Hwp5RawBorderFillFill::None)
        }
        0x0000_0001 => {
            let background_color = cur.read_u32::<LittleEndian>()?;
            let pattern_color = cur.read_u32::<LittleEndian>()?;
            let pattern_kind = Hwp5FillPatternKind::from_raw(cur.read_i32::<LittleEndian>()?);
            let alpha = cur.read_u8()?;
            let extra_len = cur.read_u32::<LittleEndian>()? as usize;
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
    let color_count = cur.read_u32::<LittleEndian>()? as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

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
