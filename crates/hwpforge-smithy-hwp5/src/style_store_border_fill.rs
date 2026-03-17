use crate::decoder::header::Hwp5DocInfoBorderFillSlot;
use crate::decoder::Hwp5Warning;
use crate::schema::border_fill::{
    Hwp5BorderLineKind, Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5FillPatternKind,
    Hwp5GradationType, Hwp5RawBorderFill, Hwp5RawBorderFillFill,
};
use hwpforge_foundation::{Color, GradientType, PatternType};
use hwpforge_smithy_hwpx::{
    style_store::{
        HwpxBorderFill, HwpxBorderLine, HwpxDiagonalLine, HwpxFill, HwpxGradientFill, HwpxImageFill,
    },
    HwpxStyleStore,
};
use std::collections::BTreeSet;

pub(crate) fn push_required_border_fills(store: &mut HwpxStyleStore) {
    store.push_border_fill(HwpxBorderFill::default_page_border()); // id=1
    store.push_border_fill(HwpxBorderFill::default_char_background()); // id=2
    store.push_border_fill(HwpxBorderFill::default_table_border()); // id=3
}

pub(crate) fn push_hwp5_border_fills(
    store: &mut HwpxStyleStore,
    border_fills: &[Hwp5DocInfoBorderFillSlot],
    warnings: &mut Vec<Hwp5Warning>,
) {
    for slot in border_fills {
        let border_fill = match &slot.fill {
            Some(fill) => hwp5_border_fill_to_hwpx(slot.id, fill, warnings),
            None => unresolved_hwp5_border_fill_placeholder(slot.id),
        };
        store.push_border_fill(border_fill);
    }
}

pub(crate) fn collect_hwp5_border_fill_image_binary_ids(
    border_fills: &[Hwp5DocInfoBorderFillSlot],
) -> BTreeSet<u16> {
    border_fills
        .iter()
        .filter_map(|slot| match slot.fill.as_ref()?.fill {
            Hwp5RawBorderFillFill::Image(ref fill) => Some(fill.bindata_id),
            _ => None,
        })
        .collect()
}

fn hwp5_border_fill_to_hwpx(
    id: u32,
    fill: &Hwp5RawBorderFill,
    warnings: &mut Vec<Hwp5Warning>,
) -> HwpxBorderFill {
    let fill_projection = hwp5_fill_to_hwpx(id, &fill.fill, warnings);
    let mut border_fill = HwpxBorderFill::new(
        id,
        fill.three_d,
        fill.shadow,
        if fill.center_line { "SOLID" } else { "NONE" },
        hwp5_border_line_to_hwpx(&fill.left),
        hwp5_border_line_to_hwpx(&fill.right),
        hwp5_border_line_to_hwpx(&fill.top),
        hwp5_border_line_to_hwpx(&fill.bottom),
        Some(hwp5_border_line_to_hwpx(&fill.diagonal)),
        HwpxDiagonalLine {
            border_type: hwp5_diagonal_shape_to_hwpx(fill.slash_diagonal_shape).into(),
            crooked: false,
            is_counter: false,
        },
        HwpxDiagonalLine {
            border_type: hwp5_diagonal_shape_to_hwpx(fill.back_slash_diagonal_shape).into(),
            crooked: false,
            is_counter: false,
        },
        None,
    );
    apply_fill_projection(&mut border_fill, fill_projection);
    border_fill
}

fn hwp5_border_line_to_hwpx(
    line: &crate::schema::border_fill::Hwp5RawBorderLine,
) -> HwpxBorderLine {
    HwpxBorderLine {
        line_type: hwp5_border_line_type_to_hwpx(line.kind).into(),
        width: hwp5_border_width_to_hwpx(line.width).into(),
        color: colorref_to_hwpx_color(line.color),
    }
}

fn hwp5_border_line_type_to_hwpx(kind: Hwp5BorderLineKind) -> &'static str {
    match kind {
        Hwp5BorderLineKind::None => "NONE",
        Hwp5BorderLineKind::Solid => "SOLID",
        Hwp5BorderLineKind::Dash => "DASH",
        Hwp5BorderLineKind::Dot => "DOT",
        Hwp5BorderLineKind::DashDot => "DASH_DOT",
        Hwp5BorderLineKind::DashDotDot => "DASH_DOT_DOT",
        Hwp5BorderLineKind::LongDash => "LONG_DASH",
        Hwp5BorderLineKind::Circle => "CIRCLE",
        Hwp5BorderLineKind::DoubleSlim => "DOUBLE_SLIM",
        Hwp5BorderLineKind::SlimThick => "SLIM_THICK",
        Hwp5BorderLineKind::ThickSlim => "THICK_SLIM",
        Hwp5BorderLineKind::SlimThickSlim => "SLIM_THICK_SLIM",
        Hwp5BorderLineKind::Wave => "WAVE",
        Hwp5BorderLineKind::DoubleWave => "DOUBLE_WAVE",
        Hwp5BorderLineKind::Thick3d => "THICK_3D",
        Hwp5BorderLineKind::Thick3dReverseLighting => "THICK_3D_REVERSE_LIGHTING",
        Hwp5BorderLineKind::Solid3d => "SOLID_3D",
        Hwp5BorderLineKind::Solid3dReverseLighting => "SOLID_3D_REVERSE_LIGHTING",
        Hwp5BorderLineKind::Unknown(_) => "NONE",
    }
}

fn hwp5_border_width_to_hwpx(width: u8) -> &'static str {
    match width {
        0 => "0.1 mm",
        1 => "0.12 mm",
        2 => "0.15 mm",
        3 => "0.2 mm",
        4 => "0.25 mm",
        5 => "0.3 mm",
        6 => "0.4 mm",
        7 => "0.5 mm",
        8 => "0.6 mm",
        9 => "0.7 mm",
        10 => "1.0 mm",
        11 => "1.5 mm",
        12 => "2.0 mm",
        13 => "3.0 mm",
        14 => "4.0 mm",
        15 => "5.0 mm",
        _ => "0.1 mm",
    }
}

fn hwp5_diagonal_shape_to_hwpx(shape: u8) -> &'static str {
    match shape {
        0 => "NONE",
        2 => "CENTER",
        3 => "CENTER_BELOW",
        6 => "CENTER_ABOVE",
        7 => "ALL",
        _ => "NONE",
    }
}

fn hwp5_fill_to_hwpx(
    border_fill_id: u32,
    fill: &Hwp5RawBorderFillFill,
    warnings: &mut Vec<Hwp5Warning>,
) -> BorderFillFillProjection {
    match fill {
        Hwp5RawBorderFillFill::Color(color_fill) => BorderFillFillProjection {
            fill: Some(HwpxFill::WinBrush {
                face_color: colorref_to_hwpx_color(color_fill.background_color),
                hatch_color: colorref_to_hwpx_color(color_fill.pattern_color),
                alpha: color_fill.alpha.to_string(),
            }),
            fill_hatch_style: hwp5_fill_pattern_to_hwpx(color_fill.pattern_kind),
            ..BorderFillFillProjection::default()
        },
        Hwp5RawBorderFillFill::Gradation(fill) => BorderFillFillProjection {
            gradient_fill: Some(HwpxGradientFill {
                gradient_type: hwp5_gradation_type_to_hwpx(fill.gradation_type),
                angle: fill.angle as i32,
                center_x: fill.center_x,
                center_y: fill.center_y,
                step: 255,
                step_center: fill.blur_center.map(i32::from).unwrap_or(50),
                alpha: 0,
                colors: fill.colors.iter().copied().map(Color::from_raw).collect(),
            }),
            ..BorderFillFillProjection::default()
        },
        Hwp5RawBorderFillFill::Image(fill) => {
            let Some(mode) = hwp5_image_fill_mode_to_hwpx(fill.mode) else {
                warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "style.border_fill.image_fill_mode",
                    reason: format!(
                        "border_fill_id={border_fill_id}, raw_mode={:?}, bindata_id={}",
                        fill.mode, fill.bindata_id
                    ),
                });
                return BorderFillFillProjection::default();
            };
            BorderFillFillProjection {
                image_fill: Some(HwpxImageFill {
                    mode: mode.to_string(),
                    binary_item_id_ref: format!("BIN{:04X}", fill.bindata_id),
                    bright: i32::from(fill.brightness),
                    contrast: i32::from(fill.contrast),
                    effect: hwp5_image_fill_effect_to_hwpx(fill.effect).to_string(),
                    alpha: 0,
                }),
                ..BorderFillFillProjection::default()
            }
        }
        Hwp5RawBorderFillFill::None | Hwp5RawBorderFillFill::Unknown { .. } => {
            BorderFillFillProjection::default()
        }
    }
}

#[derive(Default)]
struct BorderFillFillProjection {
    fill: Option<HwpxFill>,
    fill_hatch_style: Option<String>,
    gradient_fill: Option<HwpxGradientFill>,
    image_fill: Option<HwpxImageFill>,
}

fn apply_fill_projection(border_fill: &mut HwpxBorderFill, projection: BorderFillFillProjection) {
    match projection {
        BorderFillFillProjection {
            fill: Some(HwpxFill::WinBrush { face_color, hatch_color, alpha }),
            fill_hatch_style,
            ..
        } => border_fill.set_win_brush_fill(face_color, hatch_color, alpha, fill_hatch_style),
        BorderFillFillProjection { gradient_fill: Some(fill), .. } => {
            border_fill.set_gradient_fill(fill)
        }
        BorderFillFillProjection { image_fill: Some(fill), .. } => border_fill.set_image_fill(fill),
        BorderFillFillProjection { .. } => border_fill.clear_fill_brush(),
    }
}

fn unresolved_hwp5_border_fill_placeholder(id: u32) -> HwpxBorderFill {
    HwpxBorderFill::new(
        id,
        false,
        false,
        "NONE",
        HwpxBorderLine::default(),
        HwpxBorderLine::default(),
        HwpxBorderLine::default(),
        HwpxBorderLine::default(),
        None,
        HwpxDiagonalLine::default(),
        HwpxDiagonalLine::default(),
        None,
    )
}

fn hwp5_fill_pattern_to_hwpx(kind: Hwp5FillPatternKind) -> Option<String> {
    match kind {
        Hwp5FillPatternKind::None => None,
        Hwp5FillPatternKind::Horizontal => Some(PatternType::Horizontal.to_string()),
        Hwp5FillPatternKind::Vertical => Some(PatternType::Vertical.to_string()),
        Hwp5FillPatternKind::BackSlash => Some(PatternType::BackSlash.to_string()),
        Hwp5FillPatternKind::Slash => Some(PatternType::Slash.to_string()),
        Hwp5FillPatternKind::Cross => Some(PatternType::Cross.to_string()),
        Hwp5FillPatternKind::CrossDiagonal => Some(PatternType::CrossDiagonal.to_string()),
        Hwp5FillPatternKind::Unknown(_) => None,
    }
}

fn hwp5_gradation_type_to_hwpx(kind: Hwp5GradationType) -> GradientType {
    match kind {
        Hwp5GradationType::Linear => GradientType::Linear,
        Hwp5GradationType::Circular => GradientType::Radial,
        Hwp5GradationType::Conical => GradientType::Conical,
        Hwp5GradationType::Rectangular => GradientType::Square,
        Hwp5GradationType::Unknown(_) => GradientType::Linear,
    }
}

fn hwp5_image_fill_mode_to_hwpx(kind: Hwp5FillImageMode) -> Option<&'static str> {
    match kind {
        Hwp5FillImageMode::TileAll => Some("TILE"),
        // HWP5 "Resize/FitToSize" companion fixtures serialize to HWPX TOTAL.
        Hwp5FillImageMode::Resize => Some("TOTAL"),
        Hwp5FillImageMode::Center => Some("CENTER"),
        Hwp5FillImageMode::Zoom => Some("ZOOM"),
        Hwp5FillImageMode::TileHorizontalTop
        | Hwp5FillImageMode::TileHorizontalBottom
        | Hwp5FillImageMode::TileVerticalLeft
        | Hwp5FillImageMode::TileVerticalRight
        | Hwp5FillImageMode::CenterTop
        | Hwp5FillImageMode::CenterBottom
        | Hwp5FillImageMode::LeftMiddle
        | Hwp5FillImageMode::LeftTop
        | Hwp5FillImageMode::LeftBottom
        | Hwp5FillImageMode::RightMiddle
        | Hwp5FillImageMode::RightTop
        | Hwp5FillImageMode::RightBottom
        | Hwp5FillImageMode::Unknown(_) => None,
    }
}

fn hwp5_image_fill_effect_to_hwpx(kind: Hwp5FillImageEffect) -> &'static str {
    match kind {
        Hwp5FillImageEffect::RealPic => "REAL_PIC",
        Hwp5FillImageEffect::GrayScale => "GRAY_SCALE",
        Hwp5FillImageEffect::BlackWhite => "BLACK_WHITE",
        Hwp5FillImageEffect::Pattern8x8 => "PATTERN8x8",
        Hwp5FillImageEffect::Unknown(_) => "REAL_PIC",
    }
}

fn colorref_to_hwpx_color(raw: u32) -> String {
    if (raw >> 24) != 0 {
        format!("#{raw:08X}")
    } else {
        Color::from_raw(raw).to_hex_rgb()
    }
}
