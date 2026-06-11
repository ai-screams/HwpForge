//! Typography control builders — dutmal (덧말) and compose
//! (글자겹침) (task #92 split from `encoder/section.rs`).

use super::*;

/// Encodes a Core `Control::Dutmal` into `HxDutmal`.
pub(super) fn encode_dutmal_to_hx(
    main_text: &str,
    sub_text: &str,
    position: DutmalPosition,
    sz_ratio: u32,
    align: DutmalAlign,
    option: u32,
) -> HxDutmal {
    let pos_type = match position {
        DutmalPosition::Top => "TOP",
        DutmalPosition::Bottom => "BOTTOM",
        DutmalPosition::Right => "RIGHT",
        DutmalPosition::Left => "LEFT",
        _ => "TOP",
    };
    let align_str = match align {
        DutmalAlign::Center => "CENTER",
        DutmalAlign::Left => "LEFT",
        DutmalAlign::Right => "RIGHT",
        _ => "CENTER",
    };
    HxDutmal {
        pos_type: pos_type.to_string(),
        sz_ratio,
        option,
        style_id_ref: 0,
        align: align_str.to_string(),
        main_text: main_text.to_string(),
        sub_text: sub_text.to_string(),
    }
}

/// Encodes a Core `Control::Compose` into `HxCompose`.
///
/// Always emits 10 `<hp:charPr>` entries — KS X 6101 fixes
/// `charPrCnt` at 10. `char_pr_ids` from the Core variant is
/// padded with `u32::MAX` ("no override" sentinel) if shorter than
/// 10 and truncated if longer; the resulting slice maps 1:1 onto
/// the `<hp:charPr prIDRef="…"/>` children.
pub(super) fn encode_compose_to_hx(
    compose_text: &str,
    circle_type: &str,
    char_sz: i32,
    compose_type: &str,
    char_pr_ids: &[u32],
) -> HxCompose {
    let char_prs = (0..10)
        .map(|i| HxComposeCharPr { pr_id_ref: char_pr_ids.get(i).copied().unwrap_or(u32::MAX) })
        .collect();
    HxCompose {
        circle_type: circle_type.to_string(),
        char_sz,
        compose_type: compose_type.to_string(),
        char_pr_cnt: 10,
        compose_text: compose_text.to_string(),
        char_prs,
    }
}
