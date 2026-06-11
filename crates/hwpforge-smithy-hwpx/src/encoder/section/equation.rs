//! Equation (`<hp:equation>`) run builder (task #92 split from
//! `encoder/section.rs`; see HWP5_WIRE_SPEC.md §11 for the eqed
//! pairing on the HWP5 side).

use super::*;

/// Encodes a Core `Control::Equation` into `HxEquation`.
///
/// Equations have NO shape common block (no offset, orgSz, curSz, flip,
/// rotation, lineShape, fillBrush, shadow). Only sz + pos + outMargin + script.
/// Does not take `depth` because equations have no recursive sub-content.
pub(super) fn encode_equation_to_hx(ctrl: &Control) -> HwpxResult<HxEquation> {
    let (script, width, height, base_line, text_color, font, inst_id) = match ctrl {
        Control::Equation { script, width, height, base_line, text_color, font, inst_id } => {
            (script, *width, *height, *base_line, text_color, font, *inst_id)
        }
        _ => unreachable!("encode_equation_to_hx called with non-Equation"),
    };

    let w = width.as_i32();
    let h = height.as_i32();

    Ok(HxEquation {
        // Wave 12p Step 4: cross-ref target id 가 있으면 사용,
        // 없으면 fresh fallback (한컴 native 와는 id 차이만 발생).
        id: inst_id.map(|n| n.to_string()).unwrap_or_else(generate_instid),
        z_order: 0,
        numbering_type: "EQUATION".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),

        // Equation-specific attrs (hardcoded constants per ground truth)
        version: "Equation Version 60".to_string(),
        base_line,
        text_color: text_color.to_hex_rgb(),
        base_unit: 1000,
        line_mode: "CHAR".to_string(),
        font: font.clone(),

        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(HxTablePos {
            treat_as_char: 1,
            affect_l_spacing: 0,
            flow_with_text: 1, // equations always flowWithText=1
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        }),
        out_margin: Some(HxTableMargin { left: 56, right: 56, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "수식입니다.".to_string() }),
        script: Some(HxScript { text: script.clone() }),
    })
}
