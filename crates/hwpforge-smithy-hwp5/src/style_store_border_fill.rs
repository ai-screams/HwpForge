use crate::decoder::header::Hwp5DocInfoBorderFillSlot;
use crate::decoder::Hwp5Warning;
use crate::schema::border_fill::{
    Hwp5BorderLineKind, Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5FillPatternKind,
    Hwp5GradationType, Hwp5RawBorderFill, Hwp5RawBorderFillFill,
};
use crate::warning_utils::push_projection_fallback;
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
        Hwp5RawBorderFillFill::Color(color_fill) => {
            // Warning-first: an unknown hatch pattern silently collapses to a
            // solid fill (the 6 known patterns are mapped). `None` is a
            // legitimate "no pattern" and must NOT warn. See HWP5_WIRE_SPEC §22.
            if let Hwp5FillPatternKind::Unknown(raw) = color_fill.pattern_kind {
                push_projection_fallback(
                    warnings,
                    "style.border_fill.fill_pattern",
                    format!(
                        "border_fill_id={border_fill_id}, unknown hatch pattern raw={raw}; \
                         emitting solid fill (no hatch)"
                    ),
                );
            }
            BorderFillFillProjection {
                fill: Some(HwpxFill::WinBrush {
                    face_color: colorref_to_hwpx_color(color_fill.background_color),
                    hatch_color: colorref_to_hwpx_color(color_fill.pattern_color),
                    alpha: color_fill.alpha.to_string(),
                }),
                fill_hatch_style: hwp5_fill_pattern_to_hwpx(color_fill.pattern_kind),
                ..BorderFillFillProjection::default()
            }
        }
        Hwp5RawBorderFillFill::Gradation(fill) => {
            // Warning-first: an unknown gradation type silently defaults to
            // LINEAR. See HWP5_WIRE_SPEC §22.
            if let Hwp5GradationType::Unknown(raw) = fill.gradation_type {
                push_projection_fallback(
                    warnings,
                    "style.border_fill.gradation_type",
                    format!(
                        "border_fill_id={border_fill_id}, unknown gradation type raw={raw}; \
                         defaulting to LINEAR"
                    ),
                );
            }
            BorderFillFillProjection {
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
            }
        }
        Hwp5RawBorderFillFill::Image(fill) => {
            let Some(mode) = hwp5_image_fill_mode_to_hwpx(fill.mode) else {
                push_projection_fallback(
                    warnings,
                    "style.border_fill.image_fill_mode",
                    format!(
                        "border_fill_id={border_fill_id}, raw_mode={:?}, bindata_id={}",
                        fill.mode, fill.bindata_id
                    ),
                );
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
    // HWP5 raw image-fill modes 0-15 map 1:1 (same ordinal order) to the
    // KS X 6101 OWPML `ImageBrushMode` enum. Previously only TILE/TOTAL/
    // CENTER/ZOOM (0/5/6/15) were mapped and the other 12 modes silently
    // collapsed to a transparent fill. Confirmed against native fixture
    // `sample-cell-image-fill` (TILE_HORZ_TOP/CENTER_TOP/LEFT_TOP/RIGHT_BOTTOM
    // = raw 1/7/10/14) spanning every dropped family. (`Resize` → `TOTAL`,
    // `*Middle` → `*_CENTER` per the OWPML naming.)
    match kind {
        Hwp5FillImageMode::TileAll => Some("TILE"),
        Hwp5FillImageMode::TileHorizontalTop => Some("TILE_HORZ_TOP"),
        Hwp5FillImageMode::TileHorizontalBottom => Some("TILE_HORZ_BOTTOM"),
        Hwp5FillImageMode::TileVerticalLeft => Some("TILE_VERT_LEFT"),
        Hwp5FillImageMode::TileVerticalRight => Some("TILE_VERT_RIGHT"),
        Hwp5FillImageMode::Resize => Some("TOTAL"),
        Hwp5FillImageMode::Center => Some("CENTER"),
        Hwp5FillImageMode::CenterTop => Some("CENTER_TOP"),
        Hwp5FillImageMode::CenterBottom => Some("CENTER_BOTTOM"),
        Hwp5FillImageMode::LeftMiddle => Some("LEFT_CENTER"),
        Hwp5FillImageMode::LeftTop => Some("LEFT_TOP"),
        Hwp5FillImageMode::LeftBottom => Some("LEFT_BOTTOM"),
        Hwp5FillImageMode::RightMiddle => Some("RIGHT_CENTER"),
        Hwp5FillImageMode::RightTop => Some("RIGHT_TOP"),
        Hwp5FillImageMode::RightBottom => Some("RIGHT_BOTTOM"),
        Hwp5FillImageMode::Zoom => Some("ZOOM"),
        Hwp5FillImageMode::Unknown(_) => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::border_fill::Hwp5FillImageMode::{
        self, Center, CenterBottom, CenterTop, LeftBottom, LeftMiddle, LeftTop, Resize,
        RightBottom, RightMiddle, RightTop, TileAll, TileHorizontalBottom, TileHorizontalTop,
        TileVerticalLeft, TileVerticalRight, Zoom,
    };

    #[test]
    fn image_fill_mode_maps_all_16_ks_x_6101_modes() {
        // HWP5 raw 0-15 ↔ OWPML ImageBrushMode, same ordinal order. Verified
        // against native fixture sample-cell-image-fill for raw 1/7/10/14
        // (TILE_HORZ_TOP/CENTER_TOP/LEFT_TOP/RIGHT_BOTTOM) plus the
        // pre-existing 0/5/6/15. Before the P1-2 fix only 4 of 16 mapped;
        // the rest collapsed to a transparent fill.
        let cases: [(Hwp5FillImageMode, &str); 16] = [
            (TileAll, "TILE"),
            (TileHorizontalTop, "TILE_HORZ_TOP"),
            (TileHorizontalBottom, "TILE_HORZ_BOTTOM"),
            (TileVerticalLeft, "TILE_VERT_LEFT"),
            (TileVerticalRight, "TILE_VERT_RIGHT"),
            (Resize, "TOTAL"),
            (Center, "CENTER"),
            (CenterTop, "CENTER_TOP"),
            (CenterBottom, "CENTER_BOTTOM"),
            (LeftMiddle, "LEFT_CENTER"),
            (LeftTop, "LEFT_TOP"),
            (LeftBottom, "LEFT_BOTTOM"),
            (RightMiddle, "RIGHT_CENTER"),
            (RightTop, "RIGHT_TOP"),
            (RightBottom, "RIGHT_BOTTOM"),
            (Zoom, "ZOOM"),
        ];
        for (mode, expected) in cases {
            assert_eq!(hwp5_image_fill_mode_to_hwpx(mode), Some(expected), "mode {mode:?}");
        }
        // Only a genuinely unknown raw value falls back to None now.
        assert_eq!(hwp5_image_fill_mode_to_hwpx(Hwp5FillImageMode::Unknown(99)), None);
    }

    use crate::schema::border_fill::{Hwp5RawColorFill, Hwp5RawGradationFill};

    fn warnings_for(fill: &Hwp5RawBorderFillFill) -> Vec<Hwp5Warning> {
        let mut warnings = Vec::new();
        let _ = hwp5_fill_to_hwpx(7, fill, &mut warnings);
        warnings
    }

    fn has_fallback(warnings: &[Hwp5Warning], subject: &str) -> bool {
        warnings.iter().any(
            |w| matches!(w, Hwp5Warning::ProjectionFallback { subject: s, .. } if *s == subject),
        )
    }

    fn color_fill(pattern_kind: Hwp5FillPatternKind) -> Hwp5RawBorderFillFill {
        Hwp5RawBorderFillFill::Color(Hwp5RawColorFill {
            background_color: 0,
            pattern_color: 0,
            pattern_kind,
            alpha: 0,
            extra_data: Vec::new(),
        })
    }

    fn gradation_fill(gradation_type: Hwp5GradationType) -> Hwp5RawBorderFillFill {
        Hwp5RawBorderFillFill::Gradation(Hwp5RawGradationFill {
            gradation_type,
            angle: 0,
            center_x: 50,
            center_y: 50,
            blur: 0,
            colors: vec![0, 0xFF_FFFF],
            shape: None,
            blur_center: None,
            extra_data: Vec::new(),
        })
    }

    #[test]
    fn unknown_hatch_pattern_warns_but_known_and_none_stay_silent() {
        // P1-3/4 warning-first: only an unknown hatch pattern warns.
        assert!(has_fallback(
            &warnings_for(&color_fill(Hwp5FillPatternKind::Unknown(99))),
            "style.border_fill.fill_pattern"
        ));
        // `None` (no hatch) and a known pattern must NOT warn.
        assert!(!has_fallback(
            &warnings_for(&color_fill(Hwp5FillPatternKind::None)),
            "style.border_fill.fill_pattern"
        ));
        assert!(!has_fallback(
            &warnings_for(&color_fill(Hwp5FillPatternKind::Slash)),
            "style.border_fill.fill_pattern"
        ));
    }

    #[test]
    fn unknown_gradation_type_warns_but_known_stays_silent() {
        assert!(has_fallback(
            &warnings_for(&gradation_fill(Hwp5GradationType::Unknown(99))),
            "style.border_fill.gradation_type"
        ));
        assert!(!has_fallback(
            &warnings_for(&gradation_fill(Hwp5GradationType::Linear)),
            "style.border_fill.gradation_type"
        ));
    }
}
